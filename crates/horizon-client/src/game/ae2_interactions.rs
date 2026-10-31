//! Source-backed AE2 item/block interactions using the acting hand.
use super::{Game, ae2_fluids};
use horizon_client::{
    ae2_world::MachineKind, containers::BlockEntity, items::ItemStack, text::Text,
};

type Pos = (i32, i32, i32);
impl Game<'_> {
    /// Handle a water/lava or empty bucket on a real sky stone tank. This is
    /// called before generic block-menu handling; pass through other items.
    pub(super) fn ae2_try_tank_interaction(&mut self, pos: Pos) -> bool {
        if self.remote.is_some() {
            return false;
        }
        if !matches!(self.world.block_entities.get(&pos),Some(BlockEntity::Ae2(m)) if m.kind==MachineKind::SkyStoneTank)
        {
            return false;
        }
        let Some(source) = self.held().cloned() else {
            return false;
        };
        if !matches!(
            source.item.as_ref(),
            "minecraft:water_bucket" | "minecraft:lava_bucket" | "minecraft:bucket"
        ) {
            return false;
        }
        if let Err(error) = self.ae2_tank_interaction_inner(pos, source) {
            self.command_failure(Text::literal(format!("AE2 tank: {error}")));
        }
        true
    }
    fn ae2_tank_interaction_inner(&mut self, pos: Pos, source: ItemStack) -> Result<(), String> {
        if source.count == 0 || !ae2_fluids::plain_bucket(&source) {
            return Err("use an unmodified bucket".into());
        }
        let (key, result, insert) = if let Some(fluid) = ae2_fluids::bucket_fluid(&source.item) {
            (ae2_fluids::fluid_key(fluid), "minecraft:bucket", true)
        } else {
            let (key, amount) = self.ae2_tank_contents(pos)?.ok_or("tank is empty")?;
            if amount < 1000 || !key.components.is_empty() {
                return Err("tank cannot fill a vanilla bucket".into());
            }
            let bucket =
                ae2_fluids::fluid_bucket(&key.item_id).ok_or("fluid has no supported bucket")?;
            (key, bucket, false)
        };
        if self.res.items().get(result).is_none() {
            return Err("bucket item unavailable".into());
        }
        let possible = if insert {
            self.ae2_tank_fill(pos, &key, 1000, true)?
        } else {
            self.ae2_tank_drain(pos, &key, 1000, true)?
        };
        if possible != 1000 {
            return Err("tank cannot transfer a full bucket".into());
        }
        let inventory = self.inventory.clone();
        let offhand = self.offhand.clone();
        let tank = self
            .world
            .block_entities
            .get(&pos)
            .cloned()
            .ok_or("tank missing")?;
        // For an ordinary single held bucket, return its counterpart to the
        // same hand. A stacked empty bucket needs a free inventory destination.
        if source.count == 1 {
            *self.held_mut() = Some(ItemStack::new(result, 1));
        } else {
            *self.held_mut() = Some(ItemStack {
                count: source.count - 1,
                ..source.clone()
            });
            if self.give(ItemStack::new(result, 1)).is_some() {
                self.inventory = inventory;
                self.offhand = offhand;
                return Err("inventory has no room for the returned bucket".into());
            }
        }
        let moved = if insert {
            self.ae2_tank_fill(pos, &key, 1000, false)
        } else {
            self.ae2_tank_drain(pos, &key, 1000, false)
        };
        if !matches!(moved, Ok(1000)) {
            self.inventory = inventory;
            self.offhand = offhand;
            if let Some(be) = self.world.block_entities.get_mut(&pos) {
                *be = tank;
                self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
            }
            return Err("tank changed during bucket transfer".into());
        }
        Ok(())
    }
}

/// AE2 portable-cell battery helpers. Energy is stored on the actual item;
/// 20,000 AE base and each energy card adds 8x the base capacity.
fn portable_energy_cards(stack: &ItemStack) -> Result<u32, String> {
    if stack.count != 1 {
        return Err("portable cell must be unstackable".into());
    }
    let suffix = stack
        .item
        .strip_prefix("ae2:portable_item_cell_")
        .or_else(|| stack.item.strip_prefix("ae2:portable_fluid_cell_"))
        .ok_or("not a portable cell")?;
    if !["1k", "4k", "16k", "64k", "256k"].contains(&suffix) {
        return Err("unsupported portable cell".into());
    }
    let cards=horizon_client::ae2_upgrade_inventory::Inventory::read(stack)?;
    Ok(cards.slots.iter().flatten().filter(|card|card.item.as_ref()=="ae2:energy_card").count() as u32)
}
pub(super) fn portable_energy(stack: &ItemStack) -> Result<(f64, f64), String> {
    let _=portable_energy_cards(stack)?;
    let max = horizon_client::ae2_powered_items::component_power_value(stack, "ae2:energy_capacity", "ae2:energy_capacity", horizon_client::ae2_common_config::batteries().portable_cell)?;
    let power = horizon_client::ae2_powered_items::component_power_value(stack, "ae2:stored_energy", "ae2_portable_energy", 0.0)?;
    Ok((power, max))
}
pub(super) fn portable_charge_rate(stack: &ItemStack) -> Result<f64, String> {
    Ok(80.0 * (1.0 + f64::from(portable_energy_cards(stack)?)))
}
fn set_portable_energy(stack: &mut ItemStack, power: f64) -> Result<(), String> {
    let _ = portable_energy(stack)?;
    if !power.is_finite() {
        return Err("invalid portable energy".into());
    }
    horizon_client::ae2_powered_items::set_component_power_value(stack, "ae2:stored_energy", "ae2_portable_energy", (power>=0.0001).then_some(power))
}
/// Returns accepted AE; simulation does not mutate the item. Charger must
/// debit exactly this amount from its real buffer/grid and respect charge rate.
pub(super) fn portable_insert_energy(
    stack: &mut ItemStack,
    offered: f64,
    simulate: bool,
) -> Result<f64, String> {
    if !offered.is_finite() || offered < 0.0 {
        return Err("invalid offered energy".into());
    }
    let (current, max) = portable_energy(stack)?;
    let accepted = offered.min((max - current).max(0.0));
    if !simulate {
        set_portable_energy(stack, current + offered.min(max-current))?;
    }
    Ok(accepted)
}
/// Returns extracted AE; callers must apply AE2's menu idle and transfer costs.
pub(super) fn portable_extract_energy(
    stack: &mut ItemStack,
    amount: f64,
    simulate: bool,
) -> Result<f64, String> {
    if !amount.is_finite() || amount < 0.0 {
        return Err("invalid energy amount".into());
    }
    let (current, _) = portable_energy(stack)?;
    let extracted = amount.min(current);
    if !simulate && extracted > 0.0 {
        set_portable_energy(stack, current - extracted)?;
    }
    Ok(extracted)
}
pub(super) fn clamp_portable_energy(stack: &mut ItemStack) -> Result<(), String> {
    let (current, max) = portable_energy(stack)?;
    set_portable_energy(stack, current.min(max))
}
#[cfg(test)]
mod portable_tests {
    use super::*;
    #[test]
    fn source_portable_power_overrides_legacy_and_updates_on_transfer() {
        use horizon_world::nbt::Tag;
        let mut cell=ItemStack::new("ae2:portable_item_cell_1k",1);
        cell.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("ae2_portable_energy",Tag::Double(90.0f64.to_bits()))])));
        cell.data_mut().set_other("ae2:stored_energy",Some(Tag::Double(20.0f64.to_bits())));
        cell.data_mut().set_other("ae2:energy_capacity",Some(Tag::Double(50.0f64.to_bits())));
        assert_eq!(portable_energy(&cell).unwrap(),(20.0,50.0));
        portable_insert_energy(&mut cell,10.0,false).unwrap();
        assert_eq!(portable_energy(&cell).unwrap(),(30.0,50.0));
        assert_eq!(cell.data().unwrap().other("ae2:stored_energy"),Some(&Tag::Double(30.0f64.to_bits())));
        let restored=horizon_client::save::item_from_nbt(&horizon_client::save::item_to_nbt(&cell)).unwrap();
        assert_eq!(portable_energy(&restored).unwrap(),(30.0,50.0));
    }
    #[test]
    fn portable_battery_simulation_limits_and_item_persistence() {
        let mut cell = ItemStack::new("ae2:portable_item_cell_1k", 1);
        assert_eq!(portable_energy(&cell).unwrap(), (0.0, 20000.0));
        assert_eq!(portable_charge_rate(&cell).unwrap(), 80.0);
        assert_eq!(
            portable_insert_energy(&mut cell, 30000.0, true).unwrap(),
            20000.0
        );
        assert_eq!(portable_energy(&cell).unwrap().0, 0.0);
        assert_eq!(
            portable_insert_energy(&mut cell, 30000.0, false).unwrap(),
            20000.0
        );
        assert_eq!(
            portable_extract_energy(&mut cell, 500.0, false).unwrap(),
            500.0
        );
        let nbt = horizon_client::save::item_to_nbt(&cell);
        let restored = horizon_client::save::item_from_nbt(&nbt).unwrap();
        assert_eq!(portable_energy(&restored).unwrap(), (19500.0, 20000.0));
    }
}
