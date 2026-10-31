// Source-backed Condenser Memory Card settings; physical power and slots are
// owned by the target and are only changed by the normal output-mode callback.
mod condenser_memory {
    use super::*;
    const SOURCE:&str="ae2:condenser";
    const OUTPUT:&str="condenser_output";
    const MODES:&[&str]=&["trash","matter_balls","singularity"];

    #[derive(Debug)]
    pub(in crate::game) struct SavedCondenser {
        pub matching:bool,
        pub output:Option<String>,
        pub name:Option<String>,
    }
    pub(in crate::game) fn save_condenser(card:&mut ItemStack,machine:&Machine,name:Option<&str>)->Result<(),String>{
        if card.item.as_ref()!="ae2:memory_card"||card.count!=1||machine.kind!=MachineKind::Condenser{return Err("invalid Condenser card or target".into())}
        let output=setting_value(machine,OUTPUT,"trash",MODES)?;
        let mut data=match card.data().and_then(|d|d.other("minecraft:custom_data")){
            None=>Compound::new(),Some(Tag::Compound(c))=>c.clone(),_=>return Err("invalid card custom data".into()),
        };
        let mut body=Compound::new();body.insert("source".into(),Tag::String(SOURCE.into()));
        body.insert("settings".into(),Tag::compound([(OUTPUT,Tag::String(output.as_str().into()))]));
        if let Some(name)=name{body.insert("name".into(),Tag::String(name.into()));}
        data.remove(&P2P.into());data.insert(KEY.into(),Tag::Compound(body));
        card.data_mut().set_other("minecraft:custom_data",Some(Tag::Compound(data)));
        card.data_mut().set_other("ae2:exported_settings_source",Some(Tag::compound([("translate",Tag::String("block.ae2.condenser".into()))])));
        card.data_mut().set_other("ae2:exported_settings",Some(Tag::compound([(OUTPUT,Tag::String(output.to_ascii_uppercase().as_str().into()))])));
        card.data_mut().set_other("ae2:exported_custom_name",name.map(|value|Tag::String(value.into())));Ok(())
    }
    /// Generic settings are independent of source identity. Missing or unknown
    /// output values do not call the target's mode callback; same-kind import
    /// still imports the name, including clearing an absent name.
    pub(in crate::game) fn read_condenser(card:&ItemStack)->Result<Option<SavedCondenser>,String>{
        if card.item.as_ref()!="ae2:memory_card"||card.count!=1{return Err("invalid card".into())}
        if let Some(data)=card.data(){
            if ["ae2:exported_settings_source","ae2:exported_settings","ae2:exported_custom_name"].iter().any(|id|data.other(id).is_some()){
                let matching=source_description_matches(data.other("ae2:exported_settings_source"),"block.ae2.condenser");
                let output=match data.other("ae2:exported_settings"){
                    None=>None,Some(Tag::Compound(settings))=>match settings.get(&OUTPUT.into()){
                        None=>None,Some(Tag::String(value))=>{let value=value.to_string().map_err(|_|"invalid output setting")?;let lower=value.to_ascii_lowercase();(value==lower.to_ascii_uppercase()&&MODES.contains(&lower.as_str())).then_some(lower)},_=>return Err("invalid output setting".into()),
                    },_=>return Err("invalid settings".into()),
                };
                let name=if matching{match data.other("ae2:exported_custom_name"){
                    None=>None,Some(Tag::String(value))=>Some(value.to_string().map_err(|_|"invalid name")?),
                    Some(value @ Tag::Compound(_))=>match value.get("text"){Some(Tag::String(value))=>Some(value.to_string().map_err(|_|"invalid name")?),_=>return Err("unsupported structured condenser name".into())},_=>return Err("invalid name".into()),
                }}else{None};
                return Ok(Some(SavedCondenser{matching,output,name}));
            }
        }
        let Some(data)=card.data().and_then(|d|d.other("minecraft:custom_data")) else{return Ok(None)};
        let Tag::Compound(_)=data else{return Err("invalid card custom data".into())};
        let Some(body)=data.get(KEY) else{return Ok(None)};
        let Tag::Compound(_)=body else{return Err("invalid card settings".into())};
        let matching=body.get("source")==Some(&Tag::String(SOURCE.into()));
        let output=match body.get("settings"){
            None=>None,
            Some(Tag::Compound(settings))=>match settings.get(&OUTPUT.into()){
                None=>None,
                Some(Tag::String(s))=>{let value=s.to_string().map_err(|_|"invalid output text")?;MODES.contains(&value.as_str()).then_some(value)},
                _=>return Err("invalid output setting".into()),
            },
            _=>return Err("invalid settings map".into()),
        };
        let name=if matching {match body.get("name"){
            None=>None,Some(Tag::String(s))=>Some(s.to_string().map_err(|_|"invalid name")?),_=>return Err("invalid name".into()),
        }}else{None};
        Ok(Some(SavedCondenser{matching,output,name}))
    }
    pub(in crate::game) fn condenser_output_item(mode:&str)->Result<Option<&'static str>,String>{
        match mode{"trash"=>Ok(None),"matter_balls"=>Ok(Some("ae2:matter_ball")),"singularity"=>Ok(Some("ae2:singularity")),_=>Err("invalid Condenser output".into())}
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use super::super::super::ae2_storage_devices::condenser_apply_memory_mode;
        #[test]
        fn condenser_memory_replays_mode_callback_without_copying_source_inventory(){
            let mut source=Machine::new(MachineKind::Condenser);source.config.insert(OUTPUT.into(),Tag::String("matter_balls".into()));
            source.slots[1]=Some(ItemStack::new("ae2:cell_component_4k",1));let mut card=ItemStack::new("ae2:memory_card",1);
            save_condenser(&mut card,&source,Some("source")).unwrap();let saved=read_condenser(&card).unwrap().unwrap();
            assert!(saved.matching);assert_eq!(saved.name.as_deref(),Some("source"));assert_eq!(saved.output.as_deref(),Some("matter_balls"));
            let mut target=Machine::new(MachineKind::Condenser);target.slots[1]=Some(ItemStack::new("ae2:cell_component_1k",1));
            target.config.insert("condenser_power".into(),Tag::Double(500f64.to_bits()));
            condenser_apply_memory_mode(&mut target,saved.output.as_deref().unwrap(),64).unwrap();
            assert_eq!(target.slots[1].as_ref().unwrap().item.as_ref(),"ae2:cell_component_1k");
            assert_eq!(target.slots[0].as_ref().unwrap().item.as_ref(),"ae2:matter_ball");assert_eq!(target.slots[0].as_ref().unwrap().count,1);
            assert_eq!(target.config.get(&"condenser_power".into()),Some(&Tag::Double(244f64.to_bits())));
        }
        #[test]
        fn condenser_memory_missing_and_unrelated_settings_do_not_trigger_callback(){
            let make=|source:&str,settings:Option<Tag>|{
                let mut body=Compound::new();body.insert("source".into(),Tag::String(source.into()));
                if let Some(settings)=settings{body.insert("settings".into(),settings);}
                let mut card=ItemStack::new("ae2:memory_card",1);card.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([(KEY,Tag::Compound(body))])));card
            };
            let missing=read_condenser(&make(SOURCE,None)).unwrap().unwrap();assert!(missing.matching);assert_eq!(missing.output,None);assert_eq!(missing.name,None);
            let unrelated=read_condenser(&make("ae2:inscriber",Some(Tag::compound([("auto_export",Tag::String("YES".into()))])))).unwrap().unwrap();assert!(!unrelated.matching);assert_eq!(unrelated.output,None);
            let invalid=read_condenser(&make(SOURCE,Some(Tag::compound([(OUTPUT,Tag::String("invalid".into()))])))).unwrap().unwrap();assert_eq!(invalid.output,None);
            let generic=read_condenser(&make("ae2:inscriber",Some(Tag::compound([(OUTPUT,Tag::String("matter_balls".into()))])))).unwrap().unwrap();assert!(!generic.matching);assert_eq!(generic.output.as_deref(),Some("matter_balls"));
        }
        #[test]
        fn source_only_condenser_card_imports_mode_but_not_foreign_name(){
            let mut source=Machine::new(MachineKind::Condenser);source.config.insert(OUTPUT.into(),Tag::String("matter_balls".into()));
            let mut card=ItemStack::new("ae2:memory_card",1);save_condenser(&mut card,&source,Some("Condenser")).unwrap();card.data_mut().set_other("minecraft:custom_data",None);
            let saved=read_condenser(&card).unwrap().unwrap();assert!(saved.matching);assert_eq!(saved.output.as_deref(),Some("matter_balls"));assert_eq!(saved.name.as_deref(),Some("Condenser"));
            card.data_mut().set_other("ae2:exported_settings_source",Some(Tag::compound([("translate",Tag::String("block.ae2.inscriber".into()))])));
            let generic=read_condenser(&card).unwrap().unwrap();assert!(!generic.matching);assert_eq!(generic.name,None);assert_eq!(generic.output.as_deref(),Some("matter_balls"));
            card.data_mut().set_other("ae2:exported_settings",Some(Tag::compound([(OUTPUT,Tag::String("matter_balls".into()))])));
            assert_eq!(read_condenser(&card).unwrap().unwrap().output,None);
        }
        #[test]
        fn condenser_mode_callback_clamps_capacity_and_preserves_blocked_output(){
            let mut target=Machine::new(MachineKind::Condenser);target.slots[1]=Some(ItemStack::new("ae2:cell_component_1k",1));target.slots[0]=Some(ItemStack::new("ae2:singularity",1));
            target.config.insert("condenser_power".into(),Tag::Double(9000f64.to_bits()));
            condenser_apply_memory_mode(&mut target,"matter_balls",64).unwrap();
            assert_eq!(target.config.get(&"condenser_power".into()),Some(&Tag::Double(8192f64.to_bits())));
            assert_eq!(target.slots[0].as_ref().unwrap().item.as_ref(),"ae2:singularity");assert_eq!(target.slots[1].as_ref().unwrap().item.as_ref(),"ae2:cell_component_1k");
        }
    }
}
pub(in crate::game) use condenser_memory::{save_condenser,read_condenser,condenser_output_item};
