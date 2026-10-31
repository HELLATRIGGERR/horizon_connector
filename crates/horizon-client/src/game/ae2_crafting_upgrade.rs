//! Source in-world crafting-unit component exchange, AE2 19.2.18 (LGPL-3.0+).
//! AbstractCraftingUnitBlock and active CraftingUnitTransformRecipe records.
use super::{Game, KeyCode};
use horizon_client::{ae2_world::MachineKind, items::ItemStack, text::Text};
use serde_json::Value;
type Recipes = [(String, Value)];
fn crafting(kind: MachineKind) -> bool {
    matches!(
        kind,
        MachineKind::CraftingUnit
            | MachineKind::CraftingAccelerator
            | MachineKind::CraftingStorage1k
            | MachineKind::CraftingStorage4k
            | MachineKind::CraftingStorage16k
            | MachineKind::CraftingStorage64k
            | MachineKind::CraftingStorage256k
            | MachineKind::CraftingMonitor
    )
}
struct Change {
    next: MachineKind,
    returned: Option<ItemStack>,
    consume: bool,
}
fn change(
    old: MachineKind,
    held: Option<&ItemStack>,
    alternate: bool,
    hands_full: bool,
    recipes: &Recipes,
) -> Option<Change> {
    if !crafting(old) || alternate && hands_full {
        return None;
    }
    let old_part = || {
        recipes
            .iter()
            .find(|(_, r)| r["upgraded_block"].as_str() == Some(old.id()))
            .and_then(|(_, r)| r["upgrade_item"].as_str())
            .map(|id| ItemStack::new(id, 1))
    };
    if let Some(held) = held.filter(|s| s.count > 0) {
        let (_, recipe) = recipes
            .iter()
            .find(|(_, r)| r["upgrade_item"].as_str() == Some(held.item.as_ref()))?;
        let next = MachineKind::from_id(recipe["upgraded_block"].as_str()?)?;
        if !crafting(next) || next == old {
            return None;
        }
        let returned = if old == MachineKind::CraftingUnit {
            None
        } else {
            Some(old_part()?)
        };
        Some(Change {
            next,
            returned,
            consume: true,
        })
    } else if alternate && old != MachineKind::CraftingUnit {
        Some(Change {
            next: MachineKind::CraftingUnit,
            returned: Some(old_part()?),
            consume: false,
        })
    } else {
        None
    }
}
impl Game<'_> {
    pub(super) fn ae2_predict_crafting_upgrade(
        &self,
        pos: [i32; 3],
        held: Option<&ItemStack>,
    ) -> bool {
        let block = self
            .res
            .facts()
            .get(self.world.block(pos[0], pos[1], pos[2]));
        let Some(kind) = MachineKind::from_id(&block.name) else {
            return false;
        };
        let hands_full = self.inventory[self.selected].is_some() || self.offhand.is_some();
        change(
            kind,
            held,
            self.keys.contains(&KeyCode::ShiftLeft),
            hands_full,
            self.res
                .recipes()
                .custom
                .get("ae2:crafting_unit_transform")
                .map_or(&[][..], Vec::as_slice),
        )
        .is_some()
    }
    pub(super) fn ae2_try_crafting_upgrade(&mut self, pos: [i32; 3], face: [i32; 3]) -> bool {
        if self.remote.is_some() || self.spectator() {
            return false;
        }
        let old = self.world.block(pos[0], pos[1], pos[2]);
        let Some(kind) = MachineKind::from_id(&self.res.facts().get(old).name) else {
            return false;
        };
        let hands_full = self.inventory[self.selected].is_some() || self.offhand.is_some();
        let Some(change) = change(
            kind,
            self.held(),
            self.keys.contains(&KeyCode::ShiftLeft),
            hands_full,
            self.res
                .recipes()
                .custom
                .get("ae2:crafting_unit_transform")
                .map_or(&[][..], Vec::as_slice),
        ) else {
            return false;
        };
        let key = (pos[0], pos[1], pos[2]);
        self.ae2_ensure_machine(key);
        if kind != MachineKind::CraftingUnit && self.ae2_crafting_cluster_busy(key) {
            self.system_message(Text::translate("chat.ae2.CraftingCpuBusy", &[]));
            return true;
        }
        if change
            .returned
            .as_ref()
            .is_some_and(|s| self.res.items().get(&s.item).is_none())
        {
            return false;
        }
        let registry = self.pack.assets.registry;
        let Some(base) = registry.default_state(change.next.id()) else {
            return false;
        };
        let Some(mut target) = registry.state(base).cloned() else {
            return false;
        };
        if target.properties.contains_key("facing") {
            let facing = match face {
                [0, -1, 0] => "down",
                [0, 1, 0] => "up",
                [0, 0, -1] => "north",
                [0, 0, 1] => "south",
                [-1, 0, 0] => "west",
                _ => "east",
            };
            target.properties.insert("facing".into(), facing.into());
        }
        let Ok(next) = registry.resolve(&target) else {
            return false;
        };
        if let Err(error) = self.ae2_break_crafting_cluster(key) {
            self.command_failure(Text::literal(format!("AE2 crafting block: {error}")));
            return true;
        }
        let previous = self.world.block_entities.get(&key).cloned();
        let name = self.world.block_names.get(&key).cloned();
        let changed = self.level_set(pos, self.world.air).is_ok()
            && self.level_set(pos, next).is_ok()
            && self.world.block(pos[0], pos[1], pos[2]) == next;
        if !changed {
            let _ = self.level_set(pos, old);
            if let Some(previous) = previous {
                self.world.block_entities.insert(key, previous);
            }
            if let Some(name) = name {
                self.world.block_names.insert(key, name);
            }
            return false;
        }
        self.ae2_ensure_machine(key);
        self.audio.play(
            "minecraft:entity.item_frame.remove_item",
            Some(pos.map(|p| f64::from(p) + 0.5)),
            0.5,
            1.0,
        );
        // Source removeUpgrade returns the old component before consuming the
        // held one. With a full inventory that ordering can cause overflow.
        if let Some(returned) = change.returned {
            if let Some(rest) = self.give(returned) {
                self.player_drop(rest);
            }
        }
        if change.consume {
            self.consume_held();
        }
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_crafting_component_exchange_uses_active_recipe_and_alternate_rules() {
        let recipes = vec![
            (
                "one".into(),
                serde_json::json!({"upgrade_item":"ae2:cell_component_1k","upgraded_block":"ae2:1k_crafting_storage"}),
            ),
            (
                "four".into(),
                serde_json::json!({"upgrade_item":"ae2:cell_component_4k","upgraded_block":"ae2:4k_crafting_storage"}),
            ),
        ];
        let c = change(
            MachineKind::CraftingUnit,
            Some(&ItemStack::new("ae2:cell_component_1k", 2)),
            false,
            true,
            &recipes,
        )
        .unwrap();
        assert_eq!(c.next, MachineKind::CraftingStorage1k);
        assert!(c.returned.is_none() && c.consume);
        let c = change(
            MachineKind::CraftingStorage1k,
            Some(&ItemStack::new("ae2:cell_component_4k", 1)),
            false,
            true,
            &recipes,
        )
        .unwrap();
        assert_eq!(c.returned, Some(ItemStack::new("ae2:cell_component_1k", 1)));
        let c = change(MachineKind::CraftingStorage4k, None, true, false, &recipes).unwrap();
        assert_eq!(c.next, MachineKind::CraftingUnit);
        assert!(!c.consume);
        assert_eq!(c.returned, Some(ItemStack::new("ae2:cell_component_4k", 1)));
        assert!(
            change(
                MachineKind::CraftingUnit,
                Some(&ItemStack::new("ae2:cell_component_1k", 1)),
                true,
                true,
                &recipes
            )
            .is_none()
        );
        assert!(
            change(
                MachineKind::CraftingStorage1k,
                Some(&ItemStack::new("ae2:cell_component_1k", 1)),
                false,
                true,
                &recipes
            )
            .is_none()
        );
    }
}
