//! Ordered AE2 mutation replay. This bounded adapter currently supports plain
//! physical nodes, ordinary parts, quartz fiber and toggle outer nodes. It
//! rejects unsupported virtual/unknown mutations so callers can keep the
//! fallback instead of guessing service ownership.
use crate::{
    ae2_world::{Machine, MachineKind},
    block_entities::{Ae2Mutation, Ae2MutationKind},
    containers::BlockEntity,
};
use ae2_network_core::{
    ChannelMode, Device, NodeIdentity, Pos, physical_events::PhysicalTopology,
    ready_queue::ReadyQueue, service_events::GridEvent,
};
use std::collections::{BTreeMap, BTreeSet};

type Local = (i32, i32, i32);
use horizon_world::nbt::Tag;
fn part_face(part: &Tag) -> Option<u8> {
    let Tag::String(face) = part.get("face")? else {
        return None;
    };
    Some(match face.to_string().ok()?.as_str() {
        "east" => 0,
        "west" => 1,
        "up" => 2,
        "down" => 3,
        "south" => 4,
        "north" => 5,
        _ => return None,
    })
}
fn part_node(part: &Tag, host: u64) -> Result<Option<(NodeIdentity, Option<ChannelMode>)>, String> {
    let Some(Tag::String(id)) = part.get("id") else {
        return Err("part id unavailable".into());
    };
    let id = id.to_string().map_err(|_| "invalid part id")?;
    if id == "ae2:cable_anchor" {
        return Ok(None);
    }
    if !matches!(
        id.as_str(),
        "ae2:quartz_fiber"
            | "ae2:me_p2p_tunnel"
            | "ae2:toggle_bus"
            | "ae2:inverted_toggle_bus"
            | "ae2:terminal"
            | "ae2:crafting_terminal"
            | "ae2:pattern_encoding_terminal"
            | "ae2:pattern_access_terminal"
            | "ae2:import_bus"
            | "ae2:export_bus"
            | "ae2:storage_bus"
            | "ae2:level_emitter"
            | "ae2:energy_level_emitter"
            | "ae2:annihilation_plane"
            | "ae2:formation_plane"
            | "ae2:cable_energy_acceptor"
            | "ae2:monitor"
            | "ae2:semi_dark_monitor"
            | "ae2:dark_monitor"
            | "ae2:storage_monitor"
            | "ae2:conversion_monitor"
            | "ae2:cable_interface"
            | "ae2:cable_pattern_provider"
            | "ae2:item_p2p_tunnel"
            | "ae2:fluid_p2p_tunnel"
            | "ae2:fe_p2p_tunnel"
            | "ae2:redstone_p2p_tunnel"
            | "ae2:light_p2p_tunnel"
    ) {
        return Err("unsupported part topology".into());
    }
    let Some(Tag::IntArray(raw)) = part.get("ae2_part_identity") else {
        return Err("part identity unavailable".into());
    };
    let uuid: [i32; 4] = raw
        .as_slice()
        .try_into()
        .map_err(|_| "invalid part identity")?;
    if uuid == [0; 4] {
        return Err("invalid part identity".into());
    }
    let mode = match part.get("gn").and_then(|node| node.get("cm")) {
        Some(Tag::String(v)) => ChannelMode::from_source_name(&v.to_string_lossy()),
        _ => None,
    };
    Ok(Some((
        NodeIdentity {
            host,
            part: Some(uuid),
            outer: false,
        },
        mode,
    )))
}
fn is_quartz_fiber(part: &Tag) -> bool {
    matches!(part.get("id"), Some(Tag::String(id)) if id.to_string_lossy() == "ae2:quartz_fiber")
}
fn is_me_tunnel(part: &Tag) -> bool {
    matches!(part.get("id"), Some(Tag::String(id)) if id.to_string_lossy() == "ae2:me_p2p_tunnel")
}
fn is_toggle(part: &Tag) -> bool {
    matches!(part.get("id"), Some(Tag::String(id)) if matches!(id.to_string_lossy().as_str(), "ae2:toggle_bus" | "ae2:inverted_toggle_bus"))
}
fn toggle_enabled(part: &Tag) -> Result<bool, &'static str> {
    let signal = match part.get("toggle_signal") {
        Some(Tag::Byte(0)) => false,
        Some(Tag::Byte(1)) => true,
        _ => return Err("toggle redstone has not been sampled"),
    };
    Ok(
        if matches!(part.get("id"), Some(Tag::String(id)) if id.to_string_lossy() == "ae2:toggle_bus")
        {
            signal
        } else {
            !signal
        },
    )
}
fn outer_mode(part: &Tag) -> Option<ChannelMode> {
    match part.get("outer").and_then(|node| node.get("cm")) {
        Some(Tag::String(v)) => ChannelMode::from_source_name(&v.to_string_lossy()),
        _ => None,
    }
}
fn add_part(
    topology: &mut PhysicalTopology,
    pos: Pos,
    face: u8,
    part: &Tag,
    id: NodeIdentity,
    mode: Option<ChannelMode>,
    center_after: Device,
    observe_ready: &mut impl FnMut(&mut PhysicalTopology, NodeIdentity),
) -> Result<(), String> {
    if is_quartz_fiber(part) || is_toggle(part) || is_me_tunnel(part) {
        let enabled = if is_toggle(part) {
            toggle_enabled(part).map_err(str::to_owned)?
        } else {
            false
        };
        let outer = NodeIdentity { outer: true, ..id };
        topology
            .part_ready_with(pos, face, id, mode, center_after, |topology| {
                observe_ready(topology, id);
                topology.virtual_ready_at(
                    pos,
                    face,
                    outer,
                    Device::new(if is_me_tunnel(part) { ae2_network_core::DeviceKind::DenseCable } else { ae2_network_core::DeviceKind::GlassCable }),
                    outer_mode(part),
                )?;
                if enabled {
                    topology.explicit_connect(id, outer)?;
                }
                Ok(())
            })
            .map_err(str::to_owned)
    } else {
        topology
            .part_ready_with(pos, face, id, mode, center_after, |topology| {
                observe_ready(topology, id);
                Ok(())
            })
            .map_err(str::to_owned)
    }
}
fn remove_part(
    topology: &mut PhysicalTopology,
    pos: Pos,
    face: u8,
    part: &Tag,
    id: NodeIdentity,
    center_after: Option<Device>,
) -> Result<(), String> {
    let outer = NodeIdentity { outer: true, ..id };
    let remove_outer = |topology: &mut PhysicalTopology| {
        if is_quartz_fiber(part) || is_toggle(part) || is_me_tunnel(part) {
            topology.virtual_destroyed(outer)?;
        }
        Ok(())
    };
    match center_after {
        Some(device) => topology.part_destroyed_with(pos, face, device, remove_outer),
        None => topology.part_destroyed_with_host_callback(pos, face, remove_outer),
    }
    .map_err(str::to_owned)
}
/// Inventory/config edits that do not affect topology are not journaled; a
/// removal must match the physical face/part identity, not an old full tag.
fn same_part(a: &Tag, b: &Tag) -> bool {
    if part_face(a) != part_face(b) || a.get("id") != b.get("id") {
        return false;
    }
    match (a.get("ae2_part_identity"), b.get("ae2_part_identity")) {
        (Some(x), Some(y)) => x == y,
        (None, None) => true,
        _ => false,
    }
}
fn side_parts(machine: &Machine) -> Result<Vec<Tag>, String> {
    match machine.config.get(&"parts".into()) {
        None => Ok(Vec::new()),
        Some(Tag::List(parts)) => {
            let mut faces = BTreeSet::new();
            for part in parts {
                let face = part_face(part).ok_or("invalid part face")?;
                if !faces.insert(face) {
                    return Err("duplicate part face".into());
                }
            }
            Ok(parts.clone())
        }
        _ => Err("invalid part list".into()),
    }
}
// Keep non-network physical machines out of the ordered graph while still
// validating their incarnation on removal. The list mirrors the host builder.
fn networked(machine: &Machine) -> bool {
    matches!(
        machine.kind,
        MachineKind::Controller
            | MachineKind::EnergyAcceptor
            | MachineKind::EnergyCell
            | MachineKind::DenseEnergyCell
            | MachineKind::CreativeEnergyCell
            | MachineKind::VibrationChamber
            | MachineKind::Drive
            | MachineKind::Chest
            | MachineKind::CableBus
            | MachineKind::Inscriber
            | MachineKind::Charger
            | MachineKind::MolecularAssembler
            | MachineKind::GrowthAccelerator
            | MachineKind::CrystalResonanceGenerator
            | MachineKind::QuantumRing
            | MachineKind::QuantumLink
            | MachineKind::Terminal
            | MachineKind::CraftingTerminal
            | MachineKind::CraftingUnit
            | MachineKind::CraftingAccelerator
            | MachineKind::CraftingStorage1k
            | MachineKind::CraftingStorage4k
            | MachineKind::CraftingStorage16k
            | MachineKind::CraftingStorage64k
            | MachineKind::CraftingStorage256k
            | MachineKind::Interface
            | MachineKind::PatternProvider
            | MachineKind::WirelessAccessPoint
            | MachineKind::IoPort
            | MachineKind::SpatialIoPort
            | MachineKind::SpatialPylon
            | MachineKind::SpatialAnchor
            | MachineKind::CraftingMonitor
            | MachineKind::DebugPhantomNode
    )
}
fn center(mut device: Device) -> Device {
    device.attached.clear();
    device.part_channel_modes.clear();
    device
}
fn centerless(machine: &Machine) -> bool {
    machine.kind == MachineKind::CableBus
        && machine.config.get(&"ae2_centerless".into()) == Some(&Tag::Byte(1))
        && !machine.config.contains_key(&"cable".into())
}
fn add_centerless_part(topology: &mut PhysicalTopology, pos: Pos, face: u8, part: &Tag,
    id: NodeIdentity, mode: Option<ChannelMode>,
    observe_ready: &mut impl FnMut(&mut PhysicalTopology, NodeIdentity)) -> Result<(), String> {
    let enabled = if is_toggle(part) { toggle_enabled(part).map_err(str::to_owned)? } else { false };
    topology.centerless_part_ready(pos, face, id, mode, |topology| {
        observe_ready(topology, id);
        if is_quartz_fiber(part) || is_toggle(part) || is_me_tunnel(part) {
            let outer = NodeIdentity { outer: true, ..id };
            topology.virtual_ready_at(pos, face, outer,
                Device::new(if is_me_tunnel(part) { ae2_network_core::DeviceKind::DenseCable } else { ae2_network_core::DeviceKind::GlassCable }), outer_mode(part))?;
            if enabled { topology.explicit_connect(id, outer)?; }
        }
        Ok(())
    }).map_err(str::to_owned)
}

/// Opt-in boundaries for an ordered lifecycle observer. Drain membership
/// changes at each boundary. A part's node-ready boundary occurs inside its
/// addToWorld callback, before the container connects it to the center.
pub enum ReplayCheckpoint<'a> {
    BeforeMutation { dimension: i32, event: &'a Ae2Mutation },
    /// Managed node properties must be supplied before creation, including a
    /// resident CPU that formed while its node was still unready.
    NodeInitializing { node: NodeIdentity, device: &'a mut Device, preserve_faces: &'a mut bool },
    NodeReady { node: NodeIdentity },
    AfterMutation { dimension: i32, event: &'a Ae2Mutation },
    Topology,
}
#[derive(Clone)]
struct Physical {
    id: NodeIdentity,
    pos: Pos,
    machine: Machine,
    saved_mode: Option<ChannelMode>,
    quantum_faces: Option<u8>,
}
#[derive(Clone, Default)]
pub struct EventReplay {
    states: BTreeMap<(i32, Local), u32>,
    physical: BTreeMap<(i32, Local), Physical>,
    ignored: BTreeMap<(i32, Local), u64>,
    ready: BTreeSet<NodeIdentity>,
    dynamic_faces: BTreeSet<NodeIdentity>,
    queue: ReadyQueue,
    pub topology: PhysicalTopology,
}
impl EventReplay {
    pub fn node_is_ready(&self, id:NodeIdentity)->bool {self.ready.contains(&id)}
    /// Apply a globally ordered journal batch atomically. In particular, a
    /// source replace-part emits removal then addition; if its new virtual
    /// type is unsupported, neither half may become visible to the caller.
    pub fn batch(
        &mut self,
        events: impl IntoIterator<Item = (i32, Ae2Mutation)>,
        mut encode: impl FnMut(i32, Local) -> Option<Pos>,
        mut device: impl FnMut(&Machine, u32) -> Option<Device>,
    ) -> Result<(), String> {
        self.batch_filtered(events, &mut encode, &mut device, |_| true)
    }
    /// Ignore ordinary terrain states after replay; retain AE2 candidate
    /// states so a later block-entity event still sees its creation state.
    pub fn batch_filtered(
        &mut self,
        events: impl IntoIterator<Item = (i32, Ae2Mutation)>,
        encode: impl FnMut(i32, Local) -> Option<Pos>,
        device: impl FnMut(&Machine, u32) -> Option<Device>,
        is_ae2_state: impl FnMut(u32) -> bool,
    ) -> Result<(), String> {
        self.batch_observed(events, encode, device, is_ae2_state, |_, _| Ok(()))
    }
    /// Observe each committed topology transition in journal order. Both the
    /// graph and any caller-owned observer state must be staged by the caller.
    pub fn batch_observed(
        &mut self,
        events: impl IntoIterator<Item = (i32, Ae2Mutation)>,
        mut encode: impl FnMut(i32, Local) -> Option<Pos>,
        mut device: impl FnMut(&Machine, u32) -> Option<Device>,
        mut is_ae2_state: impl FnMut(u32) -> bool,
        mut observe: impl FnMut(&Self, u64) -> Result<(), String>,
    ) -> Result<(), String> {
        let before = self.clone();
        for (dimension, event) in events {
            let position = event.position;
            let server_tick = event.server_tick;
            let state = match &event.kind {
                Ae2MutationKind::State { new, .. } | Ae2MutationKind::ChunkState { state: new } => {
                    Some(*new)
                }
                _ => None,
            };
            if let Err(error) = self.mutation(dimension, event, &mut encode, &mut device) {
                *self = before;
                return Err(error);
            }
            if let Some(state) = state {
                let key = (dimension, position);
                if !is_ae2_state(state) && !self.physical.contains_key(&key) {
                    self.states.remove(&key);
                }
            }
            if let Err(error) = observe(self, server_tick) {
                *self = before;
                return Err(error);
            }
        }
        Ok(())
    }
    /// Opt into source-order lifecycle checkpoints. The caller must stage its
    /// observer state and discard it if this batch fails; observer faults must
    /// be kept separate from the independently useful topology replay.
    pub fn batch_observed_traced(
        &mut self,
        events: impl IntoIterator<Item = (i32, Ae2Mutation)>,
        mut encode: impl FnMut(i32, Local) -> Option<Pos>,
        mut device: impl FnMut(&Machine, u32) -> Option<Device>,
        mut is_ae2_state: impl FnMut(u32) -> bool,
        mut observe: impl FnMut(&Self, u64) -> Result<(), String>,
        mut trace: impl FnMut(&mut PhysicalTopology, ReplayCheckpoint<'_>),
    ) -> Result<(), String> {
        let before = self.clone();
        for (dimension, event) in events {
            let position = event.position;
            let server_tick = event.server_tick;
            let state = match &event.kind {
                Ae2MutationKind::State { new, .. } | Ae2MutationKind::ChunkState { state: new } => Some(*new),
                _ => None,
            };
            trace(&mut self.topology, ReplayCheckpoint::BeforeMutation { dimension, event: &event });
            if let Err(error) = self.mutation_observed(dimension, event.clone(), &mut encode, &mut device,
                &mut |topology, node| trace(topology, ReplayCheckpoint::NodeReady { node })) {
                *self = before;
                return Err(error);
            }
            if let Some(state) = state {
                let key = (dimension, position);
                if !is_ae2_state(state) && !self.physical.contains_key(&key) {
                    self.states.remove(&key);
                }
            }
            trace(&mut self.topology, ReplayCheckpoint::AfterMutation { dimension, event: &event });
            if let Err(error) = observe(self, server_tick) {
                *self = before;
                return Err(error);
            }
        }
        Ok(())
    }
    /// Apply one actual journal entry. `encode` gives the stable cross-dimension
    /// graph coordinate; `device` resolves the recorded state and machine.
    pub fn mutation(
        &mut self,
        dim: i32,
        event: Ae2Mutation,
        mut encode: impl FnMut(i32, Local) -> Option<Pos>,
        mut device: impl FnMut(&Machine, u32) -> Option<Device>,
    ) -> Result<(), String> {
        self.mutation_observed(dim, event, &mut encode, &mut device, &mut |_, _| {})
    }
    fn mutation_observed(
        &mut self,
        dim: i32,
        event: Ae2Mutation,
        mut encode: impl FnMut(i32, Local) -> Option<Pos>,
        mut device: impl FnMut(&Machine, u32) -> Option<Device>,
        observe_ready: &mut impl FnMut(&mut PhysicalTopology, NodeIdentity),
    ) -> Result<(), String> {
        let key = (dim, event.position);
        // Ordinary block writes cannot affect a node not yet present. Keep
        // their latest state for a later BE event without cloning the graph.
        if !self.physical.contains_key(&key) {
            if let Ae2MutationKind::State { new, .. } | Ae2MutationKind::ChunkState { state: new } =
                &event.kind
            {
                if !self.states.contains_key(&key) && self.states.len() >= 100_000 {
                    return Err("topology state replay capacity exceeded".into());
                }
                self.states.insert(key, *new);
                return Ok(());
            }
        }
        let before = self.clone();
        let result = (|| {
            let key = (dim, event.position);
            match event.kind {
                Ae2MutationKind::State { new, .. } | Ae2MutationKind::ChunkState { state: new } => {
                    if !self.states.contains_key(&key) && self.states.len() >= 100_000 {
                        return Err("topology state replay capacity exceeded".into());
                    }
                    self.states.insert(key, new);
                    if let Some(physical) = self.physical.get(&key) {
                        if self.ready.contains(&physical.id) && !centerless(&physical.machine) {
                            // A replacement block may update state before its
                            // old entity is destroyed; that destruction event
                            // must retain the old node and connection order.
                            if let Some(mut next) = device(&physical.machine, new) {
                                if let Some(faces) = physical.quantum_faces { next.faces = faces; }
                                if self.dynamic_faces.contains(&physical.id) {
                                    next.faces = self.topology.node_faces(physical.id).ok_or("dynamic node faces unavailable")?;
                                }
                                self.topology
                                    .update_node(physical.pos, center(next))
                                    .map_err(str::to_owned)?;
                            }
                        }
                    }
                }
                Ae2MutationKind::EntityAdded {
                    incarnation,
                    entity,
                    ..
                } => {
                    let BlockEntity::Ae2(machine) = entity else {
                        return Err("non-AE2 entity in AE2 journal".into());
                    };
                    if self.physical.contains_key(&key) || self.ignored.contains_key(&key) {
                        return Err("duplicate physical entity event".into());
                    }
                    if !networked(&machine) {
                        self.ignored.insert(key, incarnation);
                        return Ok(());
                    }
                    let pos = encode(dim, event.position).ok_or("invalid dimension address")?;
                    let id = NodeIdentity {
                        host: incarnation,
                        part: None,
                        outer: false,
                    };
                    let node_tag = if machine.kind == MachineKind::DebugPhantomNode { "proxy" } else { "gn" };
                    let saved_mode = match machine
                        .config
                        .get(&node_tag.into())
                        .and_then(|tag| tag.get("cm"))
                    {
                        Some(horizon_world::nbt::Tag::String(name)) => {
                            ChannelMode::from_source_name(&name.to_string_lossy())
                        }
                        _ => None,
                    };
                    self.physical.insert(
                        key,
                        Physical {
                            id,
                            pos,
                            machine: *machine,
                            saved_mode,
                            quantum_faces: None,
                        },
                    );
                    self.queue
                        .schedule(dim, event.position.0 >> 4, event.position.2 >> 4, id);
                }
                Ae2MutationKind::EntityRemoved { incarnation, .. } => {
                    if let Some(existing) = self.ignored.get(&key) {
                        if *existing != incarnation {
                            return Err("ignored entity incarnation mismatch".into());
                        }
                        self.ignored.remove(&key);
                        self.states.remove(&key);
                        return Ok(());
                    }
                    let Some(physical) = self.physical.get(&key) else {
                        return Err("removed entity was not registered".into());
                    };
                    if physical.id.host != incarnation {
                        return Err("removed entity incarnation mismatch".into());
                    }
                    self.dynamic_faces.remove(&physical.id);
                    if self.ready.remove(&physical.id) {
                        let parts = side_parts(&physical.machine)?;
                        // CableBusContainer.removeFromWorld uses DOWN, UP,
                        // NORTH, SOUTH, WEST, EAST, then the center.
                        for face in [3, 2, 5, 4, 1, 0] {
                            if let Some(part) =
                                parts.iter().find(|part| part_face(part) == Some(face))
                            {
                                if let Some((id, _)) = part_node(part, physical.id.host)? {
                                    remove_part(
                                        &mut self.topology,
                                        physical.pos,
                                        face,
                                        part,
                                        id,
                                        None,
                                    )?;
                                }
                            }
                        }
                        if !centerless(&physical.machine) {
                            self.topology
                                .node_destroyed(physical.pos)
                                .map_err(str::to_owned)?;
                        }
                    }
                    self.physical.remove(&key);
                }
                Ae2MutationKind::MeLink { a, b, added } => {
                    if added { self.topology.explicit_connect(a, b) }
                    else { self.topology.explicit_disconnect(a, b) }.map_err(str::to_owned)?;
                }
                Ae2MutationKind::QuantumLink { a, b, added } => {
                    if added { self.topology.explicit_connect(a, b) }
                    else { self.topology.explicit_disconnect(a, b) }.map_err(str::to_owned)?;
                }
                // Pattern and priority updates do not change topology. A
                // provider-registry observer consumes this ordered snapshot.
                Ae2MutationKind::ProviderRefresh { .. } | Ae2MutationKind::ProviderRawRead { .. }
                | Ae2MutationKind::EmitterProviderRefresh { .. } | Ae2MutationKind::CpuNeighbor { .. } => {}
                Ae2MutationKind::QuantumFaces { incarnation, faces } => {
                    if faces & !63 != 0 { return Err("invalid quantum face mask".into()); }
                    let physical = self.physical.get_mut(&key).ok_or("quantum node unavailable")?;
                    if physical.id.host != incarnation || !matches!(physical.machine.kind, MachineKind::QuantumLink | MachineKind::QuantumRing) {
                        return Err("quantum node identity mismatch".into());
                    }
                    physical.quantum_faces = Some(faces);
                    if self.ready.contains(&physical.id) {
                        let state = *self.states.get(&key).ok_or("quantum node state unavailable")?;
                        let mut next = device(&physical.machine, state).ok_or("quantum node device unavailable")?;
                        next.faces = faces;
                        self.topology.update_node(physical.pos, center(next)).map_err(str::to_owned)?;
                    }
                }
                Ae2MutationKind::ForcedMode { node, mode } => {
                    if self.ready.contains(&node) {
                        self.topology
                            .services
                            .apply(GridEvent::SetForcedMode { node, mode })
                            .map_err(str::to_owned)?;
                    } else if let Some(physical) = self.physical.values_mut().find(|p| p.id == node)
                    {
                        physical.saved_mode = mode;
                    } else {
                        return Err("forced-mode target is unavailable".into());
                    }
                }
                Ae2MutationKind::PartAdded { incarnation, part } => {
                    let physical = self.physical.get_mut(&key).ok_or("part host unavailable")?;
                    if physical.id.host != incarnation
                        || physical.machine.kind != MachineKind::CableBus
                    {
                        return Err("part host incarnation mismatch".into());
                    }
                    let face = part_face(&part).ok_or("part face unavailable")?;
                    let node = part_node(&part, incarnation)?;
                    let mut parts = side_parts(&physical.machine)?;
                    if parts.iter().any(|other| part_face(other) == Some(face)) {
                        return Err("occupied part face".into());
                    }
                    parts.push(part.clone());
                    physical
                        .machine
                        .config
                        .insert("parts".into(), Tag::List(parts));
                    if self.ready.contains(&physical.id) {
                        if centerless(&physical.machine) {
                            if let Some((id, mode)) = node {
                                add_centerless_part(&mut self.topology, physical.pos, face, &part, id, mode, observe_ready)?;
                            }
                            return Ok(());
                        }
                        let state = *self.states.get(&key).ok_or("part host state unavailable")?;
                        let next = center(
                            device(&physical.machine, state).ok_or("unsupported part host")?,
                        );
                        if let Some((id, mode)) = node {
                            add_part(
                                &mut self.topology,
                                physical.pos,
                                face,
                                &part,
                                id,
                                mode,
                                next,
                                observe_ready,
                            )?;
                        } else {
                            self.topology
                                .update_node(physical.pos, next)
                                .map_err(str::to_owned)?;
                        }
                    }
                }
                Ae2MutationKind::PartRemoved { incarnation, part } => {
                    let physical = self.physical.get_mut(&key).ok_or("part host unavailable")?;
                    if physical.id.host != incarnation {
                        return Err("part host incarnation mismatch".into());
                    }
                    let face = part_face(&part).ok_or("part face unavailable")?;
                    let node = part_node(&part, incarnation)?;
                    let mut parts = side_parts(&physical.machine)?;
                    let index = parts
                        .iter()
                        .position(|other| same_part(other, &part))
                        .ok_or("removed part mismatch")?;
                    parts.remove(index);
                    physical
                        .machine
                        .config
                        .insert("parts".into(), Tag::List(parts));
                    if self.ready.contains(&physical.id) {
                        if centerless(&physical.machine) {
                            if let Some((id, _)) = node {
                                remove_part(&mut self.topology, physical.pos, face, &part, id, None)?;
                            }
                            return Ok(());
                        }
                        let state = *self.states.get(&key).ok_or("part host state unavailable")?;
                        let next = center(
                            device(&physical.machine, state).ok_or("unsupported part host")?,
                        );
                        if let Some((id, _)) = node {
                            remove_part(
                                &mut self.topology,
                                physical.pos,
                                face,
                                &part,
                                id,
                                Some(next),
                            )?;
                        } else {
                            self.topology
                                .update_node(physical.pos, next)
                                .map_err(str::to_owned)?;
                        }
                    }
                }
                Ae2MutationKind::PartChanged {
                    incarnation,
                    before,
                    after,
                } => {
                    let physical = self.physical.get_mut(&key).ok_or("part host unavailable")?;
                    if physical.id.host != incarnation {
                        return Err("part host incarnation mismatch".into());
                    }
                    // Non-ME P2P frequency/settings do not alter grid edges;
                    // ME, toggle and other live part changes still fail closed
                    // until their ordered virtual-link adapters exist.
                    if self.ready.contains(&physical.id) {
                        let id = before.get("id");
                        let stable = id == after.get("id")
                            && part_face(&before) == part_face(&after)
                            && part_node(&before, incarnation)? == part_node(&after, incarnation)?;
                        let is_non_me = matches!(id,Some(Tag::String(name)) if name.to_string().is_ok_and(|s| matches!(s.as_str(),"ae2:item_p2p_tunnel"|"ae2:fluid_p2p_tunnel"|"ae2:fe_p2p_tunnel"|"ae2:redstone_p2p_tunnel"|"ae2:light_p2p_tunnel")));
                        if stable && is_toggle(&before) {
                            let (inner, _) = part_node(&before, incarnation)?
                                .ok_or("toggle node unavailable")?;
                            let outer = NodeIdentity {
                                outer: true,
                                ..inner
                            };
                            let old = toggle_enabled(&before).map_err(str::to_owned)?;
                            let new = toggle_enabled(&after).map_err(str::to_owned)?;
                            if old != new {
                                if new {
                                    self.topology.explicit_connect(inner, outer)
                                } else {
                                    self.topology.explicit_disconnect(inner, outer)
                                }
                                .map_err(str::to_owned)?;
                            }
                        } else if !stable || !(is_non_me || is_me_tunnel(&before)) {
                            return Err("live part change requires ordered link adapter".into());
                        }
                    }
                    let mut parts = side_parts(&physical.machine)?;
                    let index = parts
                        .iter()
                        .position(|part| same_part(part, &before))
                        .ok_or("changed part mismatch")?;
                    parts[index] = after;
                    physical
                        .machine
                        .config
                        .insert("parts".into(), Tag::List(parts));
                }
                Ae2MutationKind::CenterChanged {
                    incarnation,
                    before,
                    after,
                } => {
                    let physical = self
                        .physical
                        .get_mut(&key)
                        .ok_or("cable host unavailable")?;
                    if physical.id.host != incarnation
                        || physical.machine.kind != MachineKind::CableBus
                    {
                        return Err("cable host incarnation mismatch".into());
                    }
                    let current = physical.machine.config.get(&"cable".into()).cloned();
                    if current != before {
                        return Err("cable center changed unexpectedly".into());
                    }
                    let state = *self
                        .states
                        .get(&key)
                        .ok_or("cable host state unavailable")?;
                    if before.is_none() && after.is_some() && centerless(&physical.machine) {
                        physical.machine.config.insert("cable".into(), after.ok_or("missing cable")?);
                        let next = device(&physical.machine, state).ok_or("unsupported cable center")?;
                        if self.ready.contains(&physical.id) {
                            self.topology.center_added(physical.pos, physical.id, center(next), physical.saved_mode)
                                .map_err(str::to_owned)?;
                        }
                        return Ok(());
                    }
                    if before.is_some() && after.is_none() {
                        if self.ready.contains(&physical.id) {
                            self.topology.center_destroyed(physical.pos).map_err(str::to_owned)?;
                        }
                        physical.machine.config.remove(&"cable".into());
                        physical.machine.config.insert("ae2_centerless".into(), Tag::Byte(1));
                        return Ok(());
                    }
                    if before.is_none() || after.is_none() {
                        return Err("legacy cable center lifecycle is ambiguous".into());
                    }
                    // Recoloring keeps the same source node and calls setGridColor.
                    let old = device(&physical.machine, state).ok_or("unsupported cable center")?;
                    physical
                        .machine
                        .config
                        .insert("cable".into(), after.ok_or("missing cable")?);
                    let next =
                        device(&physical.machine, state).ok_or("unsupported cable center")?;
                    if old.kind != next.kind {
                        return Err("cable type replacement requires ordered adapter".into());
                    }
                    if self.ready.contains(&physical.id) {
                        self.topology
                            .update_node(physical.pos, center(next))
                            .map_err(str::to_owned)?;
                    }
                }
            }
            Ok(())
        })();
        if result.is_err() {
            *self = before;
        }
        result
    }
    /// Drain one level's source-ordered chunk snapshot at level tick end.
    /// Caller invokes callbacks chunk-by-chunk; new callbacks for the current
    /// chunk wait, while those for a later snapshot chunk can still run.
    pub fn ready_chunks(&self, dim: i32) -> Vec<(i32, i32)> {
        self.queue.snapshot_chunks(dim)
    }
    pub fn ready_chunk(
        &mut self,
        dim: i32,
        chunk: (i32, i32),
        device: impl FnMut(&Machine, u32) -> Option<Device>,
    ) -> Result<(), String> {
        self.ready_chunk_traced(dim, chunk, device, |_, _| {})
    }
    /// Call at each managed-node creation boundary, including inside each
    /// side part's addToWorld before its connection to the center. Observer
    /// state must be staged and discarded if this method returns an error.
    pub fn ready_chunk_traced(
        &mut self,
        dim: i32,
        chunk: (i32, i32),
        mut device: impl FnMut(&Machine, u32) -> Option<Device>,
        mut trace: impl FnMut(&mut PhysicalTopology, ReplayCheckpoint<'_>),
    ) -> Result<(), String> {
        let before = self.clone();
        let result = (|| {
            let live: BTreeSet<_> = self.physical.values().map(|v| v.id).collect();
            for id in self
                .queue
                .take_chunk(dim, chunk.0, chunk.1, |id| live.contains(&id))
            {
                let (key, physical) = self
                    .physical
                    .iter()
                    .find(|(_, p)| p.id == id)
                    .ok_or("ready node disappeared")?;
                let state = *self.states.get(key).ok_or("ready node state unavailable")?;
                if centerless(&physical.machine) {
                    let parts = side_parts(&physical.machine)?;
                    for face in [0, 1, 4, 5, 2, 3] {
                        if let Some(part) = parts.iter().find(|part| part_face(part) == Some(face)) {
                            if let Some((part_id, mode)) = part_node(part, id.host)? {
                                add_centerless_part(&mut self.topology, physical.pos, face, part, part_id, mode,
                                    &mut |topology, node| trace(topology, ReplayCheckpoint::NodeReady { node }))?;
                                trace(&mut self.topology, ReplayCheckpoint::Topology);
                            }
                        }
                    }
                    self.ready.insert(id);
                    continue;
                }
                let mut node =
                    device(&physical.machine, state).ok_or("unsupported physical node state")?;
                if let Some(faces) = physical.quantum_faces { node.faces = faces; }
                let mut preserve_faces = false;
                trace(&mut self.topology, ReplayCheckpoint::NodeInitializing { node: id, device: &mut node, preserve_faces: &mut preserve_faces });
                if preserve_faces { self.dynamic_faces.insert(id); }
                let parts = side_parts(&physical.machine)?;
                let central = center(node);
                self.topology
                    .node_ready(physical.pos, id, central.clone(), physical.saved_mode)
                    .map_err(str::to_owned)?;
                trace(&mut self.topology, ReplayCheckpoint::NodeReady { node: id });
                if physical.machine.kind == MachineKind::DebugPhantomNode {
                    // PhantomNodeBlockEntity.onReady invokes super first,
                    // then creates a distinct managed node at the same block,
                    // and only after create returns exposes that proxy.
                    let proxy = NodeIdentity::phantom_proxy(id.host);
                    self.topology.colocated_ready(physical.pos, proxy, central.clone()).map_err(str::to_owned)?;
                    trace(&mut self.topology, ReplayCheckpoint::NodeReady { node: proxy });
                    self.topology.expose_colocated(physical.pos, proxy).map_err(str::to_owned)?;
                    trace(&mut self.topology, ReplayCheckpoint::Topology);
                }
                // CableBusContainer.addToWorld traverses DIRECTIONS_WITH_NULL
                // in reverse: center, EAST, WEST, SOUTH, NORTH, UP, DOWN.
                for face in [0, 1, 4, 5, 2, 3] {
                    if let Some(part) = parts.iter().find(|part| part_face(part) == Some(face)) {
                        if let Some((part_id, mode)) = part_node(part, id.host)? {
                            add_part(
                                &mut self.topology,
                                physical.pos,
                                face,
                                part,
                                part_id,
                                mode,
                                central.clone(),
                                &mut |topology, node| trace(topology, ReplayCheckpoint::NodeReady { node }),
                            )?;
                            trace(&mut self.topology, ReplayCheckpoint::Topology);
                        }
                    }
                }
                self.ready.insert(id);
            }
            Ok(())
        })();
        if result.is_err() {
            *self = before;
        }
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ae2_world::{Machine, MachineKind},
        block_entities::BlockEntities,
    };
    use ae2_network_core::{ControllerState, DeviceKind};
    use horizon_world::nbt::Tag;
    fn encode(_: i32, p: Local) -> Option<Pos> {
        Some(Pos {
            x: p.0,
            y: p.1,
            z: p.2,
        })
    }
    fn device(m: &Machine, _: u32) -> Option<Device> {
        let mut result = Device::new(match m.kind {
            MachineKind::Controller => DeviceKind::Controller,
            MachineKind::CableBus => DeviceKind::GlassCable,
            MachineKind::Drive => DeviceKind::Drive,
            MachineKind::DebugPhantomNode => DeviceKind::PoweredMachine,
            _ => return None,
        });
        if m.kind == MachineKind::DebugPhantomNode { result.idle_override = Some(1.0); }
        for part in side_parts(m).ok()? {
            if let Some(face) = part_face(&part) {
                result.faces &= !(1 << face);
            }
        }
        if let Some(cable) = m
            .config
            .get(&"cable".into())
            .and_then(crate::save::item_from_nbt)
        {
            result.color = if cable.item.starts_with("ae2:red_") {
                ae2_network_core::CableColor::Red
            } else if cable.item.starts_with("ae2:blue_") {
                ae2_network_core::CableColor::Blue
            } else {
                ae2_network_core::CableColor::Transparent
            };
        }
        Some(result)
    }
    #[test]
    fn phantom_replay_creates_main_then_proxy_and_keeps_proxy_after_removal() {
        let pos = (1, 64, 0);
        let mut entities = BlockEntities::default();
        entities.start_ae2_journal();
        entities.state_changed(pos, 0, 1);
        entities.insert(pos, BlockEntity::Ae2(Box::new(Machine::new(MachineKind::DebugPhantomNode))));
        let host = entities.incarnation(&pos).unwrap();
        let main = NodeIdentity { host, part: None, outer: false };
        let proxy = NodeIdentity::phantom_proxy(host);
        let at = encode(0, pos).unwrap();
        let mut replay = EventReplay::default();
        replay.batch(entities.take_ae2_mutations().unwrap().into_iter().map(|event| (0, event)), encode, device).unwrap();
        let mut ready = Vec::new();
        replay.ready_chunk_traced(0, (0, 0), device, |topology, checkpoint| {
            if let ReplayCheckpoint::NodeReady { node } = checkpoint {
                ready.push((node, topology.exposed_colocated(at)));
            }
        }).unwrap();
        assert_eq!(ready, vec![(main, None), (proxy, None)]);
        assert_eq!(replay.topology.exposed_colocated(at), Some(proxy));
        assert_ne!(replay.topology.services.service_id(main), replay.topology.services.service_id(proxy));
        entities.remove(&pos);
        replay.batch(entities.take_ae2_mutations().unwrap().into_iter().map(|event| (0, event)), encode, device).unwrap();
        assert_eq!(replay.topology.services.service_id(main), None);
        assert!(replay.topology.services.service_id(proxy).is_some());
        assert_eq!(replay.topology.exposed_colocated(at), None);
    }
    #[test]
    fn provider_part_ready_checkpoint_precedes_center_merge_for_load_and_live_add() {
        use ae2_network_core::service_events::MembershipChange;
        for loaded_part in [true, false] {
            let mut entities = BlockEntities::default();
            entities.start_ae2_journal();
            let pos = (0, 64, 0);
            let part = Tag::compound([
                ("id", Tag::String("ae2:cable_pattern_provider".into())),
                ("face", Tag::String("east".into())),
                ("ae2_part_identity", Tag::IntArray(vec![1, 2, 3, 4])),
            ]);
            let mut machine = Machine::new(MachineKind::CableBus);
            if loaded_part { machine.config.insert("parts".into(), Tag::List(vec![part.clone()])); }
            entities.state_changed(pos, 0, 1);
            entities.insert(pos, BlockEntity::Ae2(Box::new(machine)));
            let host = entities.incarnation(&pos).unwrap();
            let center = NodeIdentity { host, part: None, outer: false };
            let part_id = NodeIdentity { host, part: Some([1, 2, 3, 4]), outer: false };
            let mut replay = EventReplay::default();
            replay.topology.services.start_membership_recording().unwrap();
            replay.batch(entities.take_ae2_mutations().unwrap().into_iter().map(|e| (0, e)), encode, device).unwrap();
            let mut checkpoints: Vec<(&str, Vec<MembershipChange>)> = Vec::new();
            let mut trace = |topology: &mut PhysicalTopology, checkpoint: ReplayCheckpoint<'_>| {
                let changes = topology.services.take_membership_changes().unwrap();
                let label = match checkpoint {
                    ReplayCheckpoint::NodeInitializing { .. } => "node-initializing",
                    ReplayCheckpoint::NodeReady { node } if node == part_id => "part-ready",
                    ReplayCheckpoint::NodeReady { .. } => "other-ready",
                    ReplayCheckpoint::BeforeMutation { .. } => "before",
                    ReplayCheckpoint::AfterMutation { .. } => "after",
                    ReplayCheckpoint::Topology => "topology",
                };
                checkpoints.push((label, changes));
            };
            replay.ready_chunk_traced(0, (0, 0), device, &mut trace).unwrap();
            if !loaded_part {
                entities.record_ae2_part(pos, part, true);
                replay.batch_observed_traced(entities.take_ae2_mutations().unwrap().into_iter().map(|e| (0, e)),
                    encode, device, |_| true, |_, _| Ok(()), &mut trace).unwrap();
            }
            drop(trace);
            let ready = checkpoints.iter().position(|(label, _)| *label == "part-ready").unwrap();
            let created = checkpoints[ready].1.iter().find(|change| change.node == part_id).unwrap();
            assert_eq!(created.from, None);
            let standalone = created.to.unwrap();
            let merged = checkpoints[ready + 1].1.iter().find(|change| change.node == part_id).unwrap();
            assert_eq!(merged.from, Some(standalone));
            assert_eq!(merged.to, replay.topology.services.service_id(center));
            assert_ne!(Some(standalone), merged.to);
            assert_eq!(replay.topology.services.service_id(part_id), merged.to);
        }
    }
    #[test]
    fn ordered_quantum_faces_and_remote_link_replay_with_identity_validation() {
        fn quantum_device(m:&Machine,state:u32)->Option<Device> {
            if matches!(m.kind,MachineKind::QuantumLink|MachineKind::QuantumRing) {
                let mut d=Device::new(DeviceKind::GlassCable);d.faces=0;Some(d)
            } else { device(m,state) }
        }
        let mut entities=BlockEntities::default();entities.start_ae2_journal();
        for pos in [(0,64,0),(1,64,0),(20,64,0)] {
            entities.state_changed(pos,0,1);
            entities.insert(pos,BlockEntity::Ae2(Box::new(Machine::new(MachineKind::QuantumLink))));
        }
        let ids:Vec<_>=[(0,64,0),(1,64,0),(20,64,0)].into_iter().map(|pos|NodeIdentity{host:entities.incarnation(&pos).unwrap(),part:None,outer:false}).collect();
        entities.record_ae2_quantum_faces((0,64,0),ids[0].host,63);
        entities.record_ae2_quantum_faces((1,64,0),ids[1].host,63);
        let mut replay=EventReplay::default();
        replay.batch(entities.take_ae2_mutations().unwrap().into_iter().map(|e|(0,e)),encode,quantum_device).unwrap();
        for chunk in replay.ready_chunks(0){replay.ready_chunk(0,chunk,quantum_device).unwrap();}
        assert!(replay.topology.services.connected(ids[0],ids[1]));
        entities.record_ae2_quantum_link((0,64,0),ids[0],ids[2],true);
        entities.record_ae2_quantum_faces((0,64,0),ids[0].host,0);
        replay.batch(entities.take_ae2_mutations().unwrap().into_iter().map(|e|(0,e)),encode,quantum_device).unwrap();
        assert!(!replay.topology.services.connected(ids[0],ids[1]));
        assert!(replay.topology.services.connected(ids[0],ids[2]));
        entities.record_ae2_quantum_faces((0,64,0),ids[0].host+100,63);
        assert!(replay.batch(entities.take_ae2_mutations().unwrap().into_iter().map(|e|(0,e)),encode,quantum_device).is_err());
        assert!(replay.topology.services.connected(ids[0],ids[2]));
        assert!(!replay.topology.services.connected(ids[0],ids[1]));
    }
    #[test]
    fn real_ordered_be_mutations_defer_ready_and_keep_split_service() {
        let mut entities = BlockEntities::default();
        entities.start_ae2_journal();
        for (x, kind) in [
            (0, MachineKind::Controller),
            (1, MachineKind::CableBus),
            (2, MachineKind::CableBus),
            (3, MachineKind::Drive),
        ] {
            let p = (x, 64, 0);
            entities.state_changed(p, 0, 1);
            entities.insert(p, BlockEntity::Ae2(Box::new(Machine::new(kind))));
        }
        let ids: Vec<_> = (0..4)
            .map(|x| NodeIdentity {
                host: entities.incarnation(&(x, 64, 0)).unwrap(),
                part: None,
                outer: false,
            })
            .collect();
        let mut replay = EventReplay::default();
        for event in entities.take_ae2_mutations().unwrap() {
            replay.mutation(0, event, encode, device).unwrap();
        }
        assert!(
            replay.topology.services.service(ids[0]).is_none(),
            "BE insertion only queues readiness"
        );
        for chunk in replay.ready_chunks(0) {
            replay.ready_chunk(0, chunk, device).unwrap();
        }
        {
            let service = replay.topology.services.service_mut(ids[0]).unwrap();
            service.channels = 1;
            service.controller = ControllerState::Online;
        }
        for i in 0..3 {
            replay
                .topology
                .services
                .orient_toward(ids[i], ids[i + 1])
                .unwrap();
        }
        entities.state_changed((2, 64, 0), 1, 0);
        entities.remove(&(2, 64, 0));
        // Zero means air: the old node is retained until the actual remove event.
        fn resolve(m: &Machine, state: u32) -> Option<Device> {
            if state == 0 { None } else { device(m, state) }
        }
        for event in entities.take_ae2_mutations().unwrap() {
            replay.mutation(0, event, encode, resolve).unwrap();
        }
        assert_eq!(
            replay.topology.services.service(ids[0]).unwrap().channels,
            1
        );
        assert_eq!(
            replay.topology.services.service(ids[3]).unwrap().channels,
            0
        );
    }
    #[test]
    fn real_face_part_events_split_then_reconnect_in_source_order() {
        let mut entities = BlockEntities::default();
        entities.start_ae2_journal();
        for (x, kind) in [
            (0, MachineKind::Controller),
            (1, MachineKind::CableBus),
            (2, MachineKind::Drive),
        ] {
            let pos = (x, 64, 0);
            entities.state_changed(pos, 0, 1);
            entities.insert(pos, BlockEntity::Ae2(Box::new(Machine::new(kind))));
        }
        let mut replay = EventReplay::default();
        for event in entities.take_ae2_mutations().unwrap() {
            replay.mutation(0, event, encode, device).unwrap();
        }
        for chunk in replay.ready_chunks(0) {
            replay.ready_chunk(0, chunk, device).unwrap();
        }
        let host = entities.incarnation(&(1, 64, 0)).unwrap();
        let center = NodeIdentity {
            host,
            part: None,
            outer: false,
        };
        let drive = NodeIdentity {
            host: entities.incarnation(&(2, 64, 0)).unwrap(),
            part: None,
            outer: false,
        };
        let part = Tag::compound([
            ("id", Tag::String("ae2:export_bus".into())),
            ("face", Tag::String("east".into())),
            ("ae2_part_identity", Tag::IntArray(vec![1, 2, 3, 4])),
        ]);
        entities.record_ae2_part((1, 64, 0), part.clone(), true);
        replay
            .mutation(
                0,
                entities.take_ae2_mutations().unwrap().remove(0),
                encode,
                device,
            )
            .unwrap();
        let part_id = NodeIdentity {
            host,
            part: Some([1, 2, 3, 4]),
            outer: false,
        };
        assert!(replay.topology.services.connected(center, part_id));
        assert!(!replay.topology.services.connected(center, drive));
        let mut removed = part;
        if let Tag::Compound(data) = &mut removed {
            data.insert(
                "state_json".into(),
                Tag::String("changed since attachment".into()),
            );
        }
        entities.record_ae2_part((1, 64, 0), removed, false);
        replay
            .mutation(
                0,
                entities.take_ae2_mutations().unwrap().remove(0),
                encode,
                device,
            )
            .unwrap();
        assert!(replay.topology.services.connected(center, drive));
        assert!(replay.topology.services.service(part_id).is_none());
    }
    #[test]
    fn unsupported_replacement_rolls_back_the_entire_ordered_batch() {
        let mut entities = BlockEntities::default();
        entities.start_ae2_journal();
        let pos = (0, 64, 0);
        entities.state_changed(pos, 0, 1);
        entities.insert(
            pos,
            BlockEntity::Ae2(Box::new(Machine::new(MachineKind::CableBus))),
        );
        let host = entities.incarnation(&pos).unwrap();
        let mut replay = EventReplay::default();
        for event in entities.take_ae2_mutations().unwrap() {
            replay.mutation(0, event, encode, device).unwrap();
        }
        for chunk in replay.ready_chunks(0) {
            replay.ready_chunk(0, chunk, device).unwrap();
        }
        let old = Tag::compound([
            ("id", Tag::String("ae2:export_bus".into())),
            ("face", Tag::String("north".into())),
            ("ae2_part_identity", Tag::IntArray(vec![1, 2, 3, 4])),
        ]);
        entities.record_ae2_part(pos, old.clone(), true);
        replay
            .mutation(
                0,
                entities.take_ae2_mutations().unwrap().remove(0),
                encode,
                device,
            )
            .unwrap();
        let old_id = NodeIdentity {
            host,
            part: Some([1, 2, 3, 4]),
            outer: false,
        };
        let new = Tag::compound([
            ("id", Tag::String("ae2:unsupported_fixture_part".into())),
            ("face", Tag::String("north".into())),
            ("ae2_part_identity", Tag::IntArray(vec![5, 6, 7, 8])),
        ]);
        entities.record_ae2_part(pos, old, false);
        entities.record_ae2_part(pos, new, true);
        let events = entities
            .take_ae2_mutations()
            .unwrap()
            .into_iter()
            .map(|event| (0, event));
        assert!(replay.batch(events, encode, device).is_err());
        assert!(replay.topology.services.service(old_id).is_some());
    }
    #[test]
    fn non_me_p2p_frequency_change_keeps_the_same_grid_node() {
        let mut entities = BlockEntities::default();
        entities.start_ae2_journal();
        let pos = (0, 64, 0);
        entities.state_changed(pos, 0, 1);
        entities.insert(
            pos,
            BlockEntity::Ae2(Box::new(Machine::new(MachineKind::CableBus))),
        );
        let host = entities.incarnation(&pos).unwrap();
        let mut replay = EventReplay::default();
        for event in entities.take_ae2_mutations().unwrap() {
            replay.mutation(0, event, encode, device).unwrap();
        }
        for chunk in replay.ready_chunks(0) {
            replay.ready_chunk(0, chunk, device).unwrap();
        }
        let part = Tag::compound([
            ("id", Tag::String("ae2:item_p2p_tunnel".into())),
            ("face", Tag::String("north".into())),
            ("ae2_part_identity", Tag::IntArray(vec![1, 2, 3, 4])),
            ("p2p_frequency", Tag::Short(7)),
        ]);
        entities.record_ae2_part(pos, part.clone(), true);
        replay
            .mutation(
                0,
                entities.take_ae2_mutations().unwrap().remove(0),
                encode,
                device,
            )
            .unwrap();
        let mut updated = part.clone();
        if let Tag::Compound(data) = &mut updated {
            data.insert("p2p_frequency".into(), Tag::Short(42));
        }
        entities.record_ae2_part_change(pos, part, updated);
        replay
            .mutation(
                0,
                entities.take_ae2_mutations().unwrap().remove(0),
                encode,
                device,
            )
            .unwrap();
        let id = NodeIdentity {
            host,
            part: Some([1, 2, 3, 4]),
            outer: false,
        };
        assert!(replay.topology.services.service(id).is_some());
    }
    #[test]
    fn real_center_recolor_event_rechecks_source_color_connections() {
        use crate::items::ItemStack;
        let mut entities = BlockEntities::default();
        entities.start_ae2_journal();
        let red = crate::save::item_to_nbt(&ItemStack::new("ae2:red_glass_cable", 1));
        let blue = crate::save::item_to_nbt(&ItemStack::new("ae2:blue_glass_cable", 1));
        for x in 0..2 {
            let pos = (x, 64, 0);
            entities.state_changed(pos, 0, 1);
            let mut machine = Machine::new(MachineKind::CableBus);
            machine.config.insert("cable".into(), red.clone());
            entities.insert(pos, BlockEntity::Ae2(Box::new(machine)));
        }
        let left = NodeIdentity {
            host: entities.incarnation(&(0, 64, 0)).unwrap(),
            part: None,
            outer: false,
        };
        let right = NodeIdentity {
            host: entities.incarnation(&(1, 64, 0)).unwrap(),
            part: None,
            outer: false,
        };
        let mut replay = EventReplay::default();
        for event in entities.take_ae2_mutations().unwrap() {
            replay.mutation(0, event, encode, device).unwrap();
        }
        for chunk in replay.ready_chunks(0) {
            replay.ready_chunk(0, chunk, device).unwrap();
        }
        assert!(replay.topology.services.connected(left, right));
        entities.record_ae2_center_change((0, 64, 0), Some(red), Some(blue));
        replay
            .mutation(
                0,
                entities.take_ae2_mutations().unwrap().remove(0),
                encode,
                device,
            )
            .unwrap();
        assert!(!replay.topology.services.connected(left, right));
    }
    #[test]
    fn unsupported_part_event_fails_without_mutating_replay() {
        let mut entities = BlockEntities::default();
        entities.start_ae2_journal();
        let p = (0, 64, 0);
        entities.state_changed(p, 0, 1);
        entities.insert(
            p,
            BlockEntity::Ae2(Box::new(Machine::new(MachineKind::CableBus))),
        );
        let mut replay = EventReplay::default();
        for event in entities.take_ae2_mutations().unwrap() {
            replay.mutation(0, event, encode, device).unwrap();
        }
        for chunk in replay.ready_chunks(0) {
            replay.ready_chunk(0, chunk, device).unwrap();
        }
        let id = NodeIdentity {
            host: entities.incarnation(&p).unwrap(),
            part: None,
            outer: false,
        };
        entities.record_ae2_part(
            p,
            Tag::compound([
                ("id", Tag::String("ae2:me_p2p_tunnel".into())),
                ("face", Tag::String("north".into())),
            ]),
            true,
        );
        let event = entities.take_ae2_mutations().unwrap().remove(0);
        assert!(replay.mutation(0, event, encode, device).is_err());
        assert!(replay.topology.services.service(id).is_some());
    }
    #[test]
    fn non_network_entity_and_unrelated_terrain_do_not_poison_replay() {
        let mut entities = BlockEntities::default();
        entities.start_ae2_journal();
        let tank = (0, 64, 0);
        entities.state_changed((1, 64, 0), 0, 42);
        entities.state_changed(tank, 0, 1);
        entities.insert(
            tank,
            BlockEntity::Ae2(Box::new(Machine::new(MachineKind::SkyStoneTank))),
        );
        let mut replay = EventReplay::default();
        replay
            .batch_filtered(
                entities
                    .take_ae2_mutations()
                    .unwrap()
                    .into_iter()
                    .map(|e| (0, e)),
                encode,
                device,
                |state| state == 1,
            )
            .unwrap();
        assert!(replay.states.get(&(0, (1, 64, 0))).is_none());
        assert!(replay.ready_chunks(0).is_empty());
        entities.remove(&tank);
        replay
            .batch_filtered(
                entities
                    .take_ae2_mutations()
                    .unwrap()
                    .into_iter()
                    .map(|e| (0, e)),
                encode,
                device,
                |state| state == 1,
            )
            .unwrap();
        assert!(replay.ignored.is_empty());
    }
    #[test]
    fn quartz_fiber_outer_finds_neighbor_and_keeps_center_grid_separate() {
        let mut entities = BlockEntities::default();
        entities.start_ae2_journal();
        for (p, kind) in [
            ((0, 64, 0), MachineKind::CableBus),
            ((1, 64, 0), MachineKind::Drive),
        ] {
            entities.state_changed(p, 0, 1);
            entities.insert(p, BlockEntity::Ae2(Box::new(Machine::new(kind))));
        }
        let host = entities.incarnation(&(0, 64, 0)).unwrap();
        let neighbor = NodeIdentity {
            host: entities.incarnation(&(1, 64, 0)).unwrap(),
            part: None,
            outer: false,
        };
        let main = NodeIdentity {
            host,
            part: None,
            outer: false,
        };
        let inner = NodeIdentity {
            host,
            part: Some([1, 2, 3, 4]),
            outer: false,
        };
        let outer = NodeIdentity {
            outer: true,
            ..inner
        };
        let part = Tag::compound([
            ("id", Tag::String("ae2:quartz_fiber".into())),
            ("face", Tag::String("east".into())),
            ("ae2_part_identity", Tag::IntArray(vec![1, 2, 3, 4])),
        ]);
        let mut replay = EventReplay::default();
        replay
            .batch(
                entities
                    .take_ae2_mutations()
                    .unwrap()
                    .into_iter()
                    .map(|e| (0, e)),
                encode,
                device,
            )
            .unwrap();
        for chunk in replay.ready_chunks(0) {
            replay.ready_chunk(0, chunk, device).unwrap();
        }
        entities.record_ae2_part((0, 64, 0), part.clone(), true);
        replay
            .batch(
                entities
                    .take_ae2_mutations()
                    .unwrap()
                    .into_iter()
                    .map(|e| (0, e)),
                encode,
                device,
            )
            .unwrap();
        assert!(replay.topology.services.connected(main, inner));
        assert!(replay.topology.services.connected(outer, neighbor));
        assert!(!replay.topology.services.connected(main, neighbor));
        assert!(!replay.topology.services.connected(inner, outer));
        entities.record_ae2_part((0, 64, 0), part, false);
        replay
            .batch(
                entities
                    .take_ae2_mutations()
                    .unwrap()
                    .into_iter()
                    .map(|e| (0, e)),
                encode,
                device,
            )
            .unwrap();
        assert!(replay.topology.services.connected(main, neighbor));
        assert!(replay.topology.services.service(outer).is_none());
    }

    #[test]
    fn toggle_signal_changes_the_real_ordered_internal_connection() {
        let mut entities = BlockEntities::default();
        entities.start_ae2_journal();
        let pos = (0, 64, 0);
        entities.state_changed(pos, 0, 1);
        entities.insert(
            pos,
            BlockEntity::Ae2(Box::new(Machine::new(MachineKind::CableBus))),
        );
        let host = entities.incarnation(&pos).unwrap();
        let inner = NodeIdentity {
            host,
            part: Some([4, 3, 2, 1]),
            outer: false,
        };
        let outer = NodeIdentity {
            outer: true,
            ..inner
        };
        let make = |signal| {
            Tag::compound([
                ("id", Tag::String("ae2:toggle_bus".into())),
                ("face", Tag::String("east".into())),
                ("ae2_part_identity", Tag::IntArray(vec![4, 3, 2, 1])),
                ("toggle_signal", Tag::Byte(signal)),
            ])
        };
        let mut replay = EventReplay::default();
        replay
            .batch(
                entities
                    .take_ae2_mutations()
                    .unwrap()
                    .into_iter()
                    .map(|e| (0, e)),
                encode,
                device,
            )
            .unwrap();
        for chunk in replay.ready_chunks(0) {
            replay.ready_chunk(0, chunk, device).unwrap();
        }
        entities.record_ae2_part(pos, make(0), true);
        replay
            .batch(
                entities
                    .take_ae2_mutations()
                    .unwrap()
                    .into_iter()
                    .map(|e| (0, e)),
                encode,
                device,
            )
            .unwrap();
        assert!(!replay.topology.services.connected(inner, outer));
        entities.record_ae2_part_change(pos, make(0), make(1));
        replay
            .batch(
                entities
                    .take_ae2_mutations()
                    .unwrap()
                    .into_iter()
                    .map(|e| (0, e)),
                encode,
                device,
            )
            .unwrap();
        assert!(replay.topology.services.connected(inner, outer));
        entities.record_ae2_part_change(pos, make(1), make(0));
        replay
            .batch(
                entities
                    .take_ae2_mutations()
                    .unwrap()
                    .into_iter()
                    .map(|e| (0, e)),
                encode,
                device,
            )
            .unwrap();
        assert!(!replay.topology.services.connected(inner, outer));
    }

    #[test]
    fn saved_world_clear_tears_down_real_journaled_nodes() {
        let mut entities = BlockEntities::default();
        entities.start_ae2_journal();
        for (pos, kind) in [
            ((0, 64, 0), MachineKind::CableBus),
            ((1, 64, 0), MachineKind::Drive),
        ] {
            entities.state_changed(pos, 0, 1);
            entities.insert(pos, BlockEntity::Ae2(Box::new(Machine::new(kind))));
        }
        let mut replay = EventReplay::default();
        replay
            .batch(
                entities
                    .take_ae2_mutations()
                    .unwrap()
                    .into_iter()
                    .map(|e| (0, e)),
                encode,
                device,
            )
            .unwrap();
        for chunk in replay.ready_chunks(0) {
            replay.ready_chunk(0, chunk, device).unwrap();
        }
        assert_eq!(replay.topology.services.node_ids().count(), 2);
        entities.clear();
        replay
            .batch(
                entities
                    .take_ae2_mutations()
                    .unwrap()
                    .into_iter()
                    .map(|e| (0, e)),
                encode,
                device,
            )
            .unwrap();
        assert_eq!(replay.topology.services.node_ids().count(), 0);
    }
    #[test]
    fn marked_centerless_host_replays_physical_center_add_and_remove() {
        let mut entities=BlockEntities::default(); entities.start_ae2_journal();
        let pos=(0,64,0); entities.state_changed(pos,0,1);
        let part = |face: &str, id: i32| Tag::compound([
            ("id",Tag::String("ae2:terminal".into())), ("face",Tag::String(face.into())),
            ("ae2_part_identity",Tag::IntArray(vec![id,0,0,0]))]);
        let mut machine=Machine::new(MachineKind::CableBus);
        machine.config.insert("ae2_centerless".into(),Tag::Byte(1));
        machine.config.insert("parts".into(),Tag::List(vec![part("east",1),part("west",2)]));
        entities.insert(pos,BlockEntity::Ae2(Box::new(machine)));
        let host=entities.incarnation(&pos).unwrap();
        let east=NodeIdentity { host,part:Some([1,0,0,0]),outer:false };
        let west=NodeIdentity { host,part:Some([2,0,0,0]),outer:false };
        let mut replay=EventReplay::default();
        replay.batch(entities.take_ae2_mutations().unwrap().into_iter().map(|e|(0,e)),encode,device).unwrap();
        for chunk in replay.ready_chunks(0) { replay.ready_chunk(0,chunk,device).unwrap(); }
        assert_ne!(replay.topology.services.service(east).unwrap().pivot,replay.topology.services.service(west).unwrap().pivot);
        let cable=crate::save::item_to_nbt(&crate::items::ItemStack::new("ae2:fluix_glass_cable",1));
        entities.record_ae2_center_change(pos,None,Some(cable.clone()));
        replay.batch(entities.take_ae2_mutations().unwrap().into_iter().map(|e|(0,e)),encode,device).unwrap();
        assert_eq!(replay.topology.services.service(east).unwrap().pivot,replay.topology.services.service(west).unwrap().pivot);
        entities.record_ae2_center_change(pos,Some(cable),None);
        replay.batch(entities.take_ae2_mutations().unwrap().into_iter().map(|e|(0,e)),encode,device).unwrap();
        assert_ne!(replay.topology.services.service(east).unwrap().pivot,replay.topology.services.service(west).unwrap().pivot);
    }

    #[test]
    fn scheduled_me_link_events_connect_and_disconnect_real_outer_nodes() {
        let mut entities=BlockEntities::default(); entities.start_ae2_journal();
        for (x, face) in [(0,"east"),(4,"west")] {
            let pos=(x,64,0); entities.state_changed(pos,0,1);
            let mut machine=Machine::new(MachineKind::CableBus);
            machine.config.insert("parts".into(),Tag::List(vec![Tag::compound([
                ("id",Tag::String("ae2:me_p2p_tunnel".into())),
                ("face",Tag::String(face.into())),
                ("ae2_part_identity",Tag::IntArray(vec![x+1,0,0,0]))])]));
            entities.insert(pos,BlockEntity::Ae2(Box::new(machine)));
        }
        let a=NodeIdentity {host:entities.incarnation(&(0,64,0)).unwrap(),part:Some([1,0,0,0]),outer:true};
        let b=NodeIdentity {host:entities.incarnation(&(4,64,0)).unwrap(),part:Some([5,0,0,0]),outer:true};
        let mut replay=EventReplay::default();
        replay.batch(entities.take_ae2_mutations().unwrap().into_iter().map(|e|(0,e)),encode,device).unwrap();
        for chunk in replay.ready_chunks(0) { replay.ready_chunk(0,chunk,device).unwrap(); }
        assert!(!replay.topology.services.connected(a,b));
        entities.record_ae2_me_link((0,64,0),a,b,true);
        replay.batch(entities.take_ae2_mutations().unwrap().into_iter().map(|e|(0,e)),encode,device).unwrap();
        assert!(replay.topology.services.connected(a,b));
        entities.record_ae2_me_link((0,64,0),a,b,false);
        replay.batch(entities.take_ae2_mutations().unwrap().into_iter().map(|e|(0,e)),encode,device).unwrap();
        assert!(!replay.topology.services.connected(a,b));
    }

}
