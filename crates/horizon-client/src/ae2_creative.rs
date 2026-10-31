//! Actual AE2 19.2.18 creative generator output from the pinned Java mod.
//! Component adaptation targets this native port; it is not a general Java
//! save importer. Derived AE2 rules: LGPL-3.0-or-later, AE2 contributors.
use crate::{items::ItemStack, save};
use horizon_world::nbt::{Compound, Tag};
use serde_json::Value;
use std::{collections::HashSet, path::Path};

pub(crate) struct CatalogTab {
    pub id: String,
    pub title: String,
    pub icon: ItemStack,
    pub items: Vec<ItemStack>,
}
/// Translate the source creative stacks into the native codecs used by the
/// machines, tools and renderer. Never expose a visually charged but inert item.
pub(crate) fn stack(source: &str) -> Result<ItemStack, String> {
    let mut stack = crate::item_data::parse_stack(source)
        .map_err(|e| e.to_string())?
        .ok_or("empty source creative stack")?;
    if stack.count != 1 {
        return Err("source creative stack must contain one item".into());
    }
    let nbt = save::item_to_nbt(&stack);
    let components = match nbt.get("components") {
        Some(Tag::Compound(values)) => values.clone(),
        _ => Compound::new(),
    };
    let mut custom = Compound::new();
    let mut contents = None;
    for (name, value) in components {
        let id = name.to_string_lossy();
        if !id.starts_with("ae2:") {
            continue;
        }
        stack.data_mut().set_other(&id, None);
        match id.as_ref() {
            "ae2:stored_energy" => {
                let key = if matches!(
                    stack.item.as_ref(),
                    "ae2:wireless_terminal" | "ae2:wireless_crafting_terminal"
                ) {
                    "ae2_wireless_energy"
                } else if stack.item.starts_with("ae2:portable_") {
                    "ae2_portable_energy"
                } else {
                    "ae2:stored_energy"
                };
                let value = if key=="ae2_wireless_energy" {
                    Tag::Double(crate::ae2_common_config::wireless().terminal_capacity.max(0.0).to_bits())
                } else { value };
                custom.insert(key.into(), value);
            }
            "ae2:facade_item" => {
                stack.data_mut().set_other("ae2:facade_item",Some(value.clone()));
                custom.insert(name, value);
            }
            "ae2:energy_capacity" => {
                custom.insert(name, value);
            }
            "ae2:upgrades" => {
                let Tag::List(cards) = value else {
                    return Err("invalid source creative upgrades".into());
                };
                let mut native = Vec::new();
                for card in cards {
                    let item = card
                        .get("item")
                        .and_then(save::item_from_nbt)
                        .ok_or("invalid source creative upgrade item")?;
                    let Some(Tag::Int(slot)) = card.get("slot") else {
                        return Err("invalid source upgrade position".into());
                    };
                    let Tag::Compound(mut tag) = save::item_to_nbt(&item) else {
                        return Err("invalid upgrade compound".into());
                    };
                    tag.insert("Slot".into(), Tag::Int(*slot));
                    native.push(Tag::Compound(tag));
                }
                custom.insert(
                    "ae2_cell_settings".into(),
                    Tag::compound([
                        ("upgrades", Tag::List(native)),
                        ("filters", Tag::List(Vec::new())),
                    ]),
                );
            }
            "ae2:storage_cell_inv" => {
                contents = Some(value);
            }
            _ => return Err(format!("unsupported source creative component {id}")),
        }
    }
    // The Java snapshot was exported with default config. Its charged variants
    // are generated from the actual configured supplier when creative rebuilds.
    let battery=crate::ae2_common_config::batteries();
    let base=match stack.item.as_ref() {
        "ae2:charged_staff"=>Some((8000.0,battery.charged_staff)),
        "ae2:entropy_manipulator"=>Some((200000.0,battery.entropy_manipulator)),
        "ae2:matter_cannon"=>Some((200000.0,battery.matter_cannon)),
        "ae2:color_applicator"=>Some((20000.0,battery.color_applicator)),
        id if id.starts_with("ae2:portable_")=>Some((20000.0,battery.portable_cell)),
        _=>None,
    };
    if let Some((source,configured))=base {
        for key in ["ae2:stored_energy","ae2_portable_energy","ae2:energy_capacity"] {
            if let Some(Tag::Double(bits))=custom.get_mut(&key.into()) {*bits=(f64::from_bits(*bits)/source*configured).to_bits();}
        }
    }
    if !custom.is_empty() {
        stack
            .data_mut()
            .set_other("minecraft:custom_data", Some(Tag::Compound(custom)));
    }
    if let Some(contents) = contents {
        let Tag::List(entries) = contents else {
            return Err("invalid source creative cell contents".into());
        };
        let mut cell = crate::ae2_cell_codec::read_cell(&stack)?;
        let mut seen = HashSet::new();
        for entry in entries {
            if entry.get("#t") != Some(&Tag::String("ae2:i".into())) {
                return Err("non-item source creative cell key".into());
            }
            let Some(Tag::String(id)) = entry.get("id") else {
                return Err("missing source cell item".into());
            };
            let Some(Tag::Long(count)) = entry.get("#") else {
                return Err("missing source cell count".into());
            };
            if *count <= 0 || !seen.insert(id.clone()) {
                return Err("invalid or repeated source creative cell entry".into());
            }
            let item = ItemStack::new(id.to_string_lossy().as_ref(), 1);
            let key = crate::ae2_cell_codec::key(&item)?;
            if cell.insert(&key, *count as u64, ae2_cell_core::Action::Modulate) != *count as u64 {
                return Err("source creative contents exceed native capacity".into());
            }
        }
        crate::ae2_cell_codec::write_cell(&mut stack, &cell)?;
    }
    Ok(stack)
}
pub(crate) fn tabs(root: &Path) -> crate::Result<Vec<CatalogTab>> {
    let catalog: Value = serde_json::from_str(include_str!("ae2_creative_catalog.json"))?;
    let registered: Value = serde_json::from_slice(&std::fs::read(root.join("items.json"))?)?;
    let debug = crate::ae2_common_config::unsupported_developer_tools();
    let mut result = Vec::new();
    for tab in catalog["normal"]
        .as_array()
        .ok_or("invalid source creative catalog")?
    {
        let id = tab["id"].as_str().ok_or("missing source tab id")?;
        let entries = if id == "ae2:main" && debug {
            &catalog["debug_main"]
        } else {
            &tab["items"]
        };
        let mut items = Vec::new();
        for source in entries.as_array().ok_or("invalid source tab stacks")? {
            let item = stack(source.as_str().ok_or("invalid source stack string")?)
                .map_err(|e| format!("AE2 creative {id}: {e}"))?;
            if registered.get(item.item.as_ref()).is_some() {
                items.push(item);
            }
        }
        let icon = if id == "ae2:facades" {
            items
                .first()
                .cloned()
                .unwrap_or_else(|| ItemStack::new("minecraft:cake", 1))
        } else {
            ItemStack::new("ae2:controller", 1)
        };
        result.push(CatalogTab {
            id: id.into(),
            title: tab["title"].as_str().unwrap_or(id).into(),
            icon,
            items,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_creative_load_exposes_source_tabs_search_external_tools_and_component_icons() {
        let root =
            std::env::temp_dir().join(format!("ae2-creative-runtime-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let source: Value =
            serde_json::from_str(include_str!("ae2_creative_catalog.json")).unwrap();
        let mut registered = serde_json::Map::new();
        for tab in source["normal"].as_array().unwrap() {
            for raw in tab["items"].as_array().unwrap() {
                let item = stack(raw.as_str().unwrap()).unwrap();
                registered.insert(item.item.to_string(), serde_json::json!({}));
            }
        }
        std::fs::write(
            root.join("items.json"),
            Value::Object(registered).to_string(),
        )
        .unwrap();
        std::fs::write(root.join("horizon-mod-content.json"), "[\"ae2\"]").unwrap();
        let mut tabs = Vec::new();
        for (top, ids) in [
            (
                true,
                vec![
                    "building_blocks",
                    "colored_blocks",
                    "natural_blocks",
                    "functional_blocks",
                    "redstone_blocks",
                    "hotbar",
                    "search",
                ],
            ),
            (
                false,
                vec![
                    "tools_and_utilities",
                    "combat",
                    "food_and_drinks",
                    "ingredients",
                    "spawn_eggs",
                    "op_blocks",
                    "inventory",
                ],
            ),
        ] {
            for (column, id) in ids.iter().enumerate() {
                tabs.push(serde_json::json!({"id":format!("minecraft:{id}"),"title":id,"type":match *id{"search"=>"SEARCH","inventory"=>"INVENTORY","hotbar"=>"HOTBAR",_=>"CATEGORY"},"visible":*id!="op_blocks","row":if top{"TOP"}else{"BOTTOM"},"column":column,"icon":"{id:\"minecraft:stone\",count:1}","items":[]}));
            }
        }
        std::fs::write(
            root.join("creative-inventory.json"),
            serde_json::json!({"tabs":tabs}).to_string(),
        )
        .unwrap();
        let creative = crate::creative::Creative::load(&root).unwrap();
        let main = creative.tabs.iter().find(|t| t.id == "ae2:main").unwrap();
        assert!(
            main.items
                .iter()
                .any(|s| s.item.as_ref() == "ae2:controller")
        );
        assert!(
            !main
                .items
                .iter()
                .any(|s| s.item.as_ref() == "ae2:matrix_frame")
        );
        let facades = creative
            .tabs
            .iter()
            .find(|t| t.id == "ae2:facades")
            .unwrap();
        assert!(
            facades
                .icon_stack
                .as_ref()
                .unwrap()
                .data()
                .unwrap()
                .other("minecraft:custom_data")
                .is_some()
        );
        assert!(
            creative
                .tabs
                .iter()
                .find(|t| t.id == "minecraft:tools_and_utilities")
                .unwrap()
                .items
                .iter()
                .any(|s| s.item.as_ref() == "ae2:certus_quartz_pickaxe")
        );
        let names = crate::creative::Names::default();
        let oak_facade=creative.search_results("ae2:facade - oak log",&names);
        assert_eq!(oak_facade.len(),1);
        assert_eq!(creative.search_items()[oak_facade[0]].item.as_ref(),"ae2:facade");
        assert_eq!(
            creative.search_results("ae2:facade", &names).len(),
            facades.items.len()
        );
        assert!(
            creative
                .search_results("ae2:controller", &names)
                .iter()
                .any(|i| creative.search_items()[*i].item.as_ref() == "ae2:controller")
        );
        let positions = creative
            .tabs
            .iter()
            .map(|t| (t.top, t.column))
            .collect::<HashSet<_>>();
        assert_eq!(positions.len(), creative.tabs.len());
        assert!(positions.iter().all(|(_, c)| *c < 7));
    }
    #[test]
    fn actual_java_catalog_variants_have_native_power_cells_enchantments_and_facades() {
        let data: Value = serde_json::from_str(include_str!("ae2_creative_catalog.json")).unwrap();
        let mut full_applicator = false;
        let mut facade = false;
        let mut silk = false;
        let mut charged = false;
        for tab in data["normal"].as_array().unwrap() {
            for raw in tab["items"].as_array().unwrap() {
                let item = stack(raw.as_str().unwrap()).unwrap();
                if item.item.as_ref() == "ae2:color_applicator" {
                    let cell = crate::ae2_cell_codec::read_cell(&item).unwrap();
                    let total: u64 = cell.entries().map(|(_, n)| *n).sum();
                    if total > 0 {
                        assert_eq!(total, 17 * 128);
                        assert_eq!(cell.entries().count(), 17);
                        assert_eq!(
                            crate::ae2_powered_items::energy(&item).unwrap(),
                            (340000.0, 340000.0)
                        );
                        full_applicator = true;
                    }
                }
                if item.item.as_ref() == "ae2:charged_staff"
                    && item
                        .data()
                        .and_then(|d| d.other("minecraft:custom_data"))
                        .is_some()
                {
                    assert_eq!(
                        crate::ae2_powered_items::energy(&item).unwrap(),
                        (8000.0, 8000.0)
                    );
                    charged = true;
                }
                if item.item.as_ref() == "ae2:facade" {
                    assert!(matches!(
                        item.data()
                            .unwrap()
                            .other("minecraft:custom_data")
                            .and_then(|t| t.get("ae2:facade_item")),
                        Some(Tag::String(_))
                    ));
                    facade = true;
                }
                if item.item.as_ref() == "ae2:annihilation_plane"
                    && item.data().is_some_and(|d| !d.enchantments.is_empty())
                {
                    assert!(
                        item.data()
                            .unwrap()
                            .enchantments
                            .iter()
                            .any(|(id, n)| id.as_ref() == "minecraft:silk_touch" && *n == 1)
                    );
                    silk = true;
                }
            }
        }
        assert!(full_applicator && facade && silk && charged);
    }
}
