// Source-aligned Cell Workbench template and physical-cell handoff.
const WORKBENCH_FILTERS:&str="ae2_workbench_filters";
const WORKBENCH_MODE:&str="ae2_workbench_copy_mode";

fn workbench_keep(machine:&horizon_client::ae2_world::Machine)->bool {
    text(&machine.config,WORKBENCH_MODE).as_deref()==Some("keep_on_remove")
}
fn workbench_decode(entries:&[Tag])->Result<Vec<Option<Tag>>,String>{
    let mut slots=horizon_client::ae2_cell_codec::decode_view_filter_slots(entries)?;
    for tag in slots.iter_mut().flatten(){if let Tag::Compound(c)=tag{c.remove(&"Slot".into());}}
    Ok(slots)
}
fn workbench_filters(machine:&horizon_client::ae2_world::Machine)->Result<Vec<Option<Tag>>,String>{
    workbench_filters_checked(machine,&|_,_|true)
}
fn workbench_filters_checked(machine:&horizon_client::ae2_world::Machine,known:&dyn Fn(&str,bool)->bool)->Result<Vec<Option<Tag>>,String>{
    let entries=match machine.config.get(&WORKBENCH_FILTERS.into()) {
        Some(Tag::List(entries))=>entries.clone(),
        Some(_)=>return Err("invalid workbench filters".into()),
        None=>match machine.slots.first().and_then(Option::as_ref) {
            None=>Vec::new(),
            Some(cell)=>match horizon_client::ae2_cell_codec::cell_settings_checked(cell,known)?.get(&"filters".into()) {
                Some(Tag::List(entries))=>entries.clone(),
                _=>return Err("invalid cell filters".into()),
            }
        }
    };
    let mut slots=workbench_decode(&entries)?;
    // Older native Workbench templates can carry item tags independently of
    // a cell. Recover removed registry keys here too, preserving the original
    // key so a later copy into a cell remains reversible.
    for entry in slots.iter_mut().flatten() {
        let Some(item)=entry.get("item").and_then(horizon_client::save::item_from_nbt) else {continue;};
        if item.item.as_ref()=="ae2:missing_content" || known(&item.item,false) {continue;}
        let generic=horizon_client::ae2_generic_stack::GenericStack{key:ae2_lab::key(&item)?,fluid:false,amount:0};
        let raw=horizon_client::ae2_generic_stack::encode(&generic)?;
        let missing=horizon_client::ae2_generic_stack::decode_tolerant_checked(&raw,known)?.ok_or("invalid missing filter")?;
        let item=horizon_client::ae2_cell_codec::prototype(&missing.key)?;
        let Tag::Compound(data)=entry else{return Err("invalid workbench filter".into());};
        data.insert("item".into(),horizon_client::save::item_to_nbt(&item));
    }
    Ok(slots)
}
fn workbench_store_filters(machine:&mut horizon_client::ae2_world::Machine,slots:&[Option<Tag>])->Result<(),String>{
    let entries=horizon_client::ae2_cell_codec::encode_view_filter_slots(slots)?;
    machine.config.insert(WORKBENCH_FILTERS.into(),Tag::List(entries));Ok(())
}
// EMPTY_ITEM is a right-click action, not a generic conversion of every
// bucket placed over a fluid-only filter. Left and drag actions use the item.
fn workbench_emptying_mode(cell:Option<&ItemStack>,button:i32)->bool {
    button==1 && cell.is_none_or(|cell|is_fluid(cell) || matches!(cell.item.as_ref(),"ae2:view_cell"|"ae2:creative_storage_cell"))
}
fn workbench_filter_allowed(cell:&ItemStack,entry:&Tag)->bool{
    if entry.get("item").is_some() {
        return !is_fluid(cell) && entry.get("item").and_then(horizon_client::save::item_from_nbt)
            .is_some_and(|s|s.count>0);
    }
    if let (Some(Tag::String(id)),Some(Tag::ByteArray(_)))=(entry.get("fluid"),entry.get("components")){
        return (is_fluid(cell)||matches!(cell.item.as_ref(),"ae2:view_cell"|"ae2:creative_storage_cell"))
            && id.to_string().ok().is_some_and(|id|ae2_fluids::valid_fluid_id(&id));
    }
    false
}
fn workbench_apply_filters(cell:&mut ItemStack,slots:&[Option<Tag>])->Result<Vec<Option<Tag>>,String>{
    workbench_apply_filters_checked(cell,slots,&|_,_|true)
}
fn workbench_apply_filters_checked(cell:&mut ItemStack,slots:&[Option<Tag>],known:&dyn Fn(&str,bool)->bool)->Result<Vec<Option<Tag>>,String>{
    if slots.len()!=63{return Err("invalid workbench filter size".into())}
    let accepted:Vec<_>=slots.iter().map(|entry|entry.clone().filter(|v|workbench_filter_allowed(cell,v))).collect();
    let mut settings=horizon_client::ae2_cell_codec::cell_settings_checked(cell,known)?;
    settings.insert("filters".into(),Tag::List(horizon_client::ae2_cell_codec::encode_view_filter_slots(&accepted)?));
    let mut staged=cell.clone();set_cell_settings(&mut staged,settings)?;
    if !is_valid_workbench_cell(&staged){return Err("invalid filtered workbench cell".into())}
    *cell=staged;Ok(accepted)
}
/// Invoke after a physical slot-zero change, using the previous workbench as
/// the fallback for legacy saves lacking a persistent template component.
pub(super) fn workbench_cell_changed(before:&horizon_client::ae2_world::Machine,after:&mut horizon_client::ae2_world::Machine)->Result<(),String>{
    workbench_cell_changed_checked(before,after,&|_,_|true)
}
fn workbench_cell_changed_checked(before:&horizon_client::ae2_world::Machine,after:&mut horizon_client::ae2_world::Machine,known:&dyn Fn(&str,bool)->bool)->Result<(),String>{
    if before.kind!=MachineKind::CellWorkbench||after.kind!=MachineKind::CellWorkbench{return Err("not a workbench".into())}
    let mut filters=workbench_filters_checked(before,known)?;
    if let Some(cell)=after.slots.first_mut().and_then(Option::as_mut) {
        if !is_valid_workbench_cell(cell){return Err("invalid workbench cell".into())}
        let settings=horizon_client::ae2_cell_codec::cell_settings_checked(cell,known)?;
        let Some(Tag::List(entries))=settings.get(&"filters".into()) else{return Err("invalid cell filters".into())};
        let installed=workbench_decode(entries)?;
        if installed.iter().any(Option::is_some){filters=installed}
        else{filters=workbench_apply_filters_checked(cell,&filters,known)?;}
    }else if !workbench_keep(after){filters.fill(None);}
    workbench_store_filters(after,&filters)
}
fn workbench_partition(machine:&mut horizon_client::ae2_world::Machine)->Result<(),String>{
    workbench_partition_checked(machine,&|_,_|true)
}
fn workbench_partition_checked(machine:&mut horizon_client::ae2_world::Machine,known:&dyn Fn(&str,bool)->bool)->Result<(),String>{
    let mut filters=vec![None;63];
    if let Some(cell)=machine.slots.first().and_then(Option::as_ref){
        let storage=if is_fluid(cell){horizon_client::ae2_cell_codec::read_fluid_cell_checked(cell,known)}else{horizon_client::ae2_cell_codec::read_cell_checked(cell,known)};
        if let Ok(storage)=storage {
            for (slot,(key,_)) in filters.iter_mut().zip(storage.entries()){
                *slot=Some(if is_fluid(cell){
                    Tag::compound([("fluid",Tag::String(key.item_id.as_str().into())),("components",Tag::ByteArray(key.components.clone()))])
                }else{Tag::compound([("item",horizon_client::save::item_to_nbt(&ae2_lab::prototype(key)?))])});
            }
        }else if let Ok(keys)=horizon_client::ae2_cell_codec::creative_cell_keys_checked(cell,known) {
            for (slot,key) in filters.iter_mut().zip(keys) {
                *slot=Some(match key {
                    CreativeCellKey::Item(key)=>Tag::compound([("item",horizon_client::save::item_to_nbt(&ae2_lab::prototype(&key)?))]),
                    CreativeCellKey::Fluid(key)=>Tag::compound([("fluid",Tag::String(key.item_id.as_str().into())),("components",Tag::ByteArray(key.components))]),
                });
            }
        }
    }
    if let Some(cell)=machine.slots.first_mut().and_then(Option::as_mut){filters=workbench_apply_filters_checked(cell,&filters,known)?;}
    workbench_store_filters(machine,&filters)
}

/// Convert the player's carried physical stack into a type-only ghost. A
/// wrapped stack selects its real key and channel, including fluid components;
/// the carried item itself is never consumed.
fn workbench_ghost_from_carried(cell:Option<&ItemStack>,held:&ItemStack,fluid_mode:bool,known:&dyn Fn(&str,bool)->bool)->Result<Tag,String>{
    let item_allowed=cell.is_none_or(|cell|!is_fluid(cell));
    let fluid_allowed=cell.is_none_or(|cell|is_fluid(cell)||matches!(cell.item.as_ref(),"ae2:view_cell"|"ae2:creative_storage_cell"));
    if let Some(value)=horizon_client::ae2_generic_stack::unwrap_item_stack_checked(held,known)? {
        if value.fluid && fluid_allowed {
            return Ok(Tag::compound([("fluid",Tag::String(value.key.item_id.as_str().into())),("components",Tag::ByteArray(value.key.components))]));
        }
        if !value.fluid && item_allowed {
            let mut item=horizon_client::ae2_cell_codec::prototype(&value.key)?;item.count=1;
            return Ok(Tag::compound([("item",horizon_client::save::item_to_nbt(&item))]));
        }
        return Err("wrapped key is incompatible with cell".into());
    }
    if fluid_mode && fluid_allowed {
        let id=ae2_fluids::bucket_fluid(&held.item).ok_or("use a supported fluid bucket")?;
        return Ok(Tag::compound([("fluid",Tag::String(id.into())),("components",Tag::ByteArray(vec![]))]));
    }
    if item_allowed {
        let mut item=held.clone();item.count=1;
        return Ok(Tag::compound([("item",horizon_client::save::item_to_nbt(&item))]));
    }
    Err("item key is incompatible with cell".into())
}

#[cfg(test)]
mod workbench_template_tests {
    use super::*;
    use horizon_client::ae2_world::Machine;
    fn item_filter(id:&str)->Tag{Tag::compound([("item",horizon_client::save::item_to_nbt(&ItemStack::new(id,1)))])}
    fn one(id:&str,slot:usize)->Vec<Option<Tag>>{let mut out=vec![None;63];out[slot]=Some(item_filter(id));out}
    #[test]
    fn filled_bucket_is_fluid_only_for_a_right_click_emptying_action() {
        let cell=ItemStack::new("ae2:fluid_storage_cell_1k",1);
        let bucket=ItemStack::new("minecraft:water_bucket",1);
        assert!(!workbench_emptying_mode(Some(&cell),0));
        assert!(workbench_ghost_from_carried(Some(&cell),&bucket,workbench_emptying_mode(Some(&cell),0),&|_,_|true).is_err());
        assert!(workbench_emptying_mode(Some(&cell),1));
        let ghost=workbench_ghost_from_carried(Some(&cell),&bucket,workbench_emptying_mode(Some(&cell),1),&|_,_|true).unwrap();
        assert_eq!(ghost.get("fluid"),Some(&Tag::String("minecraft:water".into())));
        assert_eq!(bucket.count,1);
    }
    #[test]
    fn wrapped_fluid_and_item_filters_use_contents_without_consuming_wrapper() {
        use horizon_client::ae2_generic_stack::{GenericStack,wrap};
        let patch=Tag::compound([("minecraft:custom_data",Tag::compound([("variant",Tag::String("warm".into()))]))]);
        let components=horizon_world::nbt::write_network(&patch,horizon_world::nbt::Limits::default()).unwrap();
        let fluid=GenericStack{key:ae2_cell_core::ItemKey{item_id:"minecraft:water".into(),components},fluid:true,amount:1250};
        let wrapped=wrap(&fluid).unwrap();let cell=ItemStack::new("ae2:fluid_storage_cell_1k",1);
        let ghost=workbench_ghost_from_carried(Some(&cell),&wrapped,false,&|_,_|true).unwrap();
        assert_eq!(ghost.get("fluid"),Some(&Tag::String("minecraft:water".into())));assert_eq!(wrapped.count,1);
        let mut physical_cell=cell.clone();let mut slots=vec![None;63];slots[0]=Some(ghost.clone());
        let committed=workbench_apply_filters_checked(&mut physical_cell,&slots,&|_,_|true).unwrap();
        assert!(committed[0].is_some());
        let source=physical_cell.data().unwrap().other("ae2:storage_cell_config_inv").unwrap();
        let Tag::List(source)=source else{panic!("source config list")};
        assert_eq!(horizon_client::ae2_generic_stack::decode(&source[0]).unwrap().unwrap().key,fluid.key);
        let item=ItemStack::new("minecraft:diamond",4);let value=GenericStack{key:ae2_lab::key(&item).unwrap(),fluid:false,amount:4};
        let wrapped=wrap(&value).unwrap();assert!(workbench_ghost_from_carried(Some(&cell),&wrapped,false,&|_,_|true).is_err());
        let item_cell=ItemStack::new("ae2:item_storage_cell_1k",1);
        let ghost=workbench_ghost_from_carried(Some(&item_cell),&wrapped,false,&|_,_|true).unwrap();
        assert_eq!(ghost.get("item").and_then(|v|v.get("id")),Some(&Tag::String("minecraft:diamond".into())));
        assert_eq!(ghost.get("item").and_then(|v|v.get("count")),Some(&Tag::Int(1)));
    }
    #[test]
    fn registry_aware_legacy_template_preserves_a_removed_item_key() {
        let mut before=Machine::new(MachineKind::CellWorkbench);
        before.config.insert(WORKBENCH_MODE.into(),Tag::String("keep_on_remove".into()));
        workbench_store_filters(&mut before,&one("removed:ore",4)).unwrap();
        let known=|id:&str,fluid:bool|fluid||id!="removed:ore";
        let filters=workbench_filters_checked(&before,&known).unwrap();
        let item=filters[4].as_ref().unwrap().get("item").and_then(horizon_client::save::item_from_nbt).unwrap();
        assert_eq!(item.item.as_ref(),"ae2:missing_content");
        assert!(item.data().unwrap().other("ae2:missing_content_aekey_data").is_some());
        let mut after=before.clone();after.slots[0]=Some(ItemStack::new("ae2:item_storage_cell_1k",1));
        workbench_cell_changed_checked(&before,&mut after,&known).unwrap();
        let cell=after.slots[0].as_ref().unwrap();
        let Tag::List(source)=cell.data().unwrap().other("ae2:storage_cell_config_inv").unwrap() else{panic!("source filters")};
        assert_eq!(source[4].get("id"),Some(&Tag::String("removed:ore".into())));
    }
    #[test]
    fn registry_aware_partition_keeps_missing_content_key_recoverable() {
        let raw=Tag::compound([("#t",Tag::String("ae2:i".into())),("id",Tag::String("removed:ore".into())),("#",Tag::Long(2))]);
        let mut cell=ItemStack::new("ae2:item_storage_cell_1k",1);cell.data_mut().set_other("ae2:storage_cell_inv",Some(Tag::List(vec![raw])));
        let mut machine=Machine::new(MachineKind::CellWorkbench);machine.slots[0]=Some(cell);
        let known=|id:&str,fluid:bool|fluid||id!="removed:ore";
        workbench_partition_checked(&mut machine,&known).unwrap();
        let filters=workbench_filters_checked(&machine,&known).unwrap();
        let item=filters.iter().flatten().next().unwrap().get("item").and_then(horizon_client::save::item_from_nbt).unwrap();
        assert_eq!(item.item.as_ref(),"ae2:missing_content");
        assert!(item.data().unwrap().other("ae2:missing_content_aekey_data").is_some());
    }
    #[test]
    fn keep_template_copies_empty_cells_and_nonempty_incoming_cell_wins(){
        let mut before=Machine::new(MachineKind::CellWorkbench);
        before.config.insert(WORKBENCH_MODE.into(),Tag::String("keep_on_remove".into()));
        let template=one("minecraft:diamond",5);workbench_store_filters(&mut before,&template).unwrap();
        let mut inserted=before.clone();inserted.slots[0]=Some(ItemStack::new("ae2:item_storage_cell_1k",1));
        workbench_cell_changed(&before,&mut inserted).unwrap();assert_eq!(workbench_filters(&inserted).unwrap(),template);
        let mut removed=inserted.clone();removed.slots[0]=None;workbench_cell_changed(&inserted,&mut removed).unwrap();
        assert_eq!(workbench_filters(&removed).unwrap(),template);
        let mut next=removed.clone();next.slots[0]=Some(ItemStack::new("ae2:item_storage_cell_4k",1));workbench_cell_changed(&removed,&mut next).unwrap();
        assert_eq!(workbench_filters(&next).unwrap(),template);
        let emerald=one("minecraft:emerald",2);let mut other=ItemStack::new("ae2:item_storage_cell_1k",1);workbench_apply_filters(&mut other,&emerald).unwrap();
        let mut replaced=next.clone();replaced.slots[0]=Some(other);workbench_cell_changed(&next,&mut replaced).unwrap();
        assert_eq!(workbench_filters(&replaced).unwrap(),emerald);
    }
    #[test]
    fn clear_mode_removal_discards_only_template(){
        let mut before=Machine::new(MachineKind::CellWorkbench);before.slots[0]=Some(ItemStack::new("ae2:item_storage_cell_1k",1));
        workbench_store_filters(&mut before,&one("minecraft:diamond",3)).unwrap();
        let mut removed=before.clone();let physical=removed.slots[0].take().unwrap();workbench_cell_changed(&before,&mut removed).unwrap();
        assert!(workbench_filters(&removed).unwrap().iter().all(Option::is_none));assert_eq!(physical.item.as_ref(),"ae2:item_storage_cell_1k");
    }
    #[test]
    fn partition_reads_real_cell_keys_and_keeps_physical_stock(){
        let mut item=ItemStack::new("ae2:item_storage_cell_1k",1);let mut storage=read(&item).unwrap();
        for (id,count) in [("minecraft:diamond",4),("minecraft:gold_ingot",3)] {let key=ae2_lab::key(&ItemStack::new(id,1)).unwrap();assert_eq!(storage.insert(&key,count,Action::Modulate),count);}
        write(&mut item,&storage).unwrap();let mut machine=Machine::new(MachineKind::CellWorkbench);machine.slots[0]=Some(item);
        workbench_partition(&mut machine).unwrap();let filters=workbench_filters(&machine).unwrap();
        let keys:Vec<_>=filters.iter().flatten().filter_map(|v|v.get("item").and_then(horizon_client::save::item_from_nbt)).map(|s|s.item.to_string()).collect();
        assert_eq!(keys.len(),2);assert!(keys.contains(&"minecraft:diamond".into())&&keys.contains(&"minecraft:gold_ingot".into()));
        assert_eq!(read(machine.slots[0].as_ref().unwrap()).unwrap().total_items(),7);
    }
    #[test]
    fn template_copy_filters_incompatible_item_keys_from_fluid_cell(){
        let mut before=Machine::new(MachineKind::CellWorkbench);before.config.insert(WORKBENCH_MODE.into(),Tag::String("keep_on_remove".into()));
        let mut filters=one("minecraft:diamond",0);filters[4]=Some(Tag::compound([("fluid",Tag::String("minecraft:water".into())),("components",Tag::ByteArray(vec![]))]));workbench_store_filters(&mut before,&filters).unwrap();
        let mut inserted=before.clone();inserted.slots[0]=Some(ItemStack::new("ae2:fluid_storage_cell_1k",1));workbench_cell_changed(&before,&mut inserted).unwrap();
        let filtered=workbench_filters(&inserted).unwrap();assert!(filtered[0].is_none());assert_eq!(filtered[4].as_ref().unwrap().get("fluid"),Some(&Tag::String("minecraft:water".into())));
    }
}
