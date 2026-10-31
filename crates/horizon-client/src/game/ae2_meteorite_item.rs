//! Live Debug Meteorite Placer adapter for AE2 19.2.18 (LGPL-3.0-or-later).
use super::{Dim, Game, KeyCode};
use horizon_client::{
    ae2_common_config,
    ae2_meteorite::{self, Crater, Fallout, MeteorRandom, Meteorite},
    chunk::{HEIGHT, MIN_Y},
    items::ItemStack,
    text::Text,
    vanillagen,
};
use horizon_core::{
    geometry::{Aabb, Vec3},
    position::BlockPos,
    random::{InvalidBound, LegacyRandom, RandomSource},
};
use horizon_world::{
    fluid::FluidKind,
    nbt::{Compound, Tag},
    registry::BlockStateId,
    worldgen::{biome::BiomeId, generator::GeneratorData},
};
use std::{cell::RefCell, rc::Rc};

const ITEM: &str = "ae2:debug_meteorite_placer";
const MODES: [(Crater, &str); 7] = [
    (Crater::None, "NONE"),
    (Crater::Normal, "NORMAL"),
    (Crater::Lava, "LAVA"),
    (Crater::Obsidian, "OBSIDIAN"),
    (Crater::Water, "WATER"),
    (Crater::Snow, "SNOW"),
    (Crater::Ice, "ICE"),
];

fn byte_value(value: &Tag) -> i8 {
    // CompoundTag.getByte accepts numeric tags; an unrelated tag reads as 0.
    match value {
        Tag::Byte(v) => *v,
        Tag::Short(v) => *v as i8,
        Tag::Int(v) => *v as i8,
        Tag::Long(v) => *v as i8,
        Tag::Float(v) => (f32::from_bits(*v).floor() as i32) as i8,
        Tag::Double(v) => (f64::from_bits(*v).floor() as i32) as i8,
        _ => 0,
    }
}
fn mode(stack: &ItemStack) -> Result<(Crater, &'static str), String> {
    let value = match stack
        .data()
        .and_then(|data| data.other("minecraft:custom_data"))
    {
        None => 0,
        Some(Tag::Compound(data)) => data.get(&"mode".into()).map_or(0, byte_value),
        _ => return Err("invalid meteorite placer custom data".into()),
    };
    MODES
        .get(usize::try_from(value).unwrap_or(usize::MAX))
        .copied()
        .ok_or_else(|| "invalid meteorite crater mode".into())
}
fn cycle(stack: &mut ItemStack) -> Result<&'static str, String> {
    let mut data = match stack
        .data()
        .and_then(|data| data.other("minecraft:custom_data"))
    {
        None => Compound::new(),
        Some(Tag::Compound(value)) => value.clone(),
        _ => return Err("invalid meteorite placer custom data".into()),
    };
    let next = match data.get(&"mode".into()) {
        None => 1,
        Some(value) => (i16::from(byte_value(value)) + 1) % 7,
    };
    let (_, name) = MODES
        .get(usize::try_from(next).unwrap_or(usize::MAX))
        .copied()
        .ok_or_else(|| "invalid meteorite crater mode".to_owned())?;
    data.insert("mode".into(), Tag::Byte(next as i8));
    stack
        .data_mut()
        .set_other("minecraft:custom_data", Some(Tag::Compound(data)));
    Ok(name)
}
fn geometry(clicked: [i32; 3], radius: f32) -> Option<([i32; 3], i32)> {
    let step = ((radius.ceil() as i32) + 1).min(5);
    let y = clicked[1].checked_sub(step)?;
    if y <= 10 + step {
        return None;
    }
    let range = ((radius * 2.0 + 5.0) * 5.0).ceil() as i32;
    Some(([clicked[0], y, clicked[2]], range))
}
fn biome_tags(data: &GeneratorData, name: &str) -> Result<(Fallout, &'static str), String> {
    let id = data
        .biome_names()
        .iter()
        .position(|n| n.as_ref() == name)
        .map(|i| BiomeId(i as u32))
        .ok_or_else(|| format!("meteorite biome {name} unavailable"))?;
    let has = |tag| data.biome_in_tag(id, tag);
    let fallout = if has("c:is_sandy") || has("minecraft:is_beach") {
        Fallout::Sand
    } else if has("minecraft:is_badlands") {
        Fallout::Terracotta
    } else if has("c:is_cold") {
        Fallout::IceSnow
    } else {
        Fallout::Default
    };
    let copied = if has("minecraft:is_badlands") {
        "minecraft:terracotta"
    } else if has("c:is_snowy") {
        "minecraft:snow_block"
    } else if has("minecraft:is_beach") || has("c:is_sandy") {
        "minecraft:sand"
    } else if has("c:is_plains") || has("minecraft:is_forest") {
        "minecraft:dirt"
    } else {
        "minecraft:cobblestone"
    };
    Ok((fallout, copied))
}
fn biome_at(game: &Game<'_>, p: [i32; 3]) -> Result<String, String> {
    let chunk = game
        .world
        .chunks
        .get(&(p[0] >> 4, p[2] >> 4))
        .ok_or("meteorite biome column unavailable")?;
    Ok(game
        .res
        .biomes()
        .get(chunk.biome(p[0], p[1], p[2]))
        .name
        .clone())
}

// Debug item passes level.random for both the placer RNG and the level RNG.
// Every setBlock copies that same state into and out of Game, so block update
// callbacks consuming the level RNG stay interleaved with the source draws.
struct SharedRandom(Rc<RefCell<LegacyRandom>>);
impl MeteorRandom for SharedRandom {
    fn next_float(&mut self) -> f32 {
        RandomSource::next_float(&mut *self.0.borrow_mut())
    }
    fn next_double(&mut self) -> f64 {
        RandomSource::next_double(&mut *self.0.borrow_mut())
    }
    fn next_int_bound(&mut self, bound: i32) -> Result<i32, InvalidBound> {
        RandomSource::next_int_bound(&mut *self.0.borrow_mut(), bound)
    }
}
struct LiveWorld<'a, 'g> {
    game: &'a mut Game<'g>,
    random: Rc<RefCell<LegacyRandom>>,
    min: [i32; 3],
    max: [i32; 3],
    copied: &'static str,
    error: Option<String>,
}
impl ae2_meteorite::World for LiveWorld<'_, '_> {
    type State = BlockStateId;
    fn bounds(&self) -> ([i32; 3], [i32; 3]) {
        (self.min, self.max)
    }
    fn sea_level(&self) -> i32 {
        if self.game.dim == Dim::Nether { 32 } else { 63 }
    }
    fn state(&self, name: &str) -> Option<Self::State> {
        let reg = self.game.pack.assets.registry;
        if name.ends_with("_quartz_bud") {
            reg.resolve(&horizon_world::chunk::BlockState {
                name: name.into(),
                properties: [
                    ("facing".into(), "up".into()),
                    ("waterlogged".into(), "false".into()),
                ]
                .into_iter()
                .collect(),
            })
            .ok()
        } else {
            reg.default_state(name)
        }
    }
    fn get(&self, p: [i32; 3]) -> Self::State {
        self.game.world.block(p[0], p[1], p[2])
    }
    fn put(&mut self, p: [i32; 3], state: Self::State) {
        if self.error.is_some() {
            return;
        }
        let old = self.get(p);
        let facts = self.game.res.facts();
        if old == state || facts.get(old).name.as_ref() == "minecraft:bedrock" {
            return;
        }
        self.game.world.random = self.random.borrow().clone();
        let result = self.game.level_set(p, state);
        *self.random.borrow_mut() = self.game.world.random.clone();
        if let Err(error) = result {
            self.error = Some(error.to_string());
            return;
        }
        if self.game.res.facts().get(state).name.as_ref() == "ae2:mysterious_cube"
            && self.game.world.block(p[0], p[1], p[2]) == state
        {
            self.game.ae2_ensure_machine((p[0], p[1], p[2]));
        }
    }
    fn air(&self, s: Self::State) -> bool {
        self.game.res.behaviours().is_air(s)
    }
    fn solid(&self, s: Self::State) -> bool {
        self.game.res.behaviours().is_solid(s)
    }
    fn replaceable(&self, s: Self::State) -> bool {
        self.game.res.behaviours().is_replaceable(s)
    }
    fn fluid(&self, s: Self::State) -> bool {
        self.game.res.behaviours().fluid_kind(s) != FluidKind::Empty
    }
    fn replaceable_tag(&self, s: Self::State) -> bool {
        let b = self.game.res.behaviours();
        b.tag("minecraft:replaceable")
            .is_some_and(|set| set.contains(b.block(s)))
    }
    fn same_block(&self, a: Self::State, b: Self::State) -> bool {
        self.game.res.facts().get(a).name == self.game.res.facts().get(b).name
    }
    fn copied_fallout(&self) -> &'static str {
        self.copied
    }
    fn schedule_surface_water(&mut self, p: [i32; 3]) {
        self.game.block_ticks.schedule_fluid(
            self.game.world.game_time,
            BlockPos::new(p[0], p[1], p[2]),
            FluidKind::Water,
            0,
        );
    }
    fn level_random_float(&mut self) -> f32 {
        RandomSource::next_float(&mut *self.random.borrow_mut())
    }
    fn after_crater(&mut self, m: Meteorite) {
        // Java discards ItemEntity instances intersecting the crater's AABB
        // before placing the core. This includes normal block onRemove drops.
        if self.error.is_some() {
            return;
        }
        let [x, y, z] = m.pos;
        let bounds = Aabb::new(
            Vec3::new(
                f64::from((x - 30).max(self.min[0])),
                f64::from(y - 5),
                f64::from((z - 30).max(self.min[2])),
            ),
            Vec3::new(
                f64::from((x + 30).min(self.max[0])),
                f64::from(y + 30),
                f64::from((z + 30).min(self.max[2])),
            ),
        );
        self.game
            .dropped
            .retain(|item| !item.bounds().intersects(bounds));
    }
}
impl Game<'_> {
    pub(super) fn ae2_try_meteorite_cycle(&mut self) -> bool {
        if self.remote.is_some()
            || !self.keys.contains(&KeyCode::ShiftLeft)
            || !self
                .held()
                .is_some_and(|s| s.item.as_ref() == ITEM && s.count > 0)
        {
            return false;
        }
        let result = cycle(self.held_mut().as_mut().expect("checked held item"));
        self.system_message(Text::literal(match result {
            Ok(name) => name.to_owned(),
            Err(error) => error,
        }));
        true
    }
    pub(super) fn ae2_try_meteorite_placer(&mut self, clicked: [i32; 3]) -> Option<bool> {
        if self.remote.is_some()
            || !self
                .held()
                .is_some_and(|s| s.item.as_ref() == ITEM && s.count > 0)
        {
            return None;
        }
        // Source draws before evaluating mode or unsuitable height.
        let radius = RandomSource::next_float(&mut self.world.random) * 6.0 + 2.0;
        let pure = RandomSource::next_float(&mut self.world.random) > 0.5;
        let result = mode(self.held().expect("checked held item")).and_then(|(crater, _)| {
            let (center, range) =
                geometry(clicked, radius).ok_or_else(|| "Un-suitable Location.".to_owned())?;
            self.ae2_meteorite_place(clicked, center, range, radius, pure, crater)?;
            Ok(format!("Spawned at y={} range={range}", center[1]))
        });
        let placed = result.is_ok();
        self.system_message(Text::literal(match result {
            Ok(message) => message,
            Err(message) => message,
        }));
        Some(placed)
    }
    fn ae2_meteorite_place(
        &mut self,
        clicked: [i32; 3],
        center: [i32; 3],
        range: i32,
        radius: f32,
        pure: bool,
        crater: Crater,
    ) -> Result<(), String> {
        let reg = self.pack.assets.registry;
        let data = vanillagen::shared_data(&self.options.resources.join("worldgen"), reg)
            .map_err(|e| e.to_string())?;
        let min = [
            clicked[0] - range,
            if self.dim == Dim::Overworld { MIN_Y } else { 0 },
            clicked[2] - range,
        ];
        // Source's BoundingBox limits X/Z, but placeCrater scans the world's
        // full height, not the item's Y +/- 10 bounding box.
        let max = [
            clicked[0] + range,
            if self.dim == Dim::Overworld {
                MIN_Y + HEIGHT
            } else {
                256
            },
            clicked[2] + range,
        ];
        self.ae2_with_demand_columns(|game| {
            // Source queries the crater only inside its parabolic cutoff. The
            // maximum possible reach occurs at world max Y; decay queries at
            // most 30 blocks. The extra column covers neighbor updates, while
            // NONE needs only the 3x3 packet refresh and the 8-block core.
            let (x0, x1, z0, z1) = if crater == Crater::None {
                (
                    (center[0] >> 4) - 1,
                    (center[0] >> 4) + 1,
                    (center[2] >> 4) - 1,
                    (center[2] >> 4) + 1,
                )
            } else {
                let vertical = f64::from(max[1] - center[1]) + f64::from(radius) - 1.0;
                let reach = ((vertical.max(0.0) / 0.02).sqrt().ceil() as i32 + 1)
                    .max(30)
                    .min(range);
                (((center[0]-reach) >> 4)-1, ((center[0]+reach) >> 4)+1,
                 ((center[2]-reach) >> 4)-1, ((center[2]+reach) >> 4)+1)
            };
            for cx in x0..=x1 {
                for cz in z0..=z1 {
                    game.ae2_demand_column(cx, cz)?;
                }
            }
            let (fallout, _) = biome_tags(&data, &biome_at(game, center)?)?;
            let (_, copied) = biome_tags(&data, &biome_at(game, clicked)?)?;
            let m = Meteorite {
                pos: center,
                radius: f64::from(radius),
                crater,
                fallout,
                pure_crater: pure,
                crater_lake: false,
            };
            let shared = Rc::new(RefCell::new(game.world.random.clone()));
            let mut rng = SharedRandom(shared.clone());
            let mut world = LiveWorld {
                game,
                random: shared.clone(),
                min,
                max,
                copied,
                error: None,
            };
            ae2_meteorite::place(
                &mut world,
                m,
                ae2_common_config::world_generation(),
                &mut rng,
            );
            world.game.world.random = shared.borrow().clone();
            match world.error {
                Some(error) => Err(format!("Meteorite placement stopped: {error}")),
                None => Ok(()),
            }
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn meteorite_item_mode_and_height_follow_source() {
        let mut item = ItemStack::new(ITEM, 1);
        item.data_mut().set_other(
            "minecraft:custom_data",
            Some(Tag::compound([("owner", Tag::String("kept".into()))])),
        );
        assert_eq!(mode(&item).unwrap().1, "NONE");
        for name in ["NORMAL", "LAVA", "OBSIDIAN", "WATER", "SNOW", "ICE", "NONE"] {
            assert_eq!(cycle(&mut item).unwrap(), name)
        }
        assert_eq!(
            item.data()
                .unwrap()
                .other("minecraft:custom_data")
                .unwrap()
                .get("owner"),
            Some(&Tag::String("kept".into()))
        );
        assert_eq!(geometry([4, 16, 9], 2.0), None);
        assert_eq!(geometry([4, 17, 9], 2.0), Some(([4, 14, 9], 45)));
        assert_eq!(geometry([4, 80, 9], 8.0), Some(([4, 75, 9], 105)));
    }
}
