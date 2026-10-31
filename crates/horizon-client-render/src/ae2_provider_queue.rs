//! Ordered, amount-based custody for a Pattern Provider's pending inputs.
//! AE2 19.2.18 PatternProviderLogic keeps its sendList in insertion order and
//! stores each unaccepted resource as one GenericStack (LGPL-3.0-or-later).
//!
//! This native persistence format uses an item prototype or a fluid key plus
//! a signed-long amount. It also reads the old physical-stack and opaque-fluid
//! lists; migration to the ordered form occurs only on a successful commit.
use crate::items::ItemStack;
use horizon_items::item_nbt as save;
use ae2_cell_core::ItemKey;
use horizon_world::nbt::{Compound,Tag};

pub const QUEUE:&str="ae2_provider_send_queue";
const OLD_ITEMS:&str="ae2_provider_send_items";
const OLD_FLUIDS:&str="ae2_provider_send_fluids";

#[derive(Clone,Debug,PartialEq)]
pub enum Entry {
    Item { prototype:ItemStack, amount:u64 },
    Fluid { key:ItemKey, amount:u64 },
}
impl Entry {
    pub fn amount(&self)->u64 {match self {Self::Item{amount,..}|Self::Fluid{amount,..}=>*amount}}
}
fn amount(tag:&Tag)->Result<u64,String> {
    match tag.get("amount") {Some(Tag::Long(value)) if *value>0=>Ok(*value as u64),_=>Err("invalid provider queue amount".into())}
}
fn item(entry:&Tag)->Result<Entry,String> {
    let amount=amount(entry)?;
    if entry.get("item").and_then(|item|item.get("count"))!=Some(&Tag::Int(1)) {return Err("invalid provider queue item count".into());}
    let prototype=entry.get("item").and_then(save::item_from_nbt).ok_or("invalid provider queue item")?;
    if prototype.count!=1 {return Err("provider queue item is not a prototype".into());}
    Ok(Entry::Item{prototype,amount})
}
fn fluid(entry:&Tag)->Result<Entry,String> {
    let amount=amount(entry)?;
    let (Some(Tag::String(id)),Some(Tag::ByteArray(components)))=(entry.get("fluid"),entry.get("components")) else{return Err("invalid provider queue fluid".into());};
    let id=id.to_string().map_err(|_|"invalid provider queue fluid id")?;
    if !valid_fluid_id(&id) {return Err("invalid provider queue fluid id".into());}
    Ok(Entry::Fluid{key:ItemKey{item_id:id,components:components.clone()},amount})
}
fn queue_entry(entry:&Tag)->Result<Entry,String> {
    match (entry.get("item"),entry.get("fluid")) {
        (Some(_),None)=>item(entry),
        (None,Some(_))=>fluid(entry),
        _=>Err("invalid provider queue entry".into()),
    }
}
fn list<'a>(config:&'a Compound,name:&str)->Result<Option<&'a Vec<Tag>>,String> {
    match config.get(&name.into()) {None=>Ok(None),Some(Tag::List(entries))=>Ok(Some(entries)),_=>Err(format!("invalid {name}"))}
}
/// An explicit new queue is authoritative. Mixed old and new nonempty queues
/// fail closed instead of dropping either representation's owned resources.
pub fn read(config:&Compound)->Result<Vec<Entry>,String> {
    let queue=list(config,QUEUE)?;let old_items=list(config,OLD_ITEMS)?;let old_fluids=list(config,OLD_FLUIDS)?;
    if let Some(queue)=queue {
        if old_items.is_some_and(|v|!v.is_empty())||old_fluids.is_some_and(|v|!v.is_empty()) {return Err("conflicting provider queues".into());}
        return queue.iter().map(queue_entry).collect();
    }
    let mut result=Vec::new();
    let old_len=old_items.map_or(0,Vec::len).saturating_add(old_fluids.map_or(0,Vec::len));
    result.try_reserve_exact(old_len).map_err(|_|"provider queue allocation failed")?;
    for entry in old_items.into_iter().flatten() {
        let count=match entry.get("count") {Some(Tag::Int(value)) if *value>0=>*value as u64,_=>return Err("invalid legacy provider item count".into())};
        let mut prototype=save::item_from_nbt(entry).ok_or("invalid legacy provider item")?;prototype.count=1;
        result.push(Entry::Item{prototype,amount:count});
    }
    for entry in old_fluids.into_iter().flatten() {result.push(fluid(entry)?);}
    Ok(result)
}
fn write_entry(entry:&Entry)->Result<Tag,String> {
    let amount=i64::try_from(entry.amount()).map_err(|_|"provider queue amount exceeds source long")?;
    if amount<=0 {return Err("invalid provider queue amount".into());}
    match entry {
        Entry::Item{prototype,..} if prototype.count==1=>Ok(Tag::compound([("item",save::item_to_nbt(prototype)),("amount",Tag::Long(amount))])),
        Entry::Item{..}=>Err("provider queue item is not a prototype".into()),
        Entry::Fluid{key,..} if valid_fluid_id(&key.item_id)=>Ok(Tag::compound([
            ("fluid",Tag::String(key.item_id.as_str().into())),("components",Tag::ByteArray(key.components.clone())),("amount",Tag::Long(amount))])),
        Entry::Fluid{..}=>Err("invalid provider queue fluid id".into()),
    }
}
/// Prepare every record before touching the stored data, so an error leaves
/// old item/fluid custody intact.
pub fn write(config:&mut Compound,entries:&[Entry])->Result<(),String> {
    let values=entries.iter().map(write_entry).collect::<Result<Vec<_>,_>>()?;
    config.remove(&OLD_ITEMS.into());config.remove(&OLD_FLUIDS.into());
    if values.is_empty(){config.remove(&QUEUE.into());}else{config.insert(QUEUE.into(),Tag::List(values));}
    Ok(())
}
/// Materialize item custody as NBT-safe logical stacks. Fluids remain
/// represented by their generic queue entry and do not become item drops.
pub fn physical_items(entries:&[Entry])->Result<Vec<ItemStack>,String> {
    let max=i32::MAX as u64;
    let mut chunks=0usize;
    for entry in entries {
        if let Entry::Item{amount,..}=entry {
            let n=amount/max+u64::from(amount%max!=0);
            chunks=chunks.checked_add(usize::try_from(n).map_err(|_|"provider drop allocation overflow")?).ok_or("provider drop allocation overflow")?;
        }
    }
    let mut result=Vec::new();result.try_reserve_exact(chunks).map_err(|_|"provider drop allocation failed")?;
    for entry in entries {
        if let Entry::Item{prototype,amount}=entry {
            let mut left=*amount;
            while left>0 {
                let count=left.min(max);result.push(ItemStack{count:count as u32,..prototype.clone()});left-=count;
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordered_queue_roundtrips_large_partial_amounts_and_components() {
        let mut prototype=ItemStack::new("minecraft:stone",1);
        prototype.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("variant",Tag::String("kept".into()))])));
        let water=ItemKey{item_id:"minecraft:water".into(),components:vec![1,2,3]};
        let entries=vec![Entry::Fluid{key:water.clone(),amount:1000},Entry::Item{prototype:prototype.clone(),amount:999_999},Entry::Fluid{key:water,amount:500}];
        let mut config=Compound::default();write(&mut config,&entries).unwrap();
        assert_eq!(read(&config).unwrap(),entries);
        let encoded=horizon_world::nbt::write_network(&Tag::Compound(config.clone()),Default::default()).unwrap();
        let (Tag::Compound(restored),used)=horizon_world::nbt::read_network(&encoded,Default::default()).unwrap() else{panic!("invalid roundtrip")};
        assert_eq!(used,encoded.len());assert_eq!(read(&restored).unwrap(),entries);
        assert_eq!(entries[1].amount(),999_999);assert_eq!(prototype.count,1);
        let physical=physical_items(&entries).unwrap();assert_eq!(physical.len(),1);assert_eq!(physical[0].count,999_999);
        assert!(physical[0].same_item_same_components(&prototype));
    }
    #[test]
    fn legacy_data_migrates_without_losing_opaque_fluid_or_item_custody() {
        let stack=ItemStack::new("minecraft:stone",64);
        let mut config=Compound::default();config.insert(OLD_ITEMS.into(),Tag::List(vec![save::item_to_nbt(&stack)]));
        config.insert(OLD_FLUIDS.into(),Tag::List(vec![Tag::compound([("fluid",Tag::String("minecraft:water".into())),("components",Tag::ByteArray(vec![1,2])),("amount",Tag::Long(500))])]));
        let decoded=read(&config).unwrap();assert_eq!(decoded.len(),2);assert_eq!(decoded[0].amount(),64);assert_eq!(decoded[1].amount(),500);
        write(&mut config,&decoded).unwrap();assert!(!config.contains_key(&OLD_ITEMS.into()));assert!(!config.contains_key(&OLD_FLUIDS.into()));assert_eq!(read(&config).unwrap(),decoded);
    }
    #[test]
    fn conflicting_or_invalid_queues_never_overwrite_resources() {
        let mut config=Compound::default();config.insert(OLD_ITEMS.into(),Tag::List(vec![save::item_to_nbt(&ItemStack::new("minecraft:stone",2))]));
        config.insert(QUEUE.into(),Tag::List(vec![Tag::compound([("bad",Tag::Int(1))])]));
        let before=config.clone();assert!(read(&config).is_err());assert_eq!(config,before);
        let bad=[Entry::Item{prototype:ItemStack::new("minecraft:stone",1),amount:u64::MAX}];
        assert!(write(&mut config,&bad).is_err());assert_eq!(config,before);
    }
}

/// A fluid's resource location as AE2's keys accept it.
pub fn valid_fluid_id(id: &str) -> bool {
    let Some((namespace, path)) = id.split_once(':') else {
        return false;
    };
    !namespace.is_empty()
        && !path.is_empty()
        && namespace
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_.-".contains(&c))
        && path
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_.-/".contains(&c))
        && id != "minecraft:empty"
}
