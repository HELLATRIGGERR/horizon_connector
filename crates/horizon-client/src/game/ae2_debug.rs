//! AE2 19.2.18 developer ItemGenBlockEntity behavior, LGPL-3.0-or-later.
use super::Game;
use horizon_client::{ae2_world::{Machine, MachineKind}, containers::BlockEntity, items::{ItemStack, Items}};
use horizon_world::nbt::Tag;
use std::cell::Cell;

// The upstream unfiltered generator queue is shared by all instances and is
// runtime state, while a selected item gets its own per-block queue.
thread_local! { static SHARED_CURSOR: Cell<u64> = const { Cell::new(0) }; }

fn sample(items: &Items, filter: Option<&str>, cursor: u64) -> Option<ItemStack> {
    let ids: Vec<_> = match filter {
        Some(id) if id != "minecraft:air" => vec![id],
        _ => items.registry_order.iter().map(|id| id.as_ref()).filter(|id| *id != "minecraft:air").collect(),
    };
    let total = ids.iter().filter_map(|id| items.get(id)).map(|info| u64::from(info.max_damage.max(1))).sum::<u64>();
    if total == 0 { return None; }
    let mut index = cursor % total;
    for id in ids {
        let Some(info) = items.get(id) else { continue };
        let variants = u64::from(info.max_damage.max(1));
        if index < variants {
            let mut item = ItemStack::new(id, info.max_stack.max(1));
            item.damage = index as u32;
            return Some(item);
        }
        index -= variants;
    }
    None
}
fn refill(machine: &mut Machine, items: &Items, slot: usize) {
    let filter = match machine.config.get(&"filter".into()) {
        Some(Tag::String(id)) => id.to_string().ok(), _ => None,
    }.filter(|id| id != "minecraft:air");
    let cursor = if filter.is_some() { machine.progress as u64 } else { SHARED_CURSOR.with(Cell::get) };
    let next = sample(items, filter.as_deref(), cursor);
    if next.is_some() {
        if filter.is_some() { machine.progress = cursor.wrapping_add(1) as f64; }
        else { SHARED_CURSOR.with(|c| c.set(cursor.wrapping_add(1))); }
    }
    if let Some(target) = machine.slots.get_mut(slot) { *target = next; }
}
/// Apply the source item-handler insertion to a snapshot. The caller controls
/// whether that snapshot is committed, so simulation never changes the machine.
pub(super) fn itemgen_insert(slots: &mut [Option<ItemStack>], stack: &ItemStack, limit: u32, max: u32) -> u32 {
    let mut left = stack.count.min(limit);
    let wanted = left;
    for slot in slots {
        if left == 0 { break; }
        let room = match slot {
            None => max,
            Some(existing) if existing.same_item_same_components(stack) => max.saturating_sub(existing.count),
            _ => 0,
        };
        let moved = left.min(room);
        if moved == 0 { continue; }
        if let Some(existing) = slot { existing.count += moved; }
        else { let mut inserted = stack.clone(); inserted.count = moved; *slot = Some(inserted); }
        left -= moved;
    }
    wanted - left
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn itemgen_insert_respects_components_and_slot_limits() {
        let mut base = ItemStack::new("minecraft:diamond", 60);
        base.data_mut().custom_name = Some(horizon_client::item_data::text_name("original"));
        let mut different = ItemStack::new("minecraft:diamond", 1);
        different.data_mut().custom_name = Some(horizon_client::item_data::text_name("different"));
        let input = ItemStack { count: 9, ..base.clone() };
        let mut slots = vec![Some(base), Some(different.clone()), None];
        assert_eq!(itemgen_insert(&mut slots, &input, 7, 64), 7);
        assert_eq!(slots[0].as_ref().unwrap().count, 64);
        assert_eq!(slots[1], Some(different));
        assert_eq!(slots[2].as_ref().unwrap().count, 3);
        assert!(slots[2].as_ref().unwrap().same_item_same_components(&input));
    }
}
impl Game<'_> {
    pub(super) fn ae2_tick_debug_item_generators(&mut self) {
        let items = self.res.items();
        for be in self.world.block_entities.values_mut() {
            let BlockEntity::Ae2(machine) = be else { continue };
            if machine.kind != MachineKind::DebugItemGenerator { continue; }
            for slot in 0..16 { if machine.slots[slot].is_none() { refill(machine, items, slot); } }
        }
    }
    pub(super) fn ae2_try_debug_item_generator(&mut self, pos: [i32;3]) -> bool {
        let Some(held) = self.held().cloned() else { return false };
        let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&(pos[0],pos[1],pos[2])) else { return false };
        if machine.kind != MachineKind::DebugItemGenerator { return false; }
        machine.config.insert("filter".into(), Tag::String(held.item.as_ref().into()));
        machine.progress = 0.0;
        machine.slots.fill(None);
        for slot in 0..16 { refill(machine, self.res.items(), slot); }
        self.world.modified.insert((pos[0] >> 4, pos[2] >> 4));
        true
    }
    pub(super) fn ae2_debug_refill_slot(&mut self, address: super::ae2_global::GridAddress, slot: usize) {
        let Some((dim, pos)) = self.ae2_address_parts(address) else { return };
        let world = if dim == self.dim { &mut self.world } else {
            let Some(world) = self.levels.ae2_world_mut(dim) else { return };
            world
        };
        let Some(BlockEntity::Ae2(machine)) = world.block_entities.get_mut(&pos) else { return };
        if machine.kind == MachineKind::DebugItemGenerator && machine.slots.get(slot).is_some_and(Option::is_none) {
            refill(machine, self.res.items(), slot);
        }
    }
}

use horizon_client::ae2_stack_identity::StackRef;
pub(super) struct CubeState { pub incarnation: u64, pub size: i32, pub countdown: i32, pub stack: Option<StackRef> }
impl Default for CubeState {
    fn default() -> Self { Self { incarnation: 0, size: 3, countdown: 200, stack: None } }
}
impl CubeState {
    pub fn click(&mut self, selected: Option<&mut ItemStack>, alternate: bool) {
        match selected.filter(|stack| stack.count != 0) {
            Some(stack) => { self.stack = Some(StackRef::capture(stack)); self.countdown = 200; }
            None => { self.stack = None; self.size = (self.size + if alternate { -1 } else { 1 }).clamp(3, 64); }
        }
    }
    /// Returns the source countdown message, including zero immediately before spawn.
    pub fn tick(&mut self) -> (Option<i32>, bool) {
        if self.stack.as_ref().is_none_or(|stack| stack.read().count == 0) { return (None, false); }
        self.countdown -= 1;
        (if self.countdown % 20 == 0 { Some(self.countdown / 20) } else { None }, self.countdown <= 0)
    }
    /// Source loops are intentionally half-open: size 3 produces 2*2*3=12 targets.
    pub fn offsets(&self) -> impl Iterator<Item = [i32;3]> + '_ {
        let half = self.size / 2;
        (0..self.size).flat_map(move |y| (-half..half).flat_map(move |x| (-half..half).map(move |z| [x, y - 1, z])))
    }
}



#[cfg(test)]
mod cube_tests {
    use super::*;
    #[test]
    fn source_reference_is_distinct_from_split_and_copy_and_empty_suppresses_spawn() {
        let mut selected = ItemStack::new("minecraft:stone", 4);
        let mut cube = CubeState::default();
        cube.click(Some(&mut selected), false);
        let original = cube.stack.as_ref().unwrap().clone();
        let split = original.split(2);
        let copy = original.copy();
        copy.shrink(1);
        assert_eq!(split.read().count, 2);
        assert_eq!(copy.read().count, 1);
        cube.countdown = 1;
        assert_eq!(cube.tick(), (Some(0), true));
        for _ in 0..cube.offsets().count() { original.shrink(1); }
        assert_eq!(original.read().count, 0);
        assert_eq!(split.read().count, 2);
        assert_eq!(copy.read().count, 1);
        cube.countdown = 1;
        assert_eq!(cube.tick(), (None, false));
    }
    #[test]
    fn source_bounds_countdown_and_selected_object_semantics() {
        let mut cube = CubeState::default();
        assert_eq!(cube.offsets().count(), 12);
        cube.click(None, true); assert_eq!(cube.size, 3);
        cube.click(None, false); assert_eq!(cube.size, 4);
        assert_eq!(cube.offsets().count(), 64);
        let mut stack = ItemStack::new("minecraft:stone", 32);
        cube.click(Some(&mut stack), false);
        let retained = cube.stack.as_ref().unwrap().clone();
        assert!(retained.tracks(&stack));
        for _ in 0..199 { assert!(!cube.tick().1); }
        assert_eq!(cube.tick(), (Some(0), true));
        retained.shrink(32);
        assert_eq!(cube.tick(), (None, false));
    }
}
