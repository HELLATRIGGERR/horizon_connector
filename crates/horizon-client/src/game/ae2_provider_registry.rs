//! Capture committed Pattern Provider changes for the ordered service model.
//! This records source snapshots only; scheduling remains in the existing path
//! until the complete provider lifecycle consumer is installed.
use super::{Game, ae2_global::GridAddress};
use ae2_crafting_core::{Inventory, cpu_formation::{CpuFormation, CpuPos, FormationEvent}, emittable_lifecycle::EmittableLifecycle, provider_lifecycle::ProviderLifecycle, provider_registry::Definition};
use ae2_network_core::{NodeIdentity, physical_events::PhysicalTopology, service_events::MembershipChange};
use horizon_client::{
    ae2_event_replay::ReplayCheckpoint,
    ae2_world::MachineKind,
    block_entities::{Ae2CpuInitialCore, Ae2ProviderInitialInventory, Ae2ProviderRefreshCause, Ae2ProviderSnapshot},
    containers::BlockEntity,
    items::ItemStack,
    save,
};
use horizon_world::nbt::Tag;
use std::{collections::HashMap, hash::Hash};

#[derive(Clone, Debug, PartialEq)]
struct ProviderData {
    slots: Vec<Option<ItemStack>>,
    priority: i32,
}

fn cpu_kind(kind: MachineKind) -> Option<(u64, u32)> {
    Some(match kind {
        MachineKind::CraftingUnit | MachineKind::CraftingMonitor => (0, 0),
        MachineKind::CraftingAccelerator => (0, 1),
        MachineKind::CraftingStorage1k => (1024, 0),
        MachineKind::CraftingStorage4k => (4096, 0),
        MachineKind::CraftingStorage16k => (16384, 0),
        MachineKind::CraftingStorage64k => (65536, 0),
        MachineKind::CraftingStorage256k => (262144, 0),
        _ => return None,
    })
}

fn cpu_initial_core(initial: &Ae2CpuInitialCore) -> Result<bool, String> {
    match initial {
        // Source `getBoolean` defaults an absent core flag to false. Native
        // job custody stays at its physical owner and is bound separately.
        Ae2CpuInitialCore::Constructed | Ae2CpuInitialCore::Saved(None) => Ok(false),
        Ae2CpuInitialCore::Saved(Some(Tag::Byte(0))) => Ok(false),
        Ae2CpuInitialCore::Saved(Some(Tag::Byte(1))) => Ok(true),
        Ae2CpuInitialCore::Saved(Some(_)) => Err("saved CPU core flag malformed".into()),
        Ae2CpuInitialCore::Unavailable => Err("saved CPU core source unavailable".into()),
    }
}

/// Decode only after the exact snapshot has been journaled. Missing fields have
/// the source default; malformed fields fail closed and are never replaced by
/// a convenient empty inventory or priority zero.
fn provider_items(tag: Option<&Tag>, size: usize) -> Result<Vec<Option<ItemStack>>, String> {
    let slots = super::ae2_parts::strict_part_items(tag, size)?;
    if let Some(Tag::List(entries)) = tag {
        for entry in entries {
            if let Some(components) = entry.get("components") {
                let Tag::Compound(raw) = components else { return Err("malformed provider item components".into()); };
                let stack = save::item_from_nbt(entry).ok_or("invalid provider item")?;
                let encoded = save::item_to_nbt(&stack);
                if encoded.get("components") != Some(components)
                    && !(raw.is_empty() && encoded.get("components").is_none()) {
                    return Err("provider item components were normalized".into());
                }
            }
        }
    }
    Ok(slots)
}
fn decode_snapshot(snapshot: &Ae2ProviderSnapshot) -> Result<ProviderData, String> {
    let priority = |tag: Option<&Tag>| match tag {
        None => Ok(0),
        Some(Tag::Int(value)) => Ok(*value),
        Some(_) => Err("malformed provider priority".to_owned()),
    };
    match snapshot {
        Ae2ProviderSnapshot::Block {slot_count,raw_slots,raw_priority} => {
            if *slot_count != MachineKind::PatternProvider.physical_slots() {
                return Err("invalid provider block slot count".into());
            }
            Ok(ProviderData {slots:provider_items(Some(raw_slots),*slot_count)?,
                priority:priority(raw_priority.as_ref())?})
        }
        Ae2ProviderSnapshot::Part {raw_machine} => {
            let data = match raw_machine {
                None => return Ok(ProviderData {slots:vec![None;MachineKind::PatternProvider.physical_slots()],priority:0}),
                Some(Tag::Compound(data)) => data,
                Some(_) => return Err("malformed provider part machine".into()),
            };
            let config = match data.get(&"config".into()) {
                None => None,
                Some(Tag::Compound(config)) => Some(config),
                Some(_) => return Err("malformed provider part config".into()),
            };
            Ok(ProviderData {
                slots:provider_items(data.get(&"slots".into()),MachineKind::PatternProvider.physical_slots())?,
                priority:priority(config.and_then(|config|config.get(&"priority".into())))?,
            })
        }
    }
}

/// Decode physical slots into source-style pattern identities and their
/// primary output. Item count is not part of an AE item key. Recipe validity
/// comes from the real host recipe tables supplied by the caller.
fn provider_definitions(
    snapshot: &Ae2ProviderSnapshot,
    mut crafting_output: impl FnMut(&ItemStack, &[Option<ItemStack>], &ItemStack) -> Option<ItemStack>,
    mut special_output: impl FnMut(&ItemStack, &super::ae2_processing::SpecialPattern) -> Option<ItemStack>,
    mut known_item: impl FnMut(&ItemStack) -> bool,
) -> Result<Vec<Definition<String, String>>, String> {
    fn item_key(item: &ItemStack) -> String {
        let mut one = item.clone(); one.count = 1;
        horizon_client::item_data::canon_stack(Some(&one))
    }
    fn fluid_key(key: &ae2_cell_core::ItemKey) -> String {
        let components = key.components.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        format!("fluid|{}|{}", key.item_id, components)
    }
    let mut result = Vec::new();
    for mut item in decode_snapshot(snapshot)?.slots.into_iter().flatten() {
        item.count = 1;
        let primary = if let Some(special) = super::ae2_processing::ae2_decode_special_pattern(&item) {
            let Some(output)=special_output(&item,&special) else { continue; };
            item_key(&output)
        } else if let Some((inputs, outputs)) = super::ae2_processing::ae2_decode_processing_pattern(&item) {
            if inputs.iter().chain(&outputs).flatten().any(|value|!known_item(value)) {continue;}
            let (_, fluids) = super::ae2_processing::ae2_decode_processing_fluids(&item).ok_or("invalid processing fluids")?;
            match super::ae2_processing::ae2_decode_processing_positions(&item)? {
                Some((_, positioned_fluids)) => {
                    if let Some(output) = outputs.first().and_then(Option::as_ref) { item_key(output) }
                    else if let Some(output) = positioned_fluids.first().and_then(Option::as_ref) { fluid_key(&output.key) }
                    else { return Err("missing processing primary output".into()); }
                }
                None => {
                    // Legacy terminal encoding flattened both output lists,
                    // so item index zero cannot prove that no fluid preceded it.
                    if outputs.iter().any(Option::is_some) && !fluids.is_empty() {
                        return Err("ambiguous legacy mixed processing primary output".into());
                    }
                    match outputs.iter().flatten().next() {
                        Some(output) => item_key(output),
                        None => match fluids.first() { Some(output) => fluid_key(&output.key), None => continue },
                    }
                }
            }
        } else if let Some((grid, output)) = super::ae2_processing::decode_pattern(&item) {
            let Some(output)=crafting_output(&item,&grid,&output) else { continue; };
            item_key(&output)
        } else {
            // The source ignores stacks that do not decode to a pattern.
            continue;
        };
        result.push(Definition { key: item_key(&item), primary_output: primary });
    }
    Ok(result)
}

/// `GridNode.loadFromNBT` compares node data while ignoring only player ID;
/// absent and empty compounds are equivalent for that comparison.
pub(super) fn provider_node_data_unchanged(before: Option<&Tag>, after: Option<&Tag>) -> bool {
    fn normalized(tag: Option<&Tag>) -> Option<horizon_world::nbt::Compound> {
        let Some(Tag::Compound(raw)) = tag else { return None; };
        let mut data = raw.clone();
        data.remove(&"p".into());
        (!data.is_empty()).then_some(data)
    }
    normalized(before) == normalized(after)
}

/// `AppEngInternalInventory.readFromNBT` overwrites only listed slots on an
/// existing logic object. Missing slots or a missing/non-list tag stay as-is.
pub(super) fn provider_merge_raw_slots(previous: &[Option<ItemStack>], raw: Option<&Tag>) -> Vec<Option<ItemStack>> {
    let mut slots = previous.to_vec();
    if let Some(Tag::List(entries)) = raw {
        for entry in entries {
            let index = match entry.get("Slot") {
                Some(Tag::Byte(n)) => i64::from(*n),
                Some(Tag::Short(n)) => i64::from(*n),
                Some(Tag::Int(n)) => i64::from(*n),
                Some(Tag::Long(n)) => *n,
                _ => 0,
            };
            if let Ok(index) = usize::try_from(index) {
                if let Some(slot) = slots.get_mut(index) { *slot = save::item_from_nbt(entry); }
            }
        }
    }
    slots
}

#[derive(Clone)]
struct ProviderPhysical {
    dimension: i32,
    position: (i32, i32, i32),
    face: Option<u8>,
    snapshot: Ae2ProviderSnapshot,
}
#[derive(Clone)]
struct EmitterPhysical {
    dimension: i32,
    position: (i32, i32, i32),
    face: u8,
    snapshot: Tag,
}

/// An actual registered medium and its source-effective pattern cache. The
/// cache can differ from current physical NBT until `updatePatterns` fires.
#[derive(Clone, Debug)]
pub(super) struct ProviderMedium {
    pub node: NodeIdentity,
    pub dimension: i32,
    pub position: (i32, i32, i32),
    pub face: Option<u8>,
    pub effective_patterns: Vec<String>,
}
#[derive(Clone, Debug)]
pub(super) struct PlanningDefinition {
    pub key: String,
    pub primary_output: String,
    pub medium: ProviderMedium,
}

/// A real cluster replacement callback whose physical custody still needs to
/// be reconciled by the level owning these block entities.
#[derive(Clone, Debug)]
pub(super) struct CpuBreakNotice {
    pub sequence: u64,
    pub members: Vec<(NodeIdentity, CpuPos, bool)>,
    pub removed: Option<NodeIdentity>,
    pub removed_owned_job: bool,
    /// Members proven idle by their actual unload callback, not a world scan.
    pub unloaded_idle: Vec<NodeIdentity>,
}
#[derive(Clone)]
struct UnloadedCpu {
    position: CpuPos,
    waiting: Result<Option<Inventory>,String>,
    stale: bool,
}

/// Ordered provider observer. A diagnostic shadow may opt in before node
/// creation; the planner does not yet consume these services.
#[derive(Clone)]
pub(super) struct ProviderObserver<D, K> {
    physical: HashMap<NodeIdentity, ProviderPhysical>,
    lifecycle: ProviderLifecycle<NodeIdentity, D, K>,
    emitters: HashMap<NodeIdentity, EmitterPhysical>,
    emittable: EmittableLifecycle<NodeIdentity, K>,
    failure: Option<String>,
    cpu: Option<CpuFormation<NodeIdentity>>,
    cpu_failure: Option<String>,
    cpu_breaks: Vec<CpuBreakNotice>,
    cpu_break_members: usize,
    next_cpu_break: u64,
    cpu_unloaded: HashMap<NodeIdentity,UnloadedCpu>,
}
impl<D, K> Default for ProviderObserver<D, K> {
    fn default() -> Self {
        Self { physical: HashMap::new(), lifecycle: ProviderLifecycle::default(),
            emitters: HashMap::new(), emittable: EmittableLifecycle::default(), failure: None,
            cpu: None, cpu_failure: None, cpu_breaks: Vec::new(), cpu_break_members: 0, next_cpu_break: 0,
            cpu_unloaded: HashMap::new() }
    }
}

fn emitter_part(part: &Tag, host: u64) -> Result<Option<(NodeIdentity, u8)>, String> {
    if !matches!(part.get("id"), Some(Tag::String(id)) if id.to_string_lossy() == "ae2:level_emitter") {
        return Ok(None);
    }
    let face = match part.get("face") {
        Some(Tag::String(face)) => super::ae2_grid::face_index(&face.to_string_lossy()).ok_or("invalid emitter face")?,
        _ => return Err("emitter face unavailable".into()),
    };
    let identity = super::ae2_channels::part_identity(part).ok_or("emitter identity unavailable")?;
    Ok(Some((NodeIdentity { host, part: Some(identity), outer: false }, face)))
}

fn provider_part(part: &Tag, host: u64) -> Result<Option<(NodeIdentity, u8, Ae2ProviderSnapshot)>, String> {
    if !matches!(part.get("id"), Some(Tag::String(id)) if id.to_string_lossy() == "ae2:cable_pattern_provider") {
        return Ok(None);
    }
    let face = match part.get("face") {
        Some(Tag::String(face)) => super::ae2_grid::face_index(&face.to_string_lossy()).ok_or("invalid provider part face")?,
        _ => return Err("provider part face unavailable".into()),
    };
    let identity = super::ae2_channels::part_identity(part).ok_or("provider part identity unavailable")?;
    Ok(Some((NodeIdentity { host, part: Some(identity), outer: false }, face,
        Ae2ProviderSnapshot::Part { raw_machine: part.get("machine").cloned() })))
}

impl<D: Clone + Eq + Hash, K: Clone + Eq> ProviderObserver<D, K> {
    /// Enable before the first node callback; this observer remains optional
    /// until the formed-cluster custody and service tick bridge are complete.
    pub(super) fn enable_cpu(&mut self) { self.cpu = Some(CpuFormation::default()); }
    pub(super) fn cpu_failure(&self) -> Option<&str> { self.cpu_failure.as_deref() }
    pub(super) fn cpu(&self) -> Result<&CpuFormation<NodeIdentity>, String> {
        if let Some(reason) = self.cpu_failure() { return Err(reason.to_owned()); }
        self.cpu.as_ref().ok_or_else(|| "CPU formation observer disabled".to_owned())
    }
    pub(super) fn cpu_mut(&mut self) -> Result<&mut CpuFormation<NodeIdentity>, String> {
        if let Some(reason) = self.cpu_failure() { return Err(reason.to_owned()); }
        self.cpu.as_mut().ok_or_else(|| "CPU formation observer disabled".to_owned())
    }
    pub(super) fn invalidate_cpu(&mut self, reason: String) {
        if self.cpu_failure.is_none() {
            if let Some(cpu) = self.cpu.as_mut() { cpu.invalidate(reason.clone()); }
            self.cpu_failure = Some(reason);
        }
    }
    pub(super) fn cpu_break_for_dimension(&self, dimension: i32) -> Result<Option<CpuBreakNotice>, String> {
        self.cpu()?;
        Ok(self.cpu_breaks.iter().find(|notice| notice.members.first().is_some_and(|(_,pos,_)| pos.dimension == dimension)).cloned())
    }
    pub(super) fn acknowledge_cpu_break(&mut self, sequence: u64) -> Result<(), String> {
        let index = self.cpu_breaks.iter().position(|notice| notice.sequence == sequence).ok_or("CPU break callback disappeared")?;
        let notice = self.cpu_breaks.remove(index);
        self.cpu_break_members -= notice.members.len();
        Ok(())
    }
    pub(super) fn unloaded_cpu_waiting(&self,node:NodeIdentity) -> Result<Option<Inventory>,String> {
        self.cpu()?;
        let snapshot=self.cpu_unloaded.get(&node).ok_or("unloaded CPU snapshot unavailable")?;
        if snapshot.stale {return Err("unloaded CPU snapshot invalidated by rebind".into());}
        snapshot.waiting.clone()
    }
    pub(super) fn cpu_service_end_tick(&mut self) {
        if self.cpu_failure.is_some() || self.cpu.is_none() { return; }
        let result = (|| {
            let cpu = self.cpu.as_mut().expect("enabled CPU observer");
            let services = cpu.dirty_services().map_err(|e| format!("CPU dirty services: {e:?}"))?;
            for service in services {
                cpu.service_tick(service).map_err(|e| format!("CPU service tick: {e:?}"))?;
            }
            Ok::<_, String>(())
        })();
        if let Err(reason) = result { self.invalidate_cpu(reason); }
    }
    pub(super) fn invalidate(&mut self, reason: String) {
        if self.failure.is_none() {
            self.lifecycle.invalidate(reason.clone());
            self.emittable.invalidate(reason.clone());
            self.failure = Some(reason);
        }
    }
    pub(super) fn failure(&self) -> Option<&str> { self.failure.as_deref() }
    pub(super) fn provider_count(&self) -> usize { self.physical.len() }
    pub(super) fn emittable_keys(&self, service: u64) -> Result<Vec<K>, String> {
        if let Some(reason) = self.failure() { return Err(reason.to_owned()); }
        self.emittable.keys(service).map_err(|e| format!("emittable service: {e:?}"))
    }
    fn insert(&mut self, node: NodeIdentity, physical: ProviderPhysical) -> Result<(), String> {
        if self.physical.contains_key(&node) || self.emitters.contains_key(&node) || self.physical.values().any(|old|
            old.dimension == physical.dimension && old.position == physical.position && old.face == physical.face)
            || self.emitters.values().any(|old| old.dimension == physical.dimension && old.position == physical.position && Some(old.face) == physical.face)
        { return Err("duplicate provider identity or face".into()); }
        let data = decode_snapshot(&physical.snapshot)?;
        self.lifecycle.add_provider(node, data.priority).map_err(|e| format!("provider constructor: {e:?}"))?;
        self.physical.insert(node, physical);
        Ok(())
    }
    fn membership(&mut self, change: MembershipChange, update_cpu: bool) {
        let provider = (|| {
            if self.failure.is_some() { return Ok(()); }
            if self.physical.contains_key(&change.node) {
                self.lifecycle.membership(&change.node, change.from, change.to)
                    .map_err(|e| format!("provider membership: {e:?}"))?;
            }
            if self.emitters.contains_key(&change.node) {
                self.emittable.membership(&change.node, change.from, change.to)
                    .map_err(|e| format!("emitter membership: {e:?}"))?;
            }
            Ok::<_, String>(())
        })();
        if let Err(reason) = provider { self.invalidate(reason); }
        if update_cpu && self.cpu_failure.is_none() {
            if let Some(cpu) = self.cpu.as_mut().filter(|cpu| cpu.contains(&change.node)) {
                if let Err(error) = cpu.membership(&change.node, change.from, change.to) {
                    self.invalidate_cpu(format!("CPU membership: {error:?}"));
                }
            }
        }
    }
    fn drain_membership(&mut self, topology: &mut PhysicalTopology,
        deliver: &mut impl FnMut(Result<MembershipChange, String>)) -> Result<(), String> {
        for change in topology.services.take_membership_changes().map_err(str::to_owned)? {
            self.membership(change, true);
            deliver(Ok(change));
        }
        Ok(())
    }
    fn initialize_cpu_node(&mut self, node: NodeIdentity, device: &mut ae2_network_core::Device, preserve: &mut bool) {
        if self.cpu_failure.is_some() { return; }
        let Some(cpu) = self.cpu.as_ref().filter(|cpu| cpu.contains(&node)) else { return; };
        match cpu.formed(&node) {
            Ok(formed) => { device.faces = if formed { 63 } else { 0 }; *preserve = true; }
            Err(error) => self.invalidate_cpu(format!("CPU initial node: {error:?}")),
        }
    }
    fn observe_cpu(&mut self, topology: &mut PhysicalTopology, checkpoint: &ReplayCheckpoint<'_>,
        deliver: &mut impl FnMut(Result<MembershipChange, String>)) -> Result<(), String> {
        if self.cpu_failure.is_some() || self.cpu.is_none() { return Ok(()); }
        use horizon_client::block_entities::Ae2MutationKind;
        let tracked = |incarnation: u64| self.cpu.as_ref().is_some_and(|cpu|
            cpu.contains(&NodeIdentity { host: incarnation, part: None, outer: false }));
        let relevant = match checkpoint {
            ReplayCheckpoint::BeforeMutation { event, .. } => match &event.kind {
                Ae2MutationKind::EntityAdded { entity: BlockEntity::Ae2(machine), .. } => cpu_kind(machine.kind).is_some(),
                Ae2MutationKind::EntityRemoved { incarnation, bulk_unload: false, .. } => tracked(*incarnation),
                _ => false,
            },
            ReplayCheckpoint::AfterMutation { event, .. } => match &event.kind {
                Ae2MutationKind::EntityRemoved { incarnation, .. } | Ae2MutationKind::CpuNeighbor { incarnation, .. } => tracked(*incarnation),
                _ => false,
            },
            ReplayCheckpoint::NodeReady { node } => self.cpu.as_ref().is_some_and(|cpu| cpu.contains(node)),
            _ => false,
        };
        if !relevant { return Ok(()); }
        let mut cpu = self.cpu.take().expect("enabled CPU observer");
        // The removed member may already be gone when the journal is replayed;
        // keep its actual identity and position from before this callback.
        let before_cpu = cpu.clone();
        let (removed, removed_owned_job) = match checkpoint {
            ReplayCheckpoint::BeforeMutation { event, .. } => match &event.kind {
                Ae2MutationKind::EntityRemoved { incarnation, entity, bulk_unload: false } => {
                    let owned = matches!(entity, BlockEntity::Ae2(machine)
                        if machine.config.contains_key(&"ae2_recursive_job".into()) || machine.config.contains_key(&"ae2_cpu_job".into()));
                    (Some(NodeIdentity { host: *incarnation, part: None, outer: false }), owned)
                }
                _ => (None, false),
            },
            _ => (None, false),
        };
        let mut next_topology = topology.clone();
        let mut changes = Vec::new();
        let mut face = |cpu: &mut CpuFormation<NodeIdentity>, node: &NodeIdentity, formed: bool| {
            use ae2_crafting_core::cpu_formation::FormationError;
            if cpu.service_of(node)?.is_none() { return Ok(()); } // resident but not ready
            next_topology.update_node_faces(*node, if formed { 63 } else { 0 })
                .map_err(|e| FormationError::Unavailable(e.to_owned()))?;
            for change in next_topology.services.take_membership_changes().map_err(|e| FormationError::Unavailable(e.to_owned()))? {
                if cpu.contains(&change.node) { cpu.membership(&change.node, change.from, change.to)?; }
                changes.push(change);
            }
            Ok(())
        };
        let result = (|| {
            match checkpoint {
                ReplayCheckpoint::BeforeMutation { dimension, event } => match &event.kind {
                    Ae2MutationKind::EntityAdded { incarnation, entity: BlockEntity::Ae2(machine), cpu_core, .. } => {
                        if let Some((storage, coprocessors)) = cpu_kind(machine.kind) {
                            let core = cpu_initial_core(cpu_core)?;
                            let (x, y, z) = event.position;
                            cpu.add_block(NodeIdentity { host: *incarnation, part: None, outer: false },
                                CpuPos { dimension: *dimension, x, y, z }, storage, coprocessors, core)
                                .map_err(|e| format!("CPU addition: {e:?}"))?;
                        }
                    }
                    Ae2MutationKind::EntityRemoved { incarnation, bulk_unload: false, .. } => {
                        let node = NodeIdentity { host: *incarnation, part: None, outer: false };
                        if cpu.contains(&node) { cpu.before_remove_with(&node, &mut face).map_err(|e| format!("CPU removal: {e:?}"))?; }
                    }
                    _ => {}
                },
                ReplayCheckpoint::AfterMutation { dimension, event } => match &event.kind {
                    Ae2MutationKind::EntityRemoved { incarnation, bulk_unload, .. } => {
                        let node = NodeIdentity { host: *incarnation, part: None, outer: false };
                        if cpu.contains(&node) {
                            if *bulk_unload { cpu.after_unload(&node) } else { cpu.after_remove(&node) }
                                .map_err(|e| format!("CPU removal completion: {e:?}"))?;
                        }
                    }
                    Ae2MutationKind::CpuNeighbor { incarnation, from } => {
                        let node = NodeIdentity { host: *incarnation, part: None, outer: false };
                        if cpu.contains(&node) {
                            match from {
                                Some((x, y, z)) => cpu.neighbor_with(&node, CpuPos { dimension: *dimension, x: *x, y: *y, z: *z }, &mut face),
                                None => cpu.neighbor_unlocated_with(&node, &mut face),
                            }.map_err(|e| format!("CPU neighbor: {e:?}"))?;
                        }
                    }
                    _ => {}
                },
                ReplayCheckpoint::NodeReady { node } => if cpu.contains(node) {
                    cpu.ready_with(node, &mut face).map_err(|e| format!("CPU ready: {e:?}"))?;
                },
                ReplayCheckpoint::NodeInitializing { .. } | ReplayCheckpoint::Topology => {}
            }
            Ok::<_, String>(())
        })();
        drop(face);
        let result = result.and_then(|()| {
            let events = cpu.take_events().map_err(|e| format!("CPU callback queue: {e:?}"))?;
            let mut notices = Vec::new();
            let mut members = self.cpu_break_members;
            let mut sequence = self.next_cpu_break;
            for event in events {
                let FormationEvent::Broken(cluster) = event else { continue; };
                let entries = cluster.members.iter().map(|node| {
                    Ok((*node, before_cpu.position_of(node).map_err(|e| format!("CPU broken member: {e:?}"))?, before_cpu.contains(node)))
                }).collect::<Result<Vec<_>, String>>()?;
                members = members.checked_add(entries.len()).ok_or("CPU break backlog overflow")?;
                if self.cpu_breaks.len() + notices.len() >= 1024 || members > 32_768 {
                    return Err("CPU break backlog exceeded its bound".into());
                }
                let id = sequence; sequence = sequence.checked_add(1).ok_or("CPU break sequence exhausted")?;
                let unloaded_idle=entries.iter().filter_map(|(node,_,resident)| {
                    (!resident && self.cpu_unloaded.get(node).is_some_and(|snapshot|matches!(&snapshot.waiting,Ok(None)))).then_some(*node)
                }).collect();
                notices.push(CpuBreakNotice { sequence:id, members:entries, removed, removed_owned_job, unloaded_idle });
            }
            self.cpu_breaks.extend(notices); self.cpu_break_members = members; self.next_cpu_break = sequence;
            Ok(())
        });
        if result.is_ok() {
            // Retain only reduced metadata from the real unload callback, and
            // only while its old cluster retains this node as a tombstone.
            if let ReplayCheckpoint::AfterMutation { dimension, event } = checkpoint {
                if let Ae2MutationKind::EntityRemoved { incarnation, entity:BlockEntity::Ae2(machine), bulk_unload:true } = &event.kind {
                    let node=NodeIdentity{host:*incarnation,part:None,outer:false};
                    if before_cpu.contains(&node) && cpu.position_of(&node).is_ok() {
                        let position=before_cpu.position_of(&node).expect("tracked CPU position");
                        let waiting=super::ae2_event_live::dimension(*dimension).ok_or_else(||"invalid CPU unload dimension".to_owned())
                            .and_then(|dim|Game::ae2_cpu_snapshot_waiting(machine,dim))
                            .and_then(|waiting| {
                                if waiting.as_ref().is_some_and(|values|values.len()>4096 || values.keys().map(String::len).sum::<usize>()>1_048_576) {
                                    Err("unloaded CPU waiting metadata exceeds its bound".into())
                                } else {Ok(waiting)}
                            });
                        self.cpu_unloaded.insert(node,UnloadedCpu{position,waiting,stale:false});
                    }
                }
            }
            if let ReplayCheckpoint::BeforeMutation { dimension, event } = checkpoint {
                if matches!(&event.kind,Ae2MutationKind::EntityAdded {entity:BlockEntity::Ae2(machine),..} if cpu_kind(machine.kind).is_some()) {
                    let (x,y,z)=event.position;
                    let added=CpuPos{dimension:*dimension,x,y,z};
                    for old in self.cpu_unloaded.values_mut().filter(|old|old.position==added) {old.stale=true;}
                }
            }
            self.cpu_unloaded.retain(|node,_|cpu.position_of(node).is_ok());
        }
        self.cpu = Some(cpu);
        match result {
            Ok(()) => {
                *topology = next_topology;
                for change in changes { self.membership(change, false); deliver(Ok(change)); }
            }
            Err(reason) => self.invalidate_cpu(reason),
        }
        Ok(())
    }
    fn insert_part(&mut self, dimension: i32, position: (i32, i32, i32), host: u64, part: &Tag,
        decode_emitter: &mut impl FnMut(&Tag) -> Result<Vec<K>, String>) -> Result<(), String> {
        if let Some((node, face, snapshot)) = provider_part(part, host)? {
            self.insert(node, ProviderPhysical { dimension, position, face: Some(face), snapshot })?;
        }
        if let Some((node, face)) = emitter_part(part, host)? {
            if self.emitters.contains_key(&node) || self.physical.contains_key(&node)
                || self.emitters.values().any(|old| old.dimension == dimension && old.position == position && old.face == face)
                || self.physical.values().any(|old| old.dimension == dimension && old.position == position && old.face == Some(face)) {
                return Err("duplicate emitter identity or face".into());
            }
            let keys = decode_emitter(part)?;
            self.emittable.add(node, keys).map_err(|e| format!("emitter constructor: {e:?}"))?;
            self.emitters.insert(node, EmitterPhysical { dimension, position, face, snapshot: part.clone() });
        }
        Ok(())
    }
    fn before(&mut self, dimension: i32, event: &horizon_client::block_entities::Ae2Mutation,
        decode_emitter: &mut impl FnMut(&Tag) -> Result<Vec<K>, String>) -> Result<(), String> {
        use horizon_client::block_entities::Ae2MutationKind;
        let position = event.position;
        match &event.kind {
            Ae2MutationKind::EntityAdded { incarnation, entity: BlockEntity::Ae2(machine), provider_inventory, .. } => {
                if machine.kind == MachineKind::PatternProvider {
                    let node = NodeIdentity { host: *incarnation, part: None, outer: false };
                    let raw_slots = match provider_inventory {
                        Ae2ProviderInitialInventory::Constructed => save::encode_items(&machine.slots),
                        Ae2ProviderInitialInventory::Saved(raw) => raw.clone().unwrap_or_else(|| Tag::List(Vec::new())),
                        Ae2ProviderInitialInventory::Unavailable => return Err("saved provider inventory source unavailable".into()),
                    };
                    let snapshot = Ae2ProviderSnapshot::Block { slot_count: machine.slots.len(),
                        raw_slots, raw_priority: machine.config.get(&"priority".into()).cloned() };
                    if decode_snapshot(&snapshot)?.slots != machine.slots {
                        return Err("saved provider inventory was normalized".into());
                    }
                    self.insert(node, ProviderPhysical { dimension, position, face: None, snapshot })?;
                } else if machine.kind == MachineKind::CableBus {
                    match machine.config.get(&"parts".into()) {
                        None => {},
                        Some(Tag::List(parts)) => for part in parts { self.insert_part(dimension, position, *incarnation, part, decode_emitter)?; },
                        Some(_) => return Err("malformed provider host part list".into()),
                    }
                }
            }
            Ae2MutationKind::PartAdded { incarnation, part } => self.insert_part(dimension, position, *incarnation, part, decode_emitter)?,
            Ae2MutationKind::PartChanged { incarnation, before, after } => {
                if before != after && (provider_part(before, *incarnation)?.is_some() || provider_part(after, *incarnation)?.is_some()) {
                    return Err("unobserved provider part mutation".into());
                }
                if before != after && (emitter_part(before, *incarnation)?.is_some() || emitter_part(after, *incarnation)?.is_some()) {
                    return Err("unobserved emitter part mutation".into());
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn after(&mut self, dimension: i32, event: &horizon_client::block_entities::Ae2Mutation,
        decode: &mut impl FnMut(&Ae2ProviderSnapshot) -> Result<Vec<Definition<D, K>>, String>,
        decode_emitter: &mut impl FnMut(&Tag) -> Result<Vec<K>, String>) -> Result<(), String> {
        use horizon_client::block_entities::Ae2MutationKind;
        let position = event.position;
        match &event.kind {
            Ae2MutationKind::EntityRemoved { incarnation, .. } => {
                let ids: Vec<_> = self.physical.iter().filter_map(|(id, physical)|
                    (id.host == *incarnation && physical.dimension == dimension && physical.position == position).then_some(*id)).collect();
                for id in ids {
                    self.lifecycle.forget_provider(&id).map_err(|e| format!("provider removal: {e:?}"))?;
                    self.physical.remove(&id);
                }
                let ids: Vec<_> = self.emitters.iter().filter_map(|(id, physical)|
                    (id.host == *incarnation && physical.dimension == dimension && physical.position == position).then_some(*id)).collect();
                for id in ids {
                    self.emittable.forget(&id).map_err(|e| format!("emitter removal: {e:?}"))?;
                    self.emitters.remove(&id);
                }
            }
            Ae2MutationKind::PartRemoved { incarnation, part } => {
                if let Some((id, face, _)) = provider_part(part, *incarnation)? {
                    let physical = self.physical.get(&id).ok_or("removed provider part unavailable")?;
                    if physical.dimension != dimension || physical.position != position || physical.face != Some(face) {
                        return Err("removed provider part location changed".into());
                    }
                    self.lifecycle.forget_provider(&id).map_err(|e| format!("provider part removal: {e:?}"))?;
                    self.physical.remove(&id);
                }
                if let Some((id, face)) = emitter_part(part, *incarnation)? {
                    let physical = self.emitters.get(&id).ok_or("removed emitter unavailable")?;
                    if physical.dimension != dimension || physical.position != position || physical.face != face {
                        return Err("removed emitter location changed".into());
                    }
                    self.emittable.forget(&id).map_err(|e| format!("emitter part removal: {e:?}"))?;
                    self.emitters.remove(&id);
                }
            }
            Ae2MutationKind::EmitterProviderRefresh { node, face, snapshot } => {
                let physical = self.emitters.get_mut(node).ok_or("refreshed emitter unavailable")?;
                if physical.dimension != dimension || physical.position != position || physical.face != *face
                    || emitter_part(snapshot, node.host)? != Some((*node, *face)) {
                    return Err("emitter refresh identity or location changed".into());
                }
                let keys = decode_emitter(snapshot)?;
                self.emittable.refresh(node, keys).map_err(|e| format!("emitter refresh: {e:?}"))?;
                physical.snapshot = snapshot.clone();
            }
            Ae2MutationKind::ProviderRefresh { node, face, cause, snapshot } => {
                let physical = self.physical.get_mut(node).ok_or("refreshed provider unavailable")?;
                if physical.dimension != dimension || physical.position != position || physical.face != *face
                    || !matches!((snapshot, face), (Ae2ProviderSnapshot::Block { .. }, None) | (Ae2ProviderSnapshot::Part { .. }, Some(_))) {
                    return Err("provider refresh location changed".into());
                }
                let data = decode_snapshot(snapshot)?;
                match cause {
                    Ae2ProviderRefreshCause::PrioritySetter => self.lifecycle.priority_setter(node, data.priority)
                        .map_err(|e| format!("provider priority setter: {e:?}"))?,
                    Ae2ProviderRefreshCause::PatternsChanged => {
                        let patterns = decode(snapshot)?;
                        // A full card import applies priority and patterns in one
                        // synchronous transaction; this is the final priority.
                        self.lifecycle.read_priority_without_refresh(node, data.priority)
                            .map_err(|e| format!("provider priority read: {e:?}"))?;
                        self.lifecycle.patterns_changed(node, patterns).map_err(|e| format!("provider pattern update: {e:?}"))?;
                    }
                }
                physical.snapshot = snapshot.clone();
            }
            Ae2MutationKind::ProviderRawRead { node, snapshot } => {
                let physical = self.physical.get_mut(node).ok_or("read provider unavailable")?;
                if physical.dimension != dimension || physical.position != position || physical.face.is_some()
                    || !matches!(snapshot, Ae2ProviderSnapshot::Block { .. }) {
                    return Err("provider raw-read location changed".into());
                }
                let data = decode_snapshot(snapshot)?;
                self.lifecycle.read_priority_without_refresh(node, data.priority)
                    .map_err(|e| format!("provider raw read: {e:?}"))?;
                physical.snapshot = snapshot.clone();
            }
            _ => {}
        }
        Ok(())
    }
    pub(super) fn observe(&mut self, topology: &mut PhysicalTopology, checkpoint: ReplayCheckpoint<'_>,
        decode: &mut impl FnMut(&Ae2ProviderSnapshot) -> Result<Vec<Definition<D, K>>, String>,
        decode_emitter: &mut impl FnMut(&Tag) -> Result<Vec<K>, String>) {
        self.observe_with_membership(topology, checkpoint, decode, decode_emitter, &mut |_| {});
    }
    /// All service consumers see the same actual ordered membership events.
    /// A CPU face callback forwards its nested batch before the next replay
    /// checkpoint, after the CPU model has consumed it synchronously.
    pub(super) fn observe_with_membership(&mut self, topology: &mut PhysicalTopology, mut checkpoint: ReplayCheckpoint<'_>,
        decode: &mut impl FnMut(&Ae2ProviderSnapshot) -> Result<Vec<Definition<D, K>>, String>,
        decode_emitter: &mut impl FnMut(&Tag) -> Result<Vec<K>, String>,
        deliver: &mut impl FnMut(Result<MembershipChange, String>)) {
        if let Err(error) = self.drain_membership(topology, deliver) {
            self.invalidate(error.clone());
            if self.cpu.is_some() { self.invalidate_cpu(error.clone()); }
            deliver(Err(error));
            return;
        }
        if let ReplayCheckpoint::NodeInitializing { node, device, preserve_faces } = &mut checkpoint {
            self.initialize_cpu_node(*node, device, preserve_faces);
        }
        if let Err(error) = self.observe_cpu(topology, &checkpoint, deliver) {
            self.invalidate_cpu(error);
        }
        if self.failure.is_some() { return; }
        let result = (|| {
            match checkpoint {
                ReplayCheckpoint::BeforeMutation { dimension, event } => self.before(dimension, event, decode_emitter)?,
                ReplayCheckpoint::AfterMutation { dimension, event } => self.after(dimension, event, decode, decode_emitter)?,
                ReplayCheckpoint::NodeReady { node } => if let Some(physical) = self.physical.get(&node) {
                    let patterns = decode(&physical.snapshot)?;
                    self.lifecycle.patterns_changed(&node, patterns).map_err(|e| format!("provider ready update: {e:?}"))?;
                },
                ReplayCheckpoint::NodeInitializing { .. } | ReplayCheckpoint::Topology => {}
            }
            Ok::<_, String>(())
        })();
        if let Err(error) = result { self.invalidate(error); }
    }
}

/// Decode only the committed face snapshot. An empty filter is valid but
/// cannot register an emittable key, even if the Crafting Card is installed.
pub(super) fn emitter_definitions(part: &Tag) -> Result<Vec<String>, String> {
    let Tag::Compound(data) = part else { return Err("invalid emitter snapshot".into()); };
    let state = super::ae2_emitter::emitter_read(data)?;
    if let Some(raw) = data.get(&"emitter_filter".into()).and_then(|item| item.get("components")) {
        let Some(super::ae2_emitter::EmitterFilter::Item(item)) = state.filter.as_ref() else { return Err("emitter item filter changed type".into()); };
        let encoded = save::item_to_nbt(item);
        if encoded.get("components") != Some(raw) && !matches!(raw, Tag::Compound(values) if values.is_empty() && encoded.get("components").is_none()) {
            return Err("emitter item components were normalized".into());
        }
    }
    let crafting = state.upgrades.iter().flatten().any(|item| item.item.as_ref() == "ae2:crafting_card");
    if !crafting || !state.craft_via_redstone { return Ok(Vec::new()); }
    let key = match state.filter {
        Some(super::ae2_emitter::EmitterFilter::Item(item)) => horizon_client::item_data::canon_stack(Some(&item)),
        Some(super::ae2_emitter::EmitterFilter::Fluid(key)) => {
            let components = key.components.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
            format!("fluid|{}|{}", key.item_id, components)
        }
        None => return Ok(Vec::new()),
    };
    Ok(vec![key])
}

impl ProviderObserver<String, String> {
    fn medium(&self, node: NodeIdentity) -> Result<ProviderMedium, String> {
        let physical = self.physical.get(&node).ok_or("provider medium unavailable")?;
        let effective_patterns = self.lifecycle.effective_patterns(&node)
            .map_err(|e| format!("provider cache: {e:?}"))?.iter().map(|definition| definition.key.clone()).collect();
        Ok(ProviderMedium { node, dimension:physical.dimension, position:physical.position,
            face:physical.face, effective_patterns })
    }
    pub(super) fn planning_definitions(&self, service: u64) -> Result<Vec<PlanningDefinition>, String> {
        if let Some(reason) = self.failure() { return Err(reason.to_owned()); }
        let Some(registry) = self.lifecycle.service(service).map_err(|e| format!("provider service: {e:?}"))? else { return Ok(Vec::new()); };
        registry.all_ranked().into_iter().map(|(_, definition)| {
            let node = registry.media_snapshot(&definition.key).into_iter().next().ok_or("registered definition has no medium")?;
            Ok(PlanningDefinition { key:definition.key, primary_output:definition.primary_output, medium:self.medium(node)? })
        }).collect()
    }
    pub(super) fn media_pass(&self, service: u64, key: &str) -> Result<Option<ae2_crafting_core::provider_registry::MediaPass<String>>, String> {
        if let Some(reason) = self.failure() { return Err(reason.to_owned()); }
        Ok(self.lifecycle.service(service).map_err(|e| format!("provider service: {e:?}"))?
            .and_then(|registry| registry.media_pass(&key.to_owned())))
    }
    pub(super) fn next_medium(&mut self, service: u64, pass: &mut ae2_crafting_core::provider_registry::MediaPass<String>) -> Result<Option<ProviderMedium>, String> {
        if let Some(reason) = self.failure() { return Err(reason.to_owned()); }
        let node = self.lifecycle.service_mut(service).map_err(|e| format!("provider service: {e:?}"))?
            .ok_or("provider service unavailable")?.next_medium(pass).map_err(|e| format!("provider media pass: {e:?}"))?;
        node.map(|node| self.medium(node)).transpose()
    }
    pub(super) fn contains_medium(&self, service: u64, node: NodeIdentity, definition: &str,
        pass: &ae2_crafting_core::provider_registry::MediaPass<String>) -> Result<bool, String> {
        if let Some(reason) = self.failure() { return Err(reason.to_owned()); }
        let Some(registry) = self.lifecycle.service(service).map_err(|e| format!("provider service: {e:?}"))? else { return Ok(false); };
        if !registry.pass_is_current_for(pass, &definition.to_owned()) { return Err("provider media pass stale".into()); }
        Ok(registry.media_snapshot(&definition.to_owned()).contains(&node)
            && self.lifecycle.effective_patterns(&node).map_err(|e| format!("provider cache: {e:?}"))?.iter().any(|p| p.key == definition))
    }
    /// Diagnostic only: enumerate registration order without advancing the
    /// shared medium iterator or speculatively simulating a push.
    pub(super) fn summary(&self, service: u64, output: &str) -> Result<serde_json::Value, String> {
        if let Some(reason) = self.failure() { return Err(reason.to_owned()); }
        let mut groups = Vec::new();
        if let Some(registry) = self.lifecycle.service(service).map_err(|e| format!("provider service: {e:?}"))? {
            for group in registry.ranked_definitions(&output.to_owned()) {
                let mut definitions = Vec::new();
                for key in group.definitions {
                    let mut media = Vec::new();
                    for node in registry.media_snapshot(&key) {
                        let physical = self.physical.get(&node).ok_or("provider medium unavailable")?;
                        media.push(serde_json::json!({"dimension":physical.dimension,"position":physical.position,
                            "face":physical.face,"host":node.host,"part":node.part}));
                    }
                    definitions.push(serde_json::json!({"key":key,"media":media}));
                }
                groups.push(serde_json::json!({"priority":group.priority,"definitions":definitions}));
            }
        }
        Ok(serde_json::json!({"service":service,"output":output,"groups":groups,"tracked_providers":self.physical.len()}))
    }
}

impl Game<'_> {
    /// Notify only after the real face and owning player transaction commit.
    /// A stale identity faults the ordered provider observer, without rolling
    /// back or altering the completed physical operation.
    pub(super) fn ae2_emitter_provider_refresh(&self, address: super::ae2_parts::PartAddress) {
        let result = (|| {
            let (dim, pos) = self.ae2_address_parts(address.host).ok_or("invalid emitter address")?;
            let world = self.ae2_world_for(dim).ok_or("emitter level unavailable")?;
            let incarnation = world.block_entities.incarnation(&pos).ok_or("emitter host incarnation unavailable")?;
            let Some(BlockEntity::Ae2(host)) = world.block_entities.get(&pos) else { return Err("emitter host unavailable".to_owned()); };
            if host.kind != MachineKind::CableBus { return Err("emitter host changed".into()); }
            let face_name = ["east", "west", "up", "down", "south", "north"]
                .get(usize::from(address.face)).ok_or("invalid emitter face")?;
            let Some(Tag::List(parts)) = host.config.get(&"parts".into()) else { return Err("emitter part list unavailable".into()); };
            let mut matches = parts.iter().filter(|part| part.get("face") == Some(&Tag::String((*face_name).into())));
            let part = matches.next().ok_or("emitter face unavailable")?;
            if matches.next().is_some() { return Err("duplicate emitter face".into()); }
            let (node, face) = emitter_part(part, incarnation)?.ok_or("emitter face changed")?;
            if face != address.face { return Err("emitter face changed".into()); }
            world.block_entities.record_ae2_emitter_provider_refresh(pos, node, face, part.clone()).map_err(str::to_owned)
        })();
        if let Err(error) = result { self.ae2_provider_events_fault(error); }
    }
    pub(super) fn ae2_provider_definitions(&self, snapshot: &Ae2ProviderSnapshot) -> Result<Vec<Definition<String, String>>, String> {
        let known=|item:&ItemStack|item.item.as_ref()!="ae2:missing_content" && self.res.items().get(&item.item).is_some();
        provider_definitions(snapshot,
            |pattern, grid, encoded| {
                if !known(encoded) || grid.iter().flatten().any(|item|!known(item)) {return None;}
                let actual=if let Some(source)=super::ae2_source_patterns::crafting(pattern) {
                    let source=source.ok()?;self.res.recipes().assemble_named(&source.recipe,grid,3)?
                } else {
                    let (_,actual)=self.res.recipes().find(grid,3)?;
                    if actual!=*encoded {return None;}actual
                };
                known(&actual).then_some(actual)
            },
            |pattern, special| {
                if !known(&special.output) || special.grid.iter().flatten().any(|item|!known(item)) {return None;}
                let actual=super::ae2_processing::special_recipe_result(self.res.stations(),&special.kind,&special.recipe,&special.grid)?;
                if super::ae2_source_patterns::special(pattern).is_none() && actual!=special.output {return None;}
                known(&actual).then_some(actual)
            },
            known)
    }
    /// Capture a committed in-place `/data` load; its source read does not
    /// refresh the cached pattern list or mounted provider entry.
    pub(super) fn ae2_provider_raw_read_local(&self, pos: (i32, i32, i32), raw_slots: Option<Tag>) {
        let result = (|| {
            let world = &self.world;
            let incarnation = world.block_entities.incarnation(&pos).ok_or("provider incarnation unavailable")?;
            let Some(BlockEntity::Ae2(machine)) = world.block_entities.get(&pos) else { return Err("provider host unavailable".to_owned()); };
            if machine.kind != MachineKind::PatternProvider { return Err("provider block changed".into()); }
            // The input can be a partial list; validate its changed entries,
            // then journal the resulting complete inventory after the merge.
            if matches!(raw_slots.as_ref(), Some(Tag::List(_))) { provider_items(raw_slots.as_ref(), machine.slots.len())?; }
            let snapshot = Ae2ProviderSnapshot::Block { slot_count: machine.slots.len(),
                raw_slots: save::encode_items(&machine.slots),
                raw_priority: machine.config.get(&"priority".into()).cloned() };
            if decode_snapshot(&snapshot)?.slots != machine.slots { return Err("provider raw-read inventory was normalized".into()); }
            let node = NodeIdentity { host: incarnation, part: None, outer: false };
            world.block_entities.record_ae2_provider_raw_read(pos, node, snapshot).map_err(str::to_owned)
        })();
        if let Err(error) = result { self.ae2_provider_events_fault(error); }
    }
    /// Notify after a real pattern-inventory change or source priority setter
    /// has committed. A capture failure disables the provider event stream,
    /// never the independent physical edit or the topology shadow.
    fn ae2_provider_refresh(&self, address: GridAddress, face: Option<u8>, cause: Ae2ProviderRefreshCause) {
        if let Err(error) = self.ae2_capture_provider_refresh(address, face, cause) {
            self.ae2_provider_events_fault(error);
        }
    }
    pub(super) fn ae2_provider_priority_setter(&self, address: GridAddress, face: Option<u8>) {
        self.ae2_provider_refresh(address,face,Ae2ProviderRefreshCause::PrioritySetter);
    }
    pub(super) fn ae2_provider_patterns_changed(&self, address: GridAddress, face: Option<u8>) {
        self.ae2_provider_refresh(address,face,Ae2ProviderRefreshCause::PatternsChanged);
    }
    /// Convenience for a commit in the active level, including an invalid
    /// coordinate which must be reported rather than silently ignored.
    fn ae2_provider_refresh_local(&self, pos: (i32,i32,i32), face: Option<u8>, cause: Ae2ProviderRefreshCause) {
        match self.ae2_local_address(pos) {
            Some(address) => self.ae2_provider_refresh(address, face, cause),
            None => self.ae2_provider_events_fault("invalid provider refresh address".into()),
        }
    }
    pub(super) fn ae2_provider_priority_setter_local(&self, pos: (i32,i32,i32), face: Option<u8>) {
        self.ae2_provider_refresh_local(pos,face,Ae2ProviderRefreshCause::PrioritySetter);
    }
    pub(super) fn ae2_provider_patterns_changed_local(&self, pos: (i32,i32,i32), face: Option<u8>) {
        self.ae2_provider_refresh_local(pos,face,Ae2ProviderRefreshCause::PatternsChanged);
    }
    fn ae2_capture_provider_refresh(&self, address: GridAddress, face: Option<u8>, cause: Ae2ProviderRefreshCause) -> Result<(), String> {
        let (dim, pos) = self.ae2_address_parts(address).ok_or("invalid provider address")?;
        let world = self.ae2_world_for(dim).ok_or("provider level unavailable")?;
        let incarnation = world.block_entities.incarnation(&pos).ok_or("provider incarnation unavailable")?;
        let Some(BlockEntity::Ae2(host)) = world.block_entities.get(&pos) else {
            return Err("provider host unavailable".into());
        };
        let (node, snapshot) = match face {
            None => {
                if host.kind != MachineKind::PatternProvider { return Err("provider block changed".into()); }
                (NodeIdentity {host:incarnation,part:None,outer:false},
                    Ae2ProviderSnapshot::Block {slot_count:host.slots.len(),raw_slots:save::encode_items(&host.slots),
                        raw_priority:host.config.get(&"priority".into()).cloned()})
            }
            Some(face) => {
                let face_name = ["east","west","up","down","south","north"]
                    .get(usize::from(face)).ok_or("invalid provider face")?;
                if host.kind != MachineKind::CableBus { return Err("provider part host changed".into()); }
                let Some(Tag::List(parts)) = host.config.get(&"parts".into()) else { return Err("provider part list unavailable".into()); };
                let mut matches = parts.iter().filter(|part|part.get("face")==Some(&Tag::String((*face_name).into())));
                let part = matches.next().ok_or("provider part unavailable")?;
                if matches.next().is_some() || part.get("id") != Some(&Tag::String("ae2:cable_pattern_provider".into())) {
                    return Err("provider part changed or duplicated".into());
                }
                let identity = super::ae2_channels::part_identity(part).ok_or("provider part identity unavailable")?;
                (NodeIdentity {host:incarnation,part:Some(identity),outer:false},
                    Ae2ProviderSnapshot::Part {raw_machine:part.get("machine").cloned()})
            }
        };
        world.block_entities.record_ae2_provider_refresh(pos,node,face,cause,snapshot).map_err(str::to_owned)?;
        // A bounded, opt-in diagnostic of successful journal commits. This
        // is observational only and does not consume or mutate the journal.
        if std::env::var_os("HORIZON_AE2_PROVIDER_JOURNAL_TRACE").is_some() {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static EVENTS: AtomicUsize = AtomicUsize::new(0);
            let number = EVENTS.fetch_add(1, Ordering::Relaxed);
            if number < 4096 {
                let reason = match cause { Ae2ProviderRefreshCause::PrioritySetter => "priority", Ae2ProviderRefreshCause::PatternsChanged => "patterns" };
                log::info!("AE2 provider journal n={} dim={} x={} y={} z={} face={} cause={} host={} part={:?}",
                    number, super::ae2_event_live::ordinal(dim), pos.0, pos.1, pos.2,
                    face.map_or(-1, i32::from), reason, node.host, node.part);
            } else if number == 4096 {
                log::warn!("AE2 provider journal trace limit reached");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ae2_crafting_core::provider_registry::PriorityGroup;
    use ae2_network_core::{Device, DeviceKind, Pos};
    use horizon_client::{ae2_event_replay::EventReplay, ae2_world::Machine, block_entities::{Ae2Mutation, BlockEntities}};

    fn fake_patterns(snapshot: &Ae2ProviderSnapshot) -> Result<Vec<Definition<String, String>>, String> {
        Ok(decode_snapshot(snapshot)?.slots.into_iter().flatten().map(|stack| Definition {
            key: stack.item.to_string(), primary_output: "test:output".into(),
        }).collect())
    }
    fn test_device(machine: &Machine, _: u32) -> Option<Device> {
        let mut device = Device::new(match machine.kind {
            MachineKind::CableBus => DeviceKind::GlassCable,
            MachineKind::PatternProvider => DeviceKind::Interface,
            MachineKind::CreativeEnergyCell => DeviceKind::CreativeEnergyCell,
            kind if cpu_kind(kind).is_some() => DeviceKind::CraftingCpu,
            _ => return None,
        });
        if matches!(machine.config.get(&"parts".into()), Some(Tag::List(parts)) if parts.iter().any(|part| part.get("face") == Some(&Tag::String("east".into())))) {
            device.faces &= !1;
        }
        Some(device)
    }
    fn test_encode(_: i32, p: (i32, i32, i32)) -> Option<Pos> { Some(Pos { x: p.0, y: p.1, z: p.2 }) }
    fn apply(replay: &mut EventReplay, observer: &mut ProviderObserver<String, String>, events: Vec<Ae2Mutation>) {
        replay.batch_observed_traced(events.into_iter().map(|event| (0, event)), test_encode, test_device,
            |_| true, |_, _| Ok(()), |topology, checkpoint| observer.observe(topology, checkpoint, &mut fake_patterns, &mut emitter_definitions)).unwrap();
    }
    fn ready(replay: &mut EventReplay, observer: &mut ProviderObserver<String, String>, chunk: (i32, i32)) {
        replay.ready_chunk_traced(0, chunk, test_device,
            |topology, checkpoint| observer.observe(topology, checkpoint, &mut fake_patterns, &mut emitter_definitions)).unwrap();
    }
    fn snapshot(item: &str, priority: i32) -> Ae2ProviderSnapshot {
        let mut slots = vec![None; 9];
        slots[0] = Some(ItemStack::new(item, 1));
        Ae2ProviderSnapshot::Block { slot_count: 9, raw_slots: save::encode_items(&slots), raw_priority: Some(Tag::Int(priority)) }
    }

    #[test]
    fn cpu_observer_faces_follow_formation_and_deliver_nested_membership() {
        let mut entities = BlockEntities::default(); entities.start_ae2_journal();
        let power=(0,64,0);let unit=(1,64,0);let storage=(2,64,0);
        for (pos,kind) in [(power,MachineKind::CreativeEnergyCell),(unit,MachineKind::CraftingUnit)] {
            entities.state_changed(pos,0,1);entities.insert(pos,BlockEntity::Ae2(Box::new(Machine::new(kind))));
        }
        let node = |pos| NodeIdentity {host:entities.incarnation(&pos).unwrap(),part:None,outer:false};
        let power_id=node(power);let unit_id=node(unit);
        let mut replay=EventReplay::default();replay.topology.services.start_membership_recording().unwrap();
        let mut observer=ProviderObserver::<String,String>::default();observer.enable_cpu();
        let mut delivered=Vec::new();
        let mut trace=|topology:&mut PhysicalTopology,checkpoint:ReplayCheckpoint<'_>| {
            observer.observe_with_membership(topology,checkpoint,&mut fake_patterns,&mut emitter_definitions,&mut |change| delivered.push(change));
        };
        replay.batch_observed_traced(entities.take_ae2_mutations().unwrap().into_iter().map(|event|(0,event)),test_encode,test_device,
            |_|true,|_,_|Ok(()),&mut trace).unwrap();
        replay.ready_chunk_traced(0,(0,0),test_device,&mut trace).unwrap();
        assert_eq!(replay.topology.node_faces(unit_id),Some(0));
        assert_ne!(replay.topology.services.service_id(power_id),replay.topology.services.service_id(unit_id));

        entities.state_changed(storage,0,1);entities.insert(storage,BlockEntity::Ae2(Box::new(Machine::new(MachineKind::CraftingStorage1k))));
        let storage_id=NodeIdentity {host:entities.incarnation(&storage).unwrap(),part:None,outer:false};
        entities.record_ae2_cpu_neighbor(unit,Some(storage));
        replay.batch_observed_traced(entities.take_ae2_mutations().unwrap().into_iter().map(|event|(0,event)),test_encode,test_device,
            |_|true,|_,_|Ok(()),&mut trace).unwrap();
        assert_eq!(replay.topology.node_faces(unit_id),Some(63));
        assert_eq!(replay.topology.services.service_id(power_id),replay.topology.services.service_id(unit_id));
        assert_eq!(replay.topology.services.service_id(storage_id),None);
        replay.ready_chunk_traced(0,(0,0),test_device,&mut trace).unwrap();
        assert_eq!(replay.topology.node_faces(storage_id),Some(63));
        assert_eq!(replay.topology.services.service_id(storage_id),replay.topology.services.service_id(unit_id));

        entities.state_changed(unit,1,2);
        replay.batch_observed_traced(entities.take_ae2_mutations().unwrap().into_iter().map(|event|(0,event)),test_encode,test_device,
            |_|true,|_,_|Ok(()),&mut trace).unwrap();
        assert_eq!(replay.topology.node_faces(unit_id),Some(63)); // preserve formed mask across visual state
        entities.remove(&storage);
        replay.batch_observed_traced(entities.take_ae2_mutations().unwrap().into_iter().map(|event|(0,event)),test_encode,test_device,
            |_|true,|_,_|Ok(()),&mut trace).unwrap();
        drop(trace);
        assert_eq!(replay.topology.node_faces(unit_id),Some(0));
        assert_ne!(replay.topology.services.service_id(power_id),replay.topology.services.service_id(unit_id));
        assert!(delivered.iter().all(Result::is_ok));
        assert!(delivered.iter().filter_map(|change|change.as_ref().ok()).any(|change|
            (change.node==unit_id||change.node==power_id)&&change.from.is_some()&&change.to.is_some()),
            "source tie handling may move either endpoint: {delivered:?}");
        assert_eq!(observer.failure(),None);assert_eq!(observer.cpu_failure(),None);
        assert_eq!(observer.cpu().unwrap().ready_network_nodes().unwrap(),vec![(unit_id,CpuPos{dimension:0,x:1,y:64,z:0},None)]);
    }

    #[test]
    fn cpu_observer_and_membership_delivery_survive_provider_snapshot_fault() {
        let mut entities=BlockEntities::default();entities.start_ae2_journal();
        let pos=(0,64,0);entities.state_changed(pos,0,1);
        entities.insert(pos,BlockEntity::Ae2(Box::new(Machine::new(MachineKind::CraftingStorage1k))));
        let node=NodeIdentity{host:entities.incarnation(&pos).unwrap(),part:None,outer:false};
        let mut replay=EventReplay::default();replay.topology.services.start_membership_recording().unwrap();
        let mut observer=ProviderObserver::<String,String>::default();observer.enable_cpu();
        observer.invalidate("bad provider snapshot".into());
        let mut delivered=Vec::new();
        let mut trace=|topology:&mut PhysicalTopology,checkpoint:ReplayCheckpoint<'_>| {
            observer.observe_with_membership(topology,checkpoint,&mut fake_patterns,&mut emitter_definitions,&mut |change|delivered.push(change));
        };
        replay.batch_observed_traced(entities.take_ae2_mutations().unwrap().into_iter().map(|event|(0,event)),test_encode,test_device,
            |_|true,|_,_|Ok(()),&mut trace).unwrap();
        replay.ready_chunk_traced(0,(0,0),test_device,&mut trace).unwrap();drop(trace);
        assert_eq!(observer.failure(),Some("bad provider snapshot"));assert_eq!(observer.cpu_failure(),None);
        assert!(observer.cpu().unwrap().cluster_of(&node).unwrap().is_some());
        assert!(delivered.iter().filter_map(|change|change.as_ref().ok()).any(|change|change.node==node));
        assert!(delivered.iter().all(Result::is_ok));
    }

    #[test]
    fn cpu_observer_replays_actual_ready_neighbor_and_membership_boundaries() {
        let mut entities = BlockEntities::default(); entities.start_ae2_journal();
        let a = (15, 64, 0); let b = (16, 64, 0);
        entities.state_changed(a, 0, 1);
        entities.insert(a, BlockEntity::Ae2(Box::new(Machine::new(MachineKind::CraftingStorage1k))));
        entities.state_changed(b, 0, 1);
        entities.insert(b, BlockEntity::Ae2(Box::new(Machine::new(MachineKind::CraftingAccelerator))));
        let aid = NodeIdentity { host: entities.incarnation(&a).unwrap(), part: None, outer: false };
        let bid = NodeIdentity { host: entities.incarnation(&b).unwrap(), part: None, outer: false };
        let mut replay = EventReplay::default(); replay.topology.services.start_membership_recording().unwrap();
        let mut observer = ProviderObserver::<String, String>::default(); observer.enable_cpu();
        apply(&mut replay, &mut observer, entities.take_ae2_mutations().unwrap());
        ready(&mut replay, &mut observer, (0, 0));
        let service = replay.topology.services.service_id(aid).unwrap();
        assert_eq!(replay.topology.services.service_id(bid), None);
        let cluster = observer.cpu().unwrap().cluster_of(&aid).unwrap().unwrap().clone();
        assert_eq!(cluster.members, vec![bid, aid]);
        assert_eq!((cluster.core, cluster.storage, cluster.coprocessors), (aid, 1024, 1));
        assert_eq!(observer.cpu().unwrap().cluster_of(&bid).unwrap().unwrap().id, cluster.id);
        assert!(observer.cpu().unwrap().published(service).unwrap().is_empty());
        observer.cpu_mut().unwrap().service_tick(service).unwrap();
        assert_eq!(observer.cpu().unwrap().published(service).unwrap(), vec![cluster.id]);

        ready(&mut replay, &mut observer, (1, 0));
        assert_eq!(observer.cpu().unwrap().cluster_of(&aid).unwrap().unwrap().id, cluster.id);
        let service = replay.topology.services.service_id(aid).unwrap();
        assert_eq!(replay.topology.services.service_id(bid), Some(service));
        for service in observer.cpu().unwrap().dirty_services().unwrap() {
            observer.cpu_mut().unwrap().service_tick(service).unwrap();
        }
        assert_eq!(observer.cpu().unwrap().published(service).unwrap(), vec![cluster.id]);

        entities.remove(&b);
        entities.record_ae2_cpu_neighbor(a, Some(b));
        apply(&mut replay, &mut observer, entities.take_ae2_mutations().unwrap());
        let broken=observer.cpu_break_for_dimension(0).unwrap().expect("real CPU break recorded");
        assert_eq!(broken.removed,Some(bid));
        assert!(!broken.removed_owned_job);
        assert_eq!(broken.members.iter().map(|(id, pos, resident)|(*id,(pos.x,pos.y,pos.z),*resident)).collect::<Vec<_>>(),
            vec![(bid,b,true),(aid,a,true)]);
        observer.acknowledge_cpu_break(broken.sequence).unwrap();
        assert!(observer.cpu_break_for_dimension(0).unwrap().is_none());
        let replacement = observer.cpu().unwrap().cluster_of(&aid).unwrap().unwrap();
        assert_ne!(replacement.id, cluster.id);
        assert_eq!(replacement.members, vec![aid]);
        assert_eq!(observer.failure(), None);
        assert_eq!(observer.cpu_failure(), None);
    }

    #[test]
    fn cpu_observer_unload_keeps_validated_waiting_metadata_until_rebind() {
        for case in ["idle","waiting","orphaned"] {
            let a=(15,64,0);let b=(16,64,0);
            let mut unit=Machine::new(MachineKind::CraftingUnit);
            if case=="waiting" {
                let mut registry=ae2_crafting_core::Registry::default();
                let output=horizon_client::item_data::canon_stack(Some(&ItemStack::new("minecraft:gold_ingot",1)));
                registry.register(ae2_crafting_core::Pattern{id:"p".into(),output:output.clone(),output_count:1,inputs:Inventory::from([("iron".into(),1)])}).unwrap();
                let mut available=Inventory::from([("iron".into(),1)]);
                let plan=registry.plan(&available,&output,1).unwrap();
                let mut job=ae2_crafting_core::Job::submit(plan,&mut available).unwrap();job.dispatch().unwrap();
                let raw=serde_json::json!({"job":job,"assembler":[0,64,1]}).to_string();
                unit.config.insert("ae2_cpu_job".into(),Tag::String(raw.as_str().into()));
            } else if case=="orphaned" {
                unit.config.insert("ae2_cpu_escrow".into(),Tag::List(vec![save::item_to_nbt(&ItemStack::new("minecraft:diamond",1))]));
            }
            let mut entities=BlockEntities::default();entities.start_ae2_journal();
            for (pos,machine) in [(a,unit),(b,Machine::new(MachineKind::CraftingStorage1k))] {
                entities.state_changed(pos,0,1);entities.insert(pos,BlockEntity::Ae2(Box::new(machine)));
            }
            let aid=NodeIdentity{host:entities.incarnation(&a).unwrap(),part:None,outer:false};
            let mut replay=EventReplay::default();replay.topology.services.start_membership_recording().unwrap();
            let mut observer=ProviderObserver::<String,String>::default();observer.enable_cpu();
            apply(&mut replay,&mut observer,entities.take_ae2_mutations().unwrap());
            ready(&mut replay,&mut observer,(0,0));ready(&mut replay,&mut observer,(1,0));
            entities.retain(|pos,_|*pos!=a);
            apply(&mut replay,&mut observer,entities.take_ae2_mutations().unwrap());
            match case {
                "idle"=>assert_eq!(observer.unloaded_cpu_waiting(aid).unwrap(),None),
                "waiting"=>assert_eq!(observer.unloaded_cpu_waiting(aid).unwrap(),Some(Inventory::from([(horizon_client::item_data::canon_stack(Some(&ItemStack::new("minecraft:gold_ingot",1))),1)]))),
                _=>assert!(observer.unloaded_cpu_waiting(aid).is_err()),
            }
            entities.state_changed(a,1,1);entities.insert(a,BlockEntity::Ae2(Box::new(Machine::new(MachineKind::CraftingUnit))));
            apply(&mut replay,&mut observer,entities.take_ae2_mutations().unwrap());
            assert!(observer.unloaded_cpu_waiting(aid).is_err(),"old snapshot cannot survive a new incarnation");
            ready(&mut replay,&mut observer,(0,0));
            let notice=observer.cpu_break_for_dimension(0).unwrap().expect("rebind replaces old cluster");
            assert_eq!(notice.unloaded_idle.contains(&aid),case=="idle");
        }
    }

    #[test]
    fn cpu_observer_missing_neighbor_source_is_scoped_to_formed_cpu() {
        let mut entities = BlockEntities::default(); entities.start_ae2_journal();
        let pos = (0, 64, 0);
        entities.state_changed(pos, 0, 1);
        entities.insert(pos, BlockEntity::Ae2(Box::new(Machine::new(MachineKind::CraftingUnit))));
        let mut replay = EventReplay::default(); replay.topology.services.start_membership_recording().unwrap();
        let mut observer = ProviderObserver::<String, String>::default(); observer.enable_cpu();
        apply(&mut replay, &mut observer, entities.take_ae2_mutations().unwrap());
        entities.record_ae2_cpu_neighbor(pos, None);
        apply(&mut replay, &mut observer, entities.take_ae2_mutations().unwrap());
        assert_eq!(observer.cpu_failure(), None); // No cluster; source always recalculates.
        entities.remove(&pos);
        entities.state_changed(pos, 1, 1);
        entities.insert(pos, BlockEntity::Ae2(Box::new(Machine::new(MachineKind::CraftingStorage1k))));
        apply(&mut replay, &mut observer, entities.take_ae2_mutations().unwrap());
        ready(&mut replay, &mut observer, (0, 0));
        entities.record_ae2_cpu_neighbor(pos, None);
        apply(&mut replay, &mut observer, entities.take_ae2_mutations().unwrap());
        assert!(observer.cpu_failure().is_some_and(|e| e.contains("formed CPU neighbor source")));
        assert_eq!(observer.failure(), None);
    }

    #[test]
    fn cpu_observer_saved_core_uses_raw_source_and_isolates_bad_provenance() {
        use horizon_client::{chunk::Chunk, save::ChunkExtras};
        use std::sync::Arc;
        for case in ["core", "absent", "native_job", "malformed", "missing"] {
            let pos = (0, 64, 0);
            let mut machine=Machine::new(MachineKind::CraftingStorage1k);
            if case=="native_job" {machine.config.insert("ae2_recursive_job".into(),Tag::String("opaque-owned-job".into()));}
            let mut raw = save::encode_block_entity(pos, &BlockEntity::Ae2(Box::new(machine)), "ae2:1k_crafting_storage");
            let Tag::Compound(compound) = &mut raw else { panic!("CPU tag") };
            match case {
                "core" => { compound.insert("core".into(), Tag::Byte(1)); },
                "malformed" => { compound.insert("core".into(), Tag::String("bad".into())); },
                _ => {}
            }
            let (_, entity) = save::decode_block_entity(&raw).unwrap();
            let mut chunk = Chunk::empty(0, 0); chunk.set(0, 64, 0, 1);
            chunk.block_entities.push((pos, entity));
            chunk.saved = Some(Arc::new(ChunkExtras { block_entity_sources: if case == "missing" { vec![] } else { vec![(pos, raw)] }, ..Default::default() }));
            let mut entities = BlockEntities::default(); entities.start_ae2_journal();
            entities.record_ae2_chunk_states(&chunk); entities.load_chunk(std::mem::take(&mut chunk.block_entities));
            let mut replay = EventReplay::default(); replay.topology.services.start_membership_recording().unwrap();
            let mut observer = ProviderObserver::<String, String>::default(); observer.enable_cpu();
            apply(&mut replay, &mut observer, entities.take_ae2_mutations().unwrap());
            match case {
                "core" | "absent" | "native_job" => assert_eq!(observer.cpu_failure(), None),
                "malformed" => assert_eq!(observer.cpu_failure(), Some("saved CPU core flag malformed")),
                _ => assert_eq!(observer.cpu_failure(), Some("saved CPU core source unavailable")),
            }
            assert_eq!(observer.failure(), None);
            if case=="native_job" {
                let Some(BlockEntity::Ae2(machine))=entities.get(&pos) else {panic!("CPU owner disappeared")};
                assert_eq!(machine.config.get(&"ae2_recursive_job".into()),Some(&Tag::String("opaque-owned-job".into())));
            }
        }
    }

    fn emitter(face: &str, identity: [i32; 4], item: &str) -> Tag {
        let mut card = save::item_to_nbt(&ItemStack::new("ae2:crafting_card", 1));
        let Tag::Compound(values) = &mut card else { unreachable!() };
        values.insert("Slot".into(), Tag::Int(0));
        Tag::compound([
            ("id", Tag::String("ae2:level_emitter".into())),
            ("face", Tag::String(face.into())),
            ("ae2_part_identity", Tag::IntArray(identity.to_vec())),
            ("upgrades", Tag::List(vec![card])),
            ("emitter_craft_via_redstone", Tag::Byte(1)),
            ("emitter_filter", save::item_to_nbt(&ItemStack::new(item, 1))),
        ])
    }

    #[test]
    fn emitter_observer_tracks_real_membership_refresh_and_stale_identity() {
        let mut entities = BlockEntities::default(); entities.start_ae2_journal();
        let pos = (0, 64, 0);
        let first = emitter("east", [1,2,3,4], "minecraft:iron_ingot");
        let second = emitter("north", [5,6,7,8], "minecraft:iron_ingot");
        let mut host = Machine::new(MachineKind::CableBus);
        host.config.insert("parts".into(), Tag::List(vec![first.clone(), second]));
        entities.state_changed(pos, 0, 1); entities.insert(pos, BlockEntity::Ae2(Box::new(host)));
        let incarnation = entities.incarnation(&pos).unwrap();
        let first_node = NodeIdentity { host:incarnation, part:Some([1,2,3,4]), outer:false };
        let second_node = NodeIdentity { host:incarnation, part:Some([5,6,7,8]), outer:false };
        let mut replay = EventReplay::default(); replay.topology.services.start_membership_recording().unwrap();
        let mut observer = ProviderObserver::<String, String>::default();
        apply(&mut replay, &mut observer, entities.take_ae2_mutations().unwrap()); ready(&mut replay, &mut observer, (0,0));
        let service = replay.topology.services.service_id(first_node).unwrap();
        assert_eq!(replay.topology.services.service_id(second_node), Some(service));
        let iron = emitter_definitions(&first).unwrap().remove(0);
        assert_eq!(observer.emittable.count(service, &iron), Ok(2));
        assert_eq!(observer.emittable_keys(service).unwrap(), vec![iron.clone()]);

        let changed = emitter("east", [1,2,3,4], "minecraft:gold_ingot");
        let gold = emitter_definitions(&changed).unwrap().remove(0);
        entities.record_ae2_emitter_provider_refresh(pos, first_node, 0, changed).unwrap();
        apply(&mut replay, &mut observer, entities.take_ae2_mutations().unwrap());
        assert_eq!(observer.emittable.count(service, &iron), Ok(1));
        assert_eq!(observer.emittable.count(service, &gold), Ok(1));

        // An observed event that names a different incarnation must not bind
        // that snapshot to the old face, and must not poison topology.
        let mut bad_replay = replay.clone(); let mut bad_observer = observer.clone();
        let stale = emitter("east", [9,9,9,9], "minecraft:diamond");
        entities.record_ae2_emitter_provider_refresh(pos, first_node, 0, stale).unwrap();
        apply(&mut bad_replay, &mut bad_observer, entities.take_ae2_mutations().unwrap());
        assert_eq!(bad_observer.failure(), Some("emitter refresh identity or location changed"));
        assert_eq!(bad_replay.topology.services.service_id(first_node), Some(service));

        entities.remove(&pos);
        apply(&mut replay, &mut observer, entities.take_ae2_mutations().unwrap());
        assert_eq!(observer.failure(), None);
        assert_eq!(observer.emittable.count(service, &iron), Ok(0));
        assert_eq!(observer.emittable.count(service, &gold), Ok(0));
    }

    #[test]
    fn emitter_definitions_require_crafting_card_setting_and_configured_key() {
        let active = emitter("east", [1,2,3,4], "minecraft:iron_ingot");
        assert_eq!(emitter_definitions(&active).unwrap().len(), 1);
        let Tag::Compound(mut data) = active else { unreachable!() };
        data.insert("emitter_craft_via_redstone".into(), Tag::Byte(0));
        assert!(emitter_definitions(&Tag::Compound(data.clone())).unwrap().is_empty());
        data.insert("emitter_craft_via_redstone".into(), Tag::Byte(1));
        data.insert("upgrades".into(), Tag::List(Vec::new()));
        assert!(emitter_definitions(&Tag::Compound(data.clone())).unwrap().is_empty());
        data.remove(&"emitter_filter".into());
        let mut card = save::item_to_nbt(&ItemStack::new("ae2:crafting_card", 1));
        let Tag::Compound(values) = &mut card else { unreachable!() }; values.insert("Slot".into(), Tag::Int(0));
        data.insert("upgrades".into(), Tag::List(vec![card]));
        assert!(emitter_definitions(&Tag::Compound(data.clone())).unwrap().is_empty());
        data.insert("emitter_fluid_filter".into(), Tag::compound([
            ("fluid", Tag::String("minecraft:water".into())), ("components", Tag::ByteArray(vec![1, 2]))
        ]));
        assert_eq!(emitter_definitions(&Tag::Compound(data)).unwrap(), vec!["fluid|minecraft:water|0102"]);
    }

    #[test]
    fn provider_observer_replays_ready_priority_pattern_and_removal() {
        let mut entities = BlockEntities::default(); entities.start_ae2_journal();
        let host_pos = (0, 64, 0); let block_pos = (-1, 64, 0);
        let mut part_slots = vec![None; 9]; part_slots[0] = Some(ItemStack::new("test:old", 1));
        let part = Tag::compound([
            ("id", Tag::String("ae2:cable_pattern_provider".into())), ("face", Tag::String("east".into())),
            ("ae2_part_identity", Tag::IntArray(vec![1, 2, 3, 4])),
            ("machine", Tag::compound([("slots", save::encode_items(&part_slots)), ("config", Tag::compound([("priority", Tag::Int(1))]))])),
        ]);
        let mut host = Machine::new(MachineKind::CableBus); host.config.insert("parts".into(), Tag::List(vec![part]));
        entities.state_changed(host_pos, 0, 1); entities.insert(host_pos, BlockEntity::Ae2(Box::new(host)));
        let mut block = Machine::new(MachineKind::PatternProvider); block.slots[0] = Some(ItemStack::new("test:old", 1));
        block.config.insert("priority".into(), Tag::Int(10));
        entities.state_changed(block_pos, 0, 1); entities.insert(block_pos, BlockEntity::Ae2(Box::new(block)));
        let part_id = NodeIdentity { host: entities.incarnation(&host_pos).unwrap(), part: Some([1,2,3,4]), outer: false };
        let block_id = NodeIdentity { host: entities.incarnation(&block_pos).unwrap(), part: None, outer: false };
        let mut replay = EventReplay::default(); replay.topology.services.start_membership_recording().unwrap();
        let mut observer = ProviderObserver::<String, String>::default();
        apply(&mut replay, &mut observer, entities.take_ae2_mutations().unwrap());
        ready(&mut replay, &mut observer, (0, 0)); ready(&mut replay, &mut observer, (-1, 0));
        assert_eq!(observer.failure, None);
        let service = replay.topology.services.service_id(block_id).unwrap();
        assert_eq!(replay.topology.services.service_id(part_id), Some(service));
        let summary = observer.summary(service, "test:output").unwrap();
        assert_eq!(summary["groups"][0]["priority"], 10);
        assert_eq!(summary["groups"][0]["definitions"][0]["media"][0]["host"], part_id.host);
        assert_eq!(summary["groups"][0]["definitions"][0]["media"][1]["host"], block_id.host);
        let registry = observer.lifecycle.service_mut(service).unwrap().unwrap();
        assert_eq!(registry.ranked_definitions(&"test:output".into()), vec![PriorityGroup { priority: 10, definitions: vec!["test:old".into()] }]);
        let mut pass = registry.media_pass(&"test:old".into()).unwrap();
        assert_eq!(registry.next_medium(&mut pass).unwrap(), Some(part_id));
        assert_eq!(registry.next_medium(&mut pass).unwrap(), Some(block_id));

        // A direct NBT read changes physical slots and current priority while
        // leaving the mounted entry and effective pattern cache untouched.
        entities.record_ae2_provider_raw_read(block_pos, block_id, snapshot("test:new", -10)).unwrap();
        apply(&mut replay, &mut observer, entities.take_ae2_mutations().unwrap());
        assert_eq!(observer.lifecycle.service(service).unwrap().unwrap().ranked_definitions(&"test:output".into()),
            vec![PriorityGroup { priority: 10, definitions: vec!["test:old".into()] }]);
        // Planning is read-only and resolves the effective cached definition,
        // which differs from the direct raw read above.
        let plan = observer.planning_definitions(service).unwrap();
        assert_eq!(plan.len(), 1); assert_eq!(plan[0].key, "test:old");
        assert_eq!(observer.planning_definitions(service).unwrap()[0].key, "test:old");
        let mut actual = observer.media_pass(service, "test:old").unwrap().unwrap();
        let medium = observer.next_medium(service, &mut actual).unwrap().unwrap();
        assert_eq!(medium.node, part_id);
        assert!(observer.contains_medium(service, medium.node, "test:old", &actual).unwrap());
        // The subsequent setter remounts the old effective definition; only
        // a pattern callback publishes the newly read physical slot.
        entities.record_ae2_provider_refresh(block_pos, block_id, None, Ae2ProviderRefreshCause::PrioritySetter, snapshot("test:new", -10)).unwrap();
        apply(&mut replay, &mut observer, entities.take_ae2_mutations().unwrap());
        assert_eq!(observer.lifecycle.service(service).unwrap().unwrap().ranked_definitions(&"test:output".into()),
            vec![PriorityGroup { priority: 1, definitions: vec!["test:old".into()] }]);
        assert!(observer.contains_medium(service, medium.node, "test:old", &actual).is_err());
        entities.record_ae2_provider_refresh(block_pos, block_id, None, Ae2ProviderRefreshCause::PatternsChanged, snapshot("test:new", -10)).unwrap();
        apply(&mut replay, &mut observer, entities.take_ae2_mutations().unwrap());
        assert_eq!(observer.lifecycle.service(service).unwrap().unwrap().ranked_definitions(&"test:output".into()), vec![
            PriorityGroup { priority: 1, definitions: vec!["test:old".into()] },
            PriorityGroup { priority: -10, definitions: vec!["test:new".into()] },
        ]);
        entities.remove(&block_pos);
        apply(&mut replay, &mut observer, entities.take_ae2_mutations().unwrap());
        assert_eq!(observer.failure, None);
        assert!(!observer.physical.contains_key(&block_id));
        assert_eq!(observer.lifecycle.service(service).unwrap().unwrap().ranked_definitions(&"test:output".into()),
            vec![PriorityGroup { priority: 1, definitions: vec!["test:old".into()] }]);
    }

    #[test]
    fn provider_observer_invalid_snapshot_does_not_poison_topology() {
        let mut entities = BlockEntities::default(); entities.start_ae2_journal();
        let pos = (0, 64, 0); entities.state_changed(pos, 0, 1);
        entities.insert(pos, BlockEntity::Ae2(Box::new(Machine::new(MachineKind::PatternProvider))));
        let node = NodeIdentity { host: entities.incarnation(&pos).unwrap(), part: None, outer: false };
        let mut replay = EventReplay::default(); replay.topology.services.start_membership_recording().unwrap();
        let mut observer = ProviderObserver::<String, String>::default();
        apply(&mut replay, &mut observer, entities.take_ae2_mutations().unwrap()); ready(&mut replay, &mut observer, (0, 0));
        let service = replay.topology.services.service_id(node);
        entities.record_ae2_provider_refresh(pos, node, None, Ae2ProviderRefreshCause::PrioritySetter, snapshot("test:new", 4)).unwrap();
        let mut events = entities.take_ae2_mutations().unwrap(); events[0].position = (2, 64, 0);
        apply(&mut replay, &mut observer, events);
        assert_eq!(observer.failure.as_deref(), Some("provider refresh location changed"));
        assert!(observer.lifecycle.service(service.unwrap()).is_err());
        assert_eq!(replay.topology.services.service_id(node), service);
    }

    #[test]
    fn provider_observer_bulk_unload_tracks_surviving_provider_split() {
        let mut entities = BlockEntities::default(); entities.start_ae2_journal();
        let cable_pos = (0, 64, 0); let left_pos = (-1, 64, 0); let right_pos = (1, 64, 0);
        for (pos, kind) in [(cable_pos, MachineKind::CableBus), (left_pos, MachineKind::PatternProvider), (right_pos, MachineKind::PatternProvider)] {
            let mut machine = Machine::new(kind);
            if kind == MachineKind::PatternProvider { machine.slots[0] = Some(ItemStack::new("test:old", 1)); }
            entities.state_changed(pos, 0, 1); entities.insert(pos, BlockEntity::Ae2(Box::new(machine)));
        }
        let provider = |pos| NodeIdentity { host: entities.incarnation(&pos).unwrap(), part: None, outer: false };
        let left = provider(left_pos); let right = provider(right_pos);
        let mut replay = EventReplay::default(); replay.topology.services.start_membership_recording().unwrap();
        let mut observer = ProviderObserver::<String, String>::default();
        apply(&mut replay, &mut observer, entities.take_ae2_mutations().unwrap());
        ready(&mut replay, &mut observer, (0, 0)); ready(&mut replay, &mut observer, (-1, 0));
        let original = replay.topology.services.service_id(left).unwrap();
        assert_eq!(replay.topology.services.service_id(right), Some(original));
        entities.retain(|pos, _| *pos != cable_pos);
        apply(&mut replay, &mut observer, entities.take_ae2_mutations().unwrap());
        assert_eq!(observer.failure, None);
        let left_service = replay.topology.services.service_id(left).unwrap();
        let right_service = replay.topology.services.service_id(right).unwrap();
        assert_ne!(left_service, right_service);
        assert!(left_service != original || right_service != original);
        for (node, service) in [(left, left_service), (right, right_service)] {
            let registry = observer.lifecycle.service_mut(service).unwrap().unwrap();
            let mut pass = registry.media_pass(&"test:old".into()).unwrap();
            assert_eq!(registry.next_medium(&mut pass).unwrap(), Some(node));
            assert_eq!(registry.next_medium(&mut pass).unwrap(), None);
        }
    }

    #[test]
    fn provider_observer_loaded_source_preserves_and_validates_raw_inventory() {
        use horizon_client::{chunk::Chunk, save::ChunkExtras};
        use std::sync::Arc;
        for case in ["valid", "absent", "malformed", "missing"] {
            let pos = (0, 64, 0);
            let mut machine = Machine::new(MachineKind::PatternProvider);
            machine.slots[0] = Some(ItemStack::new("test:old", 1));
            let mut raw = save::encode_block_entity(pos, &BlockEntity::Ae2(Box::new(machine)), "ae2:pattern_provider");
            if case == "absent" {
                let Tag::Compound(compound) = &mut raw else { panic!("provider tag") };
                compound.remove(&"Items".into());
            } else if case == "malformed" {
                let Tag::Compound(compound) = &mut raw else { panic!("provider tag") };
                compound.insert("Items".into(), Tag::List(vec![Tag::compound([
                    ("Slot", Tag::Byte(0)), ("id", Tag::String("test:old".into())),
                    ("count", Tag::Int(1)), ("components", Tag::Int(7)),
                ])]));
            }
            // Exercise the generic loader that normally normalizes bad data.
            let (_, entity) = save::decode_block_entity(&raw).unwrap();
            let mut chunk = Chunk::empty(0, 0); chunk.set(0, 64, 0, 1);
            chunk.block_entities.push((pos, entity));
            chunk.saved = Some(Arc::new(ChunkExtras { block_entity_sources: if case == "missing" { vec![] } else { vec![(pos, raw)] }, ..Default::default() }));
            let mut entities = BlockEntities::default(); entities.start_ae2_journal();
            entities.record_ae2_chunk_states(&chunk);
            entities.load_chunk(std::mem::take(&mut chunk.block_entities));
            let node = NodeIdentity { host: entities.incarnation(&pos).unwrap(), part: None, outer: false };
            let mut replay = EventReplay::default(); replay.topology.services.start_membership_recording().unwrap();
            let mut observer = ProviderObserver::<String, String>::default();
            apply(&mut replay, &mut observer, entities.take_ae2_mutations().unwrap());
            ready(&mut replay, &mut observer, (0, 0));
            assert!(replay.topology.services.service_id(node).is_some());
            match case {
                "valid" => {
                    assert_eq!(observer.failure, None);
                    let service = replay.topology.services.service_id(node).unwrap();
                    assert_eq!(observer.lifecycle.service(service).unwrap().unwrap().ranked_definitions(&"test:output".into()),
                        vec![PriorityGroup { priority: 0, definitions: vec!["test:old".into()] }]);
                }
                "absent" => {
                    assert_eq!(observer.failure, None);
                    let service = replay.topology.services.service_id(node).unwrap();
                    assert!(observer.lifecycle.service(service).unwrap().unwrap().ranked_definitions(&"test:output".into()).is_empty());
                }
                "malformed" => assert_eq!(observer.failure.as_deref(), Some("malformed provider item components")),
                _ => assert_eq!(observer.failure.as_deref(), Some("saved provider inventory source unavailable")),
            }
        }
    }

    #[test]
    fn provider_observer_node_data_comparison_ignores_only_player_id() {
        let a = Tag::compound([("p", Tag::Int(1)), ("cm", Tag::String("X2".into()))]);
        let owner_change = Tag::compound([("p", Tag::Int(9)), ("cm", Tag::String("X2".into()))]);
        let mode_change = Tag::compound([("p", Tag::Int(1)), ("cm", Tag::String("X3".into()))]);
        assert!(provider_node_data_unchanged(Some(&a), Some(&owner_change)));
        assert!(!provider_node_data_unchanged(Some(&a), Some(&mode_change)));
        assert!(provider_node_data_unchanged(None, Some(&Tag::compound([("p", Tag::Int(1))]))));
        assert!(provider_node_data_unchanged(None, Some(&Tag::Int(1))));
    }

    #[test]
    fn provider_observer_partial_raw_inventory_read_preserves_unlisted_slots() {
        let mut previous = vec![None; 9];
        previous[0] = Some(ItemStack::new("test:first", 1));
        previous[1] = Some(ItemStack::new("test:second", 1));
        let mut changed = vec![None; 9]; changed[1] = Some(ItemStack::new("test:new", 1));
        let actual = provider_merge_raw_slots(&previous, Some(&save::encode_items(&changed)));
        assert_eq!(actual[0], previous[0]);
        assert_eq!(actual[1], changed[1]);
        assert_eq!(provider_merge_raw_slots(&previous, None), previous);
        assert_eq!(provider_merge_raw_slots(&previous, Some(&Tag::List(Vec::new()))), previous);
        assert_eq!(provider_merge_raw_slots(&previous, Some(&Tag::Int(3))), previous);
    }

    #[test]
    fn provider_observer_accepts_encoded_processing_pattern_components() {
        let pattern = super::super::ae2_processing::ae2_encode_processing_pattern(
            &[Some(ItemStack::new("minecraft:iron_ore", 1))],
            &[Some(ItemStack::new("minecraft:iron_ingot", 1))]).unwrap();
        let mut slots = vec![None; 9]; slots[0] = Some(pattern);
        let data = Ae2ProviderSnapshot::Block { slot_count: 9, raw_slots: save::encode_items(&slots), raw_priority: None };
        assert_eq!(decode_snapshot(&data).unwrap().slots, slots);
    }

    #[test]
    fn provider_indexes_resolved_recipe_output_not_stale_encoded_display() {
        let mut grid=vec![None;9];grid[0]=Some(ItemStack::new("minecraft:oak_log",1));
        let encoded=ItemStack::new("minecraft:oak_planks",4);
        let pattern=super::super::ae2_processing::ae2_encode_pattern_source(&grid,&encoded,"example:changed",false,false).unwrap();
        let mut slots=vec![None;9];slots[0]=Some(pattern);
        let snapshot=Ae2ProviderSnapshot::Block {slot_count:9,raw_slots:save::encode_items(&slots),raw_priority:None};
        let expected=ItemStack::new("minecraft:diamond",1);
        let definitions=provider_definitions(&snapshot,|pattern,_,_| {
            assert_eq!(super::super::ae2_source_patterns::crafting(pattern).unwrap().unwrap().recipe,"example:changed");
            Some(expected.clone())
        },|_,special|Some(special.output.clone()),|_|true).unwrap();
        assert_eq!(definitions.len(),1);
        assert_eq!(definitions[0].primary_output,horizon_client::item_data::canon_stack(Some(&expected)));
    }
    #[test]
    fn provider_observer_decodes_definition_identity_and_primary_output() {
        let pattern = |input: &str| super::super::ae2_processing::ae2_encode_processing_pattern(
            &[Some(ItemStack::new(input, 1))], &[Some(ItemStack::new("minecraft:diamond", 1))]).unwrap();
        let first = pattern("minecraft:iron_ingot");
        let mut duplicate = first.clone(); duplicate.count = 2;
        let second = pattern("minecraft:gold_ingot");
        let mut grid = vec![None; 9]; grid[0] = Some(ItemStack::new("minecraft:oak_planks", 1));
        let crafting = super::super::ae2_processing::ae2_encode_pattern(&grid, &ItemStack::new("minecraft:stick", 4)).unwrap();
        let mut slots = vec![None; 9];
        slots[0] = Some(first); slots[1] = Some(duplicate); slots[2] = Some(second);
        slots[3] = Some(ItemStack::new("minecraft:stone", 1)); slots[4] = Some(crafting);
        let snapshot = Ae2ProviderSnapshot::Block { slot_count: 9, raw_slots: save::encode_items(&slots), raw_priority: None };
        let valid = provider_definitions(&snapshot, |_,_,output| Some(output.clone()), |_,special| Some(special.output.clone()), |_| true).unwrap();
        assert_eq!(valid.len(), 4);
        assert_eq!(valid[0].key, valid[1].key);
        assert_ne!(valid[0].key, valid[2].key);
        assert_eq!(valid[0].primary_output, valid[2].primary_output);
        let mut output = ItemStack::new("minecraft:diamond", 1); output.count = 1;
        assert_eq!(valid[0].primary_output, horizon_client::item_data::canon_stack(Some(&output)));
        assert_eq!(provider_definitions(&snapshot, |_,_,_| None, |_,special| Some(special.output.clone()), |_| true).unwrap().len(), 3);
    }

    #[test]
    fn provider_observer_keeps_bare_encoded_items_but_skips_undecodable_definitions() {
        let mut slots=vec![None;9];
        for (slot,id) in ["ae2:crafting_pattern","ae2:processing_pattern","ae2:smithing_table_pattern","ae2:stonecutting_pattern"].into_iter().enumerate() {
            slots[slot]=Some(ItemStack::new(id,1));
        }
        slots[4]=Some(super::super::ae2_processing::ae2_encode_processing_pattern(
            &[Some(ItemStack::new("minecraft:iron_ingot",1))], &[Some(ItemStack::new("minecraft:diamond",1))]).unwrap());
        let snapshot=Ae2ProviderSnapshot::Block {slot_count:9,raw_slots:save::encode_items(&slots),raw_priority:None};
        assert_eq!(decode_snapshot(&snapshot).unwrap().slots,slots);
        let definitions=provider_definitions(&snapshot,|_,_,output|Some(output.clone()),|_,special|Some(special.output.clone()),|_|true).unwrap();
        assert_eq!(definitions.len(),1);
        assert_eq!(definitions[0].primary_output,horizon_client::item_data::canon_stack(Some(&ItemStack::new("minecraft:diamond",1))));
    }

    #[test]
    fn provider_observer_rejects_legacy_mixed_output_without_slot_order() {
        let fluid = super::super::ae2_processing::PatternFluid {
            key: ae2_cell_core::ItemKey { item_id: "minecraft:water".into(), components: vec![] }, amount: 1000,
        };
        let pattern = super::super::ae2_processing::ae2_encode_processing_with_fluids(
            &[Some(ItemStack::new("minecraft:iron_ingot", 1))],
            &[Some(ItemStack::new("minecraft:diamond", 1))], &[], &[fluid]).unwrap();
        let mut slots = vec![None; 9]; slots[0] = Some(pattern);
        let snapshot = Ae2ProviderSnapshot::Block { slot_count: 9, raw_slots: save::encode_items(&slots), raw_priority: None };
        assert_eq!(provider_definitions(&snapshot, |_,_,output| Some(output.clone()), |_,special| Some(special.output.clone()), |_| true), Err("ambiguous legacy mixed processing primary output".into()));
    }

    #[test]
    fn positioned_provider_definition_uses_fluid_primary() {
        let fluid = super::super::ae2_processing::PatternFluid {
            key: ae2_cell_core::ItemKey { item_id: "minecraft:water".into(), components: vec![1, 2] }, amount: 1000,
        };
        let mut inputs = vec![None; 81]; inputs[1] = Some(ItemStack::new("minecraft:iron_ingot", 1));
        let mut outputs = vec![None; 27]; outputs[4] = Some(ItemStack::new("minecraft:diamond", 1));
        let fluid_inputs = vec![None; 81]; let mut fluid_outputs = vec![None; 27]; fluid_outputs[0] = Some(fluid);
        let pattern = super::super::ae2_processing::ae2_encode_processing_positioned(&inputs, &outputs, &fluid_inputs, &fluid_outputs).unwrap();
        let mut slots = vec![None; 9]; slots[0] = Some(pattern);
        let snapshot = Ae2ProviderSnapshot::Block { slot_count: 9, raw_slots: save::encode_items(&slots), raw_priority: None };
        let definitions = provider_definitions(&snapshot, |_,_,output| Some(output.clone()), |_,special| Some(special.output.clone()), |_| true).unwrap();
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].primary_output, "fluid|minecraft:water|0102");
    }

    #[test]
    fn provider_snapshots_distinguish_missing_from_malformed_and_retain_patterns() {
        let mut slots=vec![None;9];slots[2]=Some(ItemStack::new("ae2:processing_pattern",1));
        let encoded=save::encode_items(&slots);
        let block=Ae2ProviderSnapshot::Block {slot_count:9,raw_slots:encoded.clone(),raw_priority:Some(Tag::Int(-7))};
        assert_eq!(decode_snapshot(&block).unwrap(),ProviderData {slots:slots.clone(),priority:-7});
        let part=Ae2ProviderSnapshot::Part {raw_machine:Some(Tag::compound([
            ("slots",encoded.clone()),("config",Tag::compound([("priority",Tag::Int(-7))]))
        ]))};
        assert_eq!(decode_snapshot(&part).unwrap(),ProviderData {slots,priority:-7});
        assert_eq!(decode_snapshot(&Ae2ProviderSnapshot::Part {raw_machine:None}).unwrap().priority,0);
        let bad_priority=Ae2ProviderSnapshot::Block {slot_count:9,raw_slots:encoded,raw_priority:Some(Tag::String("bad".into()))};
        assert_eq!(decode_snapshot(&bad_priority),Err("malformed provider priority".into()));
        assert_eq!(decode_snapshot(&Ae2ProviderSnapshot::Part {raw_machine:Some(Tag::Int(3))}),Err("malformed provider part machine".into()));
        assert_eq!(decode_snapshot(&Ae2ProviderSnapshot::Part {raw_machine:Some(Tag::compound([("config",Tag::Int(1))]))}),Err("malformed provider part config".into()));
        assert!(decode_snapshot(&Ae2ProviderSnapshot::Part {raw_machine:Some(Tag::compound([("slots",Tag::Int(1))]))}).is_err());
    }
}
