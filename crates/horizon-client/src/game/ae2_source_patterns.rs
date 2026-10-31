//! Persistent encoded-pattern records from AE2 19.2.18 (LGPL-3.0-or-later).
use horizon_client::{ae2_generic_stack::{self,GenericStack},items::ItemStack,save};
use horizon_world::nbt::Tag;

fn source_int(tag:&Tag)->Option<i32> {
    match tag {
        Tag::Float(value)=>Some(f32::from_bits(*value) as i32),
        Tag::Double(value)=>Some(f64::from_bits(*value) as i32),
        value=>save::number(Some(value)).map(|number|number as i32),
    }
}
fn item(tag:&Tag)->Result<ItemStack,String> {
    let count=match tag.get("count") {None=>1,Some(value)=>source_int(value).ok_or("invalid pattern item count")?};
    if !(1..=99).contains(&count) {return Err("invalid pattern item count".into());}
    let mut item=save::item_from_nbt(tag).ok_or("invalid pattern item")?;
    item.count=count as u32;
    if !item.item.contains(':') {item.item=format!("minecraft:{}",item.item).into();}
    if item.item.as_ref()=="minecraft:air" || !horizon_client::ae2_cell_codec::valid_fluid_id(&item.item) {return Err("invalid pattern item id".into());}
    Ok(item)
}
fn name(tag:&Tag,key:&str)->Result<String,String> {
    let Some(Tag::String(value))=tag.get(key) else{return Err(format!("missing pattern {key}"));};
    let value=value.to_string().map_err(|_|format!("invalid pattern {key}"))?;
    if value.is_empty(){return Err(format!("empty pattern {key}"));}
    Ok(if value.contains(':') {value} else {format!("minecraft:{value}")})
}
fn flag(tag:&Tag,key:&str)->Result<bool,String> {
    match tag.get(key) {Some(value)=>source_int(value).map(|number|number as i8 != 0).ok_or_else(||format!("invalid pattern {key}")),None=>Err(format!("invalid pattern {key}"))}
}

#[derive(Clone,Debug,PartialEq)]
pub(super) struct SourceCrafting {pub grid:Vec<Option<ItemStack>>,pub input_amounts:Vec<u64>,pub output:ItemStack,pub recipe:String,pub substitutes:bool,pub fluid_substitutes:bool}
pub(super) fn crafting(stack:&ItemStack)->Option<Result<SourceCrafting,String>> {
    let source=stack.data()?.other("ae2:encoded_crafting_pattern")?;
    Some((||{
        let Some(Tag::List(inputs))=source.get("inputs") else{return Err("invalid crafting pattern inputs".into());};
        // The source constructor indexes all nine sparse positions while
        // condensing inputs; a shorter list cannot become a crafting pattern.
        if inputs.len()!=9 {return Err("crafting pattern requires nine inputs".into());}
        let mut grid=Vec::with_capacity(9);let mut input_amounts=Vec::with_capacity(9);
        for entry in inputs {
            if matches!(entry,Tag::Compound(data) if data.is_empty()){grid.push(None);input_amounts.push(0);}else{
                let item=item(entry)?;
                // getCraftingInputs keeps the GenericStack amount; recipe
                // matching separately uses AEItemKey.toStack(), a one-item
                // prototype. Wrapped fluid fails the source's item-key cast.
                let value=ae2_generic_stack::from_item_stack(&item)?.ok_or("empty crafting input")?;
                if value.fluid {return Err("fluid crafting input".into());}
                let amount=u64::try_from(value.amount).ok().filter(|amount|*amount>0).ok_or("nonpositive crafting input amount")?;
                grid.push(Some(horizon_client::ae2_cell_codec::prototype(&value.key)?));input_amounts.push(amount);
            }
        }
        grid.resize(9,None);input_amounts.resize(9,0);
        if grid.iter().all(Option::is_none){return Err("empty crafting pattern".into());}
        Ok(SourceCrafting {grid,input_amounts,output:item(source.get("result").ok_or("missing pattern result")?)?,recipe:name(source,"recipeId")?,
            substitutes:flag(source,"canSubstitute")?,fluid_substitutes:flag(source,"canSubstituteFluids")?})
    })())
}
pub(super) fn encode_crafting(grid:&[Option<ItemStack>],output:&ItemStack,recipe:&str,substitutes:bool,fluid:bool)->Result<Tag,String> {
    if grid.len()!=9 || grid.iter().all(Option::is_none) || grid.iter().flatten().any(|item|item.count!=1) || output.count==0 || recipe.is_empty() {return Err("invalid crafting pattern".into());}
    horizon_client::ae2_components::normalize("ae2:encoded_crafting_pattern",Tag::compound([
        ("inputs",Tag::List(grid.iter().map(|slot|slot.as_ref().map(save::item_to_nbt).unwrap_or_else(||Tag::Compound(Default::default()))).collect())),
        ("result",save::item_to_nbt(output)),("recipeId",Tag::String(recipe.into())),
        ("canSubstitute",Tag::Byte(i8::from(substitutes))),("canSubstituteFluids",Tag::Byte(i8::from(fluid))),
    ]))
}

pub(super) fn processing(stack:&ItemStack)->Option<Result<(Vec<Option<GenericStack>>,Vec<Option<GenericStack>>),String>> {
    let source=stack.data()?.other("ae2:encoded_processing_pattern")?;
    Some((||{
        let lists=|field:&str,max:usize|->Result<Vec<Option<GenericStack>>,String>{
            let Some(Tag::List(entries))=source.get(field) else{return Err(format!("invalid {field}"));};
            if entries.len()>max{return Err(format!("too many {field}"));}
            entries.iter().map(ae2_generic_stack::decode_tolerant).collect()
        };
        let inputs=lists("sparseInputs",81)?;let outputs=lists("sparseOutputs",27)?;
        if inputs.iter().all(Option::is_none)||outputs.first().and_then(Option::as_ref).is_none() {return Err("empty processing pattern".into());}
        Ok((inputs,outputs))
    })())
}
pub(super) fn encode_processing(inputs:&[Option<GenericStack>],outputs:&[Option<GenericStack>])->Result<Tag,String> {
    if inputs.len()>81||outputs.len()>27||inputs.iter().all(Option::is_none)||outputs.first().and_then(Option::as_ref).is_none() {return Err("invalid processing pattern".into());}
    let list=|slots:&[Option<GenericStack>]|slots.iter().map(|entry|match entry {None=>Ok(Tag::Compound(Default::default())),Some(entry)=>ae2_generic_stack::encode(entry)}).collect::<Result<Vec<_>,String>>();
    horizon_client::ae2_components::normalize("ae2:encoded_processing_pattern",Tag::compound([("sparseInputs",Tag::List(list(inputs)?)),("sparseOutputs",Tag::List(list(outputs)?))]))
}

#[derive(Clone,Debug,PartialEq)]
pub(super) struct SourceSpecial {pub grid:Vec<Option<ItemStack>>,pub output:ItemStack,pub recipe:String,pub substitutes:bool}
pub(super) fn special(stack:&ItemStack)->Option<Result<SourceSpecial,String>> {
    let (component,fields,output)=match stack.item.as_ref(){
        "ae2:stonecutting_pattern"=>("ae2:encoded_stonecutting_pattern",vec!["input"],"output"),
        "ae2:smithing_table_pattern"=>("ae2:encoded_smithing_table_pattern",vec!["template","base","addition"],"resultItem"),
        _=>return None,
    };
    let source=stack.data()?.other(component)?;
    Some((||{
        let mut grid=vec![None;9];for (index,field) in fields.iter().enumerate(){
            let input=item(source.get(field).ok_or("missing special input")?)?;
            if input.count!=1{return Err("special input is not singular".into());}grid[index]=Some(input);
        }
        Ok(SourceSpecial{grid,output:item(source.get(output).ok_or("missing special output")?)?,recipe:name(source,"recipeId")?,substitutes:flag(source,"canSubstitute")?})
    })())
}
pub(super) fn encode_special(kind:&str,grid:&[Option<ItemStack>],output:&ItemStack,recipe:&str,substitutes:bool)->Result<Tag,String> {
    if grid.len()!=9||recipe.is_empty()||output.count==0 {return Err("invalid special pattern".into());}
    let value=|i:usize|grid.get(i).and_then(Option::as_ref).filter(|item|item.count==1).map(save::item_to_nbt).ok_or("invalid special input");
    let (component,tag)=match kind {
        "stonecutting" if grid.iter().skip(1).all(Option::is_none)=>("ae2:encoded_stonecutting_pattern",Tag::compound([("input",value(0)?),("output",save::item_to_nbt(output)),("canSubstitute",Tag::Byte(i8::from(substitutes))),("recipeId",Tag::String(recipe.into()))])),
        "smithing" if grid.iter().skip(3).all(Option::is_none)=>("ae2:encoded_smithing_table_pattern",Tag::compound([("template",value(0)?),("base",value(1)?),("addition",value(2)?),("resultItem",save::item_to_nbt(output)),("canSubstitute",Tag::Byte(i8::from(substitutes))),("recipeId",Tag::String(recipe.into()))])),
        _=>return Err("invalid special pattern kind".into()),
    };
    horizon_client::ae2_components::normalize(component,tag)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrapped_singular_source_crafting_input_uses_the_inner_item_key() {
        let mut input=ItemStack::new("minecraft:oak_log",1);
        input.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("variant",Tag::String("wrapped".into()))])));
        let key=horizon_client::ae2_cell_codec::key(&input).unwrap();
        let wrapped=ae2_generic_stack::wrap(&GenericStack{key:key.clone(),fluid:false,amount:1}).unwrap();
        let mut grid=vec![None;9];grid[0]=Some(wrapped.clone());
        let output=ItemStack::new("minecraft:oak_planks",4);
        let mut pattern=ItemStack::new("ae2:crafting_pattern",1);
        pattern.data_mut().set_other("ae2:encoded_crafting_pattern",Some(encode_crafting(&grid,&output,"minecraft:oak_planks",false,false).unwrap()));
        let decoded=crafting(&pattern).unwrap().unwrap();
        assert_eq!(decoded.grid[0],Some(input));assert_eq!(decoded.output,output);
        let mut too_many=wrapped.clone();too_many.data_mut().set_other("ae2:wrapped_stack",Some(ae2_generic_stack::encode(&GenericStack{key,fluid:false,amount:2}).unwrap()));
        grid[0]=Some(too_many);pattern.data_mut().set_other("ae2:encoded_crafting_pattern",Some(encode_crafting(&grid,&decoded.output,"minecraft:oak_planks",false,false).unwrap()));
        let decoded=crafting(&pattern).unwrap().unwrap();
        assert_eq!(decoded.grid[0].as_ref().unwrap().count,1);assert_eq!(decoded.input_amounts[0],2);
        let mut ordinary=ItemStack::new("minecraft:oak_log",3);ordinary.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("variant",Tag::String("wrapped".into()))])));
        let mut encoded=encode_crafting(&vec![Some(ItemStack::new("minecraft:oak_log",1)),None,None,None,None,None,None,None,None],&decoded.output,"minecraft:oak_planks",false,false).unwrap();
        if let Tag::Compound(data)=&mut encoded {if let Some(Tag::List(inputs))=data.get_mut(&"inputs".into()){inputs[0]=save::item_to_nbt(&ordinary);}}
        pattern.data_mut().set_other("ae2:encoded_crafting_pattern",Some(encoded));
        let decoded=crafting(&pattern).unwrap().unwrap();assert_eq!(decoded.grid[0].as_ref().unwrap().count,1);assert_eq!(decoded.input_amounts[0],3);
        let fluid=GenericStack{key:horizon_client::ae2_cell_codec::fluid_key("minecraft:water"),fluid:true,amount:1000};
        grid[0]=Some(ae2_generic_stack::wrap(&fluid).unwrap());pattern.data_mut().set_other("ae2:encoded_crafting_pattern",Some(encode_crafting(&grid,&decoded.output,"minecraft:oak_planks",false,false).unwrap()));
        assert!(crafting(&pattern).unwrap().is_err());
    }
    #[test]
    fn crafting_pattern_requires_the_nine_source_sparse_positions() {
        let mut grid=vec![None;9];grid[0]=Some(ItemStack::new("minecraft:oak_log",1));
        let mut source=encode_crafting(&grid,&ItemStack::new("minecraft:oak_planks",4),"minecraft:oak_planks",false,false).unwrap();
        let Tag::Compound(data)=&mut source else{panic!()};
        let Some(Tag::List(inputs))=data.get_mut(&"inputs".into()) else{panic!()};inputs.pop();
        let mut pattern=ItemStack::new("ae2:crafting_pattern",1);pattern.data_mut().set_other("ae2:encoded_crafting_pattern",Some(source));
        assert!(crafting(&pattern).unwrap().is_err());
    }
    #[test]
    fn raw_source_flags_follow_dynamic_ops_number_byte_value() {
        // NbtOps.getNumberValue returns the Java Number; DynamicOps reads its
        // byteValue, including Java's float-to-int clamp before byte narrowing.
        for (value,expected) in [
            (Tag::Int(256),false), (Tag::Int(-256),false),
            (Tag::Long(4_294_967_297),true), (Tag::Float(256.0f32.to_bits()),false),
            (Tag::Double((-257.0f64).to_bits()),true), (Tag::Double(f64::NAN.to_bits()),false),
            (Tag::Double(f64::INFINITY.to_bits()),true),
        ] {
            let record=Tag::compound([("canSubstitute",value)]);
            assert_eq!(flag(&record,"canSubstitute").unwrap(),expected);
        }
    }
    #[test]
    fn raw_crafting_item_fields_follow_source_numeric_and_default_namespace_rules() {
        let mut pattern=ItemStack::new("ae2:crafting_pattern",1);
        pattern.data_mut().set_other("ae2:encoded_crafting_pattern",Some(Tag::compound([
            ("inputs",Tag::List(std::iter::once(Tag::compound([("id",Tag::String("oak_log".into())),("count",Tag::Long(4_294_967_297))])).chain(std::iter::repeat_with(||Tag::Compound(Default::default())).take(8)).collect())),
            ("result",Tag::compound([("id",Tag::String("oak_planks".into())),("count",Tag::Short(4))])),
            ("recipeId",Tag::String("oak_planks".into())),("canSubstitute",Tag::Int(1)),("canSubstituteFluids",Tag::Double(0f64.to_bits())),
        ])));
        let source=crafting(&pattern).unwrap().unwrap();
        assert_eq!(source.grid[0].as_ref().unwrap().item.as_ref(),"minecraft:oak_log");
        assert_eq!(source.output.item.as_ref(),"minecraft:oak_planks");assert_eq!(source.output.count,4);
        assert_eq!(source.recipe,"minecraft:oak_planks");assert!(source.substitutes);assert!(!source.fluid_substitutes);
    }
}
