// Derived from AE2 1.21.1, db17504a86128fdf3dae31f5fb7a112a646e0b93.
// Copyright (c) 2021 TeamAppliedEnergistics. LGPL-3.0-or-later; see LICENSE.
use ae2_cell_core::ItemKey;
use serde::{Deserialize, Serialize};

pub mod stored_energy;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Channel {
    Item,
    Fluid,
}
impl Channel {
    pub fn amount_per_operation(self) -> u64 {
        match self {
            Self::Item => 1,
            Self::Fluid => 125,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Key {
    pub id: String,
    pub components: Vec<u8>,
}
impl From<&ItemKey> for Key {
    fn from(k: &ItemKey) -> Self {
        Self {
            id: k.item_id.clone(),
            components: k.components.clone(),
        }
    }
}
impl From<&Key> for ItemKey {
    fn from(k: &Key) -> Self {
        Self {
            item_id: k.id.clone(),
            components: k.components.clone(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Redstone {
    Ignore,
    Low,
    High,
    Pulse,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Schedule {
    Default,
    RoundRobin,
    Random,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Import,
    Export,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickResult {
    Sleep,
    Idle,
    Faster,
    Slower,
}

/// The host owns actual inventories, energy, and transactions. `transfer_atomic`
/// commits no more than limit, and moves an identical quantity from source to
/// destination as one atomic operation; export must also charge energy. If it
/// cannot guarantee this it must return an error without committing anything.
pub trait TransferHost {
    type Error;
    /// Snapshot of adjacent slots in slot order for import.
    fn adjacent_slots(&self) -> Vec<(Key, u64)>;
    /// Key-space conversion; None retains the bus's fixed channel for legacy hosts.
    fn amount_per_operation(&self, _key: &Key) -> Option<u64> {
        None
    }
    /// True when a candidate matches a configured fuzzy key. Exact mode is
    /// handled internally; fuzzy semantics must be supplied by the game host.
    fn fuzzy_matches(&self, filter: &Key, candidate: &Key) -> bool;
    /// Fuzzy export candidates from the network for a configured key.
    fn fuzzy_candidates(&self, filter: &Key) -> Vec<Key>;
    /// Craft-only export skips storage extraction but still uses the same
    /// per-slot operation budget and target-capacity preflight.
    /// Draw a bounded game-world random slot for source RANDOM scheduling.
    fn random_slot(&mut self, _bound: usize) -> usize {
        0
    }
    fn craft_only(&self) -> bool {
        false
    }
    /// Queue a real slot-bound craft request and return the preflighted amount.
    /// Hosts without crafting support retain the original transfer behavior.
    fn request_craft(&mut self, _slot: usize, _key: &Key, _limit: u64) -> Result<u64, Self::Error> {
        Ok(0)
    }
    fn transfer_atomic(
        &mut self,
        direction: Direction,
        key: &Key,
        limit: u64,
    ) -> Result<u64, Self::Error>;
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BusState {
    pub channel: Channel,
    pub config: Vec<Option<Key>>,
    pub speed_cards: u8,
    pub capacity_cards: u8,
    pub redstone_card: bool,
    pub redstone: Redstone,
    pub last_redstone: bool,
    pub pending_pulse: bool,
    pub inverted: bool,
    pub fuzzy: bool,
    pub schedule: Schedule,
    pub next_slot: usize,
}
impl BusState {
    pub fn new(channel: Channel) -> Self {
        Self {
            channel,
            config: vec![None; 63],
            speed_cards: 0,
            capacity_cards: 0,
            redstone_card: false,
            redstone: Redstone::Ignore,
            last_redstone: false,
            pending_pulse: false,
            inverted: false,
            fuzzy: false,
            schedule: Schedule::Default,
            next_slot: 0,
        }
    }
    pub fn slots(&self) -> usize {
        (18 + usize::from(self.capacity_cards) * 9)
            .min(63)
            .min(self.config.len())
    }
    pub fn operations(&self) -> u64 {
        match self.speed_cards {
            1 => 8,
            2 => 32,
            3 => 64,
            4 => 96,
            _ => 1,
        }
    }
    pub fn observe_redstone(&mut self, powered: bool) {
        if self.redstone == Redstone::Pulse && powered && !self.last_redstone {
            self.pending_pulse = true;
        }
        self.last_redstone = powered;
    }
    pub fn set_redstone_mode(&mut self, mode: Redstone, current: bool) {
        self.redstone = mode;
        if mode == Redstone::Pulse {
            self.last_redstone = current;
        } else {
            self.pending_pulse = false;
        }
    }
    pub fn sleeping(&self) -> bool {
        if self.redstone == Redstone::Pulse && self.pending_pulse {
            return false;
        }
        if !self.redstone_card {
            return false;
        }
        match self.redstone {
            Redstone::Ignore => false,
            Redstone::Low => self.last_redstone,
            Redstone::High => !self.last_redstone,
            Redstone::Pulse => true,
        }
    }
    fn allow<H: TransferHost>(&self, h: &H, key: &Key) -> bool {
        // AE2's filter is built from all config slots, not just visible slots.
        let mut filters = self.config.iter().flatten().peekable();
        if filters.peek().is_none() {
            return true;
        }
        let matched = filters.any(|f| {
            if self.fuzzy {
                h.fuzzy_matches(f, key)
            } else {
                f == key
            }
        });
        matched != self.inverted
    }
    pub fn tick<H: TransferHost>(
        &mut self,
        direction: Direction,
        active: bool,
        target_loaded: bool,
        host: &mut H,
    ) -> Result<TickResult, H::Error> {
        if self.sleeping() {
            return Ok(TickResult::Sleep);
        }
        if !active || !target_loaded {
            return Ok(TickResult::Idle);
        }
        self.pending_pulse = false;
        let initial_ops = self.operations();
        let mut ops = initial_ops;
        let factor = self.channel.amount_per_operation();
        match direction {
            Direction::Import => {
                // Import strategies process each key space with a shared operation
                // budget; each strategy retains its own amount budget across slots.
                let mut group_factor = None;
                let mut remaining_amount = 0;
                for (key, _) in host.adjacent_slots() {
                    let key_factor = host.amount_per_operation(&key).unwrap_or(factor).max(1);
                    if group_factor != Some(key_factor) {
                        group_factor = Some(key_factor);
                        remaining_amount = ops.saturating_mul(key_factor);
                    }
                    if remaining_amount == 0 {
                        continue;
                    }
                    if !self.allow(host, &key) {
                        continue;
                    }
                    let moved = host.transfer_atomic(direction, &key, remaining_amount)?;
                    assert!(
                        moved <= remaining_amount,
                        "host violated atomic transfer limit"
                    );
                    if moved > 0 {
                        ops = ops.saturating_sub((moved / key_factor).max(1));
                        remaining_amount -= moved;
                    }
                }
            }
            Direction::Export => {
                let slots = self.slots();
                let mut visited = 0;
                while visited < slots && ops > 0 {
                    let index = match self.schedule {
                        Schedule::Default => visited,
                        Schedule::RoundRobin => (self.next_slot + visited) % slots,
                        Schedule::Random => host.random_slot(slots) % slots,
                    };
                    visited += 1;
                    let Some(key) = self.config[index].clone() else {
                        continue;
                    };
                    let factor = host.amount_per_operation(&key).unwrap_or(factor).max(1);
                    if host.craft_only() {
                        let limit = ops.saturating_mul(factor);
                        let requested = host.request_craft(index, &key, limit)?;
                        assert!(requested <= limit, "host violated craft request limit");
                        if requested > 0 {
                            ops = ops.saturating_sub((requested / factor).max(1));
                        }
                        continue;
                    }
                    let before = ops;
                    let candidates = if self.fuzzy {
                        host.fuzzy_candidates(&key)
                    } else {
                        vec![key.clone()]
                    };
                    for candidate in candidates {
                        let limit = ops * factor;
                        let moved = host.transfer_atomic(direction, &candidate, limit)?;
                        assert!(moved <= limit, "host violated atomic transfer limit");
                        // Upstream consumes at least one op per fuzzy candidate, even if zero moved.
                        if moved > 0 || self.fuzzy {
                            ops = ops.saturating_sub((moved / factor).max(1));
                        }
                        if ops == 0 {
                            break;
                        }
                    }
                    if before == ops {
                        let limit = ops.saturating_mul(factor);
                        let requested = host.request_craft(index, &key, limit)?;
                        assert!(requested <= limit, "host violated craft request limit");
                        if requested > 0 {
                            ops = ops.saturating_sub((requested / factor).max(1));
                        }
                    }
                }
                // StackTransferContextImpl.hasDoneWork compares the initial
                // and remaining operations. Crafting preflight and a fuzzy
                // candidate that moved nothing can both consume an operation.
                if ops < initial_ops && self.schedule == Schedule::RoundRobin && slots > 0 {
                    self.next_slot = (self.next_slot + visited) % slots;
                }
            }
        }
        if self.sleeping() {
            Ok(TickResult::Sleep)
        } else if ops < initial_ops {
            Ok(TickResult::Faster)
        } else {
            Ok(TickResult::Slower)
        }
    }
}
/// AE2 StorageHelper powered extraction affordability. Host must additionally
/// debit the corresponding AE power atomically and apply its configured multiplier.
pub fn powered_export_limit(retrieved: u64, available_ae: f64, channel: Channel) -> u64 {
    let factor = channel.amount_per_operation() as f64;
    ((available_ae.max(0.0) * factor + 0.9) as u64).min(retrieved)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Access {
    ReadWrite,
    ReadOnly,
    WriteOnly,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StorageBus {
    pub priority: i32,
    pub access: Access,
    pub filter: Vec<Key>,
    pub inverted: bool,
    pub filter_on_extract: bool,
}
impl StorageBus {
    pub fn allows(&self, key: &Key, extraction: bool) -> bool {
        if extraction && !self.filter_on_extract {
            return true;
        }
        if (extraction && self.access == Access::WriteOnly)
            || (!extraction && self.access == Access::ReadOnly)
        {
            return false;
        }
        self.filter.is_empty() || (self.filter.contains(key) != self.inverted)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmitterMode {
    HighAtOrAbove,
    HighBelow,
}
pub fn level_emitter(active: bool, threshold: u64, amount: u64, mode: EmitterMode) -> bool {
    active
        && match mode {
            EmitterMode::HighAtOrAbove => amount >= threshold,
            EmitterMode::HighBelow => amount < threshold,
        }
}

/// Formation plane is exposed as a write-only mounted storage endpoint. The
/// host is responsible for Minecraft placement rules, blocked positions, and
/// correctly simulating without mutation.
pub trait FormationHost {
    type Error;
    fn fuzzy_matches(&self, filter: &Key, candidate: &Key) -> bool;
    fn place(
        &mut self,
        key: &Key,
        amount: u64,
        simulate: bool,
        place_block: bool,
    ) -> Result<u64, Self::Error>;
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FormationPlane {
    pub priority: i32,
    pub config: Vec<Option<Key>>,
    pub capacity_cards: u8,
    pub inverted: bool,
    pub fuzzy: bool,
    pub place_block: bool,
}
impl FormationPlane {
    pub fn insert<H: FormationHost>(
        &self,
        online: bool,
        key: &Key,
        amount: u64,
        simulate: bool,
        host: &mut H,
    ) -> Result<u64, H::Error> {
        if !online || amount == 0 {
            return Ok(0);
        }
        let slots = (18 + usize::from(self.capacity_cards) * 9).min(self.config.len());
        let mut filter = self.config[..slots].iter().flatten().peekable();
        if filter.peek().is_some() {
            let matches = filter.any(|f| {
                if self.fuzzy {
                    host.fuzzy_matches(f, key)
                } else {
                    f == key
                }
            });
            if matches == self.inverted {
                return Ok(0);
            }
        }
        let placed = host.place(key, amount, simulate, self.place_block)?;
        assert!(placed <= amount, "host violated placement limit");
        Ok(placed)
    }
}

/// Annihilation pickup strategies are host-defined because Minecraft block,
/// entity, tool and energy behavior must be evaluated by the live world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickupResult {
    PickedUp,
    CantStore,
    NoPickup,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaneTick {
    Sleep,
    Idle,
    Urgent,
}
pub fn annihilation_tick(
    active: bool,
    results: impl IntoIterator<Item = PickupResult>,
) -> PlaneTick {
    if !active {
        return PlaneTick::Sleep;
    }
    for result in results {
        match result {
            PickupResult::PickedUp => return PlaneTick::Urgent,
            PickupResult::CantStore => return PlaneTick::Idle,
            PickupResult::NoPickup => {}
        }
    }
    PlaneTick::Sleep
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn k(id: &str) -> Key {
        Key {
            id: id.into(),
            components: vec![],
        }
    }
    #[derive(Default)]
    struct Host {
        outside: BTreeMap<Key, u64>,
        network: BTreeMap<Key, u64>,
        fail: bool,
    }
    impl TransferHost for Host {
        type Error = &'static str;
        fn adjacent_slots(&self) -> Vec<(Key, u64)> {
            self.outside.iter().map(|(k, v)| (k.clone(), *v)).collect()
        }
        fn fuzzy_matches(&self, f: &Key, c: &Key) -> bool {
            f.id == c.id
        }
        fn fuzzy_candidates(&self, f: &Key) -> Vec<Key> {
            self.network
                .keys()
                .filter(|k| k.id == f.id)
                .cloned()
                .collect()
        }
        fn transfer_atomic(
            &mut self,
            d: Direction,
            key: &Key,
            limit: u64,
        ) -> Result<u64, Self::Error> {
            if self.fail {
                return Err("transaction aborted");
            }
            let (src, dst) = match d {
                Direction::Import => (&mut self.outside, &mut self.network),
                Direction::Export => (&mut self.network, &mut self.outside),
            };
            let n = src.get(key).copied().unwrap_or(0).min(limit);
            if n > 0 {
                *src.get_mut(key).unwrap() -= n;
                *dst.entry(key.clone()).or_default() += n;
            }
            Ok(n)
        }
    }
    #[test]
    fn mixed_key_spaces_share_operations_and_use_their_own_amount_factors() {
        struct Mixed(Host);
        impl TransferHost for Mixed {
            type Error = &'static str;
            fn amount_per_operation(&self, key: &Key) -> Option<u64> {
                Some(if key.id == "water" { 125 } else { 1 })
            }
            fn adjacent_slots(&self) -> Vec<(Key, u64)> {
                vec![
                    (
                        k("diamond"),
                        self.0.outside.get(&k("diamond")).copied().unwrap_or(0),
                    ),
                    (
                        k("water"),
                        self.0.outside.get(&k("water")).copied().unwrap_or(0),
                    ),
                ]
            }
            fn fuzzy_matches(&self, a: &Key, b: &Key) -> bool {
                a == b
            }
            fn fuzzy_candidates(&self, a: &Key) -> Vec<Key> {
                vec![a.clone()]
            }
            fn transfer_atomic(
                &mut self,
                d: Direction,
                key: &Key,
                limit: u64,
            ) -> Result<u64, Self::Error> {
                self.0.transfer_atomic(d, key, limit)
            }
        }
        let mut bus = BusState::new(Channel::Item);
        bus.speed_cards = 1;
        let mut import = Mixed(Host::default());
        import.0.outside.insert(k("diamond"), 3);
        import.0.outside.insert(k("water"), 1000);
        assert_eq!(
            bus.tick(Direction::Import, true, true, &mut import),
            Ok(TickResult::Faster)
        );
        assert_eq!(import.0.network.get(&k("diamond")), Some(&3));
        assert_eq!(import.0.network.get(&k("water")), Some(&625));
        let mut export = Mixed(Host::default());
        export.0.network.insert(k("diamond"), 3);
        export.0.network.insert(k("water"), 1000);
        bus.config[0] = Some(k("diamond"));
        bus.config[1] = Some(k("water"));
        assert_eq!(
            bus.tick(Direction::Export, true, true, &mut export),
            Ok(TickResult::Faster)
        );
        assert_eq!(export.0.outside.get(&k("diamond")), Some(&3));
        assert_eq!(export.0.outside.get(&k("water")), Some(&625));
    }
    #[test]
    fn random_schedule_draws_a_fresh_slot_per_attempt() {
        struct RandomHost {
            indices: std::collections::VecDeque<usize>,
            seen: Vec<Key>,
        }
        impl TransferHost for RandomHost {
            type Error = &'static str;
            fn adjacent_slots(&self) -> Vec<(Key, u64)> {
                Vec::new()
            }
            fn fuzzy_matches(&self, a: &Key, b: &Key) -> bool {
                a == b
            }
            fn fuzzy_candidates(&self, key: &Key) -> Vec<Key> {
                vec![key.clone()]
            }
            fn random_slot(&mut self, _bound: usize) -> usize {
                self.indices.pop_front().unwrap_or(0)
            }
            fn transfer_atomic(
                &mut self,
                _d: Direction,
                key: &Key,
                _limit: u64,
            ) -> Result<u64, Self::Error> {
                self.seen.push(key.clone());
                Ok(0)
            }
        }
        let mut host = RandomHost {
            indices: [1, 0, 1].into(),
            seen: Vec::new(),
        };
        let mut bus = BusState::new(Channel::Item);
        bus.schedule = Schedule::Random;
        bus.config.truncate(3);
        bus.config[0] = Some(k("a"));
        bus.config[1] = Some(k("b"));
        assert_eq!(
            bus.tick(Direction::Export, true, true, &mut host),
            Ok(TickResult::Slower)
        );
        assert_eq!(host.seen, vec![k("b"), k("a"), k("b")]);
    }
    #[test]
    fn export_crafting_uses_remaining_budget_per_slot_and_craft_only_skips_stock() {
        struct CraftHost {
            inner: Host,
            requests: Vec<(usize, Key, u64)>,
            only: bool,
        }
        impl TransferHost for CraftHost {
            type Error = &'static str;
            fn adjacent_slots(&self) -> Vec<(Key, u64)> {
                self.inner.adjacent_slots()
            }
            fn fuzzy_matches(&self, a: &Key, b: &Key) -> bool {
                self.inner.fuzzy_matches(a, b)
            }
            fn fuzzy_candidates(&self, a: &Key) -> Vec<Key> {
                self.inner.fuzzy_candidates(a)
            }
            fn transfer_atomic(
                &mut self,
                d: Direction,
                key: &Key,
                limit: u64,
            ) -> Result<u64, Self::Error> {
                self.inner.transfer_atomic(d, key, limit)
            }
            fn craft_only(&self) -> bool {
                self.only
            }
            fn request_craft(
                &mut self,
                slot: usize,
                key: &Key,
                limit: u64,
            ) -> Result<u64, Self::Error> {
                self.requests.push((slot, key.clone(), limit));
                Ok(limit)
            }
        }
        let mut host = CraftHost {
            inner: Host::default(),
            requests: Vec::new(),
            only: false,
        };
        host.inner.network.insert(k("available"), 3);
        let mut bus = BusState::new(Channel::Item);
        bus.speed_cards = 1;
        bus.config[0] = Some(k("available"));
        bus.config[1] = Some(k("missing"));
        assert_eq!(
            bus.tick(Direction::Export, true, true, &mut host),
            Ok(TickResult::Faster)
        );
        assert_eq!(host.inner.outside.get(&k("available")), Some(&3));
        assert_eq!(host.requests, vec![(1, k("missing"), 5)]);
        host.only = true;
        host.requests.clear();
        host.inner.network.insert(k("available"), 3);
        bus.schedule = Schedule::RoundRobin;
        bus.next_slot = 0;
        assert_eq!(
            bus.tick(Direction::Export, true, true, &mut host),
            Ok(TickResult::Faster)
        );
        assert_eq!(host.inner.network.get(&k("available")), Some(&3));
        assert_eq!(host.requests, vec![(0, k("available"), 8)]);
        assert_eq!(bus.next_slot, 1);
        host.requests.clear();
        assert_eq!(
            bus.tick(Direction::Export, true, true, &mut host),
            Ok(TickResult::Faster)
        );
        assert_eq!(host.requests, vec![(1, k("missing"), 8)]);
        assert_eq!(bus.next_slot, 2);
    }
    #[test]
    fn failed_fuzzy_export_consumes_an_operation_and_rotates() {
        struct RejectingHost;
        impl TransferHost for RejectingHost {
            type Error = ();
            fn adjacent_slots(&self) -> Vec<(Key, u64)> {
                Vec::new()
            }
            fn fuzzy_matches(&self, a: &Key, b: &Key) -> bool {
                a == b
            }
            fn fuzzy_candidates(&self, filter: &Key) -> Vec<Key> {
                vec![filter.clone()]
            }
            fn transfer_atomic(&mut self, _: Direction, _: &Key, _: u64) -> Result<u64, ()> {
                Ok(0)
            }
        }
        let mut bus = BusState::new(Channel::Item);
        bus.schedule = Schedule::RoundRobin;
        bus.fuzzy = true;
        bus.config[0] = Some(k("blocked"));
        bus.config[1] = Some(k("next"));
        let mut host = RejectingHost;
        assert_eq!(
            bus.tick(Direction::Export, true, true, &mut host),
            Ok(TickResult::Faster)
        );
        assert_eq!(bus.next_slot, 1);
        assert_eq!(
            bus.tick(Direction::Export, true, true, &mut host),
            Ok(TickResult::Faster)
        );
        assert_eq!(bus.next_slot, 2);
    }
    #[test]
    fn import_filter_inversion_speed_and_transaction_failure() {
        let mut host = Host::default();
        host.outside.insert(k("a"), 100);
        host.outside.insert(k("b"), 100);
        let mut bus = BusState::new(Channel::Item);
        bus.speed_cards = 1;
        bus.config[0] = Some(k("b"));
        assert_eq!(
            bus.tick(Direction::Import, true, true, &mut host),
            Ok(TickResult::Faster)
        );
        assert_eq!(host.network.get(&k("b")), Some(&8));
        assert_eq!(host.outside.get(&k("a")), Some(&100));
        bus.inverted = true;
        host.fail = true;
        assert_eq!(
            bus.tick(Direction::Import, true, true, &mut host),
            Err("transaction aborted")
        );
        assert_eq!(host.outside.get(&k("a")), Some(&100));
        host.fail = false;
        bus.tick(Direction::Import, true, true, &mut host).unwrap();
        assert_eq!(host.network.get(&k("a")), Some(&8));
    }
    #[test]
    fn export_round_robin_and_pulse_survive_serialization() {
        let mut host = Host::default();
        host.network.insert(k("a"), 10);
        host.network.insert(k("b"), 10);
        let mut bus = BusState::new(Channel::Item);
        bus.config[0] = Some(k("a"));
        bus.config[1] = Some(k("b"));
        bus.schedule = Schedule::RoundRobin;
        bus.redstone_card = true;
        bus.set_redstone_mode(Redstone::Pulse, false);
        bus.observe_redstone(true);
        let json = serde_json::to_string(&bus).unwrap();
        let mut bus: BusState = serde_json::from_str(&json).unwrap();
        assert_eq!(
            bus.tick(Direction::Export, false, true, &mut host).unwrap(),
            TickResult::Idle
        );
        assert!(bus.pending_pulse);
        assert_eq!(
            bus.tick(Direction::Export, true, true, &mut host).unwrap(),
            TickResult::Sleep
        );
        assert_eq!(host.outside.get(&k("a")), Some(&1));
        bus.observe_redstone(false);
        bus.observe_redstone(true);
        bus.tick(Direction::Export, true, true, &mut host).unwrap();
        assert_eq!(host.outside.get(&k("b")), Some(&1));
    }
    #[test]
    fn formation_filters_and_simulates_host_placement() {
        struct World {
            placed: u64,
        }
        impl FormationHost for World {
            type Error = ();
            fn fuzzy_matches(&self, f: &Key, c: &Key) -> bool {
                f.id == c.id
            }
            fn place(&mut self, _: &Key, n: u64, simulate: bool, _: bool) -> Result<u64, ()> {
                let n = n.min(1);
                if !simulate {
                    self.placed += n;
                }
                Ok(n)
            }
        }
        let mut world = World { placed: 0 };
        let plane = FormationPlane {
            priority: 0,
            config: vec![Some(k("stone"))],
            capacity_cards: 0,
            inverted: false,
            fuzzy: false,
            place_block: true,
        };
        assert_eq!(plane.insert(true, &k("dirt"), 10, false, &mut world), Ok(0));
        assert_eq!(plane.insert(true, &k("stone"), 10, true, &mut world), Ok(1));
        assert_eq!(world.placed, 0);
        assert_eq!(
            plane.insert(true, &k("stone"), 10, false, &mut world),
            Ok(1)
        );
        assert_eq!(world.placed, 1);
        assert_eq!(
            annihilation_tick(
                true,
                [
                    PickupResult::NoPickup,
                    PickupResult::CantStore,
                    PickupResult::PickedUp
                ]
            ),
            PlaneTick::Idle
        );
    }
    #[test]
    fn fluid_factor_storage_filter_and_emitter_boundary() {
        let mut host = Host::default();
        host.outside.insert(k("water"), 1000);
        let mut bus = BusState::new(Channel::Fluid);
        bus.tick(Direction::Import, true, true, &mut host).unwrap();
        assert_eq!(host.network.get(&k("water")), Some(&125));
        assert_eq!(powered_export_limit(1000, 2.0, Channel::Fluid), 250);
        let mut storage = StorageBus {
            priority: 0,
            access: Access::ReadWrite,
            filter: vec![k("water")],
            inverted: false,
            filter_on_extract: false,
        };
        assert!(!storage.allows(&k("lava"), false));
        assert!(storage.allows(&k("lava"), true));
        storage.access = Access::WriteOnly;
        assert!(storage.allows(&k("lava"), true));
        storage.filter_on_extract = true;
        assert!(!storage.allows(&k("water"), true));
        assert!(!level_emitter(true, 10, 9, EmitterMode::HighAtOrAbove));
        assert!(level_emitter(true, 10, 10, EmitterMode::HighAtOrAbove));
        assert!(!level_emitter(false, 10, 20, EmitterMode::HighAtOrAbove));
    }
}
