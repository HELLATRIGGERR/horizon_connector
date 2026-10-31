//! EntropyManipulatorItem / ChargedStaffItem, AE2 19.2.18, LGPL-3.0-or-later.
use super::{Game, Hit, KeyCode, view_vector};
use horizon_client::{
    ae2_powered_items as power,
    blocks::Fluid,
    crafting::Template,
    first_person::UseResult,
    items::ItemStack,
    mobs::{Damage, DamageKind},
    particles::{ParticleOptions, ParticleType},
};
use horizon_core::{position::BlockPos, random::RandomSource};
use horizon_world::{
    chunk::BlockState,
    registry::{BlockRegistry, BlockStateId},
};
use serde_json::Value;
use std::collections::BTreeMap;

fn property_matches(value: &str, rule: &Value) -> bool {
    match rule {
        Value::String(expected) => value == expected,
        // Preserve upstream MultiValue.matches' first-entry behavior.
        Value::Array(values) => values.first().is_none_or(|v| v.as_str() == Some(value)),
        Value::Object(range) => {
            let (Some(min), Some(max)) = (
                range.get("min").and_then(Value::as_str),
                range.get("max").and_then(Value::as_str),
            ) else {
                return false;
            };
            match (value.parse::<i64>(), min.parse::<i64>(), max.parse::<i64>()) {
                (Ok(value), Ok(min), Ok(max)) => value >= min && value <= max,
                _ => value >= min && value <= max,
            }
        }
        _ => false,
    }
}
fn input_matches(state: &BlockState, rule: &Value) -> bool {
    if rule.is_null() {
        return true;
    }
    if rule["id"].as_str() != Some(state.name.as_str()) {
        return false;
    }
    rule.get("properties")
        .and_then(Value::as_object)
        .is_none_or(|props| {
            props.iter().all(|(name, rule)| {
                state
                    .properties
                    .get(name)
                    .is_some_and(|v| property_matches(v, rule))
            })
        })
}
fn matches_recipe(recipe: &Value, cool: bool, block: &BlockState, fluid: &BlockState) -> bool {
    recipe["mode"].as_str() == Some(if cool { "cool" } else { "heat" })
        && input_matches(block, &recipe["input"]["block"])
        && input_matches(fluid, &recipe["input"]["fluid"])
}
fn with_output_properties(
    registry: &BlockRegistry,
    rule: &Value,
    original: &BlockState,
) -> Result<BlockStateId, String> {
    let id = rule["id"].as_str().ok_or("entropy output block id")?;
    let mut state = registry
        .default_state(id)
        .and_then(|s| registry.state(s))
        .cloned()
        .ok_or("unregistered entropy output block")?;
    if rule.get("").and_then(Value::as_bool) == Some(true) {
        for (name, value) in &original.properties {
            if state.properties.contains_key(name) {
                state.properties.insert(name.clone(), value.clone());
            }
        }
    }
    if let Some(props) = rule.get("properties").and_then(Value::as_object) {
        for (name, value) in props {
            let value = value
                .as_str()
                .ok_or("entropy output property is not a string")?;
            if !state.properties.contains_key(name) {
                return Err("entropy output has no such property".into());
            }
            state.properties.insert(name.clone(), value.into());
        }
    }
    registry.resolve(&state).map_err(|e| e.to_string())
}
fn fluid_legacy_output(
    registry: &BlockRegistry,
    rule: &Value,
    original: &BlockState,
) -> Result<BlockStateId, String> {
    let id = rule["id"].as_str().ok_or("entropy fluid output id")?;
    if id == "minecraft:empty" {
        return registry
            .default_state("minecraft:air")
            .ok_or("air missing".into());
    }
    let (block, flowing) = match id {
        "minecraft:water" => ("minecraft:water", false),
        "minecraft:flowing_water" => ("minecraft:water", true),
        "minecraft:lava" => ("minecraft:lava", false),
        "minecraft:flowing_lava" => ("minecraft:lava", true),
        _ => return Err("unsupported entropy fluid output".into()),
    };
    // Verified against the pinned native Java registry: the default flowing
    // state is falling=true, amount=1, whose legacy block level is 15.
    let mut props = BTreeMap::from([("falling".to_owned(), "true".to_owned())]);
    if flowing {
        props.insert("level".into(), "1".into());
    }
    if rule.get("").and_then(Value::as_bool) == Some(true) {
        for (k, v) in &original.properties {
            if props.contains_key(k) {
                props.insert(k.clone(), v.clone());
            }
        }
    }
    if let Some(over) = rule.get("properties").and_then(Value::as_object) {
        for (k, v) in over {
            if !props.contains_key(k) {
                return Err("entropy fluid has no such property".into());
            }
            props.insert(k.clone(), v.as_str().ok_or("fluid property string")?.into());
        }
    }
    let falling = props["falling"] == "true";
    let amount = props
        .get("level")
        .map_or(Ok(8), |v| v.parse::<u8>())
        .map_err(|_| "fluid level integer")?;
    if !(1..=8).contains(&amount) {
        return Err("fluid amount out of range".into());
    }
    let level = if !flowing {
        0
    } else {
        8 - amount + if falling { 8 } else { 0 }
    };
    registry
        .resolve(&BlockState {
            name: block.into(),
            properties: BTreeMap::from([("level".into(), level.to_string())]),
        })
        .map_err(|e| e.to_string())
}

impl Game<'_> {
    pub(super) fn ae2_item_display_name(&self, stack: &ItemStack, base: &str) -> String {
        if stack.item.as_ref()=="ae2:facade" {return horizon_client::stations::hover_name(&|id|self.pack.names.display_item(id),stack);}
        if stack.item.as_ref() != "ae2:color_applicator" || stack.custom_name().is_some() {
            return base.into();
        }
        let color = power::applicator_color(self.res.items(), stack)
            .ok()
            .flatten();
        let label = color
            .and_then(power::color_translation)
            .and_then(|key| self.pack.names.display_get(key))
            .unwrap_or_else(|| self.translate("gui.ae2.Empty", "Empty"));
        format!("{base} - {label}")
    }
    pub(super) fn ae2_item_energy(&self, stack: &ItemStack) -> Option<(f64, f64)> {
        let energy = if power::profile(stack).is_some() {
            power::energy(stack)
        } else if super::ae2_wireless::is_wireless(stack) {
            super::ae2_wireless::energy(stack)
        } else if stack.item.starts_with("ae2:portable_item_cell_")
            || stack.item.starts_with("ae2:portable_fluid_cell_")
        {
            super::ae2_interactions::portable_energy(stack)
        } else {
            return None;
        }
        .ok()?;
        Some(energy)
    }
    /// AEBasePoweredItem always shows its stored-energy bar, including empty.
    pub(super) fn ae2_energy_bar(&self, stack: &ItemStack) -> Option<(i32, u32)> {
        let energy = self.ae2_item_energy(stack)?;
        let width = if energy.1 > 0.0 {
            (energy.0 / energy.1 * 13.0).round().clamp(0.0, 13.0) as i32
        } else {
            0
        };
        Some((width, 0x00ff00))
    }
    /// The powered melee effect available on the attacking player's real stack.
    pub(super) fn ae2_pvp_tool(&self) -> Option<bool> {
        let stack = self.inventory[self.selected].as_ref()?;
        let (cost, staff) = match stack.item.as_ref() {
            "ae2:charged_staff" => (300.0, true),
            "ae2:entropy_manipulator" => (1600.0, false),
            _ => return None,
        };
        power::can_use(stack, cost).then_some(staff)
    }
    pub(super) fn ae2_pvp_spend(&mut self, staff: bool) {
        let expected = if staff {
            "ae2:charged_staff"
        } else {
            "ae2:entropy_manipulator"
        };
        let Some(stack) = self.inventory[self.selected]
            .as_mut()
            .filter(|s| s.item.as_ref() == expected)
        else {
            return;
        };
        let _ = power::transfer(stack, if staff { 300.0 } else { 1600.0 }, false, false);
    }
    pub(super) fn ae2_pvp_effect(&mut self, staff: bool) {
        if staff {
            if let Some(bounds) = self.player.as_ref().map(|p| p.bounds()) {
                self.ae2_staff_fx(
                    bounds.min_position().components(),
                    bounds.max_position().components(),
                );
            }
            self.hurt_player_by("minecraft:magic", 6.0);
        } else if let Some(player) = self.player.as_mut() {
            let _ = player.ignite_for_ticks(160);
        }
    }
    fn ae2_entropy_fluid_state(&self, state: BlockStateId) -> BlockState {
        let facts = self.res.facts().get(state);
        let base = match facts.fluid {
            Fluid::Water => "water",
            Fluid::Lava => "lava",
            _ => {
                return BlockState {
                    name: "minecraft:empty".into(),
                    properties: Default::default(),
                };
            }
        };
        let flowing = !facts.waterlogged && (facts.fluid_falling || facts.fluid_level < 8);
        let mut properties = BTreeMap::from([("falling".into(), facts.fluid_falling.to_string())]);
        if flowing {
            properties.insert("level".into(), facts.fluid_level.to_string());
        }
        BlockState {
            name: format!("minecraft:{}{base}", if flowing { "flowing_" } else { "" }),
            properties,
        }
    }
    /// Fluid.ANY hit selection for this source tool; normal block outlines
    /// still bound the ray, including when a block is waterlogged.
    fn ae2_entropy_hit(&self) -> Option<Hit> {
        let block = self.pick();
        let origin = self.eye_position()?;
        let dir = view_vector(self.yaw, self.pitch);
        let distance = |p: [f64; 3]| (0..3).map(|i| (p[i] - origin[i]).powi(2)).sum::<f64>();
        let limit = block
            .as_ref()
            .map_or(self.reach(), |h| distance(h.point).sqrt().min(self.reach()));
        let mut last = origin.map(|v| v.floor() as i32);
        let mut t = 0.0;
        while t <= limit {
            let point = [
                origin[0] + dir[0] * t,
                origin[1] + dir[1] * t,
                origin[2] + dir[2] * t,
            ];
            let at = point.map(|v| v.floor() as i32);
            let facts = self.res.facts().get(self.world.block(at[0], at[1], at[2]));
            if facts.fluid != Fluid::None && !facts.waterlogged {
                let above = self
                    .res
                    .facts()
                    .get(self.world.block(at[0], at[1] + 1, at[2]));
                let height = if above.fluid == facts.fluid {
                    1.0
                } else {
                    f64::from(facts.fluid_level) / 9.0
                };
                if point[1] - f64::from(at[1]) <= height {
                    let face = [last[0] - at[0], last[1] - at[1], last[2] - at[2]];
                    return Some(Hit {
                        pos: at,
                        face: if face == [0, 0, 0] { [0, 1, 0] } else { face },
                        point,
                    });
                }
            }
            last = at;
            t += 0.025;
        }
        block
    }
    fn ae2_tool_drop(&mut self, pos: [i32; 3], stack: ItemStack) {
        let at = pos.map(|n| f64::from(n) + 0.5);
        let velocity = [
            self.world.random.next_gaussian() * 0.05,
            self.world.random.next_gaussian() * 0.05 + 0.2,
            self.world.random.next_gaussian() * 0.05,
        ];
        self.spawn_item(stack, at, velocity, 10);
    }
    fn ae2_entropy_sound(&mut self, pos: [i32; 3], recipe: bool) {
        let at = pos.map(|n| f64::from(n) + 0.5);
        if recipe {
            self.audio.play(
                "minecraft:block.fire.extinguish",
                Some(at),
                0.5,
                2.6 + (self.world.random.next_float() - self.world.random.next_float()) * 0.8,
            );
            let options = ParticleOptions::simple(ParticleType::LargeSmoke);
            self.particles
                .send_particles(&options, at, [0.5; 3], 0.0, 8);
        } else {
            self.audio.play(
                "minecraft:item.flintandsteel.use",
                Some(at),
                1.0,
                self.world.random.next_float() * 0.4 + 0.8,
            );
        }
    }
    pub(super) fn ae2_use_entropy(&mut self, stack: &ItemStack) -> Result<UseResult, String> {
        if !power::can_use(stack, 1600.0) { return Ok(UseResult::Pass); }
        let Some(hit) = self.ae2_entropy_hit() else { return Ok(UseResult::Fail); };
        if !self.may_build() { return Ok(UseResult::Fail); }
        let mut updated = stack.clone();
        let cool = self.keys.contains(&KeyCode::ShiftLeft);
        let result = self.ae2_entropy_at(&mut updated, hit, cool, false);
        *self.held_mut() = Some(updated);
        result
    }
    /// IBlockTool null-player use tries cooling first, then heating. Its
    /// physical dispenser slot is the owner; no player inventory is borrowed.
    pub(super) fn ae2_entropy_at(&mut self, stack: &mut ItemStack, hit: Hit, cool: bool, try_both: bool) -> Result<UseResult, String> {
        if !power::can_use(stack, 1600.0) { return Ok(UseResult::Pass); }
        let state = self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
        let Some(block) = self.pack.assets.registry.state(state).cloned() else {
            return Ok(UseResult::Fail);
        };
        let fluid = self.ae2_entropy_fluid_state(state);
        if !try_both && matches!(self.guest_use, Some(super::GuestUse::Item(_)))
            && fluid.name == "minecraft:empty"
        {
            // Source Item.use only invokes useOn for a fluid ray hit.
            return Ok(UseResult::Success { swing: true });
        }
        let recipes = self.res.recipes().ae2_custom.get("ae2:entropy");
        let find = |mode| recipes.and_then(|recipes| recipes.iter()
            .find(|(_, recipe)| matches_recipe(recipe, mode, &block, &fluid)))
            .map(|(_, recipe)| recipe.clone());
        let (recipe, cool) = if try_both {
            if let Some(recipe) = find(true) { (Some(recipe), true) }
            else { (find(false), false) }
        } else { (find(cool), cool) };
        if let Some(recipe) = recipe {
            let output = &recipe["output"];
            let next = if !output["block"].is_null() {
                with_output_properties(self.pack.assets.registry, &output["block"], &block)?
            } else if !output["fluid"].is_null() {
                fluid_legacy_output(self.pack.assets.registry, &output["fluid"], &fluid)?
            } else {
                self.world.air
            };
            let mut drops = Vec::new();
            if let Some(items) = output["drops"].as_array() {
                for item in items {
                    let drop = Template::parse(item)
                        .ok_or("invalid entropy drop")?
                        .create();
                    if drop.count == 0 || self.res.items().get(&drop.item).is_none() {
                        return Ok(UseResult::Fail);
                    }
                    drops.push(drop);
                }
            }
            let mut updated = stack.clone();
            power::transfer(&mut updated, 1600.0, false, false)?;
            self.level_set(hit.pos, next).map_err(|e| e.to_string())?;
            *stack = updated;
            for drop in drops {
                self.ae2_tool_drop(hit.pos, drop);
            }
            if !cool {
                self.ae2_entropy_sound(hit.pos, true);
            }
            return Ok(UseResult::Success { swing: true });
        }
        if !cool && matches!(block.name.as_str(), "minecraft:tnt" | "ae2:tiny_tnt") {
            let pos = BlockPos::new(hit.pos[0], hit.pos[1], hit.pos[2]);
            if block.name == "ae2:tiny_tnt" {
                self.ae2_ignite_tiny_tnt(pos, false);
            } else {
                self.spawn_primed_tnt(pos, false);
            }
            self.level_set(hit.pos, self.world.air)
                .map_err(|e| e.to_string())?;
            return Ok(UseResult::Success { swing: true }); // source TNT ignition costs no energy
        }
        if cool {
            return Ok(UseResult::Fail);
        }
        let drops = self.res.items().block_drops_in(
            self.pack.assets.registry,
            state,
            Some(stack),
            &mut self.world.random,
            Some((&mut self.rand_sequences, self.level.seed)),
        );
        let mut smelted = Vec::new();
        let mut next = None;
        let mut valid = !drops.is_empty();
        for drop in drops {
            let Some(recipe) = self.res.recipes().cook("minecraft:smelting", &drop) else {
                valid = false;
                break;
            };
            let result = recipe.result.clone();
            let block_id = self.res.items().get(&result.item).and_then(|i| i.block);
            if let Some(block_id) = block_id {
                if self.res.facts().get(block_id).name.as_ref() == block.name {
                    valid = false;
                    break;
                }
                if next.is_none() && !self.res.facts().is_air(block_id) {
                    next = Some(block_id);
                    continue;
                }
            }
            smelted.push(result);
        }
        if valid && (next.is_some() || !smelted.is_empty()) {
            let mut updated = stack.clone();
            power::transfer(&mut updated, 1600.0, false, false)?;
            self.level_set(hit.pos, next.unwrap_or(self.world.air))
                .map_err(|e| e.to_string())?;
            *stack = updated;
            for drop in smelted {
                self.ae2_tool_drop(hit.pos, drop);
            }
            self.ae2_entropy_sound(hit.pos, false);
            return Ok(UseResult::Success { swing: true });
        }
        let target = [
            hit.pos[0] + hit.face[0],
            hit.pos[1] + hit.face[1],
            hit.pos[2] + hit.face[2],
        ];
        if self
            .res
            .facts()
            .is_air(self.world.block(target[0], target[1], target[2]))
        {
            let mut updated = stack.clone();
            power::transfer(&mut updated, 1600.0, false, false)?;
            self.place_used_fire(target).map_err(|e| e.to_string())?;
            *stack = updated;
            self.ae2_entropy_sound(target, false);
        }
        Ok(UseResult::Success { swing: true })
    }
    pub(super) fn ae2_tool_hurt_enemy(&mut self, index: usize) -> bool {
        let Some(mut stack) = self.inventory[self.selected].clone() else {
            return false;
        };
        let (cost, staff) = match stack.item.as_ref() {
            "ae2:entropy_manipulator" => (1600.0, false),
            "ae2:charged_staff" => (300.0, true),
            _ => return false,
        };
        if !power::can_use(&stack, cost) || power::transfer(&mut stack, cost, false, false).is_err()
        {
            return false;
        }
        self.inventory[self.selected] = Some(stack);
        if staff {
            let bounds = self.mobs[index].bounds();
            self.ae2_staff_fx(
                bounds.min_position().components(),
                bounds.max_position().components(),
            );
            let health = self.mobs[index].health;
            if self.mobs[index].damage(6.0, Damage::of(DamageKind::Magic))
                && self.mobs[index].health != health
            {
                self.mob_damage_event(self.mobs[index].id);
            }
            true
        } else {
            self.mobs[index].ignite_for_ticks(160);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn entropy_mode_and_source_property_matching() {
        let block = BlockState {
            name: "minecraft:water".into(),
            properties: Default::default(),
        };
        let source = BlockState {
            name: "minecraft:water".into(),
            properties: BTreeMap::from([("falling".into(), "false".into())]),
        };
        let recipe = serde_json::json!({"mode":"cool","input":{"fluid":{"id":"minecraft:water"}},"output":{"block":{"id":"minecraft:ice"}}});
        assert!(matches_recipe(&recipe, true, &block, &source));
        assert!(!matches_recipe(&recipe, false, &block, &source));
        let flowing = BlockState {
            name: "minecraft:flowing_water".into(),
            properties: Default::default(),
        };
        assert!(!matches_recipe(&recipe, true, &block, &flowing));
        assert!(property_matches(
            "5",
            &serde_json::json!({"min":"2","max":"8"})
        ));
        assert!(!property_matches(
            "9",
            &serde_json::json!({"min":"2","max":"8"})
        ));
        assert!(!property_matches("b", &serde_json::json!(["a", "b"])));
    }
}
