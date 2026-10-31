//! Face-owned AE2 Storage/Conversion Monitor interaction state.
//! Source: AbstractMonitorPart and ConversionMonitorPart, AE2 19.2.18 (LGPL-3.0+).
use super::{Game, ae2_lab};
use ae2_cell_core::ItemKey;
use horizon_client::{containers::BlockEntity, items::ItemStack};
use horizon_world::nbt::Tag;
use winit::keyboard::KeyCode;
type Pos=(i32,i32,i32);
#[derive(Clone,Debug)]
pub(super) struct MonitorState {
    pub key: Option<(ItemKey,bool)>,
    pub locked: bool,
    pub conversion: bool,
}
fn monitor_id(id:&str)->bool {matches!(id,"ae2:storage_monitor"|"ae2:conversion_monitor")}
fn read_part<'a>(entity:&'a BlockEntity,face:&str)->Option<&'a Tag> {
    let BlockEntity::Ae2(machine)=entity else{return None};
    let Tag::List(parts)=machine.config.get(&"parts".into())? else{return None};
    let mut matches=parts.iter().filter(|part|part.get("face")==Some(&Tag::String(face.into())));
    let part=matches.next()?;if matches.next().is_some(){return None;}
    let Some(Tag::String(id))=part.get("id") else{return None};
    if !id.to_string().ok().is_some_and(|id|monitor_id(&id)){return None;}
    Some(part)
}
fn decode(part:&Tag)->Option<MonitorState> {
    let Tag::String(id)=part.get("id")? else{return None};let id=id.to_string().ok()?;
    let locked=matches!(part.get("ae2_monitor_locked"),Some(Tag::Byte(1)));
    let key=match part.get("ae2_monitor_key") {
        None=>None,
        Some(tag)=>{
            let (Some(Tag::String(id)),Some(Tag::ByteArray(components)),Some(Tag::Byte(fluid)))=(tag.get("item"),tag.get("components"),tag.get("fluid")) else{return None};
            if !matches!(*fluid,0|1){return None;}
            let key=ItemKey{item_id:id.to_string().ok()?,components:components.clone()};
            if *fluid==0 {ae2_lab::prototype(&key).ok()?;} else if !super::ae2_fluids::valid_fluid_id(&key.item_id){return None;}
            Some((key,*fluid==1))
        }
    };
    Some(MonitorState{key,locked,conversion:id=="ae2:conversion_monitor"})
}
fn encoded_key(key:&ItemKey,fluid:bool)->Tag {
    Tag::compound([("item",Tag::String(key.item_id.as_str().into())),("components",Tag::ByteArray(key.components.clone())),("fluid",Tag::Byte(i8::from(fluid)))])
}
impl Game<'_> {
    pub(super) fn ae2_monitor_client_use_consumed(&self,pos:Pos,face:&str,has_item:bool,alternate:bool)->bool {
        let Some(part)=self.ae2_visual_cache.get(&pos).and_then(|v|v.parts.iter().find(|p|p.face==face && monitor_id(&p.id))) else{return false};
        if alternate {return !has_item;}
        if part.id=="ae2:conversion_monitor" {return has_item || part.monitor_locked;}
        has_item && !part.monitor_locked
    }
    pub(super) fn ae2_conversion_monitor_client_active(&self,pos:Pos,face:&str)->bool {
        self.ae2_visual_cache.get(&pos).is_some_and(|v|v.parts.iter().any(|p|p.face==face && p.id=="ae2:conversion_monitor" && p.online))
    }
    pub(super) fn ae2_monitor_state(&self,pos:Pos,face:&str)->Option<MonitorState> {
        decode(read_part(self.world.block_entities.get(&pos)?,face)?)
    }
    /// An empty conversion monitor offers crafting only for the exact watched
    /// item key when the powered grid advertises a matching pattern.
    /// The caller must revalidate this before starting a confirmed job.
    pub(super) fn ae2_monitor_craft_target(&self,pos:Pos,face:&str)->Option<ItemKey> {
        let state=self.ae2_monitor_state(pos,face)?;
        if !state.conversion || !self.ae2_part_online(pos,face) {return None;}
        let (key,false)=state.key? else{return None};
        if self.ae2_grid_inventory(pos).ok()?.iter().any(|(k,n)|k==&key && *n>0) {return None;}
        self.ae2_craftable_outputs(pos).contains(&key).then_some(key)
    }
    fn ae2_monitor_write(&mut self,pos:Pos,face:&str,key:Option<(&ItemKey,bool)>,locked:bool)->Result<(),String> {
        let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get_mut(&pos) else{return Err("monitor host missing".into())};
        let Some(Tag::List(parts))=machine.config.get_mut(&"parts".into()) else{return Err("monitor parts missing".into())};
        let mut found=None;
        for (i,part) in parts.iter().enumerate() {
            if part.get("face")==Some(&Tag::String(face.into())) {if found.is_some(){return Err("duplicate monitor face".into());}found=Some(i);}
        }
        let i=found.ok_or("monitor disappeared")?;
        let Some(id)=parts[i].get("id").and_then(|v|if let Tag::String(s)=v{s.to_string().ok()}else{None}) else{return Err("monitor id missing".into())};
        if !monitor_id(&id){return Err("monitor changed".into());}
        let Tag::Compound(data)=&mut parts[i] else{return Err("invalid monitor state".into())};
        data.insert("ae2_monitor_locked".into(),Tag::Byte(i8::from(locked)));
        if let Some((key,fluid))=key {data.insert("ae2_monitor_key".into(),encoded_key(key,fluid));} else {data.remove(&"ae2_monitor_key".into());}
        self.world.modified.insert((pos.0>>4,pos.2>>4));Ok(())
    }
    /// Handle an authoritative use on the selected physical monitor face.
    pub(super) fn ae2_try_monitor_use(&mut self,pos:Pos,face:&str)->bool {
        let Some(state)=self.ae2_monitor_state(pos,face) else{return false};
        let alternate=self.keys.contains(&KeyCode::ShiftLeft);
        let held=self.held().cloned();
        if !self.ae2_part_online(pos,face){return false;}
        if alternate {
            if held.is_none() {
                let _=self.ae2_monitor_write(pos,face,state.key.as_ref().map(|(k,f)|(k,*f)),!state.locked);
                return true;
            }
            return false;
        }
        if state.conversion {
            if let Some(stack)=held.as_ref() {
                let matches=state.key.as_ref().is_some_and(|(key,fluid)|!*fluid && ae2_lab::key(stack).is_ok_and(|k|k==*key));
                if state.locked || matches {
                    if self.ae2_wrench_can_rotate(stack) && state.locked && !matches {return false;}
                    if let Ok(moved)=self.ae2_grid_insert(pos,stack,u64::from(stack.count)) {
                        let slot=self.held_slot();if let Some(item)=self.stack_in_mut(slot) {item.count=item.count.saturating_sub(moved as u32);if item.count==0 {*self.stack_in_mut(slot)=None;}}
                    }
                    return true;
                }
            } else if state.locked {
                if let Some((key,false))=state.key.as_ref(){let _=self.ae2_monitor_insert_inventory(pos,key);}
                return true;
            }
        }
        let Some(stack)=held else{return false};
        if state.locked{return false;}
        let Ok(item)=ae2_lab::key(&stack) else{return true};
        let configured=if state.key.as_ref().is_some_and(|(key,fluid)|!*fluid && *key==item) {
            super::ae2_fluids::bucket_fluid(&stack.item).map(|id|(super::ae2_fluids::fluid_key(id),true)).unwrap_or((item,false))
        } else {(item,false)};
        let _=self.ae2_monitor_write(pos,face,Some((&configured.0,configured.1)),false);
        true
    }
    fn ae2_monitor_insert_inventory(&mut self,pos:Pos,key:&ItemKey)->Result<(),String> {
        let before=self.ae2_snapshot_blocks();let old=self.inventory.clone();let mut plan=super::ae2_buses::FormationPlan::default();
        let result=(||{
            for i in 0..self.inventory.len() {
                let Some(stack)=self.inventory[i].clone() else{continue};
                if ae2_lab::key(&stack).ok().as_ref()!=Some(key){continue;}
                let moved=self.ae2_grid_insert_staged(pos,&stack,u64::from(stack.count),&mut plan)?;
                if moved>u64::from(stack.count){return Err("monitor inserted too much".into());}
                self.inventory[i]=(moved<u64::from(stack.count)).then_some(ItemStack{count:stack.count-moved as u32,..stack});
            }
            self.ae2_commit_formation(plan)
        })();
        if result.is_err(){self.ae2_restore_blocks(before);self.inventory=old;}result
    }
    /// ConversionMonitorPart.onClicked/onShiftClicked: take a real stored
    /// stack (or one item on secondary use), then give/drop any overflow.
    pub(super) fn ae2_try_monitor_attack(&mut self,pos:Pos,face:&str)->bool {
        let Some(state)=self.ae2_monitor_state(pos,face) else{return false};
        if !state.conversion || !self.ae2_part_online(pos,face){return false;}
        let Some((key,false))=state.key else{return true};
        let Ok(prototype)=ae2_lab::prototype(&key) else{return true};
        let max=self.res.items().stack_max(&prototype);
        let amount=if self.keys.contains(&KeyCode::ShiftLeft){1}else{max};
        if let Ok(Some(item))=self.ae2_grid_extract(pos,&key,u64::from(amount)) {
            if let Some(rest)=self.give(item) && let Some(eye)=self.eye_position() {
                self.spawn_item(rest,[eye[0],eye[1]-0.3,eye[2]],[0.0,0.1,0.0],40);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn face_state_keeps_item_components_lock_and_rejects_duplicate_faces() {
        let mut machine=horizon_client::ae2_world::Machine::new(horizon_client::ae2_world::MachineKind::CableBus);
        let key=ae2_lab::key(&ItemStack::new("minecraft:diamond",1)).unwrap();
        let part=Tag::compound([("id",Tag::String("ae2:storage_monitor".into())),("face",Tag::String("north".into())),("ae2_monitor_locked",Tag::Byte(1)),("ae2_monitor_key",encoded_key(&key,false))]);
        machine.config.insert("parts".into(),Tag::List(vec![part.clone()]));
        let entity=BlockEntity::Ae2(Box::new(machine.clone()));
        let state=decode(read_part(&entity,"north").unwrap()).unwrap();
        assert!(state.locked);assert!(!state.conversion);assert_eq!(state.key,Some((key,false)));
        machine.config.insert("parts".into(),Tag::List(vec![part.clone(),part]));
        assert!(read_part(&BlockEntity::Ae2(Box::new(machine)),"north").is_none());
    }
}
