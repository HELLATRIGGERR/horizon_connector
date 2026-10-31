//! AE2 in-world environmental behavior, pinned AE2 19.2.18 (LGPL-3.0+).
use super::Game;
use horizon_client::blocks::Fluid;
use horizon_core::random::RandomSource;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
type Key = (PathBuf, &'static str, u32);
static FLUID_TICKS: OnceLock<Mutex<HashMap<Key, (u8, u32)>>> = OnceLock::new();
impl Game<'_> {
    /// Source ItemEntityMixin's non-persisted 61-tick fluid transformation.
    pub(super) fn tick_ae2_environment(&mut self) {
        if self.remote.is_some() {
            return;
        }
        let rules = horizon_client::ae2_transform::rules(self.res.recipes(), self.res.items());
        let scope = (self.world_root.clone(), self.dim.id());
        let Ok(mut counters) = FLUID_TICKS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
        else {
            return;
        };
        let live: HashSet<_> = self.dropped.iter().map(|e| e.id).collect();
        counters
            .retain(|(root, dim, id), _| root != &scope.0 || *dim != scope.1 || live.contains(id));
        let mut produced = Vec::new();
        for i in 0..self.dropped.len() {
            let item = &self.dropped[i];
            if item.stack.count == 0 || !rules.iter().any(|r| r.any_fluid_anchor(&item.stack)) {
                continue;
            }
            let key = (scope.0.clone(), scope.1, item.id);
            let p = item.position;
            let facts = self.res.facts().get(self.world.block(
                p.x.floor() as i32,
                (p.y + 0.125).floor() as i32,
                p.z.floor() as i32,
            ));
            let flowing = !facts.waterlogged && (facts.fluid_falling || facts.fluid_level < 8);
            let fluid = match (facts.fluid, flowing) {
                (Fluid::Water, false) => Some("minecraft:water"),
                (Fluid::Water, true) => Some("minecraft:flowing_water"),
                (Fluid::Lava, false) => Some("minecraft:lava"),
                (Fluid::Lava, true) => Some("minecraft:flowing_lava"),
                _ => None,
            };
            if !fluid.is_some_and(|id| {
                rules
                    .iter()
                    .any(|r| r.any_fluid_anchor(&item.stack) && r.fluid(id))
            }) {
                counters.remove(&key);
                continue;
            }
            let timer = counters.entry(key).or_insert((0, item.ticks));
            if item.ticks < timer.1 {
                timer.0 = 0;
            }
            timer.1 = item.ticks;
            timer.0 = timer.0.saturating_add(1);
            if timer.0 <= 60 {
                continue;
            }
            let selected = rules
                .iter()
                .filter(|r| r.fluid(fluid.unwrap()))
                .find_map(|rule| {
                    self.res.items().get(&rule.output.item)?;
                    let claims = rule.select(i, &self.dropped)?;
                    let output = rule.assemble().ok()?;
                    Some((output, claims))
                });
            let Some((output, claims)) = selected else {
                timer.0 = 0;
                continue;
            };
            let at = [
                p.x.floor() + 0.25 + self.world.random.next_double() * 0.5,
                p.y.floor() + 0.25 + self.world.random.next_double() * 0.5,
                p.z.floor() + 0.25 + self.world.random.next_double() * 0.5,
            ];
            let velocity = [
                self.world.random.next_double() * 0.25 - 0.125,
                self.world.random.next_double() * 0.25 - 0.125,
                self.world.random.next_double() * 0.25 - 0.125,
            ];
            for (index, count) in claims {
                self.dropped[index].stack.count -= count;
            }
            produced.push((output, at, velocity));
        }
        self.dropped.retain(|e| e.stack.count > 0);
        drop(counters);
        for (output, at, velocity) in produced {
            self.spawn_item(output, at, velocity, 0);
        }
    }
}
/// A source-exact geometric classification for the meteorite core. World
/// integration still needs structure placement, biome checks, RNG, cross-chunk
/// clipping, center chest, crater/fallout and loot; this is not worldgen yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MeteoriteCoreCell {
    Outside,
    Cavity,
    QuartzFloor,
    SkyStone,
}
pub(super) fn meteorite_core_cell(dx: i32, dy: i32, dz: i32, radius: f64) -> MeteoriteCoreCell {
    if !radius.is_finite()
        || radius <= 0.0
        || !(-8..=8).contains(&dx)
        || !(-8..8).contains(&dy)
        || !(-8..=8).contains(&dz)
    {
        return MeteoriteCoreCell::Outside;
    }
    let dist = (f64::from(dx) * f64::from(dx) * 0.7)
        + (f64::from(dy) * f64::from(dy) * if dy > 0 { 1.4 } else { 0.8 })
        + (f64::from(dz) * f64::from(dz) * 0.7);
    if dist >= radius * radius {
        return MeteoriteCoreCell::Outside;
    }
    if dx.abs() <= 1 && dy.abs() <= 1 && dz.abs() <= 1 {
        if dy == -1 {
            MeteoriteCoreCell::QuartzFloor
        } else {
            MeteoriteCoreCell::Cavity
        }
    } else {
        MeteoriteCoreCell::SkyStone
    }
}

/// Source: BuddingCertusQuartzBlock.randomTick (AE2 v19.2.18).
fn next_certus_stage(
    name: &str,
    facing: Option<&str>,
    direction: &str,
    source_water: bool,
) -> Option<&'static str> {
    match name {
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air" => {
            Some("ae2:small_quartz_bud")
        }
        "minecraft:water" if source_water => Some("ae2:small_quartz_bud"),
        "ae2:small_quartz_bud" if facing == Some(direction) => Some("ae2:medium_quartz_bud"),
        "ae2:medium_quartz_bud" if facing == Some(direction) => Some("ae2:large_quartz_bud"),
        "ae2:large_quartz_bud" if facing == Some(direction) => Some("ae2:quartz_cluster"),
        _ => None,
    }
}
fn decayed_certus(name: &str) -> Option<&'static str> {
    match name {
        "ae2:flawed_budding_quartz" => Some("ae2:chipped_budding_quartz"),
        "ae2:chipped_budding_quartz" => Some("ae2:damaged_budding_quartz"),
        "ae2:damaged_budding_quartz" => Some("ae2:quartz_block"),
        _ => None,
    }
}
fn is_budding(name: &str) -> bool {
    matches!(
        name,
        "ae2:flawless_budding_quartz"
            | "ae2:flawed_budding_quartz"
            | "ae2:chipped_budding_quartz"
            | "ae2:damaged_budding_quartz"
    )
}
impl Game<'_> {
    /// Called by authoritative random tick dispatch and by powered accelerators.
    pub(super) fn ae2_random_tick_certus(&mut self, pos: [i32; 3]) {
        use horizon_world::chunk::BlockState;
        if self.remote.is_some() || !self.world.loaded(pos[0], pos[2]) {
            return;
        }
        let old = self.world.block(pos[0], pos[1], pos[2]);
        let source = self.res.facts().get(old).name.to_string();
        if !is_budding(&source) || self.world.random.next_int_bound(5) != Ok(0) {
            return;
        }
        // Direction.values() order in 1.21.1: down, up, north, south, west, east.
        let directions = [
            ("down", [0, -1, 0]),
            ("up", [0, 1, 0]),
            ("north", [0, 0, -1]),
            ("south", [0, 0, 1]),
            ("west", [-1, 0, 0]),
            ("east", [1, 0, 0]),
        ];
        let Ok(n) = self.world.random.next_int_bound(6) else {
            return;
        };
        let (face, offset) = directions[n as usize];
        let target = [pos[0] + offset[0], pos[1] + offset[1], pos[2] + offset[2]];
        if !self.world.loaded(target[0], target[2]) {
            return;
        }
        let target_id = self.world.block(target[0], target[1], target[2]);
        let Some(state) = self.pack.assets.registry.state(target_id) else {
            return;
        };
        let water = state.name == "minecraft:water"
            && state.properties.get("level").is_none_or(|v| v == "0");
        let Some(new_id) = next_certus_stage(
            &state.name,
            state.properties.get("facing").map(String::as_str),
            face,
            water,
        ) else {
            return;
        };
        let waterlogged = water
            || state
                .properties
                .get("waterlogged")
                .is_some_and(|v| v == "true");
        let mut properties = std::collections::BTreeMap::new();
        properties.insert("facing".into(), face.into());
        properties.insert("waterlogged".into(), waterlogged.to_string());
        let Ok(new_state) = self.pack.assets.registry.resolve(&BlockState {
            name: new_id.into(),
            properties,
        }) else {
            return;
        };
        if self.level_set(target, new_state).is_err() {
            return;
        }
        if let Some(next) = decayed_certus(&source)
            && self.world.random.next_int_bound(12) == Ok(0)
        {
            if let Some(next_state) = self.pack.assets.registry.default_state(next) {
                let _ = self.level_set(pos, next_state);
            }
        }
    }
    /// AE2 default interval 10 ticks. Network idle power is accounted by
    /// the grid; the accelerator also drains up to 80 AE from its local buffer.
    pub(super) fn tick_ae2_growth_accelerators(&mut self) {
        use horizon_client::{ae2_world::MachineKind, containers::BlockEntity};
        if self.remote.is_some() {
            return;
        }
        let positions: Vec<_> = self
            .world
            .block_entities
            .iter()
            .filter_map(|(&p, e)| match e {
                BlockEntity::Ae2(m) if m.kind == MachineKind::GrowthAccelerator => Some(p),
                _ => None,
            })
            .collect();
        for p in positions {
            let interval=horizon_client::ae2_common_config::processing().growth_interval;
            let Some(elapsed)=self.ae2_fixed_processor_elapsed(p,interval) else {continue;};
            let online = self.ae2_online(p);
            let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&p) else {
                continue;
            };
            if !online && machine.energy < 8.0 {
                continue;
            }
            if machine.energy > 0.0 {
                machine.private_energy_extract(8.0 * elapsed as f64, true);
                self.world.modified.insert((p.0 >> 4, p.2 >> 4));
            }
            for d in [
                [0, -1, 0],
                [0, 1, 0],
                [0, 0, -1],
                [0, 0, 1],
                [-1, 0, 0],
                [1, 0, 0],
            ] {
                let target = [p.0 + d[0], p.1 + d[1], p.2 + d[2]];
                if !self.world.loaded(target[0], target[2]) {
                    continue;
                }
                let state = self.world.block(target[0], target[1], target[2]);
                let name = &self.res.facts().get(state).name;
                let eligible = self.res.items().block_tags.get("ae2:growth_acceleratable")
                    .is_some_and(|tag| tag.contains(name.as_ref()));
                if !eligible {
                    continue;
                }
                // GrowthAcceleratorBlockEntity.onTick calls the block's own
                // randomTick directly (including datapack-added tag members),
                // without an additional random fluid tick.
                let pos = horizon_core::position::BlockPos::new(target[0], target[1], target[2]);
                self.with_level(|b, level, ticks| b.random_tick_block(level, ticks, pos));
                if let Err(error) = self.process_level_events() {
                    log::warn!("AE2 growth accelerator random tick: {error}");
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn meteorite_core_geometry_preserves_source_cavity_and_asymmetry() {
        use MeteoriteCoreCell::*;
        assert_eq!(meteorite_core_cell(0, 0, 0, 5.0), Cavity);
        assert_eq!(meteorite_core_cell(0, -1, 0, 5.0), QuartzFloor);
        assert_eq!(meteorite_core_cell(2, 0, 0, 5.0), SkyStone);
        assert_eq!(meteorite_core_cell(0, 5, 0, 5.0), Outside);
        assert_eq!(meteorite_core_cell(0, -5, 0, 5.0), SkyStone);
        assert_eq!(meteorite_core_cell(9, 0, 0, 20.0), Outside);
        assert_eq!(meteorite_core_cell(0, 0, 0, f64::NAN), Outside);
    }

    #[test]
    fn budding_requires_correct_face_source_water_and_decays_in_order() {
        assert_eq!(
            next_certus_stage("minecraft:water", None, "north", false),
            None
        );
        assert_eq!(
            next_certus_stage("minecraft:water", None, "north", true),
            Some("ae2:small_quartz_bud")
        );
        assert_eq!(
            next_certus_stage("ae2:small_quartz_bud", Some("south"), "north", false),
            None
        );
        assert_eq!(
            next_certus_stage("ae2:small_quartz_bud", Some("north"), "north", false),
            Some("ae2:medium_quartz_bud")
        );
        assert_eq!(
            next_certus_stage("ae2:medium_quartz_bud", Some("north"), "north", false),
            Some("ae2:large_quartz_bud")
        );
        assert_eq!(
            next_certus_stage("ae2:large_quartz_bud", Some("north"), "north", false),
            Some("ae2:quartz_cluster")
        );
        assert_eq!(decayed_certus("ae2:flawless_budding_quartz"), None);
        assert_eq!(
            decayed_certus("ae2:flawed_budding_quartz"),
            Some("ae2:chipped_budding_quartz")
        );
        assert_eq!(
            decayed_certus("ae2:chipped_budding_quartz"),
            Some("ae2:damaged_budding_quartz")
        );
        assert_eq!(
            decayed_certus("ae2:damaged_budding_quartz"),
            Some("ae2:quartz_block")
        );
    }
}
