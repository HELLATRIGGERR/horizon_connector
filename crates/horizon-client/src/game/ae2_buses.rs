//! Experimental AE2 I/O buses attached to cable-bus block entities.
//! Inventories remain real world block entities; the host snapshots block
//! entities before each transfer to preserve contents on an error.
use super::{Game, ae2_lab};
use ae2_machines_core::{BusState, Channel, Direction, Key, Redstone, TransferHost};
use horizon_client::{ae2_world::MachineKind, containers::BlockEntity, items::ItemStack, save};
use horizon_core::random::RandomSource;
use horizon_world::nbt::{Compound, Tag};
use serde_json::json;

fn only_energy_emitter_runtime_changes(before:&[Tag],after:&[Tag])->bool {
    before.len()==after.len() && before.iter().zip(after).all(|(a,b)|{
        if a==b{return true}
        let (Tag::Compound(a),Tag::Compound(b))=(a,b) else{return false};
        if a.get(&"id".into())!=Some(&Tag::String("ae2:energy_level_emitter".into())){return false}
        let mut a=a.clone();let mut b=b.clone();
        for key in ["emitter_active","emitter_count"]{a.remove(&key.into());b.remove(&key.into());}
        a==b
    })
}
fn encoded(p: (i32, i32, i32)) -> Option<super::ae2_global::GridAddress> {
    super::ae2_global::GridAddress::from_network(ae2_network_core::Pos {
        x: p.0,
        y: p.1,
        z: p.2,
    })
}
/// A crafting requester is the physical export bus on one cable face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct ExportBusCraftOwner {
    pub host: super::ae2_global::GridAddress,
    pub face: u8,
}
impl ExportBusCraftOwner {
    pub(super) fn anchor(self) -> super::ae2_global::GridAddress {
        self.host
    }
    fn face_name(self) -> Option<&'static str> {
        ["east", "west", "up", "down", "south", "north"]
            .get(self.face as usize)
            .copied()
    }
}
const BUS_CRAFT_LINKS: &str = "ae2_bus_craft_links";
const BUS_CANCELLED_LINKS: &str = "ae2_bus_cancelled_links";
fn export_craft_links(
    data: &Compound,
) -> Result<Vec<Option<super::ae2_global::GridAddress>>, String> {
    let mut links = vec![None; 63];
    let Some(tag) = data.get(&BUS_CRAFT_LINKS.into()) else {
        return Ok(links);
    };
    let Tag::List(entries) = tag else {
        return Err("invalid export bus crafting links".into());
    };
    for entry in entries {
        let (Some(Tag::Byte(slot)), Some(Tag::IntArray(pos))) =
            (entry.get("Slot"), entry.get("cpu"))
        else {
            return Err("invalid export bus crafting link".into());
        };
        let slot = *slot as usize;
        if slot >= links.len() || links[slot].is_some() || pos.len() != 3 {
            return Err("duplicate or invalid export bus link".into());
        }
        links[slot] = Some(
            super::ae2_global::GridAddress::from_network(ae2_network_core::Pos {
                x: pos[0],
                y: pos[1],
                z: pos[2],
            })
            .ok_or("invalid export bus CPU address")?,
        );
    }
    Ok(links)
}
fn store_export_craft_links(data: &mut Compound, links: &[Option<super::ae2_global::GridAddress>]) {
    let entries = links
        .iter()
        .enumerate()
        .filter_map(|(i, cpu)| {
            let (x, y, z) = (*cpu)?.encoded();
            Some(Tag::compound([
                ("Slot", Tag::Byte(i as i8)),
                ("cpu", Tag::IntArray(vec![x, y, z])),
            ]))
        })
        .collect();
    data.insert(BUS_CRAFT_LINKS.into(), Tag::List(entries));
}
fn export_has_crafting_card(data: &Compound) -> bool {
    matches!(data.get(&"upgrades".into()), Some(Tag::List(entries)) if entries.iter().any(|entry| entry.get("id") == Some(&Tag::String("ae2:crafting_card".into())) && entry.get("count") == Some(&Tag::Int(1))))
}
fn export_link_cancelled(data: &Compound, slot: usize) -> bool {
    match data.get(&BUS_CANCELLED_LINKS.into()) {
        None => false,
        Some(Tag::Long(mask)) => (*mask as u64) & (1u64 << slot) != 0,
        _ => true,
    }
}
pub(super) fn mark_export_card_removed(data: &mut Compound) {
    super::ae2_autocrafting::advance_crafting_card_generation(data);
    let mask = export_craft_links(data)
        .map(|links| {
            links.iter().enumerate().fold(0u64, |mask, (slot, link)| {
                mask | if link.is_some() { 1u64 << slot } else { 0 }
            })
        })
        .unwrap_or(u64::MAX >> 1);
    let old = match data.get(&BUS_CANCELLED_LINKS.into()) {
        Some(Tag::Long(mask)) => *mask as u64,
        _ => 0,
    };
    data.insert(BUS_CANCELLED_LINKS.into(), Tag::Long((old | mask) as i64));
}
fn str_field(compound: &Compound, name: &str) -> Option<String> {
    match compound.get(&name.into()) {
        Some(Tag::String(s)) => s.to_string().ok(),
        _ => None,
    }
}
fn target(pos: (i32, i32, i32), face: &str) -> Option<(i32, i32, i32)> {
    let (x, y, z) = pos;
    Some(match face {
        "down" => (x, y - 1, z),
        "up" => (x, y + 1, z),
        "north" => (x, y, z - 1),
        "south" => (x, y, z + 1),
        "west" => (x - 1, y, z),
        "east" => (x + 1, y, z),
        _ => return None,
    })
}
pub(super) fn bus_upgrade_slots(part: &Compound) -> Result<Vec<Option<ItemStack>>, String> {
    match part.get(&"upgrades".into()) {
        None => Ok(vec![None; 5]),
        Some(Tag::List(entries)) => horizon_client::ae2_upgrade_inventory::decode(entries, 5),
        _ => Err("invalid bus upgrades".into()),
    }
}
pub(super) fn store_bus_upgrade_slots(
    part: &mut Compound,
    slots: &[Option<ItemStack>],
) -> Result<(), String> {
    if slots.len() != 5 {
        return Err("invalid bus upgrade slot count".into());
    }
    part.insert(
        "upgrades".into(),
        Tag::List(horizon_client::ae2_upgrade_inventory::encode(slots)?),
    );
    Ok(())
}
pub(super) fn bus_state(part: &Compound) -> Result<BusState, String> {
    let mut bus = if let Some(s) = str_field(part, "state_json") {
        serde_json::from_str(&s).map_err(|e| format!("bad bus state: {e}"))?
    } else {
        let mut bus = BusState::new(Channel::Item);
        if let Some(Tag::List(filters)) = part.get(&"filters".into()) {
            for (i, tag) in filters.iter().enumerate().take(bus.config.len()) {
                if let Some(stack) = save::item_from_nbt(tag) {
                    bus.config[i] = Some(Key::from(&ae2_lab::key(&stack)?));
                }
            }
        }
        bus
    };
    if bus.channel != Channel::Item || bus.config.len() > 63 {
        return Err("unsupported bus channel or invalid filter state".into());
    }
    let id = str_field(part, "id").ok_or("missing bus part id")?;
    let upgrades = bus_upgrade_slots(part)?;
    let mut speed = 0u8;
    let mut capacity = 0u8;
    let mut fuzzy = 0u8;
    let mut inverter = 0u8;
    let mut redstone = 0u8;
    let mut crafting = 0u8;
    let mut void = 0u8;
    for card in upgrades.iter().flatten() {
        match card.item.as_ref() {
            "ae2:speed_card" => speed += 1,
            "ae2:capacity_card" => capacity += 1,
            "ae2:fuzzy_card" => fuzzy += 1,
            "ae2:inverter_card" => inverter += 1,
            "ae2:redstone_card" => redstone += 1,
            "ae2:crafting_card" => crafting += 1,
            "ae2:void_card" => void += 1,
            _ => return Err("unsupported bus card".into()),
        }
    }
    let valid = match id.as_str() {
        "ae2:import_bus" => {
            speed <= 4
                && capacity <= 5
                && fuzzy <= 1
                && inverter <= 1
                && redstone <= 1
                && crafting == 0
                && void == 0
        }
        "ae2:export_bus" => {
            speed <= 4
                && capacity <= 5
                && fuzzy <= 1
                && redstone <= 1
                && crafting <= 1
                && inverter == 0
                && void == 0
        }
        "ae2:formation_plane" => {
            capacity <= 5
                && fuzzy <= 1
                && inverter <= 1
                && speed == 0
                && redstone == 0
                && crafting == 0
                && void == 0
        }
        "ae2:storage_bus" => {
            capacity <= 5
                && fuzzy <= 1
                && inverter <= 1
                && void <= 1
                && speed == 0
                && redstone == 0
                && crafting == 0
        }
        _ => false,
    };
    if !valid {
        return Err("bus cards exceed source limits".into());
    }
    bus.speed_cards = speed;
    bus.capacity_cards = capacity;
    bus.fuzzy = fuzzy > 0;
    bus.inverted = inverter > 0;
    bus.redstone_card = redstone > 0;
    let slots = bus.slots();
    bus.next_slot = if slots == 0 { 0 } else { bus.next_slot % slots };
    Ok(bus)
}
/// AEKey.fuzzyEquals: primary key equality plus damage-range partition.
pub(super) fn fuzzy_key_match(a: &Key, b: &Key, mode: &str, base_max: u32) -> bool {
    if a.id != b.id {
        return false;
    }
    if mode == "ignore_all" {
        return true;
    }
    let Ok(a) = ae2_lab::prototype(&ae2_cell_core::ItemKey::from(a)) else {
        return false;
    };
    let Ok(b) = ae2_lab::prototype(&ae2_cell_core::ItemKey::from(b)) else {
        return false;
    };
    let max = |s: &ItemStack| match s.data().and_then(|d| d.other("minecraft:max_damage")) {
        Some(Tag::Int(n)) if *n >= 0 => *n as u32,
        _ => base_max,
    };
    let max_a = max(&a);
    let max_b = max(&b);
    if max_a == 0 {
        return true;
    }
    if mode == "percent_99" {
        return (a.damage > 0) == (b.damage > 0);
    }
    let threshold = match mode {
        "percent_75" => 0.25f32,
        "percent_50" => 0.50,
        "percent_25" => 0.75,
        _ => return false,
    };
    (a.damage as f32 / max_a as f32 > threshold) == (b.damage as f32 / max_b as f32 > threshold)
}
/// A mounted adjacent container, identified by its actual block position.
#[derive(Clone, Debug)]
pub(super) struct StorageMount {
    pub identity: ae2_network_core::NodeIdentity,
    pub target: super::ae2_global::GridAddress,
    pub cable: super::ae2_global::GridAddress,
    pub face: horizon_core::geometry::Direction,
    pub priority: i32,
    pub access: ae2_machines_core::Access,
    pub filter: Vec<Key>,
    pub inverted: bool,
    pub filter_on_extract: bool,
    /// StorageFilter controls available-stack visibility independently from
    /// actual extraction and insertion decisions.
    pub extractable_only: bool,
    pub fuzzy: bool,
    pub fluid: bool,
    pub fuzzy_mode: String,
    pub damage_limits: std::collections::BTreeMap<String, u32>,
    pub void_overflow: bool,
    /// Physical cards and all ghost slots, including currently inactive slots.
    pub monitor_configuration: String,
}
impl StorageMount {
    pub(super) fn allows(&self, key: &Key, extract: bool) -> bool {
        use ae2_machines_core::Access;
        if extract && !self.filter_on_extract {
            // MEInventoryHandler.extract delegates directly in this mode,
            // including when ACCESS is WRITE.
            return true;
        }
        if (extract && self.access == Access::WriteOnly)
            || (!extract && self.access == Access::ReadOnly)
        {
            return false;
        }
        if self.filter.is_empty() {
            return true;
        }
        let matched = self.filter.iter().any(|f| {
            if self.fuzzy {
                if self.fluid {
                    f.id == key.id
                } else {
                    fuzzy_key_match(
                        f,
                        key,
                        &self.fuzzy_mode,
                        self.damage_limits.get(&f.id).copied().unwrap_or(0),
                    )
                }
            } else {
                f == key
            }
        });
        matched != self.inverted
    }

    pub(super) fn visible(&self, key: &Key) -> bool {
        !self.extractable_only || !self.filter_on_extract || self.allows(key, true)
    }
}
fn target_face(face: &str) -> Option<horizon_core::geometry::Direction> {
    use horizon_core::geometry::Direction as F;
    Some(match face {
        "east" => F::West,
        "west" => F::East,
        "up" => F::Down,
        "down" => F::Up,
        "south" => F::North,
        "north" => F::South,
        _ => return None,
    })
}
type QueuedCraft = (
    ExportBusCraftOwner,
    usize,
    ae2_cell_core::ItemKey,
    bool,
    u64,
);
#[derive(Default)]
struct CraftState {
    owner: Option<ExportBusCraftOwner>,
    links: Vec<Option<super::ae2_global::GridAddress>>,
    only: bool,
    requests: Vec<QueuedCraft>,
}
impl CraftState {
    // ExportBusPart.attemptCrafting charges the preflighted operation budget
    // even when MultiCraftingTracker rejects a duplicate in-flight request.
    fn record_preflight(
        &mut self,
        owner: ExportBusCraftOwner,
        slot: usize,
        key: ae2_cell_core::ItemKey,
        fluid: bool,
        amount: u64,
    ) -> u64 {
        if amount > 0
            && self.links.get(slot).is_some_and(Option::is_none)
            && !self.requests.iter().any(|(_, queued, _, _, _)| *queued == slot)
        {
            self.requests.push((owner, slot, key, fluid, amount));
        }
        amount
    }
    fn request(
        &mut self,
        game: &mut Game<'_>,
        slot: usize,
        key: &Key,
        limit: u64,
        fluid: bool,
    ) -> u64 {
        let Some(owner) = self.owner else { return 0 };
        if self.links.get(slot).is_none() {
            return 0;
        }
        let Some((target, _)) = game.export_craft_target(owner) else {
            return 0;
        };
        let mounts = if fluid {
            game.ae2_fluid_storage_mounts_encoded(owner.host)
        } else {
            game.ae2_storage_mounts_encoded(owner.host)
        };
        if mounts.iter().any(|mount| mount.target == target) {
            return 0;
        }
        let key = ae2_cell_core::ItemKey::from(key);
        let amount = game.export_craft_room(owner, &key, fluid, limit);
        self.record_preflight(owner, slot, key, fluid, amount)
    }
}
struct Host<'a, 'g> {
    game: &'a mut Game<'g>,
    cable: (i32, i32, i32),
    target: (i32, i32, i32),
    face: horizon_core::geometry::Direction,
    fuzzy_mode: String,
    craft: CraftState,
}
impl Host<'_, '_> {
    fn snapshot(&self) -> super::ae2_global::BlockSnapshot {
        self.game.ae2_snapshot_blocks()
    }
    fn restore(&mut self, snapshot: super::ae2_global::BlockSnapshot) {
        self.game.ae2_restore_blocks(snapshot);
    }
    fn candidates(&self) -> Vec<ItemStack> {
        let Some(address) = self.game.ae2_local_address(self.target) else {
            return Vec::new();
        };
        self.game
            .ae2_item_p2p_candidates(address, self.face)
            .unwrap_or_else(|| self.game.ae2_native_item_candidates(address, self.face))
    }
}
impl TransferHost for Host<'_, '_> {
    type Error = String;
    fn random_slot(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            self.game
                .world
                .random
                .next_int_bound(bound as i32)
                .unwrap_or(0) as usize
        }
    }
    fn craft_only(&self) -> bool {
        self.craft.owner.is_some() && self.craft.only
    }
    fn request_craft(&mut self, slot: usize, key: &Key, limit: u64) -> Result<u64, String> {
        Ok(self.craft.request(self.game, slot, key, limit, false))
    }
    fn adjacent_slots(&self) -> Vec<(Key, u64)> {
        self.candidates()
            .iter()
            .filter_map(|s| {
                ae2_lab::key(s)
                    .ok()
                    .map(|k| (Key::from(&k), u64::from(s.count)))
            })
            .collect()
    }
    fn fuzzy_matches(&self, a: &Key, b: &Key) -> bool {
        let max = self.game.res.items().get(&a.id).map_or(0, |i| i.max_damage);
        fuzzy_key_match(a, b, &self.fuzzy_mode, max)
    }
    fn fuzzy_candidates(&self, filter: &Key) -> Vec<Key> {
        self.game
            .ae2_grid_inventory(self.cable)
            .unwrap_or_default()
            .iter()
            .map(|(k, _)| Key::from(k))
            .filter(|k| self.fuzzy_matches(filter, k))
            .collect()
    }
    fn transfer_atomic(
        &mut self,
        direction: Direction,
        key: &Key,
        limit: u64,
    ) -> Result<u64, String> {
        if limit == 0 {
            return Ok(0);
        }
        // Avoid feeding a bus back into a storage bus on the same container.
        if self.game.ae2_storage_mounts(self.cable).iter().any(|m| {
            self.game
                .ae2_local_address(self.target)
                .is_some_and(|a| m.target == a)
        }) {
            return Ok(0);
        }
        let snapshot = self.snapshot();
        let mut formation = FormationPlan::default();
        let result = (|| -> Result<u64, String> {
            let address = self
                .game
                .ae2_local_address(self.target)
                .ok_or("invalid bus target")?;
            let item_key = ae2_cell_core::ItemKey::from(key);
            let prototype = ae2_lab::prototype(&item_key)?;
            match direction {
                Direction::Import => {
                    let requested = limit.min(u32::MAX as u64) as u32;
                    let preview = match self.game.ae2_item_p2p_extract_staged(address, self.face, requested, Some(&prototype), true, &mut formation) {
                        Some(result) => result,
                        None => self.game.ae2_native_item_extract_staged(address, self.face, requested, Some(&prototype), true, &mut formation),
                    };
                    let Some(stack) = preview else { return Ok(0) };
                    let wanted = u64::from(stack.count).min(limit);
                    let moved = self.game.ae2_grid_insert_staged(
                        self.cable,
                        &stack,
                        wanted,
                        &mut formation,
                    )?;
                    if moved == 0 {
                        return Ok(0);
                    }
                    let actual = match self.game.ae2_item_p2p_extract_staged(address, self.face, moved as u32, Some(&prototype), false, &mut formation) {
                        Some(result) => result,
                        None => self.game.ae2_native_item_extract_staged(address, self.face, moved as u32, Some(&prototype), false, &mut formation),
                    };
                    if !actual.is_some_and(|s| {
                        u64::from(s.count) == moved && s.same_item_same_components(&prototype)
                    }) {
                        return Err("bus source changed after preview".into());
                    }
                    Ok(moved)
                }
                Direction::Export => {
                    let mut offered = prototype.clone();
                    offered.count = limit.min(u32::MAX as u64) as u32;
                    let room = match self.game.ae2_item_p2p_insert_staged(address, self.face, &offered, true, &mut formation) {
                        Some(n) => n,
                        None => self.game.ae2_native_item_insert_staged(address, self.face, &offered, offered.count, true, &mut formation),
                    };
                    if room == 0 {
                        return Ok(0);
                    }
                    let Some(extracted) =
                        self.game
                            .ae2_grid_extract_staged(self.cable, &item_key, u64::from(room), &mut formation)?
                    else {
                        return Ok(0);
                    };
                    let inserted = match self.game.ae2_item_p2p_insert_staged(address, self.face, &extracted, false, &mut formation) {
                        Some(n) => n,
                        None => self.game.ae2_native_item_insert_staged(address, self.face, &extracted, extracted.count, false, &mut formation),
                    };
                    if inserted != extracted.count {
                        return Err("bus target changed after preview".into());
                    }
                    Ok(u64::from(inserted))
                }
            }
        })();
        let result =
            result.and_then(|moved| self.game.ae2_commit_formation(formation).map(|_| moved));
        if result.is_err() {
            self.restore(snapshot);
        }
        result
    }
}

/// Dispatches each actual resource key to its matching capability while one
/// BusState owns the source operation budget and per-slot schedule.
struct MixedHost<'a, 'g> {
    game: &'a mut Game<'g>,
    cable: (i32, i32, i32),
    target: (i32, i32, i32),
    face: horizon_core::geometry::Direction,
    fuzzy_mode: String,
    craft: CraftState,
}
fn fluid_bus_key(key: &Key) -> bool {
    let key = ae2_cell_core::ItemKey::from(key);
    super::ae2_fluids::valid_fluid_id(&key.item_id) && ae2_lab::prototype(&key).is_err()
}
impl TransferHost for MixedHost<'_, '_> {
    type Error = String;
    fn amount_per_operation(&self, key: &Key) -> Option<u64> {
        Some(
            if fluid_bus_key(key) {
                Channel::Fluid
            } else {
                Channel::Item
            }
            .amount_per_operation(),
        )
    }
    fn random_slot(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            self.game
                .world
                .random
                .next_int_bound(bound as i32)
                .unwrap_or(0) as usize
        }
    }
    fn craft_only(&self) -> bool {
        self.craft.owner.is_some() && self.craft.only
    }
    fn request_craft(&mut self, slot: usize, key: &Key, limit: u64) -> Result<u64, String> {
        Ok(self
            .craft
            .request(self.game, slot, key, limit, fluid_bus_key(key)))
    }
    fn adjacent_slots(&self) -> Vec<(Key, u64)> {
        let Some(address) = self.game.ae2_local_address(self.target) else {
            return Vec::new();
        };
        let mut result: Vec<_> = self
            .game
            .ae2_item_p2p_candidates(address, self.face)
            .unwrap_or_else(|| self.game.ae2_native_item_candidates(address, self.face))
            .iter()
            .filter_map(|s| {
                ae2_lab::key(s)
                    .ok()
                    .map(|k| (Key::from(&k), u64::from(s.count)))
            })
            .collect();
        let fluids = match self.game.ae2_fluid_p2p_entries(address, self.face) {
            Some(entries) => entries,
            None => self
                .game
                .ae2_interface_fluid_entries(address, self.face)
                .unwrap_or_else(|| {
                    self.game
                        .ae2_tank_contents_encoded(address)
                        .ok()
                        .flatten()
                        .into_iter()
                        .collect()
                }),
        };
        result.extend(
            fluids
                .into_iter()
                .map(|(key, amount)| (Key::from(&key), amount)),
        );
        result
    }
    fn fuzzy_matches(&self, a: &Key, b: &Key) -> bool {
        if fluid_bus_key(a) {
            a.id == b.id
        } else {
            let max = self.game.res.items().get(&a.id).map_or(0, |i| i.max_damage);
            fuzzy_key_match(a, b, &self.fuzzy_mode, max)
        }
    }
    fn fuzzy_candidates(&self, filter: &Key) -> Vec<Key> {
        if fluid_bus_key(filter) {
            self.game
                .ae2_grid_fluids(self.cable)
                .unwrap_or_default()
                .into_iter()
                .map(|(key, _)| Key::from(&key))
                .filter(|key| self.fuzzy_matches(filter, key))
                .collect()
        } else {
            self.game
                .ae2_grid_inventory(self.cable)
                .unwrap_or_default()
                .iter()
                .map(|(key, _)| Key::from(key))
                .filter(|key| self.fuzzy_matches(filter, key))
                .collect()
        }
    }
    fn transfer_atomic(
        &mut self,
        direction: Direction,
        key: &Key,
        limit: u64,
    ) -> Result<u64, String> {
        let craft = std::mem::take(&mut self.craft);
        if fluid_bus_key(key) {
            let mut host = FluidHost {
                game: self.game,
                cable: self.cable,
                target: self.target,
                face: self.face,
                craft,
            };
            let result = host.transfer_atomic(direction, key, limit);
            self.craft = host.craft;
            result
        } else {
            let mut host = Host {
                game: self.game,
                cable: self.cable,
                target: self.target,
                face: self.face,
                fuzzy_mode: self.fuzzy_mode.clone(),
                craft,
            };
            let result = host.transfer_atomic(direction, key, limit);
            self.craft = host.craft;
            result
        }
    }
}

struct FluidHost<'a, 'g> {
    game: &'a mut Game<'g>,
    cable: (i32, i32, i32),
    target: (i32, i32, i32),
    face: horizon_core::geometry::Direction,
    craft: CraftState,
}
impl TransferHost for FluidHost<'_, '_> {
    type Error = String;
    fn random_slot(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            self.game
                .world
                .random
                .next_int_bound(bound as i32)
                .unwrap_or(0) as usize
        }
    }
    fn craft_only(&self) -> bool {
        self.craft.owner.is_some() && self.craft.only
    }
    fn request_craft(&mut self, slot: usize, key: &Key, limit: u64) -> Result<u64, String> {
        Ok(self.craft.request(self.game, slot, key, limit, true))
    }
    fn adjacent_slots(&self) -> Vec<(Key, u64)> {
        self.game
            .ae2_local_address(self.target)
            .and_then(|address| {
                self.game
                    .ae2_fluid_p2p_contents(address, self.face)
                    .unwrap_or_else(|| {
                        self.game
                            .ae2_interface_fluid_contents(address, self.face)
                            .unwrap_or_else(|| {
                                self.game.ae2_tank_contents_encoded(address).ok().flatten()
                            })
                    })
            })
            .map(|(k, n)| vec![(Key::from(&k), n)])
            .unwrap_or_default()
    }
    fn fuzzy_matches(&self, a: &Key, b: &Key) -> bool {
        a.id == b.id
    }
    fn fuzzy_candidates(&self, filter: &Key) -> Vec<Key> {
        self.game
            .ae2_grid_fluids(self.cable)
            .unwrap_or_default()
            .into_iter()
            .map(|(k, _)| Key::from(&k))
            .filter(|k| k.id == filter.id)
            .collect()
    }
    fn transfer_atomic(
        &mut self,
        direction: Direction,
        key: &Key,
        limit: u64,
    ) -> Result<u64, String> {
        if limit == 0
            || self
                .game
                .ae2_fluid_storage_mounts(self.cable)
                .iter()
                .any(|m| {
                    self.game
                        .ae2_local_address(self.target)
                        .is_some_and(|a| m.target == a)
                })
        {
            return Ok(0);
        }
        let before = self.game.ae2_snapshot_blocks();
        let key = ae2_cell_core::ItemKey::from(key);
        let mut formation = FormationPlan::default();
        let result =
            (|| -> Result<u64, String> {
                match direction {
                    Direction::Import => {
                        let address = self
                            .game
                            .ae2_local_address(self.target)
                            .ok_or("invalid fluid target")?;
                        let available = match self
                            .game
                            .ae2_fluid_p2p_drain(address, self.face, &key, limit, true)
                        {
                            Some(n) => n,
                            None => match self.game.ae2_interface_fluid_move_staged(
                                address, self.face, &key, limit, false, true, &mut formation,
                            ) {
                                Some(n) => n,
                                None => self
                                    .game
                                    .ae2_tank_drain_encoded(address, &key, limit, true)?,
                            },
                        };
                        let moved = self
                            .game
                            .ae2_grid_fluid_insert_staged(self.cable, &key, available, false, &mut formation)?;
                        let actual = match self
                            .game
                            .ae2_fluid_p2p_drain(address, self.face, &key, moved, false)
                        {
                            Some(n) => n,
                            None => match self.game.ae2_interface_fluid_move_staged(
                                address, self.face, &key, moved, false, false, &mut formation,
                            ) {
                                Some(n) => n,
                                None => self
                                    .game
                                    .ae2_tank_drain_encoded(address, &key, moved, false)?,
                            },
                        };
                        if actual != moved {
                            return Err("fluid source changed".into());
                        }
                        Ok(moved)
                    }
                    Direction::Export => {
                        let address = self
                            .game
                            .ae2_local_address(self.target)
                            .ok_or("invalid fluid target")?;
                        let room = match self
                            .game
                            .ae2_fluid_p2p_fill_staged(address, self.face, &key, limit, true, &mut formation)
                        {
                            Some(n) => n,
                            None => match self.game.ae2_interface_fluid_move_staged(
                                address, self.face, &key, limit, true, true, &mut formation,
                            ) {
                                Some(n) => n,
                                None => match self.game.ae2_provider_return_fluid_at(
                                    address, self.face, &key, limit, true,
                                ) {
                                    Some(n) => n,
                                    None => self
                                        .game
                                        .ae2_tank_fill_encoded(address, &key, limit, true)?,
                                },
                            },
                        };
                        let moved = self
                            .game
                            .ae2_grid_fluid_extract(self.cable, &key, room, false)?;
                        let actual = match self
                            .game
                            .ae2_fluid_p2p_fill_staged(address, self.face, &key, moved, false, &mut formation)
                        {
                            Some(n) => n,
                            None => match self.game.ae2_interface_fluid_move_staged(
                                address, self.face, &key, moved, true, false, &mut formation,
                            ) {
                                Some(n) => n,
                                None => match self.game.ae2_provider_return_fluid_at(
                                    address, self.face, &key, moved, false,
                                ) {
                                    Some(n) => n,
                                    None => self
                                        .game
                                        .ae2_tank_fill_encoded(address, &key, moved, false)?,
                                },
                            },
                        };
                        if actual != moved {
                            return Err("fluid target changed".into());
                        }
                        Ok(moved)
                    }
                }
            })().and_then(|moved| self.game.ae2_commit_formation(formation).map(|()| moved));
        if result.is_err() {
            self.game.ae2_restore_blocks(before);
        }
        result
    }
}
impl Game<'_> {
    fn export_part(&self, owner: ExportBusCraftOwner) -> Option<Compound> {
        let face = owner.face_name()?;
        let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(owner.host) else {
            return None;
        };
        if machine.kind != MachineKind::CableBus {
            return None;
        }
        let Some(Tag::List(parts)) = machine.config.get(&"parts".into()) else {
            return None;
        };
        let mut matches = parts
            .iter()
            .filter(|part| part.get("face") == Some(&Tag::String(face.into())));
        let Tag::Compound(data) = matches.next()? else {
            return None;
        };
        if matches.next().is_some() || str_field(data, "id").as_deref() != Some("ae2:export_bus") {
            return None;
        }
        Some(data.clone())
    }
    fn store_export_part(
        &mut self,
        owner: ExportBusCraftOwner,
        data: Compound,
    ) -> Result<(), String> {
        let face = owner.face_name().ok_or("invalid export bus face")?;
        let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity_mut(owner.host) else {
            return Err("export bus disappeared".into());
        };
        if machine.kind != MachineKind::CableBus {
            return Err("export host changed".into());
        }
        let Some(Tag::List(parts)) = machine.config.get_mut(&"parts".into()) else {
            return Err("export parts missing".into());
        };
        let matches: Vec<_> = parts
            .iter()
            .enumerate()
            .filter(|(_, part)| part.get("face") == Some(&Tag::String(face.into())))
            .map(|(i, _)| i)
            .collect();
        if matches.len() != 1
            || parts[matches[0]].get("id") != Some(&Tag::String("ae2:export_bus".into()))
        {
            return Err("export bus changed".into());
        }
        parts[matches[0]] = Tag::Compound(data);
        self.ae2_mark_modified(owner.host);
        Ok(())
    }
    pub(super) fn ae2_export_craft_links(
        &self,
        owner: ExportBusCraftOwner,
    ) -> Result<Vec<Option<super::ae2_global::GridAddress>>, String> {
        export_craft_links(&self.export_part(owner).ok_or("export bus missing")?)
    }
    /// A future cannot be transferred to a changed bus, card, or filter slot.
    pub(super) fn ae2_export_pending_craft_generation(
        &self, owner: ExportBusCraftOwner, slot: usize, key: &ae2_cell_core::ItemKey,
    ) -> Option<i64> {
        let data = self.export_part(owner)?;
        if slot >= 63 || !export_has_crafting_card(&data) || export_craft_links(&data).ok()?.get(slot)?.is_some() { return None; }
        let state = bus_state(&data).ok()?;
        if slot >= state.slots() || !state.config.get(slot).and_then(Option::as_ref).is_some_and(|configured| *configured == Key::from(key)) { return None; }
        super::ae2_autocrafting::crafting_card_generation(&data)
    }
    pub(super) fn ae2_export_set_craft_link(
        &mut self,
        owner: ExportBusCraftOwner,
        slot: usize,
        old: Option<super::ae2_global::GridAddress>,
        new: Option<super::ae2_global::GridAddress>,
    ) -> Result<(), String> {
        let mut data = self.export_part(owner).ok_or("export bus missing")?;
        let mut links = export_craft_links(&data)?;
        if slot >= links.len() || links[slot] != old {
            return Err("export bus crafting link changed".into());
        }
        if new.is_some() && !export_has_crafting_card(&data) {
            return Err("export bus crafting card missing".into());
        }
        links[slot] = new;
        let mask = match data.get(&BUS_CANCELLED_LINKS.into()) {
            Some(Tag::Long(mask)) => *mask as u64,
            _ => 0,
        };
        data.insert(
            BUS_CANCELLED_LINKS.into(),
            Tag::Long((mask & !(1u64 << slot)) as i64),
        );
        store_export_craft_links(&mut data, &links);
        self.store_export_part(owner, data)
    }
    pub(super) fn ae2_export_craft_link_valid(
        &self,
        owner: ExportBusCraftOwner,
        slot: usize,
        cpu: super::ae2_global::GridAddress,
        key: &ae2_cell_core::ItemKey,
        _fluid: bool,
    ) -> bool {
        let Some(data) = self.export_part(owner) else {
            return false;
        };
        if slot >= 63
            || !export_has_crafting_card(&data)
            || export_link_cancelled(&data, slot)
            || export_craft_links(&data).ok().and_then(|links| links[slot]) != Some(cpu)
        {
            return false;
        }
        let Ok(state) = bus_state(&data) else {
            return false;
        };
        slot < state.slots()
            && state
                .config
                .get(slot)
                .and_then(Option::as_ref)
                .is_some_and(|configured| *configured == Key::from(key))
    }
    fn export_craft_target(
        &self,
        owner: ExportBusCraftOwner,
    ) -> Option<(
        super::ae2_global::GridAddress,
        horizon_core::geometry::Direction,
    )> {
        let face = owner.face_name()?;
        let online = self
            .ae2_network()
            .analyze()
            .ok()?
            .nodes
            .get(&owner.host.network_pos())?
            .parts
            .get(&owner.face)?
            .online;
        if !online {
            return None;
        }
        Some((
            encoded(target(owner.host.encoded(), face)?)?,
            target_face(face)?,
        ))
    }
    fn export_craft_room(
        &mut self,
        owner: ExportBusCraftOwner,
        key: &ae2_cell_core::ItemKey,
        fluid: bool,
        amount: u64,
    ) -> u64 {
        let Some((target, face)) = self.export_craft_target(owner) else {
            return 0;
        };
        if fluid {
            if let Some(n) = self.ae2_fluid_p2p_fill(target, face, key, amount, true) {
                return n;
            }
            if let Some(n) = self.ae2_interface_fluid_move(target, face, key, amount, true, true) {
                return n;
            }
            if let Some(n) = self.ae2_provider_return_fluid_at(target, face, key, amount, true) {
                return n;
            }
            return self
                .ae2_tank_fill_encoded(target, key, amount, true)
                .unwrap_or(0);
        }
        let Ok(mut stack) = ae2_lab::prototype(key) else {
            return 0;
        };
        stack.count = amount.min(u32::MAX as u64) as u32;
        u64::from(
            self.ae2_item_p2p_insert(target, face, &stack, true)
                .unwrap_or_else(|| {
                    self.ae2_native_item_insert(target, face, &stack, stack.count, true)
                }),
        )
    }
    /// The CPU owns the stack; callers snapshot and roll back both target and CPU.
    pub(super) fn ae2_export_deliver_item(
        &mut self,
        owner: ExportBusCraftOwner,
        slot: usize,
        cpu: super::ae2_global::GridAddress,
        stack: &ItemStack,
        simulate: bool,
        plan: &mut FormationPlan,
    ) -> Result<u32, String> {
        let key = ae2_lab::key(stack)?;
        if !self.ae2_export_craft_link_valid(owner, slot, cpu, &key, false) {
            return Ok(0);
        }
        let Some((target, face)) = self.export_craft_target(owner) else {
            return Ok(0);
        };
        Ok(match self.ae2_item_p2p_insert_staged(target, face, stack, simulate, plan) {
            Some(n) => n,
            None => self.ae2_native_item_insert_staged(target, face, stack, stack.count, simulate, plan),
        })
    }
    pub(super) fn ae2_export_deliver_fluid(
        &mut self,
        owner: ExportBusCraftOwner,
        slot: usize,
        cpu: super::ae2_global::GridAddress,
        key: &ae2_cell_core::ItemKey,
        amount: u64,
        simulate: bool,
        plan: &mut FormationPlan,
    ) -> Result<u64, String> {
        if !self.ae2_export_craft_link_valid(owner, slot, cpu, key, true) {
            return Ok(0);
        }
        let Some((target, face)) = self.export_craft_target(owner) else {
            return Ok(0);
        };
        if let Some(n) = self.ae2_fluid_p2p_fill_staged(target, face, key, amount, simulate, plan) {
            return Ok(n);
        }
        if let Some(n) = self.ae2_interface_fluid_move_staged(target, face, key, amount, true, simulate, plan) {
            return Ok(n);
        }
        if let Some(n) = self.ae2_provider_return_fluid_at(target, face, key, amount, simulate) {
            return Ok(n);
        }
        Ok(self
            .ae2_tank_fill_encoded(target, key, amount, simulate)
            .unwrap_or(0))
    }
    /// Collect active storage buses on the connected network. Every physical
    /// bus contributes its own filtered handler and priority, including when
    /// another bus reaches the same target. The handler operates on its real
    /// adjacent inventory rather than making a recursive network call.
    pub(super) fn ae2_storage_mounts(&self, pos: (i32, i32, i32)) -> Vec<StorageMount> {
        self.ae2_local_address(pos)
            .map_or_else(Vec::new, |a| self.ae2_storage_mounts_encoded(a))
    }
    pub(super) fn ae2_fluid_storage_mounts(&self, pos: (i32, i32, i32)) -> Vec<StorageMount> {
        self.ae2_local_address(pos)
            .map_or_else(Vec::new, |a| self.ae2_fluid_storage_mounts_encoded(a))
    }
    pub(super) fn ae2_storage_mounts_encoded(
        &self,
        pos: super::ae2_global::GridAddress,
    ) -> Vec<StorageMount> {
        self.ae2_storage_mounts_for(pos, false)
    }
    pub(super) fn ae2_fluid_storage_mounts_encoded(
        &self,
        pos: super::ae2_global::GridAddress,
    ) -> Vec<StorageMount> {
        self.ae2_storage_mounts_for(pos, true)
    }
    fn ae2_storage_mounts_for(
        &self,
        pos: super::ae2_global::GridAddress,
        fluid: bool,
    ) -> Vec<StorageMount> {
        let network = self.ae2_network();
        let Ok(analysis) = network.analyze() else {
            return Vec::new();
        };
        let node = pos.network_pos();
        let Some(status) = analysis.nodes.get(&node).filter(|s| s.online) else {
            return Vec::new();
        };
        let mut mounts = Vec::new();
        for node in &analysis.components[status.component].positions {
            if !analysis.nodes.get(node).is_some_and(|s| s.online) {
                continue;
            }
            let cable = (node.x, node.y, node.z);
            let Some(cable_address) = encoded(cable) else {
                continue;
            };
            let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity(cable_address) else {
                continue;
            };
            if m.kind != MachineKind::CableBus {
                continue;
            }
            let Some(Tag::List(parts)) = m.config.get(&"parts".into()) else {
                continue;
            };
            for part in parts {
                let Tag::Compound(data) = part else { continue };
                if str_field(data, "id").as_deref() != Some("ae2:storage_bus") {
                    continue;
                }
                let Some(face) = str_field(data, "face") else {
                    continue;
                };
                let side = match face.as_str() {
                    "east" => 0,
                    "west" => 1,
                    "up" => 2,
                    "down" => 3,
                    "south" => 4,
                    "north" => 5,
                    _ => continue,
                };
                if !analysis
                    .nodes
                    .get(node)
                    .and_then(|n| n.parts.get(&side))
                    .is_some_and(|p| p.online)
                {
                    continue;
                }
                let Some(target) = target(cable, &face).and_then(encoded) else {
                    continue;
                };
                let Some(target_side) = target_face(&face) else {
                    continue;
                };
                let target_exists = self.ae2_has_interface_face(target, target_side)
                    || if fluid {
                        matches!(self.ae2_block_entity(target),Some(BlockEntity::Ae2(m)) if matches!(m.kind,MachineKind::SkyStoneTank|MachineKind::Condenser))
                    } else {
                        self.ae2_native_container_supports(target, target_side)
                    };
                if !target_exists {
                    continue;
                }
                let Ok(state) = bus_state(data) else { continue };
                if state.redstone != Redstone::Ignore {
                    continue;
                }
                let access = match str_field(data, "access").as_deref() {
                    None | Some("read_write") => ae2_machines_core::Access::ReadWrite,
                    Some("read_only") => ae2_machines_core::Access::ReadOnly,
                    Some("write_only") => ae2_machines_core::Access::WriteOnly,
                    _ => continue,
                };
                let priority = match data.get(&"priority".into()) {
                    Some(Tag::Int(n)) => *n,
                    _ => 0,
                };
                let filter_on_extract =
                    !matches!(data.get(&"filter_on_extract".into()), Some(Tag::Byte(0)));
                let extractable_only = match str_field(data, "storage_filter").as_deref() {
                    None | Some("extractable_only") => true,
                    Some("none") => false,
                    _ => continue,
                };
                let filter = state
                    .config
                    .iter()
                    .take(state.slots())
                    .flatten()
                    .cloned()
                    .collect();
                let fuzzy_mode =
                    str_field(data, "fuzzy_mode").unwrap_or_else(|| "ignore_all".into());
                if ![
                    "ignore_all",
                    "percent_99",
                    "percent_75",
                    "percent_50",
                    "percent_25",
                ]
                .contains(&fuzzy_mode.as_str())
                {
                    continue;
                }
                let damage_limits = state
                    .config
                    .iter()
                    .flatten()
                    .map(|key| {
                        (
                            key.id.clone(),
                            self.res.items().get(&key.id).map_or(0, |i| i.max_damage),
                        )
                    })
                    .collect();
                let Some(identity)=network.part_identities.get(&(*node,side)).copied() else {continue};
                let monitor_configuration = format!("{:?}:{:?}:{:?}:{:?}", state.config, data.get(&"upgrades".into()), state.redstone, state.schedule);
                mounts.push(StorageMount {
                    identity,
                    target,
                    cable: cable_address,
                    face: target_side,
                    priority,
                    access,
                    filter,
                    inverted: state.inverted,
                    filter_on_extract,
                    extractable_only,
                    fuzzy: state.fuzzy,
                    fluid,
                    fuzzy_mode,
                    damage_limits,
                    void_overflow: matches!(data.get(&"upgrades".into()), Some(Tag::List(cards)) if cards.iter().any(|card|card.get("id")==Some(&Tag::String("ae2:void_card".into())))),
                    monitor_configuration,
                });
            }
        }
        mounts.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then(a.cable.cmp(&b.cable))
                .then(a.target.cmp(&b.target))
        });
        // Each Storage Bus has its own handler, access/filter state and
        // priority, even when multiple buses reach the same physical target.
        mounts
    }
    /// List the physically stored items of one mount, with read access and
    /// partitioning applied. Does not recurse into ME network storage.
    pub(super) fn ae2_storage_inventory(
        &self,
        mount: &StorageMount,
    ) -> Vec<(ae2_cell_core::ItemKey, u64)> {
        if self.ae2_has_interface_face(mount.target,mount.face) {return self.ae2_storage_inventory_uncached(mount);}
        self.ae2_monitor_read(mount).map(|(items,_)|items.into_iter().filter(|(key,_)|mount.visible(&Key::from(key))).collect()).unwrap_or_default()
    }
    pub(super) fn ae2_storage_inventory_uncached(
        &self,
        mount: &StorageMount,
    ) -> Vec<(ae2_cell_core::ItemKey, u64)> {
        self.ae2_storage_inventory_raw(mount).into_iter().filter(|(key,_)|mount.visible(&Key::from(key))).collect()
    }
    pub(super) fn ae2_storage_inventory_raw(
        &self,
        mount: &StorageMount,
    ) -> Vec<(ae2_cell_core::ItemKey, u64)> {
        let interface_items = self.ae2_interface_item_candidates(mount.target, mount.face);
        let slots: Vec<ItemStack> = if let Some(items) = interface_items {
            items
        } else if let Some(items) = self.ae2_native_container_candidates(mount.target, mount.face, mount.extractable_only) {
            items
        } else {
            return Vec::new();
        };
        let mut counts = std::collections::BTreeMap::new();
        for stack in &slots {
            let Ok(key) = ae2_lab::key(stack) else {
                continue;
            };
            *counts.entry(key).or_insert(0u64) += u64::from(stack.count);
        }
        counts.into_iter().collect()
    }
    /// Simulate or commit insertion to exactly one adjacent container.
    pub(super) fn ae2_storage_insert(
        &mut self,
        mount: &StorageMount,
        stack: &ItemStack,
        amount: u64,
        simulate: bool,
        plan: &mut FormationPlan,
    ) -> Result<u64, String> {
        let key = Key::from(&ae2_lab::key(stack)?);
        if !mount.allows(&key, false) {
            return Ok(0);
        }
        if self.ae2_has_interface_face(mount.target, mount.face) {
            if !self.ae2_interface_mount_allowed(mount.target, mount.face, true) {
                return Ok(0);
            }
            let offered = amount.min(u64::from(stack.count)).min(u32::MAX as u64) as u32;
            let moved = self
                .ae2_interface_item_insert_staged(mount.target, mount.face, stack, offered, simulate, plan)
                .unwrap_or(0);
            return Ok(if mount.void_overflow {
                u64::from(offered)
            } else {
                u64::from(moved)
            });
        }
        if !self.ae2_monitor_ready(mount) {return Ok(0)}
        let offered = amount.min(u64::from(stack.count)).min(u32::MAX as u64) as u32;
        let (moved, changed) = self.ae2_native_container_insert_staged(mount.target, mount.face, stack, offered, simulate)
            .ok_or("storage bus target changed")?;
        if !changed.is_empty() { self.ae2_monitor_invalidate(mount.identity); }
        plan.actions.extend(changed.into_iter().map(|(address, incarnation)| FormationAction::ContainerChanged { address, incarnation }));
        Ok(u64::from(if mount.void_overflow { offered } else { moved }))
    }
    /// Simulate or commit extraction to a stack, with components preserved.
    pub(super) fn ae2_storage_extract(
        &mut self,
        mount: &StorageMount,
        key: &ae2_cell_core::ItemKey,
        amount: u64,
        simulate: bool,
        plan: &mut FormationPlan,
    ) -> Result<Option<ItemStack>, String> {
        if !mount.allows(&Key::from(key), true) {
            return Ok(None);
        }
        if self.ae2_has_interface_face(mount.target, mount.face) {
            if !self.ae2_interface_mount_allowed(mount.target, mount.face, false) {
                return Ok(None);
            }
            let prototype = ae2_lab::prototype(key)?;
            return Ok(self
                .ae2_interface_item_extract_staged(
                    mount.target,
                    mount.face,
                    amount.min(u32::MAX as u64) as u32,
                    Some(&prototype),
                    simulate,
                    plan,
                )
                .flatten());
        }
        if !self.ae2_monitor_ready(mount) {return Ok(None)}
        let out = ae2_lab::prototype(key)?;
        let registry_max = self.res.items().get(&out.item).map_or(64, |i| i.max_stack).max(1);
        let max = super::ae2_formation::max_stack(&out, registry_max);
        let limit = amount.min(u64::from(max)).min(u32::MAX as u64) as u32;
        let (out, changed) = self.ae2_native_container_extract_staged(mount.target, mount.face, &out, limit, simulate)
            .ok_or("storage bus target changed")?;
        if !changed.is_empty() { self.ae2_monitor_invalidate(mount.identity); }
        plan.actions.extend(changed.into_iter().map(|(address, incarnation)| FormationAction::ContainerChanged { address, incarnation }));
        Ok(out)
    }

    /// Aggregate reservations use the same mount access rules and drain a
    /// complete external mount before advancing through grid priorities.
    pub(super) fn ae2_storage_extract_bulk(
        &mut self, mount: &StorageMount, key: &ae2_cell_core::ItemKey,
        amount: u64, plan: &mut FormationPlan,
    ) -> Result<u64, String> {
        if !mount.allows(&Key::from(key), true) { return Ok(0); }
        if self.ae2_has_interface_face(mount.target, mount.face) {
            if !self.ae2_interface_mount_allowed(mount.target, mount.face, false) { return Ok(0); }
            return self.ae2_interface_item_extract_bulk_staged(mount.target, mount.face, key, amount, plan)
                .unwrap_or_else(|| Ok(0));
        }
        let mut left = amount;
        while left > 0 {
            let Some(stack) = self.ae2_storage_extract(mount, key, left, false, plan)? else { break };
            let moved = u64::from(stack.count);
            if moved == 0 { break; }
            if moved > left { return Err("storage mount exceeded requested extraction".into()); }
            left -= moved;
        }
        Ok(amount - left)
    }

    /// Enumerates exact-filtered storage bus slots on one cable bus. The grid
    /// must deduplicate targets, apply priorities/access, and route writes to
    /// the same physical slot; this helper alone does not mount inventory.
    pub(super) fn ae2_storage_bus_slots(
        &self,
        cable: (i32, i32, i32),
    ) -> Vec<((i32, i32, i32), usize, ae2_cell_core::ItemKey, u64)> {
        if !self.ae2_online(cable) {
            return Vec::new();
        }
        let parts = match self.world.block_entities.get(&cable) {
            Some(BlockEntity::Ae2(m)) => match m.config.get(&"parts".into()) {
                Some(Tag::List(p)) => p,
                _ => return Vec::new(),
            },
            _ => return Vec::new(),
        };
        let mut result = Vec::new();
        for part in parts {
            let Tag::Compound(data) = part else { continue };
            if str_field(data, "id").as_deref() != Some("ae2:storage_bus") {
                continue;
            }
            let Some(face) = str_field(data, "face") else {
                continue;
            };
            if !self.ae2_part_online(cable, &face) {
                continue;
            }
            let Some(pos) = target(cable, &face) else {
                continue;
            };
            let Ok(state) = bus_state(data) else { continue };
            if state.redstone != Redstone::Ignore {
                continue;
            }
            let Some(BlockEntity::Chest(slots)) = self.world.block_entities.get(&pos) else {
                continue;
            };
            let filter: Vec<_> = state.config.iter().take(state.slots()).flatten().collect();
            for (slot, stack) in slots.iter().enumerate() {
                let Some(stack) = stack else { continue };
                let Ok(key) = ae2_lab::key(stack) else {
                    continue;
                };
                let matched = filter.iter().any(|f| {
                    if state.fuzzy {
                        f.id == key.item_id
                    } else {
                        **f == Key::from(&key)
                    }
                });
                if filter.is_empty() || matched != state.inverted {
                    result.push((pos, slot, key, u64::from(stack.count)));
                }
            }
        }
        result
    }

    /// Configure a physically attached bus. Filters are ghost references;
    /// upgrade cards are consumed into `parts[].upgrades` and drop with the part.
    pub(super) fn ae2_bus_command(&mut self, args: &[&str]) -> Result<serde_json::Value, String> {
        use serde_json::json;
        if args.len() < 5 {
            return Err("usage: /ae2 bus status|install|remove|filter|redstone|schedule|priority|access x y z face [value]".into());
        }
        let pos = (
            args[1].parse().map_err(|_| "invalid x")?,
            args[2].parse().map_err(|_| "invalid y")?,
            args[3].parse().map_err(|_| "invalid z")?,
        );
        let face = args[4];
        if target(pos, face).is_none() {
            return Err("invalid face".into());
        }
        let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get(&pos) else {
            return Err("cable missing".into());
        };
        if machine.kind != MachineKind::CableBus {
            return Err("not a cable bus".into());
        }
        let Some(Tag::List(existing)) = machine.config.get(&"parts".into()) else {
            return Err("no cable parts".into());
        };
        let mut parts = existing.clone();
        let part = parts
            .iter_mut()
            .find(|p| p.get("face") == Some(&Tag::String(face.into())))
            .ok_or("no part on that face")?;
        let Tag::Compound(data) = part else {
            return Err("invalid part data".into());
        };
        let mut state = bus_state(data)?;
        let id = str_field(data, "id").ok_or("part missing id")?;
        match args[0] {
            "status" if args.len() == 5 => {
                return Ok(
                    json!({"id":id,"state":state,"priority":data.get(&"priority".into()).and_then(|t|if let Tag::Int(n)=t{Some(*n)}else{None}).unwrap_or(0)}),
                );
            }
            "install" if args.len() == 5 => {
                let held = self.inventory[self.selected]
                    .clone()
                    .ok_or("hold an upgrade card")?;
                if held.count == 0 {
                    return Err("invalid held card count".into());
                }
                let mut card = held.clone();
                card.count = 1;
                let mut cards = bus_upgrade_slots(data)?;
                let slot = cards
                    .iter()
                    .position(Option::is_none)
                    .ok_or("bus upgrade slots full")?;
                cards[slot] = Some(card.clone());
                store_bus_upgrade_slots(data, &cards)?;
                state = bus_state(data)?;
                data.insert("bus_next_tick".into(), Tag::Long(0));
                let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&pos)
                else {
                    return Err("cable disappeared".into());
                };
                machine.config.insert("parts".into(), Tag::List(parts));
                self.inventory[self.selected] = if held.count <= 1 {
                    None
                } else {
                    Some(ItemStack {
                        count: held.count - 1,
                        ..held
                    })
                };
                self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
                return Ok(json!({"installed":card.item.as_ref(),"state":state}));
            }
            "remove" if args.len() == 6 => {
                let index: usize = args[5].parse().map_err(|_| "invalid upgrade slot")?;
                let mut cards = bus_upgrade_slots(data)?;
                let card = cards
                    .get_mut(index)
                    .ok_or("upgrade slot out of range")?
                    .take()
                    .ok_or("empty upgrade slot")?;
                store_bus_upgrade_slots(data, &cards)?;
                if card.item.as_ref() == "ae2:crafting_card" && !export_has_crafting_card(data) {
                    mark_export_card_removed(data);
                }
                data.insert("bus_next_tick".into(), Tag::Long(0));
                let before = self.inventory.clone();
                if self.give(card.clone()).is_some() {
                    self.inventory = before;
                    return Err("inventory full".into());
                }
                let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&pos)
                else {
                    self.inventory = before;
                    return Err("cable disappeared".into());
                };
                machine.config.insert("parts".into(), Tag::List(parts));
                self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
                return Ok(json!({"removed":card.item.as_ref()}));
            }
            "filter" if args.len() == 6 => {
                let slot: usize = args[5].parse().map_err(|_| "invalid filter slot")?;
                let slots = if id == "ae2:formation_plane" {
                    (18 + usize::from(state.capacity_cards) * 9).min(state.config.len())
                } else {
                    state.slots()
                };
                if slot >= slots {
                    return Err("filter slot not enabled by capacity cards".into());
                }
                state.config[slot] = self.inventory[self.selected]
                    .as_ref()
                    .map(ae2_lab::key)
                    .transpose()?
                    .as_ref()
                    .map(Key::from);
            }
            "filter_fluid" if args.len() == 6 => {
                let slot: usize = args[5].parse().map_err(|_| "invalid filter slot")?;
                if slot >= state.slots() {
                    return Err("filter slot not enabled by capacity cards".into());
                }
                let held = self.inventory[self.selected]
                    .as_ref()
                    .ok_or("hold a fluid bucket")?;
                let fluid = super::ae2_fluids::bucket_fluid(&held.item)
                    .ok_or("hold a supported fluid bucket")?;
                state.config[slot] = Some(Key::from(&super::ae2_fluids::fluid_key(fluid)));
            }
            "redstone" if args.len() == 6 => {
                let mode = match args[5] {
                    "ignore" => Redstone::Ignore,
                    "high" => Redstone::High,
                    "low" => Redstone::Low,
                    "pulse" => Redstone::Pulse,
                    _ => return Err("invalid redstone mode".into()),
                };
                let at = horizon_core::position::BlockPos::new(pos.0, pos.1, pos.2);
                let signal =
                    self.with_level(|b, level, ticks| b.has_neighbor_signal(level, ticks, at));
                state.set_redstone_mode(mode, signal);
            }
            "fuzzy" if args.len() == 6 => {
                if ![
                    "ignore_all",
                    "percent_99",
                    "percent_75",
                    "percent_50",
                    "percent_25",
                ]
                .contains(&args[5])
                {
                    return Err("invalid fuzzy mode".into());
                }
                data.insert("fuzzy_mode".into(), Tag::String(args[5].into()));
            }
            "schedule" if args.len() == 6 => {
                state.schedule = match args[5] {
                    "default" => ae2_machines_core::Schedule::Default,
                    "round_robin" => ae2_machines_core::Schedule::RoundRobin,
                    "random" => ae2_machines_core::Schedule::Random,
                    _ => return Err("invalid schedule".into()),
                };
            }
            "priority"
                if args.len() == 6
                    && matches!(id.as_str(), "ae2:storage_bus" | "ae2:formation_plane") =>
            {
                let n: i32 = args[5].parse().map_err(|_| "invalid priority")?;
                data.insert("priority".into(), Tag::Int(n));
            }
            "access" if args.len() == 6 && id == "ae2:storage_bus" => {
                if !["read_write", "read_only", "write_only"].contains(&args[5]) {
                    return Err("invalid access".into());
                }
                data.insert("access".into(), Tag::String(args[5].into()));
            }
            _ => return Err("invalid bus operation".into()),
        }
        let json = serde_json::to_string(&state).map_err(|e| e.to_string())?;
        data.insert("state_json".into(), Tag::String(json.as_str().into()));
        data.insert("bus_next_tick".into(), Tag::Long(0));
        let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&pos) else {
            return Err("cable disappeared".into());
        };
        machine.config.insert("parts".into(), Tag::List(parts));
        self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
        if id == "ae2:storage_bus" && matches!(args[0], "access" | "fuzzy") {
            if let (Some(cable),Some(adjacent))=(self.ae2_local_address(pos),target(pos,face).and_then(|p|self.ae2_local_address(p))) {
                self.ae2_monitor_setting_changed(cable,adjacent);
            }
        }
        Ok(json!({"configured":true}))
    }

    /// Called once each authoritative local world tick; each part persists
    /// its own source-derived 5..40/60 adaptive rate and pulse state.
    pub(super) fn tick_ae2_buses(&mut self) {
        self.tick_ae2_buses_inner(false);
    }
    /// Only IOBusPart's grid tickables run here; entity collision and other
    /// host block/entity behavior remains governed by vanilla freezing.
    pub(super) fn tick_ae2_frozen_buses(&mut self) {
        self.tick_ae2_buses_inner(true);
    }
    fn tick_ae2_buses_inner(&mut self, frozen: bool) {
        if self.remote.is_some() {
            return;
        }
        let now = self.ae2_server_tick();
        let network = self.ae2_network();
        let analysis = network.analyze().ok();
        let services = self.ae2_event_service_ids(&network);
        self.ae2_device_scheduler.borrow_mut().prune(now);
        let mut positions: Vec<_> = self
            .world
            .block_entities
            .iter()
            .filter_map(|(p, b)| match b {
                BlockEntity::Ae2(m)
                    if m.kind == MachineKind::CableBus
                        && matches!(m.config.get(&"parts".into()), Some(Tag::List(_))) =>
                {
                    Some(*p)
                }
                _ => None,
            })
            .collect();
        positions.sort_unstable();
        for pos in positions {
            let parts = match self.world.block_entities.get(&pos) {
                Some(BlockEntity::Ae2(m)) => match m.config.get(&"parts".into()) {
                    Some(Tag::List(p)) => p.clone(),
                    _ => continue,
                },
                _ => continue,
            };
            let original_parts = parts.clone();
            let previous_parts = self.world.block_entities.ae2_journal_enabled().then(|| parts.clone());
            let mut updated = parts;
            let mut emitter_changed = Vec::new();
            let mut craft_requests = Vec::new();
            for part in &mut updated {
                let Tag::Compound(data) = part else { continue };
                let Some(id) = str_field(data, "id") else {
                    continue;
                };
                if frozen && !matches!(id.as_str(), "ae2:import_bus" | "ae2:export_bus" | "ae2:annihilation_plane" | "ae2:energy_level_emitter") { continue; }
                if id == "ae2:annihilation_plane" {
                    let Some(face) = str_field(data, "face") else {
                        continue;
                    };
                    let online=self.ae2_part_online(pos,&face);
                    // Entity collision pickup: capture only real dropped stacks
                    // intersecting the plane, and remove exactly what the grid accepted.
                    if !frozen && online {
                    let hits: Vec<_> = self
                        .dropped
                        .iter()
                        .filter(|e| {
                            if self
                                .res
                                .items()
                                .item_tags
                                .get("ae2:blacklisted/annihilation_plane")
                                .is_some_and(|tag| tag.contains(e.stack.item.as_ref()))
                            {
                                return false;
                            }
                            let x = e.position.x;
                            let y = e.position.y;
                            let z = e.position.z;
                            let (px, py, pz) =
                                (f64::from(pos.0), f64::from(pos.1), f64::from(pos.2));
                            let cx = x > px && x < px + 1.0;
                            let cy = y + 0.125 > py && y + 0.125 < py + 1.0;
                            let cz = z > pz && z < pz + 1.0;
                            match face.as_str() {
                                "down" => cx && cz && y < py + 0.1 && y + 0.25 > py - 0.125,
                                "up" => cx && cz && y > py + 0.9 && y < py + 1.125,
                                "north" => cx && cy && z < pz + 0.1 && z + 0.125 > pz,
                                "south" => cx && cy && z > pz + 0.9 && z - 0.125 < pz + 1.0,
                                "west" => cz && cy && x < px + 0.1 && x + 0.125 > px,
                                "east" => cz && cy && x > px + 0.9 && x - 0.125 < px + 1.0,
                                _ => false,
                            }
                        })
                        .map(|e| (e.id, e.stack.clone()))
                        .collect();
                    for (entity, stack) in hits {
                        let moved = self
                            .ae2_grid_insert(pos, &stack, u64::from(stack.count))
                            .unwrap_or(0)
                            .min(u64::from(stack.count)) as u32;
                        if moved > 0 {
                            if let Some(e) = self.dropped.iter_mut().find(|e| {
                                e.id == entity && e.stack.same_item_same_components(&stack)
                            }) {
                                e.stack.count = e.stack.count.saturating_sub(moved);
                            }
                        }
                    }
                    self.dropped.retain(|e| e.stack.count > 0);
                    }
                    let Some(address)=self.ae2_local_address(pos) else {continue};
                    let Some(side)=super::ae2_grid::face_index(&face) else {continue};
                    let Some(identity)=network.part_identities.get(&(address.network_pos(),side)).copied() else {continue};
                    let service=services.as_ref().and_then(|s|s.get(&identity).copied());
                    let (min,max)=horizon_client::ae2_tick_config::rates(horizon_client::ae2_tick_config::Kind::AnnihilationPlane);
                    if face == "up" && pos.1 >= self.dim.max_y() {
                        let (due, continuous) = {
                            let mut scheduler=self.ae2_device_scheduler.borrow_mut();
                            let due=scheduler.due(identity,service,now,min,max,!online,false);
                            let continuous=scheduler.continuous_mode(identity,horizon_client::ae2_common_config::gameplay().annihilation_sky_dust);
                            (due,continuous)
                        };
                        if continuous {
                            if due {
                                let amount=self.ae2_device_scheduler.borrow_mut().continuous_generate(identity,now,200);
                                if amount>0 {
                                    let stack=ItemStack::new("ae2:sky_dust",amount.min(u32::MAX as u64) as u32);
                                    let _=self.ae2_grid_insert(pos,&stack,amount);
                                }
                                self.ae2_device_scheduler.borrow_mut().completed(identity,now,super::ae2_tick_tracker::Modulation::Idle);
                            }
                            continue;
                        }
                    }
                    let Some(front)=target(pos,&face) else {continue};
                    let block=self.world.block(front.0,front.1,front.2);
                    let facts=self.res.facts().get(block);
                    let eligible=facts.hardness>=0.0 && !matches!(facts.name.as_ref(),"minecraft:air"|"minecraft:cave_air"|"minecraft:void_air"|"minecraft:water"|"minecraft:lava")
                        && self.ae2_plane_supported_target(front)
                        && !self.res.items().block_in_tag("ae2:blacklisted/annihilation_plane",&facts.name);
                    let token=format!("{}:{online}:{eligible}",block.0);
                    data.remove(&"plane_next_tick".into());
                    let due=self.ae2_device_scheduler.borrow_mut().due_with_token(identity,service,now,min,max,!online || !eligible,token.as_bytes());
                    if due {
                        let part_stack = data
                            .get(&"stack".into())
                            .and_then(horizon_client::save::item_from_nbt);
                        let picked=self.ae2_plane_pickup_block(pos, &face, part_stack.as_ref()).unwrap_or(false);
                        let modulation=if picked {super::ae2_tick_tracker::Modulation::Urgent} else {super::ae2_tick_tracker::Modulation::Idle};
                        self.ae2_device_scheduler.borrow_mut().completed(identity,now,modulation);
                    }
                    continue;
                }
                if matches!(id.as_str(), "ae2:toggle_bus" | "ae2:inverted_toggle_bus") {
                    let at = horizon_core::position::BlockPos::new(pos.0, pos.1, pos.2);
                    let signal =
                        self.with_level(|b, level, ticks| b.has_neighbor_signal(level, ticks, at));
                    data.insert("toggle_signal".into(), Tag::Byte(i8::from(signal)));
                    continue;
                }
                if id == "ae2:energy_level_emitter" {
                    let Some(face)=str_field(data,"face") else{continue};
                    let Ok(state)=super::ae2_emitter::energy_emitter_read(data) else{continue};
                    let (Some(host),Some(face_index))=(self.ae2_local_address(pos),super::ae2_grid::face_index(&face)) else{continue};
                    let Some(identity)=network.part_identities.get(&(host.network_pos(),face_index)).copied() else{continue};
                    let online=analysis.as_ref().and_then(|a|network.part_status(a,host.network_pos(),face_index)).is_some_and(|p|p.online);
                    let value=self.ae2_live_events.borrow_mut().energy.observation(identity,state,online);
                    let (reported,on)=value.unwrap_or((0,false));
                    if data.get(&"emitter_active".into())!=Some(&Tag::Byte(i8::from(on))){emitter_changed.push(face.clone());}
                    data.insert("emitter_count".into(),Tag::Long(reported));data.insert("emitter_active".into(),Tag::Byte(i8::from(on)));
                    continue;
                }
                if id == "ae2:level_emitter" {
                    let Some(face)=str_field(data,"face") else {continue};
                    let Ok(emitter)=super::ae2_emitter::emitter_read(data) else {continue};
                    let online=self.ae2_part_online(pos,&face);
                    let crafting=emitter.upgrades.iter().flatten().any(|s|s.item.as_ref()=="ae2:crafting_card");
                    let fuzzy=emitter.upgrades.iter().flatten().any(|s|s.item.as_ref()=="ae2:fuzzy_card");
                    let stop_early=emitter.filter.is_none()||fuzzy;
                    let mut count=0i64;let mut enough=false;
                    if online && !crafting {
                        if let Ok(items)=self.ae2_grid_inventory(pos) {
                            for (key,amount) in items {
                                let max=self.res.items().get(&key.item_id).map_or(0,|i|i.max_damage);
                                if super::ae2_emitter::emitter_matches_item(&emitter,&key,max) {
                                    count=count.wrapping_add(amount.min(i64::MAX as u64) as i64);
                                    if stop_early&&count>emitter.threshold {enough=true;break}
                                }
                            }
                        }
                        if !enough {
                            if let Ok(fluids)=self.ae2_grid_fluids(pos) {
                                for (key,amount) in fluids {
                                    if super::ae2_emitter::emitter_matches_fluid(&emitter,&key) {
                                        count=count.wrapping_add(amount.min(i64::MAX as u64) as i64);
                                        if stop_early&&count>emitter.threshold {break}
                                    }
                                }
                            }
                        }
                    }
                    let on=if !online {false} else if crafting {
                        let requested=match emitter.filter.as_ref() {
                            Some(super::ae2_emitter::EmitterFilter::Item(stack))=>{
                                let mut item=stack.clone();item.count=1;
                                Some(horizon_client::item_data::canon_stack(Some(&item)))
                            },
                            Some(super::ae2_emitter::EmitterFilter::Fluid(key))=>Some(format!("fluid|{}|{}",key.item_id,key.components.iter().map(|byte|format!("{byte:02x}")).collect::<String>())),
                            None=>None,
                        };
                        match (self.ae2_local_address(pos),super::ae2_grid::face_index(&face)) {
                            (Some(host),Some(face))=>self.ae2_crafting_is_requesting(super::ae2_parts::PartAddress{host,face},requested.as_deref()).unwrap_or(false),
                            _=>false,
                        }
                    }else{super::ae2_emitter::emitter_threshold_signal(&emitter,count)};
                    if data.get(&"emitter_active".into())!=Some(&Tag::Byte(i8::from(on))){emitter_changed.push(face.clone());}
                    data.insert("emitter_count".into(),Tag::Long(count));
                    data.insert("emitter_active".into(),Tag::Byte(i8::from(on)));
                    continue;
                }
                let direction = match id.as_str() {
                    "ae2:import_bus" => Direction::Import,
                    "ae2:export_bus" => Direction::Export,
                    _ => continue,
                };
                let Some(face) = str_field(data, "face") else {
                    continue;
                };
                let Some(adjacent) = target(pos, &face) else {
                    continue;
                };
                let Ok(mut state) = bus_state(data) else {
                    continue;
                };
                if state.redstone_card && state.redstone != Redstone::Ignore {
                    let at = horizon_core::position::BlockPos::new(pos.0, pos.1, pos.2);
                    let signal =
                        self.with_level(|b, level, ticks| b.has_neighbor_signal(level, ticks, at));
                    state.observe_redstone(signal);
                }
                let alert = matches!(data.remove(&"bus_next_tick".into()), Some(Tag::Long(0)))
                    || (state.redstone == Redstone::Pulse && state.pending_pulse);
                data.remove(&"bus_tick_rate".into());
                let kind = if direction == Direction::Import {horizon_client::ae2_tick_config::Kind::ImportBus} else {horizon_client::ae2_tick_config::Kind::ExportBus};
                let (min_rate,max_rate)=horizon_client::ae2_tick_config::rates(kind);
                let Some(address) = self.ae2_local_address(pos) else { continue };
                let Some(face_index) = super::ae2_grid::face_index(&face) else { continue };
                let Some(identity) = network.part_identities.get(&(address.network_pos(), face_index)).copied() else { continue };
                let service = services.as_ref().and_then(|s|s.get(&identity).copied());
                let due = self.ae2_device_scheduler.borrow_mut().due(identity,service,now,min_rate,max_rate,state.sleeping(),alert);
                if due {
                    let active = self.ae2_part_online(pos, &face);
                    let Some(neighbor_face) = target_face(&face) else {
                        continue;
                    };
                    let Some(address) = self.ae2_local_address(adjacent) else {
                        continue;
                    };
                    let loaded = self.ae2_block_entity(address).is_some();
                    let fuzzy_mode =
                        str_field(data, "fuzzy_mode").unwrap_or_else(|| "ignore_all".into());
                    if ![
                        "ignore_all",
                        "percent_99",
                        "percent_75",
                        "percent_50",
                        "percent_25",
                    ]
                    .contains(&fuzzy_mode.as_str())
                    {
                        continue;
                    }
                    let owner = if direction == Direction::Export && export_has_crafting_card(data)
                    {
                        let face_index = match face.as_str() {
                            "east" => 0,
                            "west" => 1,
                            "up" => 2,
                            "down" => 3,
                            "south" => 4,
                            "north" => 5,
                            _ => continue,
                        };
                        self.ae2_local_address(pos).map(|host| ExportBusCraftOwner {
                            host,
                            face: face_index,
                        })
                    } else {
                        None
                    };
                    let links = export_craft_links(data).unwrap_or_default();
                    let craft = CraftState {
                        owner,
                        links,
                        only: matches!(data.get(&"craft_only".into()), Some(Tag::Byte(1))),
                        requests: Vec::new(),
                    };
                    // The core invokes source-style crafting per configured slot
                    // against the remaining operation budget. Requests are
                    // submitted only after the physical part state is saved.
                    let mut host = MixedHost {
                        game: self,
                        cable: pos,
                        target: adjacent,
                        face: neighbor_face,
                        fuzzy_mode,
                        craft,
                    };
                    let outcome = state.tick(direction, active, loaded, &mut host);
                    craft_requests.extend(host.craft.requests);
                    let modulation = match outcome {
                        Ok(ae2_machines_core::TickResult::Faster) => super::ae2_tick_tracker::Modulation::Faster,
                        Ok(ae2_machines_core::TickResult::Slower) => super::ae2_tick_tracker::Modulation::Slower,
                        Ok(ae2_machines_core::TickResult::Sleep) => super::ae2_tick_tracker::Modulation::Sleep,
                        Ok(ae2_machines_core::TickResult::Idle) | Err(_) => super::ae2_tick_tracker::Modulation::Idle,
                    };
                    self.ae2_device_scheduler.borrow_mut().completed(identity,now,modulation);
                }
                if let Ok(json) = serde_json::to_string(&state) {
                    data.insert("state_json".into(), Tag::String(json.as_str().into()));
                }
            }
            let topology_changes: Vec<_> = previous_parts.as_ref().map(|old| old.iter().zip(&updated)
                .filter(|(before, after)| matches!(before.get("id"), Some(Tag::String(id)) if id.to_string().is_ok_and(|s| matches!(s.as_str(), "ae2:toggle_bus" | "ae2:inverted_toggle_bus")))
                    && before.get("toggle_signal") != after.get("toggle_signal"))
                .map(|(before,after)| (before.clone(),after.clone())).collect()).unwrap_or_default();
            let mut committed = false;
            if let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&pos) {
                let dirty = !only_energy_emitter_runtime_changes(&original_parts,&updated);
                let next = Tag::List(updated);
                if m.config.get(&"parts".into()) != Some(&next) {
                    m.config.insert("parts".into(), next);
                    if dirty {self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));}
                    committed = true;
                }
            }
            if committed { for (before,after) in topology_changes { self.world.block_entities.record_ae2_part_change(pos,before,after); } }
            for (owner, slot, key, fluid, amount) in craft_requests {
                // Planning failures (unavailable recipe/CPU/inputs) simply let
                // the bus retry on its normal next tick, as upstream does.
                let _ = self.ae2_begin_export_craft(owner, slot, &key, fluid, amount);
            }
            if !emitter_changed.is_empty() {
                let at = horizon_core::position::BlockPos::new(pos.0, pos.1, pos.2);
                let state = self.world.block(pos.0, pos.1, pos.2);
                let adjacent: Vec<_> = emitter_changed
                    .iter()
                    .filter_map(|face| target(pos, face))
                    .map(|p| horizon_core::position::BlockPos::new(p.0, p.1, p.2))
                    .collect();
                self.with_level(|b, level, ticks| {
                    b.update_neighbors_at(level, ticks, at, b.block(state));
                    for target in adjacent {
                        b.update_neighbors_at(level, ticks, target, b.block(state));
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn craft_preflight_spends_budget_without_duplicating_in_flight_requests() {
        let address = encoded((0, 64, 0)).unwrap();
        let owner = ExportBusCraftOwner { host: address, face: 0 };
        let key = ae2_cell_core::ItemKey { item_id: "minecraft:diamond".into(), components: vec![] };
        let mut craft = CraftState { owner: Some(owner), links: vec![None, Some(address)], ..Default::default() };
        assert_eq!(craft.record_preflight(owner, 0, key.clone(), false, 8), 8);
        assert_eq!(craft.requests.len(), 1);
        // A queued or linked request is not duplicated, but AE2 still consumes
        // the amount accepted by the adjacent target's simulation.
        assert_eq!(craft.record_preflight(owner, 0, key.clone(), false, 8), 8);
        assert_eq!(craft.record_preflight(owner, 1, key, false, 8), 8);
        assert_eq!(craft.requests.len(), 1);
    }
    #[test]
    fn storage_bus_listing_and_extraction_follow_source_switches() {
        use ae2_machines_core::Access;
        let address=super::super::ae2_global::GridAddress::from_network(ae2_network_core::Pos{x:0,y:64,z:0}).unwrap();
        for (fluid, accepted, rejected) in [
            (false, "minecraft:diamond", "minecraft:gold_ingot"),
            (true, "minecraft:water", "minecraft:lava"),
        ] {
            let accepted=Key{id:accepted.into(),components:Vec::new()};
            let rejected=Key{id:rejected.into(),components:Vec::new()};
            let mut mount=StorageMount {
                identity:ae2_network_core::NodeIdentity{host:1,part:None,outer:false},
                target:address,cable:address,face:horizon_core::geometry::Direction::North,
                priority:0,access:Access::ReadWrite,filter:vec![accepted.clone()],inverted:false,
                filter_on_extract:true,extractable_only:true,fuzzy:false,fluid,
                fuzzy_mode:"ignore_all".into(),damage_limits:Default::default(),
                void_overflow:false,monitor_configuration:String::new(),
            };
            assert!(mount.visible(&accepted));
            assert!(!mount.visible(&rejected));
            assert!(!mount.allows(&rejected,true));
            mount.extractable_only=false;
            assert!(mount.visible(&rejected));
            assert!(!mount.allows(&rejected,true));
            mount.access=Access::WriteOnly;
            assert!(mount.visible(&accepted));
            assert!(!mount.allows(&accepted,true));
            assert!(mount.allows(&accepted,false));
            mount.filter_on_extract=false;
            mount.extractable_only=true;
            assert!(mount.visible(&rejected));
            assert!(mount.allows(&rejected,true));
            assert!(!mount.allows(&rejected,false));
            mount.access=Access::ReadOnly;
            assert!(!mount.allows(&accepted,false));
            assert!(mount.allows(&accepted,true));
        }
    }
    #[test]
    fn sparse_bus_cards_preserve_holes_and_still_enforce_upgrade_limits() {
        let mut part = Compound::new();
        part.insert("id".into(), Tag::String("ae2:export_bus".into()));
        let mut slots = vec![None; 5];
        slots[0] = Some(ItemStack::new("ae2:capacity_card", 1));
        slots[3] = Some(ItemStack::new("ae2:speed_card", 1));
        store_bus_upgrade_slots(&mut part, &slots).unwrap();
        assert_eq!(bus_upgrade_slots(&part).unwrap(), slots);
        let state = bus_state(&part).unwrap();
        assert_eq!(state.capacity_cards, 1);
        assert_eq!(state.speed_cards, 1);
        slots[2] = Some(ItemStack::new("minecraft:diamond", 1));
        store_bus_upgrade_slots(&mut part, &slots).unwrap();
        assert!(bus_state(&part).is_err());
    }
    #[test]
    fn export_crafting_links_roundtrip_and_mark_removed_card() {
        let mut data = Compound::new();
        let cpu = super::super::ae2_global::GridAddress::from_network(ae2_network_core::Pos {
            x: 2,
            y: 12340,
            z: -8,
        })
        .unwrap();
        let mut links = vec![None; 63];
        links[20] = Some(cpu);
        store_export_craft_links(&mut data, &links);
        assert_eq!(export_craft_links(&data).unwrap(), links);
        let original_generation = super::super::ae2_autocrafting::crafting_card_generation(&data);
        mark_export_card_removed(&mut data);
        assert_ne!(super::super::ae2_autocrafting::crafting_card_generation(&data), original_generation);
        assert!(export_link_cancelled(&data, 20));
        assert!(!export_link_cancelled(&data, 19));
        let Some(Tag::List(entries)) = data.get_mut(&BUS_CRAFT_LINKS.into()) else {
            panic!("missing links")
        };
        entries.push(entries[0].clone());
        assert!(export_craft_links(&data).is_err());
    }
    #[test]
    fn fuzzy_partitions_follow_upstream_durability_breakpoints() {
        let mk = |damage| {
            let mut s = ItemStack::new("minecraft:test_tool", 1);
            s.damage = damage;
            Key::from(&ae2_lab::key(&s).unwrap())
        };
        let a = mk(25);
        let b = mk(26);
        let pristine = mk(0);
        assert!(fuzzy_key_match(&a, &b, "ignore_all", 100));
        assert!(!fuzzy_key_match(&a, &b, "percent_75", 100));
        assert!(fuzzy_key_match(&a, &b, "percent_50", 100));
        assert!(!fuzzy_key_match(&pristine, &b, "percent_99", 100));
        assert!(fuzzy_key_match(&pristine, &b, "percent_99", 0));
        assert!(!fuzzy_key_match(&a, &b, "invalid", 100));
    }
    #[test]
    fn part_faces_and_persisted_bus_state() {
        assert_eq!(target((1, 2, 3), "north"), Some((1, 2, 2)));
        assert_eq!(target((1, 2, 3), "east"), Some((2, 2, 3)));
        assert_eq!(target((1, 2, 3), "bogus"), None);
        let mut state = BusState::new(Channel::Item);
        state.speed_cards = 2;
        state.next_slot = 4;
        let json = serde_json::to_string(&state).unwrap();
        let card = save::item_to_nbt(&ItemStack::new("ae2:speed_card", 1));
        let part = match Tag::compound([
            ("id", Tag::String("ae2:import_bus".into())),
            ("state_json", Tag::String(json.as_str().into())),
            ("upgrades", Tag::List(vec![card.clone(), card])),
        ]) {
            Tag::Compound(v) => v,
            _ => unreachable!(),
        };
        let back = bus_state(&part).unwrap();
        assert_eq!(back.speed_cards, 2);
        assert_eq!(back.next_slot, 4);
    }
}

impl Game<'_> {
    /// Configure one real level emitter without consuming the filter item.
    /// /ae2 emitter status|filter|clear|threshold|mode X Y Z FACE [value]
    pub(super) fn ae2_emitter_command(
        &mut self,
        args: &[&str],
    ) -> Result<serde_json::Value, String> {
        if args.len() < 5 {
            return Err(
                "usage: /ae2 emitter status|filter|clear|threshold|mode x y z face [value]".into(),
            );
        }
        let pos = (
            args[1].parse().map_err(|_| "invalid x")?,
            args[2].parse().map_err(|_| "invalid y")?,
            args[3].parse().map_err(|_| "invalid z")?,
        );
        let face = args[4];
        if target(pos, face).is_none() {
            return Err("invalid face".into());
        }
        let held = self.inventory[self.selected].clone();
        let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&pos) else {
            return Err("missing cable bus".into());
        };
        let Some(Tag::List(parts)) = machine.config.get_mut(&"parts".into()) else {
            return Err("missing emitter".into());
        };
        let Some(Tag::Compound(part)) = parts.iter_mut().find(|p| {
            p.get("id") == Some(&Tag::String("ae2:level_emitter".into()))
                && p.get("face") == Some(&Tag::String(face.into()))
        }) else {
            return Err("missing emitter".into());
        };
        match args[0] {
            "status" if args.len() == 5 => {
                return Ok(
                    json!({"active":part.get(&"emitter_active".into())==Some(&Tag::Byte(1)),"count":match part.get(&"emitter_count".into()){Some(Tag::Long(v))=>*v,_=>0},"threshold":match part.get(&"emitter_threshold".into()){Some(Tag::Long(v))=>*v,_=>0},"low":part.get(&"emitter_low".into())==Some(&Tag::Byte(1))}),
                );
            }
            "filter" if args.len() == 5 => {
                let mut item = held.ok_or("hold a filter item")?;
                item.count = 1;
                part.insert(
                    "emitter_filter".into(),
                    horizon_client::save::item_to_nbt(&item),
                );
            }
            "clear" if args.len() == 5 => {
                part.remove(&"emitter_filter".into());
            }
            "threshold" if args.len() == 6 => {
                let value: i64 = args[5].parse().map_err(|_| "invalid threshold")?;
                if value < 0 {
                    return Err("invalid threshold".into());
                }
                part.insert("emitter_threshold".into(), Tag::Long(value));
            }
            "mode" if args.len() == 6 && matches!(args[5], "high" | "low") => {
                part.insert("emitter_low".into(), Tag::Byte(i8::from(args[5] == "low")));
            }
            _ => return Err("invalid emitter operation".into()),
        }
        self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
        Ok(json!({"configured":true}))
    }
}

impl Game<'_> {
    /// Admit modeled block entities whose normal removal and block-loot
    /// components Horizon can preserve. Unknown saved entity types remain
    /// guarded until their side effects and loot are understood.
    fn ae2_plane_supported_target(&self,target:(i32,i32,i32))->bool {
        match self.world.block_entities.get(&target) {
            Some(BlockEntity::Chest(_) | BlockEntity::Furnace(_) | BlockEntity::Brewing(_)
                | BlockEntity::Crafter(_) | BlockEntity::Beacon(_) | BlockEntity::Ae2(_) | BlockEntity::Campfire(_)
                | BlockEntity::Jukebox(_) | BlockEntity::Lectern(_) | BlockEntity::Banner(_)
                | BlockEntity::Sign(_) | BlockEntity::Skull(_)
                | BlockEntity::Spawner(_) | BlockEntity::TrialSpawner(_) | BlockEntity::Vault(_)
                | BlockEntity::CommandBlock(_) | BlockEntity::Beehive(_)
                | BlockEntity::Sculk(_) | BlockEntity::CreakingHeart(_))=>true,
            Some(_)=>false,
            None=>match super::spill::saved_entity(&self.world,[target.0,target.1,target.2]) {
                None=>true,
                Some(tag)=>{
                    let block=&self.res.facts().get(self.world.block(target.0,target.1,target.2)).name;
                    let id=tag.get("id");
                    (block.as_ref()=="minecraft:decorated_pot" && id==Some(&Tag::String("minecraft:decorated_pot".into())))
                        || (block.as_ref()=="minecraft:chiseled_bookshelf" && id==Some(&Tag::String("minecraft:chiseled_bookshelf".into())))
                        || (block.ends_with("_shelf") && id==Some(&Tag::String("minecraft:shelf".into())))
                        || (matches!(block.as_ref(),"minecraft:campfire"|"minecraft:soul_campfire") && id==Some(&Tag::String("minecraft:campfire".into())))
                        || (block.as_ref()=="minecraft:enchanting_table" && id==Some(&Tag::String("minecraft:enchanting_table".into())))
                }
            },
        }
    }
    /// Item pickup strategy, source ItemPickupStrategy: select the
    /// appropriate diamond fake tool, evaluate actual loot, preflight network
    /// capacity and energy, destroy without default drops, then insert the
    /// evaluated items; any unexpected overflow becomes a real item entity.
    /// Modeled block entities preserve their physical contents and block loot.
    fn ae2_plane_pickup_block(
        &mut self,
        pos: (i32, i32, i32),
        face: &str,
        plane: Option<&ItemStack>,
    ) -> Result<bool, String> {
        use horizon_core::random::RandomSource;
        let Some(target) = target(pos, face) else {
            return Ok(false);
        };
        if !self.ae2_plane_supported_target(target) {
            return Ok(false);
        }
        let state = self.world.block(target.0, target.1, target.2);
        let facts = self.res.facts().get(state).clone();
        if facts.hardness < 0.0
            || matches!(
                facts.name.as_ref(),
                "minecraft:air"
                    | "minecraft:cave_air"
                    | "minecraft:void_air"
                    | "minecraft:water"
                    | "minecraft:lava"
            )
            || self
                .res
                .items()
                .block_in_tag("ae2:blacklisted/annihilation_plane", &facts.name)
        {
            return Ok(false);
        }
        let mut tool = None;
        for (tag, item) in [
            ("minecraft:mineable/pickaxe", "minecraft:diamond_pickaxe"),
            ("minecraft:mineable/axe", "minecraft:diamond_axe"),
            ("minecraft:mineable/shovel", "minecraft:diamond_shovel"),
            ("minecraft:mineable/hoe", "minecraft:diamond_hoe"),
        ] {
            if self.res.items().block_in_tag(tag, &facts.name) {
                tool = Some(ItemStack::new(item, 1));
                break;
            }
        }
        let enchantments = plane.map(|s| s.enchantments()).unwrap_or(&[]);
        if tool.is_none() && (facts.requires_tool || !enchantments.is_empty()) {
            tool = Some(ItemStack::new("minecraft:diamond_pickaxe", 1));
        }
        if let Some(tool) = tool.as_mut() {
            for (id, level) in enchantments {
                tool.enchant(id, *level);
            }
        }
        let at=[target.0,target.1,target.2];
        let entity=self.world.block_entities.get(&target).cloned();
        let saved=super::spill::saved_entity(&self.world,at);
        let components=entity.as_ref().and_then(|entity|self.world.block_entities.vanilla_metadata(&target).map(|raw|(entity,raw))).map(|(entity,raw)|{
            let mut current=save::live_vanilla_block_entity(target,entity,&facts.name,self.world.block_names.get(&target).map(String::as_str),Some(raw));
            let table=save::text(raw.get("LootTable"));
            let current_roll=self.world.chunks.get(&(target.0>>4,target.2>>4))
                .and_then(|chunk|chunk.saved.as_deref())
                .and_then(|saved|saved.rolled_loot.iter().find(|roll|roll.pos==target));
            if current_roll.is_some_and(|roll|table.as_deref()==Some(roll.table.as_str())) {
                self.restore_loot_table(target,&mut current);
            }
            let canonical=horizon_client::block_entity_nbt::canonical(&current).unwrap_or(current);
            horizon_client::block_entity_item::pick(&canonical)
        }).or_else(||saved.as_ref().map(horizon_client::block_entity_item::pick));
        if let Some(entity)=entity.as_ref() {
            self.pending.record(at,entity,&facts.name,self.world.block_names.get(&target).cloned());
        } else if let Some(tag)=saved.as_ref() {
            self.pending.record_saved(at,tag);
        }
        let mut drops=self.block_loot_at(at,state,tool.as_ref());
        for stack in &mut drops {self.pending.attach(at,stack);}
        self.pending.clear(at);
        drops.retain(|stack|stack.count>0 && stack.item.as_ref()!="minecraft:air");
        if let Some(components)=components.as_ref() {
            for stack in &mut drops {
                let shulker=stack.item.ends_with("shulker_box");
                if shulker || stack.item.as_ref()==facts.name.as_ref() {
                    if let Some(name)=components.get(&"minecraft:custom_name".into()) {
                        stack.set_custom_name_component(Some(name.clone()));
                    }
                    if shulker {
                        for key in ["minecraft:lock","minecraft:container_loot"] {
                            if let Some(value)=components.get(&key.into()) {
                                stack.data_mut().set_other(key,Some(value.clone()));
                            }
                        }
                        if components.contains_key(&"minecraft:container_loot".into()) {
                            stack.data_mut().container.clear();
                        }
                    }
                }
            }
        }
        if let Some(BlockEntity::Ae2(machine))=entity.as_ref() {
            if let Some(block)=drops.iter_mut().find(|item|item.item.as_ref()==facts.name.as_ref()) {
                let name=self.world.block_names.get(&target).map(String::as_str);
                block.set_custom_name_component(super::ae2_memory_host::machine_name_component(machine,name));
                if matches!(machine.kind,MachineKind::EnergyCell|MachineKind::DenseEnergyCell) && machine.energy>0.0 {
                    block.data_mut().set_other("ae2:stored_energy",Some(Tag::Double(machine.energy.to_bits())));
                }
            }
        }
        if let Some(BlockEntity::Skull(skull))=entity.as_ref() {
            for stack in drops.iter_mut().filter(|item|item.item.ends_with("_skull") || item.item.ends_with("_head")) {
                if let Some(name)=skull.custom_name.as_ref() {
                    stack.set_custom_name_component(Some(name.clone()));
                }
                if stack.item.as_ref()=="minecraft:player_head" {
                    if let Some(profile)=skull.profile.as_ref() {
                        stack.data_mut().set_other("minecraft:profile",Some(profile.clone()));
                    }
                    if let Some(sound)=skull.note_block_sound.as_ref() {
                        stack.data_mut().set_other("minecraft:note_block_sound",Some(Tag::String(sound.as_str().into())));
                    }
                }
            }
        }
        // The source calls level.destroyBlock directly, not playerDestroy:
        // Silk Touch copies the resident bees and honey-level loot components.
        if let Some(BlockEntity::Beehive(hive))=entity.as_ref()
            && tool.as_ref().is_some_and(|tool|tool.enchantment_level("minecraft:silk_touch")>0) {
            let bees=hive.to_nbt().into_iter().find_map(|(key,value)|(key=="bees").then_some(value));
            let honey=self.pack.assets.registry.state(state)
                .and_then(|state|state.properties.get("honey_level")).cloned();
            for stack in drops.iter_mut().filter(|item|item.item.as_ref()==facts.name.as_ref()) {
                if let Some(Tag::List(bees))=bees.as_ref() && !bees.is_empty() {
                    stack.data_mut().set_other("minecraft:bees",Some(Tag::List(bees.clone())));
                }
                if let Some(honey)=honey.as_ref() {
                    stack.data_mut().set_other("minecraft:block_state",Some(Tag::compound([("honey_level",Tag::String(honey.as_str().into()))])));
                }
            }
        }
        let efficiency = plane.map_or(0, |s| s.enchantment_level("minecraft:efficiency"));
        let unbreaking = plane.map_or(0, |s| s.enchantment_level("minecraft:unbreaking"));
        let other = enchantments
            .iter()
            .map(|(_, v)| u64::from(*v))
            .sum::<u64>()
            .saturating_sub(u64::from(efficiency) + u64::from(unbreaking));
        let use_energy = unbreaking == 0
            || self
                .world
                .random
                .next_int_bound(unbreaking.saturating_add(1).min(i32::MAX as u32) as i32)
                .unwrap_or(0)
                == 0;
        let multiplier = if other > 0 { 8.0 * other as f64 } else { 1.0 };
        let required = if use_energy {
            (1.0 + f64::from(facts.hardness)
                + drops.iter().map(|s| f64::from(s.count)).sum::<f64>())
                * multiplier
                * 0.85f64.powf(f64::from(efficiency))
        } else {
            0.0
        };
        if required > 0.0 && self.ae2_take_power(pos, required, true) <= required - 0.1 {
            return Ok(false);
        }
        let mut can_store = drops.is_empty();
        for stack in &drops {
            let result = self.ae2_grid_insert_simulate(pos, stack, u64::from(stack.count));
            if result.is_ok_and(|n| n == u64::from(stack.count)) {
                can_store = true;
            }
        }
        if !can_store {
            return Ok(false);
        }
        self.ae2_prepare_cpu_removal(target)?;
        let block = horizon_core::position::BlockPos::new(target.0, target.1, target.2);
        if !self.with_level(|b, level, ticks| b.destroy_block(level, ticks, block, false)) {
            return Ok(false);
        }
        if matches!(entity.as_ref(),Some(BlockEntity::Sculk(_))) {
            // The host queues sculk removal writes for its own observer;
            // deliver the target's pre-removal callback before pickup returns.
            self.sculk_sync_index();
        }
        if matches!(entity,Some(BlockEntity::Ae2(_)))
            && (matches!(self.world.block_entities.get(&target),Some(BlockEntity::Ae2(_)))
                || super::spill::saved_entity(&self.world,at).is_some())
        {
            // AE2 states currently inherit iron block flags, so the native
            // setBlock path may not call the block-entity removal callback.
            self.with_level(|_,level,_| {level.remove_entity(block,state,true);});
        }
        // Source breakBlockAndStoreExtraItems captures all item entities in
        // the target block AABB inflated by 0.2, including spilled contents.
        let extra:Vec<_>=self.dropped.iter().filter(|e|{
            let p=e.position;
            p.x+0.125>f64::from(target.0)-0.2 && p.x-0.125<f64::from(target.0)+1.2
                && p.y+0.25>f64::from(target.1)-0.2 && p.y<f64::from(target.1)+1.2
                && p.z+0.125>f64::from(target.2)-0.2 && p.z-0.125<f64::from(target.2)+1.2
        }).map(|e|(e.id,e.stack.clone())).collect();
        for (id,stack) in extra {
            let inserted=self.ae2_grid_insert(pos,&stack,u64::from(stack.count)).unwrap_or(0).min(u64::from(stack.count)) as u32;
            if inserted>0 {
                if let Some(entity)=self.dropped.iter_mut().find(|entity|entity.id==id && entity.stack.same_item_same_components(&stack)) {
                    entity.stack.count=entity.stack.count.saturating_sub(inserted);
                }
            }
        }
        self.dropped.retain(|e|e.stack.count>0);
        for mut stack in drops {
            let inserted = self
                .ae2_grid_insert(pos, &stack, u64::from(stack.count))
                .unwrap_or(0)
                .min(u64::from(stack.count)) as u32;
            stack.count -= inserted;
            if stack.count > 0 {
                self.spawn_item(
                    stack,
                    [
                        f64::from(target.0) + 0.5,
                        f64::from(target.1) + 0.5,
                        f64::from(target.2) + 0.5,
                    ],
                    [0.0, 0.1, 0.0],
                    10,
                );
            }
        }
        self.ae2_take_power(pos, required, false);
        Ok(true)
    }
}

pub(super) use super::ae2_formation::FormationPlan;
use super::ae2_formation::{self, FormationAction};
impl Game<'_> {
    /// Priorities advertised by online formation-plane storage providers.
    pub(super) fn ae2_formation_priorities(
        &self,
        anchor: super::ae2_global::GridAddress,
    ) -> Vec<i32> {
        let net = self.ae2_network();
        let Ok(analysis) = net.analyze() else {
            return Vec::new();
        };
        let Some(status) = analysis
            .nodes
            .get(&anchor.network_pos())
            .filter(|s| s.online)
        else {
            return Vec::new();
        };
        let mut priorities = std::collections::BTreeSet::new();
        for node in &analysis.components[status.component].positions {
            let Some(address) = super::ae2_global::GridAddress::from_network(*node) else {
                continue;
            };
            if !self
                .ae2_address_parts(address)
                .is_some_and(|(dim, _)| dim == self.dim)
            {
                continue;
            }
            let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(address) else {
                continue;
            };
            let Some(Tag::List(parts)) = machine.config.get(&"parts".into()) else {
                continue;
            };
            for part in parts {
                let Tag::Compound(data) = part else { continue };
                if str_field(data, "id").as_deref() != Some("ae2:formation_plane") {
                    continue;
                }
                let Some(face) = str_field(data, "face") else {
                    continue;
                };
                let side = match face.as_str() {
                    "east" => 0,
                    "west" => 1,
                    "up" => 2,
                    "down" => 3,
                    "south" => 4,
                    "north" => 5,
                    _ => continue,
                };
                if analysis
                    .nodes
                    .get(node)
                    .and_then(|n| n.parts.get(&side))
                    .is_some_and(|p| p.online)
                {
                    priorities.insert(match data.get(&"priority".into()) {
                        Some(Tag::Int(n)) => *n,
                        _ => 0,
                    });
                }
            }
        }
        priorities.into_iter().rev().collect()
    }
    /// Shared physical Formation targets. Current native placement execution
    /// is scoped to the active dimension; never synthesize unloaded worlds.
    pub(super) fn ae2_formation_targets(
        &self, anchor:super::ae2_global::GridAddress,key:&ae2_cell_core::ItemKey,fluid:bool,priority:i32,
    )->Vec<super::ae2_formation::PlaneTarget> {
        let network = self.ae2_network();
        let Ok(analysis) = network.analyze() else {
            return Vec::new();
        };
        let Some(status) = analysis
            .nodes
            .get(&anchor.network_pos())
            .filter(|s| s.online)
        else {
            return Vec::new();
        };
        let mut targets = Vec::new();
        for node in &analysis.components[status.component].positions {
            let Some(address) = super::ae2_global::GridAddress::from_network(*node) else {
                continue;
            };
            let Some((dim, cable)) = self.ae2_address_parts(address) else {
                continue;
            };
            if dim != self.dim {
                continue;
            }
            let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity(address) else {
                continue;
            };
            let Some(Tag::List(parts)) = m.config.get(&"parts".into()) else {
                continue;
            };
            for part in parts {
                let Tag::Compound(data) = part else { continue };
                if str_field(data, "id").as_deref() != Some("ae2:formation_plane") {
                    continue;
                }
                let plane_priority = match data.get(&"priority".into()) {
                    Some(Tag::Int(n)) => *n,
                    _ => 0,
                };
                if plane_priority != priority {
                    continue;
                }
                let Ok(config) = bus_state(data) else {
                    continue;
                };
                let slots = (18 + usize::from(config.capacity_cards) * 9).min(config.config.len());
                let candidate=Key::from(key);
                let filters: Vec<_> = config.config[..slots].iter().flatten().collect();
                if !filters.is_empty() {
                    let mode = str_field(data, "fuzzy_mode").unwrap_or_else(|| "ignore_all".into());
                    let matched = filters.iter().any(|filter| {
                        if config.fuzzy {
                            if fluid { filter.id==candidate.id } else {
                                let max = self.res.items().get(&filter.id).map_or(0, |i| i.max_damage);
                                fuzzy_key_match(filter, &candidate, &mode, max)
                            }
                        } else {
                            **filter == candidate
                        }
                    });
                    if matched == config.inverted {
                        continue;
                    }
                }
                let Some(face) = str_field(data, "face") else {
                    continue;
                };
                let side = match face.as_str() {
                    "east" => 0,
                    "west" => 1,
                    "up" => 2,
                    "down" => 3,
                    "south" => 4,
                    "north" => 5,
                    _ => continue,
                };
                if !analysis
                    .nodes
                    .get(node)
                    .and_then(|n| n.parts.get(&side))
                    .is_some_and(|p| p.online)
                {
                    continue;
                }
                if let Some(t) = target(cable, &face) {
                    let entity = matches!(data.get(&"formation_entity".into()), Some(Tag::Byte(1)));
                    let Some(identity)=network.part_identities.get(&(*node,side)).copied() else {continue};
                    let player_owner=match part.get("ae2_node_owner_uuid") {Some(Tag::IntArray(words))=>words.as_slice().try_into().ok(),_=>None};
                    targets.push(super::ae2_formation::PlaneTarget {
                        owner:super::ae2_formation::PlaneOwner {address,identity,face:side},
                        host:cable,target:t,entity,player_owner,
                    });
                }
            }
        }
        targets
    }
    /// Source SIMULATE is optimistic. MODULATE stages actual native item use
    /// against all prior placements, preserving components and secondary writes.
    pub(super) fn ae2_form_simple_blocks(
        &mut self,anchor:super::ae2_global::GridAddress,stack:&ItemStack,
        limit:u64,priority:i32,simulate:bool,plan:&mut FormationPlan,
    )->u64 {
        if limit==0 {return 0;}
        let Ok(key)=ae2_lab::key(stack) else {return 0};
        let mut placed=0;
        for target in self.ae2_formation_targets(anchor,&key,false,priority) {
            if placed>=limit {break;}
            let pos=target.target;
            if !self.world.loaded(pos.0,pos.2) {continue;}
            let old=self.world.block(pos.0,pos.1,pos.2);
            if simulate && (self.ae2_item_cached_blocked(target.owner) || !self.res.facts().get(old).replaceable) {self.ae2_item_update_blocked(target.owner,pos);continue;}
            if target.entity {
                let Ok(Some((old,actual)))=self.ae2_preview_formation_entity(plan,&target) else {continue};
                let area=ae2_formation::area(pos);
                let accepted=ae2_formation::accepted(actual.saturating_add(plan.entities_in(self.dim,area)),
                    horizon_client::ae2_common_config::formation_entity_limit(),
                    (limit-placed).min(u64::from(stack.count)),
                    ae2_formation::max_stack(stack,self.res.items().get(&stack.item).map_or(64,|i|i.max_stack)));
                if accepted==0 {continue;}
                let direction=[pos.0-target.host.0,pos.1-target.host.1,pos.2-target.host.2];
                let mut rng=self.world.random.clone();
                for _ in 0..plan.len().saturating_mul(3) {rng.next_float();}
                let position=ae2_formation::spawn_position(target.host,direction,[rng.next_float(),rng.next_float(),rng.next_float()]);
                let mut spawned=stack.clone();spawned.count=accepted;
                plan.actions.push(FormationAction::Entity {dim:self.dim,target:pos,old,position,direction,stack:spawned,plane:target.owner});
                placed+=u64::from(accepted);
            } else if simulate {
                // AE2 checks replaceability here, without calling Item.useOn.
                plan.actions.push(FormationAction::Reservation {dim:self.dim,pos});
                placed+=1;
            } else {
                let mut physical=stack.clone();physical.count=1;
                match self.ae2_preview_formation_block(plan,&target,&physical) {
                    Ok(Some(old))=>{
                        plan.actions.push(FormationAction::Place {dim:self.dim,pos,old,stack:physical,plane:target.owner,player_owner:target.player_owner});
                        placed+=1;
                    },
                    Ok(None)=>{},
                    Err(error)=>{log::debug!("AE2 Formation item preview rejected: {error}");},
                }
                self.ae2_item_update_blocked(target.owner,pos);
            }
        }
        placed
    }
    pub(super) fn ae2_commit_formation(&mut self,plan:FormationPlan)->Result<(),String> {
        self.ae2_execute_formation_plan(plan)
    }

}
