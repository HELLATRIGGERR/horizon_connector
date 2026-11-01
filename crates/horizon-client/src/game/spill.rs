//! Block entities removed by the block behaviour (`LevelChunk.setBlockState`
//! running `BlockEntity.preRemoveSideEffects`) and the resources a destroyed
//! block drops (`Block.dropResources`), drawn from the level random at the
//! moment vanilla draws them. The item entities they make are added to the
//! world as soon as the behaviour returns, within the same tick.
use super::{Game, GameLevel, Screen, dispenser::new_item_motion, storage::is_shulker_box};
use horizon_client::{
    block_entity_item, block_entity_nbt,
    containers::{Banner, BlockEntity},
    furnace::FurnaceCooking,
    item_data,
    items::ItemStack,
    save,
};
use horizon_core::position::BlockPos;
use horizon_core::random::RandomSource;
use horizon_world::{
    level::{
        LevelEvent,
        removal::{self, PreRemove},
    },
    nbt::{Compound, JavaString, Tag},
    registry::BlockStateId,
};
use std::collections::HashMap;

/// Source AEItemKey.addDrops creates natural-size stacks until the shared
/// drop list already contains more than 1000 stacks. Fluids produce no item.
#[cfg(feature = "ae2-lab")]
pub(super) fn append_ae2_key_drops(out:&mut Vec<ItemStack>,prototype:&ItemStack,mut amount:u64,max:&impl Fn(&ItemStack)->u32) {
    let limit=u64::from(max(prototype).max(1));
    while amount>0 && out.len()<=1000 {
        let count=amount.min(limit);out.push(ItemStack{count:count as u32,..prototype.clone()});amount-=count;
    }
    if amount>0 {log::warn!("AE2 source drop limit omitted {amount} of {}",prototype.item);}
}
#[cfg(feature = "ae2-lab")]
fn append_provider_custody(out:&mut Vec<ItemStack>,config:&horizon_world::nbt::Compound,max:&impl Fn(&ItemStack)->u32) {
    if config.contains_key(&horizon_client::ae2_provider_queue::QUEUE.into()) {
        if let Ok(entries)=horizon_client::ae2_provider_queue::read(config) {
            for entry in entries {
                if let horizon_client::ae2_provider_queue::Entry::Item{prototype,amount}=entry {append_ae2_key_drops(out,&prototype,amount,max);}
            }
        }
    } else if let Some(Tag::List(entries))=config.get(&"ae2_provider_send_items".into()) {
        if entries.len()<=8192 {
            for entry in entries {
                if matches!(entry.get("count"),Some(Tag::Int(n)) if *n>0) {
                    if let Some(item)=horizon_client::save::item_from_nbt(entry) {append_ae2_key_drops(out,&item,u64::from(item.count),max);}
                }
            }
        }
    }
    if let Some(Tag::List(entries))=config.get(&"ae2_provider_return_items".into()) {
        for entry in entries.iter().take(9) {
            if matches!(entry.get("count"),Some(Tag::Int(n)) if *n>0) {
                if let Some(item)=horizon_client::save::item_from_nbt(entry) {append_ae2_key_drops(out,&item,u64::from(item.count),max);}
            }
        }
    }
}
/// Physical machine slots and installed parts only. Terminal listings are
/// virtual and never stored in these slots. Cable type is not recorded yet.
#[cfg(feature = "ae2-lab")]
pub(super) fn ae2_physical_drops_with_max(machine:&horizon_client::ae2_world::Machine,max:&impl Fn(&ItemStack)->u32)->Vec<ItemStack> {
    ae2_collect_physical_drops(machine,max,false)
}
/// Wrench removal adds the selected part item and its contents to one list,
/// unlike cable-block removal, whose part loot is collected separately.
#[cfg(feature = "ae2-lab")]
pub(super) fn ae2_physical_part_drops_with_max(machine:&horizon_client::ae2_world::Machine,max:&impl Fn(&ItemStack)->u32)->Vec<ItemStack> {
    ae2_collect_physical_drops(machine,max,true)
}
#[cfg(feature = "ae2-lab")]
fn ae2_collect_physical_drops(machine:&horizon_client::ae2_world::Machine,max:&impl Fn(&ItemStack)->u32,wrenched_part:bool)->Vec<ItemStack> {
    use horizon_client::ae2_world::MachineKind;
    let mut out=machine.physical_slots_and_upgrades();
    let mut additional=Vec::new();
    if matches!(machine.kind,MachineKind::CraftingUnit|MachineKind::CraftingAccelerator|MachineKind::CraftingStorage1k|MachineKind::CraftingStorage4k|MachineKind::CraftingStorage16k|MachineKind::CraftingStorage64k|MachineKind::CraftingStorage256k|MachineKind::CraftingMonitor) {
        let mut counts=std::collections::BTreeMap::<String,(ItemStack,u64)>::new();
        if let Some(Tag::List(entries))=machine.config.get(&"ae2_cpu_escrow".into()) {
            for entry in entries {
                if !matches!(entry.get("count"),Some(Tag::Int(n)) if *n>0) {continue;}
                let Some(mut item)=horizon_client::save::item_from_nbt(entry) else{continue;};
                let amount=u64::from(item.count);item.count=1;
                let key=horizon_client::item_data::canon_stack(Some(&item));
                let value=counts.entry(key).or_insert((item,0));value.1=value.1.saturating_add(amount);
            }
        }
        // CraftingBlockEntity uses a fresh drop list for each key.
        for (_, (item,amount)) in counts {
            let mut key_drops=Vec::new();append_ae2_key_drops(&mut key_drops,&item,amount,max);out.extend(key_drops);
        }
    }
    if machine.kind==MachineKind::PatternProvider {append_provider_custody(&mut out,&machine.config,max);}
    if machine.kind == horizon_client::ae2_world::MachineKind::CableBus {
        let cable = machine
            .config
            .get(&"cable".into())
            .filter(|tag| tag.get("count") == Some(&Tag::Int(1)))
            .and_then(horizon_client::save::item_from_nbt)
            .filter(|stack| {
                if stack.count != 1 {
                    return false;
                }
                let Some(id) = stack.item.strip_prefix("ae2:") else {
                    return false;
                };
                let kinds = [
                    "glass_cable",
                    "covered_cable",
                    "smart_cable",
                    "covered_dense_cable",
                    "smart_dense_cable",
                ];
                kinds.iter().any(|kind| {
                    id.strip_suffix(kind)
                        .and_then(|p| p.strip_suffix('_'))
                        .is_some_and(|color| {
                            matches!(
                                color,
                                "fluix"
                                    | "white"
                                    | "orange"
                                    | "magenta"
                                    | "light_blue"
                                    | "yellow"
                                    | "lime"
                                    | "pink"
                                    | "gray"
                                    | "light_gray"
                                    | "cyan"
                                    | "purple"
                                    | "blue"
                                    | "brown"
                                    | "green"
                                    | "red"
                                    | "black"
                            )
                        })
                })
            });
        if let Some(cable) = cable {
            out.push(cable);
        }
    }
    if let Some(Tag::List(parts)) = machine.config.get(&"parts".into()) {
        // CableBusContainer.addAdditionalDrops iterates Platform.DIRECTIONS_WITH_NULL,
        // not the insertion order used by the native part storage.
        let mut ordered:Vec<_>=parts.iter().collect();
        ordered.sort_by_key(|part|match part.get("face").and_then(|tag|match tag {Tag::String(face)=>face.to_string().ok(),_=>None}).as_deref() {
            Some("down")=>0,Some("up")=>1,Some("north")=>2,Some("south")=>3,Some("west")=>4,Some("east")=>5,_=>6,
        });
        for part in ordered {
            let Some(Tag::String(id)) = part.get("id") else {
                continue;
            };
            let Ok(id) = id.to_string() else {
                continue;
            };
            if !matches!(
                id.as_str(),
                "ae2:terminal"
                    | "ae2:crafting_terminal"
                    | "ae2:pattern_encoding_terminal"
                    | "ae2:import_bus"
                    | "ae2:export_bus"
                    | "ae2:storage_bus"
                    | "ae2:level_emitter"
                    | "ae2:energy_level_emitter"
                    | "ae2:toggle_bus"
                    | "ae2:inverted_toggle_bus"
                    | "ae2:annihilation_plane"
                    | "ae2:formation_plane"
                    | "ae2:monitor" | "ae2:semi_dark_monitor" | "ae2:dark_monitor"
                    | "ae2:cable_interface" | "ae2:cable_pattern_provider"
                    | "ae2:pattern_access_terminal" | "ae2:cable_energy_acceptor"
                    | "ae2:quartz_fiber" | "ae2:cable_anchor"
                    | "ae2:me_p2p_tunnel" | "ae2:item_p2p_tunnel"
                    | "ae2:fluid_p2p_tunnel" | "ae2:fe_p2p_tunnel"
                    | "ae2:redstone_p2p_tunnel" | "ae2:light_p2p_tunnel"
            ) {
                continue;
            }
            let saved = part
                .get("stack")
                .filter(|tag| tag.get("count") == Some(&Tag::Int(1)))
                .and_then(horizon_client::save::item_from_nbt)
                .filter(|stack| stack.count == 1 && stack.item.as_ref() == id);
            let part_item=saved.unwrap_or_else(|| ItemStack {
                item: id.as_str().into(),
                count: 1,
                damage: 0,
                extra: None,
            });
            if wrenched_part {additional.push(part_item)} else {out.push(part_item)}
            // Interface and provider part machines own physical, slot-tagged
            // inventories separate from virtual filters and fluid return data.
            if matches!(id.as_str(),"ae2:cable_interface"|"ae2:cable_pattern_provider"|"ae2:crafting_terminal"|"ae2:pattern_encoding_terminal") {
                if let Some(machine)=part.get("machine") {
                    for (key,limit) in [("slots",if id=="ae2:pattern_encoding_terminal" {2} else {9}),("upgrades",if id=="ae2:cable_interface" {1} else {0})] {
                        if let Ok(slots)=super::ae2_parts::strict_part_items(machine.get(key),limit) {
                            additional.extend(slots.into_iter().flatten());
                        }
                    }
                    if id=="ae2:cable_pattern_provider" {
                        if let Some(Tag::Compound(config))=machine.get("config") {append_provider_custody(&mut additional,config,max);}
                    }
                }
            }
            // These cells are physical inventory owned by each terminal face.
            if matches!(id.as_str(), "ae2:terminal" | "ae2:crafting_terminal" | "ae2:pattern_encoding_terminal") {
                let tag = part.get("machine").and_then(|m| m.get("config")).and_then(|c| c.get("ae2_view_cells"));
                if let Ok(cells) = super::ae2_parts::decode_terminal_view_cells(tag) {
                    additional.extend(cells.into_iter().flatten());
                }
            }
            // Bus upgrades are real cards, distinct from state_json filters.
            if !matches!(id.as_str(),"ae2:cable_interface"|"ae2:cable_pattern_provider"|"ae2:crafting_terminal"|"ae2:pattern_encoding_terminal") && let Some(Tag::List(upgrades)) = part.get("upgrades") {
                if let Ok(slots)=horizon_client::ae2_upgrade_inventory::decode(upgrades,5) {
                    additional.extend(slots.into_iter().flatten());
                }
            }
        }
    }
    if machine.kind == horizon_client::ae2_world::MachineKind::CableBus {
        let mut faces=std::collections::HashSet::new();
        if let Some(Tag::List(facades))=machine.config.get(&"facades".into()) {
            for facade in facades {
                let Some(Tag::String(face))=facade.get("face") else {continue};
                let Ok(face)=face.to_string() else {continue};
                if !matches!(face.as_str(),"down"|"up"|"north"|"south"|"west"|"east") || faces.contains(&face) {continue;}
                let Some(stack)=facade.get("stack").filter(|n|n.get("count")==Some(&Tag::Int(1))).and_then(horizon_client::save::item_from_nbt) else {continue};
                if stack.count!=1 || stack.item.as_ref()!="ae2:facade" {continue;}
                if horizon_client::item_tooltip::ae2_facade_texture(&stack).is_none() {continue;}
                faces.insert(face);
                out.push(stack);
            }
        }
    }
    out.extend(additional);
    out
}
#[cfg(all(feature="ae2-lab",test))]
fn ae2_physical_drops(machine:&horizon_client::ae2_world::Machine)->Vec<ItemStack> {
    ae2_physical_drops_with_max(machine,&|_|64)
}

/// Cable bus is a multipart host, not an additional physical block item.
#[cfg(feature = "ae2-lab")]
pub(super) fn ae2_synthetic_cable_host_drop(block: &str, stack: &ItemStack) -> bool {
    block == "ae2:cable_bus" && stack.item.as_ref() == "ae2:cable_bus"
}

/// What a broken block's entity gives its own dropped item (`copy_components`
/// in the block loot tables): a shulker box's contents, a banner's layers, a
/// container's custom name.
#[derive(Clone, Default)]
pub(super) struct Pending {
    pub(super) shulker: HashMap<[i32; 3], Vec<(u8, ItemStack)>>,
    pub(super) banner: HashMap<[i32; 3], Banner>,
    /// The block's item and the custom name.
    pub(super) names: HashMap<[i32; 3], (String, String)>,
    /// A decorated pot's sherds (back, left, right, front).
    pub(super) pot: HashMap<[i32; 3], [Option<ItemStack>; 4]>,
    /// A head's profile, note block sound and name, or a hive's bees: the
    /// dropped item and the component patch it takes.
    pub(super) copied: HashMap<[i32; 3], (String, Compound)>,
}
impl Pending {
    /// Remember what the block entity at `pos` copies into the drops of `block`.
    pub(super) fn record(
        &mut self,
        pos: [i32; 3],
        entity: &BlockEntity,
        block: &str,
        name: Option<String>,
    ) {
        if let BlockEntity::Banner(banner) = entity {
            self.banner.insert(pos, banner.clone());
            return;
        }
        if let Some(name) = name {
            self.names.insert(pos, (block.to_owned(), name));
        }
        // `copy_components` from the block entity: the head tables copy the
        // profile (player heads), note block sound and name, the hive tables
        // the bees.
        let include: &[&str] = match entity {
            BlockEntity::Skull(_) if block.starts_with("minecraft:player_") => &[
                "minecraft:profile",
                "minecraft:note_block_sound",
                "minecraft:custom_name",
            ],
            BlockEntity::Skull(_) => &["minecraft:custom_name"],
            BlockEntity::Beehive(_) => &["minecraft:bees"],
            _ => &[],
        };
        if !include.is_empty() {
            let saved = save::encode_block_entity((pos[0], pos[1], pos[2]), entity, block);
            let mut patch = block_entity_nbt::canonical(&saved)
                .map(|saved| block_entity_item::pick(&saved))
                .unwrap_or_default();
            patch.retain(|k, _| include.contains(&k.to_string_lossy().as_str()));
            if !patch.is_empty() {
                // `Blocks.wallVariant`: a wall head drops the standing item.
                let item = block.replacen("_wall_", "_", 1);
                self.copied.insert(pos, (item, patch));
            }
        }
        if is_shulker_box(block)
            && let BlockEntity::Chest(items) = entity
        {
            let kept = items
                .iter()
                .enumerate()
                .filter_map(|(i, s)| Some((i as u8, s.clone()?)))
                .collect();
            self.shulker.insert(pos, kept);
        }
    }
    /// Copy what was remembered for `pos` into a drop of the block.
    pub(super) fn attach(&mut self, pos: [i32; 3], stack: &mut ItemStack) {
        if is_shulker_box(&stack.item)
            && let Some(contents) = self.shulker.remove(&pos)
            && !contents.is_empty()
        {
            stack.data_mut().container = contents;
        }
        if self
            .names
            .get(&pos)
            .is_some_and(|(item, _)| **item == *stack.item)
            && let Some((_, name)) = self.names.remove(&pos)
        {
            stack.data_mut().custom_name =
                Some(horizon_world::nbt::Tag::String(name.as_str().into()));
        }
        if self
            .copied
            .get(&pos)
            .is_some_and(|(item, _)| **item == *stack.item)
            && let Some((_, copied)) = self.copied.remove(&pos)
        {
            let mut patch = match item_data::encode_components(stack) {
                Some(Tag::Compound(c)) => c,
                _ => Default::default(),
            };
            patch.extend(copied);
            (stack.damage, stack.extra) = item_data::decode_components(Some(&Tag::Compound(patch)));
        }
        if &*stack.item == "minecraft:decorated_pot" {
            // The pot's block entity always copies its decorations, empty or not.
            let sides = self.pot.remove(&pos).unwrap_or_default();
            stack.data_mut().pot_decorations = Some(Box::new(sides));
        }
        if stack.item.ends_with("_banner")
            && let Some(Banner {
                layers,
                name,
                components,
            }) = self.banner.remove(&pos)
        {
            let mut patch = match item_data::encode_components(stack) {
                Some(Tag::Compound(c)) => c,
                _ => Default::default(),
            };
            // The remaining components in the banner loot table's copy_components.
            for key in [
                "minecraft:item_name",
                "minecraft:rarity",
                "minecraft:tooltip_display",
            ] {
                if let Some(value) = components.get(&JavaString::from(key)) {
                    patch.insert(key.into(), value.clone());
                }
            }
            patch.insert(
                "minecraft:banner_patterns".into(),
                horizon_client::banner::layers_to_nbt(&layers),
            );
            if let Some(name) = name {
                patch.insert(
                    "minecraft:custom_name".into(),
                    Tag::String(name.as_str().into()),
                );
            }
            (stack.damage, stack.extra) = item_data::decode_components(Some(&Tag::Compound(patch)));
        }
    }
    /// Remember a decorated pot's sherds from its saved block entity.
    pub(super) fn record_saved(&mut self, pos: [i32; 3], tag: &horizon_world::nbt::Tag) {
        use horizon_world::nbt::Tag;
        if tag.get("id") != Some(&Tag::String("minecraft:decorated_pot".into())) {
            return;
        }
        let sherds = tag.get("sherds");
        let side = |name: &str| match sherds.and_then(|s| s.get(name)) {
            Some(Tag::String(id)) => Some(ItemStack {
                item: std::sync::Arc::from(id.to_string_lossy().as_str()),
                count: 1,
                damage: 0,
                extra: None,
            }),
            Some(t) => horizon_client::save::item_from_nbt(t),
            None => None,
        };
        self.pot.insert(
            pos,
            [side("back"), side("left"), side("right"), side("front")],
        );
    }
    /// The block entity's dynamic drops: a decorated pot's sherds, left,
    /// back, front then right (`DecoratedPotBlock.getDrops`).
    pub(super) fn dynamic(&self, pos: [i32; 3]) -> Vec<ItemStack> {
        self.pot
            .get(&pos)
            .map_or_else(Vec::new, |[back, left, right, front]| {
                [left, back, front, right]
                    .into_iter()
                    .flatten()
                    .cloned()
                    .collect()
            })
    }
    pub(super) fn clear(&mut self, pos: [i32; 3]) {
        self.pot.remove(&pos);
        self.shulker.remove(&pos);
        self.banner.remove(&pos);
        self.names.remove(&pos);
        self.copied.remove(&pos);
    }
}

/// An item entity to add: where, its motion (None: `new ItemEntity`'s own
/// random motion) and its pickup delay.
#[derive(Clone)]
pub(super) struct Spawn {
    pub(super) stack: ItemStack,
    pub(super) pos: [f64; 3],
    pub(super) motion: Option<[f64; 3]>,
    pub(super) delay: u32,
}

#[derive(Clone, Default)]
pub(super) struct Spill {
    pub(super) spawns: Vec<Spawn>,
    /// Experience orbs (`ExperienceOrb.award`): where and how much, each
    /// and the merge id it drew from the level random.
    pub(super) orbs: Vec<([f64; 3], u32, i32)>,
    /// Block entities the behaviour removed, for the game's own follow-ups
    /// (open screens, playing songs).
    pub(super) removed: Vec<([i32; 3], BlockEntity)>,
}

impl Spill {
    pub(super) fn is_empty(&self) -> bool {
        self.spawns.is_empty() && self.orbs.is_empty() && self.removed.is_empty()
    }
}

/// `Block.popResource`: a spot around the block's centre from the level random.
pub(super) fn pop_position(
    random: &mut impl horizon_core::random::RandomSource,
    pos: [i32; 3],
) -> [f64; 3] {
    let mut offset = || -0.25 + random.next_double() * 0.5;
    // EntityTypes.ITEM is 0.25 high.
    let half_height = f64::from(0.25f32) / 2.0;
    [
        f64::from(pos[0]) + 0.5 + offset(),
        f64::from(pos[1]) + 0.5 + offset() - half_height,
        f64::from(pos[2]) + 0.5 + offset(),
    ]
}

/// The block entity at `pos` Horizon keeps as it was loaded, if any
/// (campfires, decorated pots, chiseled bookshelves, shelves).
pub(super) fn saved_entity(
    world: &horizon_client::world::World,
    pos: [i32; 3],
) -> Option<horizon_world::nbt::Tag> {
    use horizon_world::nbt::Tag;
    let saved = world
        .chunks
        .get(&(pos[0] >> 4, pos[2] >> 4))?
        .saved
        .as_ref()?;
    saved
        .block_entities
        .iter()
        .find(|t| {
            t.get("x") == Some(&Tag::Int(pos[0]))
                && t.get("y") == Some(&Tag::Int(pos[1]))
                && t.get("z") == Some(&Tag::Int(pos[2]))
        })
        .cloned()
}

/// Keep `tag` as the block entity at its position Horizon does not model,
/// replacing the one there.
pub(super) fn put_saved_entity(
    world: &mut horizon_client::world::World,
    pos: [i32; 3],
    tag: horizon_world::nbt::Tag,
) {
    use horizon_world::nbt::Tag;
    let Some(chunk) = world.chunks.get_mut(&(pos[0] >> 4, pos[2] >> 4)) else {
        return;
    };
    let chunk = std::sync::Arc::make_mut(chunk);
    let saved = std::sync::Arc::make_mut(chunk.saved.get_or_insert_default());
    let at = |t: &Tag| {
        t.get("x") == Some(&Tag::Int(pos[0]))
            && t.get("y") == Some(&Tag::Int(pos[1]))
            && t.get("z") == Some(&Tag::Int(pos[2]))
    };
    match saved.block_entities.iter_mut().find(|t| at(t)) {
        Some(old) => *old = tag,
        None => saved.block_entities.push(tag),
    }
}

impl GameLevel<'_> {
    /// `Block.popResource(level, pos, stack)` with the broken block's copied components.
    fn pop_resource(&mut self, pos: [i32; 3], mut stack: ItemStack) {
        self.pending.attach(pos, &mut stack);
        let at = pop_position(&mut self.world.random, pos);
        self.spill.spawns.push(Spawn {
            stack,
            pos: at,
            motion: None,
            delay: 10,
        });
    }
    /// `Block.popResource(level, pos, new ItemStack(block))`.
    pub(super) fn pop_block_item(&mut self, pos: BlockPos, state: BlockStateId) {
        let item = self.facts.get(state).item.clone();
        if &*item != "minecraft:air" {
            self.pop_resource([pos.x, pos.y, pos.z], ItemStack::new(&item, 1));
        }
    }
    /// An item `atLowerCornerWithOffset(pos, 0.5, 1.01, 0.5).offsetRandomXZ(random,
    /// 0.7)` with the default pickup delay (`ComposterBlock.extractProduce`).
    pub(super) fn pop_above(&mut self, pos: BlockPos, item: std::sync::Arc<str>) {
        let at = removal::jukebox_pop_position(&mut self.world.random, pos);
        self.spill.spawns.push(Spawn {
            stack: ItemStack::new(&item, 1),
            pos: at,
            motion: None,
            delay: 10,
        });
    }
    /// `Block.dropFromBlockInteractLootTable` with `Block.popResource` of
    /// every stack.
    pub(super) fn drop_interact_loot(&mut self, pos: BlockPos, state: BlockStateId, table: &str) {
        let drops = self.items.interact_loot(
            table,
            self.registry,
            state,
            &mut self.world.random,
            self.rand_sequences,
            self.seed,
        );
        for stack in drops {
            if &*stack.item != "minecraft:air" && stack.count > 0 {
                self.pop_resource([pos.x, pos.y, pos.z], stack);
            }
        }
    }
    /// `Block.dropResources(state, level, pos, blockEntity)`: the block's loot
    /// (from its random sequence, else the level random), with what its block
    /// entity copies into it, popped at once.
    pub(super) fn drop_resources_now(&mut self, pos: BlockPos, state: BlockStateId) {
        // CertusQuartzClusterBlock.getDrops requires an entity in the loot
        // context. This native path calls loot without an actor (e.g. support
        // loss); player mining uses its separate actor-bearing path.
        #[cfg(feature = "ae2-lab")]
        if matches!(self.facts.get(state).name.as_ref(),
            "ae2:small_quartz_bud" | "ae2:medium_quartz_bud" |
            "ae2:large_quartz_bud" | "ae2:quartz_cluster") {return;}
        let key = (pos.x, pos.y, pos.z);
        let at = [pos.x, pos.y, pos.z];
        if let Some(entity) = self.world.block_entities.get(&key) {
            let entity = entity.clone();
            let name = self.world.block_names.get(&key).cloned();
            let block = self.facts.get(state).name.clone();
            self.pending.record(at, &entity, &block, name);
        } else if let Some(tag) = saved_entity(self.world, [pos.x, pos.y, pos.z]) {
            self.pending.record_saved(at, &tag);
        }
        let dynamic = self.pending.dynamic(at);
        let drops = match self.items.block_loot_sequence(self.registry, state) {
            Some(sequence) => {
                let random = self.rand_sequences.get(&sequence, self.seed);
                self.items
                    .block_drops_with(self.registry, state, None, &dynamic, random)
            }
            None => self.items.block_drops_with(
                self.registry,
                state,
                None,
                &dynamic,
                &mut self.world.random,
            ),
        };
        for stack in drops {
            #[cfg(feature = "ae2-lab")]
            if ae2_synthetic_cable_host_drop(&self.facts.get(state).name, &stack) { continue; }
            if &*stack.item != "minecraft:air" && stack.count > 0 {
                self.pop_resource(at, stack);
            }
        }
        self.pending.clear(at);
        // BlockBehaviour.spawnAfterBreak with no tool: ores, sculk and spawners
        // award experience at the block's centre.
        let name = self.facts.get(state).name.clone();
        if let Some(xp) = removal::break_experience(&mut self.world.random, &name)
            && self.rules.block_drops
        {
            let centre = [
                f64::from(pos.x) + 0.5,
                f64::from(pos.y) + 0.5,
                f64::from(pos.z) + 0.5,
            ];
            self.award_orbs(centre, xp);
        }
    }
    /// `ExperienceOrb.award`: orbs of the largest sizes, each drawing the id
    /// `tryMergeToExisting` compares from the level random.
    pub(super) fn award_orbs(&mut self, at: [f64; 3], mut amount: u32) {
        while amount > 0 {
            let value = removal::orb_value(amount);
            amount -= value;
            let merge_id = self.world.random.next_int_bound(40).unwrap_or(0);
            self.spill.orbs.push((at, value, merge_id));
        }
    }
    /// `BlockEntity.preRemoveSideEffects` and `removeBlockEntity` for the block
    /// entity of `old` at `pos`. A block entity Horizon never created reads as
    /// vanilla's fresh one: an empty container still draws a spot per slot.
    pub(super) fn remove_entity(
        &mut self,
        pos: BlockPos,
        old: BlockStateId,
        side_effects: bool,
    ) -> u8 {
        let key = (pos.x, pos.y, pos.z);
        // Prefer the live AE2 block entity over an older saved snapshot.
        // Removing the cached tag without side effects prevents double drops.
        #[cfg(feature = "ae2-lab")]
        let live_ae2 = matches!(
            self.world.block_entities.get(&key),
            Some(BlockEntity::Ae2(_))
        );
        #[cfg(not(feature = "ae2-lab"))]
        let live_ae2 = false;
        if live_ae2 {
            self.remove_saved_entity(pos, old, false);
        } else if self.remove_saved_entity(pos, old, side_effects) {
            return 0;
        }
        // Sculk blocks and creaking hearts notice their removal on their own tick.
        if matches!(
            self.world.block_entities.get(&key),
            Some(BlockEntity::Sculk(_) | BlockEntity::CreakingHeart(_))
        ) {
            return 0;
        }
        #[cfg(feature = "ae2-lab")]
        if !self.client_side && matches!(self.world.block_entities.get(&key),Some(BlockEntity::Ae2(m)) if matches!(m.kind,horizon_client::ae2_world::MachineKind::QuantumRing|horizon_client::ae2_world::MachineKind::QuantumLink)) {
            if let (Some(address),Some(host))=(super::ae2_global::GridAddress::from_local(self.dim,key),self.world.block_entities.incarnation(&key)) {
                self.quantum.removed(address.network_pos(),host);
                self.quantum.flush_events(self.world,self.dim,key);
            }
        }
        let entity = self.world.block_entities.remove(&key);
        self.world.block_names.remove(&key);
        self.world.forget_block_entity_source(key);
        let name = self.facts.get(old).name.clone();
        let kind = if live_ae2 {
            PreRemove::Nothing
        } else {
            match &entity {
                // BaseContainerBlockEntity.preRemoveSideEffects: Containers.dropContents.
                Some(BlockEntity::Mod(m))
                    if !m.items.is_empty()
                        && self.world.block_entities.mod_container(old.0).is_some() =>
                {
                    PreRemove::Contents {
                        slots: m.items.len(),
                        furnace: false,
                    }
                }
                _ => removal::pre_remove(&name),
            }
        };
        let mut neighbour_updates = 0;
        if side_effects {
            #[cfg(feature = "ae2-lab")]
            if let Some(BlockEntity::Ae2(machine)) = &entity {
                let drops=ae2_physical_drops_with_max(machine,&|stack| {
                    let default=self.items.get(&stack.item).map_or(64,|info|info.max_stack).max(1);
                    super::ae2_formation::max_stack(stack,default)
                });
                for stack in drops {self.drop_item_stack(pos,Some(stack));}
            }
            match kind {
                PreRemove::Contents { slots, furnace } => {
                    let items = entity.as_ref().map_or(&[][..], BlockEntity::slots);
                    for slot in 0..slots {
                        let stack = items.get(slot).cloned().flatten();
                        self.drop_item_stack(pos, stack);
                    }
                    // AbstractFurnaceBlockEntity.preRemoveSideEffects: then the
                    // experience of what it made, at the block's centre.
                    if furnace && let Some(BlockEntity::Furnace(f)) = &entity {
                        let at = [
                            f64::from(pos.x) + 0.5,
                            f64::from(pos.y) + 0.5,
                            f64::from(pos.z) + 0.5,
                        ];
                        for (count, experience) in f.recipe_experience(self.recipes) {
                            let xp = horizon_client::containers::create_experience(
                                count,
                                experience,
                                &mut self.world.random,
                            );
                            self.award_orbs(at, xp);
                        }
                    }
                }
                PreRemove::Campfire => {
                    let items = match &entity {
                        Some(BlockEntity::Campfire(c)) => c.items.clone(),
                        _ => Default::default(),
                    };
                    for stack in items {
                        self.drop_item_stack(pos, stack);
                    }
                }
                PreRemove::Jukebox => {
                    // popOutTheItem: removeTheItem stops a playing song (its
                    // events and a neighbour update), the record pops, and
                    // onSongChanged updates the neighbours again.
                    if let Some(BlockEntity::Jukebox(j)) = &entity
                        && let Some(record) = j.record().cloned()
                    {
                        if j.playing {
                            self.jukebox_stopped(pos, old);
                            neighbour_updates += 1;
                        }
                        let at = removal::jukebox_pop_position(&mut self.world.random, pos);
                        self.spill.spawns.push(Spawn {
                            stack: record,
                            pos: at,
                            motion: None,
                            delay: 10,
                        });
                        neighbour_updates += 1;
                    }
                }
                PreRemove::Lectern => {
                    let props = self.registry.state(old).map(|s| &s.properties);
                    let has_book = props
                        .and_then(|p| p.get("has_book"))
                        .is_some_and(|v| v == "true");
                    let step = match props.and_then(|p| p.get("facing")).map(String::as_str) {
                        Some("north") => [0, -1],
                        Some("south") => [0, 1],
                        Some("west") => [-1, 0],
                        _ => [1, 0],
                    };
                    if has_book
                        && let Some(BlockEntity::Lectern(l)) = &entity
                        && let Some(book) = l.book.clone()
                    {
                        self.spill.spawns.push(Spawn {
                            stack: book,
                            pos: removal::lectern_book_position(pos, step),
                            motion: None,
                            delay: 10,
                        });
                    }
                }
                PreRemove::ShulkerBox
                | PreRemove::SculkShrieker
                | PreRemove::CreakingHeart
                | PreRemove::Beacon
                | PreRemove::Nothing => {}
            }
        }
        // BlockEntity.setRemoved: a jukebox always reports its song stopping,
        // a beacon plays its deactivation.
        match kind {
            PreRemove::Jukebox => self.jukebox_stopped(pos, old),
            PreRemove::Beacon => self.events.push(LevelEvent::Sound {
                pos,
                sound: "minecraft:block.beacon.deactivate".into(),
                volume: 1.0,
                pitch: 1.0,
            }),
            _ => {}
        }
        if let Some(entity) = entity {
            self.spill.removed.push(([pos.x, pos.y, pos.z], entity));
        }
        neighbour_updates
    }
    /// The jukebox's `JUKEBOX_STOP_PLAY` game event and level event 1011.
    fn jukebox_stopped(&mut self, pos: BlockPos, state: BlockStateId) {
        self.events.push(LevelEvent::GameEvent {
            pos,
            event: "minecraft:jukebox_stop_play",
            state: Some(state),
        });
        self.events.push(LevelEvent::LevelEvent {
            id: 1011,
            pos,
            data: 0,
        });
    }
    /// A block entity Horizon keeps as it was loaded (campfires, decorated
    /// pots, chiseled bookshelves, shelves): its items spill as vanilla's do.
    fn remove_saved_entity(
        &mut self,
        pos: BlockPos,
        old: BlockStateId,
        side_effects: bool,
    ) -> bool {
        use horizon_world::nbt::Tag;
        let chunk_key = (pos.x >> 4, pos.z >> 4);
        let at = |t: &Tag| {
            let int = |k: &str| match t.get(k) {
                Some(Tag::Int(v)) => Some(*v),
                _ => None,
            };
            int("x") == Some(pos.x) && int("y") == Some(pos.y) && int("z") == Some(pos.z)
        };
        let Some(chunk) = self.world.chunks.get_mut(&chunk_key) else {
            return false;
        };
        let Some(saved) = chunk.saved.as_ref() else {
            return false;
        };
        let Some(index) = saved.block_entities.iter().position(at) else {
            return false;
        };
        let chunk = std::sync::Arc::make_mut(chunk);
        let Some(saved) = chunk.saved.as_mut() else {
            return false;
        };
        let tag = std::sync::Arc::make_mut(saved).block_entities.remove(index);
        if !side_effects {
            return true;
        }
        #[cfg(feature = "ae2-lab")]
        if let Some((_, BlockEntity::Ae2(machine))) =
            horizon_client::save::decode_block_entity(&tag)
        {
            let drops=ae2_physical_drops_with_max(&machine,&|stack| {
                let default=self.items.get(&stack.item).map_or(64,|info|info.max_stack).max(1);
                super::ae2_formation::max_stack(stack,default)
            });
            for stack in drops {self.drop_item_stack(pos,Some(stack));}
            return true;
        }
        let name = self.facts.get(old).name.clone();
        let items = match removal::pre_remove(&name) {
            PreRemove::Contents { slots, .. } if &*name == "minecraft:decorated_pot" => {
                // Containers.dropContents reads getTheItem, which unpacks the loot table.
                let mut tag = tag;
                super::decorated_pot::unpack_loot(
                    &mut tag,
                    self.loot.as_deref(),
                    &mut self.world.random,
                    self.rand_sequences,
                    self.seed,
                );
                let mut items = vec![None; slots];
                items[0] = tag
                    .get("item")
                    .and_then(horizon_client::save::item_from_nbt);
                items
            }
            PreRemove::Contents { slots, .. } => {
                horizon_client::save::decode_items(tag.get("Items"), slots)
            }
            PreRemove::Campfire => horizon_client::save::decode_items(tag.get("Items"), 4),
            _ => return true,
        };
        for stack in items {
            self.drop_item_stack(pos, stack);
        }
        true
    }
    /// `Containers.dropContents` for a mod block entity that left the world
    /// during its own tick (the block broke itself): what it held spills as
    /// vanilla's `preRemoveSideEffects` would, instead of vanishing with it.
    pub(super) fn drop_lost_contents(&mut self, pos: BlockPos, items: Vec<Option<ItemStack>>) {
        for stack in items {
            self.drop_item_stack(pos, stack);
        }
    }
    /// `Containers.dropItemStack` of one slot (an empty one still draws its spot).
    fn drop_item_stack(&mut self, pos: BlockPos, stack: Option<ItemStack>) {
        let count = stack.as_ref().map_or(0, |s| s.count);
        for piece in removal::drop_item_stack(&mut self.world.random, pos, count) {
            let Some(stack) = &stack else { break };
            self.spill.spawns.push(Spawn {
                stack: ItemStack {
                    count: piece.count,
                    ..stack.clone()
                },
                pos: piece.pos,
                motion: Some(piece.motion),
                delay: 0,
            });
        }
    }
}

impl Game<'_> {
    /// Add the item entities the block behaviour spawned and follow up on the
    /// block entities it removed.
    pub(super) fn flush_spill(&mut self) {
        if self.spill.is_empty() {
            return;
        }
        let spill = std::mem::take(&mut self.spill);
        for s in spill.spawns {
            let motion = s.motion.unwrap_or_else(new_item_motion);
            self.spawn_item(s.stack, s.pos, motion, s.delay);
        }
        for (at, value, merge_id) in spill.orbs {
            self.award_orb(at, value, merge_id);
        }
        for (pos, entity) in spill.removed {
            let key = (pos[0], pos[1], pos[2]);
            if matches!(entity, BlockEntity::Jukebox(_)) {
                self.jukebox_removed(pos);
            }
            if self.container == Some(key) {
                #[cfg(feature = "ae2-lab")]
                if matches!(entity, BlockEntity::Ae2(_)) {
                    // Return a real cursor item before closing the virtual view.
                    self.close_container();
                    self.screen = Screen::Playing;
                    continue;
                }
                self.container_opener_event(key, false);
                self.container = None;
                self.screen = Screen::Playing;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature="ae2-lab")]
    #[test]
    fn ae2_provider_drop_limit_is_shared_across_entries_and_return_inventory() {
        use horizon_client::{ae2_provider_queue::{self,Entry},ae2_world::{Machine,MachineKind}};
        let mut provider=Machine::new(MachineKind::PatternProvider);
        provider.slots[0]=Some(ItemStack::new("ae2:processing_pattern",1));
        let first=ItemStack::new("minecraft:stone",1);let second=ItemStack::new("minecraft:diamond",1);
        ae2_provider_queue::write(&mut provider.config,&[
            Entry::Item{prototype:first.clone(),amount:999*64},Entry::Item{prototype:second.clone(),amount:129},
        ]).unwrap();
        provider.config.insert("ae2_provider_return_items".into(),Tag::List(vec![horizon_client::save::item_to_nbt(&ItemStack::new("minecraft:emerald",1))]));
        let drops=ae2_physical_drops_with_max(&provider,&|_|64);
        assert_eq!(drops.len(),1001);assert_eq!(drops[0].item.as_ref(),"ae2:processing_pattern");
        assert_eq!(drops.iter().filter(|item|item.same_item_same_components(&first)).map(|item|u64::from(item.count)).sum::<u64>(),999*64);
        assert_eq!(drops.iter().filter(|item|item.same_item_same_components(&second)).map(|item|u64::from(item.count)).sum::<u64>(),64);
        assert!(!drops.iter().any(|item|item.item.as_ref()=="minecraft:emerald"));
    }
    #[cfg(feature="ae2-lab")]
    #[test]
    fn cable_break_additional_limit_follows_source_face_order() {
        use horizon_client::{ae2_provider_queue::{self,Entry},ae2_world::{Machine,MachineKind}};
        let make_part=|face:&str,item:&str,amount:u64| {
            let mut config=Default::default();
            ae2_provider_queue::write(&mut config,&[Entry::Item{prototype:ItemStack::new(item,1),amount}]).unwrap();
            Tag::compound([("id",Tag::String("ae2:cable_pattern_provider".into())),
                ("face",Tag::String(face.into())),("machine",Tag::compound([("config",Tag::Compound(config))]))])
        };
        let mut cable=Machine::new(MachineKind::CableBus);
        // Native persistence order is deliberately opposite the Java side order.
        cable.config.insert("parts".into(),Tag::List(vec![make_part("south","minecraft:diamond",64),make_part("north","minecraft:stone",999_999)]));
        let drops=ae2_physical_drops_with_max(&cable,&|_|64);
        assert_eq!(drops.iter().filter(|stack|stack.item.as_ref()=="ae2:cable_pattern_provider").count(),2);
        assert_eq!(drops.iter().filter(|stack|stack.item.as_ref()=="minecraft:stone").count(),1001);
        assert!(!drops.iter().any(|stack|stack.item.as_ref()=="minecraft:diamond"));
    }
    #[cfg(feature="ae2-lab")]
    #[test]
    fn ae2_cpu_drop_limit_uses_a_fresh_list_per_exact_key_and_max_stack() {
        use horizon_client::ae2_world::{Machine,MachineKind};
        let mut cpu=Machine::new(MachineKind::CraftingStorage256k);
        let mut first=ItemStack::new("minecraft:stone",2_002);first.data_mut().custom_name = Some(horizon_client::item_data::text_name("one"));
        let mut second=ItemStack::new("minecraft:stone",2_002);second.data_mut().custom_name = Some(horizon_client::item_data::text_name("two"));
        cpu.config.insert("ae2_cpu_escrow".into(),Tag::List(vec![horizon_client::save::item_to_nbt(&first),horizon_client::save::item_to_nbt(&second)]));
        let drops=ae2_physical_drops_with_max(&cpu,&|_|2);
        assert_eq!(drops.len(),2_002);assert!(drops.iter().all(|item|item.count==2));
        assert_eq!(drops.iter().filter(|item|item.same_item_same_components(&first)).count(),1_001);
        assert_eq!(drops.iter().filter(|item|item.same_item_same_components(&second)).count(),1_001);
    }
    use horizon_client::{block_entity_item, item_data, mobs::raider, save};
    use horizon_world::nbt::Tag;

    #[cfg(feature = "ae2-lab")]
    #[test]
    fn terminal_faces_drop_only_their_physical_view_cells() {
        use horizon_client::ae2_world::{Machine, MachineKind};
        let mut cell = ItemStack::new("ae2:view_cell", 1);
        cell.data_mut().custom_name = Some(horizon_client::item_data::text_name("owned cell"));
        for (id, kind) in [("ae2:terminal", MachineKind::Terminal), ("ae2:crafting_terminal", MachineKind::CraftingTerminal), ("ae2:pattern_encoding_terminal", MachineKind::PatternEncodingTerminal)] {
            let mut terminal = Machine::new(kind);
            super::super::ae2_parts::ae2_set_terminal_view_cells(&mut terminal, &[None, Some(cell.clone()), None, None, None]).unwrap();
            let mut cable = Machine::new(MachineKind::CableBus);
            cable.config.insert("parts".into(), Tag::List(vec![Tag::compound([
                ("id", Tag::String(id.into())),
                ("machine", Tag::compound([("config", Tag::Compound(terminal.config))])),
            ])]));
            assert_eq!(ae2_physical_drops(&cable), vec![ItemStack::new(id, 1), cell.clone()]);
        }
    }
    #[cfg(feature = "ae2-lab")]
    #[test]
    fn ae2_drop_list_preserves_physical_cells_and_parts_after_save() {
        use horizon_client::ae2_world::{Machine, MachineKind};
        let mut drive = Machine::new(MachineKind::Drive);
        let mut cell = ItemStack::new("ae2:item_storage_cell_1k", 1);
        cell.data_mut().set_other(
            "minecraft:custom_data",
            Some(Tag::compound([("payload", Tag::Int(7))])),
        );
        drive.slots[0] = Some(cell.clone());
        let encoded =
            save::encode_block_entity((1, 64, 2), &BlockEntity::Ae2(Box::new(drive)), "ae2:drive");
        let (_, BlockEntity::Ae2(loaded)) = save::decode_block_entity(&encoded).unwrap() else {
            panic!("AE2 saved entity")
        };
        assert_eq!(ae2_physical_drops(&loaded), vec![cell]);
        let mut cable = Machine::new(MachineKind::CableBus);
        let mut physical_cable = ItemStack::new("ae2:light_blue_smart_dense_cable", 1);
        physical_cable.data_mut().custom_name = Some(horizon_client::item_data::text_name("Named cable"));
        cable
            .config
            .insert("cable".into(), save::item_to_nbt(&physical_cable));
        let mut named_terminal = ItemStack::new("ae2:terminal", 1);
        named_terminal.data_mut().custom_name = Some(horizon_client::item_data::text_name("Storage"));
        cable.config.insert(
            "parts".into(),
            Tag::List(vec![
                Tag::compound([
                    ("id", Tag::String("ae2:terminal".into())),
                    ("face", Tag::String("north".into())),
                    ("stack", save::item_to_nbt(&named_terminal)),
                ]),
                Tag::compound([
                    ("id", Tag::String("ae2:import_bus".into())),
                    ("face", Tag::String("south".into())),
                    (
                        "upgrades",
                        Tag::List(vec![save::item_to_nbt(&ItemStack::new(
                            "ae2:speed_card",
                            1,
                        ))]),
                    ),
                ]),
            ]),
        );
        let drops = ae2_physical_drops(&cable);
        assert_eq!(
            drops.iter().map(|s| s.item.as_ref()).collect::<Vec<_>>(),
            vec![
                "ae2:light_blue_smart_dense_cable",
                "ae2:terminal",
                "ae2:import_bus",
                "ae2:speed_card"
            ]
        );
        assert_eq!(drops[0], physical_cable);
        assert_eq!(drops[1], named_terminal);
        let mut facade=ItemStack::new("ae2:facade",1);
        facade.data_mut().custom_name = Some(horizon_client::item_data::text_name("Preserved facade"));
        facade.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("ae2:facade_item",Tag::String("minecraft:stone".into()))])));
        cable.config.insert("facades".into(),Tag::List(vec![
            Tag::compound([("face",Tag::String("north".into())),("stack",save::item_to_nbt(&facade))]),
            Tag::compound([("face",Tag::String("north".into())),("stack",save::item_to_nbt(&facade))]),
        ]));
        assert_eq!(ae2_physical_drops(&cable).iter().filter(|s|s.item.as_ref()=="ae2:facade").cloned().collect::<Vec<_>>(),vec![facade]);
        let mut source_facade=ItemStack::new("ae2:facade",1);
        source_facade.data_mut().set_other("ae2:facade_item",Some(Tag::String("minecraft:stone".into())));
        cable.config.insert("facades".into(),Tag::List(vec![Tag::compound([("face",Tag::String("south".into())),("stack",save::item_to_nbt(&source_facade))])]));
        assert_eq!(ae2_physical_drops(&cable).iter().filter(|s|s.item.as_ref()=="ae2:facade").cloned().collect::<Vec<_>>(),vec![source_facade]);
        let mut nested=Machine::new(MachineKind::CableBus);
        let physical=ItemStack::new("minecraft:iron_ingot",3);
        let sending=ItemStack::new("minecraft:diamond",2);
        nested.config.insert("parts".into(),Tag::List(vec![Tag::compound([
            ("id",Tag::String("ae2:cable_pattern_provider".into())),
            ("stack",save::item_to_nbt(&ItemStack::new("ae2:cable_pattern_provider",1))),
            ("machine",Tag::compound([("slots",Tag::List(vec![])),("upgrades",Tag::List(vec![])),
                ("config",Tag::compound([("ae2_provider_return_items",Tag::List(vec![save::item_to_nbt(&physical)])),("ae2_provider_send_items",Tag::List(vec![save::item_to_nbt(&sending)])),("ae2_provider_send_fluids",Tag::List(vec![Tag::compound([("fluid",Tag::String("minecraft:water".into()))])]))]))])),
        ])]));
        assert_eq!(ae2_physical_drops(&nested),vec![ItemStack::new("ae2:cable_pattern_provider",1),sending,physical]);
        let mut queued=ItemStack::new("minecraft:stone",1);
        queued.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("variant",Tag::String("exact".into()))])));
        let mut queue_config=Default::default();
        horizon_client::ae2_provider_queue::write(&mut queue_config,&[horizon_client::ae2_provider_queue::Entry::Item{prototype:queued.clone(),amount:999_999}]).unwrap();
        nested.config.insert("parts".into(),Tag::List(vec![Tag::compound([
            ("id",Tag::String("ae2:cable_pattern_provider".into())),
            ("machine",Tag::compound([("config",Tag::Compound(queue_config))]))
        ])]));
        // Block loot and additional drops use separate source lists, whereas
        // wrenching a part puts its item and additional drops in one list.
        let recovered=ae2_physical_drops(&nested);assert_eq!(recovered.len(),1002);
        assert_eq!(recovered.iter().skip(1).map(|item|u64::from(item.count)).sum::<u64>(),64_064);
        assert!(recovered.iter().skip(1).all(|item|item.same_item_same_components(&queued)));
        let wrench=ae2_physical_part_drops_with_max(&nested,&|_|64);assert_eq!(wrench.len(),1001);
        assert_eq!(wrench.iter().skip(1).map(|item|u64::from(item.count)).sum::<u64>(),64_000);
        let mut encoded=ItemStack::new("ae2:crafting_pattern",1);encoded.data_mut().custom_name = Some(horizon_client::item_data::text_name("physical encoded pattern"));
        let blank=ItemStack::new("ae2:blank_pattern",3);
        let mut enc_host=Machine::new(MachineKind::CableBus);
        let slot=|n,stack:&ItemStack|{let mut tag=save::item_to_nbt(stack);if let Tag::Compound(c)=&mut tag {c.insert("Slot".into(),Tag::Byte(n));}tag};
        enc_host.config.insert("parts".into(),Tag::List(vec![Tag::compound([
            ("id",Tag::String("ae2:pattern_encoding_terminal".into())),
            ("machine",Tag::compound([("slots",Tag::List(vec![slot(0,&blank),slot(1,&encoded)])),("upgrades",Tag::List(vec![])),("config",Tag::Compound(Default::default()))]))
        ])]));
        assert_eq!(ae2_physical_drops(&enc_host),vec![ItemStack::new("ae2:pattern_encoding_terminal",1),blank,encoded]);
        let mut sparse=Machine::new(MachineKind::CableBus);
        let card=ItemStack::new("ae2:speed_card",1);
        let valid=horizon_client::ae2_upgrade_inventory::encode(&[None,Some(card.clone())]).unwrap();
        let install=|host:&mut Machine,cards:Vec<Tag>| {host.config.insert("parts".into(),Tag::List(vec![Tag::compound([("id",Tag::String("ae2:import_bus".into())),("upgrades",Tag::List(cards))])]));};
        install(&mut sparse,valid.clone());
        assert_eq!(ae2_physical_drops(&sparse),vec![ItemStack::new("ae2:import_bus",1),card.clone()]);
        install(&mut sparse,vec![valid[0].clone(),valid[0].clone()]);
        assert_eq!(ae2_physical_drops(&sparse),vec![ItemStack::new("ae2:import_bus",1)]);
        let outside=horizon_client::ae2_upgrade_inventory::encode(&[None,None,None,None,None,Some(card)]).unwrap();
        install(&mut sparse,outside);
        assert_eq!(ae2_physical_drops(&sparse),vec![ItemStack::new("ae2:import_bus",1)]);
        let mut invalid = Machine::new(MachineKind::CableBus);
        invalid.config.insert(
            "cable".into(),
            save::item_to_nbt(&ItemStack::new("minecraft:diamond", 1)),
        );
        assert!(ae2_physical_drops(&invalid).is_empty());
        assert!(ae2_synthetic_cable_host_drop("ae2:cable_bus", &ItemStack::new("ae2:cable_bus",1)));
        assert!(!ae2_synthetic_cable_host_drop("ae2:cable_bus", &physical_cable));
        assert!(!ae2_synthetic_cable_host_drop("ae2:drive", &ItemStack::new("ae2:drive",1)));
        assert!(ae2_physical_drops(&Machine::new(MachineKind::Terminal)).is_empty());
    }

    #[test]
    fn ominous_banner_survives_placement_save_and_drop() {
        let original = raider::ominous_banner();
        let Tag::Compound(components) = item_data::encode_components(&original).unwrap() else {
            panic!("banner components");
        };
        for block in ["minecraft:white_banner", "minecraft:white_wall_banner"] {
            let pos = (1, 64, 2);
            let fresh = BlockEntity::for_block(block).unwrap();
            let fresh = save::encode_block_entity(pos, &fresh, block);
            let placed = block_entity_item::place(&fresh, &components, false);
            let (_, placed) = save::decode_block_entity(&placed).unwrap();
            let saved = save::encode_block_entity(pos, &placed, block);
            let (_, loaded) = save::decode_block_entity(&saved).unwrap();
            let BlockEntity::Banner(banner) = &loaded else {
                panic!("placed banner");
            };
            assert_eq!(banner.layers.len(), item_data::OMINOUS_LAYERS.len());
            let mut pending = Pending::default();
            pending.record([pos.0, pos.1, pos.2], &loaded, block, None);
            let mut dropped = ItemStack::new("minecraft:white_banner", 1);
            pending.attach([pos.0, pos.1, pos.2], &mut dropped);
            assert!(dropped.data().unwrap().ominous_banner);
            assert!(horizon_client::banner::tooltip_layers(&dropped).is_empty());
            assert_eq!(
                horizon_client::item_tooltip::name_line(&dropped, &|_| None),
                "Ominous Banner"
            );
            assert_eq!(
                horizon_client::item_tooltip::rarity(&dropped, "common"),
                "uncommon"
            );
            assert_eq!(
                item_data::encode_components(&dropped),
                Some(Tag::Compound(components.clone()))
            );
        }
    }
}
