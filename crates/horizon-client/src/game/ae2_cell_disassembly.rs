//! Source BasicStorageCell / AbstractPortableCell disassembly (AE2 19.2.18,
//! LGPL-3.0-or-later). Outputs come from the active cell-disassembly recipes.
use super::{Game, KeyCode};
use horizon_client::{
    ae2_cell_codec as codec, ae2_upgrade_inventory::Inventory, crafting::Template,
    first_person::Hand, items::ItemStack, text::Text,
};
use horizon_world::nbt::{Compound, Tag};
enum Prepared {
    Unsupported,
    Filled,
    Ready(Vec<ItemStack>),
}
fn prepare(
    stack: &ItemStack,
    recipes: &[(String, serde_json::Value)],
    mut energy: f64,
) -> Result<Prepared, String> {
    let Some((_, recipe)) = recipes
        .iter()
        .find(|(_, recipe)| recipe["cell"].as_str() == Some(stack.item.as_ref()))
    else {
        return Ok(Prepared::Unsupported);
    };
    let Some(outputs) = recipe["cell_disassembly_items"]
        .as_array()
        .filter(|v| !v.is_empty())
    else {
        return Ok(Prepared::Unsupported);
    };
    if stack.count != 1 {
        return Err("cell disassembly requires one physical cell".into());
    }
    let cell = if codec::is_fluid_cell(&stack.item) {
        codec::read_fluid_cell(stack)?
    } else {
        codec::read_cell(stack)?
    };
    if cell.total_items()!=0 {
        return Ok(Prepared::Filled);
    }
    let upgrades = Inventory::read(stack)?;
    let mut results = Vec::new();
    for output in outputs {
        let mut item = Template::parse(output)
            .ok_or("invalid cell-disassembly output")?
            .create();
        if item.count == 0 {
            return Err("empty cell-disassembly output".into());
        }
        let maximum = match item.item.as_ref() {
            "ae2:energy_cell" => Some(200_000.0),
            "ae2:dense_energy_cell" => Some(1_600_000.0),
            _ => None,
        };
        if let Some(maximum) = maximum.filter(|_| energy > 0.0) {
            let mut data = match item.data().and_then(|d| d.other("minecraft:custom_data")) {
                None => Compound::new(),
                Some(Tag::Compound(map)) => map.clone(),
                _ => return Err("invalid disassembly battery data".into()),
            };
            let existing = match data.get(&"ae2:stored_energy".into()) {
                None => 0.0,
                Some(Tag::Double(bits)) => f64::from_bits(*bits),
                _ => return Err("invalid disassembly battery charge".into()),
            };
            if !existing.is_finite() || existing < 0.0 || existing > maximum {
                return Err("invalid disassembly battery charge".into());
            }
            let moved = energy.min(maximum - existing);
            energy -= moved;
            data.insert(
                "ae2:stored_energy".into(),
                Tag::Double((existing + moved).to_bits()),
            );
            item.data_mut()
                .set_other("minecraft:custom_data", Some(Tag::Compound(data)));
        }
        results.push(item);
    }
    results.extend(upgrades.slots.into_iter().flatten());
    Ok(Prepared::Ready(results))
}
impl Game<'_> {
    /// onItemUseFirst precedes block interaction. Portable use consumes a
    /// filled-cell rejection so its normal air menu cannot hide the message.
    pub(super) fn ae2_cell_item_use(&mut self, air: bool) -> bool {
        if self.remote.is_some() {
            return false;
        }
        let Some(stack) = self.held().filter(|s| codec::is_cell(&s.item)).cloned() else {
            return false;
        };
        let portable = stack.item.starts_with("ae2:portable_");
        if !self.keys.contains(&KeyCode::ShiftLeft) || self.hand != Hand::Main || stack.count != 1 {
            return air && !portable;
        }
        let energy = if portable {
            super::ae2_interactions::portable_energy(&stack).map(|v| v.0)
        } else {
            Ok(0.0)
        };
        let prepared = energy.and_then(|energy| {
            prepare(
                &stack,
                self.res
                    .recipes()
                    .ae2_custom
                    .get("ae2:storage_cell_disassembly")
                    .map_or(&[][..], Vec::as_slice),
                energy,
            )
        });
        match prepared {
            Ok(Prepared::Unsupported) => air && !portable,
            Ok(Prepared::Filled) => {
                self.system_message(Text::translate(
                    "chat.ae2.OnlyEmptyCellsCanBeDisassembled",
                    &[],
                ));
                portable || air
            }
            Err(error) => {
                self.command_failure(Text::literal(format!("AE2 cell: {error}")));
                true
            }
            Ok(Prepared::Ready(outputs)) => {
                if self.eye_position().is_none()
                    || outputs
                        .iter()
                        .any(|s| self.res.items().get(&s.item).is_none())
                {
                    return true;
                }
                *self.held_mut() = None;
                for output in outputs {
                    if let Some(rest) = self.give(output) {
                        self.player_drop(rest);
                    }
                }
                true
            }
        }
    }
    pub(super) fn ae2_predict_cell_disassembly(&self, stack: &ItemStack) -> bool {
        if !self.keys.contains(&KeyCode::ShiftLeft)
            || stack.count != 1
            || !codec::is_cell(&stack.item)
        {
            return false;
        }
        if stack.item.starts_with("ae2:portable_") {
            return true;
        }
        matches!(
            prepare(
                stack,
                self.res
                    .recipes()
                    .ae2_custom
                    .get("ae2:storage_cell_disassembly")
                    .map_or(&[][..], Vec::as_slice),
                0.0
            ),
            Ok(Prepared::Ready(_))
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disassembly_uses_active_recipe_recovers_physical_cards_and_portable_charge() {
        let recipes = vec![(
            "ae2:disassemble_test".into(),
            serde_json::json!({"cell":"ae2:portable_item_cell_1k","cell_disassembly_items":[{"id":"ae2:energy_cell","count":1},{"id":"ae2:cell_component_1k","count":1}]}),
        )];
        let mut holder = ItemStack::new("ae2:portable_item_cell_1k", 1);
        let mut inv = Inventory::read(&holder).unwrap();
        let mut card = ItemStack::new("ae2:energy_card", 1);
        card.data_mut().custom_name = Some(horizon_client::item_data::text_name("Actual"));
        assert!(inv.insert_one(&card));
        inv.store(&mut holder).unwrap();
        let Prepared::Ready(items) = prepare(&holder, &recipes, 12345.0).unwrap() else {
            panic!("recipe absent")
        };
        assert_eq!(items.len(), 3);
        assert_eq!(items[2], card);
        assert_eq!(
            items[0]
                .data()
                .unwrap()
                .other("minecraft:custom_data")
                .unwrap()
                .get("ae2:stored_energy"),
            Some(&Tag::Double(12345.0f64.to_bits()))
        );
        let mut cell = codec::read_cell(&holder).unwrap();
        let key = codec::key(&ItemStack::new("minecraft:diamond", 1)).unwrap();
        cell.insert(&key, 1, ae2_cell_core::Action::Modulate);
        codec::write_cell(&mut holder, &cell).unwrap();
        assert!(matches!(
            prepare(&holder, &recipes, 12345.0).unwrap(),
            Prepared::Filled
        ));
    }
    /// MEGA Cells' `ae2:storage_cell_disassembly` recipes return the
    /// component and the MEGA housing, and the cards.
    #[cfg(feature = "ae2-megacells")]
    #[test]
    fn mega_cells_disassemble_by_their_own_recipes() {
        let recipes = vec![(
            "megacells:cells/standard/fluid_storage_cell_64m_disassembly".into(),
            serde_json::json!({"type":"ae2:storage_cell_disassembly","cell":"megacells:fluid_storage_cell_64m","cell_disassembly_items":[{"count":1,"id":"megacells:cell_component_64m"},{"count":1,"id":"megacells:mega_fluid_cell_housing"}]}),
        )];
        let mut holder = ItemStack::new("megacells:fluid_storage_cell_64m", 1);
        let mut inv = Inventory::read(&holder).unwrap();
        assert_eq!(inv.slots.len(), 3);
        assert!(inv.insert_one(&ItemStack::new("ae2:void_card", 1)));
        assert!(!inv.insert_one(&ItemStack::new("ae2:fuzzy_card", 1)));
        inv.store(&mut holder).unwrap();
        let Prepared::Ready(items) = prepare(&holder, &recipes, 0.0).unwrap() else {
            panic!("recipe absent")
        };
        let ids: Vec<_> = items.iter().map(|item| item.item.as_ref()).collect();
        assert_eq!(ids, ["megacells:cell_component_64m", "megacells:mega_fluid_cell_housing", "ae2:void_card"]);
        let mut cell = codec::read_fluid_cell(&holder).unwrap();
        cell.insert(&codec::fluid_key("minecraft:water"), 1000, ae2_cell_core::Action::Modulate);
        codec::write_fluid_cell(&mut holder, &cell).unwrap();
        assert!(matches!(prepare(&holder, &recipes, 0.0).unwrap(), Prepared::Filled));
    }
}
