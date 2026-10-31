//! Runtime identities for the upstream debug Cube Generator's live ItemStack reference.
//! These handles are transient: their identities are never written to item NBT or packets.
//!
//! `move` transfers the handle; `copy` and `split` create distinct Java-like objects.
//! A retained handle remains valid after its inventory owner replaces or removes it.
use crate::items::ItemStack;
use std::{cell::{Ref, RefCell}, collections::{HashMap, HashSet}, rc::{Rc, Weak}, sync::Arc};
thread_local! {
    static TRACKED: RefCell<HashMap<usize, Weak<RefCell<ItemStack>>>> = RefCell::new(HashMap::new());
    static DETACHED: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}
fn marker(stack: &ItemStack) -> usize { Arc::as_ptr(&stack.item) as *const u8 as usize }

#[derive(Debug, Clone)]
pub struct StackRef(Rc<RefCell<ItemStack>>);
impl StackRef {
    pub fn new(stack: ItemStack) -> Self { Self(Rc::new(RefCell::new(stack))) }
    /// Mark one real stack object without changing its item ID, components or
    /// wire/NBT form. Value snapshots retain the unique item Arc on moves.
    pub fn capture(stack: &mut ItemStack) -> Self {
        if let Some(existing) = TRACKED.with(|tracked| tracked.borrow().get(&marker(stack)).and_then(Weak::upgrade)) {
            *existing.borrow_mut() = stack.clone();
            return Self(existing);
        }
        detach_identity(stack);
        let reference = Self::new(stack.clone());
        DETACHED.with(|set| { set.borrow_mut().remove(&marker(stack)); });
        TRACKED.with(|tracked| {
            let mut tracked = tracked.borrow_mut();
            tracked.retain(|_, entry| entry.strong_count() != 0);
            tracked.insert(marker(stack), Rc::downgrade(&reference.0));
        });
        reference
    }
    pub fn tracks(&self, stack: &ItemStack) -> bool {
        Arc::ptr_eq(&self.read().item, &stack.item)
    }
    /// Synchronize a tracked owner's updated value. Other equal stacks do not
    /// qualify: identity is not inferred from item IDs or components.
    pub fn synchronize(&self, stack: &ItemStack) -> bool {
        if !self.tracks(stack) { return false; }
        DETACHED.with(|set| { set.borrow_mut().remove(&marker(stack)); });
        *self.0.borrow_mut() = stack.clone();
        true
    }
    pub fn is_known_detached(&self) -> bool { DETACHED.with(|set| set.borrow().contains(&marker(&self.read()))) }
    pub fn read(&self) -> Ref<'_, ItemStack> { self.0.borrow() }
    pub fn marker_strong_count(&self) -> usize { Arc::strong_count(&self.read().item) }
    /// Java ItemStack.copy: same values, independent object identity.
    pub fn copy(&self) -> Self { let mut stack = self.read().clone(); detach_identity(&mut stack); Self::new(stack) }
    /// Java ItemStack.split: a fresh object receives up to `amount`; the
    /// original stays alive and is reduced even when no inventory owns it.
    pub fn split(&self, amount: u32) -> Self {
        let mut original = self.0.borrow_mut();
        let amount = amount.min(original.count);
        let mut split = original.clone();
        split.count = amount;
        detach_identity(&mut split);
        original.count -= amount;
        Self::new(split)
    }
    pub fn shrink(&self, amount: u32) { let mut original = self.0.borrow_mut(); original.count = original.count.saturating_sub(amount); }
    pub fn same_object(&self, other: &Self) -> bool { Rc::ptr_eq(&self.0, &other.0) }
}

/// Keep a retained object current when a tracked real stack changes, including
/// immediately before a zero-count stack is removed from its owning slot.
pub fn synchronize_tracked(stack: &ItemStack) {
    TRACKED.with(|tracked| {
        let key = marker(stack);
        let mut tracked = tracked.borrow_mut();
        match tracked.get(&key).and_then(Weak::upgrade) {
            Some(reference) => { *reference.borrow_mut() = stack.clone(); }
            None => { tracked.remove(&key); }
        }
    });
}

/// A direct slot replacement removes the old object without mutating it.
/// Mark only a tracked old object and synchronize its last real value first.
pub fn mark_detached_by_replacement(old: &ItemStack, new: Option<&ItemStack>) {
    if new.is_some_and(|new| Arc::ptr_eq(&old.item, &new.item)) { return; }
    let exists=TRACKED.with(|entries| entries.borrow().get(&marker(old)).and_then(Weak::upgrade).is_some());
    if exists { synchronize_tracked(old); DETACHED.with(|set| { set.borrow_mut().insert(marker(old)); }); }
}

/// Only tracked originals need a new marker when an operation copies them.
pub fn detach_if_tracked(stack: &mut ItemStack) {
    let tracked = TRACKED.with(|entries| entries.borrow().get(&marker(stack)).and_then(Weak::upgrade).is_some());
    if tracked { detach_identity(stack); }
}

/// The resulting value has the same item ID and components but a fresh
/// runtime identity, as Java ItemStack.copy/split require.
pub fn detach_identity(stack: &mut ItemStack) { stack.item = Arc::from(stack.item.as_ref()); }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direct_replacement_preserves_a_known_detached_live_object() {
        let mut old = ItemStack::new("minecraft:stone", 4);
        let retained = StackRef::capture(&mut old);
        mark_detached_by_replacement(&old, Some(&ItemStack::new("minecraft:dirt", 1)));
        assert!(retained.is_known_detached());
        retained.shrink(1);
        assert_eq!(retained.read().count, 3);
        assert!(retained.synchronize(&old));
        assert!(!retained.is_known_detached());
    }
    #[test]
    fn capturing_the_same_stack_twice_reuses_the_same_live_object() {
        let mut stack = ItemStack::new("minecraft:stone", 8);
        let first = StackRef::capture(&mut stack);
        let second = StackRef::capture(&mut stack);
        assert!(first.same_object(&second));
        assert!(first.tracks(&stack));
        stack.count = 5;
        synchronize_tracked(&stack);
        assert_eq!(first.read().count, 5);
        assert_eq!(second.read().count, 5);
    }
    #[test]
    fn tracked_item_marker_survives_move_without_changing_values_or_wire_data() {
        let mut item = ItemStack::new("minecraft:diamond", 7);
        let before = crate::item_nbt::item_to_nbt(&item);
        let held = StackRef::capture(&mut item);
        assert_eq!(crate::item_nbt::item_to_nbt(&item), before);
        let moved = item.clone();
        assert!(held.tracks(&moved));
        let mut copy = moved.clone();
        detach_identity(&mut copy);
        assert!(!held.tracks(&copy));
        assert!(moved.same_item_same_components(&copy));
        assert!(!held.synchronize(&copy));
        let mut changed = moved; changed.count = 3;
        assert!(held.synchronize(&changed));
        assert_eq!(held.read().count, 3);
    }
    #[test]
    fn moved_reference_keeps_identity_while_split_and_copy_are_independent() {
        let original = StackRef::new(ItemStack::new("minecraft:stone", 10));
        let retained = original.clone();
        let mut slot = Some(original);
        let moved = slot.take().unwrap();
        assert!(retained.same_object(&moved));
        let part = moved.split(3);
        let copied = moved.copy();
        assert!(!retained.same_object(&part));
        assert!(!retained.same_object(&copied));
        assert_eq!(retained.read().count, 7);
        part.shrink(2);
        copied.shrink(4);
        assert_eq!(retained.read().count, 7);
        assert_eq!(part.read().count, 1);
        assert_eq!(copied.read().count, 3);
    }
    #[test]
    fn save_roundtrip_reconstructs_a_distinct_object() {
        let original = StackRef::new(ItemStack::new("minecraft:stone", 4));
        let saved = crate::item_nbt::item_to_nbt(&original.read());
        let loaded = StackRef::new(crate::item_nbt::item_from_nbt(&saved).unwrap());
        assert!(!original.same_object(&loaded));
        loaded.shrink(1);
        assert_eq!(original.read().count, 4);
        assert_eq!(loaded.read().count, 3);
    }
    #[test]
    fn retained_object_survives_replacement_and_full_split_empties_it() {
        let mut inventory = Some(StackRef::new(ItemStack::new("minecraft:stone", 4)));
        let retained = inventory.as_ref().unwrap().clone();
        let old = inventory.replace(StackRef::new(ItemStack::new("minecraft:dirt", 1))).unwrap();
        drop(old);
        assert_eq!(retained.read().count, 4);
        let taken = retained.split(99);
        assert_eq!(retained.read().count, 0);
        assert_eq!(taken.read().count, 4);
        assert_eq!(inventory.unwrap().read().item.as_ref(), "minecraft:dirt");
    }
}
