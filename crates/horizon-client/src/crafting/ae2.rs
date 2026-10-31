//! AE2 19.2.18 dynamic crafting recipes, derived under LGPL-3.0-or-later.
//! Reference: appeng/recipes/game/StorageCellUpgradeRecipe.java.
use super::{ItemStack, Recipes, Value};

fn storage_upgrade<'a>(
    grid: &'a [Option<ItemStack>],
    recipe: &Value,
) -> Option<(usize, &'a ItemStack, ItemStack)> {
    let input_cell = recipe["input_cell"].as_str()?;
    let input_component = recipe["input_component"].as_str()?;
    let mut found = None;
    let mut components = 0;
    for (index, stack) in grid
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.as_ref().map(|s| (i, s)))
    {
        if stack.count == 0 {
            return None;
        }
        if stack.item.as_ref() == input_cell {
            if stack.count != 1 || found.is_some() {
                return None;
            }
            found = Some((index, stack));
        } else if stack.item.as_ref() == input_component {
            components += 1;
        } else {
            return None;
        }
    }
    let (index, cell) = found?;
    if components != 1 {
        return None;
    }
    let result = ItemStack {
        item: recipe["result_cell"].as_str()?.into(),
        count: 1,
        ..cell.clone()
    };
    Some((index, cell, result))
}
fn item_upgrade(
    grid: &[Option<ItemStack>],
    add: bool,
) -> Option<(ItemStack, Option<(usize, ItemStack)>)> {
    use crate::ae2_upgrade_inventory::{Inventory, capacity};
    let occupied = grid
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.as_ref().map(|s| (i, s)))
        .collect::<Vec<_>>();
    if if add {
        occupied.len() < 2
    } else {
        occupied.len() != 1
    } {
        return None;
    }
    let &(owner, stack) = occupied.iter().find(|(_, s)| capacity(&s.item).is_some())?;
    let mut upgraded = stack.clone();
    let mut inventory = Inventory::read(&upgraded).ok()?;
    if add {
        for &(index, card) in &occupied {
            if index != owner && !inventory.insert_one(card) {
                return None;
            }
        }
        inventory.store(&mut upgraded).ok()?;
        Some((upgraded, None))
    } else {
        let removed = inventory.remove_first()?;
        inventory.store(&mut upgraded).ok()?;
        Some((removed, Some((owner, upgraded))))
    }
}
pub(super) fn find<'a>(
    recipes: &'a Recipes,
    grid: &[Option<ItemStack>],
) -> Option<(&'a str, ItemStack)> {
    if let Some(upgrades) = recipes.ae2_custom.get("ae2:storage_cell_upgrade") {
        for (name, recipe) in upgrades {
            if let Some((_, _, result)) = storage_upgrade(grid, recipe) {
                return Some((name.as_str(), result));
            }
        }
    }
    for (kind, add) in [
        ("ae2:add_item_upgrade", true),
        ("ae2:remove_item_upgrade", false),
    ] {
        if let Some((name, _)) = recipes.ae2_custom.get(kind).and_then(|v| v.first()) {
            if let Some((result, _)) = item_upgrade(grid, add) {
                return Some((name.as_str(), result));
            }
        }
    }
    None
}
pub(super) fn remainders(
    recipes: &Recipes,
    name: &str,
    grid: &[Option<ItemStack>],
    size: usize,
) -> Option<Vec<Option<ItemStack>>> {
    let mut out = vec![None; size * size];
    for (i, stack) in grid
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.as_ref().map(|s| (i, s)))
    {
        *out.get_mut(i)? = recipes
            .facts
            .get(&stack.item)
            .and_then(|f| f.remainder.as_ref())
            .map(|id| ItemStack::new(id.as_ref(), 1));
    }
    if let Some((_, recipe)) = recipes
        .ae2_custom
        .get("ae2:storage_cell_upgrade")
        .and_then(|v| v.iter().find(|(id, _)| id == name))
    {
        let (slot, _, _) = storage_upgrade(grid, recipe)?;
        *out.get_mut(slot)? = Some(ItemStack::new(recipe["result_component"].as_str()?, 1));
        return Some(out);
    }
    for (kind, add) in [
        ("ae2:add_item_upgrade", true),
        ("ae2:remove_item_upgrade", false),
    ] {
        if recipes
            .ae2_custom
            .get(kind)
            .is_some_and(|v| v.iter().any(|(id, _)| id == name))
        {
            let (_, remaining) = item_upgrade(grid, add)?;
            if let Some((slot, item)) = remaining {
                *out.get_mut(slot)? = Some(item);
            }
            return Some(out);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use ae2_cell_core::{Action, Cell};
    #[test]
    fn storage_upgrade_keeps_filled_cell_components_and_returns_the_original_component_once() {
        let recipe = serde_json::json!({"input_cell":"ae2:item_storage_cell_1k","input_component":"ae2:cell_component_4k","result_cell":"ae2:item_storage_cell_4k","result_component":"ae2:cell_component_1k"});
        let recipes = Recipes {
            crafting: vec![],
            cooking: vec![],
            custom: Default::default(),
            ae2_custom: std::collections::HashMap::from([(
                "ae2:storage_cell_upgrade".into(),
                vec![("ae2:upgrade/test".into(), recipe)],
            )]),
            fluid_tags:Default::default(),
            facade_items: Default::default(),
            meta: Default::default(),
            facts: Default::default(),
            curses: Default::default(),
            cook_index: Default::default(),
        };
        let diamond = ItemStack::new("minecraft:diamond", 1);
        let key = crate::ae2_cell_codec::key(&diamond).unwrap();
        let mut cell = Cell::new_1k();
        assert_eq!(cell.insert(&key, 7, Action::Modulate), 7);
        let mut holder = ItemStack::new("ae2:item_storage_cell_1k", 1);
        holder.data_mut().custom_name = Some(crate::item_data::text_name("My physical cell"));
        crate::ae2_cell_codec::write_cell(&mut holder, &cell).unwrap();
        let mut grid = vec![None; 9];
        grid[7] = Some(holder.clone());
        grid[2] = Some(ItemStack::new("ae2:cell_component_4k", 64));
        let (name, out) = recipes.find(&grid, 3).unwrap();
        assert_eq!(out.item.as_ref(), "ae2:item_storage_cell_4k");
        assert_eq!(out.extra, holder.extra);
        assert_eq!(
            crate::ae2_cell_codec::read_cell(&out)
                .unwrap()
                .entries()
                .next()
                .unwrap()
                .1,
            &7
        );
        let rest = recipes.remainders(name, &grid, 3);
        assert_eq!(rest[7], Some(ItemStack::new("ae2:cell_component_1k", 1)));
        assert_eq!(rest.iter().flatten().count(), 1);
        assert_eq!(grid[2].as_ref().unwrap().count, 64);
        grid[7].as_mut().unwrap().count = 2;
        assert!(recipes.find(&grid, 3).is_none());
        grid[7].as_mut().unwrap().count = 1;
        grid[3] = grid[2].clone();
        assert!(recipes.find(&grid, 3).is_none());
    }
    #[test]
    fn source_card_crafting_preserves_actual_cell_and_returns_the_marked_card() {
        let recipes = Recipes {
            crafting: vec![],
            cooking: vec![],
            custom: Default::default(),
            ae2_custom: std::collections::HashMap::from([
                (
                    "ae2:add_item_upgrade".into(),
                    vec![("ae2:add_item_upgrade".into(), serde_json::json!({}))],
                ),
                (
                    "ae2:remove_item_upgrade".into(),
                    vec![("ae2:remove_item_upgrade".into(), serde_json::json!({}))],
                ),
            ]),
            fluid_tags:Default::default(),
            facade_items: Default::default(),
            meta: Default::default(),
            facts: Default::default(),
            curses: Default::default(),
            cook_index: Default::default(),
        };
        let key = crate::ae2_cell_codec::key(&ItemStack::new("minecraft:diamond", 1)).unwrap();
        let mut cell = Cell::new_1k();
        cell.insert(&key, 7, Action::Modulate);
        let mut holder = ItemStack::new("ae2:item_storage_cell_1k", 1);
        holder.data_mut().custom_name = Some(crate::item_data::text_name("Physical"));
        crate::ae2_cell_codec::write_cell(&mut holder, &cell).unwrap();
        let mut card = ItemStack::new("ae2:fuzzy_card", 3);
        card.data_mut().custom_name = Some(crate::item_data::text_name("Marked"));
        let mut grid = vec![None; 9];
        grid[7] = Some(holder);
        grid[2] = Some(card.clone());
        let (id, upgraded) = recipes.find(&grid, 3).unwrap();
        assert_eq!(id, "ae2:add_item_upgrade");
        assert_eq!(
            crate::ae2_cell_codec::read_cell(&upgraded)
                .unwrap()
                .entries()
                .next()
                .unwrap()
                .1,
            &7
        );
        assert_eq!(grid[2].as_ref().unwrap().count, 3);
        grid[7] = Some(upgraded);
        grid[2] = None;
        let (id, removed) = recipes.find(&grid, 3).unwrap();
        assert_eq!(id, "ae2:remove_item_upgrade");
        assert_eq!(removed, ItemStack { count: 1, ..card });
        let remaining = recipes.remainders(id, &grid, 3);
        let holder = remaining[7].as_ref().unwrap();
        assert_eq!(
            holder.data().unwrap().custom_name_plain().as_deref(),
            Some("Physical")
        );
        assert_eq!(
            crate::ae2_cell_codec::read_cell(holder)
                .unwrap()
                .entries()
                .next()
                .unwrap()
                .1,
            &7
        );
        assert!(
            crate::ae2_upgrade_inventory::Inventory::read(holder)
                .unwrap()
                .slots
                .iter()
                .all(Option::is_none)
        );
        assert_eq!(remaining.iter().flatten().count(), 1);
        grid[7] = Some(holder.clone());
        grid[2] = Some(ItemStack::new("ae2:energy_card", 1));
        assert!(recipes.find(&grid, 3).is_none());
    }
}
