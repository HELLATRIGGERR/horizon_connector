//! Transactional removal of a single physical multipart face.
use super::{Game,KeyCode};
use horizon_client::{ae2_world::{Machine,MachineKind},containers::BlockEntity};
use horizon_world::nbt::Tag;
fn valid_list(tag:Option<&Tag>,limit:usize)->bool {
    match tag {None=>true,Some(Tag::List(entries)) if entries.len()<=limit=>entries.iter().all(|e|
        matches!(e.get("count"),Some(Tag::Int(n)) if *n>0) && horizon_client::save::item_from_nbt(e).is_some()),_=>false}
}
fn physical_record_valid(part:&Tag)->bool {
    match part.get("upgrades") {
        None=>{},
        Some(Tag::List(entries)) if horizon_client::ae2_upgrade_inventory::decode(entries,5).is_ok()=>{},
        _=>return false,
    }
    if let Some(machine)=part.get("machine") {
        let id=match part.get("id") {Some(Tag::String(id))=>id.to_string_lossy(),_=>return false};
        let upgrade_limit=if id=="ae2:cable_interface" {1} else {0};
        let slot_limit=if id=="ae2:pattern_encoding_terminal" {2} else {9};
        if super::ae2_parts::strict_part_items(machine.get("slots"),slot_limit).is_err()
            || super::ae2_parts::strict_part_items(machine.get("upgrades"),upgrade_limit).is_err() {return false}
        if !valid_list(machine.get("config").and_then(|c|c.get("ae2_provider_return_items")),9){return false}
        if let Some(Tag::Compound(config))=machine.get("config").filter(|c|c.get(horizon_client::ae2_provider_queue::QUEUE).is_some()) {
            if horizon_client::ae2_provider_queue::read(config).is_err(){return false}
        } else if !valid_list(machine.get("config").and_then(|c|c.get("ae2_provider_send_items")),8192){return false}
        if matches!(id.as_str(), "ae2:terminal" | "ae2:crafting_terminal" | "ae2:pattern_encoding_terminal")
            && super::ae2_parts::decode_terminal_view_cells(machine.get("config").and_then(|c| c.get("ae2_view_cells"))).is_err() { return false; }
    }
    true
}
impl Game<'_> {
    /// Sneak-wrench one face, returning items to inventory and dropping overflow.
    pub(super) fn ae2_try_remove_part(&mut self,hit:[i32;3],face:[i32;3])->bool {
        if self.hand != super::Hand::Main { return false; }
        let side=match face {[0,-1,0]=>"down",[0,1,0]=>"up",[0,0,-1]=>"north",[0,0,1]=>"south",[-1,0,0]=>"west",[1,0,0]=>"east",_=>return false};
        if !self.keys.contains(&KeyCode::ShiftLeft)||!self.held().is_some_and(|s|self.ae2_wrench_can_disassemble(s)){return false}
        let pos=(hit[0],hit[1],hit[2]);
        let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get(&pos) else{return false};
        if machine.kind!=MachineKind::CableBus {return false}
        let Some(Tag::List(parts))=machine.config.get(&"parts".into()) else{return false};
        let Some(index)=parts.iter().position(|p|p.get("face")==Some(&Tag::String(side.into()))) else{return false};
        let part=parts[index].clone();
        if !physical_record_valid(&part){return true}
        let mut single=Machine::new(MachineKind::CableBus);
        single.config.insert("parts".into(),Tag::List(vec![part.clone()]));
        let items=super::spill::ae2_physical_part_drops_with_max(&single,&|stack| {
            let default=self.res.items().get(&stack.item).map_or(64,|info|info.max_stack).max(1);
            super::ae2_formation::max_stack(stack,default)
        });
        if items.is_empty(){return true} // malformed/unknown parts stay installed
        let before=self.inventory.clone();
        let mut overflow=Vec::new();
        for item in items {if let Some(rest)=self.give(item){overflow.push(rest);}}
        let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get_mut(&pos) else{self.inventory=before;return true};
        let Some(Tag::List(parts))=machine.config.get_mut(&"parts".into()) else{self.inventory=before;return true};
        if parts.get(index)!=Some(&part){self.inventory=before;return true}
        parts.remove(index);
        let has_terminal=parts.iter().any(|p|matches!(p.get("id"),Some(Tag::String(id)) if id.to_string().ok().is_some_and(|s|s.ends_with("terminal"))));
        let no_parts=parts.is_empty();
        if has_terminal {machine.config.insert("terminal_part".into(),Tag::Byte(1));} else {machine.config.remove(&"terminal_part".into());}
        let no_facades=match machine.config.get(&"facades".into()) { None=>true, Some(Tag::List(v))=>v.is_empty(), _=>false };
        let remove_host=machine.config.get(&"ae2_centerless".into())==Some(&Tag::Byte(1))
            && !machine.config.contains_key(&"cable".into()) && no_parts && no_facades;
        self.world.block_entities.record_ae2_part(pos,part,false);
        self.world.modified.insert((pos.0>>4,pos.2>>4));
        if remove_host { let _=self.set_state_named(hit,"minecraft:air",&[]); }
        for stack in overflow {self.pop_resource(hit,stack);}
        if self.ae2_has_anchor_owners() {
            if let Err(error)=self.ae2_apply_anchor_callbacks(){log::debug!("AE2 anchor callbacks deferred: {error}");}
        }
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::{items::ItemStack,ae2_upgrade_inventory};
    #[test]
    fn provider_send_list_validates_physical_records_before_removal() {
        let item=horizon_client::save::item_to_nbt(&ItemStack::new("minecraft:diamond",2));
        let part=|list|Tag::compound([("id",Tag::String("ae2:cable_pattern_provider".into())),("machine",Tag::compound([("config",Tag::compound([("ae2_provider_send_items",Tag::List(list))]))]))]);
        assert!(physical_record_valid(&part(vec![item])));
        assert!(!physical_record_valid(&part(vec![Tag::compound([("id",Tag::String("minecraft:diamond".into()))])])));
        assert!(!physical_record_valid(&part(vec![horizon_client::save::item_to_nbt(&ItemStack::new("minecraft:diamond",1));8193])));
        let mut config=Default::default();
        horizon_client::ae2_provider_queue::write(&mut config,&[horizon_client::ae2_provider_queue::Entry::Item{prototype:ItemStack::new("minecraft:diamond",1),amount:999_999}]).unwrap();
        let with_queue=|config|Tag::compound([("id",Tag::String("ae2:cable_pattern_provider".into())),("machine",Tag::compound([("config",Tag::Compound(config))]))]);
        assert!(physical_record_valid(&with_queue(config.clone())));
        config.insert("ae2_provider_send_items".into(),Tag::List(vec![horizon_client::save::item_to_nbt(&ItemStack::new("minecraft:diamond",1))]));
        assert!(!physical_record_valid(&with_queue(config)));
    }
    #[test]
    fn malformed_terminal_view_cells_cannot_be_removed() {
        let cell = ItemStack::new("ae2:view_cell", 1);
        let entry = ae2_upgrade_inventory::encode(&[Some(cell), None, None, None, None]).unwrap()[0].clone();
        let part = |cells| Tag::compound([
            ("id", Tag::String("ae2:terminal".into())),
            ("machine", Tag::compound([("config", Tag::compound([("ae2_view_cells", Tag::List(cells))]))])),
        ]);
        assert!(physical_record_valid(&part(vec![entry.clone()])));
        assert!(!physical_record_valid(&part(vec![entry.clone(), entry])));
    }
    #[test]
    fn malformed_part_upgrade_slots_cannot_be_removed() {
        let card=ItemStack::new("ae2:speed_card",1);
        let encoded=ae2_upgrade_inventory::encode(&[None,Some(card)]).unwrap();
        let part=|entries:Vec<Tag>|Tag::compound([("id",Tag::String("ae2:import_bus".into())),("upgrades",Tag::List(entries))]);
        assert!(physical_record_valid(&part(encoded.clone())));
        assert!(!physical_record_valid(&part(vec![encoded[0].clone(),encoded[0].clone()])));
        let mut invalid=encoded[0].clone();
        if let Tag::Compound(map)=&mut invalid {map.insert("Slot".into(),Tag::Int(5));}
        assert!(!physical_record_valid(&part(vec![invalid])));
    }
}
