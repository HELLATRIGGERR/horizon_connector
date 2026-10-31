//! Source NetworkToolItem's two right-click inventory overrides.
use super::{Env, Menu, tidy};
use crate::ae2_network_tool::{Inventory, TOOL};
impl Menu {
    pub(super) fn network_tool_click(&mut self, env: &Env, s: usize, primary: bool) -> bool {
        if primary || !self.allow_modification(env, s) {
            return false;
        }
        if let Some(mut tool) = self.carried.clone().filter(|s| s.item.as_ref() == TOOL) {
            let Ok(mut inventory) = Inventory::read(&tool) else {
                return true;
            };
            if let Some(other) = self.slot(s).cloned() {
                let maximum = Self::max_stack(env, &other);
                let mut probe = other.clone();
                let wanted = inventory.clone().insert(&mut probe, maximum);
                if wanted > 0
                    && let Some(mut taken) = self.safe_take(env, s, wanted, wanted)
                {
                    inventory.insert(&mut taken, maximum);
                    if taken.count > 0 {
                        self.drop_stack(&taken);
                    }
                }
            }
            let _ = inventory.store(&mut tool);
            self.carried = Some(tool);
            self.inventory_slots_changed(env);
            return true;
        }
        let Some(mut tool) = self.slot(s).cloned().filter(|s| s.item.as_ref() == TOOL) else {
            return false;
        };
        if self.carried.is_none() {
            return false;
        }
        let Ok(mut inventory) = Inventory::read(&tool) else {
            return true;
        };
        if let Some(mut item) = self.carried.take() {
            let maximum = Self::max_stack(env, &item);
            inventory.insert(&mut item, maximum);
            self.carried = Some(item);
            tidy(&mut self.carried);
        }
        let _ = inventory.store(&mut tool);
        let slot = self.slots[s];
        self.containers[slot.container][slot.index] = Some(tool);
        self.inventory_slots_changed(env);
        true
    }
}
