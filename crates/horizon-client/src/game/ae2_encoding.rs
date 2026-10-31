//! Persisted face-owned Pattern Encoding Terminal configuration. Ghost values
//! are recipe descriptions only; physical blank/output stacks live in Machine.slots.
use super::ae2_processing::PatternFluid;
use horizon_client::{ae2_world::Machine,items::ItemStack,save};
use horizon_world::nbt::Tag;

/// Convert a real source wrapper into a ghost value without moving the
/// physical carrier; preserve both its key components and stored amount.
#[derive(Clone,Debug,PartialEq)]
pub(super) enum WrappedGhost {Item(ItemStack), Fluid(PatternFluid)}
pub(super) fn wrapped_ghost(stack:&ItemStack,known:&dyn Fn(&str,bool)->bool)->Result<Option<WrappedGhost>,String> {
    let Some(value)=horizon_client::ae2_generic_stack::unwrap_item_stack_checked(stack,known)? else{return Ok(None);};
    if value.fluid {return Ok(Some(WrappedGhost::Fluid(PatternFluid{key:value.key,amount:value.amount.max(0) as u64})));}
    let mut item=horizon_client::ae2_cell_codec::prototype(&value.key)?;
    item.count=value.amount.clamp(1,i64::from(i32::MAX)) as u32;
    Ok(Some(WrappedGhost::Item(item)))
}

/// Decode the carried stack once. The boolean records whether its configured
/// quantity came from a wrapped stack rather than its physical carrier count.
pub(super) fn cursor_ghost(stack:Option<&ItemStack>,known:&dyn Fn(&str,bool)->bool)->Result<Option<(WrappedGhost,bool)>,String> {
    let Some(stack)=stack else{return Ok(None)};
    Ok(Some(match wrapped_ghost(stack,known)? {
        Some(value)=>(value,true),
        None=>(WrappedGhost::Item(stack.clone()),false),
    }))
}
/// The native fluid selector is a separate UI route; valid source wrappers
/// always retain their own key and amount instead of the selected fluid.
pub(super) fn select_ghost(cursor:&Result<Option<(WrappedGhost,bool)>,String>,fluid_mode:bool,selected:Option<PatternFluid>,right:bool)->Option<Option<(WrappedGhost,bool)>> {
    match cursor {
        Err(_)=>None,
        Ok(None) if right=>Some(None),
        Ok(Some((value,true)))=>Some(Some((value.clone(),true))),
        Ok(_) if fluid_mode=>selected.map(|value|Some((WrappedGhost::Fluid(value),true))),
        Ok(value)=>Some(value.clone()),
    }
}

/// AEBaseMenu.handleFakeSlotAction and FakeSlot.increase/decrease for a
/// CONFIG_STACKS inventory. A wrapper's physical count remains one while its
/// contained amount is preserved; ordinary right-click insertion uses one.
pub(super) fn click_ghost(
    item:&mut Option<ItemStack>, fluid:&mut Option<PatternFluid>,
    held:Option<(WrappedGhost,bool)>, right:bool, item_max:u32, item_capacity:u32, fluid_capacity:u64,
) {
    let held_is_wrapper=held.as_ref().is_some_and(|(_,wrapped)|*wrapped);
    let held=held.map(|(value,_)|value);
    let current=item.take().map(WrappedGhost::Item).or_else(||fluid.take().map(WrappedGhost::Fluid));
    let same=|a:&WrappedGhost,b:&WrappedGhost|match (a,b) {
        (WrappedGhost::Item(a),WrappedGhost::Item(b))=>a.same_item_same_components(b),
        (WrappedGhost::Fluid(a),WrappedGhost::Fluid(b))=>a.key==b.key,
        _=>false,
    };
    let next=if !right {
        match (current,held) {
            (Some(mut current),Some(held)) if same(&current,&held)=>{
                match (&mut current,held) {
                    (WrappedGhost::Item(a),WrappedGhost::Item(b))=>a.count=a.count.saturating_add(b.count).min(item_capacity),
                    (WrappedGhost::Fluid(a),WrappedGhost::Fluid(b))=>a.amount=a.amount.saturating_add(b.amount).min(fluid_capacity),
                    _=>unreachable!(),
                }
                Some(current)
            },
            (Some(_),Some(WrappedGhost::Fluid(value))) if value.amount==0=>None,
            (_,held)=>held,
        }
    } else {
        match (current,held) {
            (None,Some(WrappedGhost::Item(mut value))) if !held_is_wrapper=>{value.count=1;Some(WrappedGhost::Item(value))},
            (None,value)=>value,
            (Some(mut current),Some(held)) if same(&current,&held)=>{
                let remains=match (&mut current,held) {
                    (WrappedGhost::Item(a),WrappedGhost::Item(b))=>{a.count=a.count.saturating_sub(b.count);a.count>0},
                    (WrappedGhost::Fluid(a),WrappedGhost::Fluid(b))=>{a.amount=a.amount.saturating_sub(b.amount);a.amount>0},
                    _=>unreachable!(),
                };
                remains.then_some(current)
            },
            (Some(current),Some(_))=>Some(current),
            (Some(WrappedGhost::Item(mut current)),None) if current.count<=item_max.max(1)=>{
                current.count-=1;(current.count>0).then_some(WrappedGhost::Item(current))
            },
            (Some(_),None)=>None,
        }
    };
    match next {
        Some(WrappedGhost::Item(mut value))=>{value.count=value.count.min(item_capacity.max(1));*item=Some(value);},
        Some(WrappedGhost::Fluid(mut value))=>{value.amount=value.amount.min(fluid_capacity.max(1)).max(1);*fluid=Some(value);},
        None=>{}
    }
}

#[derive(Clone,Debug,PartialEq)]
pub(super) struct EncodingState {
    pub inputs:Vec<Option<ItemStack>>,pub outputs:Vec<Option<ItemStack>>,
    pub fluid_inputs:Vec<Option<PatternFluid>>,pub fluid_outputs:Vec<Option<PatternFluid>>,
    pub substitutes:bool,pub fluid_substitutes:bool,pub processing:bool,pub special_mode:u8,pub stone_recipe:Option<String>,
}
impl Default for EncodingState {
    fn default()->Self {Self{inputs:vec![None;81],outputs:vec![None;27],fluid_inputs:vec![None;81],fluid_outputs:vec![None;27],substitutes:false,fluid_substitutes:true,processing:false,special_mode:0,stone_recipe:None}}
}
impl EncodingState {
    /// PatternEncodingLogic.fixCraftingRecipes runs after input/mode changes.
    pub fn normalize_crafting_inputs(&mut self) {
        if !self.processing {
            self.fluid_inputs.fill(None);
            for item in self.inputs.iter_mut().flatten() {item.count=1;}
        }
    }
    pub fn load(machine:&Machine)->Result<Self,String> {
        let Some(tag)=machine.config.get(&"ae2_encoding_state".into()) else{return Ok(Self::default())};
        if !matches!(tag,Tag::Compound(_)){return Err("invalid encoding state".into());}
        let mut result=Self::default();
        fn flag(tag:&Tag,key:&str,default:bool)->Result<bool,String> {match tag.get(key) {None=>Ok(default),Some(Tag::Byte(0))=>Ok(false),Some(Tag::Byte(1))=>Ok(true),_=>Err(format!("invalid {key}"))}}
        result.substitutes=flag(tag,"substitutes",false)?;result.fluid_substitutes=flag(tag,"fluid_substitutes",true)?;result.processing=flag(tag,"processing",false)?;
        result.special_mode=match tag.get("special_mode") {None=>0,Some(Tag::Byte(n)) if (0..=2).contains(n)=>*n as u8,_=>return Err("invalid encoding mode".into())};
        result.stone_recipe=match tag.get("stone_recipe") {None=>None,Some(Tag::String(id))=>Some(id.to_string().map_err(|_|"invalid stone recipe")?),_=>return Err("invalid stone recipe".into())};
        if result.processing && result.special_mode!=0 {return Err("conflicting encoding mode".into());}
        fn items(tag:&Tag,key:&str,dest:&mut [Option<ItemStack>])->Result<(),String> {
            let Some(value)=tag.get(key) else{return Ok(())};let Tag::List(entries)=value else{return Err(format!("invalid {key}"));};
            for entry in entries {let Some(Tag::Int(i))=entry.get("slot") else{return Err(format!("invalid {key} slot"));};let n=usize::try_from(*i).map_err(|_|format!("invalid {key} slot"))?;let target=dest.get_mut(n).ok_or_else(||format!("invalid {key} slot"))?;
                let stack=entry.get("item").and_then(save::item_from_nbt).filter(|s|s.count>0).ok_or_else(||format!("invalid {key} item"))?;
                if target.replace(stack).is_some(){return Err(format!("duplicate {key} slot"));}
            } Ok(())
        }
        fn fluids(tag:&Tag,key:&str,dest:&mut [Option<PatternFluid>])->Result<(),String> {
            let Some(value)=tag.get(key) else{return Ok(())};let Tag::List(entries)=value else{return Err(format!("invalid {key}"));};
            for entry in entries {let (Some(Tag::Int(i)),Some(Tag::String(name)),Some(Tag::ByteArray(components)),Some(Tag::Long(amount)))=(entry.get("slot"),entry.get("fluid"),entry.get("components"),entry.get("amount")) else{return Err(format!("invalid {key} fluid"));};
                let n=usize::try_from(*i).map_err(|_|format!("invalid {key} slot"))?;let target=dest.get_mut(n).ok_or_else(||format!("invalid {key} slot"))?;
                let name=name.to_string().map_err(|_|format!("invalid {key} fluid"))?;if name.is_empty() || *amount<=0{return Err(format!("invalid {key} fluid"));}
                if target.replace(PatternFluid{key:ae2_cell_core::ItemKey{item_id:name,components:components.clone()},amount:*amount as u64}).is_some(){return Err(format!("duplicate {key} slot"));}
            } Ok(())
        }
        items(tag,"inputs",&mut result.inputs)?;items(tag,"outputs",&mut result.outputs)?;
        fluids(tag,"fluid_inputs",&mut result.fluid_inputs)?;fluids(tag,"fluid_outputs",&mut result.fluid_outputs)?;
        if result.inputs.iter().zip(&result.fluid_inputs).any(|(a,b)|a.is_some()&&b.is_some()) || result.outputs.iter().zip(&result.fluid_outputs).any(|(a,b)|a.is_some()&&b.is_some()){return Err("overlapping item and fluid ghost slots".into());}
        Ok(result)
    }
    pub fn from_pattern(pattern:&ItemStack)->Option<Self> {
        let mut state=Self::default();
        if let Some(special)=super::ae2_processing::ae2_decode_special_pattern(pattern) {
            state.inputs[..9].clone_from_slice(&special.grid);state.substitutes=special.substitutes;
            state.special_mode=if special.kind=="smithing" {1}else{2};
            if state.special_mode==2 {state.stone_recipe=Some(special.recipe);}return Some(state);
        }
        if let Some((grid,_))=super::ae2_processing::decode_pattern(pattern) {
            state.inputs[..9].clone_from_slice(&grid);
            if super::ae2_source_patterns::crafting(pattern).is_some() {
                let (substitutes,fluid_substitutes)=super::ae2_processing::ae2_crafting_pattern_options(pattern)?;
                state.substitutes=substitutes;state.fluid_substitutes=fluid_substitutes;
            } else if let Some(data)=pattern.data().and_then(|d|d.other("minecraft:custom_data")).and_then(|v|v.get("ae2_lab_pattern")) {
                state.substitutes=matches!(data.get("substitutes"),Some(Tag::Byte(1)));
                state.fluid_substitutes=!matches!(data.get("fluid_substitutes"),Some(Tag::Byte(0)));
            }
            return Some(state);
        }
        let (inputs,outputs)=super::ae2_processing::ae2_decode_processing_pattern(pattern)?;
        state.processing=true;state.inputs=inputs;state.outputs=outputs;
        match super::ae2_processing::ae2_decode_processing_positions(pattern).ok()? {
            Some((fluids_in,fluids_out))=>{state.fluid_inputs=fluids_in;state.fluid_outputs=fluids_out;}
            None=>{
                // Existing patterns lacked slot metadata. Preserve their old
                // best-effort display; their mixed order cannot be recovered.
                let (fluids_in,fluids_out)=super::ae2_processing::ae2_decode_processing_fluids(pattern)?;
                for fluid in fluids_in {let i=state.inputs.iter().zip(&state.fluid_inputs).position(|(item,fluid)|item.is_none()&&fluid.is_none())?;state.fluid_inputs[i]=Some(fluid);}
                for fluid in fluids_out {let i=state.outputs.iter().zip(&state.fluid_outputs).position(|(item,fluid)|item.is_none()&&fluid.is_none())?;state.fluid_outputs[i]=Some(fluid);}
            }
        }
        Some(state)
    }
    pub fn save(&self,machine:&mut Machine)->Result<(),String> {
        if self.inputs.len()!=81 || self.outputs.len()!=27 || self.fluid_inputs.len()!=81 || self.fluid_outputs.len()!=27 {return Err("invalid encoding grid size".into());}
        let items=|values:&[Option<ItemStack>]|Tag::List(values.iter().enumerate().filter_map(|(i,s)|s.as_ref().map(|s|Tag::compound([("slot",Tag::Int(i as i32)),("item",save::item_to_nbt(s))]))).collect());
        let fluids=|values:&[Option<PatternFluid>]|Tag::List(values.iter().enumerate().filter_map(|(i,f)|f.as_ref().map(|f|Tag::compound([("slot",Tag::Int(i as i32)),("fluid",Tag::String(f.key.item_id.as_str().into())),("components",Tag::ByteArray(f.key.components.clone())),("amount",Tag::Long(f.amount as i64))]))).collect());
        if self.fluid_inputs.iter().chain(&self.fluid_outputs).flatten().any(|f|f.amount==0||f.amount>i64::MAX as u64||f.key.item_id.is_empty()){return Err("invalid encoding fluid".into());}
        machine.config.insert("ae2_encoding_state".into(),Tag::compound([("substitutes",Tag::Byte(i8::from(self.substitutes))),("fluid_substitutes",Tag::Byte(i8::from(self.fluid_substitutes))),("processing",Tag::Byte(i8::from(self.processing))),("special_mode",Tag::Byte(self.special_mode as i8)),("inputs",items(&self.inputs)),("outputs",items(&self.outputs)),("fluid_inputs",fluids(&self.fluid_inputs)),("fluid_outputs",fluids(&self.fluid_outputs))]));
        if let Some(recipe)=&self.stone_recipe {if recipe.is_empty(){return Err("invalid stone recipe".into());}if let Some(Tag::Compound(data))=machine.config.get_mut(&"ae2_encoding_state".into()){data.insert("stone_recipe".into(),Tag::String(recipe.as_str().into()));}}
        Self::load(machine)?;Ok(())
    }
}

/// A window stays bound to the exact physical part, not just its coordinates.
impl super::Game<'_> {
    pub(super) fn ae2_terminal_part_identity(&self,part:super::ae2_parts::PartAddress)->Option<(u64,[i32;4])> {
        let (dim,pos)=self.ae2_address_parts(part.host)?;let world=self.ae2_world_for(dim)?;
        let incarnation=world.block_entities.incarnation(&pos)?;
        let horizon_client::containers::BlockEntity::Ae2(host)=world.block_entities.get(&pos)? else{return None};
        let Tag::List(parts)=host.config.get(&"parts".into())? else{return None};
        let face=["east","west","up","down","south","north"].get(usize::from(part.face))?;
        let mut matches=parts.iter().filter(|v|v.get("face")==Some(&Tag::String((*face).into())));
        let value=matches.next()?;if matches.next().is_some(){return None;}
        let Some(Tag::String(id))=value.get("id") else{return None;};
        if !matches!(id.to_string().ok()?.as_str(),"ae2:terminal"|"ae2:crafting_terminal"|"ae2:pattern_encoding_terminal"){return None;}
        Some((incarnation,super::ae2_channels::part_identity(value)?))
    }
    pub(super) fn ae2_encoding_identity(&self,part:super::ae2_parts::PartAddress)->Option<(u64,[i32;4])> {
        let (dim,pos)=self.ae2_address_parts(part.host)?;let world=self.ae2_world_for(dim)?;
        let incarnation=world.block_entities.incarnation(&pos)?;
        let horizon_client::containers::BlockEntity::Ae2(host)=world.block_entities.get(&pos)? else{return None};
        let Tag::List(parts)=host.config.get(&"parts".into())? else{return None};
        let face=["east","west","up","down","south","north"].get(usize::from(part.face))?;
        let mut matches=parts.iter().filter(|v|v.get("face")==Some(&Tag::String((*face).into())));
        let value=matches.next()?;if matches.next().is_some(){return None;}
        if value.get("id")!=Some(&Tag::String("ae2:pattern_encoding_terminal".into())){return None;}
        Some((incarnation,super::ae2_channels::part_identity(value)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::ae2_world::MachineKind;
    #[test]
    fn source_crafting_pattern_reopen_preserves_both_substitution_flags() {
        let mut grid=vec![None;9];grid[0]=Some(ItemStack::new("minecraft:oak_planks",1));
        let output=ItemStack::new("minecraft:oak_button",1);
        let mut pattern=super::super::ae2_processing::ae2_encode_pattern_source(&grid,&output,"minecraft:oak_button",true,false).unwrap();
        pattern.data_mut().set_other("minecraft:custom_data",None);
        let restored=EncodingState::from_pattern(&pattern).unwrap();
        assert!(restored.substitutes);assert!(!restored.fluid_substitutes);
        assert_eq!(&restored.inputs[..9],grid.as_slice());
    }
    #[test]
    fn wrapped_ghosts_preserve_inner_identity_amount_and_components() {
        let component=Tag::compound([("minecraft:custom_data",Tag::compound([("variant",Tag::String("warm".into()))]))]);
        let bytes=horizon_world::nbt::write_network(&component,horizon_world::nbt::Limits::default()).unwrap();
        let value=horizon_client::ae2_generic_stack::GenericStack{key:ae2_cell_core::ItemKey{item_id:"minecraft:water".into(),components:bytes},fluid:true,amount:1750};
        let physical=horizon_client::ae2_generic_stack::wrap(&value).unwrap();
        assert_eq!(wrapped_ghost(&physical,&|_,_|true).unwrap(),Some(WrappedGhost::Fluid(PatternFluid{key:value.key,amount:1750})));
        assert_eq!(physical.count,1);
        let mut physical_item=ItemStack::new("minecraft:oak_log",1);physical_item.data_mut().custom_name=Some("marked".into());
        let key=horizon_client::ae2_cell_codec::key(&physical_item).unwrap();
        let wrapped=horizon_client::ae2_generic_stack::wrap(&horizon_client::ae2_generic_stack::GenericStack{key,fluid:false,amount:5}).unwrap();
        physical_item.count=5;assert_eq!(wrapped_ghost(&wrapped,&|_,_|true).unwrap(),Some(WrappedGhost::Item(physical_item.clone())));
        let mut state=EncodingState::default();state.inputs[0]=Some(physical_item);
        state.fluid_inputs[1]=Some(PatternFluid{key:ae2_cell_core::ItemKey{item_id:"minecraft:water".into(),components:vec![]},amount:1750});
        state.normalize_crafting_inputs();assert_eq!(state.inputs[0].as_ref().unwrap().count,1);assert!(state.fluid_inputs[1].is_none());
    }
    #[test]
    fn fake_slot_clicks_add_subtract_and_preserve_mismatched_keys() {
        let mut item=None;let mut fluid=None;
        let oak=ItemStack::new("minecraft:oak_planks",3);
        click_ghost(&mut item,&mut fluid,Some((WrappedGhost::Item(oak.clone()),false)),true,64,i32::MAX as u32,i64::MAX as u64);
        assert_eq!(item.as_ref().unwrap().count,1,"right click of an empty slot inserts one");
        click_ghost(&mut item,&mut fluid,Some((WrappedGhost::Item(oak.clone()),false)),false,64,i32::MAX as u32,i64::MAX as u64);
        assert_eq!(item.as_ref().unwrap().count,4,"left click adds the held amount");
        click_ghost(&mut item,&mut fluid,Some((WrappedGhost::Item(ItemStack::new("minecraft:stone",1)),false)),true,64,i32::MAX as u32,i64::MAX as u64);
        assert_eq!(item.as_ref().unwrap().count,4,"right click with a different key does not clear it");
        click_ghost(&mut item,&mut fluid,Some((WrappedGhost::Item(oak),false)),true,64,i32::MAX as u32,i64::MAX as u64);
        assert_eq!(item.as_ref().unwrap().count,1,"right click subtracts the held amount");
        click_ghost(&mut item,&mut fluid,None,true,64,i32::MAX as u32,i64::MAX as u64);
        assert!(item.is_none());
        let water=PatternFluid{key:ae2_cell_core::ItemKey{item_id:"minecraft:water".into(),components:vec![]},amount:1250};
        click_ghost(&mut item,&mut fluid,Some((WrappedGhost::Fluid(water.clone()),true)),true,64,i32::MAX as u32,i64::MAX as u64);
        assert_eq!(fluid.as_ref().unwrap().amount,1250,"a wrapper retains its embedded amount");
        click_ghost(&mut item,&mut fluid,Some((WrappedGhost::Fluid(water),true)),false,64,i32::MAX as u32,i64::MAX as u64);
        assert_eq!(fluid.as_ref().unwrap().amount,2500);
        click_ghost(&mut item,&mut fluid,None,true,64,i32::MAX as u32,i64::MAX as u64);
        assert!(fluid.is_none(),"right click on a displayed wrapper shrinks its one carrier");
        let zero=PatternFluid{key:ae2_cell_core::ItemKey{item_id:"minecraft:water".into(),components:vec![]},amount:0};
        click_ghost(&mut item,&mut fluid,Some((WrappedGhost::Fluid(zero.clone()),true)),false,64,i32::MAX as u32,i64::MAX as u64);
        assert_eq!(fluid.as_ref().unwrap().amount,1,"zero on an empty slot configures one unit");
        click_ghost(&mut item,&mut fluid,Some((WrappedGhost::Fluid(zero.clone()),true)),false,64,i32::MAX as u32,i64::MAX as u64);
        assert_eq!(fluid.as_ref().unwrap().amount,1,"inserting zero of the same key has no effect");
        item=Some(ItemStack::new("minecraft:stone",1));fluid=None;
        click_ghost(&mut item,&mut fluid,Some((WrappedGhost::Fluid(zero),true)),false,64,i32::MAX as u32,i64::MAX as u64);
        assert!(item.is_none() && fluid.is_none(),"zero of a different key clears an occupied slot");
    }
    #[test]
    fn positioned_mixed_pattern_restores_original_ghost_slots() {
        let mut state=EncodingState::default();state.processing=true;
        state.inputs[1]=Some(ItemStack::new("minecraft:iron_ingot",3));
        state.outputs[2]=Some(ItemStack::new("minecraft:gold_ingot",2));
        let fluid=PatternFluid{key:ae2_cell_core::ItemKey{item_id:"minecraft:water".into(),components:vec![0,42]},amount:750};
        state.fluid_inputs[8]=Some(fluid.clone());state.fluid_outputs[0]=Some(fluid);
        let encode=|state:&EncodingState|super::super::ae2_processing::ae2_encode_processing_positioned(
            &state.inputs,&state.outputs,&state.fluid_inputs,&state.fluid_outputs);
        let pattern=encode(&state).unwrap();assert_eq!(EncodingState::from_pattern(&pattern),Some(state.clone()));
        state.fluid_outputs[0]=None;assert!(encode(&state).is_err(),"a later item cannot replace an empty primary output");
    }
    #[test]
    fn special_mode_and_selected_recipe_survive_state_roundtrip() {
        let mut machine=Machine::new(MachineKind::PatternEncodingTerminal);let mut state=EncodingState::default();state.special_mode=2;state.stone_recipe=Some("minecraft:stone_slab".into());state.inputs[0]=Some(ItemStack::new("minecraft:stone",1));
        state.save(&mut machine).unwrap();assert_eq!(EncodingState::load(&machine).unwrap(),state);
        state.processing=true;assert!(state.save(&mut machine).is_err());
    }
    #[test]
    fn encoding_state_roundtrips_sparse_items_fluids_and_rejects_duplicate_slots() {
        let mut machine=Machine::new(MachineKind::PatternEncodingTerminal);
        let mut state=EncodingState::default();state.processing=true;state.substitutes=true;
        let mut item=ItemStack::new("minecraft:iron_ingot",3);item.data_mut().custom_name=Some("Tagged input".into());
        state.inputs[80]=Some(item);state.outputs[26]=Some(ItemStack::new("minecraft:gold_ingot",2));
        state.fluid_inputs[2]=Some(PatternFluid{key:ae2_cell_core::ItemKey{item_id:"example:water".into(),components:vec![0,42]},amount:750});
        state.save(&mut machine).unwrap();assert_eq!(EncodingState::load(&machine).unwrap(),state);
        let Tag::Compound(data)=machine.config.get_mut(&"ae2_encoding_state".into()).unwrap() else{panic!()};
        let Tag::List(inputs)=data.get_mut(&"inputs".into()).unwrap() else{panic!()};inputs.push(inputs[0].clone());
        assert!(EncodingState::load(&machine).is_err());
    }
}
