//! AE2 19.2.18 Pattern Provider Memory Card import (LGPL-3.0-or-later).
//! Pattern definitions are descriptions; restoration spends physical blanks.
//! Face ownership and the source block/part description asymmetry are preserved.
use super::Game;
use horizon_client::{ae2_world::{Machine,MachineKind},containers::BlockEntity,items::ItemStack,save};
use horizon_world::nbt::{Compound,Tag};
const KEY:&str="ae2_memory_settings";
const BLOCK_SOURCE:&str="ae2:pattern_provider";
const PART_SOURCE:&str="ae2:cable_pattern_provider";
fn provider_source(saved:&Tag)->bool {
    saved.get("source")==Some(&Tag::String(BLOCK_SOURCE.into())) || saved.get("source")==Some(&Tag::String(PART_SOURCE.into()))
}
fn has_source_exports(card:&ItemStack)->bool {
    card.data().is_some_and(|data|[
        "ae2:exported_settings_source","ae2:exported_custom_name","ae2:exported_upgrades",
        "ae2:exported_settings","ae2:exported_priority","ae2:exported_p2p_type",
        "ae2:exported_p2p_frequency","ae2:exported_config_inv","ae2:exported_level_emitter_value",
        "ae2:exported_patterns","ae2:exported_push_direction",
    ].iter().any(|id|data.other(id).is_some()))
}
#[derive(Clone,Copy)]
enum Target {Block,Part {index:usize,face:u8}}
struct Snapshot {target:Target,host:Machine,machine:Machine,incarnation:u64,name:Option<String>,name_component:Option<Tag>,push:String}


/// Generic priority is independent of the pattern component: unlike-machine
/// imports must not decode, clear, or reconstruct a provider's physical slots.
pub(super) fn priority_from_card(card:&ItemStack)->Result<Option<i32>,String>{
    if card.item.as_ref()!="ae2:memory_card"||card.count!=1{return Err("invalid memory card".into())}
    if has_source_exports(card) {
        return match card.data().and_then(|data|data.other("ae2:exported_priority")) {None=>Ok(None),Some(Tag::Int(n))=>Ok(Some(*n)),_=>Err("invalid exported priority".into())};
    }
    let Some(custom)=card.data().and_then(|d|d.other("minecraft:custom_data")) else{return Ok(None)};
    let Tag::Compound(_)=custom else{return Err("invalid memory card data".into())};
    let Some(saved)=custom.get(KEY) else{return Ok(None)};
    if !provider_source(saved){return Ok(None)}
    match saved.get("priority"){None=>Ok(None),Some(Tag::Int(n))=>Ok(Some(*n)),_=>Err("invalid exported priority".into())}
}

#[derive(Clone,Debug)]
struct Saved {
    patterns:Vec<Option<ItemStack>>,name:Option<String>,push:Option<String>,settings:Settings,
}
#[derive(Clone,Debug,Default)]
struct Settings {blocking:Option<bool>,visible:Option<bool>,lock:Option<i32>,priority:Option<i32>}
impl Settings {
    fn any(&self)->bool{self.blocking.is_some()||self.visible.is_some()||self.lock.is_some()||self.priority.is_some()}
}
#[derive(Clone,Copy)]
enum Refresh {Patterns,Priority}
fn integer(m:&Machine,key:&str,default:i32)->Result<i32,String>{
    match m.config.get(&key.into()){None=>Ok(default),Some(Tag::Int(n))=>Ok(*n),_=>Err(format!("invalid provider {key}"))}
}
fn boolean(m:&Machine,key:&str,default:bool)->Result<bool,String>{
    match m.config.get(&key.into()){None=>Ok(default),Some(Tag::Byte(0))=>Ok(false),Some(Tag::Byte(1))=>Ok(true),_=>Err(format!("invalid provider {key}"))}
}
fn copy_count(stack:&ItemStack,count:u32)->ItemStack{
    let mut copy=stack.clone();copy.count=count;
    horizon_client::ae2_stack_identity::detach_if_tracked(&mut copy);copy
}
fn save_card(card:&mut ItemStack,m:&Machine,name:Option<&str>,source:&str,push:Option<&str>)->Result<(),String>{
    if m.kind!=MachineKind::PatternProvider||m.slots.len()!=9{return Err("invalid provider inventory".into())}
    let lock=integer(m,"ae2_lock_mode",0)?;if !(0..=4).contains(&lock){return Err("invalid provider lock".into())}
    if ![BLOCK_SOURCE,PART_SOURCE].contains(&source){return Err("invalid provider source".into())}
    if push.is_some_and(|push|!["all","down","up","north","south","west","east"].contains(&push)){return Err("invalid provider direction".into())}
    let mut entries=Vec::new();for (slot,item) in m.slots.iter().enumerate(){if let Some(item)=item{
        if item.count==0{return Err("empty provider stack".into())}
        let Tag::Compound(mut entry)=save::item_to_nbt(item) else{return Err("invalid provider stack".into())};
        entry.insert("Slot".into(),Tag::Byte(slot as i8));entries.push(Tag::Compound(entry));
    }}
    let mut saved=Compound::new();saved.insert("source".into(),Tag::String(source.into()));
    saved.insert("patterns".into(),Tag::List(entries));if let Some(push)=push{saved.insert("push".into(),Tag::String(push.into()));}
    saved.insert("blocking".into(),Tag::Byte(i8::from(boolean(m,"ae2_blocking",false)?)));
    saved.insert("visible".into(),Tag::Byte(i8::from(boolean(m,"terminal_visible",true)?)));
    saved.insert("lock".into(),Tag::Int(lock));saved.insert("priority".into(),Tag::Int(integer(m,"priority",0)?));
    if let Some(name)=name{saved.insert("name".into(),Tag::String(name.into()));}
    super::ae2_memory::clear(card)?;
    let mut data=match card.data().and_then(|d|d.other("minecraft:custom_data")){None=>Compound::new(),Some(Tag::Compound(data))=>data.clone(),_=>return Err("invalid memory card data".into())};
    data.insert(KEY.into(),Tag::Compound(saved));card.data_mut().set_other("minecraft:custom_data",Some(Tag::Compound(data)));
    let description=if source==BLOCK_SOURCE {"block.ae2.pattern_provider"}else{"item.ae2.cable_pattern_provider"};
    card.data_mut().set_other("ae2:exported_settings_source",Some(Tag::compound([("translate",Tag::String(description.into()))])));
    card.data_mut().set_other("ae2:exported_patterns",Some(Tag::List(m.slots.iter().enumerate().filter_map(|(slot,item)|item.as_ref().map(|item|Tag::compound([("slot",Tag::Int(slot as i32)),("item",save::item_to_nbt(item))]))).collect())));
    const LOCKS:[&str;5]=["NONE","LOCK_UNTIL_PULSE","LOCK_WHILE_HIGH","LOCK_WHILE_LOW","LOCK_UNTIL_RESULT"];
    card.data_mut().set_other("ae2:exported_settings",Some(Tag::compound([
        ("blocking_mode",Tag::String(if boolean(m,"ae2_blocking",false)? {"YES"}else{"NO"}.into())),
        ("pattern_access_terminal",Tag::String(if boolean(m,"terminal_visible",true)? {"YES"}else{"NO"}.into())),
        ("lock_crafting_mode",Tag::String(LOCKS[lock as usize].into())),
    ])));
    card.data_mut().set_other("ae2:exported_priority",Some(Tag::Int(integer(m,"priority",0)?)));
    card.data_mut().set_other("ae2:exported_push_direction",push.map(|value|Tag::String(value.into())));
    card.data_mut().set_other("ae2:exported_custom_name",name.map(|value|Tag::String(value.into())));
    Ok(())
}
fn read_card(card:&ItemStack)->Result<Option<Saved>,String>{
    if has_source_exports(card) {return read_source_card(card);}
    let Some(data)=card.data().and_then(|d|d.other("minecraft:custom_data")).and_then(|d|d.get(KEY)) else{return Ok(None)};
    if data.get("source")!=Some(&Tag::String("ae2:pattern_provider".into())){return Ok(None)}
    // Source getOrDefault(EXPORTED_PATTERNS, EMPTY) clears/recycles old
    // patterns even when a same-kind card omits that component entirely.
    let entries=match data.get("patterns"){None=>&[][..],Some(Tag::List(entries))=>entries.as_slice(),_=>return Err("invalid provider patterns".into())};
    if entries.len()>9{return Err("too many provider patterns".into())}
    let mut patterns=vec![None;9];for entry in entries{
        let Some(Tag::Byte(slot))=entry.get("Slot") else{return Err("missing pattern slot".into())};
        if !(0..9).contains(slot)||patterns[*slot as usize].is_some(){return Err("invalid pattern slot".into())}
        let stack=save::item_from_nbt(entry).filter(|s|s.count>0).ok_or("invalid saved pattern")?;
        patterns[*slot as usize]=Some(stack);
    }
    let push=match data.get("push"){
        None=>None,Some(Tag::String(value))=>{
            let value=value.to_string().map_err(|_|"invalid direction")?;
            if !["all","down","up","north","south","west","east"].contains(&value.as_str()){return Err("invalid direction".into())}Some(value)
        },_=>return Err("invalid direction".into())
    };
    let name=match data.get("name"){None=>None,Some(Tag::String(s))=>Some(s.to_string().map_err(|_|"invalid name")?),_=>return Err("invalid name".into())};
    Ok(Some(Saved{patterns,name,push,settings:read_settings(data)?}))
}
fn read_source_settings(card:&ItemStack)->Result<Settings,String> {
    let data=card.data().ok_or("invalid memory card")?;
    let settings=data.other("ae2:exported_settings");
    if settings.is_some_and(|value|!matches!(value,Tag::Compound(_))){return Err("invalid exported settings".into());}
    let string=|key:&str|settings.and_then(|value|value.get(key)).and_then(|value|if let Tag::String(v)=value {v.to_string().ok()}else{None});
    let flag=|key:&str|match string(key).as_deref(){Some("YES")=>Some(true),Some("NO")=>Some(false),_=>None};
    let lock=match string("lock_crafting_mode").as_deref(){Some("NONE")=>Some(0),Some("LOCK_UNTIL_PULSE")=>Some(1),Some("LOCK_WHILE_HIGH")=>Some(2),Some("LOCK_WHILE_LOW")=>Some(3),Some("LOCK_UNTIL_RESULT")=>Some(4),_=>None};
    let priority=match data.other("ae2:exported_priority"){None=>None,Some(Tag::Int(n))=>Some(*n),_=>return Err("invalid exported priority".into())};
    Ok(Settings{blocking:flag("blocking_mode"),visible:flag("pattern_access_terminal"),lock,priority})
}
fn read_source_card(card:&ItemStack)->Result<Option<Saved>,String> {
    let Some(data)=card.data() else{return Ok(None)};
    if !super::ae2_memory::source_description_matches(data.other("ae2:exported_settings_source"),"block.ae2.pattern_provider") {return Ok(None);}
    let entries=match data.other("ae2:exported_patterns"){None=>&[][..],Some(Tag::List(entries))=>entries.as_slice(),_=>return Err("invalid source provider patterns".into())};
    let mut patterns=vec![None;9];for entry in entries {
        let Some(Tag::Int(slot))=entry.get("slot") else{return Err("missing source pattern slot".into())};
        if !(0..9).contains(slot)||patterns[*slot as usize].is_some(){return Err("invalid source pattern slot".into())}
        patterns[*slot as usize]=Some(entry.get("item").and_then(save::item_from_nbt).filter(|item|item.count>0).ok_or("invalid source pattern")?);
    }
    let push=match data.other("ae2:exported_push_direction") {
        None=>None,Some(Tag::String(value))=>{let value=value.to_string().map_err(|_|"invalid source direction")?;
            if !["all","down","up","north","south","west","east"].contains(&value.as_str()){return Err("invalid source direction".into())}Some(value)},_=>return Err("invalid source direction".into())
    };
    let name=match data.other("ae2:exported_custom_name") {
        None=>None,Some(Tag::String(value))=>Some(value.to_string().map_err(|_|"invalid source name")?),
        Some(Tag::Compound(_))=>match data.other("ae2:exported_custom_name").and_then(|v|v.get("text")){Some(Tag::String(value))=>Some(value.to_string().map_err(|_|"invalid source name")?),_=>return Err("unsupported structured provider name".into())},
        _=>return Err("invalid source name".into()),
    };
    Ok(Some(Saved{patterns,name,push,settings:read_source_settings(card)?}))
}
fn read_settings(data:&Tag)->Result<Settings,String>{
    // ConfigManager independently ignores an absent or invalid enum value.
    // A malformed typed priority remains an error before any physical commit.
    let flag=|key:&str|match data.get(key){Some(Tag::Byte(0))=>Some(false),Some(Tag::Byte(1))=>Some(true),_=>None};
    let lock=match data.get("lock"){Some(Tag::Int(n)) if (0..=4).contains(n)=>Some(*n),_=>None};
    let priority=match data.get("priority"){None=>None,Some(Tag::Int(n))=>Some(*n),_=>return Err("invalid provider priority".into())};
    Ok(Settings{blocking:flag("blocking"),visible:flag("visible"),lock,priority})
}
fn apply_partial_settings(machine:&mut Machine,settings:&Settings){
    if let Some(value)=settings.blocking{machine.config.insert("ae2_blocking".into(),Tag::Byte(i8::from(value)));}
    if let Some(value)=settings.visible{machine.config.insert("terminal_visible".into(),Tag::Byte(i8::from(value)));}
    if let Some(value)=settings.lock{
        machine.config.insert("ae2_lock_mode".into(),Tag::Int(value));
        machine.config.remove(&"ae2_unlock_event".into());machine.config.remove(&"ae2_unlock_stack".into());
    }
    if let Some(value)=settings.priority{machine.config.insert("priority".into(),Tag::Int(value));}
}
#[cfg(test)]
fn apply_settings(machine:&mut Machine,blocking:bool,visible:bool,lock:i32,priority:i32){
    apply_partial_settings(machine,&Settings{blocking:Some(blocking),visible:Some(visible),lock:Some(lock),priority:Some(priority)});
}
/// Some(bool) means compatible settings applied; bool records whether the
/// source priority setter was invoked. A settings-only import must not refresh
/// the effective provider pattern list or its registration/media cursor.
fn apply_generic_provider(machine:&mut Machine,card:&ItemStack)->Result<Option<bool>,String>{
    if has_source_exports(card) {
        let settings=read_source_settings(card)?;if !settings.any(){return Ok(None)}
        apply_partial_settings(machine,&settings);return Ok(Some(settings.priority.is_some()));
    }
    let Some(data)=card.data().and_then(|d|d.other("minecraft:custom_data")).and_then(|d|d.get(KEY)) else{return Ok(None)};
    if !provider_source(data){return Ok(None)}
    let settings=read_settings(data)?;if !settings.any(){return Ok(None)}
    apply_partial_settings(machine,&settings);Ok(Some(settings.priority.is_some()))
}

#[derive(Clone)]
struct Player {main:Vec<Option<ItemStack>>,offhand:Option<ItemStack>,selected:usize,armor_blanks:u64}
#[derive(Default)]
struct Changes {drops:Vec<ItemStack>,changed:Vec<ItemStack>,missing:u64}
fn merge(slot:&mut Option<ItemStack>,stack:&mut ItemStack,max:u32,changed:&mut Vec<ItemStack>){
    if let Some(existing)=slot.as_mut().filter(|s|s.same_item_same_components(stack)){
        let n=stack.count.min(max.saturating_sub(existing.count));
        if n>0{existing.count+=n;stack.count-=n;changed.push(existing.clone());}
    }
}
fn return_item(player:&mut Player,mut stack:ItemStack,max:u32,changes:&mut Changes){
    // Inventory.getSlotWithRemainingSpace: selected, offhand, then main.
    if let Some(slot)=player.main.get_mut(player.selected){merge(slot,&mut stack,max,&mut changes.changed);}
    merge(&mut player.offhand,&mut stack,max,&mut changes.changed);
    for slot in &mut player.main{merge(slot,&mut stack,max,&mut changes.changed);if stack.count==0{return}}
    for slot in &mut player.main{if slot.is_none(){let n=stack.count.min(max);*slot=Some(copy_count(&stack,n));stack.count-=n;if stack.count==0{return}}}
    if stack.count>0{changes.drops.push(stack)}
}
fn apply_patterns(m:&mut Machine,player:&mut Player,definitions:&[Option<ItemStack>],creative:bool,max:impl Fn(&ItemStack)->u32)->Result<Changes,String>{
    if m.kind!=MachineKind::PatternProvider||m.slots.len()!=9||definitions.len()!=9||player.main.len()!=36||player.selected>=9{return Err("invalid provider transaction".into())}
    let mut changes=Changes::default();
    if creative {m.slots.fill(None);} else {
        let mut blanks=0u32;for slot in &mut m.slots{if let Some(stack)=slot.take(){
            if matches!(stack.item.as_ref(),"ae2:blank_pattern"|"ae2:crafting_pattern"|"ae2:processing_pattern"|"ae2:smithing_table_pattern"|"ae2:stonecutting_pattern"){
                blanks=blanks.checked_add(stack.count).ok_or("blank count overflow")?;
            }else{let limit=max(&stack).max(1);return_item(player,copy_count(&stack,stack.count),limit,&mut changes);}
        }}
        if blanks>0{return_item(player,ItemStack::new("ae2:blank_pattern",blanks),64,&mut changes);}
    }
    // Source countItem includes all inventory compartments and ignores components.
    let available=if creative{u64::MAX}else{
        player.main.iter().chain(std::iter::once(&player.offhand)).flatten().filter(|s|s.item.as_ref()=="ae2:blank_pattern").map(|s|u64::from(s.count)).sum::<u64>()+player.armor_blanks
    };
    let mut wanted=0u64;let mut next=0;for definition in definitions.iter().flatten(){wanted+=1;if wanted<=available{m.slots[next]=Some(copy_count(definition,1));next+=1;}}
    if !creative {
        // Literal Source PlayerInternalInventory only removes default-component
        // blanks from the 36 main slots. Offhand/named-blank nonconsumption is
        // verified by the actual Java oracle, not an invented free-copy rule.
        let filter=ItemStack::new("ae2:blank_pattern",1);let mut remaining=wanted;
        for slot in &mut player.main{if remaining==0{break}let Some(stack)=slot.as_ref().filter(|s|s.same_item_same_components(&filter)) else{continue};
            let n=remaining.min(u64::from(stack.count)) as u32;let left=stack.count-n;*slot=if left==0{None}else{Some(copy_count(stack,left))};remaining-=u64::from(n);
        }
    }
    changes.missing=wanted.saturating_sub(available);Ok(changes)
}

impl Game<'_>{
    fn memory_pattern_definition(&self,p:&ItemStack)->Option<ItemStack>{
        let valid=if let Some((grid,expected))=super::ae2_processing::decode_pattern(p){
            self.res.recipes().find(&grid,3).is_some_and(|(_,actual)|actual==expected)
        }else if let Some(special)=super::ae2_processing::ae2_decode_special_pattern(p){
            super::ae2_processing::special_recipe_result(self.res.stations(),&special.kind,&special.recipe,&special.grid)==Some(special.output)
        }else{super::ae2_processing::ae2_decode_processing_pattern(p).is_some()};
        valid.then(||copy_count(p,1))
    }
    fn provider_memory_snapshot(&self,pos:[i32;3],face:&str)->Result<Option<Snapshot>,String>{
        let key=(pos[0],pos[1],pos[2]);
        let Some(BlockEntity::Ae2(host))=self.world.block_entities.get(&key) else{return Ok(None)};
        let (target,machine,name,name_component,push)=if host.kind==MachineKind::PatternProvider {
            let state=self.pack.assets.registry.state(self.world.block(pos[0],pos[1],pos[2])).ok_or("invalid provider state")?;
            if state.name!="ae2:pattern_provider"{return Ok(None)}
            let name=self.world.block_names.get(&key).cloned();
            let component=super::ae2_memory_host::machine_name_component(host,name.as_deref());
            (Target::Block,(**host).clone(),name,component,state.properties.get("push_direction").cloned().unwrap_or_else(||"all".into()))
        }else if host.kind==MachineKind::CableBus {
            let Some(face_index)=["east","west","up","down","south","north"].iter().position(|f|*f==face) else{return Err("invalid provider face".into())};
            let Some(Tag::List(parts))=host.config.get(&"parts".into()) else{return Ok(None)};
            let mut found=parts.iter().enumerate().filter(|(_,p)|p.get("face")==Some(&Tag::String(face.into())));
            let Some((index,part))=found.next() else{return Ok(None)};
            if found.next().is_some(){return Err("duplicate provider face".into())}
            if part.get("id")!=Some(&Tag::String(PART_SOURCE.into())){return Ok(None)}
            super::ae2_channels::part_identity(part).ok_or("invalid provider part identity")?;
            let stack=part.get("stack").and_then(save::item_from_nbt).filter(|s|s.item.as_ref()==PART_SOURCE&&s.count==1).ok_or("invalid provider part stack")?;
            let address=super::ae2_parts::PartAddress{host:self.ae2_local_address(key).ok_or("invalid host address")?,face:face_index as u8};
            let machine=self.ae2_part_machine(address)?.ok_or("missing provider part")?;
            let name=stack.custom_name();
            let component=super::ae2_memory_host::stack_name_component(&stack);
            (Target::Part{index,face:face_index as u8},machine,name,component,"all".into())
        }else{return Ok(None)};
        if machine.kind!=MachineKind::PatternProvider||machine.slots.len()!=9{return Err("invalid provider inventory".into())}
        Ok(Some(Snapshot{target,host:(**host).clone(),machine,incarnation:self.world.block_entities.incarnation(&key).ok_or("missing provider identity")?,name,name_component,push}))
    }
    fn provider_memory_commit(&mut self,pos:[i32;3],before:&Snapshot,machine:Machine,name:Option<Option<Tag>>,refresh:Option<Refresh>)->Result<(),String>{
        let key=(pos[0],pos[1],pos[2]);
        if self.world.block_entities.incarnation(&key)!=Some(before.incarnation){return Err("provider identity changed".into())}
        let mut next=match before.target {
            Target::Block=>machine,
            Target::Part{index,face}=>{
                let mut host=before.host.clone();
                let Some(Tag::List(parts))=host.config.get_mut(&"parts".into()) else{return Err("missing provider part list".into())};
                let part=parts.get_mut(index).ok_or("missing provider part")?;
                let expected_face=["east","west","up","down","south","north"][usize::from(face)];
                if part.get("face")!=Some(&Tag::String(expected_face.into())){return Err("provider face changed".into())}
                let Tag::Compound(part)=part else{return Err("invalid provider part".into())};
                part.insert("machine".into(),Tag::compound([
                    ("slots",save::encode_items(&machine.slots)),("upgrades",save::encode_items(&machine.upgrades)),("config",Tag::Compound(machine.config))]));
                if let Some(name)=name.as_ref(){
                    let mut stack=part.get(&"stack".into()).and_then(save::item_from_nbt).ok_or("invalid provider stack")?;
                    stack.set_custom_name_component(name.clone());
                    part.insert("stack".into(),save::item_to_nbt(&stack));
                }
                host
            }
        };
        if matches!(before.target,Target::Block){if let Some(name)=name.as_ref(){super::ae2_memory_host::set_machine_name_component(&mut next,name.as_ref());}}
        let Some(BlockEntity::Ae2(current))=self.world.block_entities.get_mut(&key) else{return Err("provider disappeared".into())};
        if **current!=before.host{return Err("provider changed during import".into())}
        std::mem::swap(&mut **current,&mut next);
        if matches!(before.target,Target::Block){if let Some(name)=name{match name{Some(name)=>{self.world.block_names.insert(key,horizon_client::entity_nbt::name_text(&name).plain());},None=>{self.world.block_names.remove(&key);}}}}
        self.world.modified.insert((pos[0]>>4,pos[2]>>4));
        // Source full pattern imports and generic priority setters request a
        // remove/add refresh even when the final priority value is unchanged.
        let face=match before.target{Target::Block=>None,Target::Part{face,..}=>Some(face)};
        match refresh{Some(Refresh::Patterns)=>self.ae2_provider_patterns_changed_local(key,face),Some(Refresh::Priority)=>self.ae2_provider_priority_setter_local(key,face),None=>{}}
        Ok(())
    }
    pub(super) fn ae2_try_provider_memory(&mut self,pos:[i32;3],face:&str)->bool{
        let Some(card)=self.held().filter(|s|s.item.as_ref()=="ae2:memory_card"&&s.count==1).cloned() else{return false};
        let result=match self.provider_memory_snapshot(pos,face){Ok(None)=>return false,Ok(Some(before))=>self.provider_memory_action(pos,before,card),Err(error)=>Err(error)};
        if let Err(error)=result{self.system_message(horizon_client::text::Text::literal(format!("Memory Card: {error}")));}true
    }
    fn provider_memory_action(&mut self,pos:[i32;3],before:Snapshot,mut card:ItemStack)->Result<(),String>{
        if self.keys.contains(&super::KeyCode::ShiftLeft){
            let (source,push)=match before.target{Target::Block=>(BLOCK_SOURCE,Some(before.push.as_str())),Target::Part{..}=>(PART_SOURCE,None)};
            save_card(&mut card,&before.machine,before.name.as_deref(),source,push)?;
            super::ae2_memory_host::publish_memory_name(&mut card,before.name_component.clone());
            horizon_client::ae2_stack_identity::synchronize_tracked(&card);*self.held_mut()=Some(card);
            self.system_message(horizon_client::text::Text::literal("Settings saved"));return Ok(());
        }
        let plain_card=super::ae2_memory_host::memory_card_with_plain_name(&card);
        let Some(saved)=read_card(&plain_card)? else{
            // Java AEBasePart normalizes a provider's target description to the
            // block, but exports the part item's own description. Part-saved
            // cards therefore import generic settings only, even on a part.
            let mut machine=before.machine.clone();
            let refresh=match apply_generic_provider(&mut machine,&card)? {
                Some(priority_set)=>priority_set.then_some(Refresh::Priority),
                None=>{
                    let priority=match super::ae2_memory::interface_priority_from_card(&card)? {
                        Some(priority)=>Some(priority),
                        None=>match super::ae2_memory::storage_priority_from_card(&card)? {
                            Some(priority)=>Some(priority),
                            None=>super::ae2_memory::bus_priority_from_card(&card)?,
                        },
                    };
                    let Some(priority)=priority else{return Err("Invalid machine".into())};
                    machine.config.insert("priority".into(),Tag::Int(priority));Some(Refresh::Priority)
                }
            };
            self.provider_memory_commit(pos,&before,machine,None,refresh)?;
            self.system_message(horizon_client::text::Text::literal("Compatible settings restored"));return Ok(());
        };
        let definitions=saved.patterns.iter().map(|p|p.as_ref().and_then(|p|self.memory_pattern_definition(p))).collect::<Vec<_>>();
        let mut machine=before.machine.clone();let mut player=Player{main:self.inventory.clone(),offhand:self.offhand.clone(),selected:self.selected,armor_blanks:self.armor.iter().flatten().filter(|s|s.item.as_ref()=="ae2:blank_pattern").map(|s|u64::from(s.count)).sum()};
        let changes=apply_patterns(&mut machine,&mut player,&definitions,self.creative,|s|self.res.items().stack_max(s))?;
        apply_partial_settings(&mut machine,&saved.settings);
        if matches!(before.target,Target::Block)&&saved.push.is_some(){
            let old_state=self.world.block(pos[0],pos[1],pos[2]);
            let mut state=self.pack.assets.registry.state(old_state).ok_or("invalid provider state")?.clone();
            state.properties.insert("push_direction".into(),saved.push.unwrap());
            let next_state=self.pack.assets.registry.resolve(&state).map_err(|e|e.to_string())?;
            if next_state!=old_state{self.level_set(pos,next_state).map_err(|e|e.to_string())?;}
        }
        let name=super::ae2_memory_host::memory_card_name_component(&card,saved.name.as_deref());
        self.provider_memory_commit(pos,&before,machine,Some(name),Some(Refresh::Patterns))?;
        self.inventory=player.main;self.offhand=player.offhand;
        for item in changes.changed{horizon_client::ae2_stack_identity::synchronize_tracked(&item);}
        for item in changes.drops{self.player_drop(item);}
        if changes.missing>0{self.system_message(horizon_client::text::Text::literal(format!("Missing {} blank patterns",changes.missing)));}
        self.system_message(horizon_client::text::Text::literal("Settings loaded"));Ok(())
    }
}

#[cfg(test)]mod tests{
    use super::*;
    fn player()->Player{Player{main:vec![None;36],offhand:None,selected:0,armor_blanks:0}}
    fn definitions()->Vec<Option<ItemStack>>{let mut v=vec![None;9];v[3]=Some(ItemStack::new("ae2:crafting_pattern",1));v[8]=Some(ItemStack::new("ae2:processing_pattern",1));v}
    #[test]fn source_provider_components_restore_patterns_with_real_blanks_and_partial_priority(){
        let mut source=Machine::new(MachineKind::PatternProvider);source.slots=definitions();
        apply_settings(&mut source,true,false,3,42);
        let mut card=ItemStack::new("ae2:memory_card",1);save_card(&mut card,&source,Some("provider"),BLOCK_SOURCE,Some("north")).unwrap();
        card.data_mut().set_other("minecraft:custom_data",None);
        let saved=read_card(&card).unwrap().unwrap();
        assert_eq!(saved.patterns,source.slots);assert_eq!(saved.push.as_deref(),Some("north"));
        assert_eq!(saved.name.as_deref(),Some("provider"));assert_eq!(saved.settings.lock,Some(3));
        let mut target=Machine::new(MachineKind::PatternProvider);let mut owner=player();owner.main[0]=Some(ItemStack::new("ae2:blank_pattern",2));
        let changes=apply_patterns(&mut target,&mut owner,&saved.patterns,false,|_|64).unwrap();
        apply_partial_settings(&mut target,&saved.settings);
        assert_eq!(target.slots[0],source.slots[3]);assert_eq!(target.slots[1],source.slots[8]);assert_eq!(changes.missing,0);assert!(owner.main.iter().all(Option::is_none));
        let mut part=ItemStack::new("ae2:memory_card",1);
        part.data_mut().set_other("ae2:exported_settings_source",Some(Tag::compound([("translate",Tag::String("item.ae2.cable_pattern_provider".into()))])));
        part.data_mut().set_other("ae2:exported_priority",Some(Tag::Int(7)));
        part.data_mut().set_other("ae2:exported_patterns",Some(Tag::List(Vec::new())));
        let existing=target.slots.clone();assert!(read_card(&part).unwrap().is_none());assert_eq!(apply_generic_provider(&mut target,&part).unwrap(),Some(true));
        assert_eq!(target.slots,existing);assert_eq!(integer(&target,"priority",0).unwrap(),7);
        let mut styled=card.clone();
        styled.data_mut().set_other("ae2:exported_settings_source",Some(Tag::compound([
            ("translate",Tag::String("block.ae2.pattern_provider".into())),("color",Tag::String("red".into())),
        ])));
        styled.data_mut().set_other("ae2:exported_patterns",Some(Tag::List(Vec::new())));
        assert!(read_card(&styled).unwrap().is_none());
        assert_eq!(apply_generic_provider(&mut target,&styled).unwrap(),Some(true));
        assert_eq!(target.slots,existing,"foreign styled description cannot clear physical patterns");
    }
    #[test]fn provider_part_card_uses_generic_settings_without_touching_patterns_or_name(){
        let mut source=Machine::new(MachineKind::PatternProvider);
        source.slots[4]=Some(ItemStack::new("ae2:crafting_pattern",1));
        apply_settings(&mut source,true,false,3,42);
        let mut card=ItemStack::new("ae2:memory_card",1);
        save_card(&mut card,&source,Some("Source part"),PART_SOURCE,None).unwrap();
        assert!(read_card(&card).unwrap().is_none());assert_eq!(priority_from_card(&card).unwrap(),Some(42));
        let mut target=Machine::new(MachineKind::PatternProvider);target.slots[8]=Some(ItemStack::new("ae2:processing_pattern",1));target.energy=123.;
        target.config.insert("in_flight_marker".into(),Tag::Int(9));
        let slots=target.slots.clone();assert_eq!(apply_generic_provider(&mut target,&card).unwrap(),Some(true));
        assert_eq!(target.slots,slots);assert_eq!(target.energy,123.);assert_eq!(integer(&target,"priority",0).unwrap(),42);
        assert_eq!(target.config.get(&"in_flight_marker".into()),Some(&Tag::Int(9)));
        // Full block export has the matching target description and push data;
        // a part export has neither a matching description nor push data.
        let data=card.data().unwrap().other("minecraft:custom_data").unwrap().get(KEY).unwrap();
        assert!(data.get("push").is_none());
        save_card(&mut card,&source,None,BLOCK_SOURCE,Some("south")).unwrap();
        let full=read_card(&card).unwrap().unwrap();assert_eq!(full.push,Some("south".into()));assert!(full.name.is_none());
    }
    #[test]fn partial_components_match_actual_java_full_and_generic_imports(){
        let card=|source:&str,fields:Vec<(&str,Tag)>|{
            let mut data=Compound::new();data.insert("source".into(),Tag::String(source.into()));
            for (key,value) in fields{data.insert(key.into(),value);}
            let mut card=ItemStack::new("ae2:memory_card",1);card.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([(KEY,Tag::Compound(data))])));card
        };
        let mut original=Machine::new(MachineKind::PatternProvider);original.slots[0]=Some(ItemStack::new("ae2:processing_pattern",1));
        apply_settings(&mut original,true,false,2,17);original.config.insert("ae2_unlock_event".into(),Tag::Int(8));
        let full=read_card(&card(BLOCK_SOURCE,vec![("priority",Tag::Int(42))])).unwrap().unwrap();
        assert!(full.name.is_none()&&full.push.is_none());assert!(full.patterns.iter().all(Option::is_none));
        let mut machine=original.clone();let mut owner=player();let changes=apply_patterns(&mut machine,&mut owner,&full.patterns,false,|_|64).unwrap();
        apply_partial_settings(&mut machine,&full.settings);assert_eq!(owner.main[0],Some(ItemStack::new("ae2:blank_pattern",1)));assert!(changes.drops.is_empty());
        assert!(machine.slots.iter().all(Option::is_none));assert_eq!(integer(&machine,"priority",0).unwrap(),42);
        assert_eq!(machine.config.get(&"ae2_unlock_event".into()),Some(&Tag::Int(8)));assert!(boolean(&machine,"ae2_blocking",false).unwrap());
        for (fields,refresh,priority,blocking) in [
            (vec![("priority",Tag::Int(42))],true,42,true),
            (vec![("blocking",Tag::Byte(0))],false,17,false),
            (vec![("blocking",Tag::Byte(2)),("lock",Tag::Int(99)),("priority",Tag::Int(42))],true,42,true),
        ]{
            let mut machine=original.clone();assert_eq!(apply_generic_provider(&mut machine,&card(PART_SOURCE,fields)).unwrap(),Some(refresh));
            assert_eq!(machine.slots,original.slots);assert_eq!(integer(&machine,"priority",0).unwrap(),priority);
            assert_eq!(boolean(&machine,"ae2_blocking",false).unwrap(),blocking);assert_eq!(integer(&machine,"ae2_lock_mode",0).unwrap(),2);
            assert_eq!(machine.config.get(&"ae2_unlock_event".into()),Some(&Tag::Int(8)));
        }
    }
    #[test]fn provider_reconstruction_matches_actual_java_blank_ownership_cases(){
        let defs=definitions();let mut m=Machine::new(MachineKind::PatternProvider);m.energy=37.;m.config.insert("send_list_marker".into(),Tag::Int(9));
        m.slots[0]=Some(ItemStack::new("ae2:crafting_pattern",1));m.slots[2]=Some(ItemStack::new("ae2:blank_pattern",2));m.slots[5]=Some(ItemStack::new("minecraft:stone",3));
        let mut p=player();let c=apply_patterns(&mut m,&mut p,&defs,false,|_|64).unwrap();assert!(c.drops.is_empty());assert_eq!(c.missing,0);assert_eq!(m.slots[0].as_ref().unwrap().item.as_ref(),"ae2:crafting_pattern");assert_eq!(m.slots[1].as_ref().unwrap().item.as_ref(),"ae2:processing_pattern");assert_eq!(p.main.iter().flatten().filter(|s|s.item.as_ref()=="ae2:blank_pattern").map(|s|s.count).sum::<u32>(),1);assert_eq!(m.energy,37.);assert_eq!(m.config.get(&"send_list_marker".into()),Some(&Tag::Int(9)));
        let mut m=Machine::new(MachineKind::PatternProvider);m.slots[0]=Some(ItemStack::new("ae2:crafting_pattern",1));let mut p=player();p.main.fill(Some(ItemStack::new("minecraft:stone",64)));let c=apply_patterns(&mut m,&mut p,&defs,false,|_|64).unwrap();assert_eq!(c.drops.len(),1);assert_eq!(c.drops[0],ItemStack::new("ae2:blank_pattern",1));assert_eq!(c.missing,2);assert!(m.slots.iter().all(Option::is_none));
        for offhand in [false,true]{let mut m=Machine::new(MachineKind::PatternProvider);let mut p=player();let mut blanks=ItemStack::new("ae2:blank_pattern",2);if offhand{p.offhand=Some(blanks.clone())}else{blanks.data_mut().custom_name=Some(horizon_client::item_data::text_name("Marked blank"));p.main[1]=Some(blanks.clone())}let c=apply_patterns(&mut m,&mut p,&defs,false,|_|64).unwrap();assert_eq!(m.slots.iter().flatten().count(),2);assert_eq!(c.missing,0);assert_eq!(if offhand{p.offhand.as_ref()}else{p.main[1].as_ref()},Some(&blanks));}
        let mut m=Machine::new(MachineKind::PatternProvider);let mut p=player();p.main[1]=Some(ItemStack::new("ae2:blank_pattern",1));let c=apply_patterns(&mut m,&mut p,&defs,false,|_|64).unwrap();assert_eq!(m.slots.iter().flatten().count(),1);assert_eq!(c.missing,1);assert!(p.main[1].is_none());
        let mut m=Machine::new(MachineKind::PatternProvider);let mut p=player();let c=apply_patterns(&mut m,&mut p,&defs,true,|_|64).unwrap();assert_eq!(m.slots.iter().flatten().count(),2);assert!(c.drops.is_empty());assert_eq!(c.missing,0);
    }
}
