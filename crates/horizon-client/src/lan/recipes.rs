//! ClientboundUpdateRecipesPacket: what RecipeManager.finalizeRecipeLoading
//! synchronizes, the recipe property sets (the items each furnace, smithing
//! table slot and brewing stand slot accepts) and the stonecutter's recipes,
//! read from the data pack's recipe files.
use super::data::SyncData;
use horizon_protocol::buf::Writer;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// RecipeManager.RECIPE_PROPERTY_SETS: each set and the ingredients it takes
/// from a recipe of the given type.
const PROPERTY_SETS: &[(&str, &str, &[&str])] = &[
    ("smithing_addition", "smithing", &["addition"]),
    ("smithing_base", "smithing", &["base"]),
    ("smithing_template", "smithing", &["template"]),
    ("furnace_input", "minecraft:smelting", &["ingredient"]),
    ("blast_furnace_input", "minecraft:blasting", &["ingredient"]),
    ("smoker_input", "minecraft:smoking", &["ingredient"]),
    (
        "campfire_input",
        "minecraft:campfire_cooking",
        &["ingredient"],
    ),
    ("brewing_input", "minecraft:brewing", &["input"]),
    ("brewing_reagent", "minecraft:brewing", &["reagent"]),
];

/// An ingredient as its HolderSet: a tag, or the items listed.
enum Ingredient {
    Tag(String),
    Items(Vec<String>),
}

fn ingredient(v: &Value) -> Option<Ingredient> {
    match v {
        Value::String(s) => Some(match s.strip_prefix('#') {
            Some(tag) => Ingredient::Tag(tag.to_owned()),
            None => Ingredient::Items(vec![s.clone()]),
        }),
        Value::Array(a) => Some(Ingredient::Items(
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect(),
        )),
        // Source 1.21 item/tag ingredients (AE2), and brewing predicates.
        Value::Object(o) => {
            if let Some(tag)=o.get("tag").and_then(Value::as_str) {Some(Ingredient::Tag(tag.to_owned()))}
            else {ingredient(o.get("item")?)}
        },
        _ => None,
    }
}

/// Every enabled recipe, with complete resource locations.
pub(super) fn resource_recipes(resources: &Path) -> BTreeMap<String, Value> {
    fn collect(dir: &Path, prefix: &str, namespace: &str, out: &mut BTreeMap<String, Value>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = std::fs::symlink_metadata(&path).map(|m|m.file_type()) else {continue;};
            if kind.is_dir() { collect(&path, &format!("{prefix}{name}/"), namespace, out); }
            else if (kind.is_file() || kind.is_symlink()) && name.ends_with(".json") {
                if let Ok(v) = serde_json::from_slice(&std::fs::read(&path).unwrap_or_default()) {
                    out.insert(format!("{namespace}:{prefix}{}",name.trim_end_matches(".json")),v);
                }
            }
        }
    }
    let mut namespaces=vec!["minecraft".to_owned()];namespaces.extend(crate::mod_content::namespaces(resources));
    namespaces.sort();namespaces.dedup();
    let mut all=BTreeMap::new();
    for namespace in namespaces {collect(&resources.join("data").join(&namespace).join("recipe"),"",&namespace,&mut all);}
    let tags=crate::items::load_tags(resources,"item",&horizon_world::loot::PackOverlay::NONE).unwrap_or_default();
    all.retain(|_,v|crate::recipe_conditions::enabled(v,&tags));
    all
}

/// The packs' recipe files over the game's own: the top pack's file for
/// each id (`SimpleJsonResourceReloadListener`).
pub(super) fn overlay_recipes(
    overlay: &horizon_world::loot::PackOverlay,
    out: &mut BTreeMap<String, Value>,
) {
    for (id, files) in overlay.kind("recipe") {
        if let Ok(v) = serde_json::from_slice::<Value>(horizon_world::loot::PackOverlay::top(files))
        {
            out.insert(id.to_owned(), v);
        }
    }
}

/// The packet's payload for the vanilla data pack in `resources`.
pub fn update_recipes(
    resources: &Path,
    sync: &SyncData,
    overlay: &horizon_world::loot::PackOverlay,
) -> Vec<u8> {
    let mut all = resource_recipes(resources);
    overlay_recipes(overlay, &mut all);
    let tags: BTreeMap<&str, &[i32]> = sync
        .tags
        .registries
        .iter()
        .filter(|r| r.registry == "minecraft:item")
        .flat_map(|r| r.tags.iter())
        .map(|t| (t.name.trim_start_matches("minecraft:"), t.ids.as_slice()))
        .collect();
    let item = |name: &str| sync.id("minecraft:item", name).unwrap_or(0);
    let items = |i: &Ingredient| -> Vec<i32> {
        match i {
            Ingredient::Tag(t) => tags
                .get(t.trim_start_matches("minecraft:"))
                .map(|ids| ids.to_vec())
                .unwrap_or_default(),
            Ingredient::Items(list) => list.iter().map(|n| item(n)).collect(),
        }
    };
    let mut w = Writer::new();
    w.varint(PROPERTY_SETS.len() as i32);
    for (key, kind, fields) in PROPERTY_SETS {
        let mut set = BTreeSet::new();
        for recipe in all.values() {
            let t = recipe["type"].as_str().unwrap_or("");
            let matches = if *kind == "smithing" {
                t.starts_with("minecraft:smithing_")
            } else {
                t == *kind
            };
            if matches {
                for field in *fields {
                    if let Some(i) = ingredient(&recipe[*field]) {
                        set.extend(items(&i));
                    }
                }
            }
        }
        w.string(&format!("minecraft:{key}"));
        w.varint(set.len() as i32);
        for id in set {
            w.varint(id);
        }
    }
    // SelectableRecipe.SingleInputSet: each stonecutting recipe's input and
    // its result as an ItemStackSlotDisplay.
    let stonecutting: Vec<&Value> = all
        .values()
        .filter(|r| r["type"] == "minecraft:stonecutting")
        .collect();
    let item_stack = sync
        .id("minecraft:slot_display", "minecraft:item_stack")
        .unwrap_or(0);
    w.varint(stonecutting.len() as i32);
    for r in stonecutting {
        match ingredient(&r["ingredient"]) {
            Some(Ingredient::Tag(t)) => {
                w.varint(0);
                w.string(&t);
            }
            Some(i) => {
                let ids = items(&i);
                w.varint(ids.len() as i32 + 1);
                for id in ids {
                    w.varint(id);
                }
            }
            None => w.varint(1),
        }
        w.varint(item_stack);
        w.varint(item(
            horizon_world::loot::recipe_result_id(&r["result"]).unwrap_or("minecraft:air"),
        ));
        w.varint(r["result"]["count"].as_i64().unwrap_or(1) as i32);
        w.varint(0); // no components added
        w.varint(0); // nor removed
    }
    w.data
}

#[cfg(test)] mod ae2_tests {
    use super::*;
    #[test] fn old_source_item_and_tag_ingredients_keep_their_namespace() {
        assert!(matches!(ingredient(&serde_json::json!({"tag":"c:ingots/iron"})),Some(Ingredient::Tag(t)) if t=="c:ingots/iron"));
        assert!(matches!(ingredient(&serde_json::json!({"item":"ae2:fluix_crystal"})),Some(Ingredient::Items(v)) if v==["ae2:fluix_crystal"]));
        assert!(matches!(ingredient(&serde_json::json!("#minecraft:planks")),Some(Ingredient::Tag(t)) if t=="minecraft:planks"));
        assert!(ingredient(&serde_json::json!({"unknown":"ae2:fluix_crystal"})).is_none());
    }
    #[test]
    #[ignore="needs merged native resources and pinned Java registry export"]
    fn ae2_synchronized_recipe_data_matches_actual_java_registry() {
        let root=std::env::var_os("AE2_NATIVE_RESOURCES").expect("AE2_NATIVE_RESOURCES");
        let oracle=std::env::var_os("AE2_RECIPE_ORACLE").expect("AE2_RECIPE_ORACLE");
        let rows:Value=serde_json::from_slice(&std::fs::read(oracle).unwrap()).unwrap();
        let expected:BTreeSet<_>=rows.as_array().unwrap().iter().map(|r|r["id"].as_str().unwrap().to_owned()).collect();
        let actual:BTreeSet<_>=resource_recipes(Path::new(&root)).into_keys().filter(|id|id.starts_with("ae2:")).collect();
        assert_eq!(actual,expected);
    }
}
