//! AE2 19.2.18 ItemUpgradeInventory / UpgradeInventory, LGPL-3.0-or-later.
//! Full physical one-card slots; legacy dense lists retain their old indices.
use crate::{items::ItemStack, save};
use horizon_world::nbt::{Compound, Tag};

/// The size of an item's upgrade inventory, from the definitions registry.
pub fn capacity(id: &str) -> Option<usize> {
    crate::ae2x::defs::get().upgrade_slots(id)
}
/// How many of `card` the item `owner` takes (`Upgrades.add` rows).
fn limit(owner: &str, card: &str) -> usize {
    crate::ae2x::defs::get().upgrade_limit(card, owner)
}
pub fn decode(cards: &[Tag], max: usize) -> Result<Vec<Option<ItemStack>>, String> {
    let mut slots = vec![None; max];
    for (legacy, tag) in cards.iter().enumerate() {
        let index = match tag.get("Slot") {
            None => legacy,
            Some(Tag::Int(i)) if *i >= 0 => *i as usize,
            _ => return Err("invalid item-upgrade slot".into()),
        };
        let cell = slots
            .get_mut(index)
            .ok_or("item-upgrade slot outside inventory")?;
        if cell.is_some() || tag.get("count") != Some(&Tag::Int(1)) {
            return Err("duplicate or stacked item upgrade".into());
        }
        *cell = Some(save::item_from_nbt(tag).ok_or("invalid physical item upgrade")?);
    }
    Ok(slots)
}
pub fn encode(slots: &[Option<ItemStack>]) -> Result<Vec<Tag>, String> {
    slots
        .iter()
        .enumerate()
        .filter_map(|(slot, item)| item.as_ref().map(|item| (slot, item)))
        .map(|(slot, item)| {
            if item.count != 1 {
                return Err("item upgrades occupy one physical slot".into());
            }
            let mut tag = save::item_to_nbt(item);
            let Tag::Compound(map) = &mut tag else {
                return Err("invalid upgrade encoding".into());
            };
            map.insert("Slot".into(), Tag::Int(slot as i32));
            Ok(tag)
        })
        .collect()
}
/// ItemContainerContents.CODEC stores sparse `{slot, item}` compounds.
pub fn decode_source(cards: &[Tag], max: usize) -> Result<Vec<Option<ItemStack>>, String> {
    let mut slots = vec![None; max];
    for entry in cards {
        let Some(Tag::Int(index)) = entry.get("slot") else { return Err("invalid source upgrade slot".into()); };
        let index = usize::try_from(*index).map_err(|_| "invalid source upgrade slot")?;
        let target = slots.get_mut(index).ok_or("source upgrade slot outside inventory")?;
        if target.is_some() { return Err("duplicate source upgrade slot".into()); }
        let tag=entry.get("item").ok_or("invalid source upgrade item")?;
        if !matches!(tag.get("count"),None|Some(Tag::Byte(1))|Some(Tag::Short(1))|Some(Tag::Int(1))|Some(Tag::Long(1))) {return Err("invalid source upgrade count".into());}
        let item = save::item_from_nbt(tag).ok_or("invalid source upgrade item")?;
        if item.count != 1 { return Err("stacked source upgrade".into()); }
        *target = Some(item);
    }
    Ok(slots)
}
pub fn encode_source(slots: &[Option<ItemStack>]) -> Result<Vec<Tag>, String> {
    slots.iter().enumerate().filter_map(|(slot,item)|item.as_ref().map(|item|(slot,item)))
        .map(|(slot,item)| {
            if item.count != 1 { return Err("stacked source upgrade".into()); }
            Ok(Tag::compound([("slot",Tag::Int(slot as i32)),("item",save::item_to_nbt(item))]))
        }).collect()
}
#[derive(Clone, Debug)]
pub struct Inventory {
    owner: String,
    pub slots: Vec<Option<ItemStack>>,
}
impl Inventory {
    pub fn read(stack: &ItemStack) -> Result<Self, String> {
        let max = capacity(&stack.item).ok_or("item has no upgrade inventory")?;
        if stack.count != 1 {
            return Err("item upgrade owner must be singular".into());
        }
        let custom = match stack.data().and_then(|d| d.other("minecraft:custom_data")) {
            None => Compound::new(),
            Some(Tag::Compound(map)) => map.clone(),
            _ => return Err("invalid upgrade owner custom data".into()),
        };
        let slots = if let Some(source) = stack.data().and_then(|data|data.other("ae2:upgrades")) {
            let Tag::List(cards) = source else { return Err("invalid source upgrades component".into()); };
            decode_source(cards,max)?
        } else if stack.item.starts_with("ae2:wireless_") {
            match custom.get(&"ae2_wireless_upgrades".into()) {
                None => vec![None; max],
                Some(Tag::List(cards)) => decode(cards, max)?,
                _ => return Err("invalid upgrades component".into()),
            }
        } else {
            let settings = crate::ae2_cell_codec::cell_settings(stack)?;
            let Some(Tag::List(cards)) = settings.get(&"upgrades".into()) else {
                return Err("invalid upgrades component".into());
            };
            decode(cards, max)?
        };
        let inventory = Self {
            owner: stack.item.to_string(),
            slots,
        };
        inventory.validate()?;
        Ok(inventory)
    }
    fn validate(&self) -> Result<(), String> {
        if capacity(&self.owner) != Some(self.slots.len()) {
            return Err("wrong upgrade inventory capacity".into());
        }
        let mut counts = std::collections::BTreeMap::new();
        for card in self.slots.iter().flatten() {
            if card.count != 1 {
                return Err("stacked item upgrade".into());
            }
            let count = counts.entry(card.item.as_ref()).or_insert(0);
            *count += 1;
            if *count > limit(&self.owner, &card.item) {
                return Err("unsupported or excessive item upgrade".into());
            }
        }
        Ok(())
    }
    pub fn insert_one(&mut self, card: &ItemStack) -> bool {
        let count = self
            .slots
            .iter()
            .flatten()
            .filter(|s| s.item == card.item)
            .count();
        if card.count == 0 || count >= limit(&self.owner, &card.item) {
            return false;
        }
        let Some(slot) = self.slots.iter_mut().find(|s| s.is_none()) else {
            return false;
        };
        *slot = Some(ItemStack {
            count: 1,
            ..card.clone()
        });
        true
    }
    pub fn remove_first(&mut self) -> Option<ItemStack> {
        self.slots.iter_mut().find(|s| s.is_some())?.take()
    }
    pub fn store(&self, stack: &mut ItemStack) -> Result<(), String> {
        if stack.item.as_ref() != self.owner || stack.count != 1 {
            return Err("wrong item-upgrade owner".into());
        }
        self.validate()?;
        let source_cards = crate::ae2_components::normalize("ae2:upgrades",Tag::List(encode_source(&self.slots)?))?;
        let mut custom = match stack.data().and_then(|d| d.other("minecraft:custom_data")) {
            None => Compound::new(),
            Some(Tag::Compound(map)) => map.clone(),
            _ => return Err("invalid upgrade owner data".into()),
        };
        let cards = Tag::List(encode(&self.slots)?);
        if stack.item.starts_with("ae2:wireless_") {
            custom.insert("ae2_wireless_upgrades".into(), cards);
        } else {
            let mut settings = crate::ae2_cell_codec::cell_settings(stack)?;
            settings.insert("upgrades".into(), cards);
            custom.insert("ae2_cell_settings".into(), Tag::Compound(settings));
        }
        // Source onUpgradesChanged resets capacity and permanently clamps
        // excess charge; removing and reinstalling a card cannot restore it.
        let energy_cards = self
            .slots
            .iter()
            .flatten()
            .filter(|s| s.item.as_ref() == "ae2:energy_card")
            .count() as f64;
        let power = if let Some((base, _)) = crate::ae2_powered_items::profile(stack) {
            Some((base, base * (1.0 + 8.0 * energy_cards), "ae2:stored_energy"))
        } else if stack.item.starts_with("ae2:portable_") {
            Some((
                crate::ae2_common_config::batteries().portable_cell,
                crate::ae2_common_config::batteries().portable_cell * (1.0 + 8.0 * energy_cards),
                "ae2_portable_energy",
            ))
        } else if stack.item.starts_with("ae2:wireless_") {
            Some((
                crate::ae2_common_config::wireless().terminal_capacity,
                crate::ae2_common_config::wireless().terminal_capacity * (1.0 + energy_cards),
                "ae2_wireless_energy",
            ))
        } else {
            None
        };
        let mut capacity_component = None;
        let mut storage_component_change = None;
        if let Some((base, maximum, key)) = power {
            if (maximum - base).abs() < 0.0001 {
                custom.remove(&"ae2:energy_capacity".into());
            } else {
                custom.insert("ae2:energy_capacity".into(), Tag::Double(maximum.to_bits()));
                capacity_component = Some(Tag::Double(maximum.to_bits()));
            }
            let current = crate::ae2_powered_items::component_power_value(stack, "ae2:stored_energy", key, 0.0)?;
            let next = current.min(maximum);
            if current > maximum || stack.data().and_then(|data|data.other("ae2:stored_energy")).is_some() {
                if next < 0.0001 { custom.remove(&key.into()); }
                else { custom.insert(key.into(), Tag::Double(next.to_bits())); }
                if current > maximum { storage_component_change = Some((next >= 0.0001).then(|| Tag::Double(next.to_bits()))); }
            }
        }
        stack
            .data_mut()
            .set_other("minecraft:custom_data", Some(Tag::Compound(custom)));
        stack.data_mut().set_other("ae2:upgrades", Some(source_cards));
        if power.is_some() {
            stack.data_mut().set_other("ae2:energy_capacity", capacity_component);
        }
        if let Some(value) = storage_component_change {
            stack.data_mut().set_other("ae2:stored_energy", value);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_upgrade_slots_override_legacy_and_roundtrip_physical_components() {
        let mut owner=ItemStack::new("ae2:matter_cannon",1);
        let mut card=ItemStack::new("ae2:energy_card",1);card.data_mut().custom_name = Some(crate::item_data::text_name("Kept card"));
        owner.data_mut().set_other("ae2:upgrades",Some(Tag::List(vec![Tag::compound([("slot",Tag::Int(2)),("item",save::item_to_nbt(&card))])])));
        owner.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("ae2_cell_settings",Tag::compound([("upgrades",Tag::List(vec![])),("filters",Tag::List(vec![]))]))])));
        owner.data_mut().set_other("ae2:stored_energy",Some(Tag::Double(200_000.0f64.to_bits())));
        let mut upgrades=Inventory::read(&owner).unwrap();assert_eq!(upgrades.slots[2],Some(card.clone()));
        upgrades.store(&mut owner).unwrap();
        let reloaded=save::item_from_nbt(&save::item_to_nbt(&owner)).unwrap();
        assert_eq!(Inventory::read(&reloaded).unwrap().slots[2],Some(card));
        assert_eq!(crate::ae2_powered_items::energy(&reloaded).unwrap(),(200_000.0,1_800_000.0));
        upgrades.slots[2]=None;upgrades.store(&mut owner).unwrap();
        assert_eq!(crate::ae2_powered_items::energy(&owner).unwrap(),(200_000.0,200_000.0));
        let mut bad=owner.clone();bad.data_mut().set_other("ae2:upgrades",Some(Tag::String("invalid".into())));
        assert!(Inventory::read(&bad).is_err());
    }
    #[test]
    fn physical_upgrade_holes_keep_source_removal_and_first_empty_insertion_order() {
        let mut tool = ItemStack::new("ae2:matter_cannon", 1);
        tool.data_mut().custom_name = Some(crate::item_data::text_name("Actual tool"));
        let mut first = ItemStack::new("ae2:speed_card", 64);
        first.data_mut().custom_name = Some(crate::item_data::text_name("Marked"));
        let mut inventory = Inventory::read(&tool).unwrap();
        assert!(inventory.insert_one(&first));
        assert!(inventory.insert_one(&ItemStack::new("ae2:energy_card", 1)));
        inventory.store(&mut tool).unwrap();
        let mut inventory = Inventory::read(&tool).unwrap();
        let removed = inventory.remove_first().unwrap();
        assert_eq!(removed.count, 1);
        assert_eq!(
            removed.data().unwrap().custom_name_plain().as_deref(),
            Some("Marked")
        );
        inventory.store(&mut tool).unwrap();
        let mut inventory = Inventory::read(&tool).unwrap();
        assert!(inventory.slots[0].is_none());
        assert_eq!(
            inventory.slots[1].as_ref().unwrap().item.as_ref(),
            "ae2:energy_card"
        );
        assert!(inventory.insert_one(&first));
        inventory.store(&mut tool).unwrap();
        let mut inventory = Inventory::read(&tool).unwrap();
        assert_eq!(inventory.remove_first().unwrap(), removed);
        assert_eq!(
            tool.data().unwrap().custom_name_plain().as_deref(),
            Some("Actual tool")
        );
        assert!(!inventory.insert_one(&ItemStack::new("minecraft:diamond", 1)));
        let legacy = vec![save::item_to_nbt(&removed)];
        assert_eq!(decode(&legacy, 4).unwrap()[0], Some(removed));
        let duplicate = vec![legacy[0].clone(), legacy[0].clone()];
        let mut duplicate = duplicate;
        for tag in &mut duplicate {
            if let Tag::Compound(map) = tag {
                map.insert("Slot".into(), Tag::Int(0));
            }
        }
        assert!(decode(&duplicate, 4).is_err());
    }
    #[test]
    fn powered_upgrade_callback_caches_capacity_and_clamps_only_on_store() {
        for (id, base, energy_key) in [
            ("ae2:portable_item_cell_1k", crate::ae2_common_config::batteries().portable_cell, "ae2_portable_energy"),
            ("ae2:matter_cannon", crate::ae2_common_config::batteries().matter_cannon, "ae2:stored_energy"),
            ("ae2:color_applicator", crate::ae2_common_config::batteries().color_applicator, "ae2:stored_energy"),
            ("ae2:wireless_terminal", crate::ae2_common_config::wireless().terminal_capacity, "ae2_wireless_energy"),
        ] {
            let mut item = ItemStack::new(id, 1);
            let mut data = Compound::new();
            data.insert(energy_key.into(), Tag::Double((base * 3.0).to_bits()));
            data.insert("ae2:energy_capacity".into(), Tag::Double((base * 4.0).to_bits()));
            item.data_mut().set_other("minecraft:custom_data", Some(Tag::Compound(data)));
            let mut inventory = Inventory::read(&item).unwrap();
            assert!(inventory.insert_one(&ItemStack::new("ae2:energy_card", 1)));
            inventory.store(&mut item).unwrap();
            let data = item.data().unwrap().other("minecraft:custom_data").unwrap();
            let multiplier = if id == "ae2:wireless_terminal" { 2.0 } else { 9.0 };
            assert_eq!(data.get("ae2:energy_capacity"), Some(&Tag::Double((base * multiplier).to_bits())));
            inventory.remove_first();
            inventory.store(&mut item).unwrap();
            let data = item.data().unwrap().other("minecraft:custom_data").unwrap();
            assert!(data.get("ae2:energy_capacity").is_none());
            assert_eq!(data.get(energy_key), Some(&Tag::Double(base.to_bits())));
        }
    }
    #[test]
    fn removing_energy_upgrade_permanently_clamps_charge_before_reinstallation() {
        let mut cannon = ItemStack::new("ae2:matter_cannon", 1);
        let mut inventory = Inventory::read(&cannon).unwrap();
        let card = ItemStack::new("ae2:energy_card", 1);
        assert!(inventory.insert_one(&card));
        inventory.store(&mut cannon).unwrap();
        crate::ae2_powered_items::transfer(&mut cannon, 1_000_000.0, true, false).unwrap();
        assert_eq!(
            crate::ae2_powered_items::energy(&cannon).unwrap().0,
            1_000_000.0
        );
        let mut inventory = Inventory::read(&cannon).unwrap();
        inventory.remove_first();
        inventory.store(&mut cannon).unwrap();
        assert_eq!(
            crate::ae2_powered_items::energy(&cannon).unwrap(),
            (200_000.0, 200_000.0)
        );
        assert!(inventory.insert_one(&card));
        inventory.store(&mut cannon).unwrap();
        assert_eq!(
            crate::ae2_powered_items::energy(&cannon).unwrap().0,
            200_000.0
        );
    }
}
