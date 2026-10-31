// Staged Molecular Assembler Memory Card support; not included in the engine.
mod assembler_memory {
    use super::*;

    #[derive(Clone, Debug)]
    pub(in crate::game) struct SavedAssembler {
        pub desired: Vec<ItemStack>,
        pub name: Option<String>,
    }
    fn validate(upgrades: &[Option<ItemStack>], groupable: bool) -> Result<(), String> {
        if upgrades.len() != 5 { return Err("invalid Assembler upgrade slots".into()); }
        let mut first: Option<&ItemStack> = None;
        for stack in upgrades.iter().flatten() {
            if stack.item.as_ref() != "ae2:speed_card" || stack.count != 1 { return Err("invalid Assembler upgrade".into()); }
            if groupable && first.is_some_and(|old| !old.same_item_same_components(stack)) {
                return Err("mixed upgrade components require source grouping".into());
            }
            first = Some(stack);
        }
        Ok(())
    }
    pub(in crate::game) fn save_assembler(card: &mut ItemStack, machine: &Machine, name: Option<&str>) -> Result<(), String> {
        if card.item.as_ref() != "ae2:memory_card" || card.count != 1 || machine.kind != MachineKind::MolecularAssembler { return Err("invalid card or target".into()); }
        validate(&machine.upgrades, true)?;
        let mut data = match card.data().and_then(|d| d.other("minecraft:custom_data")) {
            None => Compound::new(), Some(Tag::Compound(c)) => c.clone(), _ => return Err("invalid card custom data".into()),
        };
        let mut body = Compound::new();
        body.insert("source".into(),Tag::String("ae2:molecular_assembler".into()));
        body.insert("settings".into(),Tag::Compound(Compound::new()));
        body.insert("upgrades".into(),Tag::List(machine.upgrades.iter().flatten().map(save::item_to_nbt).collect()));
        if let Some(name) = name {body.insert("name".into(),Tag::String(name.into()));}
        data.remove(&P2P.into());data.insert(KEY.into(),Tag::Compound(body));
        card.data_mut().set_other("minecraft:custom_data",Some(Tag::Compound(data)));Ok(())
    }
    pub(in crate::game) fn read_assembler(card: &ItemStack) -> Result<Option<SavedAssembler>, String> {
        if card.item.as_ref() != "ae2:memory_card" || card.count != 1 {return Err("invalid card".into());}
        let Some(data)=card.data().and_then(|d|d.other("minecraft:custom_data")) else{return Ok(None)};
        let Some(body)=data.get(KEY) else{return Ok(None)};
        if body.get("source")!=Some(&Tag::String("ae2:molecular_assembler".into())){return Ok(None)}
        match body.get("settings") {Some(Tag::Compound(c)) if c.is_empty()=>{},_=>return Err("unexpected Assembler settings".into())}
        let Some(Tag::List(entries))=body.get("upgrades") else{return Err("invalid upgrades".into())};
        if entries.len()>5{return Err("too many upgrades".into())}
        let mut desired=Vec::with_capacity(entries.len());
        for entry in entries {if entry.get("count")!=Some(&Tag::Int(1)){return Err("invalid upgrade count".into())}desired.push(save::item_from_nbt(entry).ok_or("invalid upgrade")?);}
        let mut slots=vec![None;5];for (i,stack) in desired.iter().enumerate(){slots[i]=Some(stack.clone())}validate(&slots,true)?;
        let name=match body.get("name"){None=>None,Some(Tag::String(s))=>Some(s.to_string().map_err(|_|"invalid name")?),_=>return Err("invalid name".into())};
        Ok(Some(SavedAssembler{desired,name}))
    }
    pub(in crate::game) fn apply_assembler(
        machine:&mut Machine,player:&mut Vec<Option<ItemStack>>,
        pocket:&mut Option<horizon_client::ae2_network_tool::Inventory>,saved:&SavedAssembler,creative:bool,
    )->Result<PhysicalChanges,String>{
        if machine.kind!=MachineKind::MolecularAssembler||player.len()!=36||saved.desired.len()>5{return Err("invalid Assembler target".into())}
        validate(&machine.upgrades,false)?;
        let mut slots=vec![None;5];for (i,stack) in saved.desired.iter().enumerate(){slots[i]=Some(stack.clone())}validate(&slots,true)?;
        let mut staged=machine.clone();let mut inventory=player.clone();let mut tool=pocket.clone();
        let mut changes=PhysicalChanges{drops:Vec::new(),changed_tracked:Vec::new(),replaced_tracked:Vec::new(),missing:0};
        if creative {
            for old in staged.upgrades.iter().flatten(){changes.replaced_tracked.push(old.clone())}
            staged.upgrades=slots;for stack in staged.upgrades.iter_mut().flatten(){horizon_client::ae2_stack_identity::detach_if_tracked(stack);}
        }
        for slot in &mut staged.upgrades {
            let Some(mut removed)=slot.take() else{continue};changes.replaced_tracked.push(removed.clone());horizon_client::ae2_stack_identity::detach_if_tracked(&mut removed);
            add_to_player(&mut inventory,&mut removed,64,&mut changes.replaced_tracked);
            if removed.count>0{if let Some(tool)=tool.as_mut(){add_to_player(&mut tool.slots,&mut removed,64,&mut changes.replaced_tracked);}}
            if removed.count>0{changes.drops.push(removed)}
        }
        let need=saved.desired.len() as u32;let filter=ItemStack::new("ae2:speed_card",need.max(1));
        let mut found=take_from_slots(&mut inventory,&filter,need,&mut changes,false);
        let have:u32=found.iter().map(|s|s.count).sum();
        if have<need{if let Some(tool)=tool.as_mut(){found.extend(take_from_slots(&mut tool.slots,&filter,need-have,&mut changes,true));}}
        let mut placed=0;for stack in found {for _ in 0..stack.count{if placed>=5{return Err("Assembler upgrade capacity changed".into())}staged.upgrades[placed]=Some(moved_copy(&stack,1));placed+=1;}}
        changes.missing=need.saturating_sub(placed as u32);
        *machine=staged;*player=inventory;*pocket=tool;Ok(changes)
    }
    fn partial(source:&[ItemStack])->SavedAssembler{
        SavedAssembler{desired:source.iter().filter(|s|s.item.as_ref()=="ae2:speed_card").take(5).cloned().collect(),name:None}
    }
    pub(in crate::game) fn inscriber_to_assembler(source:&SavedInscriber)->SavedAssembler{partial(&source.desired)}
    pub(in crate::game) fn io_to_assembler(source:&SavedIo)->SavedAssembler{partial(&source.desired)}
    pub(in crate::game) fn vibration_to_assembler(source:&SavedVibration)->SavedAssembler{partial(&source.desired)}
    pub(in crate::game) fn interface_to_assembler()->SavedAssembler{SavedAssembler{desired:Vec::new(),name:None}}
    pub(in crate::game) fn assembler_to_inscriber(machine:&Machine,source:&SavedAssembler)->Result<SavedInscriber,String>{
        if machine.kind!=MachineKind::Inscriber{return Err("invalid cross-import target".into())}
        let mut settings=Compound::new();for (key,default,values) in FIELDS{settings.insert(key.into(),Tag::String(setting_value(machine,key,default,values)?.as_str().into()));}
        Ok(SavedInscriber{settings,desired:source.desired.iter().take(4).cloned().collect(),name:None})
    }
    pub(in crate::game) fn assembler_to_io(machine:&Machine,source:&SavedAssembler)->Result<SavedIo,String>{
        if machine.kind!=MachineKind::IoPort{return Err("invalid cross-import target".into())}
        let mut settings=Compound::new();for (key,default,values) in IO_FIELDS{settings.insert(key.into(),Tag::String(setting_value(machine,key,default,values)?.as_str().into()));}
        Ok(SavedIo{settings,desired:source.desired.iter().take(3).cloned().collect(),name:None})
    }
    pub(in crate::game) fn assembler_to_vibration(source:&SavedAssembler)->SavedVibration{
        SavedVibration{desired:source.desired.iter().take(3).cloned().collect(),name:None}
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn assembler_memory_preserves_inflight_state_and_physical_ownership(){
            let mut source=Machine::new(MachineKind::MolecularAssembler);for i in 0..5{source.upgrades[i]=Some(ItemStack::new("ae2:speed_card",1));}
            let mut card=ItemStack::new("ae2:memory_card",1);save_assembler(&mut card,&source,Some("source")).unwrap();let saved=read_assembler(&card).unwrap().unwrap();
            let mut target=source.clone();target.slots[0]=Some(ItemStack::new("minecraft:oak_log",3));target.energy=73.0;target.progress=49.0;target.config.insert("ae2_assembler_pattern".into(),Tag::String("pending".into()));
            let mut expected=target.clone();expected.upgrades=vec![None;5];let mut full=vec![Some(ItemStack::new("minecraft:stone",64));36];let mut pocket=None;
            let changes=apply_assembler(&mut target,&mut full,&mut pocket,&saved,false).unwrap();assert_eq!(changes.drops.len(),5);assert_eq!(changes.missing,5);assert_eq!(target,expected);assert_eq!(saved.name.as_deref(),Some("source"));
        }
        #[test]
        fn assembler_cross_kind_capacity_and_marked_target_cards(){
            let source=SavedAssembler{desired:vec![ItemStack::new("ae2:speed_card",1);5],name:Some("source".into())};
            let ins=Machine::new(MachineKind::Inscriber);assert_eq!(assembler_to_inscriber(&ins,&source).unwrap().desired.len(),4);
            let io=Machine::new(MachineKind::IoPort);assert_eq!(assembler_to_io(&io,&source).unwrap().desired.len(),3);assert_eq!(assembler_to_vibration(&source).desired.len(),3);
            let mut target=Machine::new(MachineKind::MolecularAssembler);let mut marked=ItemStack::new("ae2:speed_card",1);marked.data_mut().custom_name=Some(horizon_client::item_data::text_name("marked"));target.upgrades[0]=Some(marked.clone());target.upgrades[1]=Some(ItemStack::new("ae2:speed_card",1));
            let saved=SavedAssembler{desired:vec![ItemStack::new("ae2:speed_card",1)],name:None};let mut inventory=vec![None;36];let mut pocket=None;
            let changes=apply_assembler(&mut target,&mut inventory,&mut pocket,&saved,false).unwrap();assert_eq!(changes.missing,0);assert!(changes.drops.is_empty());assert!(inventory.iter().flatten().any(|s|s.same_item_same_components(&marked)));assert_eq!(target.upgrades.iter().flatten().count(),1);
        }
    }
}
pub(in crate::game) use assembler_memory::{SavedAssembler,save_assembler,read_assembler,apply_assembler,inscriber_to_assembler,io_to_assembler,vibration_to_assembler,interface_to_assembler,assembler_to_inscriber,assembler_to_io,assembler_to_vibration};
