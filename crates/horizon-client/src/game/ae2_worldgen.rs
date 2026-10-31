//! Source AE2 meteorites registered in Horizon's native structure pipeline.
//! AE2 19.2.18 MeteoriteStructure/StructurePiece/Placer, LGPL-3.0-or-later.
use crate::ae2_meteorite::{self as source, Crater, Fallout, MeteorRandom, Meteorite};
use horizon_core::{
    position::{BlockPos, ChunkPos},
    random::{LegacyRandom, RandomSource, WorldgenRandom},
};
use horizon_world::{
    block_properties::{BlockProperty, BoundBlockProperty},
    fluid::{FluidKind, FluidStateTable},
    heightmap::HeightmapType,
    nbt::{Compound, Tag},
    registry::{BlockRegistry, BlockStateId},
    worldgen::{
        biome::BiomeId,
        biome_temperature::BiomeTemperatureSettings,
        generator::GeneratorData,
        placement::PlacementQueryError,
        structure::{
            BoundingBox,
            extension::{GeneratedPieces, StructureExtension},
            legacy::{LegacyPiece, PieceContext, PieceRandom, PieceWorld, piece_tag},
            start::StartContext,
        },
    },
};
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
const TYPE: &str = "ae2:ae2mtrt";
struct Bindings {
    registry: &'static BlockRegistry,
    solid: BoundBlockProperty,
    replaceable: BoundBlockProperty,
    fluids: FluidStateTable,
    tags: BTreeMap<&'static str, BTreeSet<BiomeId>>,
    replaceable_tag: BTreeSet<BlockStateId>,
    temperatures: Vec<BiomeTemperatureSettings>,
}
impl std::fmt::Debug for Bindings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AE2 meteorite bindings")
    }
}
impl Bindings {
    fn new(data: &GeneratorData) -> Option<Arc<Self>> {
        let registry = data.registry();
        registry.default_state("ae2:mysterious_cube")?;
        let mut tags = BTreeMap::new();
        for tag in [
            "c:is_ocean",
            "c:is_sandy",
            "c:is_cold",
            "c:is_snowy",
            "c:is_plains",
            "minecraft:is_beach",
            "minecraft:is_badlands",
            "minecraft:is_forest",
        ] {
            tags.insert(
                tag,
                data.biome_names()
                    .iter()
                    .enumerate()
                    .filter_map(|(i, _)| {
                        let id = BiomeId(i as u32);
                        data.biome_in_tag(id, tag).then_some(id)
                    })
                    .collect(),
            );
        }
        let replaceable_tag = (0..registry.len())
            .map(|i| BlockStateId(i as u32))
            .filter(|s| data.block_in_tag(*s, "minecraft:replaceable"))
            .collect();
        Some(Arc::new(Self {
            registry,
            solid: data.bind_block_property(BlockProperty::Solid)?,
            replaceable: data.bind_block_property(BlockProperty::Replaceable)?,
            fluids: FluidStateTable::new(registry).ok()?,
            tags,
            replaceable_tag,
            temperatures: data
                .biome_names()
                .iter()
                .enumerate()
                .map(|(i, _)| data.biome_temperature_settings(BiomeId(i as u32)))
                .collect(),
        }))
    }
    fn has(&self, id: BiomeId, tag: &str) -> bool {
        self.tags.get(tag).is_some_and(|s| s.contains(&id))
    }
    fn fallout(&self, id: BiomeId) -> Fallout {
        if self.has(id, "c:is_sandy") || self.has(id, "minecraft:is_beach") {
            Fallout::Sand
        } else if self.has(id, "minecraft:is_badlands") {
            Fallout::Terracotta
        } else if self.has(id, "c:is_cold") {
            Fallout::IceSnow
        } else {
            Fallout::Default
        }
    }
    fn copy_block(&self, id: BiomeId) -> &'static str {
        if self.has(id, "minecraft:is_badlands") {
            "minecraft:terracotta"
        } else if self.has(id, "c:is_snowy") {
            "minecraft:snow_block"
        } else if self.has(id, "minecraft:is_beach") || self.has(id, "c:is_sandy") {
            "minecraft:sand"
        } else if self.has(id, "c:is_plains") || self.has(id, "minecraft:is_forest") {
            "minecraft:dirt"
        } else {
            "minecraft:cobblestone"
        }
    }
}
#[derive(Debug)]
struct Extension {
    bindings: Arc<Bindings>,
}
/// Registration occurs before any dimension generator shares the loaded data.
/// The source structure and structure-set JSON remain active data-pack inputs.
pub(super) fn register(
    data: Arc<GeneratorData>,
) -> Result<Arc<GeneratorData>, horizon_world::worldgen::generator::GeneratorError> {
    let Some(bindings) = Bindings::new(&data) else {
        return Ok(data);
    };
    data.with_structure_extension(
        TYPE.into(),
        Arc::new(Extension {
            bindings,
        }),
    )
}
struct PieceRng<'a>(&'a mut dyn PieceRandom);
impl MeteorRandom for PieceRng<'_> {
    fn next_float(&mut self) -> f32 {
        self.0.float()
    }
    fn next_double(&mut self) -> f64 {
        self.0.double()
    }
    fn next_int_bound(&mut self, bound: i32) -> Result<i32, horizon_core::random::InvalidBound> {
        if bound <= 0 {
            Err(horizon_core::random::InvalidBound)
        } else {
            Ok(self.0.int(bound))
        }
    }
}
impl StructureExtension for Extension {
    fn generate(
        &self,
        context: &mut dyn StartContext,
        random: &mut dyn PieceRandom,
        chunk: ChunkPos,
    ) -> Option<GeneratedPieces> {
        let mut check = WorldgenRandom::new(LegacyRandom::new(0));
        check.set_large_feature_seed(context.seed(), chunk.x, chunk.z);
        if !check.next_boolean() {
            return None;
        }
        let (sx, sz) = (chunk.x * 16 + 8, chunk.z * 16 + 8);
        let sy = context.base_height(sx, sz, HeightmapType::OceanFloorWg) - 1;
        if !context.column_has_valid_biome(sx, sz, context.min_y() - 1, context.max_y()) {
            return None;
        }
        let x = chunk.x * 16 + random.int(16);
        let z = chunk.z * 16 + random.int(16);
        let radius = random.float() * 6.0 + 2.0;
        let biome = context.noise_biome([x >> 2, context.sea_level() >> 2, z >> 2]);
        let ocean = self.bindings.has(biome, "c:is_ocean");
        let kind = if ocean {
            HeightmapType::OceanFloorWg
        } else {
            HeightmapType::WorldSurfaceWg
        };
        let scan = (radius * 2.0).max(1.0) as i32;
        let (mut count, mut mean, mut squares, mut low) = (0_u32, 0.0, 0.0, i32::MAX);
        for dx in -scan..=scan {
            for dz in -scan..=scan {
                let h = context.base_height(x + dx, z + dz, kind);
                count += 1;
                let delta = f64::from(h) - mean;
                mean += delta / f64::from(count);
                squares += delta * (f64::from(h) - mean);
                low = low.min(h);
            }
        }
        let mut y = mean as i32;
        if squares / f64::from(count) > 5.0 {
            y = (f64::from(y) - (mean - f64::from(low)) * 0.75) as i32;
        }
        let offset = radius.ceil() as i32 + 1;
        y = (y - offset).max(context.min_y() + offset);
        let climate = *self.bindings.temperatures.get(biome.0 as usize)?;
        let snow = context.cold_enough_to_snow(biome, [x, y, z]);
        let crater_lake = surrounding_water(context, [x, y, z], radius);
        let crater = source::choose_crater(
            &mut PieceRng(random),
            climate.base_temperature,
            climate.has_precipitation,
            snow,
            ocean,
        );
        let m = Meteorite {
            pos: [x, y, z],
            radius: f64::from(radius),
            crater,
            fallout: self.bindings.fallout(biome),
            pure_crater: random.float() > 0.9,
            crater_lake,
        };
        Some((
            [sx, sy, sz],
            vec![Box::new(MeteorPiece {
                meteor: m,
                bindings: self.bindings.clone(),
            })],
        ))
    }
    fn read_piece(&self, piece: &Value) -> Result<Box<dyn LegacyPiece>, String> {
        Ok(Box::new(MeteorPiece {
            meteor: decode_settings(piece)?,
            bindings: self.bindings.clone(),
        }))
    }
}
fn surrounding_water(context: &mut dyn StartContext, [x, y, z]: [i32; 3], radius: f32) -> bool {
    let max = context.sea_level() - 1;
    let h = f64::from(y as f32 - radius + 1.0);
    for dx in -32..=32 {
        for dz in -32..=32 {
            let d = f64::from(dx * dx + dz * dz);
            if f64::from(max) > h + d * 0.0175
                && f64::from(max) < h + d * 0.02
                && context.base_height(x + dx, z + dz, HeightmapType::OceanFloor)
                    < context.sea_level()
            {
                return true;
            }
        }
    }
    false
}
fn meteor_box(m: Meteorite) -> BoundingBox {
    let [x, y, z] = m.pos;
    BoundingBox::new(
        [(x >> 4) * 16 - 64, y, (z >> 4) * 16 - 64],
        [(x >> 4) * 16 + 79, y, (z >> 4) * 16 + 79],
    )
}
#[derive(Debug)]
struct MeteorPiece {
    meteor: Meteorite,
    bindings: Arc<Bindings>,
}
fn settings_tag(m: Meteorite) -> Compound {
    let mut tag = piece_tag(TYPE, meteor_box(m), None, 0);
    tag.insert(
        "c".into(),
        Tag::Long(BlockPos::new(m.pos[0], m.pos[1], m.pos[2]).pack()),
    );
    tag.insert("r".into(), Tag::Float((m.radius as f32).to_bits()));
    let crater = match m.crater {
        Crater::None => 0,
        Crater::Normal => 1,
        Crater::Lava => 2,
        Crater::Obsidian => 3,
        Crater::Water => 4,
        Crater::Snow => 5,
        Crater::Ice => 6,
    };
    let fallout = match m.fallout {
        Fallout::Default => 1,
        Fallout::Sand => 2,
        Fallout::Terracotta => 3,
        Fallout::IceSnow => 4,
    };
    for (key, value) in [
        ("t", crater),
        ("f", fallout),
        ("p", i8::from(m.pure_crater)),
        ("l", i8::from(m.crater_lake)),
    ] {
        tag.insert(key.into(), Tag::Byte(value));
    }
    tag
}
fn decode_settings(piece: &Value) -> Result<Meteorite, String> {
    if piece["id"].as_str() != Some(TYPE) {
        return Err("wrong AE2 meteorite piece type".into());
    }
    let packed = piece["c"].as_i64().ok_or("missing center")?;
    let pos = BlockPos::unpack(packed);
    let radius = piece["r"].as_f64().ok_or("missing radius")? as f32;
    if !radius.is_finite() || !(2.0..=8.0).contains(&radius) {
        return Err("invalid meteorite radius".into());
    }
    let crater = match piece["t"].as_i64() {
        Some(0) => Crater::None,
        Some(1) => Crater::Normal,
        Some(2) => Crater::Lava,
        Some(3) => Crater::Obsidian,
        Some(4) => Crater::Water,
        Some(5) => Crater::Snow,
        Some(6) => Crater::Ice,
        _ => return Err("invalid crater type".into()),
    };
    let fallout = match piece["f"].as_i64() {
        Some(0 | 1) => Fallout::Default,
        Some(2) => Fallout::Sand,
        Some(3) => Fallout::Terracotta,
        Some(4) => Fallout::IceSnow,
        _ => return Err("invalid fallout mode".into()),
    };
    let flag = |key: &str| {
        piece[key]
            .as_bool()
            .or_else(|| piece[key].as_i64().map(|v| v != 0))
            .ok_or_else(|| format!("missing {key}"))
    };
    Ok(Meteorite {
        pos: [pos.x, pos.y, pos.z],
        radius: f64::from(radius),
        crater,
        fallout,
        pure_crater: flag("p")?,
        crater_lake: flag("l")?,
    })
}
impl LegacyPiece for MeteorPiece {
    fn bounding_box(&self) -> BoundingBox {
        meteor_box(self.meteor)
    }
    fn type_name(&self) -> &'static str {
        TYPE
    }
    fn save(&self) -> Option<Compound> {
        Some(settings_tag(self.meteor))
    }
    fn post_process(&self, context: &mut PieceContext<'_>) -> Result<(), PlacementQueryError> {
        let copied = {
            let biome = context.world.biome(context.chunk_box.center())?;
            self.bindings.copy_block(biome)
        };
        let sea = context.world.sea_level()?;
        let mut world = NativeWorld {
            world: RefCell::new(&mut *context.world),
            bindings: &self.bindings,
            bounds: context.chunk_box,
            copied,
            sea,
            error: RefCell::new(None),
        };
        source::place(
            &mut world,
            self.meteor,
            crate::ae2_common_config::world_generation(),
            &mut PieceRng(&mut *context.random),
        );
        world.error.into_inner().map_or(Ok(()), Err)
    }
}
struct NativeWorld<'a> {
    world: RefCell<&'a mut dyn PieceWorld>,
    bindings: &'a Bindings,
    bounds: BoundingBox,
    copied: &'static str,
    sea: i32,
    error: RefCell<Option<PlacementQueryError>>,
}
impl NativeWorld<'_> {
    fn result<T>(&self, value: Result<T, PlacementQueryError>, fallback: T) -> T {
        match value {
            Ok(v) => v,
            Err(e) => {
                self.error.borrow_mut().get_or_insert(e);
                fallback
            }
        }
    }
    fn air_id(&self) -> BlockStateId {
        self.bindings
            .registry
            .default_state("minecraft:air")
            .expect("vanilla registry")
    }
}
impl source::World for NativeWorld<'_> {
    type State = BlockStateId;
    fn bounds(&self) -> ([i32; 3], [i32; 3]) {
        (self.bounds.min, self.bounds.max)
    }
    fn sea_level(&self) -> i32 {
        self.sea
    }
    fn state(&self, name: &str) -> Option<Self::State> {
        let reg = self.bindings.registry;
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
        let result = self.world.borrow_mut().block(p);
        self.result(result, self.air_id())
    }
    fn put(&mut self, p: [i32; 3], state: Self::State) {
        if !self.bounds.is_inside(p) || self.error.borrow().is_some() {
            return;
        }
        let current = self.get(p);
        let reg = self.bindings.registry;
        if current == state
            || reg
                .state(current)
                .is_some_and(|s| s.name == "minecraft:bedrock")
        {
            return;
        }
        let result = self.world.borrow_mut().set(p, state, 3);
        self.result(result, false);
        if reg
            .state(state)
            .is_some_and(|s| s.name == "ae2:mysterious_cube")
        {
            let mut data = Compound::new();
            data.insert("id".into(), Tag::String("ae2:mysterious_cube".into()));
            let result = self.world.borrow_mut().block_entity(p, data);
            self.result(result, ());
        }
    }
    fn air(&self, state: Self::State) -> bool {
        self.bindings.registry.state(state).is_some_and(|s| {
            matches!(
                s.name.as_str(),
                "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
            )
        })
    }
    fn solid(&self, state: Self::State) -> bool {
        self.bindings.solid.contains(state)
    }
    fn replaceable(&self, state: Self::State) -> bool {
        self.bindings.replaceable.contains(state)
    }
    fn fluid(&self, state: Self::State) -> bool {
        self.bindings
            .fluids
            .get(state)
            .is_some_and(|s| s.kind() != FluidKind::Empty)
    }
    fn replaceable_tag(&self, state: Self::State) -> bool {
        self.bindings.replaceable_tag.contains(&state)
    }
    fn same_block(&self, a: Self::State, b: Self::State) -> bool {
        self.bindings
            .registry
            .state(a)
            .zip(self.bindings.registry.state(b))
            .is_some_and(|(a, b)| a.name == b.name)
    }
    fn copied_fallout(&self) -> &'static str {
        self.copied
    }
    fn schedule_surface_water(&mut self, p: [i32; 3]) {
        let result = self.world.borrow_mut().fluid_tick(p, FluidKind::Water, 0);
        self.result(result, ());
    }
    fn level_random_float(&mut self) -> f32 {
        let result = self.world.borrow_mut().level_float();
        self.result(result, 0.5)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn anvil_saved_ae2_owner_is_restored_when_neighbor_generates_first() {
        use crate::save::SaveDir;
        use horizon_world::{compression::Compression, nbt::{Limits, NamedTag}, region::{ChunkRecord, RegionFile}, worldgen::generator::OverworldGenerator};
        let Some(root) = std::env::var_os("AE2_MERGED_TEST_RESOURCES").map(std::path::PathBuf::from) else { eprintln!("skipping: AE2_MERGED_TEST_RESOURCES absent"); return; };
        let registry = Box::leak(Box::new(horizon_world::level::data::load_registry(&root).unwrap()));
        let data = GeneratorData::load(&root.join("worldgen"), registry).unwrap();
        let data = register(data).unwrap();
        let dir = std::env::temp_dir().join(format!("ae2-saved-neighbor-{}",std::process::id()));
        std::fs::create_dir_all(dir.join("region")).unwrap();
        let m = Meteorite { pos:[8,70,8], radius:6.25, crater:Crater::Ice, fallout:Fallout::Terracotta, pure_crater:true, crater_lake:false };
        let saved_start = Tag::compound([("id",Tag::String("ae2:meteorite".into())),("ChunkX",Tag::Int(0)),("ChunkZ",Tag::Int(0)),("references",Tag::Int(7)),("Children",Tag::List(vec![Tag::Compound(settings_tag(m))]))]);
        let saved = NamedTag { name:"".into(), tag:Tag::compound([("Status",Tag::String("minecraft:full".into())),("structures",Tag::compound([("starts",Tag::compound([("ae2:meteorite",saved_start.clone())]))]))]) };
        let record = ChunkRecord::from_nbt(&saved,Compression::Zlib,0,Limits::default()).unwrap();
        RegionFile::open_writable(dir.join("region/r.0.0.mca"),true).unwrap().write(ChunkPos::new(0,0),&record).unwrap();
        let generator = OverworldGenerator::new(data,1234).unwrap();
        generator.set_saved_start_source(Arc::new(SaveDir::new(dir, crate::save::LevelShape::OVERWORLD)));
        let mut worker = generator.worker();
        let neighbor = worker.generate_proto(ChunkPos::new(1,0)).unwrap();
        assert!(neighbor.structures().iter().any(|(name,pos,_,_)| *name=="ae2:meteorite" && *pos==ChunkPos::new(0,0)));
        let starts = worker.structure_starts(ChunkPos::new(0,0));
        let start = starts.iter().find(|s| s.name=="ae2:meteorite").expect("saved AE2 start");
        assert_eq!(start.save(),Some(saved_start));
        let piece = start.pieces[0].save().unwrap();
        assert_eq!(piece.get(&"r".into()), Some(&Tag::Float(6.25_f32.to_bits())));
        assert_eq!(piece.get(&"t".into()), Some(&Tag::Byte(6)));
        assert_eq!(piece.get(&"f".into()), Some(&Tag::Byte(3)));
    }
    #[test]
    fn source_piece_settings_round_trip_all_variants_and_negative_positions() {
        for crater in [
            Crater::None,
            Crater::Normal,
            Crater::Lava,
            Crater::Obsidian,
            Crater::Water,
            Crater::Snow,
            Crater::Ice,
        ] {
            for fallout in [
                Fallout::Default,
                Fallout::Sand,
                Fallout::Terracotta,
                Fallout::IceSnow,
            ] {
                let m = Meteorite {
                    pos: [-340, 29, -531],
                    radius: 6.25,
                    crater,
                    fallout,
                    pure_crater: true,
                    crater_lake: false,
                };
                let mut json = crate::scoreboard::tag_to_json(&Tag::Compound(settings_tag(m)));
                json["t"] = Value::from(match settings_tag(m).get(&"t".into()) {
                    Some(Tag::Byte(v)) => *v,
                    _ => unreachable!(),
                });
                json["f"] = Value::from(match settings_tag(m).get(&"f".into()) {
                    Some(Tag::Byte(v)) => *v,
                    _ => unreachable!(),
                });
                let restored = decode_settings(&json).unwrap();
                assert_eq!(restored.pos, m.pos);
                assert_eq!(restored.radius, m.radius);
                assert_eq!(restored.crater, crater);
                assert_eq!(restored.fallout, fallout);
                assert!(restored.pure_crater);
                assert!(!restored.crater_lake);
                assert_eq!(meteor_box(m).min, [-416, 29, -608]);
            }
        }
    }
}
