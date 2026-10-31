// Storage Level Emitter Memory Card codec and generic physical upgrade ownership.
mod emitter_memory {
    use super::*;
    use super::super::ae2_emitter::{EmitterFilter,EmitterState};
    use super::super::ae2_interface::{interface_config,interface_fluid_config};
    const SOURCE:&str="ae2:level_emitter";
    const FUZZY:&str="ae2_fuzzy_mode";
    const FUZZY_VALUES:&[&str]=&["ignore_all","percent_99","percent_75","percent_50","percent_25"];
    #[derive(Clone,Debug)]
    pub(in crate::game) struct SavedEmitter {
        pub matching:bool,
        pub threshold:Option<i64>,
        pub low:Option<bool>,
        pub fuzzy:Option<String>,
        pub craft_via_redstone:Option<bool>,
        pub filter:Option<Option<EmitterFilter>>,
        pub desired:Option<Vec<ItemStack>>,
        pub name:Option<String>,
    }
    fn enum_setting(settings:Option<&Tag>,key:&str,allowed:&[&str])->Result<Option<String>,String>{
        let Some(settings)=settings else{return Ok(None)};
        let Tag::Compound(settings)=settings else{return Err("invalid emitter settings".into())};
        match settings.get(&key.into()){
            None=>Ok(None),Some(Tag::String(v))=>{let s=v.to_string().map_err(|_|"invalid setting text")?;Ok(allowed.contains(&s.as_str()).then_some(s))},
            _=>Err("invalid setting type".into()),
        }
    }
    pub(in crate::game) fn save_emitter(card:&mut ItemStack,state:&EmitterState,name:Option<&str>)->Result<(),String>{
        if card.item.as_ref()!="ae2:memory_card"||card.count!=1||state.upgrades.len()!=1{return Err("invalid emitter card or upgrades".into())}
        // ConfigInventory.setStack stores CONFIG_TYPES keys with amount zero.
        let source_filter=match state.filter.as_ref(){
            None=>Tag::Compound(Compound::new()),
            Some(EmitterFilter::Item(stack))=>horizon_client::ae2_generic_stack::encode(&horizon_client::ae2_generic_stack::GenericStack{key:horizon_client::ae2_cell_codec::key(stack)?,fluid:false,amount:0})?,
            Some(EmitterFilter::Fluid(key))=>horizon_client::ae2_generic_stack::encode(&horizon_client::ae2_generic_stack::GenericStack{key:key.clone(),fluid:true,amount:0})?,
        };
        let mut body=Compound::new();body.insert("source".into(),Tag::String(SOURCE.into()));
        body.insert("reporting_value".into(),Tag::Long(state.threshold));
        body.insert("settings".into(),Tag::compound([
            ("redstone_emitter",Tag::String(if state.low{"low_signal"}else{"high_signal"}.into())),
            ("craft_via_redstone",Tag::String(if state.craft_via_redstone{"yes"}else{"no"}.into())),
            (FUZZY,Tag::String(state.fuzzy_mode.as_str().into())),
        ]));
        body.insert("upgrades".into(),Tag::List(state.upgrades.iter().flatten().map(save::item_to_nbt).collect()));
        let mut items=vec![None];let mut fluids=Vec::new();
        match state.filter.as_ref(){
            Some(EmitterFilter::Item(stack))=>items[0]=Some(stack.clone()),
            Some(EmitterFilter::Fluid(key))=>fluids.push(Tag::compound([("Slot",Tag::Byte(0)),("fluid",Tag::String(key.item_id.as_str().into())),("components",Tag::ByteArray(key.components.clone())),("amount",Tag::Long(1000))])),
            None=>{}
        }
        body.insert("config_items".into(),save::encode_items(&items));body.insert("config_fluids".into(),Tag::List(fluids));
        if let Some(name)=name{body.insert("name".into(),Tag::String(name.into()));}
        let mut custom=match card.data().and_then(|d|d.other("minecraft:custom_data")){None=>Compound::new(),Some(Tag::Compound(v))=>v.clone(),_=>return Err("invalid card data".into())};
        custom.remove(&P2P.into());custom.insert(KEY.into(),Tag::Compound(body));card.data_mut().set_other("minecraft:custom_data",Some(Tag::Compound(custom)));
        card.data_mut().set_other("ae2:exported_settings_source",Some(Tag::compound([("translate",Tag::String("item.ae2.level_emitter".into()))])));
        card.data_mut().set_other("ae2:exported_level_emitter_value",Some(Tag::Long(state.threshold)));
        card.data_mut().set_other("ae2:exported_settings",Some(Tag::compound([
            ("redstone_emitter",Tag::String(if state.low{"LOW_SIGNAL"}else{"HIGH_SIGNAL"}.into())),
            ("craft_via_redstone",Tag::String(if state.craft_via_redstone{"YES"}else{"NO"}.into())),
            ("fuzzy_mode",Tag::String(state.fuzzy_mode.to_ascii_uppercase().as_str().into())),
        ])));
        card.data_mut().set_other("ae2:exported_upgrades",Some(Tag::List(state.upgrades.iter().flatten().map(save::item_to_nbt).collect())));
        card.data_mut().set_other("ae2:exported_config_inv",Some(Tag::List(vec![source_filter])));
        card.data_mut().set_other("ae2:exported_custom_name",name.map(|value|Tag::String(value.into())));
        Ok(())
    }
    pub(in crate::game) fn read_emitter(card:&ItemStack)->Result<Option<SavedEmitter>,String>{
        if card.item.as_ref()!="ae2:memory_card"||card.count!=1{return Err("invalid card".into())}
        if let Some(data)=card.data(){
            let ids=["ae2:exported_settings_source","ae2:exported_settings","ae2:exported_upgrades","ae2:exported_config_inv","ae2:exported_level_emitter_value","ae2:exported_custom_name"];
            if ids.iter().any(|id|data.other(id).is_some()){
                let matching=source_description_matches(data.other("ae2:exported_settings_source"),"item.ae2.level_emitter");
                let threshold=if matching{match data.other("ae2:exported_level_emitter_value"){
                    None=>None,Some(v)=>match horizon_client::ae2_components::normalize("ae2:exported_level_emitter_value",v.clone())?{Tag::Long(n)=>Some(n),_=>unreachable!()},
                }}else{None};
                let setting=|key:&str,allowed:&[&str]|->Result<Option<String>,String>{
                    let Some(settings)=data.other("ae2:exported_settings") else{return Ok(None)};
                    let Tag::Compound(settings)=settings else{return Err("invalid emitter settings".into())};
                    match settings.get(&key.into()){
                        None=>Ok(None),Some(Tag::String(value))=>{let source=value.to_string().map_err(|_|"invalid emitter setting")?;let value=source.to_ascii_lowercase();Ok((source==value.to_ascii_uppercase()&&allowed.contains(&value.as_str())).then_some(value))},
                        _=>Err("invalid emitter setting type".into()),
                    }
                };
                let low=setting("redstone_emitter",&["low_signal","high_signal"])?.map(|v|v=="low_signal");
                let fuzzy=setting("fuzzy_mode",FUZZY_VALUES)?;
                let craft_via_redstone=setting("craft_via_redstone",&["yes","no"])?.map(|v|v=="yes");
                let desired=match data.other("ae2:exported_upgrades"){
                    None=>None,Some(Tag::List(entries))=>{
                        let mut accepted=Vec::new();
                        for tag in entries {let stack=save::item_from_nbt(tag).ok_or("invalid exported upgrade")?;if stack.count==0{return Err("invalid exported upgrade count".into())}if matches!(stack.item.as_ref(),"ae2:crafting_card"|"ae2:fuzzy_card")&&!accepted.iter().any(|old:&ItemStack|old.item==stack.item){accepted.push(stack);}}
                        Some(accepted)
                    },_=>return Err("invalid exported upgrades".into()),
                };
                let filter=match data.other("ae2:exported_config_inv"){
                    None=>None,Some(Tag::List(entries))=>{
                        let first=if let Some(tag)=entries.first(){horizon_client::ae2_generic_stack::decode_tolerant(tag)?}else{None};
                        Some(match first {None=>None,Some(value) if value.fluid=>Some(EmitterFilter::Fluid(value.key)),Some(value)=>Some(EmitterFilter::Item(horizon_client::ae2_cell_codec::prototype(&value.key)?))})
                    },_=>return Err("invalid exported config inventory".into()),
                };
                let name=if matching{match data.other("ae2:exported_custom_name"){
                    None=>None,Some(Tag::String(value))=>Some(value.to_string().map_err(|_|"invalid name")?),
                    Some(value @ Tag::Compound(_))=>match value.get("text"){Some(Tag::String(value))=>Some(value.to_string().map_err(|_|"invalid name")?),_=>return Err("unsupported structured emitter name".into())},
                    _=>return Err("invalid name".into()),
                }}else{None};
                return Ok(Some(SavedEmitter{matching,threshold,low,fuzzy,craft_via_redstone,filter,desired,name}));
            }
        }
        let Some(custom)=card.data().and_then(|d|d.other("minecraft:custom_data"))else{return Ok(None)};
        let Some(body)=custom.get(KEY)else{return Ok(None)};let Tag::Compound(_)=body else{return Err("invalid card settings".into())};
        let matching=body.get("source")==Some(&Tag::String(SOURCE.into()));
        let threshold=if matching {match body.get("reporting_value"){None=>None,Some(Tag::Long(v))=>Some(*v),_=>return Err("invalid emitter value".into())}}else{None};
        let settings=body.get("settings");
        let low=enum_setting(settings,"redstone_emitter",&["low_signal","high_signal"])?.map(|v|v=="low_signal");
        let fuzzy=enum_setting(settings,FUZZY,FUZZY_VALUES)?;
        let craft_via_redstone=enum_setting(settings,"craft_via_redstone",&["yes","no"])?.map(|v|v=="yes");
        let desired=match body.get("upgrades"){
            None=>None,Some(Tag::List(entries))=>{
                let mut accepted=Vec::new();
                for tag in entries {let stack=save::item_from_nbt(tag).ok_or("invalid exported upgrade")?;if stack.count==0{return Err("invalid exported upgrade count".into())}if matches!(stack.item.as_ref(),"ae2:crafting_card"|"ae2:fuzzy_card")&&!accepted.iter().any(|old:&ItemStack|old.item==stack.item){accepted.push(stack);}}
                Some(accepted)
            },_=>return Err("invalid exported upgrades".into())
        };
        let filter=match(body.get("config_items"),body.get("config_fluids")){
            (None,None)=>None,(Some(items),Some(fluids))=>{
                // The emitter has one config slot; larger source inventories
                // are read only through the target's first slot.
                let first=|tag:&Tag|->Result<Tag,String>{
                    let Tag::List(entries)=tag else{return Err("invalid config inventory".into())};
                    Ok(Tag::List(entries.iter().filter(|entry|!matches!(entry.get("Slot"),Some(Tag::Byte(slot)) if *slot!=0)).cloned().collect()))
                };
                let mut machine=Machine::new(MachineKind::Interface);machine.config.insert("ae2_interface_config".into(),first(items)?);machine.config.insert("ae2_interface_fluid_config".into(),first(fluids)?);
                let mut items=interface_config(&machine)?;let mut fluids=interface_fluid_config(&machine)?;
                if items.iter().zip(fluids.iter()).any(|(a,b)|a.is_some()&&b.is_some()){return Err("conflicting ghost data".into())}
                let item=items[0].take().map(|mut s|{s.count=1;EmitterFilter::Item(s)});
                let fluid=fluids[0].take().map(|(key,_)|EmitterFilter::Fluid(key));Some(item.or(fluid))
            },_=>return Err("incomplete native config component".into()),
        };
        let name=if matching{match body.get("name"){None=>None,Some(Tag::String(v))=>Some(v.to_string().map_err(|_|"invalid name")?),_=>return Err("invalid name".into())}}else{None};
        Ok(Some(SavedEmitter{matching,threshold,low,fuzzy,craft_via_redstone,filter,desired,name}))
    }
    pub(in crate::game) fn emitter_has_components(saved:&SavedEmitter)->bool{
        saved.low.is_some()||saved.fuzzy.is_some()||saved.craft_via_redstone.is_some()||saved.filter.is_some()||saved.desired.is_some()
    }
    pub(in crate::game) fn apply_emitter(state:&mut EmitterState,player:&mut Vec<Option<ItemStack>>,pocket:&mut Option<horizon_client::ae2_network_tool::Inventory>,saved:&SavedEmitter,creative:bool)->Result<PhysicalChanges,String>{
        let mut next=state.clone();let mut inv=player.clone();let mut tool=pocket.clone();
        let mut changes=PhysicalChanges{drops:Vec::new(),changed_tracked:Vec::new(),replaced_tracked:Vec::new(),missing:0};
        if let Some(desired)=saved.desired.as_ref(){
            if next.upgrades.len()!=1 || next.upgrades.iter().flatten().any(|s|s.count!=1||!matches!(s.item.as_ref(),"ae2:crafting_card"|"ae2:fuzzy_card")){return Err("invalid emitter upgrades".into())}
            // Java's creative path creates the first accepted upgrade before
            // its shared removal pass. The Item-vs-ItemStack lookup then moves
            // all installed upgrades out even if the card also asks for them.
            if creative {
                if let Some(old)=next.upgrades[0].take(){changes.replaced_tracked.push(old);}
                if let Some(first)=desired.first(){next.upgrades[0]=Some(moved_copy(first,1));}
            }
            if let Some(mut removed)=next.upgrades[0].take(){
                changes.replaced_tracked.push(removed.clone());horizon_client::ae2_stack_identity::detach_if_tracked(&mut removed);
                add_to_player(&mut inv,&mut removed,64,&mut changes.replaced_tracked);
                if removed.count>0{if let Some(tool)=tool.as_mut(){add_to_player(&mut tool.slots,&mut removed,64,&mut changes.replaced_tracked);}}
                if removed.count>0{changes.drops.push(removed);}
            }
            // Source iterates an identity-keyed map; its exact choice when two
            // valid types compete for one slot is JVM-identity dependent. The
            // native representation follows exported entry order and retains
            // real item ownership even when a desired type is unavailable.
            for wanted in desired {
                if next.upgrades[0].is_some(){break}
                let filter=ItemStack::new(&wanted.item,1);let mut found=take_from_slots(&mut inv,&filter,1,&mut changes,false);
                if found.is_empty(){if let Some(tool)=tool.as_mut(){found=take_from_slots(&mut tool.slots,&filter,1,&mut changes,true);}}
                if let Some(card)=found.first(){next.upgrades[0]=Some(moved_copy(card,1));}else{changes.missing+=1;}
            }
        }
        if saved.matching {if let Some(v)=saved.threshold{next.threshold=v;}}
        if let Some(v)=saved.low{next.low=v;}if let Some(v)=saved.fuzzy.as_ref(){next.fuzzy_mode=v.clone();}
        if let Some(v)=saved.craft_via_redstone{next.craft_via_redstone=v;}if let Some(v)=saved.filter.as_ref(){next.filter=v.clone();}
        *state=next;*player=inv;*pocket=tool;Ok(changes)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        fn state()->EmitterState{EmitterState{threshold:9,low:false,filter:Some(EmitterFilter::Item(ItemStack::new("minecraft:emerald",1))),fuzzy_mode:"ignore_all".into(),craft_via_redstone:false,upgrades:vec![Some(ItemStack::new("ae2:crafting_card",1))]}}
        fn remove_source_components(card:&mut ItemStack){for id in ["ae2:exported_settings_source","ae2:exported_settings","ae2:exported_upgrades","ae2:exported_config_inv","ae2:exported_level_emitter_value","ae2:exported_custom_name"]{card.data_mut().set_other(id,None);}}
        #[test]
        fn emitter_memory_full_moves_real_cards_and_transfers_config(){
            let mut source=state();source.threshold=42;source.low=true;source.filter=Some(EmitterFilter::Item(ItemStack::new("minecraft:diamond",1)));source.upgrades[0]=Some(ItemStack::new("ae2:fuzzy_card",1));
            let mut card=ItemStack::new("ae2:memory_card",1);save_emitter(&mut card,&source,Some("Source Emitter")).unwrap();let saved=read_emitter(&card).unwrap().unwrap();
            let mut target=state();let mut player=vec![None;36];player[0]=Some(ItemStack::new("ae2:fuzzy_card",1));let mut pocket=None;
            let changes=apply_emitter(&mut target,&mut player,&mut pocket,&saved,false).unwrap();assert!(changes.drops.is_empty());assert_eq!(changes.missing,0);
            assert_eq!(target.threshold,42);assert!(target.low);assert_eq!(target.filter,source.filter);assert_eq!(target.upgrades[0].as_ref().unwrap().item.as_ref(),"ae2:fuzzy_card");
            assert_eq!(player.iter().flatten().filter(|s|s.item.as_ref()=="ae2:crafting_card").map(|s|s.count).sum::<u32>(),1);assert_eq!(saved.name.as_deref(),Some("Source Emitter"));
        }
        #[test]
        fn emitter_memory_creative_full_inventory_matches_observed_source_drop(){
            let mut source=state();source.upgrades[0]=Some(ItemStack::new("ae2:fuzzy_card",1));let mut card=ItemStack::new("ae2:memory_card",1);save_emitter(&mut card,&source,None).unwrap();let saved=read_emitter(&card).unwrap().unwrap();
            let mut target=state();let mut inv=vec![Some(ItemStack::new("minecraft:stone",64));36];let mut pocket=None;
            let changes=apply_emitter(&mut target,&mut inv,&mut pocket,&saved,true).unwrap();assert!(target.upgrades[0].is_none());assert_eq!(changes.missing,1);assert_eq!(changes.drops.len(),1);assert_eq!(changes.drops[0].item.as_ref(),"ae2:fuzzy_card");
        }
        #[test]
        fn emitter_memory_multiple_compatible_cards_respects_capacity_and_ownership(){
            let mut source=state();source.upgrades[0]=Some(ItemStack::new("ae2:fuzzy_card",1));let mut card=ItemStack::new("ae2:memory_card",1);save_emitter(&mut card,&source,None).unwrap();
            let Some(Tag::Compound(mut custom))=card.data().unwrap().other("minecraft:custom_data").cloned()else{panic!("custom")};let Some(Tag::Compound(body))=custom.get_mut(&KEY.into())else{panic!("body")};
            body.insert("source".into(),Tag::String("ae2:export_bus".into()));body.insert("upgrades".into(),Tag::List(vec![save::item_to_nbt(&ItemStack::new("ae2:fuzzy_card",1)),save::item_to_nbt(&ItemStack::new("ae2:crafting_card",1))]));card.data_mut().set_other("minecraft:custom_data",Some(Tag::Compound(custom)));
            remove_source_components(&mut card);
            let saved=read_emitter(&card).unwrap().unwrap();let mut target=state();let mut inv=vec![None;36];inv[0]=Some(ItemStack::new("ae2:fuzzy_card",1));let mut pocket=None;
            let changes=apply_emitter(&mut target,&mut inv,&mut pocket,&saved,false).unwrap();assert_eq!(changes.missing,0);assert!(changes.drops.is_empty());assert_eq!(target.upgrades.iter().flatten().count(),1);
            let fuzzy=target.upgrades.iter().chain(inv.iter()).flatten().filter(|s|s.item.as_ref()=="ae2:fuzzy_card").map(|s|s.count).sum::<u32>();
            let crafting=target.upgrades.iter().chain(inv.iter()).flatten().filter(|s|s.item.as_ref()=="ae2:crafting_card").map(|s|s.count).sum::<u32>();assert_eq!((fuzzy,crafting),(1,1));
        }
        #[test]
        fn emitter_card_exports_generic_fluid_and_fuzzy_to_interface_without_priority(){
            let mut source=state();source.filter=Some(EmitterFilter::Fluid(ae2_cell_core::ItemKey{item_id:"minecraft:water".into(),components:Vec::new()}));source.fuzzy_mode="percent_75".into();source.upgrades[0]=None;
            let mut card=ItemStack::new("ae2:memory_card",1);save_emitter(&mut card,&source,Some("emitter")).unwrap();let saved=emitter_as_interface(&card).unwrap().unwrap();
            assert!(!saved.source_is_interface);assert!(saved.priority.is_none());assert_eq!(saved.fuzzy.as_deref(),Some("percent_75"));
            let mut target=Machine::new(MachineKind::Interface);target.config.insert("priority".into(),Tag::Int(19));let mut inv=vec![None;36];let mut pocket=None;
            apply_interface(&mut target,&mut inv,&mut pocket,&saved,false,true).unwrap();let fluids=interface_fluid_config(&target).unwrap();assert_eq!(fluids[0].as_ref().unwrap().0.item_id,"minecraft:water");assert_eq!(target.config.get(&"priority".into()),Some(&Tag::Int(19)));
        }
        #[test]
        fn emitter_memory_independent_generic_components_leave_threshold_and_upgrade_unchanged(){
            let mut source=state();source.threshold=42;source.low=true;source.filter=Some(EmitterFilter::Item(ItemStack::new("minecraft:diamond",1)));
            let mut card=ItemStack::new("ae2:memory_card",1);save_emitter(&mut card,&source,Some("source")).unwrap();
            let Some(Tag::Compound(mut custom))=card.data().unwrap().other("minecraft:custom_data").cloned() else{panic!("custom")};let Some(Tag::Compound(body))=custom.get_mut(&KEY.into())else{panic!("body")};
            body.insert("source".into(),Tag::String("ae2:inscriber".into()));body.remove(&"upgrades".into());card.data_mut().set_other("minecraft:custom_data",Some(Tag::Compound(custom)));
            remove_source_components(&mut card);
            let saved=read_emitter(&card).unwrap().unwrap();assert!(!saved.matching);let mut target=state();let mut inv=vec![None;36];let mut pocket=None;
            apply_emitter(&mut target,&mut inv,&mut pocket,&saved,false).unwrap();assert_eq!(target.threshold,9);assert!(target.low);assert_eq!(target.filter,source.filter);assert_eq!(target.upgrades[0].as_ref().unwrap().item.as_ref(),"ae2:crafting_card");assert_eq!(saved.name,None);
        }
        #[test]
        fn source_only_emitter_card_preserves_key_components_and_physical_upgrades(){
            let mut source=state();source.threshold=42;source.low=true;source.fuzzy_mode="percent_75".into();source.craft_via_redstone=true;
            let mut item=ItemStack::new("minecraft:diamond",1);item.data_mut().custom_name=Some("filter".into());source.filter=Some(EmitterFilter::Item(item));
            source.upgrades[0]=Some(ItemStack::new("ae2:fuzzy_card",1));
            let mut card=ItemStack::new("ae2:memory_card",1);save_emitter(&mut card,&source,Some("Named")).unwrap();
            let Some(Tag::List(config))=card.data().unwrap().other("ae2:exported_config_inv")else{panic!("config")};assert_eq!(config[0].get("#"),Some(&Tag::Long(0)));
            card.data_mut().set_other("minecraft:custom_data",None);
            let saved=read_emitter(&card).unwrap().unwrap();assert!(saved.matching);assert_eq!(saved.name.as_deref(),Some("Named"));
            let mut target=state();let mut player=vec![None;36];player[0]=Some(ItemStack::new("ae2:fuzzy_card",1));let mut pocket=None;
            let changes=apply_emitter(&mut target,&mut player,&mut pocket,&saved,false).unwrap();assert!(changes.drops.is_empty());assert_eq!(changes.missing,0);
            assert_eq!(target.threshold,source.threshold);assert_eq!(target.filter,source.filter);assert_eq!(target.fuzzy_mode,source.fuzzy_mode);assert!(target.low&&target.craft_via_redstone);
            assert_eq!(target.upgrades[0].as_ref().unwrap().item.as_ref(),"ae2:fuzzy_card");assert_eq!(player.iter().flatten().filter(|s|s.item.as_ref()=="ae2:crafting_card").map(|s|s.count).sum::<u32>(),1);
            card.data_mut().set_other("ae2:exported_settings_source",Some(Tag::compound([("translate",Tag::String("item.ae2.energy_level_emitter".into()))])));
            let generic=read_emitter(&card).unwrap().unwrap();assert!(!generic.matching);assert_eq!(generic.threshold,None);assert_eq!(generic.name,None);assert_eq!(generic.low,Some(true));
            card.data_mut().set_other("ae2:exported_settings",Some(Tag::compound([("redstone_emitter",Tag::String("low_signal".into())),("fuzzy_mode",Tag::String("percent_75".into()))])));
            let invalid=read_emitter(&card).unwrap().unwrap();assert_eq!(invalid.low,None);assert_eq!(invalid.fuzzy,None);
        }
        #[test]
        fn source_emitter_fluid_patch_and_empty_inventory_roundtrip(){
            let components=Tag::compound([("test:variant",Tag::String("warm".into()))]);
            let bytes=horizon_world::nbt::write_network(&components,horizon_world::nbt::Limits::default()).unwrap();
            let mut source=state();source.filter=Some(EmitterFilter::Fluid(ae2_cell_core::ItemKey{item_id:"minecraft:water".into(),components:bytes}));
            let mut card=ItemStack::new("ae2:memory_card",1);save_emitter(&mut card,&source,None).unwrap();card.data_mut().set_other("minecraft:custom_data",None);
            assert_eq!(read_emitter(&card).unwrap().unwrap().filter,Some(source.filter.clone()));
            card.data_mut().set_other("ae2:exported_config_inv",Some(Tag::List(Vec::new())));
            assert_eq!(read_emitter(&card).unwrap().unwrap().filter,Some(None));
            card.data_mut().set_other("ae2:exported_config_inv",None);
            assert_eq!(read_emitter(&card).unwrap().unwrap().filter,None);
        }
    }
    /// Emitter and Interface share the generic fuzzy/filter/upgrade components.
    /// The synthetic source identity is only for reusing the native Interface
    /// decoder; part_source makes the generic target preserve its own name.
    pub(in crate::game) fn emitter_as_interface(card:&ItemStack)->Result<Option<SavedInterface>,String>{
        if card.data().and_then(|d|d.other("ae2:exported_settings_source")).and_then(|v|v.get("translate")).is_some_and(|source|source==&Tag::String("item.ae2.level_emitter".into())||source==&Tag::String("item.ae2.energy_level_emitter".into())){return read_source_interface(card,true)}
        let Some(mut data)=card.data().and_then(|d|d.other("minecraft:custom_data")).cloned() else{return Ok(None)};
        let Tag::Compound(ref mut custom)=data else{return Err("invalid card data".into())};
        let Some(Tag::Compound(body))=custom.get_mut(&KEY.into()) else{return Ok(None)};
        if !matches!(body.get(&"source".into()),Some(Tag::String(v)) if matches!(v.to_string_lossy().as_str(),"ae2:level_emitter"|"ae2:energy_level_emitter")){return Ok(None)}
        body.insert("source".into(),Tag::String("ae2:cable_interface".into()));
        let mut proxy=card.clone();proxy.data_mut().set_other("minecraft:custom_data",Some(data));
        Ok(read_interface(&proxy)?.map(|mut saved|{saved.source_is_interface=false;saved}))
    }
}
pub(in crate::game) use emitter_memory::{SavedEmitter,save_emitter,read_emitter,emitter_has_components,apply_emitter,emitter_as_interface};
