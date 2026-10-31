//! Opt-in, local AE2 storage-cell experiment. The LGPL cell algorithm lives
//! in the separately attributed ae2-cell-core crate; this is engine glue.
//! Commands operate on the authoritative player's inventory, never a sidecar
//! item counter. Normal ItemStack NBT persists the cell and its contents.
use super::Game;
use ae2_cell_core::Action;
#[cfg(test)]
use ae2_cell_core::Cell;
use horizon_client::items::ItemStack;
#[cfg(test)]
use horizon_client::save;
#[cfg(test)]
use horizon_world::nbt::Tag;
use serde_json::{Value, json};

#[cfg(test)]
pub(super) const CELL_ITEM: &str = "ae2:item_storage_cell_1k";
#[cfg(test)]
const COMPONENT: &str = "minecraft:custom_data";
#[cfg(test)]
const CELL_DATA: &str = "ae2_cell";

#[cfg(test)]
use horizon_client::ae2_cell_codec::nested_cell_empty;
pub(super) use horizon_client::ae2_cell_codec::{key, prototype, read_cell, write_cell};

fn amount(word: Option<&str>, default: u64) -> Result<u64, String> {
    let n = match word {
        Some(s) => s.parse().map_err(|_| "expected a positive item count")?,
        None => default,
    };
    if n == 0 {
        return Err("expected a positive item count".into());
    }
    Ok(n)
}

impl Game<'_> {
    pub(super) fn ae2_lab_command(&mut self, command: &str) -> Result<Value, String> {
        if self.player.is_none() {
            return Err("enter a world first".into());
        }
        let words: Vec<_> = command.split_whitespace().collect();
        if words.get(1) == Some(&"cpu-formation") {
            if words.len() != 5 { return Err("usage: /ae2 cpu-formation x y z".into()); }
            let pos = (words[2].parse().map_err(|_| "invalid x")?, words[3].parse().map_err(|_| "invalid y")?,
                words[4].parse().map_err(|_| "invalid z")?);
            return self.ae2_cpu_formation_status(pos);
        }
        if words.get(1) == Some(&"provider-shadow") {
            if words.len() != 6 { return Err("usage: /ae2 provider-shadow x y z item-id".into()); }
            let pos = (words[2].parse().map_err(|_| "invalid x")?, words[3].parse().map_err(|_| "invalid y")?,
                words[4].parse().map_err(|_| "invalid z")?);
            return self.ae2_provider_shadow_status(pos, words[5]);
        }
        if words.get(1)==Some(&"demand-column"){return self.ae2_demand_column_command(&words[2..]);}
        if words.get(1)==Some(&"eraser-use"){return self.ae2_eraser_use_command(&words[2..]);}
        if words.get(1)==Some(&"compass") && words.get(2)==Some(&"status") {return self.ae2_compass_status(&words[3..]);}
        if words.get(1) == Some(&"emitter") {
            return self.ae2_emitter_command(&words[2..]);
        }
        if words.get(1) == Some(&"power") {
            return self.ae2_power_command(&words[2..]);
        }
        if words.get(1) == Some(&"spatial") {
            return self.ae2_spatial_command(&words[2..]);
        }
        if words.get(1) == Some(&"anchor") {
            return self.ae2_anchor_command(&words[2..]);
        }
        if words.get(1) == Some(&"tank") {
            return self.ae2_tank_command(&words[2..]);
        }
        if words.get(1) == Some(&"bus") {
            return self.ae2_bus_command(&words[2..]);
        }
        if words.get(1) == Some(&"workbench") {
            return self.ae2_workbench_command(&words[2..]);
        }
        if words.get(1) == Some(&"io") {
            return self.ae2_io_command(&words[2..]);
        }
        if words.get(1) == Some(&"condenser") {
            return self.ae2_condenser_command(&words[2..]);
        }
        if words.get(1) == Some(&"fluid") && words.get(2) == Some(&"grid") {
            return self.ae2_fluid_grid_command(&words[3..]);
        }
        if words.get(1) == Some(&"grid") {
            return self.ae2_grid_command(&words[2..]);
        }
        if words.get(1) == Some(&"crafting") {
            return self.ae2_crafting_command(&words[2..]);
        }
        if words.get(1) == Some(&"fluid") {
            return self.ae2_fluid_command(&words[2..]);
        }
        let mut stack = self
            .offhand
            .clone()
            .ok_or("put a 1K storage cell in your offhand")?;
        if stack.item.starts_with("ae2:portable_item_cell_") {
            return Err("open the portable cell to use its powered storage".into());
        }
        let mut cell = read_cell(&stack)?;
        match words.get(1).copied().unwrap_or("status") {
            "status" if words.len() <= 2 => {
                let entries = cell.entries().enumerate().map(|(index, (key, count))| {
                    json!({"index": index, "item": key.item_id, "count": count})
                }).collect::<Vec<_>>();
                Ok(
                    json!({"kind": stack.item.as_ref(), "items": cell.total_items(), "types": entries.len(),
                    "used_bytes": cell.used_bytes(), "capacity_bytes": cell.config().total_bytes(), "entries": entries}),
                )
            }
            "insert" if words.len() <= 3 => {
                let input = self.inventory[self.selected]
                    .clone()
                    .ok_or("hold an item in your main hand")?;
                if !super::ae2_portable_ui::can_store_item(&input)
                    || &*input.item == "minecraft:air"
                {
                    return Err("nonempty or invalid storage cells cannot be inserted".into());
                }
                let requested =
                    amount(words.get(2).copied(), input.count.into())?.min(input.count.into());
                let inserted =
                    self.ae2_cell_insert(&mut cell, &key(&input)?, requested, Action::Modulate);
                write_cell(&mut stack, &cell)?;
                let left = input.count - inserted as u32;
                self.inventory[self.selected] = (left > 0).then_some(ItemStack {
                    count: left,
                    ..input.clone()
                });
                self.offhand = Some(stack);
                Ok(
                    json!({"inserted": inserted, "item": input.item.as_ref(), "items": cell.total_items(), "used_bytes": cell.used_bytes()}),
                )
            }
            "extract" if (3..=4).contains(&words.len()) => {
                let index: usize = words[2]
                    .parse()
                    .map_err(|_| "extract expects the entry index shown by status")?;
                let (item_key, available) = cell
                    .entries()
                    .nth(index)
                    .map(|(k, n)| (k.clone(), *n))
                    .ok_or("entry index is out of range")?;
                let requested = amount(words.get(3).copied(), 64)?
                    .min(available)
                    .min(u32::MAX.into());
                let mut item = prototype(&item_key)?;
                if self.res.items().get(&item.item).is_none() {
                    return Err("stored item is not registered".into());
                }
                item.count = requested as u32;
                let original = self.inventory.clone();
                let remainder = self.give(item);
                let inserted = requested - remainder.as_ref().map_or(0, |s| u64::from(s.count));
                cell.extract(&item_key, inserted, Action::Modulate);
                if let Err(e) = write_cell(&mut stack, &cell) {
                    self.inventory = original;
                    return Err(e);
                }
                self.offhand = Some(stack);
                Ok(
                    json!({"extracted": inserted, "item": item_key.item_id, "items": cell.total_items(), "used_bytes": cell.used_bytes()}),
                )
            }
            _ => Err(
                "usage: /ae2 status | /ae2 insert [count] | /ae2 extract <entry-index> [count]"
                    .into(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn item_components_survive_cell_and_normal_inventory_nbt_round_trip() {
        let mut item = ItemStack::new("minecraft:diamond_sword", 1);
        item.damage = 42;
        item.data_mut().custom_name = Some("Persistent AE2 probe".into());
        item.enchant("minecraft:sharpness", 3);
        let mut cell = Cell::new_1k();
        let k = key(&item).unwrap();
        cell.insert(&k, 1, Action::Modulate);
        let mut stack = ItemStack::new(CELL_ITEM, 1);
        write_cell(&mut stack, &cell).unwrap();
        let reloaded = save::item_from_nbt(&save::item_to_nbt(&stack)).unwrap();
        let cell = read_cell(&reloaded).unwrap();
        let restored = prototype(cell.entries().next().unwrap().0).unwrap();
        assert!(item.same_item_same_components(&restored));
    }
    #[test]
    fn only_empty_nested_storage_cells_round_trip() {
        let empty = ItemStack::new(CELL_ITEM, 1);
        assert!(nested_cell_empty(&empty));
        let mut outer = Cell::new_1k();
        outer.insert(&key(&empty).unwrap(), 1, Action::Modulate);
        let mut outer_stack = ItemStack::new(CELL_ITEM, 1);
        write_cell(&mut outer_stack, &outer).unwrap();
        assert_eq!(read_cell(&outer_stack).unwrap().total_items(), 1);
        assert!(!nested_cell_empty(&outer_stack));
        let mut enclosing = Cell::new_1k();
        enclosing.insert(&key(&outer_stack).unwrap(), 1, Action::Modulate);
        let mut bad = ItemStack::new(CELL_ITEM, 1);
        write_cell(&mut bad, &enclosing).unwrap();
        // BasicCellInventory.innerInsert rejects a nonempty nested cell, but
        // loadCellItems retains one already present in source data. The
        // legacy-only decoder still rejects the same invalid physical entry.
        assert!(!horizon_client::ae2_cell_codec::can_store_item(&outer_stack));
        let loaded=read_cell(&bad).unwrap();assert_eq!(loaded.total_items(),1);
        let nested=prototype(loaded.entries().next().unwrap().0).unwrap();
        assert!(nested.same_item_same_components(&outer_stack));
        assert!(!nested_cell_empty(&nested));
        let mut legacy=bad;
        legacy.data_mut().set_other("ae2:storage_cell_inv",None);
        assert!(read_cell(&legacy).is_err());
    }
    #[test]
    fn corrupt_or_overcapacity_cell_is_rejected_instead_of_creating_items() {
        let item = ItemStack::new("minecraft:diamond", 1);
        for n in [-1, 0, 8129] {
            let mut stack = ItemStack::new(CELL_ITEM, 1);
            stack.data_mut().set_other(
                COMPONENT,
                Some(Tag::compound([(
                    CELL_DATA,
                    Tag::compound([
                        ("version", Tag::Int(1)),
                        (
                            "entries",
                            Tag::List(vec![Tag::compound([
                                ("item", save::item_to_nbt(&item)),
                                ("amount", Tag::Long(n)),
                            ])]),
                        ),
                    ]),
                )])),
            );
            assert!(read_cell(&stack).is_err());
        }
    }

    #[test]
    fn invalid_prototype_count_is_not_repaired_into_a_valid_stored_item() {
        let mut stack = ItemStack::new(CELL_ITEM, 1);
        stack.data_mut().set_other(
            COMPONENT,
            Some(Tag::compound([(
                CELL_DATA,
                Tag::compound([
                    ("version", Tag::Int(1)),
                    (
                        "entries",
                        Tag::List(vec![Tag::compound([
                            (
                                "item",
                                Tag::compound([
                                    ("id", Tag::String("minecraft:diamond".into())),
                                    ("count", Tag::Int(0)),
                                ]),
                            ),
                            ("amount", Tag::Long(1)),
                        ])]),
                    ),
                ]),
            )])),
        );
        assert!(read_cell(&stack).is_err());
    }
}
