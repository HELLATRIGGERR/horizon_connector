//! Item stacks on the wire (`ItemStack.OPTIONAL_STREAM_CODEC`): the component
//! patch read back as vanilla's `components` compound, so the decoder the saved
//! and command item NBT use fills every field. Both sides use it: a client for
//! what its server sends, a server for a creative client's
//! `SetCreativeModeSlot`. The other direction is `lan::guest::stack_to_network`.
use crate::items::ItemStack;
use horizon_protocol::buf::{Codec, Reader};
use horizon_protocol::item::{Component, Profile, Stack, Template, component};
use horizon_protocol::registry::{Builtin, Dynamic};
use horizon_world::nbt::Tag;
use std::sync::Arc;

/// Components represented by the native item model but not yet carried by
/// this host's typed vanilla stream codecs. This envelope is used only between
/// Horizon peers; vanilla AE2 components remain the existing 35-ID list.
#[cfg(feature = "ae2-lab")]
pub(crate) fn enveloped_component(id:&str)->bool {
    crate::ae2_components::PERSISTENT_IDS.contains(&id)
        || matches!(id,"minecraft:lock"|"minecraft:container_loot"|"minecraft:bees"
            |"minecraft:block_state"|"minecraft:pot_decorations"|"minecraft:profile"
            |"minecraft:note_block_sound")
}

/// A stack from the wire: its item by the built-in registry's id, its
/// components by the synced registries' (enchantments, trims).
pub fn from_network(s: &Stack, builtin: &Builtin, synced: &Dynamic) -> ItemStack {
    let mut out = ItemStack {
        item: item_name(builtin, s.item),
        count: s.count.max(0) as u32,
        damage: 0,
        extra: None,
    };
    let components = components_tag(s, builtin, synced);
    let (damage, data) = crate::item_data::decode_components(components.as_ref());
    out.damage = damage;
    if let Some(data) = data {
        *out.data_mut() = *data;
    }
    #[cfg(feature = "ae2-lab")]
    {
        // v2 carries the complete persistent item patch. Native codecs remain
        // on the wire for rendering and compatibility; the original source
        // patch wins when their representation cannot carry every component.
        let key = "horizon:wire_ae2_components";
        let mut handled_v2 = false;
        if let Some(Tag::Compound(mut outer)) = out.data().and_then(|d| d.other("minecraft:custom_data")).cloned() {
            if let Some(Tag::Compound(mut wrapped)) = outer.get(&key.into()).cloned() {
                let known = wrapped.keys().all(|field| matches!(field.to_string_lossy().as_str(),
                    "version" | "had_custom_data" | "components" | "previous" | "noncompound_custom_data" | "ominous_layers_implicit"));
                let version = wrapped.remove(&"version".into());
                let had = wrapped.remove(&"had_custom_data".into());
                let components = wrapped.remove(&"components".into());
                let previous = wrapped.remove(&"previous".into());
                let noncompound = wrapped.remove(&"noncompound_custom_data".into());
                let implicit = wrapped.remove(&"ominous_layers_implicit".into());
                let valid_implicit = implicit.is_none() || implicit == Some(Tag::Byte(1));
                let valid_previous = previous.is_none() || had == Some(Tag::Byte(1));
                let valid_noncompound = noncompound.as_ref().is_none_or(|v| !matches!(v, Tag::Compound(_)))
                    && (noncompound.is_none() || had == Some(Tag::Byte(0)));
                if known && version == Some(Tag::Byte(2)) && matches!(had, Some(Tag::Byte(0 | 1)))
                    && valid_implicit && valid_previous && valid_noncompound {
                    if let Some(Tag::Compound(mut patch)) = components
                        .filter(|v| matches!(v, Tag::Compound(m) if !m.contains_key(&"minecraft:custom_data".into()))) {
                        outer.remove(&key.into());
                        if let Some(previous) = previous { outer.insert(key.into(), previous); }
                        if had == Some(Tag::Byte(1)) || !outer.is_empty() {
                            patch.insert("minecraft:custom_data".into(), Tag::Compound(outer));
                        } else if let Some(value) = noncompound {
                            patch.insert("minecraft:custom_data".into(), value);
                        }
                        let transient = out.data().and_then(|d| d.map_post_processing);
                        let (damage, data) = crate::item_data::decode_components(Some(&Tag::Compound(patch)));
                        out.damage = damage;
                        out.extra = data;
                        if let Some(transient) = transient { out.data_mut().map_post_processing = Some(transient); }
                        if implicit == Some(Tag::Byte(1)) && out.data().is_some_and(|d| d.ominous_banner) {
                            out.data_mut().banner_patterns.clear();
                        }
                        handled_v2 = true;
                    }
                }
            }
        }
        if !handled_v2 {
        let key = "horizon:wire_ae2_components";
        if let Some(Tag::Compound(mut envelope)) = out.data().and_then(|data| data.other("minecraft:custom_data")).cloned() {
            if let Some(Tag::Compound(mut wrapped)) = envelope.get(&key.into()).cloned() {
                let known_fields = wrapped.keys().all(|field| matches!(field.to_string_lossy().as_str(), "version" | "had_custom_data" | "components" | "previous"));
                let components = wrapped.remove(&"components".into());
                let had_custom_data = wrapped.remove(&"had_custom_data".into());
                if known_fields && wrapped.remove(&"version".into()) == Some(Tag::Byte(1)) && matches!(&had_custom_data, Some(Tag::Byte(0 | 1))) {
                    if let Some(Tag::Compound(components)) = components.filter(|value| {
                        matches!(value, Tag::Compound(map) if map.keys().all(|id| enveloped_component(&id.to_string_lossy())))
                    }) {
                        envelope.remove(&key.into());
                        if let Some(previous) = wrapped.remove(&"previous".into()) {
                            envelope.insert(key.into(), previous);
                        }
                        let data = out.data_mut();
                        data.set_other("minecraft:custom_data", (had_custom_data == Some(Tag::Byte(1)) || !envelope.is_empty()).then_some(Tag::Compound(envelope)));
                        for (id, value) in components {
                            let id=id.to_string_lossy();
                            if id=="minecraft:pot_decorations" {
                                // Pot decorations have a typed StackData field;
                                // use the existing persistent codec to reconstruct it.
                                let tag=Tag::compound([("minecraft:pot_decorations",value)]);
                                let (_,parsed)=crate::item_data::decode_components(Some(&tag));
                                data.pot_decorations=parsed.and_then(|value|value.pot_decorations);
                            } else {
                                data.set_other(&id, Some(value));
                            }
                        }
                    }
                }
            }
        }
        }
    }
    out.tidy();
    // What it decoded to, to send the server's own patch back if that holds.
    if let Some(patch) = &s.wire.0 {
        let decoded = out.clone();
        out.data_mut().wire = crate::item_data::Wire::new(decoded, patch.clone());
    }
    out
}

/// `DyeColor.byId`: an unknown id is white (`OutOfBoundsStrategy.ZERO`).
fn dye_name(id: i32) -> &'static str {
    usize::try_from(id)
        .ok()
        .and_then(|i| crate::block_entity_render::DYES.get(i))
        .map_or("white", |d| d.0)
}

fn item_name(builtin: &Builtin, id: i32) -> Arc<str> {
    Arc::from(
        builtin
            .get("minecraft:item")
            .and_then(|i| i.name(id))
            .unwrap_or("minecraft:air"),
    )
}

/// The item's patched components as vanilla's `components` compound.
fn components_tag(s: &Stack, builtin: &Builtin, synced: &Dynamic) -> Option<Tag> {
    let name = |id: i32| builtin.get("minecraft:data_component_type")?.name(id);
    let mut entries: Vec<(&str, Tag)> = Vec::new();
    for (kind, c) in &s.added {
        let Some(key) = name(*kind) else {
            continue;
        };
        let value = match c {
            // `ItemContainerContents`: a shulker box's items, for its tooltip.
            Component::Raw(bytes) if *kind == component::CONTAINER => {
                let Some(slots) = container_tag(bytes, builtin, synced) else {
                    continue;
                };
                slots
            }
            // `TypedEntityData`: a block item's block entity, which the op
            // warning and a spawner's tooltip read.
            Component::Raw(bytes) if *kind == component::BLOCK_ENTITY_DATA => {
                let Some(data) = block_entity_data_tag(bytes, builtin) else {
                    continue;
                };
                data
            }
            // Read past, not decoded: the item keeps its prototype's value.
            Component::Raw(_) => continue,
            // Trim materials and patterns are synced registries, as enchantments are.
            Component::Trim { material, pattern } => {
                let (Some(material), Some(pattern)) = (
                    synced.name("minecraft:trim_material", *material),
                    synced.name("minecraft:trim_pattern", *pattern),
                ) else {
                    continue;
                };
                Tag::compound([
                    ("material", Tag::String(material.into())),
                    ("pattern", Tag::String(pattern.into())),
                ])
            }
            // Instruments are a synced registry: a goat horn names its own.
            Component::Instrument(id) => match synced.name("minecraft:instrument", *id) {
                Some(name) => Tag::String(name.into()),
                None => continue,
            },
            // BannerPatternLayers: each layer's pattern is a synced registry entry.
            Component::BannerPatterns(layers) => {
                let layers: Option<Vec<Tag>> = layers
                    .iter()
                    .map(|(pattern, color)| {
                        let pattern = synced.name("minecraft:banner_pattern", *pattern)?;
                        Some(Tag::compound([
                            ("pattern", Tag::String(pattern.into())),
                            ("color", Tag::String(dye_name(*color).into())),
                        ]))
                    })
                    .collect();
                let Some(layers) = layers else {
                    continue;
                };
                Tag::List(layers)
            }
            // `painting/variant`: a synced registry entry, as trims are.
            Component::PaintingVariant(id) => {
                let Some(name) = synced.name("minecraft:painting_variant", *id) else {
                    continue;
                };
                Tag::String(name.into())
            }
            // A shield's `base_color`: the dye's id.
            Component::Int(v) if *kind == component::BASE_COLOR => Tag::String(dye_name(*v).into()),
            Component::Int(v) if *kind == component::RARITY => {
                let rarity = ["common", "uncommon", "rare", "epic"];
                Tag::String(rarity.get(*v as usize).copied().unwrap_or("common").into())
            }
            Component::Int(v) => Tag::Int(*v),
            Component::Unit => Tag::Compound(Default::default()),
            Component::Bool(v) => Tag::Byte(i8::from(*v)),
            Component::Text(t) | Component::Nbt(t) => t.clone(),
            Component::Ident(v) => Tag::String(v.as_str().into()),
            Component::Lore(lines) => Tag::List(lines.clone()),
            Component::WritableBook { pages, .. } => Tag::compound([(
                "pages",
                Tag::List(
                    pages
                        .iter()
                        .map(|p| Tag::String(p.as_str().into()))
                        .collect(),
                ),
            )]),
            Component::WrittenBook(b) => Tag::compound([
                ("title", Tag::String(b.title.as_str().into())),
                ("author", Tag::String(b.author.as_str().into())),
                ("generation", Tag::Int(b.generation)),
                ("pages", Tag::List(b.pages.clone())),
                ("resolved", Tag::Byte(i8::from(b.resolved))),
            ]),
            Component::Fireworks { flight, explosions } => Tag::compound([
                ("flight_duration", Tag::Byte(*flight as i8)),
                (
                    "explosions",
                    Tag::List(explosions.iter().map(fireworks_tag).collect()),
                ),
            ]),
            Component::FireworkExplosion(e) => fireworks_tag(e),
            Component::LodestoneTracker { target, tracked } => {
                let mut entries = Vec::new();
                if let Some((dimension, pos)) = target {
                    entries.push((
                        "target",
                        Tag::compound([
                            ("dimension", Tag::String(dimension.as_str().into())),
                            ("pos", Tag::IntArray(pos.to_vec())),
                        ]),
                    ));
                }
                if !tracked {
                    entries.push(("tracked", Tag::Byte(0)));
                }
                Tag::compound(entries)
            }
            Component::Enchantments(list) => {
                // Enchantments are a synced (data-driven) registry: the
                // ids index the server's RegistryData, not the builtin
                // report, so a datapack enchantment names correctly.
                let levels = list.iter().filter_map(|(id, level)| {
                    let name = synced.name("minecraft:enchantment", *id)?;
                    Some((name, Tag::Int(*level)))
                });
                Tag::compound(levels)
            }
            Component::AttackRange(v) => Tag::compound([
                ("min_reach", Tag::Float(v[0].to_bits())),
                ("max_reach", Tag::Float(v[1].to_bits())),
                ("min_attack_momentum", Tag::Float(v[2].to_bits())),
                ("max_attack_momentum", Tag::Float(v[3].to_bits())),
                ("hitbox_margin", Tag::Float(v[4].to_bits())),
                ("mob_factor", Tag::Float(v[5].to_bits())),
            ]),
            Component::Profile(p) => profile_tag(p),
            // SuspiciousStewEffects' NBT: each effect by name and its duration.
            Component::StewEffects(list) => Tag::List(
                list.iter()
                    .filter_map(|(id, duration)| {
                        let name = builtin.get("minecraft:mob_effect")?.name(*id)?;
                        Some(Tag::compound([
                            ("id", Tag::String(name.into())),
                            ("duration", Tag::Int(*duration)),
                        ]))
                    })
                    .collect(),
            ),
            // PotionContents' NBT: the potion by name and the custom colour.
            Component::Potion { potion, color, .. } => {
                let potion = potion.and_then(|id| builtin.get("minecraft:potion")?.name(id));
                Tag::compound(
                    potion
                        .map(|name| ("potion", Tag::String(name.into())))
                        .into_iter()
                        .chain(color.map(|c| ("custom_color", Tag::Int(c)))),
                )
            }
            // ItemStackTemplate.CODEC: id, count and components.
            Component::Container(stacks) => Tag::List(
                stacks
                    .iter()
                    .enumerate()
                    .filter_map(|(slot, item)| {
                        item.as_ref().map(|item| {
                            let native = from_network(item, builtin, synced);
                            Tag::compound([
                                ("slot", Tag::Int(slot as i32)),
                                ("item", crate::save::item_to_nbt(&native)),
                            ])
                        })
                    })
                    .collect(),
            ),
            Component::Stacks(stacks) => Tag::List(
                stacks
                    .iter()
                    .map(|item| crate::save::item_to_nbt(&from_network(item, builtin, synced)))
                    .collect(),
            ),
            Component::CustomModelData {
                floats,
                flags,
                strings,
                colors,
            } => Tag::compound([
                (
                    "floats",
                    Tag::List(floats.iter().map(|f| Tag::Float(f.to_bits())).collect()),
                ),
                (
                    "flags",
                    Tag::List(flags.iter().map(|b| Tag::Byte(i8::from(*b))).collect()),
                ),
                (
                    "strings",
                    Tag::List(
                        strings
                            .iter()
                            .map(|s| Tag::String(s.as_str().into()))
                            .collect(),
                    ),
                ),
                (
                    "colors",
                    Tag::List(colors.iter().map(|c| Tag::Int(*c)).collect()),
                ),
            ]),
            Component::TooltipDisplay { hide, hidden } => {
                let hidden = hidden
                    .iter()
                    .filter_map(|id| Some(Tag::String(name(*id)?.into())));
                Tag::compound([
                    ("hide_tooltip", Tag::Byte(i8::from(*hide))),
                    ("hidden_components", Tag::List(hidden.collect())),
                ])
            }
        };
        entries.push((key, value));
    }
    let mut patch = Tag::compound(entries);
    if let Tag::Compound(map) = &mut patch {
        for id in &s.removed {
            if let Some(key) = name(*id) {
                map.remove(&key.into());
                map.insert(format!("!{key}").as_str().into(), Tag::Compound(Default::default()));
            }
        }
    }
    Some(patch)
}

/// An `ItemStackTemplate` as its NBT codec writes it: id, count and components.
fn template_tag(t: &Stack, builtin: &Builtin, synced: &Dynamic) -> Tag {
    let mut entries = vec![
        (
            "id",
            Tag::String(item_name(builtin, t.item).as_ref().into()),
        ),
        ("count", Tag::Int(t.count)),
    ];
    if let Some(c) = components_tag(t, builtin, synced) {
        entries.push(("components", c));
    }
    Tag::compound(entries)
}

/// `TypedEntityData.streamCodec` of a block entity: its type's registry id,
/// then its data without the id, which the saved component keeps.
fn block_entity_data_tag(bytes: &[u8], builtin: &Builtin) -> Option<Tag> {
    let mut r = Reader::new(bytes);
    let id = r.varint().ok()?;
    let kind = builtin.get("minecraft:block_entity_type")?.name(id)?;
    let Tag::Compound(mut data) = r.nbt().ok()? else {
        return None;
    };
    data.insert("id".into(), Tag::String(kind.into()));
    Some(Tag::Compound(data))
}

/// `ItemContainerContents.STREAM_CODEC`, one optional template per slot, as
/// the saved `container` lists the slots that hold one.
fn container_tag(bytes: &[u8], builtin: &Builtin, synced: &Dynamic) -> Option<Tag> {
    let mut r = Reader::new(bytes);
    let mut slots = Vec::new();
    for slot in 0..r.count(256).ok()? {
        if r.bool().ok()? {
            let stack = Template::read(&mut r).ok()?;
            slots.push(Tag::compound([
                ("slot", Tag::Int(slot as i32)),
                ("item", template_tag(&stack, builtin, synced)),
            ]));
        }
    }
    Some(Tag::List(slots))
}

/// A wire `FireworkExplosion` as vanilla's NBT codec writes it
/// (`FireworkExplosion.Shape`'s BY_ID order).
fn fireworks_tag(e: &horizon_protocol::item::FireworkExplosion) -> Tag {
    const SHAPES: [&str; 5] = ["small_ball", "large_ball", "star", "creeper", "burst"];
    let ints = |c: &[i32]| Tag::List(c.iter().map(|c| Tag::Int(*c)).collect());
    Tag::compound([
        (
            "shape",
            Tag::String(
                SHAPES
                    .get(e.shape.max(0) as usize)
                    .copied()
                    .unwrap_or("small_ball")
                    .into(),
            ),
        ),
        ("colors", ints(&e.colors)),
        ("fade_colors", ints(&e.fade)),
        ("has_trail", Tag::Byte(i8::from(e.trail))),
        ("has_twinkle", Tag::Byte(i8::from(e.twinkle))),
    ])
}

/// A `minecraft:profile` component as the item data JSON the model reads.
fn profile_tag(p: &Profile) -> Tag {
    let properties = p
        .properties
        .iter()
        .map(|(name, value, signature)| {
            let mut entries: Vec<(&'static str, Tag)> = vec![
                ("name", Tag::String(name.as_str().into())),
                ("value", Tag::String(value.as_str().into())),
            ];
            if let Some(signature) = signature {
                entries.push(("signature", Tag::String(signature.as_str().into())));
            }
            Tag::compound(entries)
        })
        .collect::<Vec<_>>();
    let mut entries: Vec<(&'static str, Tag)> = Vec::new();
    if let Some(name) = &p.name {
        entries.push(("name", Tag::String(name.as_str().into())));
    }
    if let Some(id) = p.id {
        entries.push((
            "id",
            Tag::IntArray(vec![
                (id >> 96) as i32,
                (id >> 64) as i32,
                (id >> 32) as i32,
                id as i32,
            ]),
        ));
    }
    entries.push(("properties", Tag::List(properties)));
    Tag::compound(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_protocol::registry::Ids;

    #[cfg(feature = "ae2-lab")]
    #[test]
    fn full_item_component_patch_and_nested_transients_survive_wire() {
        use horizon_protocol::{buf::{Codec,Reader,Writer},item::Slot};
        let names = ["minecraft:white_banner", "minecraft:spawner", "minecraft:shulker_box", "minecraft:filled_map", "minecraft:bundle", "minecraft:potion", "minecraft:written_book"];
        let mut component_names = vec![String::new(); 78];
        for (id,name) in [(component::CUSTOM_DATA,"minecraft:custom_data"),
            (component::MAX_STACK_SIZE,"minecraft:max_stack_size"),
            (component::CUSTOM_NAME,"minecraft:custom_name"),
            (component::CONTAINER,"minecraft:container"),
            (component::BUNDLE_CONTENTS,"minecraft:bundle_contents"),
            (component::MAP_POST_PROCESSING,"minecraft:map_post_processing")] {
            component_names[id as usize] = name.into();
        }
        let builtin = Builtin {registries:[
            ("minecraft:item".into(),Ids::from_names(names.iter().map(|s|s.to_string()).collect())),
            ("minecraft:data_component_type".into(),Ids::from_names(component_names)),
        ].into()};
        let wire=|item:&ItemStack|crate::lan::guest::stack_to_network(
            |name|builtin.get("minecraft:item").unwrap().id(name).unwrap(),|_,_|None,&Some(item.clone())).unwrap();
        let round=|item:&ItemStack| {
            let mut writer=Writer::new(); Slot::write(&Some(wire(item)),&mut writer);
            let mut reader=Reader::new(&writer.data); let decoded=Slot::read(&mut reader).unwrap().unwrap();
            assert_eq!(reader.remaining(),0);
            from_network(&decoded,&builtin,&Dynamic::default())
        };
        let marker = Tag::compound([("version",Tag::Byte(1)),("had_custom_data",Tag::Byte(0)),
            ("components",Tag::compound([("ae2:entangled_singularity_id",Tag::Long(777))]))]);
        let mut banner=ItemStack::new("minecraft:white_banner",1);
        banner.data_mut().banner_patterns = vec![crate::banner::BannerLayer::new("minecraft:flower","black")];
        banner.data_mut().set_other("minecraft:block_entity_data",Some(Tag::compound([
            ("id",Tag::String("minecraft:banner".into())),("proof",Tag::String("kept".into()))])));
        banner.data_mut().set_other("minecraft:max_stack_size",Some(Tag::Int(5)));
        banner.data_mut().set_other("!minecraft:unbreakable",Some(Tag::Compound(Default::default())));
        banner.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("horizon:wire_ae2_components",marker)])));
        banner.set_custom_name_component(Some(Tag::compound([("text",Tag::String("Tint".into())),("color",Tag::String("red".into()))])));
        assert_eq!(wire(&banner).component(component::MAX_STACK_SIZE),Some(&Component::Int(5)));
        assert_eq!(round(&banner),banner,"complete patch preserves modeled, unknown, negative and colliding components");
        let rich=Tag::compound([("minecraft:potion_contents",Tag::compound([
            ("potion",Tag::String("minecraft:water".into())),("custom_color",Tag::Int(0x123456)),
            ("custom_effects",Tag::List(vec![Tag::compound([("id",Tag::String("minecraft:speed".into()))])]))])),
            ("minecraft:written_book_content",Tag::compound([("title",Tag::String("Title".into())),
                ("author",Tag::String("Writer".into())),("pages",Tag::List(vec![Tag::compound([
                    ("raw",Tag::compound([("text",Tag::String("scarlet".into())),("color",Tag::String("red".into()))])),
                    ("filtered",Tag::String("hidden".into()))])])),("resolved",Tag::Byte(1))]))]);
        let (_,extra)=crate::item_data::decode_components(Some(&rich));
        let mut rich_item=ItemStack::new("minecraft:written_book",1);rich_item.extra=extra;
        assert_eq!(round(&rich_item),rich_item,"opaque source potion and book fields survive byte encoding");
        let mut inner=ItemStack::new("minecraft:filled_map",1);
        inner.data_mut().map_post_processing=Some(1);
        inner.data_mut().set_other("minecraft:block_entity_data",Some(Tag::compound([("id",Tag::String("minecraft:mob_spawner".into()))])));
        let mut outer=ItemStack::new("minecraft:shulker_box",1);
        outer.data_mut().container.push((3,inner));
        let mut bundle=ItemStack::new("minecraft:bundle",1);
        bundle.data_mut().bundle_contents.push(outer);
        assert_eq!(round(&bundle),bundle,"nested item patches and transient map processing survive");
        let ominous=crate::mobs::raider::ominous_banner();
        assert_eq!(round(&ominous),ominous,"the native implicit banner representation survives");

        let mut negative=Stack::new(0,1);
        negative.removed.push(component::MAX_STACK_SIZE);
        let removed=from_network(&negative,&builtin,&Dynamic::default());
        assert!(removed.data().unwrap().other("!minecraft:max_stack_size").is_some());
        assert_eq!(round(&removed),removed,"network removals become persistent source patch removals");
        let legacy=Tag::compound([("horizon:wire_ae2_components",Tag::compound([
            ("version",Tag::Byte(1)),("had_custom_data",Tag::Byte(0)),
            ("components",Tag::compound([("ae2:entangled_singularity_id",Tag::Long(42))]))]))]);
        let mut old_packet=Stack::new(0,1);old_packet.added.push((component::CUSTOM_DATA,Component::Nbt(legacy)));
        assert_eq!(from_network(&old_packet,&builtin,&Dynamic::default()).data().unwrap().other("ae2:entangled_singularity_id"),Some(&Tag::Long(42)));
        let malformed=Tag::compound([("horizon:wire_ae2_components",Tag::compound([
            ("version",Tag::Byte(2)),("had_custom_data",Tag::Byte(0)),
            ("components",Tag::String("bad".into()))]))]);
        let mut packet=Stack::new(0,1);packet.added.push((component::CUSTOM_DATA,Component::Nbt(malformed.clone())));
        assert_eq!(from_network(&packet,&builtin,&Dynamic::default()).data().unwrap().other("minecraft:custom_data"),Some(&malformed));
    }

    #[cfg(feature = "ae2-lab")]
    #[test]
    fn name_press_and_structured_names_survive_the_integrated_wire() {
        use horizon_protocol::{buf::{Codec, Reader, Writer}, item::Slot};
        let mut components=vec![String::new();7];
        components[component::CUSTOM_DATA as usize]="minecraft:custom_data".into();
        components[component::CUSTOM_NAME as usize]="minecraft:custom_name".into();
        let builtin=Builtin {registries:[
            ("minecraft:item".into(),Ids::from_names(vec!["ae2:name_press".into(),"minecraft:diamond".into()])),
            ("minecraft:data_component_type".into(),Ids::from_names(components)),
        ].into()};
        let round=|item:&ItemStack| {
            let wire=crate::lan::guest::stack_to_network(
                |name|builtin.get("minecraft:item").unwrap().id(name).unwrap(),|_,_|None,&Some(item.clone()));
            let mut writer=Writer::new();Slot::write(&wire,&mut writer);
            let mut reader=Reader::new(&writer.data);let decoded=Slot::read(&mut reader).unwrap().unwrap();
            assert_eq!(reader.remaining(),0);
            from_network(&decoded,&builtin,&Dynamic::default())
        };
        let styled=Tag::compound([("text",Tag::String("Red".into())),("color",Tag::String("red".into()))]);
        let original_data=Tag::compound([("proof",Tag::Int(42)),("horizon:wire_name_press_name",Tag::String("existing".into()))]);
        let mut press=ItemStack::new("ae2:name_press",1);
        press.data_mut().set_other("ae2:name_press_name",Some(styled.clone()));
        press.data_mut().set_other("minecraft:custom_data",Some(original_data));
        assert_eq!(round(&press),press,"the transient envelope preserves real and existing components");
        let mut named=ItemStack::new("minecraft:diamond",1);
        named.set_custom_name_component(Some(styled.clone()));
        let after=round(&named);
        assert_eq!(after.custom_name_component(),Some(&styled));
        assert!(after.same_item_same_components(&named));
    }

    #[cfg(feature = "ae2-lab")]
    #[test]
    fn entangled_frequency_survives_wire_and_preserves_colliding_custom_data() {
        use horizon_protocol::{buf::{Codec, Reader, Writer}, item::Slot};
        let mut components = vec![String::new(); 7];
        components[component::CUSTOM_DATA as usize] = "minecraft:custom_data".into();
        let builtin = Builtin { registries: [
            ("minecraft:item".into(), Ids::from_names(vec!["minecraft:diamond".into(), "ae2:quantum_entangled_singularity".into()])),
            ("minecraft:data_component_type".into(), Ids::from_names(components)),
        ].into() };
        let round = |item: &ItemStack| {
            let wire = crate::lan::guest::stack_to_network(
                |name| builtin.get("minecraft:item").unwrap().id(name).unwrap(), |_, _| None, &Some(item.clone()));
            let mut writer = Writer::new(); Slot::write(&wire, &mut writer);
            let mut reader = Reader::new(&writer.data); let decoded = Slot::read(&mut reader).unwrap().unwrap();
            assert_eq!(reader.remaining(), 0);
            from_network(&decoded, &builtin, &Dynamic::default())
        };
        let key = "horizon:wire_ae2_components";
        let existing = Tag::compound([
            ("version", Tag::Byte(1)),
            ("components", Tag::compound([("ae2:entangled_singularity_id", Tag::Long(123))])),
        ]);
        let mut arbitrary = ItemStack::new("minecraft:diamond", 1);
        arbitrary.data_mut().set_other("minecraft:custom_data", Some(Tag::compound([(key, existing)])));
        assert_eq!(round(&arbitrary), arbitrary, "an unrelated pre-existing marker is escaped even without a frequency");
        arbitrary.data_mut().set_other("ae2:entangled_singularity_id", Some(Tag::Long(-42)));
        assert_eq!(round(&arbitrary), arbitrary, "a frequency on any source-valid item and colliding data both survive");
        let mut pair = ItemStack::new("ae2:quantum_entangled_singularity", 2);
        pair.data_mut().set_other("ae2:entangled_singularity_id", Some(Tag::Long(0)));
        assert_eq!(round(&pair), pair, "zero remains a present component");
        pair.data_mut().set_other("minecraft:custom_data", Some(Tag::compound([] as [(&str, Tag); 0])));
        assert_eq!(round(&pair), pair, "an explicitly empty custom-data component remains present");
    }

    #[cfg(feature = "ae2-lab")]
    #[test]
    fn physical_block_drop_components_survive_client_and_creative_slot_wire() {
        use horizon_protocol::{buf::{Codec,Reader,Writer},item::Slot};
        let names=["minecraft:shulker_box","minecraft:beehive","minecraft:decorated_pot","minecraft:archer_pottery_sherd","minecraft:diamond"];
        let mut components=vec![String::new();78];
        components[component::CUSTOM_DATA as usize]="minecraft:custom_data".into();
        components[component::CONTAINER as usize]="minecraft:container".into();
        let builtin=Builtin {registries:[
            ("minecraft:item".into(),Ids::from_names(names.iter().map(|name|name.to_string()).collect())),
            ("minecraft:data_component_type".into(),Ids::from_names(components)),
        ].into()};
        let round=|item:&ItemStack| {
            let packet=crate::lan::guest::stack_to_network(|name|builtin.get("minecraft:item").unwrap().id(name).unwrap(),|_,_|None,&Some(item.clone()));
            let mut writer=Writer::new();Slot::write(&packet,&mut writer);
            let mut reader=Reader::new(&writer.data);let decoded=Slot::read(&mut reader).unwrap().unwrap();
            assert_eq!(reader.remaining(),0);
            from_network(&decoded,&builtin,&Dynamic::default())
        };
        let lock=Tag::compound([("components",Tag::compound([("minecraft:custom_name",Tag::String("Key".into()))]))]);
        let loot=Tag::compound([("loot_table",Tag::String("minecraft:chests/simple_dungeon".into())),("seed",Tag::Long(12))]);
        let mut shulker=ItemStack::new("minecraft:shulker_box",1);
        shulker.data_mut().set_other("minecraft:lock",Some(lock));
        shulker.data_mut().set_other("minecraft:container_loot",Some(loot));
        shulker.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("horizon:wire_ae2_components",Tag::String("existing".into()))])));
        assert_eq!(round(&shulker),shulker,"the lock, loot and existing colliding custom data survive");
        let mut hive=ItemStack::new("minecraft:beehive",1);
        hive.data_mut().set_other("minecraft:bees",Some(Tag::List(vec![Tag::compound([
            ("entity_data",Tag::compound([("id",Tag::String("minecraft:bee".into()))])),
            ("ticks_in_hive",Tag::Int(7)),("min_ticks_in_hive",Tag::Int(600)),
        ])])));
        hive.data_mut().set_other("minecraft:block_state",Some(Tag::compound([("honey_level",Tag::String("4".into()))])));
        assert_eq!(round(&hive),hive,"the bees and actual block state survive");
        let mut pot=ItemStack::new("minecraft:decorated_pot",1);
        pot.data_mut().pot_decorations=Some(Box::new([Some(ItemStack::new("minecraft:archer_pottery_sherd",1)),None,None,None]));
        assert_eq!(round(&pot),pot,"modeled pot decorations survive");
        let mut container=ItemStack::new("minecraft:shulker_box",1);
        container.data_mut().container=vec![(3,hive)];
        assert_eq!(round(&container),container,"nested item components survive the same wire path");
    }

    #[cfg(feature = "ae2-lab")]
    #[test]
    fn every_source_component_roundtrips_and_unknown_envelopes_stay_data() {
        use horizon_protocol::{buf::{Codec, Reader, Writer}, item::Slot};
        let item = || Tag::compound([("id", Tag::String("minecraft:stone".into())), ("count", Tag::Int(1))]);
        let string = |s: &str| Tag::String(s.into());
        let list = || Tag::List(Vec::new());
        let generic = || Tag::compound([("#t", string("ae2:i")), ("id", string("minecraft:stone")), ("#", Tag::Long(1))]);
        let samples = vec![
            ("ae2:exported_settings_source", string("Example")),
            ("ae2:exported_custom_name", string("Named")),
            ("ae2:exported_upgrades", list()),
            ("ae2:exported_settings", Tag::compound([("redstone_mode", string("HIGH"))])),
            ("ae2:exported_priority", Tag::Int(4)),
            ("ae2:exported_p2p_type", string("minecraft:stone")),
            ("ae2:exported_p2p_frequency", Tag::Short(42)),
            ("ae2:memory_card_colors", Tag::List((0..8).map(|_| string("fluix")).collect())),
            ("ae2:exported_config_inv", list()),
            ("ae2:exported_level_emitter_value", Tag::Long(31)),
            ("ae2:exported_patterns", list()),
            ("ae2:exported_push_direction", string("all")),
            ("ae2:name_press_name", string("Plate")),
            ("ae2:upgrades", list()),
            ("ae2:entangled_singularity_id", Tag::Long(42)),
            ("ae2:stored_energy", Tag::Double(123.5f64.to_bits())),
            ("ae2:energy_capacity", Tag::Double(500.0f64.to_bits())),
            ("ae2:encoded_crafting_pattern", Tag::compound([
                ("inputs", list()), ("result", item()), ("recipeId", string("minecraft:stone")),
                ("canSubstitute", Tag::Byte(0)), ("canSubstituteFluids", Tag::Byte(0)),
            ])),
            ("ae2:encoded_processing_pattern", Tag::compound([("sparseInputs", list()), ("sparseOutputs", list())])),
            ("ae2:encoded_stonecutting_pattern", Tag::compound([
                ("input", item()), ("output", item()), ("recipeId", string("minecraft:stone")), ("canSubstitute", Tag::Byte(0)),
            ])),
            ("ae2:encoded_smithing_table_pattern", Tag::compound([
                ("template", item()), ("base", item()), ("addition", item()), ("resultItem", item()),
                ("recipeId", string("minecraft:stone")), ("canSubstitute", Tag::Byte(0)),
            ])),
            ("ae2:enabled_key_types", Tag::List(vec![string("ae2:i")])),
            ("ae2:wireless_link_target", Tag::compound([("dimension", string("minecraft:overworld")), ("pos", Tag::IntArray(vec![1, 64, 2]))])),
            ("ae2:selected_color", string("fluix")),
            ("ae2:storage_cell_fuzzy_mode", string("IGNORE_ALL")),
            ("ae2:storage_cell_inv", list()),
            ("ae2:storage_cell_config_inv", list()),
            ("ae2:facade_item", string("minecraft:stone")),
            ("ae2:facade_cycle_property", string("axis")),
            ("ae2:wrapped_stack", generic()),
            ("ae2:crafting_inv", list()),
            ("ae2:spatial_plot_info", Tag::compound([("id", Tag::Int(3)), ("size", Tag::IntArray(vec![2, 2, 2]))])),
            ("ae2:missing_content_itemstack_data", Tag::compound([] as [(&str, Tag); 0])),
            ("ae2:missing_content_aekey_data", Tag::compound([] as [(&str, Tag); 0])),
            ("ae2:missing_content_error", string("missing")),
        ];
        let expected: std::collections::BTreeSet<_> = crate::ae2_components::PERSISTENT_IDS.iter().copied().collect();
        assert_eq!(samples.iter().map(|(id, _)| *id).collect::<std::collections::BTreeSet<_>>(), expected);
        assert_eq!(samples.len(), 35);
        for (id, value) in &samples {
            crate::ae2_components::validate(id, value).unwrap_or_else(|error| panic!("invalid source-shaped sample for {id}: {error}"));
        }
        let mut components = vec![String::new(); 78];
        components[component::CUSTOM_DATA as usize] = "minecraft:custom_data".into();
        components[component::CONTAINER as usize] = "minecraft:container".into();
        components[component::BUNDLE_CONTENTS as usize] = "minecraft:bundle_contents".into();
        let builtin = Builtin { registries: [
            ("minecraft:item".into(), Ids::from_names(vec!["minecraft:diamond".into(), "minecraft:shulker_box".into(), "minecraft:bundle".into()])),
            ("minecraft:data_component_type".into(), Ids::from_names(components)),
        ].into() };
        let mut original = ItemStack::new("minecraft:diamond", 1);
        for (id, value) in samples { original.data_mut().set_other(id, Some(value)); }
        original.data_mut().set_other("minecraft:custom_data", Some(Tag::compound([("marker", Tag::Int(9))])));
        let wire = crate::lan::guest::stack_to_network(|name| builtin.get("minecraft:item").unwrap().id(name).unwrap(), |_, _| None, &Some(original.clone()));
        let mut writer = Writer::new(); Slot::write(&wire, &mut writer);
        let mut reader = Reader::new(&writer.data); let decoded = Slot::read(&mut reader).unwrap().unwrap();
        assert_eq!(reader.remaining(), 0);
        assert_eq!(from_network(&decoded, &builtin, &Dynamic::default()), original);
        let mut container = ItemStack::new("minecraft:shulker_box", 1);
        container.data_mut().container.push((4, original.clone()));
        let wire = crate::lan::guest::stack_to_network(|name| builtin.get("minecraft:item").unwrap().id(name).unwrap(), |_, _| None, &Some(container.clone()));
        let mut writer = Writer::new(); Slot::write(&wire, &mut writer);
        let mut reader = Reader::new(&writer.data); let decoded = Slot::read(&mut reader).unwrap().unwrap();
        assert_eq!(reader.remaining(), 0);
        assert_eq!(from_network(&decoded, &builtin, &Dynamic::default()), container, "nested item components must also be promoted");
        let mut bundle = ItemStack::new("minecraft:bundle", 1);
        bundle.data_mut().bundle_contents.push(original.clone());
        let wire = crate::lan::guest::stack_to_network(|name| builtin.get("minecraft:item").unwrap().id(name).unwrap(), |_, _| None, &Some(bundle.clone()));
        let mut writer = Writer::new(); Slot::write(&wire, &mut writer);
        let mut reader = Reader::new(&writer.data); let decoded = Slot::read(&mut reader).unwrap().unwrap();
        assert_eq!(reader.remaining(), 0);
        assert_eq!(from_network(&decoded, &builtin, &Dynamic::default()), bundle, "nested bundle items must also be promoted");

        let forged = Tag::compound([("horizon:wire_ae2_components", Tag::compound([
            ("version", Tag::Byte(1)), ("had_custom_data", Tag::Byte(0)),
            ("components", Tag::compound([("ae2:not_registered", Tag::Int(8))])),
        ]))]);
        let mut packet = Stack::new(0, 1);
        packet.added.push((component::CUSTOM_DATA, Component::Nbt(forged.clone())));
        let untouched = from_network(&packet, &builtin, &Dynamic::default());
        assert_eq!(untouched.data().unwrap().other("minecraft:custom_data"), Some(&forged));
        assert!(untouched.data().unwrap().other("ae2:not_registered").is_none());
    }

    #[test]
    fn item_container_keeps_sparse_slots_and_nested_components_on_wire() {
        use horizon_protocol::{
            buf::{Codec, Reader, Writer},
            item::Slot,
        };
        let names = [
            "minecraft:air",
            "ae2:network_tool",
            "ae2:speed_card",
            "ae2:energy_card",
        ];
        let mut components = vec![String::new(); 78];
        components[component::CUSTOM_DATA as usize] = "minecraft:custom_data".into();
        components[component::CONTAINER as usize] = "minecraft:container".into();
        let builtin = Builtin {
            registries: [
                (
                    "minecraft:item".into(),
                    Ids::from_names(names.iter().map(|s| s.to_string()).collect()),
                ),
                (
                    "minecraft:data_component_type".into(),
                    Ids::from_names(components),
                ),
            ]
            .into(),
        };
        let mut marked = ItemStack::new("ae2:speed_card", 64);
        marked.data_mut().set_other(
            "minecraft:custom_data",
            Some(Tag::compound([("proof", Tag::Int(42))])),
        );
        let mut tool = ItemStack::new("ae2:network_tool", 1);
        tool.data_mut().container = vec![(1, marked), (8, ItemStack::new("ae2:energy_card", 10))];
        let round = |item: ItemStack| {
            let wire = crate::lan::guest::stack_to_network(
                |name| names.iter().position(|id| *id == name).unwrap() as i32,
                |_, _| None,
                &Some(item),
            );
            let mut w = Writer::new();
            Slot::write(&wire, &mut w);
            let mut r = Reader::new(&w.data);
            let decoded = Slot::read(&mut r).unwrap().unwrap();
            assert_eq!(r.remaining(), 0);
            from_network(&decoded, &builtin, &Dynamic::default())
        };
        assert_eq!(round(tool.clone()), tool);
        assert!(
            round(ItemStack::new("ae2:network_tool", 1))
                .data()
                .is_none_or(|d| d.container.is_empty())
        );
    }

    #[test]
    fn ae2_cell_custom_data_survives_network_wire_and_absence() {
        use horizon_protocol::{
            buf::{Codec, Reader, Writer},
            item::Slot,
        };
        let builtin = Builtin {
            registries: [
                (
                    "minecraft:item".into(),
                    Ids::from_names(vec!["ae2:item_storage_cell_1k".into()]),
                ),
                (
                    "minecraft:data_component_type".into(),
                    Ids::from_names(vec!["minecraft:custom_data".into()]),
                ),
            ]
            .into(),
        };
        let mut cell = ItemStack::new("ae2:item_storage_cell_1k", 1);
        let data = Tag::compound([(
            "ae2_cell",
            Tag::compound([
                ("version", Tag::Int(1)),
                (
                    "entries",
                    Tag::List(vec![Tag::compound([
                        ("amount", Tag::Long(4_294_967_301)),
                        (
                            "item",
                            Tag::compound([
                                ("id", Tag::String("minecraft:emerald".into())),
                                ("count", Tag::Int(1)),
                            ]),
                        ),
                    ])]),
                ),
            ]),
        )]);
        cell.data_mut()
            .set_other("minecraft:custom_data", Some(data.clone()));
        let roundtrip = |input: Option<ItemStack>| {
            let wire = crate::lan::guest::stack_to_network(
                |name| builtin.get("minecraft:item").unwrap().id(name).unwrap(),
                |_, _| None,
                &input,
            );
            let mut writer = Writer::new();
            Slot::write(&wire, &mut writer);
            let mut reader = Reader::new(&writer.data);
            let decoded = Slot::read(&mut reader).unwrap().unwrap();
            assert_eq!(reader.remaining(), 0);
            from_network(&decoded, &builtin, &Dynamic::default())
        };
        let received = roundtrip(Some(cell.clone()));
        assert_eq!(
            received
                .data()
                .and_then(|d| d.other("minecraft:custom_data")),
            Some(&data)
        );
        assert_eq!(received, cell);
        // An explicitly removed component is also an empty fresh value.
        let mut removed = Stack::new(0, 1);
        removed.removed.push(component::CUSTOM_DATA);
        let mut writer = Writer::new();
        Slot::write(&Some(removed), &mut writer);
        let mut reader = Reader::new(&writer.data);
        let decoded = Slot::read(&mut reader).unwrap().unwrap();
        let reset = from_network(&decoded, &builtin, &Dynamic::default());
        assert!(
            reset
                .data()
                .and_then(|d| d.other("minecraft:custom_data"))
                .is_none()
        );
        // A new item without custom data must not inherit the preceding stack's payload.
        let plain = roundtrip(Some(ItemStack::new("ae2:item_storage_cell_1k", 1)));
        assert!(
            plain
                .data()
                .and_then(|d| d.other("minecraft:custom_data"))
                .is_none()
        );
    }

    #[test]
    fn ae2_terminal_count_lore_survives_full_wire_round_trip() {
        use horizon_protocol::{
            buf::{Codec, Reader, Writer},
            item::Slot,
        };
        let mut components = vec![String::new(); component::LORE as usize + 1];
        components[component::CUSTOM_DATA as usize] = "minecraft:custom_data".into();
        components[component::LORE as usize] = "minecraft:lore".into();
        let builtin = Builtin {
            registries: [
                (
                    "minecraft:item".into(),
                    Ids::from_names(vec!["minecraft:diamond".into()]),
                ),
                (
                    "minecraft:data_component_type".into(),
                    Ids::from_names(components),
                ),
            ]
            .into(),
        };
        let mut shown = ItemStack::new("minecraft:diamond", 64);
        let lore = Tag::List(vec![Tag::String("Stored: 4294967301".into())]);
        shown
            .data_mut()
            .set_other("minecraft:lore", Some(lore.clone()));
        let wire = crate::lan::guest::stack_to_network(
            |id| builtin.get("minecraft:item").unwrap().id(id).unwrap(),
            |_, _| None,
            &Some(shown.clone()),
        );
        let mut writer = Writer::new();
        Slot::write(&wire, &mut writer);
        let mut reader = Reader::new(&writer.data);
        let decoded = Slot::read(&mut reader).unwrap().unwrap();
        assert_eq!(reader.remaining(), 0);
        let received = from_network(&decoded, &builtin, &Dynamic::default());
        assert_eq!(
            received.data().and_then(|d| d.other("minecraft:lore")),
            Some(&lore)
        );
        assert_eq!(received, shown);
    }

    #[test]
    fn merchant_water_cost_survives_the_offer_packet() {
        use horizon_protocol::{
            buf::{Codec, Reader, Writer},
            packets::play::{NetOffer, Offers},
        };
        let mut components = vec![String::new(); component::POTION_CONTENTS as usize + 1];
        components[component::CUSTOM_DATA as usize] = "minecraft:custom_data".into();
        components[component::POTION_CONTENTS as usize] = "minecraft:potion_contents".into();
        let builtin = Builtin {
            registries: [
                (
                    "minecraft:item".into(),
                    Ids::from_names(vec!["minecraft:potion".into(), "minecraft:emerald".into()]),
                ),
                (
                    "minecraft:potion".into(),
                    Ids::from_names(vec!["minecraft:water".into()]),
                ),
                (
                    "minecraft:data_component_type".into(),
                    Ids::from_names(components),
                ),
            ]
            .into(),
        };
        let dynamic = Dynamic::default();
        let water = ItemStack::new("minecraft:potion", 1).with_potion("minecraft:water");
        let wire = crate::lan::guest::stack_to_network(
            |name| builtin.get("minecraft:item").unwrap().id(name).unwrap(),
            |registry, name| builtin.get(registry)?.id(name),
            &Some(water.clone()),
        )
        .unwrap();
        let offers = vec![NetOffer {
            cost_a: wire.into(),
            result: Stack::new(1, 1),
            cost_b: Some(Stack::new(1, 2).into()),
            out_of_stock: false,
            uses: 0,
            max_uses: 2,
            xp: 0,
            special_price: 0,
            price_multiplier: 0.05,
            demand: 0,
        }];
        let mut w = Writer::new();
        Offers::write(&offers, &mut w);
        let mut r = Reader::new(&w.data);
        let decoded = Offers::read(&mut r).unwrap();
        assert_eq!(r.remaining(), 0);
        let cost = crate::merchant::ItemCost {
            stack: from_network(&decoded[0].cost_a.clone().into(), &builtin, &dynamic),
        };
        assert_eq!(cost.stack, water);
        assert_eq!(cost.stack.potion(), Some("minecraft:water"));
        let authoritative = crate::merchant::ItemCost {
            stack: water.clone(),
        };
        for payment in [
            water,
            ItemStack::new("minecraft:potion", 1).with_potion("minecraft:swiftness"),
            ItemStack::new("minecraft:potion", 1),
        ] {
            assert_eq!(cost.test(&payment), authoritative.test(&payment));
        }
        assert!(cost.test(&ItemStack::new("minecraft:potion", 1).with_potion("minecraft:water")));
        assert!(
            !cost.test(&ItemStack::new("minecraft:potion", 1).with_potion("minecraft:swiftness"))
        );
        let emerald = crate::merchant::ItemCost {
            stack: from_network(
                &decoded[0].cost_b.clone().unwrap().into(),
                &builtin,
                &dynamic,
            ),
        };
        assert!(emerald.test(&ItemStack::new("minecraft:emerald", 2)));
    }

    /// A bundle's contents cross the wire, so a server's client can draw
    /// its tooltip (BundleContents' stream codec: a list of templates).
    #[test]
    fn a_bundle_keeps_its_contents_across_the_wire() {
        let mut components = vec![String::new(); 60];
        components[component::CUSTOM_DATA as usize] = "minecraft:custom_data".into();
        components[component::BUNDLE_CONTENTS as usize] = "minecraft:bundle_contents".into();
        let items = ["minecraft:air", "minecraft:bundle", "minecraft:stone"];
        let builtin = Builtin {
            registries: [
                (
                    "minecraft:item".to_owned(),
                    Ids::from_names(items.iter().map(|s| s.to_string()).collect()),
                ),
                (
                    "minecraft:data_component_type".to_owned(),
                    Ids::from_names(components),
                ),
            ]
            .into(),
        };
        let dynamic = Dynamic {
            registries: Default::default(),
            ..Default::default()
        };
        let mut bundle = ItemStack::new("minecraft:bundle", 1);
        bundle.data_mut().bundle_contents = vec![ItemStack::new("minecraft:stone", 32)];
        let wire = crate::lan::guest::stack_to_network(
            |name| items.iter().position(|i| *i == name).unwrap_or(0) as i32,
            |_, _| None,
            &Some(bundle),
        )
        .unwrap();
        let back = from_network(&wire, &builtin, &dynamic);
        let contents = &back.data().expect("components").bundle_contents;
        assert_eq!(contents.len(), 1);
        assert_eq!(
            (&*contents[0].item, contents[0].count),
            ("minecraft:stone", 32)
        );
    }

    /// An ominous bottle's `ominous_bottle_amplifier` reaches the client
    /// through the packet's bytes, so its tooltip names the Bad Omen level.
    #[test]
    fn an_ominous_bottle_keeps_its_amplifier_across_the_wire() {
        use horizon_protocol::buf::{Codec, Reader, Writer};
        use horizon_protocol::item::Slot;
        let mut components = vec![String::new(); 80];
        components[component::OMINOUS_BOTTLE_AMPLIFIER as usize] =
            "minecraft:ominous_bottle_amplifier".into();
        let items = ["minecraft:air", "minecraft:ominous_bottle"];
        let builtin = Builtin {
            registries: [
                (
                    "minecraft:item".to_owned(),
                    Ids::from_names(items.iter().map(|s| s.to_string()).collect()),
                ),
                (
                    "minecraft:data_component_type".to_owned(),
                    Ids::from_names(components),
                ),
            ]
            .into(),
        };
        let dynamic = Dynamic {
            registries: Default::default(),
            ..Default::default()
        };
        for amplifier in [None, Some(0), Some(3)] {
            let mut bottle = ItemStack::new("minecraft:ominous_bottle", 1);
            if let Some(a) = amplifier {
                bottle.data_mut().ominous_amplifier = Some(a);
            }
            let wire = crate::lan::guest::stack_to_network(
                |name| items.iter().position(|i| *i == name).unwrap_or(0) as i32,
                |_, _| None,
                &Some(bottle),
            );
            let mut w = Writer::new();
            Slot::write(&wire, &mut w);
            let mut r = Reader::new(&w.data);
            let read = Slot::read(&mut r).unwrap().unwrap();
            assert_eq!(r.remaining(), 0);
            let back = from_network(&read, &builtin, &dynamic);
            assert_eq!(
                back.data().and_then(|d| d.ominous_amplifier),
                amplifier,
                "{amplifier:?}"
            );
        }
    }

    /// What a creative client sends (`stack_to_network`) comes back whole on
    /// the server: damage, both enchantment lists, repair cost, name, trim,
    /// potion and stew effects.
    #[test]
    fn a_creative_stack_keeps_its_components_across_the_wire() {
        let mut components = vec![String::new(); 60];
        for (id, name) in [
            (component::CUSTOM_DATA, "minecraft:custom_data"),
            (component::DAMAGE, "minecraft:damage"),
            (component::CUSTOM_NAME, "minecraft:custom_name"),
            (component::ITEM_NAME, "minecraft:item_name"),
            (component::ENCHANTMENTS, "minecraft:enchantments"),
            (component::POTION_CONTENTS, "minecraft:potion_contents"),
            (component::REPAIR_COST, "minecraft:repair_cost"),
            (
                component::STORED_ENCHANTMENTS,
                "minecraft:stored_enchantments",
            ),
            (component::TRIM, "minecraft:trim"),
            (
                component::SUSPICIOUS_STEW_EFFECTS,
                "minecraft:suspicious_stew_effects",
            ),
        ] {
            components[id as usize] = name.to_owned();
        }
        let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let builtin = Builtin {
            registries: [
                (
                    "minecraft:item".to_owned(),
                    Ids::from_names(names(&[
                        "minecraft:air",
                        "minecraft:diamond_chestplate",
                        "minecraft:splash_potion",
                        "minecraft:suspicious_stew",
                    ])),
                ),
                (
                    "minecraft:data_component_type".to_owned(),
                    Ids::from_names(components),
                ),
                (
                    "minecraft:potion".to_owned(),
                    Ids::from_names(names(&["minecraft:water", "minecraft:swiftness"])),
                ),
                (
                    "minecraft:mob_effect".to_owned(),
                    Ids::from_names(names(&["minecraft:speed", "minecraft:fire_resistance"])),
                ),
            ]
            .into(),
        };
        let synced = |n: &[&str]| n.iter().map(|s| (s.to_string(), None)).collect::<Vec<_>>();
        let dynamic = Dynamic {
            registries: [
                (
                    "minecraft:enchantment".to_owned(),
                    synced(&["minecraft:protection", "minecraft:sharpness"]),
                ),
                (
                    "minecraft:trim_material".to_owned(),
                    synced(&["minecraft:gold"]),
                ),
                (
                    "minecraft:trim_pattern".to_owned(),
                    synced(&["minecraft:sentry"]),
                ),
            ]
            .into(),
            ..Default::default()
        };
        let mut stack = ItemStack::new("minecraft:diamond_chestplate", 1);
        stack.damage = 5;
        stack.enchant("minecraft:protection", 4);
        let d = stack.data_mut();
        d.stored_enchantments
            .push((Arc::from("minecraft:sharpness"), 2));
        d.repair_cost = 9;
        d.custom_name = Some(Tag::String("Keeper".into()));
        // A translatable item name reaches the client as the same component.
        let item_name = Tag::compound([("translate", Tag::String("filled_map.monument".into()))]);
        d.other
            .push(("minecraft:item_name".into(), item_name.clone()));
        d.trim = Some((Arc::from("minecraft:gold"), Arc::from("minecraft:sentry")));
        let id = |registry: &str, name: &str| {
            builtin
                .get(registry)
                .and_then(|ids| ids.id(name))
                .or_else(|| dynamic.id(registry, name))
        };
        let wire = crate::lan::guest::stack_to_network(
            |name| {
                builtin
                    .get("minecraft:item")
                    .and_then(|i| i.id(name))
                    .unwrap_or(0)
            },
            id,
            &Some(stack.clone()),
        )
        .unwrap();
        let back = from_network(&wire, &builtin, &dynamic);
        assert_eq!(back.item, stack.item);
        assert_eq!(back.damage, 5);
        let d = back.data().expect("components");
        assert_eq!(d.enchantments, vec![(Arc::from("minecraft:protection"), 4)]);
        assert_eq!(
            d.stored_enchantments,
            vec![(Arc::from("minecraft:sharpness"), 2)]
        );
        assert_eq!(d.repair_cost, 9);
        assert_eq!(d.custom_name_plain().as_deref(), Some("Keeper"));
        assert_eq!(d.other("minecraft:item_name"), Some(&item_name));
        assert_eq!(
            d.trim,
            Some((Arc::from("minecraft:gold"), Arc::from("minecraft:sentry")))
        );
        // PotionContents: the potion is a built-in registry, as a hub's
        // `SyncData::id` treats it, so the id has to come from there.
        let potion = ItemStack::new("minecraft:splash_potion", 1).with_potion("swiftness");
        let wire = crate::lan::guest::stack_to_network(
            |name| id("minecraft:item", name).unwrap_or(0),
            id,
            &Some(potion),
        )
        .unwrap();
        assert_eq!(
            from_network(&wire, &builtin, &dynamic).potion(),
            Some("minecraft:swiftness")
        );
        // SuspiciousStewEffects, as a creative tab's stew has them: without
        // them the server's stew gives no effect when eaten (#3137).
        let mut stew = ItemStack::new("minecraft:suspicious_stew", 1);
        stew.data_mut()
            .stew_effects
            .push((Arc::from("minecraft:fire_resistance"), 60));
        let wire = crate::lan::guest::stack_to_network(
            |name| id("minecraft:item", name).unwrap_or(0),
            id,
            &Some(stew),
        )
        .unwrap();
        let back = from_network(&wire, &builtin, &dynamic);
        assert_eq!(
            back.data().map(|d| d.stew_effects.clone()),
            Some(vec![(Arc::from("minecraft:fire_resistance"), 60)])
        );
    }

    /// A painting's `painting/variant` (a `painting_variant` holder, id 109)
    /// crosses the wire both ways: a server's creative slot gets the variant
    /// its client picked, and a client's tooltip names the server's.
    #[test]
    fn a_painting_keeps_its_variant_across_the_wire() {
        use horizon_protocol::buf::{Codec, Reader, Writer};
        use horizon_protocol::item::Slot;
        let mut components = vec![String::new(); 110];
        components[109] = "minecraft:painting/variant".into();
        let items = ["minecraft:air", "minecraft:painting"];
        let builtin = Builtin {
            registries: [
                (
                    "minecraft:item".to_owned(),
                    Ids::from_names(items.iter().map(|s| s.to_string()).collect()),
                ),
                (
                    "minecraft:data_component_type".to_owned(),
                    Ids::from_names(components),
                ),
            ]
            .into(),
        };
        let variants = ["minecraft:alban", "minecraft:kebab"];
        let dynamic = Dynamic {
            registries: [(
                "minecraft:painting_variant".to_owned(),
                variants.iter().map(|v| (v.to_string(), None)).collect(),
            )]
            .into(),
            ..Default::default()
        };
        // ItemStack.OPTIONAL_STREAM_CODEC: count 1, the painting, one added
        // component (109: the reference to kebab, its id plus one), none removed.
        let vanilla = [1u8, 1, 1, 0, 109, 2];
        let stack = Slot::read(&mut Reader::new(&vanilla)).unwrap().unwrap();
        let back = from_network(&stack, &builtin, &dynamic);
        let variant = |s: &ItemStack| {
            s.data()
                .and_then(|d| d.other("minecraft:painting/variant"))
                .cloned()
        };
        assert_eq!(variant(&back), Some(Tag::String("minecraft:kebab".into())));
        let mut alban = ItemStack::new("minecraft:painting", 1);
        alban.data_mut().set_other(
            "minecraft:painting/variant",
            Some(Tag::String("minecraft:alban".into())),
        );
        let wire = crate::lan::guest::stack_to_network(
            |name| items.iter().position(|i| *i == name).unwrap_or(0) as i32,
            |registry, name| dynamic.id(registry, name),
            &Some(alban.clone()),
        );
        let mut w = Writer::new();
        Slot::write(&wire, &mut w);
        #[cfg(not(feature = "ae2-lab"))]
        assert_eq!(w.data, [1, 1, 1, 0, 109, 1]);
        let wire = Slot::read(&mut Reader::new(&w.data)).unwrap().unwrap();
        assert_eq!(
            variant(&from_network(&wire, &builtin, &dynamic)),
            variant(&alban)
        );
    }
}
