//! AE2 GenericStack persistent codec, derived from GenericStack, AEItemKey and
//! AEFluidKey in 19.2.18 (LGPL-3.0-or-later). Empty maps are nullable slots.
use ae2_cell_core::ItemKey;
use horizon_world::nbt::{self, Limits, Tag};
use crate::{ae2_cell_codec, save};

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct GenericStack { pub key:ItemKey, pub fluid:bool, pub amount:i64 }

pub fn decode(tag:&Tag)->Result<Option<GenericStack>,String> {
    let Tag::Compound(data)=tag else{return Err("generic stack is not a compound".into());};
    if data.is_empty(){return Ok(None);}
    let Some(Tag::String(kind))=tag.get("#t") else{return Err("missing generic stack type".into());};
    let kind=kind.to_string().map_err(|_|"invalid generic stack type")?;
    let fluid=match kind.as_str(){"ae2:i"=>false,"ae2:f"=>true,_=>return Err("unsupported generic stack type".into())};
    let Some(Tag::String(id))=tag.get("id") else{return Err("missing generic stack id".into());};
    let id=id.to_string().map_err(|_|"invalid generic stack id")?;
    let id=if id.contains(':') {id} else {format!("minecraft:{id}")};
    if !ae2_cell_codec::valid_fluid_id(&id) || id=="minecraft:air" {return Err("invalid generic stack id".into());}
    let amount=save::number(tag.get("#")).ok_or("invalid generic stack amount")?;
    let components=match tag.get("components") {None=>None,Some(Tag::Compound(data))=>Some(data.clone()),_=>return Err("invalid generic component patch".into())};
    let key=if fluid {
        let components=match components {None=>Vec::new(),Some(data) if data.is_empty()=>Vec::new(),Some(data)=>
            nbt::write_network(&Tag::Compound(data),Limits::default()).map_err(|error|error.to_string())?};
        ItemKey{item_id:id,components}
    } else {
        let mut prototype=match Tag::compound([("id",Tag::String(id.as_str().into())),("count",Tag::Int(1))]) {Tag::Compound(m)=>m,_=>unreachable!()};
        if let Some(components)=components {prototype.insert("components".into(),Tag::Compound(components));}
        let item=save::item_from_nbt(&Tag::Compound(prototype)).ok_or("invalid generic item")?;
        ae2_cell_codec::key(&item)?
    };
    Ok(Some(GenericStack{key,fluid,amount}))
}

/// Source's fault-tolerant codec represents an unreadable key or stack as a
/// physical Missing Content item and keeps the original NBT for re-encoding.
/// Callers with a registry can reject unknown item/fluid IDs via `known`.
pub fn decode_tolerant_checked(tag:&Tag,known:impl Fn(&str,bool)->bool)->Result<Option<GenericStack>,String> {
    let result=decode(tag).and_then(|value|match value {
        Some(value) if !known(&value.key.item_id,value.fluid)=>Err("unknown generic stack id".into()),
        value=>Ok(value),
    });
    match result {
        Ok(value)=>Ok(value),
        Err(error)=>{
            let amount=save::number(tag.get("#"));
            let mut missing=crate::items::ItemStack::new("ae2:missing_content",1);
            if let Tag::Compound(raw)=tag {
                let component=if amount.is_some(){"ae2:missing_content_aekey_data"}else{"ae2:missing_content_itemstack_data"};
                missing.data_mut().set_other(component,Some(Tag::Compound(raw.clone())));
            }
            missing.data_mut().set_other("ae2:missing_content_error",Some(Tag::String(error.as_str().into())));
            Ok(Some(GenericStack{key:ae2_cell_codec::key(&missing)?,fluid:false,amount:amount.unwrap_or(1)}))
        }
    }
}
pub fn decode_tolerant(tag:&Tag)->Result<Option<GenericStack>,String> {decode_tolerant_checked(tag,|_,_|true)}

pub fn encode(stack:&GenericStack)->Result<Tag,String> {
    if !ae2_cell_codec::valid_fluid_id(&stack.key.item_id) || stack.key.item_id=="minecraft:air" {return Err("invalid generic stack id".into());}
    let mut data=if stack.fluid {
        let mut data=match Tag::compound([("id",Tag::String(stack.key.item_id.as_str().into()))]) {Tag::Compound(m)=>m,_=>unreachable!()};
        if !stack.key.components.is_empty() {
            let (tag,used)=nbt::read_network(&stack.key.components,Limits::default()).map_err(|error|error.to_string())?;
            if used!=stack.key.components.len() || !matches!(tag,Tag::Compound(_)) {return Err("unrepresentable fluid component patch".into());}
            data.insert("components".into(),tag);
        }
        data
    } else {
        let item=ae2_cell_codec::prototype(&stack.key)?;
        if item.item.as_ref()=="ae2:missing_content" {
            if let Some(original)=item.data().and_then(|data|data.other("ae2:missing_content_itemstack_data")) {
                return Ok(original.clone());
            }
            if let Some(Tag::Compound(original))=item.data().and_then(|data|data.other("ae2:missing_content_aekey_data")) {
                let mut original=original.clone();original.insert("#".into(),Tag::Long(stack.amount));return Ok(Tag::Compound(original));
            }
        }
        let Tag::Compound(mut data)=save::item_to_nbt(&item) else{return Err("invalid generic item".into());};
        data.remove(&"count".into());data
    };
    data.insert("#t".into(),Tag::String(if stack.fluid {"ae2:f"} else {"ae2:i"}.into()));
    data.insert("#".into(),Tag::Long(stack.amount));
    crate::ae2_components::normalize("ae2:wrapped_stack",Tag::Compound(data))
}

/// Decode the source WrappedGenericStack component. A legacy custom-data
/// representation remains readable for already-saved native stacks.
pub fn unwrap_item_stack(stack:&crate::items::ItemStack)->Result<Option<GenericStack>,String> {
    unwrap_item_stack_checked(stack,&|_,_|true)
}
pub fn unwrap_item_stack_checked(stack:&crate::items::ItemStack,known:&dyn Fn(&str,bool)->bool)->Result<Option<GenericStack>,String> {
    if stack.item.as_ref()!="ae2:wrapped_generic_stack" || stack.count==0 {return Ok(None);}
    let Some(data)=stack.data() else{return Ok(None);};
    let source=data.other("ae2:wrapped_stack").or_else(||data.other("minecraft:custom_data").and_then(|custom|custom.get("ae2:wrapped_stack")));
    let Some(source)=source else{return Ok(None);};
    if save::number(source.get("#")).is_none() {
        return Err("invalid wrapped amount".into());
    }
    decode_tolerant_checked(source,known)
}
/// Source GenericStack.fromItemStack unwraps a valid wrapper first and uses
/// its contained amount; a degenerate wrapper remains an ordinary item.
pub fn from_item_stack(stack:&crate::items::ItemStack)->Result<Option<GenericStack>,String> {
    from_item_stack_checked(stack,&|_,_|true)
}
pub fn from_item_stack_checked(stack:&crate::items::ItemStack,known:&dyn Fn(&str,bool)->bool)->Result<Option<GenericStack>,String> {
    if stack.count==0 {return Ok(None);}
    if let Some(value)=unwrap_item_stack_checked(stack,known)? {return Ok(Some(value));}
    Ok(Some(GenericStack{key:ae2_cell_codec::key(stack)?,fluid:false,amount:i64::from(stack.count)}))
}
/// Build the real source wrapper used for generic ghost slots. Its amount can
/// be zero (CONFIG_TYPES) without becoming an empty physical ItemStack.
pub fn wrap(stack:&GenericStack)->Result<crate::items::ItemStack,String> {
    let mut wrapped=crate::items::ItemStack::new("ae2:wrapped_generic_stack",1);
    wrapped.data_mut().set_other("ae2:wrapped_stack",Some(encode(stack)?));
    Ok(wrapped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::ItemStack;
    #[test]
    fn item_and_fluid_components_and_empty_slots_roundtrip() {
        let mut item=ItemStack::new("minecraft:diamond_sword",1);item.damage=4;item.data_mut().custom_name=Some("Physical".into());
        let key=ae2_cell_codec::key(&item).unwrap();let stack=GenericStack{key,fluid:false,amount:123};
        assert_eq!(decode(&encode(&stack).unwrap()).unwrap(),Some(stack));
        let components=Tag::compound([("test:variant",Tag::String("warm".into()))]);
        let bytes=nbt::write_network(&components,Limits::default()).unwrap();
        let fluid=GenericStack{key:ItemKey{item_id:"minecraft:water".into(),components:bytes},fluid:true,amount:1000};
        assert_eq!(decode(&encode(&fluid).unwrap()).unwrap(),Some(fluid));
        assert_eq!(decode(&Tag::Compound(Default::default())).unwrap(),None);
    }
    #[test]
    fn raw_numeric_amounts_follow_source_number_conversion_and_default_namespace() {
        for (number,expected) in [(Tag::Byte(7),7),(Tag::Short(19),19),(Tag::Int(35),35),(Tag::Float((41.75f32).to_bits()),41),(Tag::Double((5000.875f64).to_bits()),5000)] {
            let raw=Tag::compound([("#t",Tag::String("ae2:f".into())),("id",Tag::String("water".into())),("#",number)]);
            let decoded=decode(&raw).unwrap().unwrap();assert_eq!(decoded.key.item_id,"minecraft:water");assert_eq!(decoded.amount,expected);
        }
    }
    #[test]
    fn malformed_and_unrepresentable_payloads_fail_without_inventing_contents() {
        assert!(decode(&Tag::compound([("#t",Tag::String("ae2:i".into())),("id",Tag::String("minecraft:stone".into()))])).is_err());
        let fluid=GenericStack{key:ItemKey{item_id:"minecraft:water".into(),components:vec![1,2,3]},fluid:true,amount:1000};
        assert!(encode(&fluid).is_err());
    }
    #[test]
    fn wrapped_values_use_inner_type_components_and_amount_without_consuming_wrapper() {
        let patch=Tag::compound([("minecraft:custom_data",Tag::compound([("variant",Tag::String("warm".into()))]))]);
        let fluid=GenericStack{key:ItemKey{item_id:"minecraft:water".into(),components:nbt::write_network(&patch,Limits::default()).unwrap()},fluid:true,amount:1250};
        let mut wrapper=wrap(&fluid).unwrap();
        wrapper.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("ae2:wrapped_stack",Tag::compound([("#t",Tag::String("ae2:i".into())),("id",Tag::String("minecraft:diamond".into())),("#",Tag::Long(8))]))])));
        assert_eq!(wrapper.count,1);assert_eq!(from_item_stack(&wrapper).unwrap(),Some(fluid.clone()),"source component wins over conflicting legacy data");
        let zero=GenericStack{amount:0,..fluid};assert_eq!(unwrap_item_stack(&wrap(&zero).unwrap()).unwrap(),Some(zero));
        let ordinary=ItemStack::new("minecraft:stone",4);let value=from_item_stack(&ordinary).unwrap().unwrap();
        assert!(!value.fluid);assert_eq!(value.key.item_id,"minecraft:stone");assert_eq!(value.amount,4);
        let broken=ItemStack::new("ae2:wrapped_generic_stack",1);assert_eq!(from_item_stack(&broken).unwrap().unwrap().key.item_id,"ae2:wrapped_generic_stack");
        let raw=Tag::compound([("#t",Tag::String("ae2:i".into())),("id",Tag::String("removed:ore".into())),("#",Tag::Long(5))]);
        let mut missing=broken;missing.data_mut().set_other("ae2:wrapped_stack",Some(raw.clone()));
        let recovered=unwrap_item_stack_checked(&missing,&|id,fluid|fluid||id!="removed:ore").unwrap().unwrap();
        assert_eq!(recovered.key.item_id,"ae2:missing_content");assert_eq!(encode(&recovered).unwrap(),raw);
    }
    #[test]
    fn tolerant_missing_content_preserves_original_stack_and_key_data() {
        let missing_amount=Tag::compound([("#t",Tag::String("ae2:i".into())),("id",Tag::String("minecraft:stone".into()))]);
        let placeholder=decode_tolerant(&missing_amount).unwrap().unwrap();
        assert_eq!(placeholder.key.item_id,"ae2:missing_content");assert_eq!(placeholder.amount,1);
        assert_eq!(encode(&placeholder).unwrap(),missing_amount);
        let unknown=Tag::compound([("#t",Tag::String("ae2:i".into())),("id",Tag::String("missing:ore".into())),("#",Tag::Long(12))]);
        let placeholder=decode_tolerant_checked(&unknown,|id,_|id!="missing:ore").unwrap().unwrap();
        assert_eq!(placeholder.key.item_id,"ae2:missing_content");assert_eq!(placeholder.amount,12);
        assert_eq!(encode(&placeholder).unwrap(),unknown);
    }
}
