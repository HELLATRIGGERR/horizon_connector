// Mekanism's energy blocks in the game (support module `crate::game::mek_world`
// of the `mekanism` native bundle). The rules are the bundle's crate
// `mekanism_energy` (MIT, from Mekanism 10.7.19, Copyright (c) 2017-2025
// Aidan C. Brady); this module is the glue: the cubes' block entities and
// their saved compound, the ejector's neighbours, the settings from
// `config/Mekanism/`, the menu's data slots and buttons (its screen is
// `mek_ui`), and `/mek probe` for tests. Server hooks run on the
// thread of the `Game` that simulates the world, so the settings are kept per
// thread, read when a world opens and dropped when it closes. No borrow of
// that state is held across a call into the game.

use super::{
    Game,
    native_lifecycle::WorldPhase,
    native_server::{NativeCommand, NativeServerPlayer},
};
use horizon_client::{
    containers::{ModBlockEntity, ModContainer},
    text::Text,
};
use horizon_world::{
    level::flags,
    nbt::{Compound, JavaString, Tag},
};
use mekanism_energy::{
    config::EnergyConfig,
    cube::{self, CubeState},
    menu::{Button, View},
    sides::{DataType, Direction, RelativeSide},
    tier::CubeTier,
};
use std::{cell::RefCell, collections::HashMap};

/// The settings of the open world, and whether `tiers.toml` was there.
struct Settings {
    energy: EnergyConfig,
    from_files: bool,
}

thread_local! {
    static SETTINGS: RefCell<Option<Settings>> = const { RefCell::new(None) };
    /// Joules each cube took in since its last tick (`LastEnergyTracker`), by position.
    static RECEIVED: RefCell<HashMap<[i32; 3], u64>> = RefCell::new(HashMap::new());
}

fn read_settings() -> Settings {
    match Game::native_config_dir() {
        Ok(dir) => Settings {
            energy: EnergyConfig::load(&dir),
            from_files: dir.join("Mekanism").join("tiers.toml").is_file(),
        },
        Err(e) => {
            log::warn!("[mekanism] no config folder ({e}): Mekanism's defaults");
            Settings { energy: EnergyConfig::default(), from_files: false }
        }
    }
}

/// The energy settings, read on first use if the world's opening didn't.
fn energy_config() -> (EnergyConfig, bool) {
    SETTINGS.with(|s| {
        let mut s = s.borrow_mut();
        let s = s.get_or_insert_with(read_settings);
        (s.energy.clone(), s.from_files)
    })
}

/// `native_world_lifecycle`: settings are read as a world opens (ATM10's
/// files, copied by `pack/scripts/sync-atm10.ts`) and dropped as it closes.
pub(super) fn lifecycle(phase: WorldPhase) {
    let fresh = matches!(phase, WorldPhase::WorldOpenReady).then(read_settings);
    SETTINGS.with(|s| *s.borrow_mut() = fresh);
    RECEIVED.with(|r| r.borrow_mut().clear());
}

// ---- The saved compound, in Mekanism's names ----------------------------

/// `TransmissionType` ordinals in `component_config`.
const ENERGY: u8 = 0;
const ITEM: u8 = 3;

fn key(name: &str) -> JavaString {
    JavaString::from(name)
}

fn number(tag: Option<&Tag>) -> Option<i64> {
    match tag? {
        Tag::Byte(v) => Some(i64::from(*v)),
        Tag::Short(v) => Some(i64::from(*v)),
        Tag::Int(v) => Some(i64::from(*v)),
        Tag::Long(v) => Some(*v),
        // Mekanism's old FloatingLong saves are strings.
        Tag::String(s) => s.to_string_lossy().split('.').next()?.parse().ok(),
        _ => None,
    }
}

/// `energy_containers`: `[{container: 0b, stored: 4000L}]`, the container
/// left out when empty (`DataHandlerUtils.writeContents`).
fn saved_energy(nbt: &Compound) -> Option<u64> {
    let Some(Tag::List(list)) = nbt.get(&key("energy_containers")) else {
        return None;
    };
    list.iter().find_map(|entry| match entry {
        Tag::Compound(c) if number(c.get(&key("container"))).unwrap_or(0) == 0 => {
            Some(number(c.get(&key("stored"))).unwrap_or(0).max(0) as u64)
        }
        _ => None,
    })
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

fn sides_of(config: &Compound, kind: u8, into: &mut [DataType; 6]) {
    if let Some(Tag::IntArray(ids)) = config.get(&key(&format!("config{kind}"))) {
        for (side, id) in into.iter_mut().zip(ids) {
            *side = DataType::by_id(*id);
        }
    }
}

/// The cube in `nbt`; what is missing keeps a new cube's value.
fn read_cube(nbt: &Compound, tier: CubeTier, capacity: u64) -> CubeState {
    let mut cube = CubeState::new(tier, capacity);
    cube.stored = cube::stored_from(tier, saved_energy(nbt), capacity);
    if let Some(Tag::String(facing)) = nbt.get(&key("facing"))
        && let Some(d) = Direction::from_name(&facing.to_string_lossy())
    {
        cube.facing = d;
    }
    if let Some(Tag::Compound(config)) = nbt.get(&key("component_config")) {
        sides_of(config, ENERGY, &mut cube.energy);
        sides_of(config, ITEM, &mut cube.items);
        if let Some(v) = number(config.get(&key(&format!("eject{ENERGY}")))) {
            cube.eject = v != 0;
        }
        if let Some(v) = number(config.get(&key(&format!("eject{ITEM}")))) {
            cube.items_eject = v != 0;
        }
    }
    cube
}

/// `facing` and `component_config` (`TileComponentConfig.write`).
fn write_config(nbt: &mut Compound, cube: &CubeState) {
    nbt.insert(key("facing"), Tag::String(key(cube.facing.name())));
    let mut config = Compound::new();
    for (kind, sides, eject) in [(ENERGY, &cube.energy, cube.eject), (ITEM, &cube.items, cube.items_eject)] {
        config.insert(key(&format!("eject{kind}")), Tag::Byte(i8::from(eject)));
        config.insert(key(&format!("config{kind}")), Tag::IntArray(sides.iter().map(|t| t.id()).collect()));
    }
    nbt.insert(key("component_config"), Tag::Compound(config));
}

// ---- Block entities -------------------------------------------------------

/// `native_block_entity`: a cube's entity, with the item's default config.
/// A creative cube saves no energy, so it counts as full.
pub(super) fn new_entity(block: &str, entity: &mut Option<(String, Compound)>) {
    if entity.is_some() {
        return;
    }
    let Some(tier) = CubeTier::of_block(block) else {
        return;
    };
    let mut nbt = Compound::new();
    write_config(&mut nbt, &CubeState::new(tier, 0));
    *entity = Some((block.to_owned(), nbt));
}

/// `native_block_entity_ticks`: cubes tick.
pub(super) fn ticks(block: &str, ticks: &mut bool) {
    if CubeTier::of_block(block).is_some() {
        *ticks = true;
    }
}

/// `native_block_entity_container`: the discharge slot (0, fills the cube
/// from an item) and the charge slot (1), in a hopper's five-slot menu whose
/// other three slots the screen leaves out. Every face reaches both; hoppers
/// put into the charge slot and take from both.
pub(super) fn container(block: &str, container: &mut Option<ModContainer>) {
    if container.is_some() || CubeTier::of_block(block).is_none() {
        return;
    }
    *container = Some(ModContainer {
        slots: 5,
        faces: std::array::from_fn(|_| vec![0, 1]),
        inputs: vec![1],
        outputs: vec![0, 1],
        menu: Some("minecraft:hopper".into()),
    });
}

fn beside(pos: [i32; 3], face: Direction) -> [i32; 3] {
    let [dx, dy, dz] = face.offset();
    [pos[0] + dx, pos[1] + dy, pos[2] + dz]
}

/// Whether the block on `face` of `pos` takes energy from there: a cube
/// whose touching side isn't `NONE`. (Other mods' blocks join with Forge
/// Energy.)
fn acceptor(game: &mut Game<'_>, config: &EnergyConfig, pos: [i32; 3], face: Direction) -> bool {
    game.native_with_mod_block_entity(beside(pos, face), |m| {
        let tier = CubeTier::of_block(&m.id)?;
        read_cube(&m.nbt, tier, config.cube(tier).0).exposes(face.opposite()).then_some(())
    })
    .flatten()
    .is_some()
}

/// Offers `amount` Joules to the block on `face` of `pos`: what it didn't take.
fn offer(game: &mut Game<'_>, config: &EnergyConfig, pos: [i32; 3], face: Direction, amount: u64, simulate: bool) -> u64 {
    let rest = game.native_with_mod_block_entity(beside(pos, face), |m| {
        let tier = CubeTier::of_block(&m.id)?;
        let (capacity, output) = config.cube(tier);
        let mut cube = read_cube(&m.nbt, tier, capacity);
        let before = cube.stored;
        let rest = cube.receive(face.opposite(), amount, simulate, capacity, output)?;
        if cube.stored != before {
            write_energy(&mut m.nbt, cube.stored);
        }
        Some(rest)
    })
    .flatten()
    .unwrap_or(amount);
    if !simulate && rest < amount {
        let at = beside(pos, face);
        RECEIVED.with(|r| *r.borrow_mut().entry(at).or_default() += amount - rest);
    }
    rest
}

/// `native_block_entity_tick` of a cube (`TileEntityEnergyCube.onUpdateServer`):
/// the ejector pushes up to the tier's output out of the output faces, split
/// among the neighbours that take it; then the menu's data slots. Redstone
/// control is IGNORED, the default; the slots hold no energy items yet.
pub(super) fn tick(game: &mut Game<'_>, pos: [i32; 3], entity: &mut ModBlockEntity) {
    let Some(tier) = CubeTier::of_block(&entity.id) else {
        return;
    };
    let (config, _) = energy_config();
    let (capacity, output) = config.cube(tier);
    let mut cube = read_cube(&entity.nbt, tier, capacity);
    if cube.eject && cube.stored > 0 {
        let before = cube.stored;
        let targets: Vec<Direction> = cube
            .output_faces()
            .into_iter()
            .filter(|&face| acceptor(game, &config, pos, face))
            .collect();
        let mut container = cube.container(capacity, output);
        cube::emit(&mut container, output, targets.len(), |i, amount, simulate| {
            offer(game, &config, pos, targets[i], amount, simulate)
        });
        cube.stored = container.stored;
        if cube.stored != before {
            write_energy(&mut entity.nbt, cube.stored);
        }
    }
    let input = RECEIVED.with(|r| r.borrow_mut().remove(&pos)).unwrap_or(0);
    entity.data = View::of(&cube, capacity, output, input).pack();
}

/// `native_menu_button`: a side configuration button of the cube's screen
/// (`mek_ui`). A changed energy side shows on the block at once: the face's
/// property is set with `UPDATE_CLIENTS`, as Mekanism updates its model.
pub(super) fn button(game: &mut Game<'_>, pos: [i32; 3], entity: &mut ModBlockEntity, id: i32) {
    let (Some(tier), Some(button)) = (CubeTier::of_block(&entity.id), Button::from_id(id)) else {
        return;
    };
    let (config, _) = energy_config();
    let (capacity, output) = config.cube(tier);
    let mut cube = read_cube(&entity.nbt, tier, capacity);
    let before = cube.clone();
    let faces = cube.press(button);
    if cube == before {
        return;
    }
    write_config(&mut entity.nbt, &cube);
    if faces {
        for face in Direction::ALL {
            game.native_set_block_property_with(pos, face.name(), cube.side_state(face).name(), flags::UPDATE_CLIENTS);
        }
    }
    let input = entity.data.get(8..12).map_or(0, |d| d.iter().rev().fold(0u64, |v, x| (v << 16) | u64::from(*x as u16)));
    entity.data = View::of(&cube, capacity, output, input).pack();
}

// ---- /mek probe -----------------------------------------------------------

/// `native_register_commands`.
pub(super) fn commands(commands: &mut Vec<NativeCommand>) {
    commands.push(NativeCommand::op("mek probe <pos:block_pos>", 2));
}

/// A coordinate of the command line: a number, or `~`/`~n` from the player.
fn coordinate(word: Option<&str>, from: f64) -> Option<i32> {
    let word = word?;
    match word.strip_prefix('~') {
        Some("") => Some(from.floor() as i32),
        Some(rest) => Some((from + rest.parse::<f64>().ok()?).floor() as i32),
        None => word.parse().ok(),
    }
}

/// `native_perform_command`: `/mek probe x y z` answers `MEK {json}` with
/// what the Mekanism block entity there holds.
pub(super) fn perform(game: &mut Game<'_>, player: &NativeServerPlayer, command: &str, ran: &mut bool) {
    let mut words = command.split_whitespace();
    if *ran || words.next() != Some("mek") {
        return;
    }
    *ran = true;
    let answer = match words.next() {
        Some("probe") => {
            let p = player.position;
            match (coordinate(words.next(), p[0]), coordinate(words.next(), p[1]), coordinate(words.next(), p[2])) {
                (Some(x), Some(y), Some(z)) => probe(game, [x, y, z]),
                _ => serde_json::json!({ "error": "usage: /mek probe <x> <y> <z>" }),
            }
        }
        _ => serde_json::json!({ "error": "usage: /mek probe <x> <y> <z>" }),
    };
    game.native_feedback(Text::literal(format!("MEK {answer}")));
}

fn probe(game: &mut Game<'_>, pos: [i32; 3]) -> serde_json::Value {
    let (config, from_files) = energy_config();
    let found = game.native_with_mod_block_entity(pos, |m| (m.id.clone(), m.nbt.clone()));
    let Some((id, nbt)) = found else {
        return serde_json::json!({ "pos": pos, "error": "no mod block entity here" });
    };
    let Some(tier) = CubeTier::of_block(&id) else {
        return serde_json::json!({ "pos": pos, "id": id, "error": "not a Mekanism block" });
    };
    let (capacity, output) = config.cube(tier);
    let cube = read_cube(&nbt, tier, capacity);
    let faces: serde_json::Map<String, serde_json::Value> = Direction::ALL
        .iter()
        .map(|&d| (d.name().to_owned(), cube.side_state(d).name().into()))
        .collect();
    let energy_config: serde_json::Map<String, serde_json::Value> = RelativeSide::ALL
        .iter()
        .map(|&s| (s.name().to_owned(), cube.energy[s.index()].name().into()))
        .collect();
    serde_json::json!({
        "pos": pos,
        "id": id,
        "tier": tier.name(),
        "energy": cube.stored,
        "capacity": capacity,
        "output": output,
        "facing": cube.facing.name(),
        "eject": cube.eject,
        "faces": faces,
        "energy_config": energy_config,
        "config": { "from_files": from_files, "fe_conversion_rate": config.fe_conversion_rate },
    })
}
