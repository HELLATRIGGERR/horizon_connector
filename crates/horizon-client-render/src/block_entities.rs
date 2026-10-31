//! The world's block entities and the order they tick in: vanilla's
//! `Level.blockEntityTickers`, kept beside the block entity map so a tick
//! visits only the block entities that tick, in vanilla's order, instead of
//! scanning the whole map once per kind.
//!
//! The list follows `LevelChunk.updateBlockEntityTicker` and
//! `Level.tickBlockEntities`:
//! - a block entity whose block state has a ticker gets a slot at the end of
//!   the list (through `pendingBlockEntityTickers`, so a slot made during a
//!   tick first ticks on the next one);
//! - replacing the block entity at a position (`setBlockEntity`), or changing
//!   its block state to another state with a ticker, keeps its slot
//!   (`RebindableTickingBlockEntityWrapper.rebind`);
//! - removing it (`removeBlockEntity`), unloading its chunk
//!   (`clearAllBlockEntities`) or changing its state to one without a ticker
//!   (a jukebox losing its record) frees the slot; a later ticker there is
//!   appended again;
//! - a loaded chunk's block entities are added in the iteration order of its
//!   `Object2ObjectOpenHashMap<BlockPos, BlockEntity>`
//!   (`registerAllBlockEntitiesAfterLevelLoad`), filled in the order the chunk
//!   listed them.
//!
//! Which states tick is a table by state id, built by the game from the block
//! registry; whether a slot is made is decided when the slot is promoted at
//! the start of the next tick, from the block state then.
use crate::chunk::block_entity::{BlockEntity, ModContainer};
use crate::fast_hash::{FxHashMap, FxHashSet};
use std::collections::HashMap;
use std::sync::Arc;

pub type Pos = (i32, i32, i32);

#[cfg(feature = "ae2-lab")]
fn vanilla_metadata_entity(entity:&BlockEntity)->bool {
    matches!(entity,BlockEntity::Chest(_) | BlockEntity::Furnace(_)
        | BlockEntity::Brewing(_) | BlockEntity::Crafter(_) | BlockEntity::Beacon(_))
}

#[cfg(feature = "ae2-lab")]
fn ae2_cpu_kind(kind: crate::ae2_world::MachineKind) -> bool {
    use crate::ae2_world::MachineKind::*;
    matches!(kind, CraftingUnit | CraftingAccelerator | CraftingMonitor | CraftingStorage1k
        | CraftingStorage4k | CraftingStorage16k | CraftingStorage64k | CraftingStorage256k)
}

/// Exact host mutation stream for the AE2 grid event consumer. This journal is
/// opt-in until the consumer is installed; overflow is explicit, never silent.
#[cfg(feature = "ae2-lab")]
#[derive(Clone, Debug)]
pub struct Ae2Mutation {
    pub sequence: u64,
    /// Monotonic server tick when the physical mutation occurred.
    pub server_tick: u64,
    pub position: Pos,
    pub kind: Ae2MutationKind,
}
/// The provider data visible at a committed update, retained independently
/// of later edits. Missing and malformed priority tags remain distinguishable.
#[cfg(feature = "ae2-lab")]
#[derive(Clone, Debug, PartialEq)]
pub enum Ae2ProviderSnapshot {
    /// Current typed block slots encoded without interpreting their contents.
    /// The generic world loader may already have normalized malformed input;
    /// that upstream boundary requires a separate losslessness audit.
    Block { slot_count: usize, raw_slots: horizon_world::nbt::Tag, raw_priority: Option<horizon_world::nbt::Tag> },
    /// Preserve the entire part machine tag, including absent or malformed
    /// inventory/configuration, rather than decoding it at capture time.
    Part { raw_machine: Option<horizon_world::nbt::Tag> },
}
/// AE2's priority setter remounts its existing decoded patterns; a pattern
/// inventory callback rebuilds them first. These are not interchangeable.
#[cfg(feature = "ae2-lab")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ae2ProviderRefreshCause { PrioritySetter, PatternsChanged }
/// Origin of a block provider's first inventory snapshot. The original saved
/// tag is needed because generic item decoding can discard malformed entries.
#[cfg(feature = "ae2-lab")]
#[derive(Clone, Debug)]
pub enum Ae2ProviderInitialInventory {
    Constructed,
    Saved(Option<horizon_world::nbt::Tag>),
    Unavailable,
}
/// The source CPU core flag at construction, before formation mutates it.
#[cfg(feature = "ae2-lab")]
#[derive(Clone, Debug)]
pub enum Ae2CpuInitialCore {
    Constructed,
    Saved(Option<horizon_world::nbt::Tag>),
    Unavailable,
}
#[cfg(feature = "ae2-lab")]
#[derive(Clone, Debug)]
pub enum Ae2MutationKind {
    State { old: u32, new: u32 },
    /// Resident block state supplied by the incoming chunk before the BE is registered.
    ChunkState { state: u32 },
    EntityAdded { incarnation: u64, entity: BlockEntity, provider_inventory: Ae2ProviderInitialInventory, cpu_core: Ae2CpuInitialCore },
    /// Bulk removals retain the host's observed callback sequence; this does
    /// not claim that a different engine iterates its maps in the same order.
    EntityRemoved { incarnation: u64, entity: BlockEntity, bulk_unload: bool },
    PartAdded { incarnation: u64, part: horizon_world::nbt::Tag },
    PartRemoved { incarnation: u64, part: horizon_world::nbt::Tag },
    PartChanged { incarnation: u64, before: horizon_world::nbt::Tag, after: horizon_world::nbt::Tag },
    CenterChanged { incarnation: u64, before: Option<horizon_world::nbt::Tag>, after: Option<horizon_world::nbt::Tag> },
    ForcedMode { node: ae2_network_core::NodeIdentity, mode: Option<ae2_network_core::ChannelMode> },
    MeLink { a: ae2_network_core::NodeIdentity, b: ae2_network_core::NodeIdentity, added: bool },
    QuantumFaces { incarnation: u64, faces: u8 },
    QuantumLink { a: ae2_network_core::NodeIdentity, b: ae2_network_core::NodeIdentity, added: bool },
    /// Actual native `neighborChanged` delivery to a resident CPU entity.
    CpuNeighbor { incarnation: u64, from: Option<Pos> },
    ProviderRefresh { node: ae2_network_core::NodeIdentity, face: Option<u8>, cause: Ae2ProviderRefreshCause, snapshot: Ae2ProviderSnapshot },
    /// A Level Emitter's committed face state at `configureWatchers`, which
    /// synchronously refreshes its crafting-provider registration.
    EmitterProviderRefresh { node: ae2_network_core::NodeIdentity, face: u8, snapshot: horizon_world::nbt::Tag },
    /// In-place NBT read updates physical data and current priority without
    /// invoking the provider's pattern or priority refresh callbacks.
    ProviderRawRead { node: ae2_network_core::NodeIdentity, snapshot: Ae2ProviderSnapshot },
}
#[cfg(feature = "ae2-lab")]
#[derive(Clone, Default)]
struct Ae2Journal { entries: Vec<Ae2Mutation>, overflowed: bool, clock: Option<std::sync::Arc<std::sync::atomic::AtomicU64>> }

/// What a block state's block entity does each tick (`BlockState.getTicker`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ticking {
    /// No ticker, and the block has no block entity that could have one.
    #[default]
    No,
    /// A ticker.
    Yes,
    /// The block's block entity, but this state has no ticker (a jukebox
    /// without a record): a state change to it frees the slot.
    Idle,
}

#[derive(Default)]
pub struct BlockEntities {
    map: HashMap<Pos, BlockEntity>,
    /// Process-local identity of each physical block-entity incarnation.
    incarnations: FxHashMap<Pos, u64>,
    /// Raw vanilla components retained only for the incarnation that received
    /// them. Position-keyed chunk tags can otherwise belong to a removed BE.
    #[cfg(feature = "ae2-lab")]
    vanilla_metadata: FxHashMap<Pos,(u64,horizon_world::nbt::Tag)>,
    #[cfg(feature = "ae2-lab")]
    vanilla_loaded_metadata: FxHashMap<Pos,horizon_world::nbt::Tag>,
    #[cfg(feature = "ae2-lab")]
    ae2_journal: std::sync::Mutex<Ae2Journal>,
    #[cfg(feature = "ae2-lab")]
    ae2_journal_active: std::sync::atomic::AtomicBool,
    #[cfg(feature = "ae2-lab")]
    ae2_taken: FxHashMap<Pos, (u64, BlockEntity)>,
    #[cfg(feature = "ae2-lab")]
    ae2_loaded_provider_inventory: FxHashMap<Pos, Ae2ProviderInitialInventory>,
    #[cfg(feature = "ae2-lab")]
    ae2_loaded_cpu_core: FxHashMap<Pos, Ae2CpuInitialCore>,
    /// `Level.blockEntityTickers`: positions and the slot each was given.
    tickers: Arc<Vec<(Pos, u32)>>,
    /// `LevelChunk.tickersInLevel`: the live slot of each ticking position.
    live: FxHashMap<Pos, u32>,
    /// Positions that may need a slot, in the order vanilla would have made
    /// it (`pendingBlockEntityTickers`), each with a token; decided at the
    /// next promotion from the block state then.
    added: Vec<(Pos, u32)>,
    /// The token of each position's pending entry: an entry whose token is
    /// not here any more was cancelled.
    pending: FxHashMap<Pos, u32>,
    next_slot: u32,
    /// Whether a slot was freed since the list was last swept: otherwise
    /// every slot in it is still live, unless its block entity is `out`.
    freed: bool,
    /// Positions whose block entity `take` took and `put` has not brought
    /// back.
    out: FxHashSet<Pos>,
    /// Positions a loaded chunk gave a new block entity over an old one: at
    /// the next tick their slot stays only if their block state ticks
    /// (`updateBlockEntityTicker`: rebind, or remove without a ticker).
    replaced: FxHashSet<Pos>,
    /// `Ticking` by block state id; empty until the game builds it.
    states: Vec<Ticking>,
    /// The containers of mod blocks' block entities, by block state id;
    /// built with `states`.
    mod_containers: FxHashMap<u32, Arc<ModContainer>>,
}

impl std::ops::Deref for BlockEntities {
    type Target = HashMap<Pos, BlockEntity>;
    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl<'a> IntoIterator for &'a BlockEntities {
    type Item = (&'a Pos, &'a BlockEntity);
    type IntoIter = std::collections::hash_map::Iter<'a, Pos, BlockEntity>;
    fn into_iter(self) -> Self::IntoIter {
        self.map.iter()
    }
}

/// `Vec3i.hashCode`.
fn java_hash((x, y, z): Pos) -> i32 {
    y.wrapping_add(z.wrapping_mul(31))
        .wrapping_mul(31)
        .wrapping_add(x)
}
/// `HashCommon.mix`.
fn mix(x: i32) -> usize {
    let h = (x as u32).wrapping_mul(0x9E37_79B9);
    (h ^ (h >> 16)) as usize
}

/// The order `values().forEach` visits a `new Object2ObjectOpenHashMap<>()`
/// keyed by these positions, put in this order (repeats keep their first
/// slot): open addressing with linear probing, a rehash to twice the size
/// past three quarters full, and iteration from the last slot down.
pub fn chunk_map_order(positions: impl IntoIterator<Item = Pos>) -> Vec<Pos> {
    // HashCommon.arraySize(16, .75f), and maxFill of each size.
    let mut slots: Vec<Option<Pos>> = vec![None; 32];
    let max_fill = |n: usize| (n * 3).div_ceil(4).min(n - 1);
    let mut size = 0;
    for p in positions {
        let mask = slots.len() - 1;
        let mut at = mix(java_hash(p)) & mask;
        let mut known = false;
        while let Some(k) = slots[at] {
            if k == p {
                known = true;
                break;
            }
            at = (at + 1) & mask;
        }
        if known {
            continue;
        }
        slots[at] = Some(p);
        // `if (size++ >= maxFill) rehash(arraySize(size + 1, f))`, which
        // puts the keys back from the last slot down.
        size += 1;
        if size > max_fill(slots.len()) {
            let n = ((size + 1) * 4).div_ceil(3).next_power_of_two();
            let old = std::mem::replace(&mut slots, vec![None; n]);
            for k in old.into_iter().rev().flatten() {
                let mut at = mix(java_hash(k)) & (n - 1);
                while slots[at].is_some() {
                    at = (at + 1) & (n - 1);
                }
                slots[at] = Some(k);
            }
        }
    }
    slots.into_iter().rev().flatten().collect()
}

impl BlockEntities {
    pub fn placement_snapshot(&self) -> Self {
        Self {
            map: self.map.clone(),
            incarnations: self.incarnations.clone(),
            tickers: self.tickers.clone(),
            live: self.live.clone(),
            added: self.added.clone(),
            pending: self.pending.clone(),
            out: self.out.clone(),
            states: self.states.clone(),
            next_slot: self.next_slot, freed: self.freed,
            #[cfg(feature="ae2-lab")]
            vanilla_metadata: self.vanilla_metadata.clone(),
            #[cfg(feature="ae2-lab")]
            vanilla_loaded_metadata: self.vanilla_loaded_metadata.clone(),
            #[cfg(feature="ae2-lab")]
            ae2_taken: self.ae2_taken.clone(),
            #[cfg(feature="ae2-lab")]
            ae2_loaded_provider_inventory: self.ae2_loaded_provider_inventory.clone(),
            #[cfg(feature="ae2-lab")]
            ae2_loaded_cpu_core: self.ae2_loaded_cpu_core.clone(),
            #[cfg(feature="ae2-lab")]
            ae2_journal: std::sync::Mutex::new(self.ae2_journal.lock().expect("AE2 journal poisoned").clone()),
            #[cfg(feature="ae2-lab")]
            ae2_journal_active: std::sync::atomic::AtomicBool::new(self.ae2_journal_active.load(std::sync::atomic::Ordering::Relaxed)),
        }
    }

    #[cfg(feature = "ae2-lab")]
    fn record_ae2(&self, position: Pos, kind: Ae2MutationKind) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        if !self.ae2_journal_enabled() { return; }
        let mut journal = self.ae2_journal.lock().unwrap_or_else(|error| error.into_inner());
        const LIMIT: usize = 100_000;
        if journal.entries.len() == LIMIT { journal.overflowed = true; return; }
        let server_tick = journal.clock.as_ref().map_or(0, |clock| clock.load(Ordering::Relaxed));
        journal.entries.push(Ae2Mutation { sequence: NEXT.fetch_add(1, Ordering::Relaxed), server_tick, position, kind });
    }
    #[cfg(feature = "ae2-lab")]
    pub fn set_ae2_journal_clock(&self, clock: std::sync::Arc<std::sync::atomic::AtomicU64>) {
        self.ae2_journal.lock().unwrap_or_else(|error| error.into_inner()).clock = Some(clock);
    }
    #[cfg(feature = "ae2-lab")]
    pub fn start_ae2_journal(&self) {
        use std::sync::atomic::Ordering;
        let mut journal = self.ae2_journal.lock().unwrap_or_else(|error| error.into_inner());
        if self.ae2_journal_active.load(Ordering::Acquire) { return; }
        *journal = Ae2Journal::default();
        self.ae2_journal_active.store(true, Ordering::Release);
    }
    #[cfg(feature = "ae2-lab")]
    pub fn take_ae2_mutations(&self) -> Result<Vec<Ae2Mutation>, &'static str> {
        if !self.ae2_journal_enabled() { return Err("AE2 mutation journal is disabled"); }
        let mut journal = self.ae2_journal.lock().unwrap_or_else(|error| error.into_inner());
        if journal.overflowed { return Err("AE2 mutation journal overflow"); }
        Ok(std::mem::take(&mut journal.entries))
    }
    #[cfg(feature = "ae2-lab")]
    pub fn record_ae2_part(&mut self, pos: Pos, part: horizon_world::nbt::Tag, added: bool) {
        let Some(incarnation) = self.incarnation(&pos) else { return };
        let kind = if added { Ae2MutationKind::PartAdded { incarnation, part } }
            else { Ae2MutationKind::PartRemoved { incarnation, part } };
        self.record_ae2(pos, kind);
    }
    #[cfg(feature = "ae2-lab")]
    pub fn record_ae2_me_link(&self, pos: Pos, a: ae2_network_core::NodeIdentity, b: ae2_network_core::NodeIdentity, added: bool) {
        self.record_ae2(pos, Ae2MutationKind::MeLink { a, b, added });
    }
    #[cfg(feature = "ae2-lab")]
    pub fn record_ae2_quantum_faces(&self, pos: Pos, incarnation: u64, faces: u8) {
        self.record_ae2(pos, Ae2MutationKind::QuantumFaces { incarnation, faces });
    }
    #[cfg(feature = "ae2-lab")]
    pub fn record_ae2_quantum_link(&self, pos: Pos, a: ae2_network_core::NodeIdentity, b: ae2_network_core::NodeIdentity, added: bool) {
        self.record_ae2(pos, Ae2MutationKind::QuantumLink { a, b, added });
    }
    #[cfg(feature = "ae2-lab")]
    pub fn record_ae2_cpu_neighbor(&self, pos: Pos, from: Option<Pos>) {
        let Some(incarnation) = self.incarnation(&pos) else { return; };
        if !matches!(self.get(&pos), Some(BlockEntity::Ae2(machine)) if ae2_cpu_kind(machine.kind)) { return; }
        self.record_ae2(pos, Ae2MutationKind::CpuNeighbor { incarnation, from });
    }
    /// Record only after the owning provider transaction has committed. The
    /// callback never rebinds a stale block/part incarnation at this address.
    #[cfg(feature = "ae2-lab")]
    pub fn record_ae2_provider_refresh(&self, pos: Pos, node: ae2_network_core::NodeIdentity, face: Option<u8>, cause: Ae2ProviderRefreshCause, snapshot: Ae2ProviderSnapshot) -> Result<(), &'static str> {
        if !self.ae2_journal_enabled() { return Err("AE2 mutation journal is disabled"); }
        if self.incarnation(&pos) != Some(node.host) || node.outer
            || node.part.is_some() != face.is_some() || face.is_some_and(|face| face > 5)
            || !matches!((&snapshot, face), (Ae2ProviderSnapshot::Block {..}, None) | (Ae2ProviderSnapshot::Part {..}, Some(_)))
        { return Err("provider refresh identity mismatch"); }
        self.record_ae2(pos, Ae2MutationKind::ProviderRefresh { node, face, cause, snapshot });
        Ok(())
    }
    /// Preserve the exact committed emitter face, tied to the same host and
    /// part incarnations that received the source callback.
    #[cfg(feature = "ae2-lab")]
    pub fn record_ae2_emitter_provider_refresh(&self, pos: Pos, node: ae2_network_core::NodeIdentity, face: u8, snapshot: horizon_world::nbt::Tag) -> Result<(), &'static str> {
        if !self.ae2_journal_enabled() { return Err("AE2 mutation journal is disabled"); }
        if self.incarnation(&pos) != Some(node.host) || node.outer || node.part.is_none() || face > 5 {
            return Err("emitter provider refresh identity mismatch");
        }
        self.record_ae2(pos, Ae2MutationKind::EmitterProviderRefresh { node, face, snapshot });
        Ok(())
    }
    #[cfg(feature = "ae2-lab")]
    pub fn record_ae2_provider_raw_read(&self, pos: Pos, node: ae2_network_core::NodeIdentity, snapshot: Ae2ProviderSnapshot) -> Result<(), &'static str> {
        if !self.ae2_journal_enabled() { return Err("AE2 mutation journal is disabled"); }
        if self.incarnation(&pos) != Some(node.host) || node.part.is_some() || node.outer
            || !matches!(&snapshot, Ae2ProviderSnapshot::Block { .. }) {
            return Err("provider raw-read identity mismatch");
        }
        self.record_ae2(pos, Ae2MutationKind::ProviderRawRead { node, snapshot });
        Ok(())
    }
    /// Supply an incoming chunk's state before `load_chunk` consumes its AE2
    /// block entities. This is initialization, not an invented block write.
    #[cfg(feature = "ae2-lab")]
    pub fn record_ae2_loaded_state(&mut self, pos: Pos, state: u32) {
        self.record_ae2(pos, Ae2MutationKind::ChunkState { state });
    }
    #[cfg(feature = "ae2-lab")]
    pub fn record_ae2_chunk_states(&mut self, chunk: &crate::chunk::Chunk) {
        if !self.ae2_journal_enabled() { return; }
        self.ae2_loaded_provider_inventory.clear();
        self.ae2_loaded_cpu_core.clear();
        for (pos, entity) in &chunk.block_entities {
            if matches!(entity, BlockEntity::Ae2(_)) {
                self.record_ae2_loaded_state(*pos, chunk.block(pos.0 & 15, pos.1, pos.2 & 15).0);
            }
            if matches!(entity, BlockEntity::Ae2(machine) if machine.kind == crate::ae2_world::MachineKind::PatternProvider) {
                let initial = match &chunk.saved {
                    Some(saved) => {
                        let mut sources = saved.block_entity_sources.iter().filter(|(source_pos, _)| source_pos == pos);
                        match (sources.next(), sources.next()) {
                            (Some((_, raw)), None) => Ae2ProviderInitialInventory::Saved(raw.get("Items").cloned()),
                            _ => Ae2ProviderInitialInventory::Unavailable,
                        }
                    }
                    None if chunk.fresh => Ae2ProviderInitialInventory::Constructed,
                    None => Ae2ProviderInitialInventory::Unavailable,
                };
                self.ae2_loaded_provider_inventory.insert(*pos, initial);
            }
            if matches!(entity, BlockEntity::Ae2(machine) if ae2_cpu_kind(machine.kind)) {
                let initial = match &chunk.saved {
                    Some(saved) => {
                        let mut sources = saved.block_entity_sources.iter().filter(|(source_pos, _)| source_pos == pos);
                        match (sources.next(), sources.next()) {
                            (Some((_, raw)), None) => Ae2CpuInitialCore::Saved(raw.get("core").cloned()),
                            _ => Ae2CpuInitialCore::Unavailable,
                        }
                    }
                    None if chunk.fresh => Ae2CpuInitialCore::Constructed,
                    None => Ae2CpuInitialCore::Unavailable,
                };
                self.ae2_loaded_cpu_core.insert(*pos, initial);
            }
        }
    }
    #[cfg(feature = "ae2-lab")]
    pub fn ae2_journal_enabled(&self) -> bool { self.ae2_journal_active.load(std::sync::atomic::Ordering::Acquire) }
    #[cfg(feature = "ae2-lab")]
    pub fn record_ae2_part_change(&mut self, pos: Pos, before: horizon_world::nbt::Tag, after: horizon_world::nbt::Tag) {
        if before == after { return; }
        let Some(incarnation) = self.incarnation(&pos) else { return };
        self.record_ae2(pos, Ae2MutationKind::PartChanged { incarnation, before, after });
    }
    #[cfg(feature = "ae2-lab")]
    pub fn record_ae2_center_change(&mut self, pos: Pos, before: Option<horizon_world::nbt::Tag>, after: Option<horizon_world::nbt::Tag>) {
        if before == after { return; }
        let Some(incarnation) = self.incarnation(&pos) else { return };
        self.record_ae2(pos, Ae2MutationKind::CenterChanged { incarnation, before, after });
    }
    #[cfg(feature = "ae2-lab")]
    pub fn record_ae2_forced_mode(&mut self, pos: Pos, node: ae2_network_core::NodeIdentity, mode: Option<ae2_network_core::ChannelMode>) {
        self.record_ae2(pos, Ae2MutationKind::ForcedMode { node, mode });
    }
    fn new_incarnation(&mut self, pos: Pos) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let token = NEXT.fetch_add(1, Ordering::Relaxed);
        assert_ne!(token, 0, "block entity incarnation counter exhausted");
        #[cfg(feature = "ae2-lab")]
        self.vanilla_metadata.remove(&pos);
        self.incarnations.insert(pos, token);
    }
    /// Unique during this process. An entity temporarily taken for its tick
    /// keeps its identity; replacement, unload and later load create a new one.
    pub fn incarnation(&self, pos: &Pos) -> Option<u64> {
        self.incarnations.get(pos).copied()
    }
    /// Bind decoded raw data to the actual live container, never its address
    /// alone. Temporary `take`/`put` preserves its incarnation.
    #[cfg(feature = "ae2-lab")]
    pub fn remember_vanilla_metadata(&mut self,pos:Pos,raw:&horizon_world::nbt::Tag) {
        if !self.map.get(&pos).is_some_and(vanilla_metadata_entity) {return}
        if let Some(incarnation)=self.incarnation(&pos) {
            self.vanilla_metadata.insert(pos,(incarnation,raw.clone()));
        }
    }
    #[cfg(feature = "ae2-lab")]
    pub fn supports_vanilla_metadata(&self,pos:&Pos)->bool {
        self.map.get(pos).is_some_and(vanilla_metadata_entity)
    }
    #[cfg(feature = "ae2-lab")]
    pub fn vanilla_metadata(&self,pos:&Pos)->Option<&horizon_world::nbt::Tag> {
        let (owner,raw)=self.vanilla_metadata.get(pos)?;
        (self.incarnation(pos)==Some(*owner)).then_some(raw)
    }
    /// Stage only metadata for BEs that this specific incoming chunk decoded.
    /// Duplicate source records are ignored instead of guessing an owner.
    #[cfg(feature = "ae2-lab")]
    pub fn prepare_vanilla_metadata(&mut self,chunk:&crate::chunk::Chunk) {
        self.vanilla_loaded_metadata.clear();
        let Some(saved)=chunk.saved.as_deref() else{return};
        let wanted:FxHashSet<_>=chunk.block_entities.iter().filter(|(_,entity)|vanilla_metadata_entity(entity)).map(|(pos,_)|*pos).collect();
        let mut duplicate=FxHashSet::default();
        for (pos,tag) in &saved.block_entity_sources {
            if !wanted.contains(pos) || duplicate.contains(pos) {continue}
            if self.vanilla_loaded_metadata.insert(*pos,tag.clone()).is_some() {
                self.vanilla_loaded_metadata.remove(pos);
                duplicate.insert(*pos);
            }
        }
    }
    /// Replace position-only originals with the data attached to the current
    /// incarnation before serializing the authoritative chunk.
    #[cfg(feature = "ae2-lab")]
    pub fn project_vanilla_metadata(&self,chunk:&mut crate::chunk::Chunk) {
        use std::sync::Arc;
        let positions:FxHashSet<_>=chunk.block_entities.iter().filter(|(_,entity)|vanilla_metadata_entity(entity)).map(|(pos,_)|*pos).collect();
        if positions.is_empty(){return}
        let owned:Vec<_>=positions.iter().filter_map(|pos|self.vanilla_metadata(pos).map(|tag|(*pos,tag.clone()))).collect();
        if chunk.saved.is_none() && owned.is_empty(){return}
        let saved=Arc::make_mut(chunk.saved.get_or_insert_with(||Arc::new(crate::save::ChunkExtras::default())));
        saved.block_entity_sources.retain(|(pos,_)|!positions.contains(pos));
        saved.block_entity_sources.extend(owned);
        saved.rolled_loot.retain(|roll|!positions.contains(&roll.pos) || self.vanilla_metadata(&roll.pos)
            .and_then(|tag|crate::save::text(tag.get("LootTable"))).as_deref()==Some(roll.table.as_str()));
    }
    /// Whether the ticking table has been built.
    pub fn has_states(&self) -> bool {
        !self.states.is_empty()
    }
    pub fn set_states(&mut self, states: Vec<Ticking>) {
        self.states = states;
    }
    pub fn ticking(&self, state: u32) -> Ticking {
        self.states.get(state as usize).copied().unwrap_or_default()
    }
    pub fn set_mod_containers(&mut self, containers: FxHashMap<u32, Arc<ModContainer>>) {
        self.mod_containers = containers;
    }
    /// The container of a mod block's block entity in block state `state`.
    pub fn mod_container(&self, state: u32) -> Option<&Arc<ModContainer>> {
        self.mod_containers.get(&state)
    }

    fn next(&mut self) -> u32 {
        let n = self.next_slot;
        self.next_slot = n.wrapping_add(1);
        n
    }
    /// A ticker may be made for `pos` now (`updateBlockEntityTicker`).
    fn add(&mut self, pos: Pos) {
        let token = self.next();
        self.pending.insert(pos, token);
        self.added.push((pos, token));
    }
    /// `removeBlockEntityTicker`: no slot, made or pending.
    fn cancel(&mut self, pos: &Pos) {
        self.freed |= self.live.remove(pos).is_some();
        self.pending.remove(pos);
    }

    pub fn get_mut(&mut self, pos: &Pos) -> Option<&mut BlockEntity> {
        self.map.get_mut(pos)
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&Pos, &mut BlockEntity)> {
        self.map.iter_mut()
    }
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut BlockEntity> {
        self.map.values_mut()
    }
    /// `LevelChunk.setBlockEntity` (+ `addAndRegisterBlockEntity`): a new
    /// block entity may get a slot; one replacing another keeps its slot.
    pub fn insert(&mut self, pos: Pos, entity: BlockEntity) -> Option<BlockEntity> {
        #[cfg(feature = "ae2-lab")]
        let previous_incarnation = self.incarnation(&pos);
        let old = self.map.insert(pos, entity);
        self.new_incarnation(pos);
        #[cfg(feature = "ae2-lab")]
        if self.ae2_journal_enabled() {
            if let (Some(incarnation), Some(entity @ BlockEntity::Ae2(_))) = (previous_incarnation, old.as_ref()) {
                self.record_ae2(pos, Ae2MutationKind::EntityRemoved { incarnation, entity: entity.clone(), bulk_unload: false });
            }
            if let Some(entity @ BlockEntity::Ae2(_)) = self.map.get(&pos).cloned() {
                let incarnation = self.incarnation(&pos).expect("inserted incarnation");
                self.record_ae2(pos, Ae2MutationKind::EntityAdded { incarnation, entity, provider_inventory: Ae2ProviderInitialInventory::Constructed, cpu_core: Ae2CpuInitialCore::Constructed });
            }
        }
        if old.is_none() {
            self.add(pos);
        }
        old
    }
    /// Import an item into the just-constructed block entity without replacing
    /// the live object or its ticking slot/physical incarnation. The decoded
    /// data must describe the same block-entity type that was constructed.
    pub fn import_placed_in_place(&mut self, pos: Pos, entity: BlockEntity) -> Result<(), &'static str> {
        let current=self.map.get_mut(&pos).ok_or("placed block entity disappeared")?;
        if std::mem::discriminant(&*current)!=std::mem::discriminant(&entity) {
            return Err("placed block entity type changed");
        }
        #[cfg(feature = "ae2-lab")]
        if let (BlockEntity::Ae2(old),BlockEntity::Ae2(new))=(&*current,&entity) {
            if old.kind!=new.kind {return Err("placed AE2 machine type changed");}
        }
        *current=entity;
        Ok(())
    }
    /// Update the still-pending construction record after item data has been
    /// loaded into that same object. Source `onReady` has not fired yet; its
    /// observers must see the imported provider/CPU data and physical energy.
    /// None means this is not an AE2 entity or journaling is disabled. A false
    /// result means the Added record has already escaped or overflowed and the
    /// caller must not continue relying on the ordered observer as authoritative.
    #[cfg(feature = "ae2-lab")]
    pub fn refresh_pending_placed_ae2(&self, pos: Pos, raw: Option<&horizon_world::nbt::Tag>) -> Option<bool> {
        if !self.ae2_journal_enabled() {return None;}
        let BlockEntity::Ae2(machine)=self.map.get(&pos)? else {return None};
        let incarnation=match self.incarnation(&pos) {Some(id)=>id,None=>return Some(false)};
        let kind=machine.kind;
        let entity=BlockEntity::Ae2(machine.clone());
        let mut journal=self.ae2_journal.lock().unwrap_or_else(|error|error.into_inner());
        if journal.overflowed {return Some(false);}
        let Some(event)=journal.entries.iter_mut().rev().find(|event|event.position==pos && matches!(
            &event.kind,Ae2MutationKind::EntityAdded{incarnation:id,..} if *id==incarnation)) else {return Some(false)};
        let Ae2MutationKind::EntityAdded {entity: initial, provider_inventory, cpu_core, ..}=&mut event.kind else {unreachable!()};
        *initial=entity;
        if let Some(raw)=raw {
            if kind==crate::ae2_world::MachineKind::PatternProvider {
                *provider_inventory=Ae2ProviderInitialInventory::Saved(raw.get("Items").cloned());
            }
            if ae2_cpu_kind(kind) { *cpu_core=Ae2CpuInitialCore::Saved(raw.get("core").cloned()); }
        }
        Some(true)
    }
    /// A command's decoded replacement carries loaded inventory/core state,
    /// unlike a freshly constructed entity. Keep the replacement's remove/add
    /// callbacks and original raw snapshot in the same journal transaction.
    #[cfg(feature = "ae2-lab")]
    pub fn replace_from_data(&mut self, pos: Pos, entity: BlockEntity, raw: &horizon_world::nbt::Tag) -> Option<BlockEntity> {
        let previous_incarnation = self.incarnation(&pos);
        let old = self.map.insert(pos, entity);
        self.new_incarnation(pos);
        if self.ae2_journal_enabled() {
            if let (Some(incarnation), Some(entity @ BlockEntity::Ae2(_))) = (previous_incarnation, old.as_ref()) {
                self.record_ae2(pos, Ae2MutationKind::EntityRemoved { incarnation, entity: entity.clone(), bulk_unload: false });
            }
            if let Some(entity @ BlockEntity::Ae2(_)) = self.map.get(&pos).cloned() {
                let incarnation = self.incarnation(&pos).expect("loaded incarnation");
                let provider_inventory = if matches!(&entity, BlockEntity::Ae2(m) if m.kind == crate::ae2_world::MachineKind::PatternProvider) {
                    Ae2ProviderInitialInventory::Saved(raw.get("Items").cloned())
                } else { Ae2ProviderInitialInventory::Constructed };
                let cpu_core = if matches!(&entity, BlockEntity::Ae2(m) if ae2_cpu_kind(m.kind)) {
                    Ae2CpuInitialCore::Saved(raw.get("core").cloned())
                } else { Ae2CpuInitialCore::Constructed };
                self.record_ae2(pos, Ae2MutationKind::EntityAdded { incarnation, entity, provider_inventory, cpu_core });
            }
        }
        if old.is_none() { self.add(pos); }
        old
    }
    /// The block entity at `pos`, made by `make` when there is none.
    pub fn get_or_insert_with(
        &mut self,
        pos: Pos,
        make: impl FnOnce() -> BlockEntity,
    ) -> &mut BlockEntity {
        if !self.map.contains_key(&pos) { self.insert(pos, make()); }
        self.map.get_mut(&pos).expect("inserted block entity")
    }
    /// `LevelChunk.removeBlockEntity`: the block entity and its slot.
    pub fn remove(&mut self, pos: &Pos) -> Option<BlockEntity> {
        self.cancel(pos);
        #[cfg(feature = "ae2-lab")]
        self.vanilla_metadata.remove(pos);
        let incarnation = self.incarnations.remove(pos);
        let removed = self.map.remove(pos);
        #[cfg(feature = "ae2-lab")]
        if self.ae2_journal_enabled() {
            if let (Some(incarnation), Some(entity @ BlockEntity::Ae2(_))) = (incarnation, removed.as_ref()) {
                self.record_ae2(*pos, Ae2MutationKind::EntityRemoved { incarnation, entity: entity.clone(), bulk_unload: false });
            }
        }
        #[cfg(not(feature = "ae2-lab"))]
        let _ = incarnation;
        removed
    }
    /// The block entity at `pos`, out of the map for a while: its slot stays
    /// for `put` to bring it back (a tick working on it).
    pub fn take(&mut self, pos: &Pos) -> Option<BlockEntity> {
        let taken = self.map.remove(pos);
        if taken.is_some() {
            self.out.insert(*pos);
            #[cfg(feature = "ae2-lab")]
            if self.ae2_journal_enabled() {
                if let (Some(incarnation), Some(entity @ BlockEntity::Ae2(_))) = (self.incarnation(pos), taken.as_ref()) {
                    self.ae2_taken.insert(*pos, (incarnation, entity.clone()));
                }
            }
        }
        taken
    }
    /// Brings back what `take` took, in its slot.
    pub fn put(&mut self, pos: Pos, entity: BlockEntity) {
        let was_out = self.out.remove(&pos);
        #[cfg(feature = "ae2-lab")]
        self.ae2_taken.remove(&pos);
        if !was_out || !self.incarnations.contains_key(&pos) {
            self.new_incarnation(pos);
            #[cfg(feature = "ae2-lab")]
            if self.ae2_journal_enabled() && matches!(entity, BlockEntity::Ae2(_)) {
                let incarnation = self.incarnation(&pos).expect("put incarnation");
                self.record_ae2(pos, Ae2MutationKind::EntityAdded { incarnation, entity: entity.clone(), provider_inventory: Ae2ProviderInitialInventory::Constructed, cpu_core: Ae2CpuInitialCore::Constructed });
            }
        }
        if self.map.insert(pos, entity).is_none()
            && !self.live.contains_key(&pos)
            && !self.pending.contains_key(&pos)
        {
            self.add(pos);
        }
    }
    /// Removes the block entities `keep` refuses, and their slots
    /// (`clearAllBlockEntities` of unloaded chunks).
    pub fn retain(&mut self, mut keep: impl FnMut(&Pos, &mut BlockEntity) -> bool) {
        #[cfg(feature = "ae2-lab")]
        let mut removed = Vec::new();
        #[cfg(feature = "ae2-lab")]
        let journal_enabled = self.ae2_journal_enabled();
        let (live, pending, freed, incarnations) = (&mut self.live, &mut self.pending, &mut self.freed, &mut self.incarnations);
        #[cfg(feature = "ae2-lab")]
        let metadata=&mut self.vanilla_metadata;
        self.map.retain(|p, e| {
            let kept = keep(p, e);
            if !kept {
                *freed |= live.remove(p).is_some();
                pending.remove(p);
                #[cfg(feature = "ae2-lab")]
                metadata.remove(p);
                let incarnation = incarnations.remove(p);
                #[cfg(feature = "ae2-lab")]
                if journal_enabled {
                    if let (Some(incarnation), entity @ BlockEntity::Ae2(_)) = (incarnation, e) {
                        removed.push((*p, incarnation, entity.clone()));
                    }
                }
                #[cfg(not(feature = "ae2-lab"))]
                let _ = incarnation;
            }
            kept
        });
        #[cfg(feature = "ae2-lab")]
        for (pos, incarnation, entity) in removed { self.record_ae2(pos, Ae2MutationKind::EntityRemoved { incarnation, entity, bulk_unload: true }); }
    }
    pub fn clear(&mut self) {
        #[cfg(feature = "ae2-lab")]
        if self.ae2_journal_enabled() {
            let removed: Vec<_> = self.map.iter().filter_map(|(pos, entity)| match (self.incarnation(pos), entity) {
                (Some(incarnation), BlockEntity::Ae2(_)) => Some((*pos, incarnation, entity.clone())),
                _ => None,
            }).collect();
            for (pos, incarnation, entity) in removed { self.record_ae2(pos, Ae2MutationKind::EntityRemoved { incarnation, entity, bulk_unload: true }); }
        }
        self.map.clear();
        self.incarnations.clear();
        #[cfg(feature = "ae2-lab")]
        {self.vanilla_metadata.clear();self.vanilla_loaded_metadata.clear();}
        self.tickers = Arc::default();
        self.live.clear();
        self.added.clear();
        self.pending.clear();
        self.out.clear();
        #[cfg(feature = "ae2-lab")]
        self.ae2_taken.clear();
        #[cfg(feature = "ae2-lab")]
        self.ae2_loaded_provider_inventory.clear();
        #[cfg(feature = "ae2-lab")]
        self.ae2_loaded_cpu_core.clear();
        self.replaced.clear();
        self.freed = false;
    }
    /// A loaded or generated chunk's block entities, in the order the chunk
    /// lists them; they tick in its block entity map's order
    /// (`registerAllBlockEntitiesAfterLevelLoad`). One replacing an entity
    /// already at its position (`LevelChunk.setBlockEntity`) drops that one;
    /// its slot is decided by the block state at the next tick.
    pub fn load_chunk(&mut self, entities: Vec<(Pos, BlockEntity)>) {
        let order = chunk_map_order(entities.iter().map(|(p, _)| *p));
        for (p, e) in entities {
            #[cfg(feature = "ae2-lab")]
            let previous = self.incarnation(&p);
            let old = self.map.insert(p, e);
            if old.is_some() {
                self.replaced.insert(p);
            }
            self.new_incarnation(p);
            #[cfg(feature = "ae2-lab")]
            if let Some(raw)=self.vanilla_loaded_metadata.remove(&p) {
                self.remember_vanilla_metadata(p,&raw);
            }
            #[cfg(feature = "ae2-lab")]
            if self.ae2_journal_enabled() {
                if let (Some(incarnation), Some(entity @ BlockEntity::Ae2(_))) = (previous, old.as_ref()) {
                    self.record_ae2(p, Ae2MutationKind::EntityRemoved { incarnation, entity: entity.clone(), bulk_unload: false });
                }
                if let Some(entity @ BlockEntity::Ae2(_)) = self.map.get(&p).cloned() {
                    let incarnation = self.incarnation(&p).expect("loaded incarnation");
                    let provider_inventory = if matches!(&entity, BlockEntity::Ae2(machine) if machine.kind == crate::ae2_world::MachineKind::PatternProvider) {
                        self.ae2_loaded_provider_inventory.remove(&p).unwrap_or(Ae2ProviderInitialInventory::Unavailable)
                    } else { Ae2ProviderInitialInventory::Constructed };
                    let cpu_core = if matches!(&entity, BlockEntity::Ae2(machine) if ae2_cpu_kind(machine.kind)) {
                        self.ae2_loaded_cpu_core.remove(&p).unwrap_or(Ae2CpuInitialCore::Unavailable)
                    } else { Ae2CpuInitialCore::Constructed };
                    self.record_ae2(p, Ae2MutationKind::EntityAdded { incarnation, entity, provider_inventory, cpu_core });
                }
            }
            #[cfg(not(feature = "ae2-lab"))]
            let _ = old;
        }
        for p in order {
            self.add(p);
        }
        #[cfg(feature = "ae2-lab")]
        self.vanilla_loaded_metadata.clear();
    }
    /// `LevelChunk.setBlockState` on a block entity's position, from state
    /// `old` to `new`: a ticker made, kept or dropped.
    pub fn state_changed(&mut self, pos: Pos, old: u32, new: u32) {
        #[cfg(feature = "ae2-lab")]
        if old != new { self.record_ae2(pos, Ae2MutationKind::State { old, new }); }
        if self.states.is_empty() || !self.map.contains_key(&pos) {
            return;
        }
        match self.ticking(new) {
            // A new ticker, unless one was made or is pending (rebound).
            Ticking::Yes => {
                if self.ticking(old) != Ticking::Yes && !self.live.contains_key(&pos) {
                    self.add(pos);
                }
            }
            Ticking::Idle => self.cancel(&pos),
            // Another block: the game removes or replaces the block entity.
            Ticking::No => {}
        }
    }
    /// The start of `Level.tickBlockEntities`: pending slots join the list
    /// and freed ones leave it. `state` reads the block state at a position.
    /// Returns the slots to tick, in order; check each with `is_live` before
    /// ticking it, as the ticks before it can free it.
    pub fn tickers(&mut self, state: impl Fn(Pos) -> u32) -> Arc<Vec<(Pos, u32)>> {
        for (pos, token) in std::mem::take(&mut self.added) {
            if self.pending.get(&pos) != Some(&token) {
                continue;
            }
            self.pending.remove(&pos);
            if self.live.contains_key(&pos)
                || !self.map.contains_key(&pos)
                || self.ticking(state(pos)) != Ticking::Yes
            {
                continue;
            }
            let slot = self.next();
            self.live.insert(pos, slot);
            Arc::make_mut(&mut self.tickers).push((pos, slot));
        }
        for pos in std::mem::take(&mut self.replaced) {
            if self.live.contains_key(&pos) && self.ticking(state(pos)) != Ticking::Yes {
                self.cancel(&pos);
            }
        }
        if !std::mem::take(&mut self.freed) && self.out.is_empty() {
            return Arc::clone(&self.tickers);
        }
        let out: Vec<_> = self.out.drain().collect();
        for pos in out {
            if !self.map.contains_key(&pos) {
                self.incarnations.remove(&pos);
                #[cfg(feature = "ae2-lab")]
                self.vanilla_metadata.remove(&pos);
                #[cfg(feature = "ae2-lab")]
                if let Some((incarnation, entity)) = self.ae2_taken.remove(&pos) {
                    self.record_ae2(pos, Ae2MutationKind::EntityRemoved { incarnation, entity, bulk_unload: false });
                }
            }
        }
        let (live, map) = (&mut self.live, &self.map);
        Arc::make_mut(&mut self.tickers).retain(|(p, slot)| {
            let mine = live.get(p) == Some(slot);
            if mine && !map.contains_key(p) {
                live.remove(p);
            }
            mine && map.contains_key(p)
        });
        Arc::clone(&self.tickers)
    }
    /// Whether a slot from `tickers` still ticks.
    pub fn is_live(&self, (pos, slot): (Pos, u32)) -> bool {
        self.live.get(&pos) == Some(&slot) && self.map.contains_key(&pos)
    }
    /// Frees the slot at `pos` (the block entity stays).
    pub fn forget(&mut self, pos: &Pos) {
        self.cancel(pos);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "ae2-lab")]
    #[test]
    fn vanilla_metadata_follows_loaded_incarnation_and_never_a_replacement() {
        use horizon_world::nbt::Tag;
        use std::sync::Arc;
        let p=(2,64,3);
        let raw=Tag::compound([
            ("id",Tag::String("minecraft:shulker_box".into())),
            ("CustomName",Tag::compound([("text",Tag::String("Vault".into())),("color",Tag::String("red".into()))])),
            ("lock",Tag::compound([("sentinel",Tag::Int(7))])),
        ]);
        let mut chunk=crate::chunk::Chunk::empty(0,0);
        chunk.block_entities.push((p,BlockEntity::Chest(vec![None;27])));
        chunk.saved=Some(Arc::new(crate::save::ChunkExtras {block_entity_sources:vec![(p,raw.clone())],..Default::default()}));
        let mut b=level();
        b.prepare_vanilla_metadata(&chunk);
        b.load_chunk(std::mem::take(&mut chunk.block_entities));
        assert_eq!(b.vanilla_metadata(&p),Some(&raw));
        let e=b.take(&p).unwrap();b.put(p,e);
        assert_eq!(b.vanilla_metadata(&p),Some(&raw),"taking for a tick keeps the same owner");
        chunk.block_entities.push((p,b.get(&p).unwrap().clone()));
        b.project_vanilla_metadata(&mut chunk);
        assert_eq!(chunk.saved.as_ref().unwrap().block_entity_sources,vec![(p,raw.clone())]);
        b.remove(&p);
        b.insert(p,BlockEntity::Chest(vec![None;27]));
        assert!(b.vanilla_metadata(&p).is_none(),"the new block must not inherit the old name or lock");
        chunk.block_entities[0].1=b.get(&p).unwrap().clone();
        b.project_vanilla_metadata(&mut chunk);
        assert!(chunk.saved.as_ref().unwrap().block_entity_sources.is_empty());
        b.remember_vanilla_metadata(p,&raw);
        b.retain(|_,_|false);
        assert!(b.vanilla_metadata(&p).is_none(),"chunk unload removes the old owner");
        b.load_chunk(vec![(p,BlockEntity::Chest(vec![None;27]))]);
        assert!(b.vanilla_metadata(&p).is_none(),"a later unrelated load is fresh");
    }

    fn hopper() -> BlockEntity {
        BlockEntity::Chest(vec![None; 5])
    }
    /// States: 0 air, 1 ticks, 2 idle.
    fn level() -> BlockEntities {
        let mut b = BlockEntities::default();
        b.set_states(vec![Ticking::No, Ticking::Yes, Ticking::Idle]);
        b
    }
    fn order(b: &mut BlockEntities, states: &HashMap<Pos, u32>) -> Vec<Pos> {
        b.tickers(|p| states.get(&p).copied().unwrap_or(0))
            .iter()
            .map(|(p, _)| *p)
            .collect()
    }

    #[test]
    fn slots_follow_vanilla_lifecycle() {
        let mut b = level();
        let mut s = HashMap::new();
        for p in [(0, 0, 0), (1, 0, 0), (2, 0, 0)] {
            s.insert(p, 1);
            b.insert(p, hopper());
        }
        assert_eq!(order(&mut b, &s), [(0, 0, 0), (1, 0, 0), (2, 0, 0)]);
        // Replacing keeps the slot; removing and placing again appends.
        b.insert((0, 0, 0), hopper());
        b.remove(&(1, 0, 0));
        b.insert((1, 0, 0), hopper());
        assert_eq!(order(&mut b, &s), [(0, 0, 0), (2, 0, 0), (1, 0, 0)]);
        // Take and put keep it; an idle state frees it, ticking again appends.
        let e = b.take(&(0, 0, 0)).unwrap();
        b.put((0, 0, 0), e);
        assert_eq!(order(&mut b, &s), [(0, 0, 0), (2, 0, 0), (1, 0, 0)]);
        s.insert((2, 0, 0), 2);
        b.state_changed((2, 0, 0), 1, 2);
        assert_eq!(order(&mut b, &s), [(0, 0, 0), (1, 0, 0)]);
        s.insert((2, 0, 0), 1);
        b.state_changed((2, 0, 0), 2, 1);
        assert_eq!(order(&mut b, &s), [(0, 0, 0), (1, 0, 0), (2, 0, 0)]);
        // A slot freed during a tick is not live any more.
        let slots = b.tickers(|p| s.get(&p).copied().unwrap_or(0));
        b.retain(|p, _| *p != (1, 0, 0));
        assert!(b.is_live(slots[0]) && !b.is_live(slots[1]));
        // A block entity taken and never brought back loses its slot.
        b.take(&(0, 0, 0));
        assert_eq!(order(&mut b, &s), [(2, 0, 0)]);
        assert_eq!(order(&mut b, &s), [(2, 0, 0)]);
    }

    #[cfg(feature = "ae2-lab")]
    #[test]
    fn ae2_bulk_unload_emits_observed_host_iteration_order() {
        use crate::ae2_world::{Machine, MachineKind};
        let mut entities = level(); entities.start_ae2_journal();
        let [a, b, c, d, e] = [(0, 64, 0), (1, 64, 0), (2, 64, 0), (3, 64, 0), (4, 64, 0)];
        for pos in [a, b, c, d, e] { entities.insert(pos, BlockEntity::Ae2(Box::new(Machine::new(MachineKind::CableBus)))); }
        entities.take_ae2_mutations().unwrap();
        entities.remove(&a);
        assert!(matches!(entities.take_ae2_mutations().unwrap()[0].kind, Ae2MutationKind::EntityRemoved { bulk_unload: false, .. }));
        let mut removed_order = Vec::new();
        entities.retain(|pos, _| {
            if *pos == b || *pos == c { removed_order.push(*pos); false } else { true }
        });
        let events = entities.take_ae2_mutations().unwrap();
        assert_eq!(events.iter().map(|event| event.position).collect::<Vec<_>>(), removed_order);
        assert!(events.iter().all(|event| matches!(event.kind, Ae2MutationKind::EntityRemoved { bulk_unload: true, .. })));
        let clear_order: Vec<_> = entities.iter().map(|(pos, _)| *pos).collect();
        entities.clear();
        let events = entities.take_ae2_mutations().unwrap();
        assert_eq!(events.iter().map(|event| event.position).collect::<Vec<_>>(), clear_order);
        assert!(events.iter().all(|event| matches!(event.kind, Ae2MutationKind::EntityRemoved { bulk_unload: true, .. })));
    }

    #[cfg(feature = "ae2-lab")]
    #[test]
    fn opt_in_ae2_journal_records_ordered_state_entity_and_part_mutations() {
        use crate::ae2_world::{Machine, MachineKind};
        use horizon_world::nbt::Tag;
        let mut b = level(); let p = (2,64,3);
        b.start_ae2_journal();
        b.state_changed(p, 0, 17);
        b.insert(p, BlockEntity::Ae2(Box::new(Machine::new(MachineKind::CableBus))));
        let first = b.incarnation(&p).unwrap();
        let part = Tag::compound([("id", Tag::String("ae2:export_bus".into()))]);
        b.record_ae2_part(p, part.clone(), true);
        b.record_ae2_center_change(p, None, Some(Tag::String("ae2:fluix_glass_cable".into())));
        let changed = Tag::compound([("id", Tag::String("ae2:toggle_bus".into()))]);
        b.record_ae2_part_change(p, part.clone(), changed);
        let node = ae2_network_core::NodeIdentity {host:first,part:None,outer:false};
        b.record_ae2_forced_mode(p, node, Some(ae2_network_core::ChannelMode::X3));
        let held = b.take(&p).unwrap(); b.put(p, held);
        b.record_ae2_part(p, part, false);
        b.remove(&p);
        b.insert(p, BlockEntity::Ae2(Box::new(Machine::new(MachineKind::CableBus))));
        let next = b.incarnation(&p).unwrap();
        let events = b.take_ae2_mutations().unwrap();
        assert_eq!(events.len(), 9);
        assert!(events.windows(2).all(|pair| pair[0].sequence < pair[1].sequence));
        assert!(matches!(events[0].kind, Ae2MutationKind::State {old:0,new:17}));
        assert!(matches!(events[1].kind, Ae2MutationKind::EntityAdded {incarnation,..} if incarnation == first));
        assert!(matches!(events[2].kind, Ae2MutationKind::PartAdded {incarnation,..} if incarnation == first));
        assert!(matches!(events[3].kind, Ae2MutationKind::CenterChanged {incarnation,..} if incarnation == first));
        assert!(matches!(events[4].kind, Ae2MutationKind::PartChanged {incarnation,..} if incarnation == first));
        assert!(matches!(events[5].kind, Ae2MutationKind::ForcedMode {node: event_node,mode:Some(ae2_network_core::ChannelMode::X3)} if event_node == node));
        assert!(matches!(events[6].kind, Ae2MutationKind::PartRemoved {incarnation,..} if incarnation == first));
        assert!(matches!(events[7].kind, Ae2MutationKind::EntityRemoved {incarnation,..} if incarnation == first));
        assert!(matches!(events[8].kind, Ae2MutationKind::EntityAdded {incarnation,..} if incarnation == next));
        assert!(b.take_ae2_mutations().unwrap().is_empty());
    }
    #[cfg(feature = "ae2-lab")]
    #[test]
    fn ae2_raw_replacement_records_loaded_state_without_losing_lifecycle() {
        use crate::ae2_world::{Machine,MachineKind};
        use horizon_world::nbt::Tag;
        let mut entities=level(); let p=(1,64,2);
        entities.start_ae2_journal();
        entities.insert(p,BlockEntity::Ae2(Box::new(Machine::new(MachineKind::PatternProvider))));
        let old=entities.incarnation(&p).unwrap(); entities.take_ae2_mutations().unwrap();
        let raw=Tag::compound([("Items",Tag::List(Vec::new()))]);
        entities.replace_from_data(p,BlockEntity::Ae2(Box::new(Machine::new(MachineKind::PatternProvider))),&raw);
        let next=entities.incarnation(&p).unwrap(); assert_ne!(old,next);
        let events=entities.take_ae2_mutations().unwrap(); assert_eq!(events.len(),2);
        assert!(matches!(&events[0].kind,Ae2MutationKind::EntityRemoved{incarnation,..} if *incarnation==old));
        assert!(matches!(&events[1].kind,Ae2MutationKind::EntityAdded{incarnation,provider_inventory:Ae2ProviderInitialInventory::Saved(Some(Tag::List(items))),..} if *incarnation==next&&items.is_empty()));
        assert!(events[0].sequence<events[1].sequence);
    }
    #[cfg(feature = "ae2-lab")]
    #[test]
    fn placed_item_import_keeps_incarnation_and_updates_pending_ready_data() {
        use crate::ae2_world::{Machine,MachineKind};
        use crate::items::ItemStack;
        use horizon_world::nbt::Tag;
        let mut entities=level();entities.start_ae2_journal();
        let provider=(1,64,2);let cpu=(2,64,2);let energy=(3,64,2);
        for (pos,kind) in [(provider,MachineKind::PatternProvider),(cpu,MachineKind::CraftingStorage1k),(energy,MachineKind::EnergyCell)] {
            entities.insert(pos,BlockEntity::Ae2(Box::new(Machine::new(kind))));
        }
        let identities=[provider,cpu,energy].map(|pos|entities.incarnation(&pos).unwrap());
        let mut imported=Machine::new(MachineKind::PatternProvider);
        imported.slots[0]=Some(ItemStack::new("ae2:encoded_pattern",1));
        imported.config.insert("priority".into(),Tag::Int(17));
        let items=crate::save::encode_items(&imported.slots);
        let provider_raw=Tag::compound([("Items",items.clone())]);
        entities.import_placed_in_place(provider,BlockEntity::Ae2(Box::new(imported.clone()))).unwrap();
        assert_eq!(entities.refresh_pending_placed_ae2(provider,Some(&provider_raw)),Some(true));
        let mut imported_cpu=Machine::new(MachineKind::CraftingStorage1k);
        imported_cpu.config.insert("core".into(),Tag::Byte(1));
        let cpu_raw=Tag::compound([("core",Tag::Byte(1))]);
        entities.import_placed_in_place(cpu,BlockEntity::Ae2(Box::new(imported_cpu))).unwrap();
        assert_eq!(entities.refresh_pending_placed_ae2(cpu,Some(&cpu_raw)),Some(true));
        if let Some(BlockEntity::Ae2(machine))=entities.get_mut(&energy) {machine.energy=1234.0;}
        assert_eq!(entities.refresh_pending_placed_ae2(energy,None),Some(true));
        assert_eq!([provider,cpu,energy].map(|pos|entities.incarnation(&pos).unwrap()),identities);
        let events=entities.take_ae2_mutations().unwrap();
        assert_eq!(events.len(),3,"imports do not emit remove/add pairs");
        assert!(events.windows(2).all(|pair|pair[0].sequence<pair[1].sequence));
        assert!(matches!(&events[0].kind,Ae2MutationKind::EntityAdded {incarnation,entity:BlockEntity::Ae2(machine),provider_inventory:Ae2ProviderInitialInventory::Saved(Some(raw)),..}
            if *incarnation==identities[0] && machine.slots==imported.slots && machine.config.get(&"priority".into())==Some(&Tag::Int(17)) && raw==&items));
        assert!(matches!(&events[1].kind,Ae2MutationKind::EntityAdded {incarnation,cpu_core:Ae2CpuInitialCore::Saved(Some(Tag::Byte(1))),..} if *incarnation==identities[1]));
        assert!(matches!(&events[2].kind,Ae2MutationKind::EntityAdded {incarnation,entity:BlockEntity::Ae2(machine),..} if *incarnation==identities[2] && machine.energy==1234.0));
        assert_eq!(entities.refresh_pending_placed_ae2(provider,Some(&provider_raw)),Some(false),"a drained Added cannot be rewritten");
        assert!(entities.import_placed_in_place(provider,BlockEntity::Ae2(Box::new(Machine::new(MachineKind::EnergyCell)))).is_err());
        assert_eq!(entities.incarnation(&provider),Some(identities[0]));
    }
    #[cfg(feature = "ae2-lab")]
    #[test]
    fn provider_refresh_snapshots_preserve_raw_data_and_reject_stale_owners() {
        use crate::ae2_world::{Machine, MachineKind};
        use ae2_network_core::NodeIdentity;
        use horizon_world::nbt::Tag;
        let mut b=level();let pos=(3,64,2);b.start_ae2_journal();
        b.insert(pos,BlockEntity::Ae2(Box::new(Machine::new(MachineKind::PatternProvider))));
        let block=NodeIdentity{host:b.incarnation(&pos).unwrap(),part:None,outer:false};
        let snapshot=Ae2ProviderSnapshot::Block {slot_count:9,raw_slots:Tag::List(vec![]),
            raw_priority:Some(Tag::String("malformed".into()))};
        b.record_ae2_provider_refresh(pos,block,None,Ae2ProviderRefreshCause::PrioritySetter,snapshot.clone()).unwrap();
        b.insert(pos,BlockEntity::Ae2(Box::new(Machine::new(MachineKind::CableBus))));
        assert_eq!(b.record_ae2_provider_refresh(pos,block,None,Ae2ProviderRefreshCause::PrioritySetter,snapshot.clone()),Err("provider refresh identity mismatch"));
        let part=NodeIdentity{host:b.incarnation(&pos).unwrap(),part:Some([1,2,3,4]),outer:false};
        let malformed=Ae2ProviderSnapshot::Part {raw_machine:Some(Tag::Int(7))};
        b.record_ae2_provider_refresh(pos,part,Some(5),Ae2ProviderRefreshCause::PatternsChanged,malformed.clone()).unwrap();
        let events=b.take_ae2_mutations().unwrap();
        let snapshots:Vec<_>=events.iter().filter_map(|e|match &e.kind {
            Ae2MutationKind::ProviderRefresh{node,face,cause,snapshot}=>Some((*node,*face,*cause,snapshot.clone())),
            _=>None,
        }).collect();
        assert_eq!(snapshots,vec![(block,None,Ae2ProviderRefreshCause::PrioritySetter,snapshot),(part,Some(5),Ae2ProviderRefreshCause::PatternsChanged,malformed)]);
        assert!(events.windows(2).all(|pair|pair[0].sequence<pair[1].sequence));
    }
    #[cfg(feature = "ae2-lab")]
    #[test]
    fn loaded_world_journals_share_a_global_mutation_order() {
        let mut overworld=level(); let mut spatial=level();
        overworld.start_ae2_journal(); spatial.start_ae2_journal();
        let clock=std::sync::Arc::new(std::sync::atomic::AtomicU64::new(41));
        overworld.set_ae2_journal_clock(clock.clone()); spatial.set_ae2_journal_clock(clock.clone());
        overworld.state_changed((0,64,0),0,1);
        spatial.state_changed((1,64,0),0,2);
        clock.store(42,std::sync::atomic::Ordering::Relaxed);
        overworld.state_changed((2,64,0),0,3);
        let mut events=overworld.take_ae2_mutations().unwrap();
        events.extend(spatial.take_ae2_mutations().unwrap());
        events.sort_by_key(|event| event.sequence);
        assert_eq!(events.iter().map(|event|event.server_tick).collect::<Vec<_>>(), vec![41,41,42]);
        assert_eq!(events.iter().map(|event|event.position.0).collect::<Vec<_>>(),vec![0,1,2]);
        // Re-enabling cannot erase pending events.
        overworld.state_changed((4,64,0),0,4);overworld.start_ae2_journal();
        assert_eq!(overworld.take_ae2_mutations().unwrap().len(),1);
    }
    #[cfg(feature = "ae2-lab")]
    #[test]
    fn chunk_state_precedes_registered_entity_in_ordered_journal() {
        use crate::ae2_world::{Machine, MachineKind};
        let mut b=level();let p=(18,64,3);b.start_ae2_journal();
        let mut chunk = crate::chunk::Chunk::empty(1,0);
        chunk.set(p.0 & 15, p.1, p.2 & 15, 19);
        chunk.block_entities.push((p,BlockEntity::Ae2(Box::new(Machine::new(MachineKind::Drive)))));
        b.record_ae2_chunk_states(&chunk);
        b.load_chunk(std::mem::take(&mut chunk.block_entities));
        let events=b.take_ae2_mutations().unwrap();
        assert_eq!(events.len(),2);
        assert!(matches!(events[0].kind,Ae2MutationKind::ChunkState {state:19}));
        assert!(matches!(events[1].kind,Ae2MutationKind::EntityAdded {..}));
        assert!(events[0].sequence < events[1].sequence);
    }
    #[test]
    fn incarnation_tracks_replacement_unload_and_temporary_take() {
        let mut b = level();
        let p = (2, 64, 3);
        b.insert(p, hopper());
        let first = b.incarnation(&p).unwrap();
        let entity = b.take(&p).unwrap();
        assert_eq!(b.incarnation(&p), Some(first));
        b.put(p, entity);
        assert_eq!(b.incarnation(&p), Some(first));
        b.insert(p, hopper());
        let replacement = b.incarnation(&p).unwrap();
        assert_ne!(first, replacement);
        b.retain(|_, _| false);
        assert_eq!(b.incarnation(&p), None);
        b.load_chunk(vec![(p, hopper())]);
        assert_ne!(b.incarnation(&p), Some(replacement));
        b.remove(&p);
        assert_eq!(b.incarnation(&p), None);
    }

    #[test]
    fn chunk_loads_in_open_hash_map_order() {
        // A few keys: their mixed hashes over 32 slots, read from the top.
        let positions = [
            (0, 64, 0),
            (1, 64, 0),
            (0, 64, 1),
            (15, -64, 15),
            (3, 70, 9),
        ];
        let slot = |p: &Pos| mix(java_hash(*p)) & 31;
        let mut expected = positions.to_vec();
        expected.sort_by_key(|p| std::cmp::Reverse(slot(p)));
        let distinct: std::collections::HashSet<usize> = positions.iter().map(slot).collect();
        assert_eq!(distinct.len(), positions.len());
        assert_eq!(chunk_map_order(positions), expected);
        // Past 24 keys the table doubles; every key stays, once.
        let many: Vec<Pos> = (0..100).map(|i| (i % 16, i / 16, 3)).collect();
        let order = chunk_map_order(many.iter().copied().chain(many.iter().copied()));
        assert_eq!(order.len(), 100);
    }

    /// A chunk loaded over a block entity already there: the new one replaces
    /// it, and keeps the old slot only while its block state ticks.
    #[test]
    fn loaded_chunk_replaces_block_entity_by_state() {
        let mut b = level();
        let mut s = HashMap::new();
        let (p, q) = ((0, 0, 0), (1, 0, 0));
        for pos in [p, q] {
            s.insert(pos, 1);
        }
        // Two chunks, so the order is the order they load in.
        b.load_chunk(vec![(p, hopper())]);
        b.load_chunk(vec![(q, hopper())]);
        assert_eq!(order(&mut b, &s), [p, q]);
        // Replaced where the state ticks: the slot stays in its place.
        b.load_chunk(vec![(p, hopper())]);
        assert_eq!(order(&mut b, &s), [p, q]);
        // Replaced where the state has no ticker: the slot is freed, and the
        // new block entity is the one in the map.
        s.insert(p, 2);
        b.load_chunk(vec![(p, BlockEntity::Chest(vec![None; 27]))]);
        assert_eq!(order(&mut b, &s), [q]);
        assert!(matches!(b.get(&p), Some(BlockEntity::Chest(items)) if items.len() == 27));
        // Ticking again later appends it, as any new ticker does.
        s.insert(p, 1);
        b.state_changed(p, 2, 1);
        assert_eq!(order(&mut b, &s), [q, p]);
    }
}
