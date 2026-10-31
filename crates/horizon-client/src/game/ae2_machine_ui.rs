//! Machine upgrade policy and transfer helpers, based on AE2 v19.2.18
//! InitUpgrades.java. Cards remain physical ItemStacks owned by the machine.
use horizon_client::{
    ae2_world::{Machine, MachineKind},
    items::ItemStack,
};

fn max_card(kind: MachineKind, id: &str) -> usize {
    use MachineKind::*;
    match (kind, id) {
        (MolecularAssembler, "ae2:speed_card") => 5,
        (Inscriber, "ae2:speed_card") => 4,
        (VibrationChamber, "ae2:speed_card" | "ae2:energy_card") => 3,
        (IoPort, "ae2:speed_card") => 3,
        (IoPort, "ae2:redstone_card") => 1,
        (Interface, "ae2:crafting_card" | "ae2:fuzzy_card") => 1,
        _ => 0,
    }
}
/// May replace this upgrade slot with one actual card. Per-type limits are
/// checked with the prior contents of the target slot excluded.
pub(super) fn machine_upgrade_allowed(m: &Machine, slot: usize, item: &ItemStack) -> bool {
    if slot >= m.upgrades.len() || item.count == 0 {
        return false;
    }
    let max = max_card(m.kind, &item.item);
    if max == 0 {
        return false;
    }
    let used: usize = m
        .upgrades
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != slot)
        .filter_map(|(_, s)| s.as_ref())
        .filter(|s| s.item == item.item)
        .map(|s| s.count as usize)
        .sum();
    used < max
}
/// Cursor pickup/place: each slot holds one actual card. Invalid inputs leave
/// both sides untouched; no client prediction or copied physical stacks.
pub(super) fn machine_upgrade_pickup(
    m: &mut Machine,
    slot: usize,
    carried: &mut Option<ItemStack>,
) -> bool {
    if slot >= m.upgrades.len() {
        return false;
    }
    if carried.is_none() {
        *carried = m.upgrades[slot].take();
        return true;
    }
    let Some(held) = carried.as_ref() else {
        return false;
    };
    if !machine_upgrade_allowed(m, slot, held) {
        return false;
    }
    if m.upgrades[slot].is_some() && held.count != 1 {
        return false;
    }
    let mut held = carried.take().unwrap();
    let replacement = ItemStack {
        count: 1,
        ..held.clone()
    };
    held.count -= 1;
    let prior = m.upgrades[slot].replace(replacement);
    *carried = if let Some(prior) = prior {
        Some(prior)
    } else if held.count > 0 {
        Some(held)
    } else {
        None
    };
    true
}
/// Shift-insert cards into empty machine upgrade slots and return transferred
/// count; caller commits player inventory and machine under one server turn.
pub(super) fn machine_upgrade_quick_insert(m: &mut Machine, item: &mut ItemStack) -> u32 {
    let mut moved = 0;
    for i in 0..m.upgrades.len() {
        if item.count == 0 {
            break;
        }
        if m.upgrades[i].is_none() && machine_upgrade_allowed(m, i, item) {
            m.upgrades[i] = Some(ItemStack {
                count: 1,
                ..item.clone()
            });
            item.count -= 1;
            moved += 1;
        }
    }
    moved
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn io_port_cards_obey_limits_and_conserve_cursor() {
        let mut m = Machine::new(MachineKind::IoPort);
        let mut held = Some(ItemStack::new("ae2:redstone_card", 2));
        assert!(machine_upgrade_pickup(&mut m, 0, &mut held));
        assert_eq!(held.as_ref().unwrap().count, 1);
        assert!(!machine_upgrade_pickup(&mut m, 1, &mut held));
        assert!(m.upgrades[1].is_none());
        let mut speed = ItemStack::new("ae2:speed_card", 3);
        assert_eq!(machine_upgrade_quick_insert(&mut m, &mut speed), 2);
        assert_eq!(speed.count, 1);
        assert_eq!(m.installed_speed_cards(), 2);
        assert_eq!(m.physical_contents().len(), 3);
        let mut cursor = None;
        assert!(machine_upgrade_pickup(&mut m, 0, &mut cursor));
        assert_eq!(cursor.unwrap().item.as_ref(), "ae2:redstone_card");
    }
    #[test]
    fn mismatched_upgrade_and_non_card_cannot_replace_existing() {
        let mut m = Machine::new(MachineKind::Inscriber);
        m.upgrades[0] = Some(ItemStack::new("ae2:speed_card", 1));
        let mut held = Some(ItemStack::new("ae2:energy_card", 1));
        assert!(!machine_upgrade_pickup(&mut m, 0, &mut held));
        assert_eq!(
            m.upgrades[0].as_ref().unwrap().item.as_ref(),
            "ae2:speed_card"
        );
        assert_eq!(held.unwrap().item.as_ref(), "ae2:energy_card");
    }
}
