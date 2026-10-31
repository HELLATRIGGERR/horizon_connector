//! Source-backed Memory Card settings and physical ownership helpers for the
//! currently supported AE2 19.2.18 machines; broader component compatibility
//! and unimplemented machines remain separate work.
use horizon_client::{ae2_world::{Machine,MachineKind},items::ItemStack,save};
use horizon_world::nbt::{Compound,Tag};
const KEY:&str="ae2_memory_settings";
/// Compare a source item description by component content and style.
/// ComponentSerialization permits a one-element list, default-empty argument
/// and sibling lists, and ignores unknown record keys. Explicit style or
/// fallback fields still change Component equality.
pub(super) fn source_description_matches(value:Option<&Tag>,translation:&str)->bool {
    let Some(value)=value else{return false};
    if let Tag::List(items)=value {return items.len()==1&&source_description_matches(items.first(),translation)}
    let Tag::Compound(map)=value else{return false};
    if map.get(&"translate".into())!=Some(&Tag::String(translation.into())){return false}
    match map.get(&"type".into()) {
        None if map.get(&"text".into()).is_some()=>return false,
        None=>{},Some(Tag::String(kind)) if kind.to_string_lossy()=="translatable"=>{},_=>return false,
    }
    for key in ["fallback","color","shadow_color","shadowColor","bold","italic","underlined","strikethrough","obfuscated","click_event","hover_event","clickEvent","hoverEvent","insertion","font"] {
        if map.contains_key(&key.into()){return false}
    }
    for key in ["with","extra"] {
        if let Some(value)=map.get(&key.into()) {if !matches!(value,Tag::List(items) if items.is_empty()){return false}}
    }
    true
}

const P2P:&str="ae2_p2p_card";
// Source data/ae2/tags/data_component_type/exported_settings.json. Clearing
// removes only these registered exports and the separately stored LED colors.
const EXPORTED_COMPONENTS:[&str;11]=[
    "ae2:exported_config_inv","ae2:exported_patterns","ae2:exported_custom_name",
    "ae2:exported_level_emitter_value","ae2:exported_p2p_frequency","ae2:exported_p2p_type",
    "ae2:exported_priority","ae2:exported_push_direction","ae2:exported_settings",
    "ae2:exported_upgrades","ae2:exported_settings_source",
];
const FIELDS:[(&str,&str,&[&str]);3]=[
    ("auto_export","NO",&["NO","YES"]),
    ("inscriber_separate_sides","NO",&["NO","YES"]),
    ("inscriber_input_capacity","SIXTY_FOUR",&["ONE","FOUR","SIXTY_FOUR"]),
];
fn setting_value<'a>(m:&'a Machine,key:&str,default:&'static str,values:&[&str])->Result<String,String>{
    let value=match m.config.get(&key.into()) {None=>default.to_owned(),Some(Tag::String(s))=>s.to_string().map_err(|_|"invalid setting text")?,_=>return Err("invalid setting type".into())};
    if !values.contains(&value.as_str()){return Err("invalid setting value".into())}Ok(value)
}
/// Registered component representation for the four simple block hosts. The
/// legacy envelope remains available to older native saves. Source groups
/// equal upgrade stacks by item and components, rather than by physical slot.
pub(super) fn publish_simple_source(card:&mut ItemStack)->Result<(),String> {
    let saved=card.data().and_then(|d|d.other("minecraft:custom_data")).and_then(|v|v.get(KEY)).cloned().ok_or("missing card settings")?;
    let Some(Tag::String(source))=saved.get("source") else{return Err("missing card source".into())};
    let source=source.to_string().map_err(|_|"invalid card source")?;
    if !["ae2:inscriber","ae2:io_port","ae2:vibration_chamber","ae2:molecular_assembler"].contains(&source.as_str()) {return Ok(());}
    let Some(Tag::List(upgrades))=saved.get("upgrades") else{return Err("missing card upgrades".into())};
    let mut groups:Vec<ItemStack>=Vec::new();
    for entry in upgrades {
        let stack=save::item_from_nbt(entry).filter(|v|v.count>0).ok_or("invalid card upgrade")?;
        if let Some(group)=groups.iter_mut().find(|value|value.same_item_same_components(&stack)) {
            group.count=group.count.checked_add(stack.count).ok_or("too many card upgrades")?;
        } else {groups.push(stack);}
    }
    let settings=saved.get("settings").ok_or("missing card settings")?;
    let source_settings=match source.as_str() {
        "ae2:inscriber"=>Some(settings.clone()),
        "ae2:io_port"=>{
            let get=|key:&str|->Result<String,String>{match settings.get(key){Some(Tag::String(v))=>v.to_string().map_err(|_|"invalid IO setting".into()),_=>Err("missing IO setting".into())}};
            let redstone=match get("io_redstone")?.as_str(){"ignore"=>"IGNORE","high"=>"HIGH_SIGNAL","low"=>"LOW_SIGNAL",_=>return Err("invalid IO redstone".into())};
            Some(Tag::compound([("operation_mode",Tag::String(get("io_operation")?.to_ascii_uppercase().as_str().into())),("fullness_mode",Tag::String(get("io_fullness")?.to_ascii_uppercase().as_str().into())),("redstone_controlled",Tag::String(redstone.into()))]))
        },
        _=>None,
    };
    let translation=format!("block.ae2.{}",source.strip_prefix("ae2:").unwrap_or(&source));
    card.data_mut().set_other("ae2:exported_settings_source",Some(Tag::compound([("translate",Tag::String(translation.as_str().into()))])));
    card.data_mut().set_other("ae2:exported_upgrades",Some(Tag::List(groups.iter().map(save::item_to_nbt).collect())));
    card.data_mut().set_other("ae2:exported_settings",source_settings);
    card.data_mut().set_other("ae2:exported_custom_name",saved.get("name").cloned());
    Ok(())
}
/// Convert complete source exports for these hosts into their existing typed
/// transaction input. Partial or malformed source exports fail closed rather
/// than manufacturing defaults or spending the wrong physical upgrades.
pub(super) fn simple_source_card(card:&ItemStack)->Result<Option<ItemStack>,String> {
    let Some(data)=card.data() else{return Ok(None)};
    let Some(source)=data.other("ae2:exported_settings_source") else{return Ok(None)};
    let Some((_,source))=[
        ("block.ae2.inscriber","ae2:inscriber"),("block.ae2.io_port","ae2:io_port"),
        ("block.ae2.vibration_chamber","ae2:vibration_chamber"),("block.ae2.molecular_assembler","ae2:molecular_assembler"),
    ].into_iter().find(|(description,_)|source_description_matches(Some(source),description)) else{return Ok(None)};
    let max=match source {"ae2:inscriber"=>4,"ae2:io_port"|"ae2:vibration_chamber"=>3,_=>5};
    let Some(Tag::List(upgrades))=data.other("ae2:exported_upgrades") else{return Err("source card lacks upgrades".into())};
    let mut expanded=Vec::new();
    for entry in upgrades {
        let stack=save::item_from_nbt(entry).filter(|v|v.count>0).ok_or("invalid source card upgrade")?;
        if (stack.count as usize)>max-expanded.len() {return Err("too many source card upgrades".into());}
        let mut one=stack.clone();one.count=1;
        expanded.extend((0..stack.count).map(|_|save::item_to_nbt(&one)));
    }
    let settings=match source {
        "ae2:inscriber"=>data.other("ae2:exported_settings").cloned().ok_or("source card lacks settings")?,
        "ae2:io_port"=>{
            let settings=data.other("ae2:exported_settings").ok_or("source card lacks settings")?;
            let get=|key:&str|->Result<String,String>{match settings.get(key){Some(Tag::String(v))=>v.to_string().map_err(|_|"invalid IO setting".into()),_=>Err("missing IO setting".into())}};
            let redstone=match get("redstone_controlled")?.as_str(){"IGNORE"=>"ignore","HIGH_SIGNAL"=>"high","LOW_SIGNAL"=>"low",_=>return Err("unsupported IO redstone".into())};
            Tag::compound([("io_operation",Tag::String(get("operation_mode")?.to_ascii_lowercase().as_str().into())),("io_fullness",Tag::String(get("fullness_mode")?.to_ascii_lowercase().as_str().into())),("io_redstone",Tag::String(redstone.into()))])
        },
        _=>Tag::Compound(Compound::new()),
    };
    let mut body=Compound::new();body.insert("source".into(),Tag::String(source.into()));body.insert("settings".into(),settings);body.insert("upgrades".into(),Tag::List(expanded));
    if let Some(name)=data.other("ae2:exported_custom_name") {
        let name=match name {Tag::String(v)=>v.clone(),Tag::Compound(_) => match name.get("text"){Some(Tag::String(v))=>v.clone(),_=>return Err("unsupported structured machine name".into())},_=>return Err("invalid machine name".into())};
        body.insert("name".into(),Tag::String(name));
    }
    let mut custom=match data.other("minecraft:custom_data"){None=>Compound::new(),Some(Tag::Compound(v))=>v.clone(),_=>return Err("invalid card custom data".into())};
    custom.insert(KEY.into(),Tag::Compound(body));
    let mut converted=card.clone();converted.data_mut().set_other("minecraft:custom_data",Some(Tag::Compound(custom)));
    Ok(Some(converted))
}
/// Saving a physical card snapshots only whitelisted configuration and card
/// descriptions; it does not move any installed cards or machine inventory.
pub(super) fn save_inscriber(card:&mut ItemStack,m:&Machine,name:Option<&str>)->Result<(),String>{
    if card.item.as_ref()!="ae2:memory_card"||card.count!=1||m.kind!=MachineKind::Inscriber{return Err("invalid card or target".into())}
    if m.upgrades.len()!=4{return Err("invalid upgrade slots".into())}
    let mut settings=Compound::new();for (key,default,values) in FIELDS {settings.insert(key.into(),Tag::String(setting_value(m,key,default,values)?.as_str().into()));}
    let mut upgrades=Vec::new();let mut first:Option<&ItemStack>=None;for stack in m.upgrades.iter().flatten(){if stack.item.as_ref()!="ae2:speed_card"||stack.count!=1{return Err("invalid installed card".into())}if first.is_some_and(|a|!a.same_item_same_components(stack)){return Err("mixed upgrade components require source grouping".into())}first=Some(stack);upgrades.push(save::item_to_nbt(stack));}
    let mut data=match card.data().and_then(|d|d.other("minecraft:custom_data")){None=>Compound::new(),Some(Tag::Compound(c))=>c.clone(),_=>return Err("invalid card custom data".into())};
    let mut saved=Compound::new();saved.insert("source".into(),Tag::String("ae2:inscriber".into()));saved.insert("settings".into(),Tag::Compound(settings));saved.insert("upgrades".into(),Tag::List(upgrades));if let Some(name)=name {saved.insert("name".into(),Tag::String(name.into()));}
    data.remove(&P2P.into());data.insert(KEY.into(),Tag::Compound(saved));
    card.data_mut().set_other("minecraft:custom_data",Some(Tag::Compound(data)));Ok(())
}
#[derive(Clone,Debug)] pub(super) struct SavedInscriber {pub settings:Compound,pub desired:Vec<ItemStack>,pub name:Option<String>}
pub(super) fn read_inscriber(card:&ItemStack)->Result<Option<SavedInscriber>,String>{
    if card.item.as_ref()!="ae2:memory_card"||card.count!=1{return Err("invalid card".into())}
    let Some(data)=card.data().and_then(|d|d.other("minecraft:custom_data")) else{return Ok(None)};
    let Some(saved)=data.get(KEY) else{return Ok(None)};
    if saved.get("source")!=Some(&Tag::String("ae2:inscriber".into())){return Ok(None)}
    let Some(Tag::Compound(settings))=saved.get("settings") else{return Err("invalid settings".into())};
    let mut clean=Compound::new();for (key,_,values) in FIELDS {let Some(Tag::String(s))=settings.get(&key.into()) else{return Err("missing setting".into())};let value=s.to_string().map_err(|_|"invalid setting")?;if !values.contains(&value.as_str()){return Err("invalid setting value".into())}clean.insert(key.into(),Tag::String(value.as_str().into()));}
    let Some(Tag::List(entries))=saved.get("upgrades") else{return Err("invalid upgrades".into())};if entries.len()>4{return Err("too many upgrades".into())}
    let mut desired=Vec::new();for entry in entries {if entry.get("count")!=Some(&Tag::Int(1)){return Err("invalid upgrade count".into())}let stack=save::item_from_nbt(entry).ok_or("invalid upgrade")?;if stack.item.as_ref()!="ae2:speed_card" {return Err("invalid upgrade type".into())}if desired.first().is_some_and(|a:&ItemStack|!a.same_item_same_components(&stack)){return Err("mixed upgrade components require source grouping".into())}desired.push(stack);}
    let name=match saved.get("name") {None=>None,Some(Tag::String(s))=>Some(s.to_string().map_err(|_|"invalid name")?),_=>return Err("invalid name".into())};
    Ok(Some(SavedInscriber{settings:clean,desired,name}))
}
/// Source clear removes exported settings (including P2P) but leaves unrelated
/// item metadata and unrelated custom data intact.
pub(super) fn clear(card:&mut ItemStack)->Result<bool,String>{
    if card.item.as_ref()!="ae2:memory_card"||card.count!=1{return Err("invalid card".into())}
    let has_source=card.data().is_some_and(|d|d.other("ae2:memory_card_colors").is_some() || EXPORTED_COMPONENTS.iter().any(|id|d.other(id).is_some()));
    let tag=card.data().and_then(|d|d.other("minecraft:custom_data")).cloned();
    let mut data=match tag {None=>None,Some(Tag::Compound(data))=>Some(data),_=>return Err("invalid card custom data".into())};
    let changed=has_source|data.as_mut().is_some_and(|data|data.remove(&KEY.into()).is_some()|data.remove(&P2P.into()).is_some());
    if changed {
        if let Some(data)=data {card.data_mut().set_other("minecraft:custom_data",if data.is_empty(){None}else{Some(Tag::Compound(data))});}
        card.data_mut().set_other("ae2:memory_card_colors",None);
        for id in EXPORTED_COMPONENTS {card.data_mut().set_other(id,None);}
    }
    Ok(changed)
}

/// Tracked-object updates are deferred until the machine, player and Network
/// Tool snapshots have committed as one authoritative operation.
#[derive(Debug)] pub(super) struct PhysicalChanges {pub drops:Vec<ItemStack>,pub changed_tracked:Vec<ItemStack>,pub replaced_tracked:Vec<ItemStack>,pub missing:u32}
fn moved_copy(stack:&ItemStack,count:u32)->ItemStack {let mut out=stack.clone();out.count=count;horizon_client::ae2_stack_identity::detach_if_tracked(&mut out);out}
fn add_to_player(inv:&mut [Option<ItemStack>],stack:&mut ItemStack,max:u32,replaced:&mut Vec<ItemStack>){
    // InternalInventory.insertItem creates a copy even when merging into an
    // existing stack, then replaces the old object without shrinking it.
    for slot in inv.iter_mut(){let Some(old)=slot.as_ref() else{continue};if old.same_item_same_components(stack){let n=stack.count.min(max.saturating_sub(old.count));if n>0{let next=moved_copy(old,old.count+n);replaced.push(old.clone());*slot=Some(next);stack.count-=n;}if stack.count==0{return}}}
    for slot in inv {if slot.is_none(){let n=stack.count.min(max);*slot=Some(moved_copy(stack,n));stack.count-=n;if stack.count==0{return}}}
}
fn take_from_slots(inv:&mut [Option<ItemStack>],wanted:&ItemStack,mut count:u32,changes:&mut PhysicalChanges,tool:bool)->Vec<ItemStack>{
    let mut result=Vec::new();let mut first_original=None;let mut total=0;
    for slot in inv {if count==0{break}let Some(stack)=slot.as_ref() else{continue};if !stack.same_item_same_components(wanted){continue}let n=count.min(stack.count);let original=stack.clone();let remaining=stack.count-n;
        result.push(moved_copy(stack,n));count-=n;total+=n;
        if remaining==0 {if result.len()==1{first_original=Some(original.clone())}changes.replaced_tracked.push(original);*slot=None;}
        else if tool {let mut reduced=original;reduced.count=remaining;changes.changed_tracked.push(reduced.clone());*slot=Some(reduced);}
        else {changes.replaced_tracked.push(original.clone());*slot=Some(moved_copy(&original,remaining));}
    }
    // removeItems merges later extracted stacks into the first returned
    // object. If that was a whole tracked stack, its detached reference grows.
    if let Some(mut first)=first_original {if total>first.count {first.count=total;changes.changed_tracked.push(first);}}
    result
}
/// Literal Inscriber upgrade reconciliation, including the observed upstream
/// removal-pass Item-vs-ItemStack behavior. The caller supplies one real
/// Network Tool pocket, if present. No tracked-object global state changes
/// until the returned snapshots have been committed.
pub(super) fn apply_inscriber(m:&mut Machine,player:&mut Vec<Option<ItemStack>>,pocket:&mut Option<horizon_client::ae2_network_tool::Inventory>,saved:&SavedInscriber,creative:bool)->Result<PhysicalChanges,String>{
    if m.kind!=MachineKind::Inscriber||m.upgrades.len()!=4||player.len()!=36||saved.desired.len()>4{return Err("invalid reconciliation target".into())}
    if m.upgrades.iter().flatten().any(|s|s.item.as_ref()!="ae2:speed_card"||s.count!=1) {return Err("invalid existing upgrades".into())}
    let mut staged=m.clone();let mut inventory=player.clone();let mut tool=pocket.clone();let mut changes=PhysicalChanges{drops:Vec::new(),changed_tracked:Vec::new(),replaced_tracked:Vec::new(),missing:0};
    for (key,_,_) in FIELDS {staged.config.insert(key.into(),saved.settings.get(&key.into()).ok_or("missing setting")?.clone());}
    if creative {
        // Source setItemDirect clears before installing the desired cards.
        for old in staged.upgrades.iter().flatten(){changes.replaced_tracked.push(old.clone())}
        staged.upgrades=vec![None;4];
        for (slot,card) in saved.desired.iter().enumerate(){staged.upgrades[slot]=Some(moved_copy(card,1));}
    }
    // The pinned source looks up ItemStack in an Item-keyed map and therefore
    // removes all currently installed speed cards, confirmed by the Java oracle.
    for slot in &mut staged.upgrades {
        let Some(mut removed)=slot.take() else{continue};changes.replaced_tracked.push(removed.clone());horizon_client::ae2_stack_identity::detach_if_tracked(&mut removed);
        add_to_player(&mut inventory,&mut removed,64,&mut changes.replaced_tracked);
        if removed.count>0 {if let Some(tool)=tool.as_mut(){add_to_player(&mut tool.slots,&mut removed,64,&mut changes.replaced_tracked);}}
        if removed.count>0{changes.drops.push(removed)}
    }
    // Actual upstream removal uses a default-component filter, not merely an
    // item-ID comparison. Marked cards must not be silently consumed.
    let desired=saved.desired.len() as u32;let filter=ItemStack::new("ae2:speed_card",desired.max(1));
    let mut acquired=take_from_slots(&mut inventory,&filter,desired,&mut changes,false);
    let have:u32=acquired.iter().map(|s|s.count).sum();
    if have<desired {if let Some(tool)=tool.as_mut(){acquired.extend(take_from_slots(&mut tool.slots,&filter,desired-have,&mut changes,true));}}
    let mut placed=0;for stack in acquired {for _ in 0..stack.count {if placed>=4{return Err("upgrade capacity changed".into())}staged.upgrades[placed]=Some(moved_copy(&stack,1));placed+=1;}}
    changes.missing=desired.saturating_sub(placed as u32);
    *m=staged;*player=inventory;*pocket=tool;Ok(changes)
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn source_inscriber_card_groups_upgrades_and_imports_without_legacy_envelope(){
        let mut source=Machine::new(MachineKind::Inscriber);
        source.config.insert("auto_export".into(),Tag::String("YES".into()));
        source.upgrades[0]=Some(ItemStack::new("ae2:speed_card",1));source.upgrades[1]=Some(ItemStack::new("ae2:speed_card",1));
        let mut card=ItemStack::new("ae2:memory_card",1);
        save_inscriber(&mut card,&source,Some("press")).unwrap();publish_simple_source(&mut card).unwrap();
        let Some(Tag::List(upgrades))=card.data().unwrap().other("ae2:exported_upgrades") else{panic!("source upgrades missing")};
        assert_eq!(upgrades.len(),1);assert_eq!(upgrades[0].get("count"),Some(&Tag::Int(2)));
        card.data_mut().set_other("minecraft:custom_data",None);
        let imported=simple_source_card(&card).unwrap().unwrap();
        let settings=read_inscriber(&imported).unwrap().unwrap();
        assert_eq!(settings.desired.len(),2);assert_eq!(settings.settings.get(&"auto_export".into()),Some(&Tag::String("YES".into())));assert_eq!(settings.name.as_deref(),Some("press"));
        card.data_mut().set_other("ae2:exported_upgrades",Some(Tag::List(vec![Tag::compound([("id",Tag::String("ae2:speed_card".into())),("count",Tag::Int(5))])])));
        assert!(simple_source_card(&card).is_err());
    }
    #[test] fn source_description_compares_component_semantics(){
        let key="item.ae2.level_emitter";let base=Tag::compound([("translate",Tag::String(key.into()))]);
        assert!(source_description_matches(Some(&base),key));assert!(source_description_matches(Some(&Tag::List(vec![base.clone()])),key));
        let defaults=Tag::compound([("translate",Tag::String(key.into())),("type",Tag::String("translatable".into())),("with",Tag::List(vec![])),("extra",Tag::List(vec![])),("ignored",Tag::Int(7))]);
        assert!(source_description_matches(Some(&defaults),key));
        let styled=Tag::compound([("translate",Tag::String(key.into())),("color",Tag::String("red".into()))]);assert!(!source_description_matches(Some(&styled),key));
        let sibling=Tag::compound([("translate",Tag::String(key.into())),("extra",Tag::List(vec![Tag::String("!".into())]))]);assert!(!source_description_matches(Some(&sibling),key));
    }
    #[test] fn source_io_card_translates_registered_settings_names(){
        let mut source=Machine::new(MachineKind::IoPort);
        source.config.insert("io_operation".into(),Tag::String("fill".into()));source.config.insert("io_redstone".into(),Tag::String("high".into()));
        let mut card=ItemStack::new("ae2:memory_card",1);save_io(&mut card,&source,None).unwrap();publish_simple_source(&mut card).unwrap();
        let settings=card.data().unwrap().other("ae2:exported_settings").unwrap();
        assert_eq!(settings.get("operation_mode"),Some(&Tag::String("FILL".into())));
        assert_eq!(settings.get("redstone_controlled"),Some(&Tag::String("HIGH_SIGNAL".into())));
        card.data_mut().set_other("minecraft:custom_data",None);
        let imported=simple_source_card(&card).unwrap().unwrap();
        assert_eq!(read_io(&imported).unwrap().unwrap().settings.get(&"io_redstone".into()),Some(&Tag::String("high".into())));
        card.data_mut().set_other("ae2:exported_settings_source",Some(Tag::compound([
            ("translate",Tag::String("block.ae2.io_port".into())),("color",Tag::String("red".into())),
        ])));
        assert!(simple_source_card(&card).unwrap().is_none(),"styled foreign descriptions do not authorize full same-source import");
    }
    #[test] fn clear_removes_source_led_component_without_changing_dye(){
        let mut card=ItemStack::new("ae2:memory_card",1);
        card.data_mut().dyed_color=Some(0x123456);
        card.data_mut().set_other("ae2:memory_card_colors",Some(Tag::List(vec![Tag::String("red".into())])));
        card.data_mut().set_other("ae2:exported_p2p_frequency",Some(Tag::Short(42)));
        card.data_mut().set_other("ae2:exported_priority",Some(Tag::Int(7)));
        card.data_mut().set_other("ae2:facade_cycle_property",Some(Tag::String("axis".into())));
        assert!(clear(&mut card).unwrap());
        assert!(card.data().unwrap().other("ae2:memory_card_colors").is_none());
        assert!(card.data().unwrap().other("ae2:exported_p2p_frequency").is_none());
        assert!(card.data().unwrap().other("ae2:exported_priority").is_none());
        assert_eq!(card.data().unwrap().other("ae2:facade_cycle_property"),Some(&Tag::String("axis".into())));
        assert_eq!(card.data().unwrap().dyed_color,Some(0x123456));
    }
    #[test] fn source_survival_reconciliation_preserves_real_items_and_full_inventory_drop(){
        let mut card=ItemStack::new("ae2:memory_card",1);card.data_mut().custom_name=Some(horizon_client::item_data::text_name("my named card"));card.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("unrelated",Tag::Int(73)),(P2P,Tag::compound([("frequency",Tag::Short(4))]))])));
        let mut source=Machine::new(MachineKind::Inscriber);source.config.insert("auto_export".into(),Tag::String("YES".into()));source.upgrades[0]=Some(ItemStack::new("ae2:speed_card",1));source.slots[2]=Some(ItemStack::new("minecraft:gold_ingot",3));let before=source.clone();save_inscriber(&mut card,&source,Some("named machine")).unwrap();assert_eq!(source,before);assert_eq!(card.data().unwrap().custom_name_plain().as_deref(),Some("my named card"));assert_eq!(card.data().unwrap().other("minecraft:custom_data").unwrap().get("unrelated"),Some(&Tag::Int(73)));assert!(card.data().unwrap().other("minecraft:custom_data").unwrap().get(P2P).is_none());
        let saved=read_inscriber(&card).unwrap().unwrap();assert_eq!(saved.name.as_deref(),Some("named machine"));let mut target=Machine::new(MachineKind::Inscriber);target.slots[2]=Some(ItemStack::new("minecraft:diamond",2));target.energy=17.0;target.upgrades[0]=Some(ItemStack::new("ae2:speed_card",1));let mut player=vec![Some(ItemStack::new("minecraft:stone",64));36];let mut pocket=None;
        let result=apply_inscriber(&mut target,&mut player,&mut pocket,&saved,false).unwrap();assert_eq!(result.drops,vec![ItemStack::new("ae2:speed_card",1)]);assert_eq!(result.missing,1);assert!(target.upgrades.iter().all(Option::is_none));assert_eq!(target.slots[2].as_ref().unwrap().count,2);assert_eq!(target.energy,17.0);assert_eq!(target.config.get(&"auto_export".into()),Some(&Tag::String("YES".into())));
        let mut target=Machine::new(MachineKind::Inscriber);target.upgrades[0]=Some(ItemStack::new("ae2:speed_card",1));let mut empty=vec![None;36];let r=apply_inscriber(&mut target,&mut empty,&mut pocket,&saved,false).unwrap();assert!(r.drops.is_empty());assert_eq!(r.missing,0);assert_eq!(target.upgrades.iter().flatten().count(),1);
        let mut target=Machine::new(MachineKind::Inscriber);target.upgrades[0]=Some(ItemStack::new("ae2:speed_card",1));let mut full=vec![Some(ItemStack::new("minecraft:stone",64));36];let mut tool=Some(horizon_client::ae2_network_tool::Inventory::read(&ItemStack::new("ae2:network_tool",1)).unwrap());let r=apply_inscriber(&mut target,&mut full,&mut tool,&saved,false).unwrap();assert!(r.drops.is_empty());assert_eq!(r.missing,0);assert_eq!(target.upgrades.iter().flatten().count(),1);assert!(tool.unwrap().slots.iter().all(Option::is_none));
        assert!(clear(&mut card).unwrap());assert!(read_inscriber(&card).unwrap().is_none());assert_eq!(card.data().unwrap().custom_name_plain().as_deref(),Some("my named card"));assert_eq!(card.data().unwrap().other("minecraft:custom_data").unwrap().get("unrelated"),Some(&Tag::Int(73)));
    }
}
// Same-type I/O Port memory support, staged alongside the existing shared helpers.
const IO_FIELDS: [(&str,&str,&[&str]);3] = [
    ("io_operation", "empty", &["empty", "fill"]),
    ("io_fullness", "empty", &["empty", "full", "half"]),
    ("io_redstone", "ignore", &["ignore", "high", "low"]),
];
#[derive(Clone,Debug)] pub(super) struct SavedIo { pub settings: Compound, pub desired: Vec<ItemStack>, pub name: Option<String> }
fn io_validate(upgrades:&[Option<ItemStack>])->Result<(),String> {
    if upgrades.len()!=3{return Err("invalid IO upgrade slots".into())}
    let mut speed=0;let mut redstone=0;let mut first:std::collections::BTreeMap<&str,&ItemStack>=Default::default();
    for stack in upgrades.iter().flatten(){
        if stack.count!=1{return Err("invalid upgrade count".into())}
        match stack.item.as_ref(){"ae2:speed_card"=>speed+=1,"ae2:redstone_card"=>redstone+=1,_=>return Err("invalid upgrade type".into())}
        if first.get(stack.item.as_ref()).is_some_and(|other|!other.same_item_same_components(stack)){return Err("mixed upgrade components require source grouping".into())}
        first.insert(stack.item.as_ref(),stack);
    }
    if speed>3||redstone>1{return Err("invalid upgrade limits".into())} Ok(())
}
pub(super) fn save_io(card:&mut ItemStack,m:&Machine,name:Option<&str>)->Result<(),String>{
    if card.item.as_ref()!="ae2:memory_card"||card.count!=1||m.kind!=MachineKind::IoPort{return Err("invalid card or target".into())}
    io_validate(&m.upgrades)?;
    let mut settings=Compound::new();for (key,default,values) in IO_FIELDS {settings.insert(key.into(),Tag::String(setting_value(m,key,default,values)?.as_str().into()));}
    let mut data=match card.data().and_then(|d|d.other("minecraft:custom_data")){None=>Compound::new(),Some(Tag::Compound(c))=>c.clone(),_=>return Err("invalid card custom data".into())};
    let mut saved=Compound::new();saved.insert("source".into(),Tag::String("ae2:io_port".into()));saved.insert("settings".into(),Tag::Compound(settings));saved.insert("upgrades".into(),Tag::List(m.upgrades.iter().flatten().map(save::item_to_nbt).collect()));if let Some(name)=name {saved.insert("name".into(),Tag::String(name.into()));}
    data.remove(&P2P.into());data.insert(KEY.into(),Tag::Compound(saved));card.data_mut().set_other("minecraft:custom_data",Some(Tag::Compound(data)));Ok(())
}
pub(super) fn read_io(card:&ItemStack)->Result<Option<SavedIo>,String>{
    if card.item.as_ref()!="ae2:memory_card"||card.count!=1{return Err("invalid card".into())}
    let Some(data)=card.data().and_then(|d|d.other("minecraft:custom_data")) else{return Ok(None)};let Some(saved)=data.get(KEY) else{return Ok(None)};
    if saved.get("source")!=Some(&Tag::String("ae2:io_port".into())){return Ok(None)}
    let Some(Tag::Compound(settings))=saved.get("settings") else{return Err("invalid settings".into())};let mut clean=Compound::new();
    for (key,_,values) in IO_FIELDS {let Some(Tag::String(v))=settings.get(&key.into()) else{return Err("missing setting".into())};let value=v.to_string().map_err(|_|"invalid setting")?;if !values.contains(&value.as_str()){return Err("invalid setting value".into())}clean.insert(key.into(),Tag::String(value.as_str().into()));}
    let Some(Tag::List(entries))=saved.get("upgrades") else{return Err("invalid upgrades".into())};if entries.len()>3{return Err("too many upgrades".into())}
    let mut desired=Vec::new();for entry in entries {if entry.get("count")!=Some(&Tag::Int(1)){return Err("invalid count".into())}desired.push(save::item_from_nbt(entry).ok_or("invalid upgrade")?);}
    let mut slots=vec![None;3];for (i,s) in desired.iter().enumerate(){slots[i]=Some(s.clone())}io_validate(&slots)?;
    let name=match saved.get("name") {None=>None,Some(Tag::String(s))=>Some(s.to_string().map_err(|_|"invalid name")?),_=>return Err("invalid name".into())};Ok(Some(SavedIo{settings:clean,desired,name}))
}
pub(super) fn apply_io(m:&mut Machine,player:&mut Vec<Option<ItemStack>>,pocket:&mut Option<horizon_client::ae2_network_tool::Inventory>,saved:&SavedIo,creative:bool)->Result<PhysicalChanges,String>{
    if m.kind!=MachineKind::IoPort||player.len()!=36{return Err("invalid IO target".into())}io_validate(&m.upgrades)?;
    let mut desired_slots=vec![None;3];if saved.desired.len()>3{return Err("invalid desired size".into())}for (i,s) in saved.desired.iter().enumerate(){desired_slots[i]=Some(s.clone())}io_validate(&desired_slots)?;
    let mut staged=m.clone();let mut inventory=player.clone();let mut tool=pocket.clone();let mut changes=PhysicalChanges{drops:Vec::new(),changed_tracked:Vec::new(),replaced_tracked:Vec::new(),missing:0};
    for (key,_,_) in IO_FIELDS {staged.config.insert(key.into(),saved.settings.get(&key.into()).ok_or("missing setting")?.clone());}
    if creative {for old in staged.upgrades.iter().flatten(){changes.replaced_tracked.push(old.clone())}staged.upgrades=desired_slots;for slot in &mut staged.upgrades {if let Some(stack)=slot{horizon_client::ae2_stack_identity::detach_if_tracked(stack);}}}
    for slot in &mut staged.upgrades {let Some(mut removed)=slot.take() else{continue};changes.replaced_tracked.push(removed.clone());horizon_client::ae2_stack_identity::detach_if_tracked(&mut removed);add_to_player(&mut inventory,&mut removed,64,&mut changes.replaced_tracked);if removed.count>0 {if let Some(tool)=tool.as_mut(){add_to_player(&mut tool.slots,&mut removed,64,&mut changes.replaced_tracked);}}if removed.count>0{changes.drops.push(removed)}}
    let mut placed=0;for id in ["ae2:speed_card","ae2:redstone_card"] {let need=saved.desired.iter().filter(|s|s.item.as_ref()==id).count() as u32;if need==0 {continue}let filter=ItemStack::new(id,need);let mut found=take_from_slots(&mut inventory,&filter,need,&mut changes,false);let have:u32=found.iter().map(|s|s.count).sum();if have<need {if let Some(tool)=tool.as_mut(){found.extend(take_from_slots(&mut tool.slots,&filter,need-have,&mut changes,true));}}
      let count:u32=found.iter().map(|s|s.count).sum();changes.missing+=need.saturating_sub(count);for stack in found {for _ in 0..stack.count {if placed>=3{return Err("IO upgrade capacity changed".into())}staged.upgrades[placed]=Some(moved_copy(&stack,1));placed+=1;}}}
    *m=staged;*player=inventory;*pocket=tool;Ok(changes)
}
#[cfg(test)] mod io_tests {
    use super::*;
    #[test] fn io_memory_moves_real_mixed_upgrades_and_preserves_cells() {
        let mut source=Machine::new(MachineKind::IoPort);
        source.config.insert("io_operation".into(),Tag::String("fill".into()));
        source.config.insert("io_fullness".into(),Tag::String("full".into()));
        source.config.insert("io_redstone".into(),Tag::String("high".into()));
        source.upgrades[0]=Some(ItemStack::new("ae2:speed_card",1));
        source.upgrades[1]=Some(ItemStack::new("ae2:redstone_card",1));
        let mut card=ItemStack::new("ae2:memory_card",1);save_io(&mut card,&source,None).unwrap();
        let saved=read_io(&card).unwrap().unwrap();
        let mut target=Machine::new(MachineKind::IoPort);target.upgrades=source.upgrades.clone();
        target.slots[0]=Some(ItemStack::new("ae2:item_storage_cell_1k",1));target.energy=12.0;
        let mut full=vec![Some(ItemStack::new("minecraft:stone",64));36];let mut pocket=None;
        let result=apply_io(&mut target,&mut full,&mut pocket,&saved,false).unwrap();
        assert_eq!(result.missing,2);assert_eq!(result.drops.len(),2);assert!(target.upgrades.iter().all(Option::is_none));
        assert_eq!(target.slots[0].as_ref().unwrap().item.as_ref(),"ae2:item_storage_cell_1k");assert_eq!(target.energy,12.0);
        assert_eq!(target.config.get(&"io_operation".into()),Some(&Tag::String("fill".into())));
        let mut target=Machine::new(MachineKind::IoPort);target.upgrades=source.upgrades.clone();let mut empty=vec![None;36];
        let result=apply_io(&mut target,&mut empty,&mut pocket,&saved,false).unwrap();assert!(result.drops.is_empty());assert_eq!(result.missing,0);
        assert_eq!(target.upgrades.iter().flatten().count(),2);
    }
}
// Source's mismatched-block path imports generic shared components only. It
// never copies EXPORTED_CUSTOM_NAME and its incompatible configuration keys
// leave the target's own settings unchanged.
pub(super) fn io_to_inscriber(m:&Machine,source:&SavedIo)->Result<SavedInscriber,String>{
    if m.kind!=MachineKind::Inscriber{return Err("invalid cross-import target".into())}
    let mut settings=Compound::new();for (key,default,values) in FIELDS {settings.insert(key.into(),Tag::String(setting_value(m,key,default,values)?.as_str().into()));}
    let desired=source.desired.iter().filter(|s|s.item.as_ref()=="ae2:speed_card").cloned().collect();
    Ok(SavedInscriber{settings,desired,name:None})
}
pub(super) fn inscriber_to_io(m:&Machine,source:&SavedInscriber)->Result<SavedIo,String>{
    if m.kind!=MachineKind::IoPort{return Err("invalid cross-import target".into())}
    let mut settings=Compound::new();for (key,default,values) in IO_FIELDS {settings.insert(key.into(),Tag::String(setting_value(m,key,default,values)?.as_str().into()));}
    // Target I/O Port accepts at most three Speed Cards; source simulates this
    // capacity before attempting to take actual items from the player.
    let desired=source.desired.iter().take(3).cloned().collect();
    Ok(SavedIo{settings,desired,name:None})
}
/// These supported unlike-machine sources have no common configuration keys.
/// Keep the target's exact native configuration rather than materializing
/// defaults while reconciling only the exported physical upgrades.
pub(super) fn apply_inscriber_partial(m:&mut Machine,player:&mut Vec<Option<ItemStack>>,pocket:&mut Option<horizon_client::ae2_network_tool::Inventory>,saved:&SavedInscriber,creative:bool)->Result<PhysicalChanges,String>{
    let config=m.config.clone();
    let changes=apply_inscriber(m,player,pocket,saved,creative)?;
    m.config=config;
    Ok(changes)
}
pub(super) fn apply_io_partial(m:&mut Machine,player:&mut Vec<Option<ItemStack>>,pocket:&mut Option<horizon_client::ae2_network_tool::Inventory>,saved:&SavedIo,creative:bool)->Result<PhysicalChanges,String>{
    let config=m.config.clone();
    let changes=apply_io(m,player,pocket,saved,creative)?;
    m.config=config;
    Ok(changes)
}
#[cfg(test)] mod cross_tests {
    use super::*;
    #[test] fn unlike_machine_memory_moves_only_supported_physical_cards_and_keeps_settings(){
        let mut io=Machine::new(MachineKind::IoPort);io.config.insert("io_operation".into(),Tag::String("fill".into()));
        io.upgrades[0]=Some(ItemStack::new("ae2:speed_card",1));io.upgrades[1]=Some(ItemStack::new("ae2:redstone_card",1));
        let mut card=ItemStack::new("ae2:memory_card",1);save_io(&mut card,&io,Some("IO name")).unwrap();let saved=read_io(&card).unwrap().unwrap();
        let mut target=Machine::new(MachineKind::Inscriber);target.config.insert("auto_export".into(),Tag::String("YES".into()));target.upgrades[0]=Some(ItemStack::new("ae2:speed_card",1));
        let partial=io_to_inscriber(&target,&saved).unwrap();let mut player=vec![None;36];player[0]=Some(ItemStack::new("ae2:redstone_card",1));let mut pocket=None;
        let moved=apply_inscriber(&mut target,&mut player,&mut pocket,&partial,false).unwrap();assert_eq!(moved.missing,0);assert!(moved.drops.is_empty());assert_eq!(target.upgrades.iter().flatten().count(),1);assert_eq!(player[0].as_ref().unwrap().item.as_ref(),"ae2:redstone_card");assert_eq!(target.config.get(&"auto_export".into()),Some(&Tag::String("YES".into())));assert!(partial.name.is_none());
        let mut ins=Machine::new(MachineKind::Inscriber);for i in 0..4{ins.upgrades[i]=Some(ItemStack::new("ae2:speed_card",1));}save_inscriber(&mut card,&ins,None).unwrap();let source=read_inscriber(&card).unwrap().unwrap();let partial=inscriber_to_io(&io,&source).unwrap();assert_eq!(partial.desired.len(),3);assert_eq!(partial.settings.get(&"io_operation".into()),Some(&Tag::String("fill".into())));
    }
}

/// A Vibration Chamber has no generic configuration settings, only upgrades
/// and the same-kind exported name. Fuel and processing state are not saved.
#[derive(Clone, Debug)]
pub(super) struct SavedVibration {
    pub desired: Vec<ItemStack>,
    pub name: Option<String>,
}

fn vibration_validate(upgrades: &[Option<ItemStack>], groupable: bool) -> Result<(), String> {
    if upgrades.len() != 3 {
        return Err("invalid Vibration upgrade slots".into());
    }
    let mut first: std::collections::BTreeMap<&str, &ItemStack> = Default::default();
    for stack in upgrades.iter().flatten() {
        if stack.count != 1
            || !matches!(stack.item.as_ref(), "ae2:speed_card" | "ae2:energy_card")
        {
            return Err("invalid Vibration upgrade".into());
        }
        // The source groups exported stacks by item and components, then
        // keys desired counts by item. Until group order is proven, refuse to
        // save or import multiple component groups of the same item. Existing
        // physical upgrades can still be removed and returned safely.
        if groupable
            && first
                .get(stack.item.as_ref())
                .is_some_and(|other| !other.same_item_same_components(stack))
        {
            return Err("mixed upgrade components require source grouping".into());
        }
        first.insert(stack.item.as_ref(), stack);
    }
    Ok(())
}

pub(super) fn save_vibration(
    card: &mut ItemStack,
    machine: &Machine,
    name: Option<&str>,
) -> Result<(), String> {
    if card.item.as_ref() != "ae2:memory_card"
        || card.count != 1
        || machine.kind != MachineKind::VibrationChamber
    {
        return Err("invalid card or target".into());
    }
    vibration_validate(&machine.upgrades, true)?;
    let mut data = match card.data().and_then(|d| d.other("minecraft:custom_data")) {
        None => Compound::new(),
        Some(Tag::Compound(c)) => c.clone(),
        _ => return Err("invalid card custom data".into()),
    };
    let mut saved = Compound::new();
    saved.insert("source".into(), Tag::String("ae2:vibration_chamber".into()));
    saved.insert("settings".into(), Tag::Compound(Compound::new()));
    saved.insert(
        "upgrades".into(),
        Tag::List(machine.upgrades.iter().flatten().map(save::item_to_nbt).collect()),
    );
    if let Some(name) = name {
        saved.insert("name".into(), Tag::String(name.into()));
    }
    data.remove(&P2P.into());
    data.insert(KEY.into(), Tag::Compound(saved));
    card.data_mut().set_other("minecraft:custom_data", Some(Tag::Compound(data)));
    Ok(())
}

pub(super) fn read_vibration(card: &ItemStack) -> Result<Option<SavedVibration>, String> {
    if card.item.as_ref() != "ae2:memory_card" || card.count != 1 {
        return Err("invalid card".into());
    }
    let Some(data) = card.data().and_then(|d| d.other("minecraft:custom_data")) else {
        return Ok(None);
    };
    let Some(saved) = data.get(KEY) else { return Ok(None) };
    if saved.get("source") != Some(&Tag::String("ae2:vibration_chamber".into())) {
        return Ok(None);
    }
    match saved.get("settings") {
        Some(Tag::Compound(settings)) if settings.is_empty() => {}
        _ => return Err("unexpected Vibration settings".into()),
    }
    let Some(Tag::List(entries)) = saved.get("upgrades") else {
        return Err("invalid upgrades".into());
    };
    if entries.len() > 3 {
        return Err("too many upgrades".into());
    }
    let mut desired = Vec::with_capacity(entries.len());
    for entry in entries {
        if entry.get("count") != Some(&Tag::Int(1)) {
            return Err("invalid upgrade count".into());
        }
        desired.push(save::item_from_nbt(entry).ok_or("invalid upgrade")?);
    }
    let mut slots = vec![None; 3];
    for (i, stack) in desired.iter().enumerate() {
        slots[i] = Some(stack.clone());
    }
    vibration_validate(&slots, true)?;
    let name = match saved.get("name") {
        None => None,
        Some(Tag::String(s)) => Some(s.to_string().map_err(|_| "invalid name")?),
        _ => return Err("invalid name".into()),
    };
    Ok(Some(SavedVibration { desired, name }))
}

pub(super) fn apply_vibration(
    machine: &mut Machine,
    player: &mut Vec<Option<ItemStack>>,
    pocket: &mut Option<horizon_client::ae2_network_tool::Inventory>,
    saved: &SavedVibration,
    creative: bool,
) -> Result<PhysicalChanges, String> {
    if machine.kind != MachineKind::VibrationChamber || player.len() != 36 {
        return Err("invalid Vibration target".into());
    }
    vibration_validate(&machine.upgrades, false)?;
    if saved.desired.len() > 3 {
        return Err("invalid desired size".into());
    }
    let mut desired_slots = vec![None; 3];
    for (i, stack) in saved.desired.iter().enumerate() {
        desired_slots[i] = Some(stack.clone());
    }
    vibration_validate(&desired_slots, true)?;

    let mut staged = machine.clone();
    let mut inventory = player.clone();
    let mut tool = pocket.clone();
    let mut changes = PhysicalChanges { drops: Vec::new(), changed_tracked: Vec::new(), replaced_tracked: Vec::new(), missing: 0 };
    if creative {
        // Source first clears the old inventory and installs saved copies,
        // then still runs the ordinary removal/reacquisition pass below.
        for old in staged.upgrades.iter().flatten() {
            changes.replaced_tracked.push(old.clone());
        }
        staged.upgrades = desired_slots;
        for stack in staged.upgrades.iter_mut().flatten() {
            horizon_client::ae2_stack_identity::detach_if_tracked(stack);
        }
    }
    // The pinned source looks up ItemStack in an Item-keyed map. Consequently
    // every installed card is returned before attempting reacquisition.
    for slot in &mut staged.upgrades {
        let Some(mut removed) = slot.take() else { continue };
        changes.replaced_tracked.push(removed.clone());
        horizon_client::ae2_stack_identity::detach_if_tracked(&mut removed);
        add_to_player(&mut inventory, &mut removed, 64, &mut changes.replaced_tracked);
        if removed.count > 0 {
            if let Some(tool) = tool.as_mut() {
                add_to_player(&mut tool.slots, &mut removed, 64, &mut changes.replaced_tracked);
            }
        }
        if removed.count > 0 {
            changes.drops.push(removed);
        }
    }
    let mut placed = 0;
    for id in ["ae2:speed_card", "ae2:energy_card"] {
        let need = saved.desired.iter().filter(|s| s.item.as_ref() == id).count() as u32;
        if need == 0 { continue; }
        // Source searches with a freshly constructed, default-component card.
        let filter = ItemStack::new(id, need);
        let mut found = take_from_slots(&mut inventory, &filter, need, &mut changes, false);
        let have: u32 = found.iter().map(|s| s.count).sum();
        if have < need {
            if let Some(tool) = tool.as_mut() {
                found.extend(take_from_slots(&mut tool.slots, &filter, need - have, &mut changes, true));
            }
        }
        let count: u32 = found.iter().map(|s| s.count).sum();
        changes.missing += need.saturating_sub(count);
        for stack in found {
            for _ in 0..stack.count {
                if placed >= 3 { return Err("Vibration upgrade capacity changed".into()); }
                staged.upgrades[placed] = Some(moved_copy(&stack, 1));
                placed += 1;
            }
        }
    }
    *machine = staged;
    *player = inventory;
    *pocket = tool;
    Ok(changes)
}

fn vibration_partial(source: &[ItemStack]) -> SavedVibration {
    SavedVibration {
        desired: source.iter().filter(|s| s.item.as_ref() == "ae2:speed_card").take(3).cloned().collect(),
        name: None,
    }
}
pub(super) fn inscriber_to_vibration(source: &SavedInscriber) -> SavedVibration {
    vibration_partial(&source.desired)
}
pub(super) fn io_to_vibration(source: &SavedIo) -> SavedVibration {
    vibration_partial(&source.desired)
}
pub(super) fn vibration_to_inscriber(machine: &Machine, source: &SavedVibration) -> Result<SavedInscriber, String> {
    if machine.kind != MachineKind::Inscriber { return Err("invalid cross-import target".into()); }
    let mut settings = Compound::new();
    for (key, default, values) in FIELDS {
        settings.insert(key.into(), Tag::String(setting_value(machine, key, default, values)?.as_str().into()));
    }
    Ok(SavedInscriber {
        settings,
        desired: source.desired.iter().filter(|s| s.item.as_ref() == "ae2:speed_card").cloned().collect(),
        name: None,
    })
}
pub(super) fn vibration_to_io(machine: &Machine, source: &SavedVibration) -> Result<SavedIo, String> {
    if machine.kind != MachineKind::IoPort { return Err("invalid cross-import target".into()); }
    let mut settings = Compound::new();
    for (key, default, values) in IO_FIELDS {
        settings.insert(key.into(), Tag::String(setting_value(machine, key, default, values)?.as_str().into()));
    }
    Ok(SavedIo {
        settings,
        desired: source.desired.iter().filter(|s| s.item.as_ref() == "ae2:speed_card").cloned().collect(),
        name: None,
    })
}

#[cfg(test)]
mod vibration_tests {
    use super::*;

    #[test]
    fn vibration_memory_preserves_fuel_and_state_with_full_inventory_overflow() {
        let mut source = Machine::new(MachineKind::VibrationChamber);
        source.upgrades[0] = Some(ItemStack::new("ae2:speed_card", 1));
        source.upgrades[1] = Some(ItemStack::new("ae2:energy_card", 1));
        let mut card = ItemStack::new("ae2:memory_card", 1);
        save_vibration(&mut card, &source, Some("named chamber")).unwrap();
        let saved = read_vibration(&card).unwrap().unwrap();
        assert_eq!(saved.name.as_deref(), Some("named chamber"));

        let mut target = source.clone();
        target.slots[0] = Some(ItemStack::new("minecraft:coal", 3));
        target.energy = 12.0;
        target.burn_remaining = 63.0;
        target.burn_rate = 1.5;
        target.progress = 2.0;
        target.config.insert("unrelated".into(), Tag::Int(73));
        let mut expected = target.clone();
        expected.upgrades = vec![None; 3];
        let mut inventory = vec![Some(ItemStack::new("minecraft:stone", 64)); 36];
        let mut pocket = None;
        let moved = apply_vibration(&mut target, &mut inventory, &mut pocket, &saved, false).unwrap();
        assert_eq!(moved.drops.len(), 2);
        assert_eq!(moved.missing, 2);
        assert_eq!(target, expected);
    }

    #[test]
    fn vibration_creative_still_returns_then_reacquires_saved_cards() {
        let saved = SavedVibration {
            desired: vec![ItemStack::new("ae2:speed_card", 1), ItemStack::new("ae2:energy_card", 1)],
            name: None,
        };
        let mut target = Machine::new(MachineKind::VibrationChamber);
        let mut old = ItemStack::new("ae2:speed_card", 1);
        old.data_mut().custom_name = Some(horizon_client::item_data::text_name("discarded original"));
        target.upgrades[0] = Some(old);
        let mut inventory = vec![Some(ItemStack::new("minecraft:stone", 64)); 36];
        let mut pocket = None;
        let moved = apply_vibration(&mut target, &mut inventory, &mut pocket, &saved, true).unwrap();
        assert_eq!(moved.missing, 2);
        assert_eq!(moved.drops.len(), 2);
        assert!(moved.drops.iter().all(|stack| stack.data().and_then(|d| d.custom_name.as_ref()).is_none()));
        assert!(target.upgrades.iter().all(Option::is_none));
    }

    #[test]
    fn vibration_moves_real_cards_and_keeps_marked_existing_cards() {
        let mut target = Machine::new(MachineKind::VibrationChamber);
        let mut marked = ItemStack::new("ae2:speed_card", 1);
        marked.data_mut().custom_name = Some(horizon_client::item_data::text_name("marked"));
        target.upgrades[0] = Some(marked.clone());
        target.upgrades[1] = Some(ItemStack::new("ae2:speed_card", 1));
        let saved = SavedVibration { desired: vec![ItemStack::new("ae2:speed_card", 1), ItemStack::new("ae2:energy_card", 1)], name: None };
        let mut inventory = vec![None; 36];
        inventory[0] = Some(ItemStack::new("ae2:energy_card", 1));
        let mut pocket = None;
        let moved = apply_vibration(&mut target, &mut inventory, &mut pocket, &saved, false).unwrap();
        assert!(moved.drops.is_empty());
        assert_eq!(moved.missing, 0);
        assert!(inventory.iter().flatten().any(|s| s.same_item_same_components(&marked)));
        assert_eq!(target.upgrades.iter().flatten().count(), 2);
        let mut card = ItemStack::new("ae2:memory_card", 1);
        let mut mixed = Machine::new(MachineKind::VibrationChamber);
        mixed.upgrades[0] = Some(marked);
        mixed.upgrades[1] = Some(ItemStack::new("ae2:speed_card", 1));
        assert!(save_vibration(&mut card, &mixed, None).is_err());
    }

    #[test]
    fn vibration_reconciliation_defers_real_stack_replacement_notifications() {
        use horizon_client::ae2_stack_identity::{mark_detached_by_replacement, synchronize_tracked, StackRef};
        for creative in [false,true] {
            let mut existing=ItemStack::new("ae2:speed_card",1);
            let retained=StackRef::capture(&mut existing);
            let mut target=Machine::new(MachineKind::VibrationChamber);
            target.upgrades[0]=Some(existing);
            let saved=SavedVibration{desired:vec![ItemStack::new("ae2:speed_card",1)],name:None};
            let mut player=vec![None;36];let mut pocket=None;
            let changes=apply_vibration(&mut target,&mut player,&mut pocket,&saved,creative).unwrap();
            assert!(!retained.is_known_detached());
            assert_eq!(retained.read().count,1);
            for old in &changes.replaced_tracked {mark_detached_by_replacement(old,None);}
            for new in &changes.changed_tracked {synchronize_tracked(new);}
            assert!(retained.is_known_detached());
            assert_eq!(retained.read().count,1);
            assert_eq!(target.upgrades.iter().flatten().count(),1);
            assert!(changes.drops.is_empty());
        }
    }

    #[test]
    fn vibration_player_extraction_retains_source_merge_identity() {
        use horizon_client::ae2_stack_identity::{mark_detached_by_replacement, synchronize_tracked, StackRef};
        let mut first=ItemStack::new("ae2:speed_card",1);
        let retained=StackRef::capture(&mut first);
        let mut player=vec![None;36];player[0]=Some(first);player[1]=Some(ItemStack::new("ae2:speed_card",1));
        let saved=SavedVibration{desired:vec![ItemStack::new("ae2:speed_card",1);2],name:None};
        let mut target=Machine::new(MachineKind::VibrationChamber);let mut pocket=None;
        let changes=apply_vibration(&mut target,&mut player,&mut pocket,&saved,false).unwrap();
        assert_eq!(retained.read().count,1);
        for old in &changes.replaced_tracked {mark_detached_by_replacement(old,None);}
        for new in &changes.changed_tracked {synchronize_tracked(new);}
        assert!(retained.is_known_detached());
        assert_eq!(retained.read().count,2);
        assert_eq!(target.upgrades.iter().flatten().count(),2);
        assert!(player.iter().all(Option::is_none));
    }

    #[test]
    fn vibration_cross_kind_import_only_uses_supported_speed_cards() {
        let source = SavedVibration { desired: vec![ItemStack::new("ae2:speed_card", 1), ItemStack::new("ae2:energy_card", 1)], name: Some("source".into()) };
        let mut io = Machine::new(MachineKind::IoPort);
        io.config.insert("io_operation".into(), Tag::String("fill".into()));
        let partial = vibration_to_io(&io, &source).unwrap();
        assert_eq!(partial.desired.len(), 1);
        assert_eq!(partial.settings.get(&"io_operation".into()), Some(&Tag::String("fill".into())));
        assert!(partial.name.is_none());
        let before_config=io.config.clone();
        let mut player=vec![None;36];player[0]=Some(ItemStack::new("ae2:speed_card",1));
        let mut pocket=None;
        apply_io_partial(&mut io,&mut player,&mut pocket,&partial,false).unwrap();
        assert_eq!(io.config,before_config);
        let mut inscriber = Machine::new(MachineKind::Inscriber);
        let partial=vibration_to_inscriber(&inscriber, &source).unwrap();
        assert_eq!(partial.desired.len(), 1);
        let before_config=inscriber.config.clone();
        let mut player=vec![None;36];player[0]=Some(ItemStack::new("ae2:speed_card",1));
        apply_inscriber_partial(&mut inscriber,&mut player,&mut pocket,&partial,false).unwrap();
        assert_eq!(inscriber.config,before_config);
        let four = SavedInscriber { settings: Compound::new(), desired: vec![ItemStack::new("ae2:speed_card", 1); 4], name: None };
        assert_eq!(inscriber_to_vibration(&four).desired.len(), 3);
        let io_saved = SavedIo { settings: Compound::new(), desired: vec![ItemStack::new("ae2:speed_card", 1), ItemStack::new("ae2:redstone_card", 1)], name: None };
        assert_eq!(io_to_vibration(&io_saved).desired.len(), 1);
    }
}

include!("ae2_memory_interface.rs");
include!("ae2_memory_emitter.rs");
include!("ae2_memory_energy.rs");
include!("ae2_memory_storage.rs");
include!("ae2_memory_assembler.rs");
include!("ae2_memory_condenser.rs");
include!("ae2_memory_bus.rs");
