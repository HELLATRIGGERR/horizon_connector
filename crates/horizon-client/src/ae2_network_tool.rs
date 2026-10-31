//! NetworkToolItem's physical upgrade-card pocket (AE2 19.2.18, LGPL-3.0+).
//! Uses the native minecraft:container component, as the pinned source does.
use crate::items::ItemStack;
pub const TOOL: &str = "ae2:network_tool";
pub const SLOT_COUNT: usize = 9;

pub fn is_card(id: &str) -> bool {
    matches!(
        id,
        "ae2:redstone_card"
            | "ae2:capacity_card"
            | "ae2:void_card"
            | "ae2:fuzzy_card"
            | "ae2:speed_card"
            | "ae2:inverter_card"
            | "ae2:crafting_card"
            | "ae2:equal_distribution_card"
            | "ae2:energy_card"
    )
}
#[derive(Clone, Debug, PartialEq)]
pub struct Inventory {
    pub slots: [Option<ItemStack>; SLOT_COUNT],
}
impl Inventory {
    pub fn read(tool: &ItemStack) -> Result<Self, String> {
        if tool.item.as_ref() != TOOL || tool.count != 1 {
            return Err("network tool moved".into());
        }
        let mut slots = std::array::from_fn(|_| None);
        if let Some(data) = tool.data() {
            for (index, stack) in &data.container {
                let index = usize::from(*index);
                if index >= SLOT_COUNT || stack.count == 0 || slots[index].is_some() {
                    return Err("invalid network tool inventory".into());
                }
                // Source loading retains existing contents, including an
                // administrator-supplied non-card; the filter controls insert.
                slots[index] = Some(stack.clone());
            }
        }
        Ok(Self { slots })
    }
    pub fn store(&self, tool: &mut ItemStack) -> Result<(), String> {
        if tool.item.as_ref() != TOOL || tool.count != 1 {
            return Err("network tool moved".into());
        }
        tool.data_mut().container = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|s| (i as u8, s.clone())))
            .collect();
        Ok(())
    }
    /// AppEngInternalInventory.addItems: merge first, then occupy empty slots.
    pub fn insert(&mut self, item: &mut ItemStack, maximum: u32) -> u32 {
        if !is_card(&item.item) || item.count == 0 {
            return 0;
        }
        let maximum = maximum.max(1);
        let before = item.count;
        for existing in self.slots.iter_mut().flatten() {
            if existing.same_item_same_components(item) {
                let n = item.count.min(maximum.saturating_sub(existing.count));
                existing.count += n;
                item.count -= n;
                if item.count == 0 {
                    return before;
                }
            }
        }
        for slot in &mut self.slots {
            if slot.is_none() {
                let n = item.count.min(maximum);
                *slot = Some(ItemStack {
                    count: n,
                    ..item.clone()
                });
                item.count -= n;
                if item.count == 0 {
                    break;
                }
            }
        }
        before - item.count
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_cards_merge_and_components_survive_container_encoding() {
        let mut tool = ItemStack::new(TOOL, 1);
        let mut inv = Inventory::read(&tool).unwrap();
        let mut cards = ItemStack::new("ae2:speed_card", 100);
        cards.data_mut().custom_name = Some(crate::item_data::text_name("original marked cards"));
        assert_eq!(inv.insert(&mut cards, 64), 100);
        assert_eq!(cards.count, 0);
        assert_eq!(inv.slots[0].as_ref().unwrap().count, 64);
        assert_eq!(inv.slots[1].as_ref().unwrap().count, 36);
        inv.store(&mut tool).unwrap();
        let encoded = crate::save::item_to_nbt(&tool);
        let restored = crate::save::item_from_nbt(&encoded).unwrap();
        assert_eq!(Inventory::read(&restored).unwrap(), inv);
        let mut non_card = ItemStack::new("minecraft:diamond", 7);
        assert_eq!(inv.insert(&mut non_card, 64), 0);
        assert_eq!(non_card.count, 7);
        for slot in inv.slots.iter_mut().skip(2) {
            *slot = Some(ItemStack::new("ae2:capacity_card", 64));
        }
        let mut extra = ItemStack::new("ae2:speed_card", 40);
        extra.data_mut().custom_name = Some(crate::item_data::text_name("original marked cards"));
        assert_eq!(inv.insert(&mut extra, 64), 28);
        assert_eq!(extra.count, 12);
    }
}
