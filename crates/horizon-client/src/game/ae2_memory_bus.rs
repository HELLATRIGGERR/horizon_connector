// Standard Memory Card components for the four face-owned bus/plane parts.
mod bus_memory {
    use super::*;
    use ae2_machines_core::{Key, Redstone, Schedule};
    use super::super::{ae2_buses, ae2_lab, ae2_fluids};

    const KINDS: &[&str] = &["ae2:import_bus", "ae2:export_bus", "ae2:storage_bus", "ae2:formation_plane"];
    const FUZZY: &str = "ae2_fuzzy_mode";
    const FUZZY_VALUES: &[&str] = &["ignore_all","percent_99","percent_75","percent_50","percent_25"];

    #[derive(Clone,Debug)]
    pub(in crate::game) struct SavedBus {
        pub matching: bool,
        pub settings: Compound,
        pub priority: Option<i32>,
        pub ghosts: Option<Vec<Option<Key>>>,
        pub desired: Option<Vec<ItemStack>>,
        pub name: Option<String>,
    }

    fn text(data:&Compound,key:&str,default:&str)->Result<String,String> {
        match data.get(&key.into()) {
            None=>Ok(default.to_owned()),
            Some(Tag::String(v))=>v.to_string().map_err(|_|format!("invalid {key}")),
            _=>Err(format!("invalid {key}")),
        }
    }
    fn put(settings:&mut Compound,key:&str,value:&str) {
        settings.insert(key.into(),Tag::String(value.into()));
    }
    fn redstone(value:Redstone)->&'static str {
        match value {Redstone::Ignore=>"ignore",Redstone::Low=>"low_signal",Redstone::High=>"high_signal",Redstone::Pulse=>"signal_pulse"}
    }
    fn schedule(value:Schedule)->&'static str {
        match value {Schedule::Default=>"default",Schedule::RoundRobin=>"roundrobin",Schedule::Random=>"random"}
    }
    fn flag(data:&Compound,key:&str,default:bool)->bool {
        match data.get(&key.into()){Some(Tag::Byte(v))=>*v!=0,_=>default}
    }
    fn setting(settings:&Compound,key:&str,allowed:&[&str])->Result<Option<String>,String>{
        match settings.get(&key.into()) {
            None=>Ok(None),
            Some(Tag::String(v))=>{let value=v.to_string().map_err(|_|"invalid setting")?;Ok(allowed.contains(&value.as_str()).then_some(value))},
            _=>Err("invalid setting type".into()),
        }
    }
    fn encode_ghosts(ghosts:&[Option<Key>])->Result<(Tag,Tag),String>{
        let mut items=vec![None;63];let mut fluids=Vec::new();
        for (i,key) in ghosts.iter().take(63).enumerate() {
            let Some(key)=key else{continue};
            let raw=ae2_cell_core::ItemKey::from(key);
            match ae2_lab::prototype(&raw) {
                Ok(mut stack)=>{stack.count=1;items[i]=Some(stack)},
                Err(_) if ae2_fluids::valid_fluid_id(&key.id)=>fluids.push(Tag::compound([
                    ("Slot",Tag::Byte(i as i8)),("fluid",Tag::String(key.id.as_str().into())),
                    ("components",Tag::ByteArray(key.components.clone())),("amount",Tag::Long(1000))
                ])),
                Err(_)=>return Err("invalid bus filter".into()),
            }
        }
        Ok((save::encode_items(&items),Tag::List(fluids)))
    }
    fn decode_ghosts(items:&Tag,fluids:&Tag)->Result<Vec<Option<Key>>,String>{
        let (Tag::List(item_entries),Tag::List(fluid_entries))=(items,fluids) else{return Err("invalid bus filters".into())};
        let mut out=vec![None;63];
        for tag in item_entries {
            let Some(Tag::Byte(slot))=tag.get("Slot") else{return Err("invalid bus filter slot".into())};
            let i=*slot as usize;
            let stack=save::item_from_nbt(tag).filter(|s|s.count>0).ok_or("invalid item filter")?;
            if i>=63 || out[i].is_some(){return Err("duplicate or invalid bus filter".into())}
            out[i]=Some(Key::from(&ae2_lab::key(&stack)?));
        }
        for tag in fluid_entries {
            let (Some(Tag::Byte(slot)),Some(Tag::String(id)),Some(Tag::ByteArray(components)),Some(Tag::Long(amount)))=
                (tag.get("Slot"),tag.get("fluid"),tag.get("components"),tag.get("amount"))
                else{return Err("invalid fluid filter".into())};
            let i=*slot as usize;let id=id.to_string().map_err(|_|"invalid fluid id")?;
            if i>=63 || out[i].is_some() || *amount<=0 || !ae2_fluids::valid_fluid_id(&id){return Err("duplicate or invalid fluid filter".into())}
            out[i]=Some(Key{id,components:components.clone()});
        }
        Ok(out)
    }
    pub(in crate::game) fn save_bus(card:&mut ItemStack,part:&Compound,name:Option<&str>)->Result<(),String>{
        if card.item.as_ref()!="ae2:memory_card"||card.count!=1{return Err("invalid card".into())}
        let id=text(part,"id","")?;if !KINDS.contains(&id.as_str()){return Err("invalid bus".into())}
        let state=ae2_buses::bus_state(part)?;
        let mut settings=Compound::new();
        put(&mut settings,FUZZY,&text(part,"fuzzy_mode","ignore_all")?);
        if matches!(id.as_str(),"ae2:import_bus"|"ae2:export_bus"){put(&mut settings,"redstone_controlled",redstone(state.redstone));}
        if id=="ae2:export_bus" {
            put(&mut settings,"scheduling_mode",schedule(state.schedule));
            put(&mut settings,"craft_only",if flag(part,"craft_only",false){"yes"}else{"no"});
        }
        if id=="ae2:storage_bus" {
            let access=text(part,"access","read_write")?;
            put(&mut settings,"access",match access.as_str(){"read_only"=>"read","write_only"=>"write",_=>"read_write"});
            put(&mut settings,"filter_on_extract",if flag(part,"filter_on_extract",true){"yes"}else{"no"});
            put(&mut settings,"storage_filter",&text(part,"storage_filter","extractable_only")?);
        }
        if id=="ae2:formation_plane" {put(&mut settings,"place_block",if flag(part,"formation_entity",false){"no"}else{"yes"});}
        let source_filters=state.config.iter().map(|entry|{
            let Some(key)=entry else{return Ok(Tag::Compound(Compound::new()))};
            let raw=ae2_cell_core::ItemKey::from(key);
            let fluid=ae2_lab::prototype(&raw).is_err();
            horizon_client::ae2_generic_stack::encode(&horizon_client::ae2_generic_stack::GenericStack{key:raw,fluid,amount:0})
        }).collect::<Result<Vec<_>,String>>()?;
        let mut source_settings=Compound::new();
        for (key,value) in &settings {
            let Tag::String(value)=value else{return Err("invalid bus setting".into())};
            let key=key.to_string_lossy();let key=if key==FUZZY{"fuzzy_mode"}else{key.as_str()};
            source_settings.insert(key.into(),Tag::String(value.to_string().map_err(|_|"invalid setting")?.to_ascii_uppercase().as_str().into()));
        }
        let (items,fluids)=encode_ghosts(&state.config)?;
        let mut body=Compound::new();body.insert("source".into(),Tag::String(id.as_str().into()));
        body.insert("settings".into(),Tag::Compound(settings));
        let mut groups:Vec<ItemStack>=Vec::new();
        for stack in ae2_buses::bus_upgrade_slots(part)?.iter().flatten(){
            if let Some(group)=groups.iter_mut().find(|v|v.same_item_same_components(stack)){group.count+=stack.count;}
            else{groups.push(stack.clone());}
        }
        body.insert("upgrades".into(),Tag::List(groups.iter().map(save::item_to_nbt).collect()));
        body.insert("config_items".into(),items);body.insert("config_fluids".into(),fluids);
        if matches!(id.as_str(),"ae2:storage_bus"|"ae2:formation_plane") {
            let priority=match part.get(&"priority".into()){None=>0,Some(Tag::Int(v))=>*v,_=>return Err("invalid bus priority".into())};
            body.insert("priority".into(),Tag::Int(priority));
        }
        if let Some(name)=name{body.insert("name".into(),Tag::String(name.into()));}
        let source_priority=body.get(&"priority".into()).cloned();
        let mut custom=match card.data().and_then(|d|d.other("minecraft:custom_data")){None=>Compound::new(),Some(Tag::Compound(v))=>v.clone(),_=>return Err("invalid card data".into())};
        custom.remove(&P2P.into());custom.insert(KEY.into(),Tag::Compound(body));card.data_mut().set_other("minecraft:custom_data",Some(Tag::Compound(custom)));
        let description=format!("item.ae2.{}",id.strip_prefix("ae2:").unwrap_or(&id));
        card.data_mut().set_other("ae2:exported_settings_source",Some(Tag::compound([("translate",Tag::String(description.as_str().into()))])));
        card.data_mut().set_other("ae2:exported_settings",Some(Tag::Compound(source_settings)));
        card.data_mut().set_other("ae2:exported_upgrades",Some(Tag::List(groups.iter().map(save::item_to_nbt).collect())));
        card.data_mut().set_other("ae2:exported_config_inv",Some(Tag::List(source_filters)));
        card.data_mut().set_other("ae2:exported_priority",source_priority);
        card.data_mut().set_other("ae2:exported_custom_name",name.map(|value|Tag::String(value.into())));
        Ok(())
    }
    pub(in crate::game) fn read_bus(card:&ItemStack,target:&str)->Result<Option<SavedBus>,String>{
        if card.item.as_ref()!="ae2:memory_card"||card.count!=1||!KINDS.contains(&target){return Err("invalid card or bus".into())}
        if let Some(data)=card.data(){
            let ids=["ae2:exported_settings_source","ae2:exported_settings","ae2:exported_upgrades","ae2:exported_config_inv","ae2:exported_priority","ae2:exported_custom_name"];
            if ids.iter().any(|id|data.other(id).is_some()){
                let expected=format!("item.ae2.{}",target.strip_prefix("ae2:").unwrap_or(target));
                let matching=source_description_matches(data.other("ae2:exported_settings_source"),&expected);
                let mut body=Compound::new();body.insert("source".into(),Tag::String(if matching{target}else{"horizon:generic"}.into()));
                if let Some(settings)=data.other("ae2:exported_settings"){
                    let Tag::Compound(settings)=settings else{return Err("invalid settings".into())};
                    let mut mapped=Compound::new();
                    for (key,value) in settings {
                        let Tag::String(value)=value else{return Err("invalid setting type".into())};
                        let value=value.to_string().map_err(|_|"invalid setting")?;let lower=value.to_ascii_lowercase();
                        if value!=lower.to_ascii_uppercase(){continue}
                        let key=key.to_string_lossy();put(&mut mapped,if key=="fuzzy_mode"{FUZZY}else{key.as_str()},&lower);
                    }
                    body.insert("settings".into(),Tag::Compound(mapped));
                }
                if let Some(upgrades)=data.other("ae2:exported_upgrades"){body.insert("upgrades".into(),upgrades.clone());}
                if let Some(priority)=data.other("ae2:exported_priority"){
                    body.insert("priority".into(),horizon_client::ae2_components::normalize("ae2:exported_priority",priority.clone())?);
                }
                if let Some(config)=data.other("ae2:exported_config_inv"){
                    let Tag::List(entries)=config else{return Err("invalid exported config".into())};
                    let mut ghosts=vec![None;63];
                    for (i,tag) in entries.iter().take(63).enumerate(){
                        ghosts[i]=horizon_client::ae2_generic_stack::decode_tolerant(tag)?.map(|value|Key::from(&value.key));
                    }
                    let (items,fluids)=encode_ghosts(&ghosts)?;body.insert("config_items".into(),items);body.insert("config_fluids".into(),fluids);
                }
                if matching {if let Some(name)=data.other("ae2:exported_custom_name"){
                    let name=match name{Tag::String(value)=>value.clone(),Tag::Compound(_)=>match name.get("text"){Some(Tag::String(value))=>value.clone(),_=>return Err("unsupported structured bus name".into())},_=>return Err("invalid name".into())};
                    body.insert("name".into(),Tag::String(name));
                }}
                let mut proxy=card.clone();proxy.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([(KEY,Tag::Compound(body))])));
                return read_bus_legacy(&proxy,target);
            }
        }
        read_bus_legacy(card,target)
    }
    fn read_bus_legacy(card:&ItemStack,target:&str)->Result<Option<SavedBus>,String>{
        let Some(custom)=card.data().and_then(|d|d.other("minecraft:custom_data"))else{return Ok(None)};
        let Some(body)=custom.get(KEY)else{return Ok(None)};if !matches!(body,Tag::Compound(_)){return Err("invalid card body".into())}
        let matching=body.get("source")==Some(&Tag::String(target.into()));
        let input=match body.get("settings"){None=>Compound::new(),Some(Tag::Compound(v))=>v.clone(),_=>return Err("invalid settings".into())};
        let mut settings=Compound::new();
        let mut copy=|key:&str,allowed:&[&str]|->Result<(),String>{if let Some(v)=setting(&input,key,allowed)?{put(&mut settings,key,&v);}Ok(())};
        copy(FUZZY,FUZZY_VALUES)?;
        if matches!(target,"ae2:import_bus"|"ae2:export_bus"){copy("redstone_controlled",&["ignore","low_signal","high_signal","signal_pulse"])?;}
        if target=="ae2:export_bus"{copy("scheduling_mode",&["default","roundrobin","random"])?;copy("craft_only",&["yes","no"])?;}
        if target=="ae2:storage_bus"{copy("access",&["read_write","read","write"])?;copy("filter_on_extract",&["yes","no"])?;copy("storage_filter",&["extractable_only","none"])?;}
        if target=="ae2:formation_plane"{copy("place_block",&["yes","no"])?;}
        drop(copy);
        // Existing IO Port cards encode the same source Redstone setting under
        // its native machine key; translate it at this generic boundary.
        if matches!(target,"ae2:import_bus"|"ae2:export_bus") && body.get("source")==Some(&Tag::String("ae2:io_port".into())) {
            if let Some(v)=setting(&input,"io_redstone",&["ignore","low","high"])? {
                put(&mut settings,"redstone_controlled",match v.as_str(){"low"=>"low_signal","high"=>"high_signal",_=>"ignore"});
            }
        }
        let priority=if matches!(target,"ae2:storage_bus"|"ae2:formation_plane") {match body.get("priority"){None=>None,Some(Tag::Int(v))=>Some(*v),_=>return Err("invalid priority".into())}}else{None};
        let ghosts=match(body.get("config_items"),body.get("config_fluids")){
            (None,None)=>None,(Some(items),Some(fluids))=>Some(decode_ghosts(items,fluids)?),_=>return Err("incomplete config inventory".into())
        };
        let desired=match body.get("upgrades"){
            None=>None,Some(Tag::List(entries))=>{
                let mut out=Vec::new();for entry in entries {
                    let s=save::item_from_nbt(entry).filter(|s|s.count>0).ok_or("invalid upgrade")?;
                    out.push(s);
                }Some(out)
            },_=>return Err("invalid upgrades".into())
        };
        let name=if matching {match body.get("name"){None=>None,Some(Tag::String(v))=>Some(v.to_string().map_err(|_|"invalid name")?),_=>return Err("invalid name".into())}}else{None};
        Ok(Some(SavedBus{matching,settings,priority,ghosts,desired,name}))
    }
    pub(in crate::game) fn bus_has_components(saved:&SavedBus)->bool{
        !saved.settings.is_empty()||saved.priority.is_some()||saved.ghosts.is_some()||saved.desired.is_some()
    }
    fn can_insert(part:&Compound,slots:&[Option<ItemStack>],card:&ItemStack)->bool{
        let mut trial=slots.to_vec();let Some(empty)=trial.iter_mut().find(|slot|slot.is_none())else{return false};
        *empty=Some(moved_copy(card,1));let mut part=part.clone();
        ae2_buses::store_bus_upgrade_slots(&mut part,&trial).is_ok()&&ae2_buses::bus_state(&part).is_ok()
    }
    pub(in crate::game) fn apply_bus(part:&mut Compound,player:&mut Vec<Option<ItemStack>>,pocket:&mut Option<horizon_client::ae2_network_tool::Inventory>,saved:&SavedBus,creative:bool)->Result<PhysicalChanges,String>{
        if player.len()!=36{return Err("invalid player inventory".into())}
        let id=text(part,"id","")?;if !KINDS.contains(&id.as_str()){return Err("invalid bus".into())}
        let mut state=ae2_buses::bus_state(part)?;let mut next=part.clone();let mut inv=player.clone();let mut tool=pocket.clone();
        let mut changes=PhysicalChanges{drops:Vec::new(),changed_tracked:Vec::new(),replaced_tracked:Vec::new(),missing:0};
        if let Some(desired)=saved.desired.as_ref(){
            let mut slots=ae2_buses::bus_upgrade_slots(&next)?;
            if creative {
                for old in slots.iter_mut(){if let Some(stack)=old.take(){changes.replaced_tracked.push(stack)}}
                for wanted in desired {for _ in 0..wanted.count.min(5) {
                    if !can_insert(&next,&slots,wanted){break}
                    if let Some(slot)=slots.iter_mut().find(|slot|slot.is_none()){*slot=Some(moved_copy(wanted,1));}
                }}
            }
            let removed_crafting=slots.iter().flatten().any(|s|s.item.as_ref()=="ae2:crafting_card");
            for slot in &mut slots {if let Some(mut removed)=slot.take(){
                changes.replaced_tracked.push(removed.clone());horizon_client::ae2_stack_identity::detach_if_tracked(&mut removed);
                add_to_player(&mut inv,&mut removed,64,&mut changes.replaced_tracked);
                if removed.count>0{if let Some(tool)=tool.as_mut(){add_to_player(&mut tool.slots,&mut removed,64,&mut changes.replaced_tracked);}}
                if removed.count>0{changes.drops.push(removed);}
            }}
            if id=="ae2:export_bus" && (removed_crafting||creative) {ae2_buses::mark_export_card_removed(&mut next);}
            // Java keys desired counts by item, ignoring exported components.
            let mut requests:Vec<(String,u32)>=Vec::new();
            for wanted in desired {if let Some((_,count))=requests.iter_mut().find(|(id,_)|id==wanted.item.as_ref()){*count=wanted.count}else{requests.push((wanted.item.to_string(),wanted.count));}}
            for (id,count) in requests {
                let filter=ItemStack::new(&id,1);
                let mut capacity=0u32;let mut trial=slots.clone();
                for _ in 0..count.min(5){if !can_insert(&next,&trial,&filter){break}if let Some(slot)=trial.iter_mut().find(|s|s.is_none()){*slot=Some(filter.clone());capacity+=1;}}
                if capacity==0{continue}
                let mut found=take_from_slots(&mut inv,&filter,capacity,&mut changes,false);
                let have:u32=found.iter().map(|s|s.count).sum();
                if have<capacity {if let Some(tool)=tool.as_mut(){found.extend(take_from_slots(&mut tool.slots,&filter,capacity-have,&mut changes,true));}}
                let have:u32=found.iter().map(|s|s.count).sum();changes.missing+=capacity.saturating_sub(have);
                for stack in found {for _ in 0..stack.count {if let Some(slot)=slots.iter_mut().find(|s|s.is_none()){*slot=Some(moved_copy(&stack,1));}}}
            }
            ae2_buses::store_bus_upgrade_slots(&mut next,&slots)?;
        }
        if let Some(v)=setting(&saved.settings,FUZZY,FUZZY_VALUES)? {next.insert("fuzzy_mode".into(),Tag::String(v.as_str().into()));}
        if matches!(id.as_str(),"ae2:import_bus"|"ae2:export_bus") {
            if let Some(v)=setting(&saved.settings,"redstone_controlled",&["ignore","low_signal","high_signal","signal_pulse"])? {
                let mode=match v.as_str(){"low_signal"=>Redstone::Low,"high_signal"=>Redstone::High,"signal_pulse"=>Redstone::Pulse,_=>Redstone::Ignore};
                let signal=state.last_redstone;state.set_redstone_mode(mode,signal);
            }
        }
        if id=="ae2:export_bus" {
            if let Some(v)=setting(&saved.settings,"scheduling_mode",&["default","roundrobin","random"])? {state.schedule=match v.as_str(){"roundrobin"=>Schedule::RoundRobin,"random"=>Schedule::Random,_=>Schedule::Default};}
            if let Some(v)=setting(&saved.settings,"craft_only",&["yes","no"])? {next.insert("craft_only".into(),Tag::Byte(i8::from(v=="yes")));}
        }
        if id=="ae2:storage_bus" {
            if let Some(v)=setting(&saved.settings,"access",&["read_write","read","write"])? {next.insert("access".into(),Tag::String(match v.as_str(){"read"=>"read_only","write"=>"write_only",_=>"read_write"}.into()));}
            if let Some(v)=setting(&saved.settings,"filter_on_extract",&["yes","no"])? {next.insert("filter_on_extract".into(),Tag::Byte(i8::from(v=="yes")));}
            if let Some(v)=setting(&saved.settings,"storage_filter",&["extractable_only","none"])? {next.insert("storage_filter".into(),Tag::String(v.as_str().into()));}
        }
        if id=="ae2:formation_plane" {
            if let Some(v)=setting(&saved.settings,"place_block",&["yes","no"])? {next.insert("formation_entity".into(),Tag::Byte(i8::from(v=="no")));}
        }
        if let Some(priority)=saved.priority{next.insert("priority".into(),Tag::Int(priority));}
        if let Some(ghosts)=saved.ghosts.as_ref(){state.config=ghosts.clone();}
        if bus_has_components(saved) {
            next.insert("state_json".into(),Tag::String(serde_json::to_string(&state).map_err(|_|"invalid bus state")?.as_str().into()));
            next.insert("bus_next_tick".into(),Tag::Long(0));
        }
        ae2_buses::bus_state(&next)?;
        *part=next;*player=inv;*pocket=tool;Ok(changes)
    }
    pub(in crate::game) fn bus_priority_from_card(card:&ItemStack)->Result<Option<i32>,String>{
        if card.item.as_ref()!="ae2:memory_card"||card.count!=1{return Err("invalid card".into())}
        if bus_source_from_card(card).is_some()&&card.data().and_then(|d|d.other("ae2:exported_settings_source")).is_some(){
            if let Some(value)=card.data().and_then(|d|d.other("ae2:exported_priority")){return match horizon_client::ae2_components::normalize("ae2:exported_priority",value.clone())?{Tag::Int(value)=>Ok(Some(value)),_=>unreachable!()}}
            return Ok(None)
        }
        let Some(data)=card.data().and_then(|d|d.other("minecraft:custom_data"))else{return Ok(None)};
        let Some(body)=data.get(KEY)else{return Ok(None)};
        let is_bus=KINDS.iter().any(|id|body.get("source")==Some(&Tag::String((*id).into())));
        if !is_bus{return Ok(None)}
        match body.get("priority"){None=>Ok(None),Some(Tag::Int(v))=>Ok(Some(*v)),_=>Err("invalid bus priority".into())}
    }
    pub(in crate::game) fn bus_source_from_card(card:&ItemStack)->Option<&'static str>{
        if let Some(source)=card.data().and_then(|d|d.other("ae2:exported_settings_source")){
            return KINDS.iter().copied().find(|id|source_description_matches(Some(source),&format!("item.ae2.{}",id.strip_prefix("ae2:").unwrap_or(id))));
        }
        let body=card.data()?.other("minecraft:custom_data")?.get(KEY)?;
        KINDS.iter().copied().find(|id|body.get("source")==Some(&Tag::String((*id).into())))
    }
    /// Java imports only the target inventory's slots. Truncate the 63-slot
    /// export before handing it to the existing nine-slot Interface decoder.
    pub(in crate::game) fn bus_as_interface(card:&ItemStack)->Result<Option<SavedInterface>,String>{
        if bus_source_from_card(card).is_none(){return Ok(None)}
        if card.data().and_then(|d|d.other("ae2:exported_settings_source")).is_some(){return read_source_interface(card,true)}
        let saved=read_bus(card,"ae2:storage_bus")?.ok_or("missing bus card")?;
        let ghosts=if let Some(mut config)=saved.ghosts {
            for slot in config.iter_mut().skip(9){*slot=None;}
            Some(encode_ghosts(&config)?)
        }else{None};
        let desired=saved.desired.map(|cards|{
            cards.into_iter().find(|s|matches!(s.item.as_ref(),"ae2:fuzzy_card"|"ae2:crafting_card"))
                .map(|s|moved_copy(&s,1))
        });
        Ok(Some(SavedInterface{
            fuzzy:setting(&saved.settings,FUZZY,FUZZY_VALUES)?,
            priority:saved.priority,ghosts,source_ghosts:None,desired,name:None,part_source:true,source_is_interface:false,
        }))
    }
    pub(in crate::game) fn bus_as_vibration(saved:&SavedBus)->SavedVibration{
        let mut cards=Vec::new();
        if let Some(desired)=saved.desired.as_ref(){for item in desired.iter().filter(|s|matches!(s.item.as_ref(),"ae2:speed_card"|"ae2:energy_card")){
            for _ in 0..item.count.min(3){if cards.len()<3{cards.push(moved_copy(item,1));}}
        }}
        SavedVibration{desired:cards,name:None}
    }
    fn selected_cards(saved:&SavedBus,limit:usize,redstone:bool)->Vec<ItemStack>{
        let mut cards=Vec::new();let mut has_redstone=false;
        if let Some(desired)=saved.desired.as_ref(){for item in desired {
            if item.item.as_ref()=="ae2:redstone_card" && redstone && !has_redstone && cards.len()<limit {
                cards.push(moved_copy(item,1));has_redstone=true;
            }else if item.item.as_ref()=="ae2:speed_card" {
                for _ in 0..item.count.min(limit as u32){if cards.len()<limit{cards.push(moved_copy(item,1));}}
            }
        }}cards
    }
    pub(in crate::game) fn bus_to_inscriber(machine:&Machine,saved:&SavedBus)->Result<SavedInscriber,String>{
        let mut value=vibration_to_inscriber(machine,&bus_as_vibration(saved))?;value.desired=selected_cards(saved,4,false);Ok(value)
    }
    pub(in crate::game) fn bus_to_io(machine:&Machine,saved:&SavedBus)->Result<SavedIo,String>{
        let mut value=vibration_to_io(machine,&bus_as_vibration(saved))?;value.desired=selected_cards(saved,3,true);Ok(value)
    }
    pub(in crate::game) fn bus_io_redstone(saved:&SavedBus)->Option<&'static str>{
        match setting(&saved.settings,"redstone_controlled",&["ignore","low_signal","high_signal"]).ok().flatten()?.as_str(){
            "low_signal"=>Some("low"),"high_signal"=>Some("high"),"ignore"=>Some("ignore"),_=>None
        }
    }
    pub(in crate::game) fn bus_to_assembler(saved:&SavedBus)->SavedAssembler{
        SavedAssembler{desired:selected_cards(saved,5,false),name:None}
    }

    #[cfg(test)]
    mod bus_memory_tests {
        use super::*;
        fn part(id:&str)->Compound {
            let mut out=Compound::new();out.insert("id".into(),Tag::String(id.into()));out
        }
        fn store_state(part:&mut Compound,state:&ae2_machines_core::BusState){
            part.insert("state_json".into(),Tag::String(serde_json::to_string(state).unwrap().as_str().into()));
        }
        fn custom(source:&str,fields:Vec<(&str,Tag)>)->ItemStack {
            let mut body=Compound::new();body.insert("source".into(),Tag::String(source.into()));
            for (k,v) in fields{body.insert(k.into(),v);}
            let mut card=ItemStack::new("ae2:memory_card",1);
            card.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([(KEY,Tag::Compound(body))])));card
        }
        #[test]
        fn full_bus_import_moves_physical_upgrades_and_cancels_removed_crafting(){
            let mut source=part("ae2:export_bus");let mut state=ae2_buses::bus_state(&source).unwrap();
            let diamond=ItemStack::new("minecraft:diamond",1);state.config[0]=Some(Key::from(&ae2_lab::key(&diamond).unwrap()));
            state.config[22]=Some(Key{id:"minecraft:water".into(),components:Vec::new()});state.redstone=Redstone::High;state.schedule=Schedule::RoundRobin;store_state(&mut source,&state);
            ae2_buses::store_bus_upgrade_slots(&mut source,&vec![Some(ItemStack::new("ae2:speed_card",1)),Some(ItemStack::new("ae2:speed_card",1)),Some(ItemStack::new("ae2:capacity_card",1)),Some(ItemStack::new("ae2:crafting_card",1)),None]).unwrap();
            let mut card=ItemStack::new("ae2:memory_card",1);save_bus(&mut card,&source,Some("Source")).unwrap();
            let saved=read_bus(&card,"ae2:export_bus").unwrap().unwrap();assert!(saved.matching);
            let mut target=part("ae2:export_bus");ae2_buses::store_bus_upgrade_slots(&mut target,&vec![Some(ItemStack::new("ae2:crafting_card",1)),None,None,None,None]).unwrap();
            target.insert("ae2_bus_craft_links".into(),Tag::List(vec![Tag::compound([("Slot",Tag::Byte(2)),("cpu",Tag::IntArray(vec![1,64,2]))])]));
            let mut player=vec![None;36];player[0]=Some(ItemStack::new("ae2:speed_card",2));player[1]=Some(ItemStack::new("ae2:capacity_card",1));let mut pocket=None;
            let changes=apply_bus(&mut target,&mut player,&mut pocket,&saved,false).unwrap();
            assert!(changes.drops.is_empty());assert_eq!(changes.missing,0);
            let actual=ae2_buses::bus_state(&target).unwrap();assert_eq!(actual.config,state.config);assert_eq!(actual.redstone,Redstone::High);assert_eq!(actual.schedule,Schedule::RoundRobin);
            assert_eq!(ae2_buses::bus_upgrade_slots(&target).unwrap().iter().flatten().count(),4);
            assert_eq!(target.get(&"ae2_bus_cancelled_links".into()),Some(&Tag::Long(4)));assert_eq!(saved.name.as_deref(),Some("Source"));
        }
        #[test]
        fn partial_and_cross_kind_imports_keep_absent_filters_and_name(){
            let mut target=part("ae2:storage_bus");target.insert("priority".into(),Tag::Int(-7));
            let mut state=ae2_buses::bus_state(&target).unwrap();state.config[3]=Some(Key::from(&ae2_lab::key(&ItemStack::new("minecraft:emerald",1)).unwrap()));store_state(&mut target,&state);
            ae2_buses::store_bus_upgrade_slots(&mut target,&vec![Some(ItemStack::new("ae2:fuzzy_card",1)),None,None,None,None]).unwrap();
            let card=custom("ae2:interface",vec![("priority",Tag::Int(42))]);
            let saved=read_bus(&card,"ae2:storage_bus").unwrap().unwrap();assert!(!saved.matching);assert!(saved.desired.is_none()&&saved.ghosts.is_none());
            let mut inv=vec![None;36];let mut tool=None;apply_bus(&mut target,&mut inv,&mut tool,&saved,false).unwrap();
            assert_eq!(target.get(&"priority".into()),Some(&Tag::Int(42)));assert_eq!(ae2_buses::bus_state(&target).unwrap().config,state.config);
            assert_eq!(ae2_buses::bus_upgrade_slots(&target).unwrap()[0].as_ref().unwrap().item.as_ref(),"ae2:fuzzy_card");
            let empty=custom("ae2:interface",vec![("config_items",Tag::List(vec![])),("config_fluids",Tag::List(vec![]))]);
            let saved=read_bus(&empty,"ae2:storage_bus").unwrap().unwrap();apply_bus(&mut target,&mut inv,&mut tool,&saved,false).unwrap();
            assert!(ae2_buses::bus_state(&target).unwrap().config.iter().all(Option::is_none));
        }
        #[test]
        fn creative_full_inventory_preserves_source_removal_quirk(){
            let mut source=part("ae2:import_bus");ae2_buses::store_bus_upgrade_slots(&mut source,&vec![Some(ItemStack::new("ae2:fuzzy_card",1)),None,None,None,None]).unwrap();
            let mut card=ItemStack::new("ae2:memory_card",1);save_bus(&mut card,&source,None).unwrap();
            let saved=read_bus(&card,"ae2:import_bus").unwrap().unwrap();let mut target=part("ae2:import_bus");
            let mut inv=vec![Some(ItemStack::new("minecraft:stone",64));36];let mut tool=None;
            let changes=apply_bus(&mut target,&mut inv,&mut tool,&saved,true).unwrap();
            assert!(ae2_buses::bus_upgrade_slots(&target).unwrap().iter().all(Option::is_none));
            assert_eq!(changes.missing,1);assert_eq!(changes.drops.len(),1);assert_eq!(changes.drops[0].item.as_ref(),"ae2:fuzzy_card");
        }
        #[test]
        fn bus_card_cross_import_truncates_ghosts_and_keeps_shared_priority(){
            let mut source=part("ae2:storage_bus");source.insert("priority".into(),Tag::Int(18));
            let mut state=ae2_buses::bus_state(&source).unwrap();state.config[0]=Some(Key::from(&ae2_lab::key(&ItemStack::new("minecraft:diamond",1)).unwrap()));state.config[40]=Some(Key{id:"minecraft:water".into(),components:Vec::new()});store_state(&mut source,&state);
            ae2_buses::store_bus_upgrade_slots(&mut source,&vec![Some(ItemStack::new("ae2:capacity_card",1)),Some(ItemStack::new("ae2:fuzzy_card",1)),None,None,None]).unwrap();
            let mut card=ItemStack::new("ae2:memory_card",1);save_bus(&mut card,&source,Some("Bus")).unwrap();
            let generic=bus_as_interface(&card).unwrap().unwrap();assert!(!generic.source_is_interface);assert_eq!(generic.priority,Some(18));assert_eq!(generic.desired.unwrap().unwrap().item.as_ref(),"ae2:fuzzy_card");
            assert_eq!(bus_priority_from_card(&card).unwrap(),Some(18));
            let filters=generic.source_ghosts.unwrap();assert_eq!(filters.len(),9);assert_eq!(filters[0].as_ref().unwrap().key.item_id,"minecraft:diamond");assert!(filters[1..].iter().all(Option::is_none));
        }
        #[test]
        fn source_only_bus_card_roundtrips_filters_and_moves_grouped_cards(){
            let mut source=part("ae2:export_bus");let mut state=ae2_buses::bus_state(&source).unwrap();
            let mut diamond=ItemStack::new("minecraft:diamond",1);diamond.data_mut().custom_name=Some("key".into());
            state.config[0]=Some(Key::from(&ae2_lab::key(&diamond).unwrap()));state.config[3]=Some(Key{id:"minecraft:water".into(),components:Vec::new()});
            state.redstone=Redstone::High;state.schedule=Schedule::RoundRobin;store_state(&mut source,&state);
            ae2_buses::store_bus_upgrade_slots(&mut source,&vec![Some(ItemStack::new("ae2:speed_card",1)),Some(ItemStack::new("ae2:speed_card",1)),None,None,None]).unwrap();
            let mut card=ItemStack::new("ae2:memory_card",1);save_bus(&mut card,&source,Some("Bus")).unwrap();
            let Some(Tag::List(filters))=card.data().unwrap().other("ae2:exported_config_inv")else{panic!("missing source config")};
            assert_eq!(filters[0].get("#"),Some(&Tag::Long(0)));assert_eq!(filters[3].get("#"),Some(&Tag::Long(0)));assert_eq!(filters.len(),63);
            let Some(Tag::List(cards))=card.data().unwrap().other("ae2:exported_upgrades")else{panic!("missing upgrades")};assert_eq!(cards[0].get("count"),Some(&Tag::Int(2)));
            card.data_mut().set_other("minecraft:custom_data",None);let saved=read_bus(&card,"ae2:export_bus").unwrap().unwrap();assert!(saved.matching);assert_eq!(saved.name.as_deref(),Some("Bus"));
            let mut target=part("ae2:export_bus");let mut player=vec![None;36];player[0]=Some(ItemStack::new("ae2:speed_card",2));let mut pocket=None;
            let changes=apply_bus(&mut target,&mut player,&mut pocket,&saved,false).unwrap();assert_eq!(changes.missing,0);assert!(changes.drops.is_empty());
            assert_eq!(ae2_buses::bus_state(&target).unwrap().config,state.config);assert_eq!(ae2_buses::bus_state(&target).unwrap().schedule,Schedule::RoundRobin);
            assert_eq!(ae2_buses::bus_upgrade_slots(&target).unwrap().iter().flatten().count(),2);
            card.data_mut().set_other("ae2:exported_settings",Some(Tag::compound([("redstone_controlled",Tag::String("high_signal".into()))])));
            assert!(!read_bus(&card,"ae2:export_bus").unwrap().unwrap().settings.contains_key(&"redstone_controlled".into()));
        }
    }
}
pub(in crate::game) use bus_memory::{SavedBus,save_bus,read_bus,bus_has_components,apply_bus,bus_priority_from_card,bus_source_from_card,bus_as_interface,bus_as_vibration,bus_to_inscriber,bus_to_io,bus_io_redstone,bus_to_assembler};
