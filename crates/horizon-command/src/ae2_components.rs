// Derived from AE2 AEComponents, db17504a86128fdf3dae31f5fb7a112a646e0b93.
// Copyright AlgorithmX2 and AE2 contributors. LGPL-3.0-or-later; see root LICENSE.md.
//! Persistent source component identifiers and command-codec validation.
pub const PERSISTENT_IDS: &[&str] = &[
    "ae2:exported_settings_source",
    "ae2:exported_custom_name",
    "ae2:exported_upgrades",
    "ae2:exported_settings",
    "ae2:exported_priority",
    "ae2:exported_p2p_type",
    "ae2:exported_p2p_frequency",
    "ae2:memory_card_colors",
    "ae2:exported_config_inv",
    "ae2:exported_level_emitter_value",
    "ae2:exported_patterns",
    "ae2:exported_push_direction",
    "ae2:name_press_name",
    "ae2:upgrades",
    "ae2:entangled_singularity_id",
    "ae2:stored_energy",
    "ae2:energy_capacity",
    "ae2:encoded_crafting_pattern",
    "ae2:encoded_processing_pattern",
    "ae2:encoded_stonecutting_pattern",
    "ae2:encoded_smithing_table_pattern",
    "ae2:enabled_key_types",
    "ae2:wireless_link_target",
    "ae2:selected_color",
    "ae2:storage_cell_fuzzy_mode",
    "ae2:storage_cell_inv",
    "ae2:storage_cell_config_inv",
    "ae2:facade_item",
    "ae2:facade_cycle_property",
    "ae2:wrapped_stack",
    "ae2:crafting_inv",
    "ae2:spatial_plot_info",
    "ae2:missing_content_itemstack_data",
    "ae2:missing_content_aekey_data",
    "ae2:missing_content_error",
];
pub fn is_known(id: &str) -> bool {
    PERSISTENT_IDS.contains(&id)
}

use horizon_world::nbt::{Compound, Tag};

pub fn register(reg: &mut crate::args::Registries) {
    reg.persistent_components
        .extend(PERSISTENT_IDS.iter().map(|id| (*id).to_owned()));
    reg.persistent_components.sort();
    reg.persistent_components.dedup();
    let ids = &mut reg
        .by_registry
        .entry("minecraft:data_component_type".into())
        .or_default()
        .ids;
    ids.extend(PERSISTENT_IDS.iter().map(|id| (*id).to_owned()));
    ids.sort();
    ids.dedup();
}
fn integer(t: &Tag) -> Result<i32, String> {
    Ok(match t {
        Tag::Float(v) => f32::from_bits(*v) as i32,
        Tag::Double(v) => f64::from_bits(*v) as i32,
        _ => number(t)? as i32,
    })
}

/// Decode with the pinned source's NBT primitive conversions before storing
/// component identity. Java codecs narrow Number values, rather than requiring
/// the literal tag suffix to match the component's destination type.
pub fn validate(id: &str, value: &Tag) -> Result<(), String> {
    normalize(id, value.clone()).map(|_| ())
}
pub fn normalize(id: &str, value: Tag) -> Result<Tag, String> {
    decode(id, value, 0)
}
/// Registry-sensitive command boundary. Fault-tolerant pattern/key records
/// recover missing items in gameplay; strict item records must reject them.
pub fn validate_registered(
    id: &str,
    value: &Tag,
    reg: &crate::args::Registries,
) -> Result<(), String> {
    let value = normalize(id, value.clone())?;
    fn stack(t: &Tag, reg: &crate::args::Registries, depth: usize) -> Result<(), String> {
        if depth > 64 {
            return Err("AE2 component nesting exceeds 64".into());
        }
        let id = t.get("id").ok_or("Missing item ID")?;
        let id = string(id)?;
        if id == "minecraft:air" || !reg.ids("minecraft:item").contains(&id) {
            return Err(format!("Unknown or empty item: {id}"));
        }
        if let Some(Tag::Compound(components)) = t.get("components") {
            for (key, value) in components {
                let key = key.to_string_lossy();
                let removed = key.starts_with('!');
                let key = key.strip_prefix('!').unwrap_or(&key);
                if !reg.persistent_components.iter().any(|id| id == key) {
                    return Err(format!("Unknown item component: {key}"));
                }
                if !removed && is_known(key) {
                    check(key, value, reg, depth + 1)?;
                }
            }
        }
        Ok(())
    }
    fn check(
        id: &str,
        t: &Tag,
        reg: &crate::args::Registries,
        depth: usize,
    ) -> Result<(), String> {
        match id {
            "ae2:facade_item" | "ae2:exported_p2p_type" => {
                let id = string(t)?;
                if !reg.ids("minecraft:item").contains(&id) {
                    return Err(format!("Unknown item: {id}"));
                }
            }
            "ae2:exported_upgrades" => {
                for item in list(t.clone())? {
                    stack(&item, reg, depth)?;
                }
            }
            "ae2:upgrades" | "ae2:crafting_inv" | "ae2:exported_patterns" => {
                for entry in list(t.clone())? {
                    stack(entry.get("item").ok_or("Missing item")?, reg, depth)?;
                }
            }
            "ae2:encoded_stonecutting_pattern" => {
                for key in ["input", "output"] {
                    stack(t.get(key).ok_or("Missing pattern item")?, reg, depth)?;
                }
            }
            "ae2:encoded_smithing_table_pattern" => {
                for key in ["template", "base", "addition", "resultItem"] {
                    stack(t.get(key).ok_or("Missing pattern item")?, reg, depth)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    check(id, &value, reg, 0)
}
fn number(t: &Tag) -> Result<i64, String> {
    Ok(match t {
        Tag::Byte(v) => i64::from(*v),
        Tag::Short(v) => i64::from(*v),
        Tag::Int(v) => i64::from(*v),
        Tag::Long(v) => *v,
        Tag::Float(v) => f32::from_bits(*v) as i64,
        Tag::Double(v) => f64::from_bits(*v) as i64,
        _ => return Err("Not a number".into()),
    })
}
fn double(t: &Tag) -> Result<f64, String> {
    Ok(match t {
        Tag::Float(v) => f64::from(f32::from_bits(*v)),
        Tag::Double(v) => f64::from_bits(*v),
        _ => number(t)? as f64,
    })
}
fn string(t: &Tag) -> Result<String, String> {
    match t {
        Tag::String(s) => Ok(s.to_string_lossy()),
        _ => Err("Not a string".into()),
    }
}
fn resource(t: Tag) -> Result<Tag, String> {
    let s = string(&t)?;
    let s = crate::args::normalize(&s);
    let Some((namespace, path)) = s.split_once(':') else {
        unreachable!()
    };
    if namespace.is_empty()
        || path.is_empty()
        || !namespace
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
        || !path
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-/".contains(&b))
    {
        return Err(format!("Invalid resource location: {s}"));
    }
    Ok(Tag::String(s.as_str().into()))
}
fn map(t: Tag) -> Result<Compound, String> {
    if let Tag::Compound(m) = t {
        Ok(m)
    } else {
        Err("Not a map".into())
    }
}
fn list(t: Tag) -> Result<Vec<Tag>, String> {
    match t {
        Tag::List(v) => Ok(v),
        Tag::ByteArray(v) => Ok(v.into_iter().map(|n| Tag::Byte(n as i8)).collect()),
        Tag::IntArray(v) => Ok(v.into_iter().map(Tag::Int).collect()),
        Tag::LongArray(v) => Ok(v.into_iter().map(Tag::Long).collect()),
        _ => Err("Not a list".into()),
    }
}
fn take(m: &mut Compound, key: &str) -> Result<Tag, String> {
    m.remove(&key.into())
        .ok_or_else(|| format!("No key {key} in map"))
}
fn field(
    m: &mut Compound,
    key: &str,
    f: impl FnOnce(Tag) -> Result<Tag, String>,
) -> Result<(), String> {
    let value = f(take(m, key)?)?;
    m.insert(key.into(), value);
    Ok(())
}
fn boolean(t: Tag) -> Result<Tag, String> {
    Ok(Tag::Byte(i8::from(integer(&t)? as i8 != 0)))
}
const COLORS: &[&str] = &[
    "white",
    "light_gray",
    "gray",
    "black",
    "lime",
    "yellow",
    "orange",
    "brown",
    "red",
    "pink",
    "magenta",
    "purple",
    "blue",
    "light_blue",
    "cyan",
    "green",
    "fluix",
];
fn enum_value(t: Tag, values: &[&str]) -> Result<Tag, String> {
    let s = string(&t)?;
    if values.contains(&s.as_str()) {
        Ok(t)
    } else {
        Err(format!("Unknown element name:{s}"))
    }
}
fn position(t: Tag) -> Result<Tag, String> {
    let v = list(t)?;
    if v.len() != 3 {
        return Err("Expected 3 element position".into());
    }
    Ok(Tag::IntArray(
        v.iter().map(|t| integer(t)).collect::<Result<_, _>>()?,
    ))
}
fn record(mut m: Compound, keys: &[&str]) -> Tag {
    m.retain(|key, _| keys.contains(&key.to_string_lossy().as_str()));
    Tag::Compound(m)
}
fn patch(t: Tag, depth: usize) -> Result<Tag, String> {
    let mut out = Compound::new();
    for (key, mut value) in map(t)? {
        let key = key.to_string_lossy();
        let removed = key.starts_with('!');
        let raw = key.strip_prefix('!').unwrap_or(&key);
        let Tag::String(id) = resource(Tag::String(raw.into()))? else {
            unreachable!()
        };
        let id = id.to_string_lossy();
        if removed {
            if !matches!(&value,Tag::Compound(m) if m.is_empty()) {
                return Err("Expected empty component removal map".into());
            }
        } else if is_known(&id) {
            value = decode(&id, value, depth + 1)?;
        } else {
            crate::item::decode_component(&id, &value)?;
        }
        let key = if removed { format!("!{id}") } else { id };
        if out.insert(key.as_str().into(), value).is_some() {
            return Err(format!("Repeated component: {key}"));
        }
    }
    Ok(Tag::Compound(out))
}
fn item(t: Tag, optional: bool, depth: usize) -> Result<Tag, String> {
    let mut m = map(t)?;
    if optional && m.is_empty() {
        return Ok(Tag::Compound(m));
    }
    field(&mut m, "id", resource)?;
    let count = integer(&m.remove(&"count".into()).unwrap_or(Tag::Int(1)))?;
    if !(1..=99).contains(&count) {
        return Err(format!("Item count outside [1,99]: {count}"));
    }
    m.insert("count".into(), Tag::Int(count));
    if let Some(components) = m.remove(&"components".into()) {
        m.insert("components".into(), patch(components, depth)?);
    }
    Ok(record(m, &["id", "count", "components"]))
}
fn contents(t: Tag, depth: usize) -> Result<Tag, String> {
    let mut out = Vec::new();
    for entry in list(t)? {
        let mut m = map(entry)?;
        let slot = integer(&take(&mut m, "slot")?)?;
        if !(0..=255).contains(&slot) {
            return Err(format!("Slot outside [0,255]: {slot}"));
        }
        m.insert("slot".into(), Tag::Int(slot));
        field(&mut m, "item", |t| item(t, false, depth))?;
        out.push(record(m, &["slot", "item"]));
    }
    Ok(Tag::List(out))
}
fn generic(t: Tag, depth: usize) -> Result<Tag, String> {
    let mut m = map(t)?;
    // AEKey.MAP_CODEC recovers key errors as Missing Content, independently
    // of GenericStack's required numeric amount. Encoding that key restores
    // its original raw map; gameplay readers construct the typed placeholder.
    field(&mut m, "#", |t| Ok(Tag::Long(number(&t)?)))?;
    let original = m.clone();
    let key = (|| {
        field(&mut m, "#t", |t| enum_value(t, &["ae2:i", "ae2:f"]))?;
        field(&mut m, "id", resource)?;
        if let Some(components) = m.remove(&"components".into()) {
            m.insert("components".into(), patch(components, depth)?);
        }
        Ok::<_, String>(())
    })();
    if key.is_err() {
        return Ok(Tag::Compound(original));
    }
    // Registry lookup happens in the typed gameplay reader. An unknown key
    // must retain every original field for Missing Content recovery, so this
    // registry-independent command boundary cannot discard extra key data.
    Ok(Tag::Compound(m))
}
// The source fault-tolerant codecs keep unreadable payloads in Missing Content.
// Preserve those payloads byte-for-byte here; gameplay readers can recover them
// rather than a command rejecting an otherwise valid containing component.
fn tolerant_generics(t: Tag, nullable: bool, depth: usize) -> Result<Tag, String> {
    Ok(Tag::List(
        list(t)?
            .into_iter()
            .map(|t| {
                if nullable && matches!(&t,Tag::Compound(m) if m.is_empty()) {
                    t
                } else {
                    generic(t.clone(), depth).unwrap_or(t)
                }
            })
            .collect(),
    ))
}
fn decode(id: &str, t: Tag, depth: usize) -> Result<Tag, String> {
    if depth > 64 {
        return Err("AE2 component nesting exceeds 64".into());
    }
    match id {
        "ae2:exported_settings_source" | "ae2:exported_custom_name" | "ae2:name_press_name" => {
            crate::component::decode(&t)?;
            Ok(t)
        }
        "ae2:exported_priority" => Ok(Tag::Int(integer(&t)?)),
        "ae2:exported_p2p_frequency" => Ok(Tag::Short(integer(&t)? as i16)),
        "ae2:exported_level_emitter_value" | "ae2:entangled_singularity_id" => {
            Ok(Tag::Long(number(&t)?))
        }
        "ae2:stored_energy" | "ae2:energy_capacity" => Ok(Tag::Double(double(&t)?.to_bits())),
        "ae2:exported_p2p_type" | "ae2:facade_item" => resource(t),
        "ae2:facade_cycle_property" | "ae2:missing_content_error" => {
            string(&t)?;
            Ok(t)
        }
        "ae2:missing_content_itemstack_data" | "ae2:missing_content_aekey_data" => {
            map(t).map(Tag::Compound)
        }
        "ae2:selected_color" => enum_value(t, COLORS),
        "ae2:memory_card_colors" => {
            let colors = list(t)?
                .into_iter()
                .map(|t| enum_value(t, COLORS))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Tag::List(
                (0..8)
                    .map(|i| {
                        colors
                            .get(i)
                            .cloned()
                            .unwrap_or(Tag::String("fluix".into()))
                    })
                    .collect(),
            ))
        }
        "ae2:storage_cell_fuzzy_mode" => enum_value(
            t,
            &[
                "IGNORE_ALL",
                "PERCENT_99",
                "PERCENT_75",
                "PERCENT_50",
                "PERCENT_25",
            ],
        ),
        "ae2:exported_push_direction" => {
            enum_value(t, &["down", "up", "north", "south", "west", "east", "all"])
        }
        "ae2:enabled_key_types" => Ok(Tag::List(
            list(t)?
                .into_iter()
                .map(|t| enum_value(t, &["ae2:i", "ae2:f"]))
                .collect::<Result<_, _>>()?,
        )),
        "ae2:exported_settings" => {
            let m = map(t)?;
            for v in m.values() {
                string(v)?;
            }
            Ok(Tag::Compound(m))
        }
        "ae2:exported_upgrades" => Ok(Tag::List(
            list(t)?
                .into_iter()
                .map(|t| item(t, false, depth))
                .collect::<Result<_, _>>()?,
        )),
        "ae2:upgrades" | "ae2:crafting_inv" | "ae2:exported_patterns" => contents(t, depth),
        "ae2:storage_cell_inv" => tolerant_generics(t, false, depth),
        "ae2:storage_cell_config_inv" | "ae2:exported_config_inv" => {
            tolerant_generics(t, true, depth)
        }
        "ae2:wrapped_stack" => generic(t, depth),
        "ae2:wireless_link_target" => {
            let mut m = map(t)?;
            field(&mut m, "dimension", resource)?;
            field(&mut m, "pos", position)?;
            Ok(record(m, &["dimension", "pos"]))
        }
        "ae2:spatial_plot_info" => {
            let mut m = map(t)?;
            field(&mut m, "id", |t| Ok(Tag::Int(integer(&t)?)))?;
            field(&mut m, "size", position)?;
            Ok(record(m, &["id", "size"]))
        }
        "ae2:encoded_processing_pattern" => {
            let mut m = map(t)?;
            for k in ["sparseInputs", "sparseOutputs"] {
                field(&mut m, k, |t| tolerant_generics(t, true, depth))?;
            }
            Ok(record(m, &["sparseInputs", "sparseOutputs"]))
        }
        "ae2:encoded_crafting_pattern" => {
            let mut m = map(t)?;
            field(&mut m, "inputs", |t| {
                Ok(Tag::List(
                    list(t)?
                        .into_iter()
                        .map(|t| item(t.clone(), true, depth).unwrap_or(t))
                        .collect(),
                ))
            })?;
            field(&mut m, "result", |t| {
                Ok(item(t.clone(), false, depth).unwrap_or(t))
            })?;
            field(&mut m, "recipeId", resource)?;
            field(&mut m, "canSubstitute", boolean)?;
            field(&mut m, "canSubstituteFluids", boolean)?;
            Ok(record(
                m,
                &[
                    "inputs",
                    "result",
                    "recipeId",
                    "canSubstitute",
                    "canSubstituteFluids",
                ],
            ))
        }
        "ae2:encoded_stonecutting_pattern" | "ae2:encoded_smithing_table_pattern" => {
            let mut m = map(t)?;
            let keys: &[&str] = if id == "ae2:encoded_stonecutting_pattern" {
                &["input", "output"]
            } else {
                &["template", "base", "addition", "resultItem"]
            };
            for k in keys {
                field(&mut m, k, |t| item(t, false, depth))?;
            }
            field(&mut m, "recipeId", resource)?;
            field(&mut m, "canSubstitute", boolean)?;
            Ok(record(
                m,
                if id == "ae2:encoded_stonecutting_pattern" {
                    &["input", "output", "canSubstitute", "recipeId"]
                } else {
                    &[
                        "template",
                        "base",
                        "addition",
                        "resultItem",
                        "canSubstitute",
                        "recipeId",
                    ]
                },
            ))
        }
        _ => Err(format!("Unknown AE2 component: {id}")),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn snbt(s: &str) -> Tag {
        horizon_world::nbt::parse_snbt(s).unwrap().0
    }
    #[test]
    fn ae2_registered_item_commands_reject_missing_strict_content() {
        let mut reg = crate::args::Registries::default();
        register(&mut reg);
        reg.items = vec!["minecraft:stone".into(), "ae2:speed_card".into()];
        assert!(validate_registered("ae2:facade_item", &snbt("stone"), &reg).is_ok());
        assert!(validate_registered("ae2:facade_item", &snbt("removed:ore"), &reg).is_err());
        assert!(
            validate_registered(
                "ae2:upgrades",
                &snbt(r##"[{slot:0,item:{id:"ae2:speed_card"}}]"##),
                &reg
            )
            .is_ok()
        );
        assert!(
            validate_registered(
                "ae2:upgrades",
                &snbt(r##"[{slot:0,item:{id:"removed:card"}}]"##),
                &reg
            )
            .is_err()
        );
        assert!(
            validate_registered(
                "ae2:wrapped_stack",
                &snbt(r##"{id:"removed:ore","#":1}"##),
                &reg
            )
            .is_ok()
        );
    }
    #[test]
    fn ae2_numeric_components_have_typed_identity() {
        assert_eq!(
            normalize("ae2:entangled_singularity_id", Tag::Int(42)).unwrap(),
            Tag::Long(42)
        );
        assert_eq!(
            normalize("ae2:exported_p2p_frequency", Tag::Long(65535)).unwrap(),
            Tag::Short(-1)
        );
        assert_eq!(
            normalize("ae2:stored_energy", Tag::Int(-2)).unwrap(),
            Tag::Double((-2f64).to_bits())
        );
        assert_eq!(
            normalize("ae2:spatial_plot_info", snbt("{id:42b,size:[1,2,3]}")).unwrap(),
            snbt("{id:42,size:[I;1,2,3]}")
        );
    }
    #[test]
    fn ae2_nested_component_aliases_have_canonical_identity() {
        assert_eq!(
            normalize("ae2:facade_item", snbt("':stone'")).unwrap(),
            snbt("'minecraft:stone'")
        );
        let a = normalize(
            "ae2:upgrades",
            snbt(r##"[{slot:0,item:{id:stone,components:{damage:1}}}]"##),
        )
        .unwrap();
        let b=normalize("ae2:upgrades",snbt(r##"[{slot:0,item:{id:"minecraft:stone",count:1,components:{"minecraft:damage":1}}}]"##)).unwrap();
        assert_eq!(a, b);
        assert!(
            validate(
                "ae2:upgrades",
                &snbt(r##"[{slot:0,item:{id:stone,components:{damage:1,"minecraft:damage":2}}}]"##)
            )
            .is_err()
        );
    }
    #[test]
    fn ae2_component_records_canonicalize_and_reject_floating_overflow() {
        assert!(
            validate(
                "ae2:upgrades",
                &snbt("[{slot:4294967296.0d,item:{id:stone}}]")
            )
            .is_err()
        );
        assert!(
            validate(
                "ae2:exported_upgrades",
                &snbt("[{id:stone,count:4294967297.0d}]")
            )
            .is_err()
        );
        assert!(
            validate(
                "ae2:exported_upgrades",
                &snbt("[{id:stone,components:{\"minecraft:damage\":bad}}]")
            )
            .is_err()
        );
        assert_eq!(
            normalize(
                "ae2:spatial_plot_info",
                snbt("{id:7,size:[I;2,2,2],extra:1}")
            )
            .unwrap(),
            snbt("{id:7,size:[I;2,2,2]}")
        );
        assert!(validate("ae2:wrapped_stack", &snbt("{id:stone,\"#\":1}")).is_ok());
        assert!(validate("ae2:wrapped_stack", &snbt("{id:stone}")).is_err());
    }
    #[test]
    fn ae2_component_records_validate_source_shape_and_tolerant_lists() {
        assert!(
            validate(
                "ae2:encoded_processing_pattern",
                &snbt("{sparseInputs:[{},7],sparseOutputs:[]}")
            )
            .is_ok()
        );
        assert!(
            validate(
                "ae2:encoded_processing_pattern",
                &snbt("{sparseInputs:7,sparseOutputs:[]}")
            )
            .is_err()
        );
        assert!(
            validate(
                "ae2:wrapped_stack",
                &snbt("{\"#t\":\"ae2:f\",id:water,\"#\":1000}")
            )
            .is_ok()
        );
        assert!(validate("ae2:wrapped_stack", &snbt("{id:water,\"#\":1000}")).is_ok());
        assert!(validate("ae2:wrapped_stack", &snbt("{id:water}")).is_err());
        assert!(
            validate(
                "ae2:upgrades",
                &snbt("[{slot:0,item:{id:\"ae2:speed_card\",count:1}}]")
            )
            .is_ok()
        );
        assert!(validate("ae2:upgrades", &snbt("[{slot:256,item:{id:stone}}]")).is_err());
        assert!(validate("ae2:selected_color", &Tag::String("transparent".into())).is_err());
        assert_eq!(
            normalize("ae2:memory_card_colors", snbt("[red]")).unwrap(),
            snbt("[red,fluix,fluix,fluix,fluix,fluix,fluix,fluix]")
        );
        assert!(
            validate(
                "ae2:wireless_link_target",
                &snbt("{dimension:overworld,pos:[I;1,2,3]}")
            )
            .is_ok()
        );
        assert!(
            validate(
                "ae2:wireless_link_target",
                &snbt("{dimension:overworld,pos:[1,2]}")
            )
            .is_err()
        );
    }
}
