//! `mekanism-content --carry DIR --overlay DIR --out DIR`: builds the content
//! mod `mekanism` that the game loads with `--dev-mod`, from the ATM10 carry
//! (`ports/atm10/mods/mekanism/carry`, Mekanism 10.7.19's own data and
//! assets, MIT, Copyright (c) 2017-2025 Aidan C. Brady) and the pack's own
//! declarations (`pack/mods/mekanism/content`).
//!
//! - The carry is copied, then the overlay over it: its block declarations
//!   replace the carried ones.
//! - Each energy cube the overlay declares with the six face properties
//!   (`down` ... `east` = `none|input|output`) gets its model converted:
//!   Mekanism's `mekanism:energy_cube` model loader becomes a multipart
//!   blockstate over a frame and one part per face and state, and the
//!   `builtin/entity` item model a flat model of the default state.
//! - Its loot table drops the cube without Mekanism's item components, which
//!   the game refuses; its `mekanism:mek_data` recipe becomes
//!   `minecraft:crafting_shaped`, so an upgrade keeps no energy.
//! - Mekanism's GUI textures (`assets/mekanism/gui/`) are copied where the
//!   game's GUI atlas takes a content mod's (`textures/gui/`, sprites
//!   `mekanism:gui/...`), with the energy texture (`textures/liquid/energy`)
//!   as `gui/energy_fill` for gauges.
//!
//! The output is generated, never committed: Mekanism's files stay local.
//! It may only replace a folder this tool made (`mekanism-content.json`).
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const MARKER: &str = "mekanism-content.json";
const FACES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];
const STATES: [&str; 3] = ["none", "input", "output"];
/// Mekanism's relative sides, as the unrotated model (facing north) places
/// them: its left is east.
const RELATIVE: [(&str, &str); 6] = [
    ("front", "north"),
    ("back", "south"),
    ("left", "east"),
    ("right", "west"),
    ("top", "up"),
    ("bottom", "down"),
];

type Result<T> = std::result::Result<T, String>;

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut carry, mut overlay, mut out) = (None, None, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--carry" => carry = args.next().map(PathBuf::from),
            "--overlay" => overlay = args.next().map(PathBuf::from),
            "--out" => out = args.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!("Usage: mekanism-content --carry DIR --overlay DIR --out DIR");
                return;
            }
            other => fail(&format!("unknown argument {other}")),
        }
    }
    let (Some(carry), Some(overlay), Some(out)) = (carry, overlay, out) else {
        fail("needs --carry, --overlay and --out");
    };
    match build(&carry, &overlay, &out) {
        Ok(report) => println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default()),
        Err(e) => fail(&e),
    }
}

fn fail(message: &str) -> ! {
    eprintln!("mekanism-content: {message}");
    std::process::exit(1);
}

fn build(carry: &Path, overlay: &Path, out: &Path) -> Result<Value> {
    if !carry.join("horizon-mod.toml").is_file() {
        return Err(format!("{} is not a carried content mod", carry.display()));
    }
    if !overlay.join("data").is_dir() {
        return Err(format!("{} has no data/", overlay.display()));
    }
    prepare(out)?;
    let copied = copy_tree(carry, out)?;
    let overlaid = copy_tree(overlay, out)?;
    let mut cubes = Vec::new();
    let mut loot = Vec::new();
    let mut recipes = Vec::new();
    let mut files = 0;
    let blocks = overlay.join("data/mekanism/horizon/block");
    let mut names: Vec<String> = match fs::read_dir(&blocks) {
        Ok(dir) => dir
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.ends_with(".json"))
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    for name in names {
        let block = name.trim_end_matches(".json");
        let Some(tier) = block.strip_suffix("_energy_cube") else {
            continue;
        };
        let decl = read_json(&blocks.join(&name))?;
        let defaults = cube_defaults(&decl).map_err(|e| format!("{name}: {e}"))?;
        let models = carry.join("assets/mekanism/models");
        let base = read_json(&models.join("block/energy_cube/base.json"))?;
        let tier_model = read_json(&models.join(format!("block/energy_cube/{tier}.json")))?;
        let item = read_json(&models.join(format!("item/{block}.json"))).ok();
        for (path, value) in cube_models(tier, &base, &tier_model, item.as_ref(), &defaults)? {
            write_json(&out.join(path), &value)?;
            files += 1;
        }
        cubes.push(block.to_owned());
        let loot_path = format!("data/mekanism/loot_table/blocks/{block}.json");
        if !overlay.join(&loot_path).exists()
            && let Ok(table) = read_json(&carry.join(&loot_path))
        {
            write_json(&out.join(&loot_path), &plain_drop(&table))?;
            loot.push(block.to_owned());
        }
        let recipe_path = format!("data/mekanism/recipe/energy_cube/{tier}.json");
        if !overlay.join(&recipe_path).exists()
            && let Ok(recipe) = read_json(&carry.join(&recipe_path))
            && let Some(shaped) = shaped_recipe(&recipe)?
        {
            write_json(&out.join(&recipe_path), &shaped)?;
            recipes.push(block.to_owned());
        }
    }
    let gui = out.join("assets/mekanism/textures/gui");
    let mut gui_sprites = copy_pngs(&carry.join("assets/mekanism/gui"), &gui)?;
    let energy = carry.join("assets/mekanism/textures/liquid/energy.png");
    if energy.is_file() {
        fs::copy(&energy, gui.join("energy_fill.png")).map_err(|e| format!("{}: {e}", energy.display()))?;
        gui_sprites += 1;
    }
    let report = json!({
        "carry": carry, "overlay": overlay, "out": out,
        "copied": copied, "overlaid": overlaid, "converted_files": files,
        "cubes": cubes, "loot_tables": loot, "recipes": recipes, "gui_sprites": gui_sprites,
    });
    write_json(&out.join(MARKER), &report)?;
    Ok(report)
}

/// Empties `out` when this tool made it; refuses any other folder.
fn prepare(out: &Path) -> Result<()> {
    if out.exists() {
        let empty = fs::read_dir(out).map_err(|e| e.to_string())?.next().is_none();
        if !empty && !out.join(MARKER).is_file() {
            return Err(format!("{} exists and was not made by mekanism-content", out.display()));
        }
        fs::remove_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))
}

fn copy_tree(from: &Path, to: &Path) -> Result<usize> {
    let mut count = 0;
    for entry in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            fs::create_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))?;
            count += copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target).map_err(|e| format!("{}: {e}", target.display()))?;
            count += 1;
        }
    }
    Ok(count)
}

/// The PNGs under `from`, copied to the same paths under `to` (their
/// `.mcmeta` files are not GUI sprites' business).
fn copy_pngs(from: &Path, to: &Path) -> Result<usize> {
    let mut count = 0;
    let Ok(dir) = fs::read_dir(from) else {
        return Ok(0);
    };
    for entry in dir {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            count += copy_pngs(&path, &target)?;
        } else if path.extension().is_some_and(|e| e == "png") {
            fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
            fs::copy(&path, &target).map_err(|e| format!("{}: {e}", target.display()))?;
            count += 1;
        }
    }
    Ok(count)
}

fn read_json(path: &Path) -> Result<Value> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    text.push('\n');
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The face states a cube declaration starts in; an error unless it declares
/// the six face properties with the three states. A property is an `enum`
/// (`{"type": "enum", "values": [...]}`) or a plain list; its default is in
/// `default_state` (or `state_defaults`).
fn cube_defaults(decl: &Value) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for face in FACES {
        let property = &decl["state_properties"][face];
        let values = property
            .as_array()
            .or_else(|| property["values"].as_array().filter(|_| property["type"] == "enum"))
            .ok_or_else(|| format!("no state property {face}"))?;
        let mut names: Vec<&str> = values.iter().filter_map(Value::as_str).collect();
        names.sort();
        if names != ["input", "none", "output"] {
            return Err(format!("state property {face} must be none, input, output"));
        }
        let default = decl["default_state"][face]
            .as_str()
            .or_else(|| decl["state_defaults"][face].as_str())
            .ok_or_else(|| format!("no default for {face}"))?;
        out.insert(face.to_owned(), default.to_owned());
    }
    Ok(out)
}

fn elements(model: &Value, key: &str) -> Result<Vec<Value>> {
    let list = model[key].as_array().ok_or_else(|| format!("the cube model has no {key}"))?;
    if list.is_empty() {
        return Err(format!("the cube model's {key} is empty"));
    }
    Ok(list.clone())
}

fn corner(element: &Value, key: &str) -> Result<[f64; 3]> {
    let v = element[key].as_array().filter(|v| v.len() == 3).ok_or("an element without from/to")?;
    let mut out = [0.0; 3];
    for (o, n) in out.iter_mut().zip(v) {
        *o = n.as_f64().ok_or("a coordinate that isn't a number")?;
    }
    Ok(out)
}

/// The face of the block a group of elements sits on, from where it is:
/// the axis its centre is farthest out on, and which way.
fn face_of(group: &[Value]) -> Result<&'static str> {
    let (mut min, mut max) = ([f64::MAX; 3], [f64::MIN; 3]);
    for e in group {
        let (from, to) = (corner(e, "from")?, corner(e, "to")?);
        for i in 0..3 {
            min[i] = min[i].min(from[i].min(to[i]));
            max[i] = max[i].max(from[i].max(to[i]));
        }
    }
    let off: Vec<f64> = (0..3).map(|i| (min[i] + max[i]) / 2.0 - 8.0).collect();
    let mut axes = [0, 1, 2];
    axes.sort_by(|a, b| off[*b].abs().total_cmp(&off[*a].abs()));
    if (off[axes[0]].abs() - off[axes[1]].abs()).abs() < 1.0 {
        return Err("a group that sits on no single face".into());
    }
    let i = axes[0];
    Ok(match (i, off[i] > 0.0) {
        (0, false) => "west",
        (0, true) => "east",
        (1, false) => "down",
        (1, true) => "up",
        (_, false) => "north",
        (_, true) => "south",
    })
}

/// An element as vanilla reads it: without the editor's `name`, and
/// NeoForge's per-element light as `light_emission`.
fn clean(element: &Value) -> Value {
    let mut e = element.clone();
    if let Some(o) = e.as_object_mut() {
        o.remove("name");
        if let Some(data) = o.remove("neoforge_data")
            && let Some(light) = data["block_light"].as_u64()
        {
            let light = light.min(15).max(o.get("light_emission").and_then(Value::as_u64).unwrap_or(0));
            o.insert("light_emission".into(), light.into());
        }
    }
    e
}

/// A lit LED (`EnergyCubeBakedModel.LED_TRANSFORMS`): full bright, its
/// texture moved 2 pixels left (`uvShift(-0.125, 0)`).
fn lit_led(element: &Value) -> Result<Value> {
    let mut e = clean(element);
    let faces = e["faces"].as_object_mut().ok_or("an LED without faces")?;
    for face in faces.values_mut() {
        let uv = face["uv"].as_array_mut().filter(|uv| uv.len() == 4).ok_or("an LED face without uv")?;
        for i in [0, 2] {
            let u = uv[i].as_f64().ok_or("a uv that isn't a number")?;
            uv[i] = json!(u - 2.0);
        }
    }
    e["light_emission"] = json!(15);
    Ok(e)
}

/// The texture a face names, through `#` references.
fn texture<'a>(textures: &'a Map<String, Value>, mut name: &'a str) -> &'a str {
    for _ in 0..8 {
        match name.strip_prefix('#').and_then(|k| textures.get(k)).and_then(Value::as_str) {
            Some(next) => name = next,
            None => break,
        }
    }
    name
}

/// A lit port (`QuadTransformation.filtered_fullbright`): only its quads
/// whose texture is an LED's are full bright.
fn lit_port(element: &Value, textures: &Map<String, Value>) -> Value {
    let mut e = clean(element);
    let led = e["faces"].as_object().is_some_and(|faces| {
        !faces.is_empty()
            && faces.values().all(|f| f["texture"].as_str().is_some_and(|t| texture(textures, t).contains("led")))
    });
    if led {
        e["light_emission"] = json!(15);
    }
    e
}

/// The models of energy cube tier `tier` (`basic`), from Mekanism's
/// `energy_cube/base.json` and `energy_cube/<tier>.json`: (path under the
/// content root, JSON). `defaults` are the face states the item shows.
fn cube_models(
    tier: &str,
    base: &Value,
    tier_model: &Value,
    item: Option<&Value>,
    defaults: &BTreeMap<String, String>,
) -> Result<Vec<(String, Value)>> {
    let mut textures = base["textures"].as_object().cloned().ok_or("the cube model has no textures")?;
    let parts_textures = textures.clone();
    if let Some(own) = tier_model["textures"].as_object() {
        textures.extend(own.clone());
    }
    let frame: Vec<Value> = elements(base, "frame")?.iter().map(clean).collect();
    // The LEDs and port of each absolute face, placed by their coordinates.
    let mut sides: BTreeMap<&str, (Vec<Value>, Vec<Value>)> = BTreeMap::new();
    for (side, named) in RELATIVE {
        let leds = elements(base, &format!("{side}LEDs"))?;
        let port = elements(base, &format!("{side}Port"))?;
        for group in [&leds, &port] {
            let at = face_of(group)?;
            if at != named {
                return Err(format!("{side}: found on {at}, expected {named}"));
            }
        }
        sides.insert(named, (leds, port));
    }
    let state_elements = |face: &str, state: &str| -> Result<Vec<Value>> {
        let (leds, port) = &sides[face];
        Ok(match state {
            "none" => leds.iter().map(clean).collect(),
            "input" => leds.iter().chain(port).map(clean).collect(),
            _ => {
                let mut out: Vec<Value> = leds.iter().map(lit_led).collect::<Result<_>>()?;
                out.extend(port.iter().map(|e| lit_port(e, &parts_textures)));
                out
            }
        })
    };
    let dir = "assets/mekanism/models/block/energy_cube";
    let mut files = Vec::new();
    let mut multipart = vec![json!({ "apply": { "model": format!("mekanism:block/energy_cube/{tier}_frame") } })];
    for face in FACES {
        for state in STATES {
            files.push((
                format!("{dir}/parts/{face}_{state}.json"),
                json!({ "textures": parts_textures, "elements": state_elements(face, state)? }),
            ));
            multipart.push(json!({
                "when": { face: state },
                "apply": { "model": format!("mekanism:block/energy_cube/parts/{face}_{state}") },
            }));
        }
    }
    files.push((
        format!("{dir}/{tier}_frame.json"),
        json!({ "parent": "minecraft:block/block", "textures": textures, "elements": frame }),
    ));
    files.push((
        format!("assets/mekanism/blockstates/{tier}_energy_cube.json"),
        json!({ "multipart": multipart }),
    ));
    let mut item_elements = frame.clone();
    for face in FACES {
        item_elements.extend(state_elements(face, &defaults[face])?);
    }
    let mut item_model = json!({ "parent": "minecraft:block/block", "textures": textures, "elements": item_elements });
    if let Some(display) = item.and_then(|i| i.get("display")) {
        item_model["display"] = display.clone();
    }
    files.push((format!("assets/mekanism/models/item/{tier}_energy_cube.json"), item_model));
    Ok(files)
}

/// The block's loot table without its item modifiers: Mekanism copies its
/// item components (energy, side config...) from the block entity, which the
/// game refuses; the cube drops plain until items carry energy.
fn plain_drop(table: &Value) -> Value {
    let mut table = table.clone();
    if let Some(pools) = table["pools"].as_array_mut() {
        for pool in pools {
            if let Some(entries) = pool["entries"].as_array_mut() {
                for entry in entries.iter_mut().filter_map(Value::as_object_mut) {
                    entry.remove("modifier");
                    entry.remove("functions");
                }
            }
        }
    }
    table
}

/// An ingredient as 26.3 writes it: an id, `#tag`, or a list of them.
fn ingredient(value: &Value) -> Result<Value> {
    Ok(match value {
        Value::String(_) => value.clone(),
        Value::Array(list) => Value::Array(list.iter().map(ingredient).collect::<Result<_>>()?),
        Value::Object(o) => match (o.get("item"), o.get("tag")) {
            (Some(Value::String(item)), None) => json!(item),
            (None, Some(Value::String(tag))) => json!(format!("#{tag}")),
            _ => return Err(format!("an ingredient this tool can't read: {value}")),
        },
        _ => return Err(format!("an ingredient this tool can't read: {value}")),
    })
}

/// A `mekanism:mek_data` recipe (a shaped recipe that carries the
/// ingredients' data into the result) as `minecraft:crafting_shaped`; none
/// for any other type.
fn shaped_recipe(recipe: &Value) -> Result<Option<Value>> {
    if recipe["type"] != "mekanism:mek_data" {
        return Ok(None);
    }
    let key = recipe["key"].as_object().ok_or("a mek_data recipe without key")?;
    let key: Map<String, Value> = key
        .iter()
        .map(|(k, v)| Ok((k.clone(), ingredient(v)?)))
        .collect::<Result<_>>()?;
    let mut out = json!({
        "type": "minecraft:crafting_shaped",
        "key": key,
        "pattern": recipe["pattern"].clone(),
        "result": recipe["result"].clone(),
    });
    if let Some(category) = recipe.get("category") {
        out["category"] = category.clone();
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(name: &str, from: [f64; 3], to: [f64; 3], texture: &str) -> Value {
        json!({ "name": name, "from": from, "to": to,
                "faces": { "north": { "uv": [2, 0, 2.5, 1], "texture": texture } } })
    }

    /// A small cube model in Mekanism's loader format: a frame piece, and
    /// for each side two LEDs and a port with its LED face.
    fn model() -> Value {
        let mut m = json!({
            "loader": "mekanism:energy_cube",
            "textures": { "1": "mekanism:block/models/energy_cube", "2": "mekanism:block/models/ports",
                          "3": "mekanism:block/models/ports_led", "4": "mekanism:block/models/leds" },
            "frame": [{ "name": "corner1Overlay", "from": [13, 13, 0], "to": [16, 16, 3],
                        "faces": { "north": { "uv": [0, 3, 3, 6], "texture": "#corner" } },
                        "neoforge_data": { "block_light": 10, "sky_light": 10 } }],
        });
        // Each side's plate, as the north one moved onto that face.
        let place = |face: &str, a: [f64; 3], b: [f64; 3]| -> ([f64; 3], [f64; 3]) {
            let map = |p: [f64; 3]| match face {
                "north" => p,
                "south" => [p[0], p[1], 16.0 - p[2]],
                "west" => [p[2], p[1], p[0]],
                "east" => [16.0 - p[2], p[1], p[0]],
                "down" => [p[0], p[2], p[1]],
                _ => [p[0], 16.0 - p[2], p[1]],
            };
            let (a, b) = (map(a), map(b));
            ([a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])], [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])])
        };
        for (side, face) in RELATIVE {
            let (f1, t1) = place(face, [3.0, 7.5, 0.1], [3.5, 8.5, 1.0]);
            let (f2, t2) = place(face, [12.5, 7.5, 0.1], [13.0, 8.5, 1.0]);
            m[format!("{side}LEDs")] = json!([element("led1", f1, t1, "#4"), element("led2", f2, t2, "#4")]);
            let (pf, pt) = place(face, [4.0, 4.0, 0.0], [12.0, 12.0, 1.0]);
            let (lf, lt) = place(face, [4.0, 4.0, 0.0], [12.0, 12.0, 0.0]);
            m[format!("{side}Port")] = json!([element("port", pf, pt, "#2"), element("portLED", lf, lt, "#3")]);
        }
        m
    }

    fn defaults() -> BTreeMap<String, String> {
        FACES.iter().map(|f| (f.to_string(), if *f == "north" { "output" } else { "input" }.to_string())).collect()
    }

    #[test]
    fn the_cube_loader_becomes_a_multipart_over_face_parts() {
        let tier = json!({ "parent": "mekanism:block/energy_cube/base",
                           "textures": { "corner": "mekanism:block/models/energy_cube_basic_corner" } });
        let item = json!({ "parent": "builtin/entity", "display": { "gui": { "rotation": [30, 315, 0] } } });
        let files: BTreeMap<String, Value> =
            cube_models("basic", &model(), &tier, Some(&item), &defaults()).unwrap().into_iter().collect();
        assert_eq!(files.len(), 18 + 3);
        let blockstate = &files["assets/mekanism/blockstates/basic_energy_cube.json"]["multipart"];
        assert_eq!(blockstate.as_array().unwrap().len(), 1 + 18);
        assert_eq!(blockstate[0]["apply"]["model"], "mekanism:block/energy_cube/basic_frame");
        assert_eq!(blockstate[18]["when"], json!({ "east": "output" }));
        let frame = &files["assets/mekanism/models/block/energy_cube/basic_frame.json"];
        assert_eq!(frame["textures"]["corner"], "mekanism:block/models/energy_cube_basic_corner");
        assert_eq!(frame["elements"][0]["light_emission"], 10, "NeoForge's block light");
        assert!(frame["elements"][0].get("name").is_none() && frame["elements"][0].get("neoforge_data").is_none());
        // Mekanism's left side sits on the east face of the unrotated model.
        let lit = &files["assets/mekanism/models/block/energy_cube/parts/east_output.json"]["elements"];
        assert_eq!(lit.as_array().unwrap().len(), 4);
        assert_eq!(lit[0]["faces"]["north"]["uv"], json!([0.0, 0, 0.5, 1]), "lit LEDs move 2 px left");
        assert_eq!(lit[0]["light_emission"], 15);
        assert!(lit[2].get("light_emission").is_none(), "the port body stays lit by the world");
        assert_eq!(lit[3]["light_emission"], 15, "the port's LED is full bright");
        let input = &files["assets/mekanism/models/block/energy_cube/parts/down_input.json"]["elements"];
        assert_eq!(input.as_array().unwrap().len(), 4);
        assert!(input.as_array().unwrap().iter().all(|e| e.get("light_emission").is_none()));
        let none = &files["assets/mekanism/models/block/energy_cube/parts/up_none.json"]["elements"];
        assert_eq!(none.as_array().unwrap().len(), 2);
        let item = &files["assets/mekanism/models/item/basic_energy_cube.json"];
        assert_eq!(item["elements"].as_array().unwrap().len(), 1 + 6 * 4);
        assert_eq!(item["display"]["gui"]["rotation"], json!([30, 315, 0]));
    }

    #[test]
    fn a_side_found_on_the_wrong_face_stops_the_build() {
        let mut m = model();
        let left = m["leftLEDs"].clone();
        m["leftLEDs"] = m["rightLEDs"].clone();
        m["rightLEDs"] = left;
        let err = cube_models("basic", &m, &json!({}), None, &defaults()).unwrap_err();
        assert_eq!(err, "left: found on west, expected east");
    }

    #[test]
    fn declarations_need_the_six_face_properties() {
        let mut decl = json!({ "state_properties": {}, "state_defaults": {} });
        for f in FACES {
            decl["state_properties"][f] = json!(["none", "input", "output"]);
            decl["state_defaults"][f] = json!("input");
        }
        assert_eq!(cube_defaults(&decl).unwrap()["east"], "input");
        decl["state_properties"]["up"] = json!(["none", "input"]);
        assert!(cube_defaults(&decl).is_err());
        // Horizon's own spelling: typed enums and `default_state`.
        let mut typed = json!({ "state_properties": {}, "default_state": {} });
        for f in FACES {
            typed["state_properties"][f] = json!({ "type": "enum", "values": ["none", "input", "output"] });
            typed["default_state"][f] = json!(if f == "north" { "output" } else { "input" });
        }
        assert_eq!(cube_defaults(&typed).unwrap()["north"], "output");
    }

    #[test]
    fn mek_data_recipes_become_shaped_and_drops_plain() {
        let recipe = json!({"type":"mekanism:mek_data","category":"misc",
            "key":{"A":{"tag":"mekanism:alloys/basic"},"E":{"item":"mekanism:energy_tablet"}},
            "pattern":["AEA"],"result":{"count":1,"id":"mekanism:basic_energy_cube"}});
        let shaped = shaped_recipe(&recipe).unwrap().unwrap();
        assert_eq!(shaped, json!({"type":"minecraft:crafting_shaped","category":"misc",
            "key":{"A":"#mekanism:alloys/basic","E":"mekanism:energy_tablet"},
            "pattern":["AEA"],"result":{"count":1,"id":"mekanism:basic_energy_cube"}}));
        assert_eq!(shaped_recipe(&shaped).unwrap(), None);
        let table = json!({"type":"minecraft:block","pools":[{"rolls":1.0,"entries":[{"type":"minecraft:item",
            "name":"mekanism:basic_energy_cube","modifier":[{"type":"minecraft:copy_name","source":"block_entity"}]}]}]});
        assert_eq!(plain_drop(&table)["pools"][0]["entries"][0],
            json!({"type":"minecraft:item","name":"mekanism:basic_energy_cube"}));
    }
}
