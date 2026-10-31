//! Physical fluid cells and bucket transactions. mB and capacity follow AE2's
//! 1.21.1 NeoForge AEFluidKey/BasicCellInventory. No tank bridge is implied.
use super::Game;
use ae2_cell_core::Action;
#[cfg(test)]
use ae2_cell_core::ItemKey;
use horizon_client::items::ItemStack;
#[cfg(test)]
use horizon_client::save;
#[cfg(test)]
use horizon_world::nbt::Tag;
use serde_json::{Value, json};

#[cfg(test)]
const COMPONENT: &str = "minecraft:custom_data";
#[cfg(test)]
const CELL_DATA: &str = "ae2_fluid_cell";
const BUCKET_MB: u64 = 1000;

pub(super) use horizon_client::ae2_cell_codec::{
    bucket_fluid, fluid_bucket, fluid_key, read_fluid_cell, valid_fluid_id, write_fluid_cell,
};

pub(super) fn plain_bucket(stack: &ItemStack) -> bool {
    stack.same_item_same_components(&ItemStack::new(&stack.item, 1))
}
impl Game<'_> {
    /// Commands operate on the physical offhand cell and selected inventory slot.
    pub(super) fn ae2_fluid_command(&mut self, args: &[&str]) -> Result<Value, String> {
        if self.player.is_none() {
            return Err("enter a world first".into());
        }
        let mut item = self
            .offhand
            .clone()
            .ok_or("hold a fluid cell in your offhand")?;
        if item.item.starts_with("ae2:portable_fluid_cell_") {
            return Err("use the portable cell interface for powered transfers".into());
        }
        let mut cell = read_fluid_cell(&item)?;
        match args {
            [] | ["status"] => {
                let entries=cell.entries().enumerate().map(|(index,(key,amount))|json!({"index":index,"fluid":key.item_id,"amount_mb":amount,"has_components":!key.components.is_empty()})).collect::<Vec<_>>();
                Ok(
                    json!({"kind":item.item.as_ref(),"amount_mb":cell.total_items(),"used_bytes":cell.used_bytes(),"capacity_bytes":cell.config().total_bytes(),"entries":entries}),
                )
            }
            ["insert"] => {
                let input = self.inventory[self.selected]
                    .clone()
                    .ok_or("hold a filled water or lava bucket")?;
                let fluid = bucket_fluid(&input.item)
                    .ok_or("only ordinary water and lava buckets are supported")?;
                if !plain_bucket(&input) {
                    return Err("bucket components cannot be preserved by this transfer".into());
                }
                let key = fluid_key(fluid);
                if cell.insert(&key, BUCKET_MB, Action::Simulate) != BUCKET_MB {
                    return Err("cell cannot accept a full bucket".into());
                }
                cell.insert(&key, BUCKET_MB, Action::Modulate);
                write_fluid_cell(&mut item, &cell)?;
                self.exchange_bucket(&input, "minecraft:bucket")?;
                self.offhand = Some(item);
                Ok(json!({"inserted_mb":BUCKET_MB,"fluid":fluid,"used_bytes":cell.used_bytes()}))
            }
            ["extract", index] => {
                let index: usize = index.parse().map_err(|_| "expected an entry index")?;
                let (key, amount) = cell
                    .entries()
                    .nth(index)
                    .map(|(k, n)| (k.clone(), *n))
                    .ok_or("entry index is out of range")?;
                if amount < BUCKET_MB {
                    return Err("not enough fluid for a full bucket".into());
                }
                if !key.components.is_empty() {
                    return Err("fluid components cannot be preserved in a vanilla bucket".into());
                }
                let filled =
                    fluid_bucket(&key.item_id).ok_or("this fluid has no supported bucket")?;
                let input = self.inventory[self.selected]
                    .clone()
                    .ok_or("hold an empty bucket")?;
                if &*input.item != "minecraft:bucket" || !plain_bucket(&input) {
                    return Err("hold an unmodified empty bucket".into());
                }
                if cell.extract(&key, BUCKET_MB, Action::Simulate) != BUCKET_MB {
                    return Err("not enough fluid for a full bucket".into());
                }
                cell.extract(&key, BUCKET_MB, Action::Modulate);
                write_fluid_cell(&mut item, &cell)?;
                self.exchange_bucket(&input, filled)?;
                self.offhand = Some(item);
                Ok(
                    json!({"extracted_mb":BUCKET_MB,"fluid":key.item_id,"used_bytes":cell.used_bytes()}),
                )
            }
            _ => Err("usage: /ae2 fluid status | insert | extract <entry-index>".into()),
        }
    }
    pub(super) fn exchange_bucket(
        &mut self,
        source: &ItemStack,
        result: &str,
    ) -> Result<(), String> {
        if source.count == 0 {
            return Err("empty source stack".into());
        }
        if self.res.items().get(result).is_none() {
            return Err("result bucket item is unavailable".into());
        }
        let before = self.inventory.clone();
        self.inventory[self.selected] = if source.count == 1 {
            None
        } else {
            Some(ItemStack {
                count: source.count - 1,
                ..source.clone()
            })
        };
        if self.give(ItemStack::new(result, 1)).is_some() {
            self.inventory = before;
            return Err("inventory has no space for the returned bucket".into());
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tiers_preserve_fluid_and_components_through_item_nbt() {
        for (suffix, expected_bytes) in [
            ("1k", 17),
            ("4k", 65),
            ("16k", 257),
            ("64k", 1025),
            ("256k", 4097),
        ] {
            let mut item = ItemStack::new(&format!("ae2:fluid_storage_cell_{suffix}"), 1);
            let mut cell = read_fluid_cell(&item).unwrap();
            let water = fluid_key("minecraft:water");
            let tagged = ItemKey {
                item_id: "test:fluid".into(),
                components: vec![1, 2, 3],
            };
            assert_eq!(cell.insert(&water, 1000, Action::Modulate), 1000);
            assert_eq!(cell.insert(&tagged, 1234, Action::Modulate), 1234);
            write_fluid_cell(&mut item, &cell).unwrap();
            let reloaded = save::item_from_nbt(&save::item_to_nbt(&item)).unwrap();
            let restored = read_fluid_cell(&reloaded).unwrap();
            assert_eq!(restored.count(&water), 1000);
            assert_eq!(restored.count(&tagged), 1234);
            assert_eq!(restored.used_bytes(), expected_bytes);
        }
    }
    #[test]
    fn corrupt_payload_is_rejected_without_changing_item() {
        let mut item = ItemStack::new("ae2:fluid_storage_cell_1k", 1);
        let bad = Tag::compound([
            ("version", Tag::Int(1)),
            (
                "entries",
                Tag::List(vec![Tag::compound([
                    ("fluid", Tag::String("minecraft:water".into())),
                    ("components", Tag::ByteArray(vec![])),
                    ("amount", Tag::Long(8_128_001)),
                ])]),
            ),
        ]);
        item.data_mut()
            .set_other(COMPONENT, Some(Tag::compound([(CELL_DATA, bad)])));
        let before = save::item_to_nbt(&item);
        assert!(read_fluid_cell(&item).is_err());
        assert_eq!(save::item_to_nbt(&item), before);
    }
    #[test]
    fn capacity_simulation_and_bucket_conservation() {
        let item = ItemStack::new("ae2:fluid_storage_cell_1k", 1);
        let mut cell = read_fluid_cell(&item).unwrap();
        let water = fluid_key("minecraft:water");
        assert_eq!(cell.insert(&water, u64::MAX, Action::Simulate), 8_128_000);
        assert_eq!(cell.total_items(), 0);
        assert_eq!(cell.insert(&water, 1000, Action::Modulate), 1000);
        assert_eq!(cell.extract(&water, 1000, Action::Simulate), 1000);
        assert_eq!(cell.count(&water), 1000);
        assert_eq!(cell.extract(&water, 1000, Action::Modulate), 1000);
        assert_eq!(cell.total_items(), 0);
        assert_eq!(bucket_fluid("minecraft:milk_bucket"), None);
        assert_eq!(fluid_bucket("test:unknown"), None);
    }
}
