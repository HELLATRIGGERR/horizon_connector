// Forge Energy for Mekanism's energy cubes, through the engine's FE
// capability (hook points API 63): support module `crate::game::mek_fe` of
// the test bundle `mekanism_fe`, applied over the `mekanism` bundle
// (pack/mods/mekanism) for the world test beside it. The cube stays that
// bundle's (its Joules, side config and ejection into other cubes); this
// module adds what Mekanism 10.7 does at the FE edge, reading the same saved
// compound:
// - the cube as an `IEnergyStorage` on each side (`ForgeEnergyIntegration`
//   over the side's handler): `native_fe_storage`, `native_fe_receive`,
//   `native_fe_extract`;
// - its ejector's Forge Energy targets (`ForgeStrictEnergyHandler`): a
//   neighbour on an output face that has FE there and is no cube, which
//   the bundle's own tick doesn't feed.
// The rules are the bundle crate `mekanism_energy` (MIT, from Mekanism
// 10.7.19, Copyright (c) 2017-2025 Aidan C. Brady). Once the cube's own
// module takes FE (it then also counts FE targets in its one ejection),
// this test bundle goes.

use super::{Game, native_hooks::NativeFeInfo};
use horizon_client::containers::ModBlockEntity;
use horizon_core::geometry::Direction as Face;
use horizon_world::nbt::{Compound, JavaString, Tag};
use mekanism_energy::{
    config::EnergyConfig,
    container::Automation,
    cube::{self, CubeState},
    fe::{self, Conversion, StrictEnergy},
    sides::{self, DataType, Direction},
    tier::CubeTier,
};
use std::cell::RefCell;

thread_local! {
    static CONFIG: RefCell<Option<EnergyConfig>> = const { RefCell::new(None) };
}

/// The bundle's settings (`config/Mekanism/`), read once on this thread.
fn config() -> EnergyConfig {
    CONFIG.with(|c| {
        c.borrow_mut()
            .get_or_insert_with(|| Game::native_config_dir().map(|dir| EnergyConfig::load(&dir)).unwrap_or_default())
            .clone()
    })
}

fn mek(face: Face) -> Direction {
    Direction::ALL[face as usize]
}

fn face(d: Direction) -> Face {
    Face::ALL[d as usize]
}

// ---- The cube's saved compound, as the `mekanism` bundle writes it ---------

fn key(name: &str) -> JavaString {
    JavaString::from(name)
}

fn number(tag: Option<&Tag>) -> Option<i64> {
    match tag? {
        Tag::Byte(v) => Some(i64::from(*v)),
        Tag::Short(v) => Some(i64::from(*v)),
        Tag::Int(v) => Some(i64::from(*v)),
        Tag::Long(v) => Some(*v),
        _ => None,
    }
}

fn read_cube(nbt: &Compound, tier: CubeTier, capacity: u64) -> CubeState {
    let mut cube = CubeState::new(tier, capacity);
    let saved = match nbt.get(&key("energy_containers")) {
        Some(Tag::List(list)) => list.iter().find_map(|entry| match entry {
            Tag::Compound(c) if number(c.get(&key("container"))).unwrap_or(0) == 0 => {
                Some(number(c.get(&key("stored"))).unwrap_or(0).max(0) as u64)
            }
            _ => None,
        }),
        _ => None,
    };
    cube.stored = cube::stored_from(tier, saved, capacity);
    if let Some(Tag::String(facing)) = nbt.get(&key("facing"))
        && let Some(d) = Direction::from_name(&facing.to_string_lossy())
    {
        cube.facing = d;
    }
    if let Some(Tag::Compound(config)) = nbt.get(&key("component_config")) {
        if let Some(Tag::IntArray(ids)) = config.get(&key("config0")) {
            for (side, id) in cube.energy.iter_mut().zip(ids) {
                *side = DataType::by_id(*id);
            }
        }
        if let Some(v) = number(config.get(&key("eject0"))) {
            cube.eject = v != 0;
        }
    }
    cube
}

fn write_energy(nbt: &mut Compound, stored: u64) {
    if stored == 0 {
        nbt.remove(&key("energy_containers"));
        return;
    }
    let mut container = Compound::new();
    container.insert(key("container"), Tag::Byte(0));
    container.insert(key("stored"), Tag::Long(stored.min(i64::MAX as u64) as i64));
    nbt.insert(key("energy_containers"), Tag::List(vec![Tag::Compound(container)]));
}

/// A cube's energy as one side reaches it (Mekanism's sided proxy handler):
/// an input side only takes, an output side only gives, and the block itself
/// (no side) does both.
struct Side<'a> {
    cube: &'a mut CubeState,
    face: Option<Direction>,
    capacity: u64,
    output: u64,
}

impl Side<'_> {
    fn io(&self) -> (bool, bool) {
        match self.face {
            Some(d) => sides::io(self.cube.energy_at(d)).unwrap_or((false, false)),
            None => (true, true),
        }
    }
}

impl StrictEnergy for Side<'_> {
    fn insert(&mut self, joules: i64, simulate: bool) -> i64 {
        if !self.io().0 || joules <= 0 {
            return joules;
        }
        let mut container = self.cube.container(self.capacity, self.output);
        let rest = container.insert(joules as u64, simulate, Automation::External);
        self.cube.stored = container.stored;
        rest as i64
    }

    fn extract(&mut self, joules: i64, simulate: bool) -> i64 {
        if !self.io().1 || joules <= 0 {
            return 0;
        }
        let mut container = self.cube.container(self.capacity, self.output);
        let given = container.extract(joules as u64, simulate, Automation::External);
        self.cube.stored = container.stored;
        given as i64
    }
}

/// The cube in `entity` and its tier's size, when it is one and `face`
/// exposes its energy (a `NONE` side has no capability).
fn exposed(entity: &ModBlockEntity, face: Option<Face>, config: &EnergyConfig) -> Option<(CubeState, u64, u64)> {
    let tier = CubeTier::of_block(&entity.id)?;
    if config.blacklist_forge {
        return None;
    }
    let (capacity, output) = config.cube(tier);
    let cube = read_cube(&entity.nbt, tier, capacity);
    if let Some(f) = face
        && !cube.exposes(mek(f))
    {
        return None;
    }
    Some((cube, capacity, output))
}

// ---- The FE hook points (API 63) -------------------------------------------

/// `native_fe_storage`: `ForgeEnergyIntegration`'s readback, in whole FE;
/// Mekanism answers `canReceive` and `canExtract` true on every side it
/// exposes.
pub(super) fn storage(face: Option<Face>, entity: &ModBlockEntity, info: &mut Option<NativeFeInfo>) {
    if info.is_some() {
        return;
    }
    let config = config();
    let Some((cube, capacity, _)) = exposed(entity, face, &config) else {
        return;
    };
    let rate = Conversion(config.fe_conversion_rate);
    *info = Some(NativeFeInfo {
        stored: i64::from(rate.to_fe_int(cube.stored.min(i64::MAX as u64) as i64)),
        capacity: i64::from(rate.to_fe_int(capacity.min(i64::MAX as u64) as i64)),
        can_receive: true,
        can_extract: true,
    });
}

/// `native_fe_receive` and `native_fe_extract`: `ForgeEnergyIntegration`'s
/// `receiveEnergy` and `extractEnergy` on the side's handler.
pub(super) fn transfer(
    face: Option<Face>,
    entity: &mut ModBlockEntity,
    max: i64,
    simulate: bool,
    extract: bool,
    moved: &mut Option<i64>,
) {
    if moved.is_some() {
        return;
    }
    let config = config();
    let Some((mut cube, capacity, output)) = exposed(entity, face, &config) else {
        return;
    };
    let rate = Conversion(config.fe_conversion_rate);
    let before = cube.stored;
    let mut side = Side { cube: &mut cube, face: face.map(mek), capacity, output };
    let max = max.clamp(0, i64::from(i32::MAX)) as i32;
    let fe = if extract {
        fe::extract_fe(&mut side, rate, max, simulate)
    } else {
        fe::receive_fe(&mut side, rate, max, simulate)
    };
    if !simulate && cube.stored != before {
        write_energy(&mut entity.nbt, cube.stored);
    }
    *moved = Some(i64::from(fe));
}

// ---- The ejector's Forge Energy targets ------------------------------------

fn beside(pos: [i32; 3], d: Direction) -> [i32; 3] {
    let [dx, dy, dz] = d.offset();
    [pos[0] + dx, pos[1] + dy, pos[2] + dz]
}

/// `native_block_entity_tick` of a cube, after the bundle's own: the
/// ejector (`CableUtils.emit`) pushes up to the tier's output into the
/// Forge Energy neighbours on its output faces, split as Mekanism splits it,
/// each seen through `ForgeStrictEnergyHandler` (whole FE at
/// `feConversionRate` Joules each).
pub(super) fn tick(game: &mut Game<'_>, pos: [i32; 3], entity: &mut ModBlockEntity) {
    let Some(tier) = CubeTier::of_block(&entity.id) else {
        return;
    };
    let config = config();
    if config.blacklist_forge {
        return;
    }
    let (capacity, output) = config.cube(tier);
    let mut cube = read_cube(&entity.nbt, tier, capacity);
    if !cube.eject || cube.stored == 0 {
        return;
    }
    let dimension = game.native_dimension();
    let targets: Vec<(Direction, bool)> = cube
        .output_faces()
        .into_iter()
        .filter_map(|d| {
            let at = beside(pos, d);
            if CubeTier::of_block(&game.native_block(at)).is_some() {
                return None;
            }
            let info = game.native_fe_info(dimension, at, Some(face(d.opposite())), false)?;
            Some((d, info.can_receive))
        })
        .collect();
    if targets.is_empty() {
        return;
    }
    let rate = Conversion(config.fe_conversion_rate);
    let before = cube.stored;
    let mut container = cube.container(capacity, output);
    cube::emit(&mut container, output, targets.len(), |i, joules, simulate| {
        let (d, can_receive) = targets[i];
        let at = beside(pos, d);
        let joules = joules.min(i64::MAX as u64) as i64;
        let rest = fe::insert_into_fe(joules, rate, can_receive, simulate, |fe, simulate| {
            game.native_fe_offer(dimension, at, Some(face(d.opposite())), i64::from(fe), simulate, false) as i32
        });
        rest.max(0) as u64
    });
    cube.stored = container.stored;
    if cube.stored != before {
        write_energy(&mut entity.nbt, cube.stored);
    }
}
