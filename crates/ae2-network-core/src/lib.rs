// Derived from Applied Energistics 2, commit db17504a86128fdf3dae31f5fb7a112a646e0b93.
// Copyright (c) 2013-2014 AlgorithmX2 and the AE2 contributors. LGPL-3.0-or-later; see LICENSE.
//! A deliberately bounded network simulation. See README for fidelity and host responsibilities.
pub mod me_tick;
mod pathing;
pub mod physical_events;
pub mod quantum_registry;
pub mod ready_queue;
pub mod service_events;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Pos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}
impl Pos {
    fn neighbours(self) -> impl Iterator<Item = Pos> {
        [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ]
        .into_iter()
        .filter_map(move |(x, y, z)| {
            Some(Pos {
                x: self.x.checked_add(x)?,
                y: self.y.checked_add(y)?,
                z: self.z.checked_add(z)?,
            })
        })
    }
}
/// AE2 ChannelMode; forced modes on merged nodes choose the most permissive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelMode {
    #[default]
    Default,
    X2,
    X3,
    X4,
    Infinite,
}
impl ChannelMode {
    /// Parse the exact enum strings used by AE2's common config and node NBT.
    pub fn from_source_name(name: &str) -> Option<Self> {
        match name {
            "DEFAULT" => Some(Self::Default),
            "X2" => Some(Self::X2),
            "X3" => Some(Self::X3),
            "X4" => Some(Self::X4),
            "INFINITE" => Some(Self::Infinite),
            _ => None,
        }
    }
    pub fn source_name(self) -> &'static str {
        match self {
            Self::Default => "DEFAULT",
            Self::X2 => "X2",
            Self::X3 => "X3",
            Self::X4 => "X4",
            Self::Infinite => "INFINITE",
        }
    }
    pub fn ad_hoc_limit(self) -> usize {
        match self {
            Self::Default => 8,
            Self::X2 => 16,
            Self::X3 => 24,
            Self::X4 => 32,
            Self::Infinite => i32::MAX as usize,
        }
    }
    pub fn capacity(self, base: usize) -> usize {
        if base == 0 {
            return 0;
        }
        match self {
            Self::Default => base,
            Self::X2 => base * 2,
            Self::X3 => base * 3,
            Self::X4 => base * 4,
            Self::Infinite => i32::MAX as usize,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Controller,
    EnergyAcceptor,
    VibrationChamber,
    EnergyCell,
    DenseEnergyCell,
    Drive,
    Terminal,
    NormalCable,
    GlassCable,
    CoveredCable,
    SmartCable,
    DenseCable,
    DenseCoveredCable,
    DenseSmartCable,
    ImportBus,
    ExportBus,
    CraftingCpu,
    Interface,
    /// Inscriber, charger, molecular assembler: no channel and no idle drain.
    PoweredMachine,
    CreativeEnergyCell,
}
impl DeviceKind {
    pub fn requires_channel(self) -> bool {
        matches!(
            self,
            Self::Drive
                | Self::Terminal
                | Self::ImportBus
                | Self::ExportBus
                | Self::CraftingCpu
                | Self::Interface
        )
    }
    fn carries(self) -> bool {
        matches!(
            self,
            Self::NormalCable
                | Self::GlassCable
                | Self::CoveredCable
                | Self::SmartCable
                | Self::DenseCable
                | Self::DenseCoveredCable
                | Self::DenseSmartCable
        )
    }
    fn capacity(self) -> usize {
        if matches!(
            self,
            Self::DenseCable | Self::DenseCoveredCable | Self::DenseSmartCable
        ) {
            32
        } else {
            8
        }
    }
    pub fn energy_capacity(self) -> f64 {
        match self {
            Self::Controller => 8000.0,
            Self::EnergyCell => 200000.0,
            Self::DenseEnergyCell => 1600000.0,
            _ => 0.0,
        }
    }
    fn idle(self) -> f64 {
        match self {
            Self::Controller => 3.0,
            Self::Terminal => 0.5,
            Self::NormalCable
            | Self::GlassCable
            | Self::CoveredCable
            | Self::SmartCable
            | Self::DenseCable
            | Self::DenseCoveredCable
            | Self::DenseSmartCable
            | Self::EnergyCell
            | Self::DenseEnergyCell
            | Self::EnergyAcceptor
            | Self::VibrationChamber
            | Self::PoweredMachine => 0.0,
            Self::CreativeEnergyCell => 0.0,
            _ => 1.0,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CableColor {
    #[default]
    Transparent,
    White,
    LightGray,
    Gray,
    Black,
    Lime,
    Yellow,
    Orange,
    Brown,
    Red,
    Pink,
    Magenta,
    Purple,
    Blue,
    LightBlue,
    Cyan,
    Green,
}

/// A channel-requiring part on one face of a cable. Face uses +X,-X,+Y,-Y,+Z,-Z
/// as indices 0..5. The cable remains a routing node even if the part is offline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AttachedPart {
    pub face: u8,
    pub idle: f64,
    #[serde(default)]
    pub compressed: bool,
    #[serde(default = "requires_part_channel")]
    pub requires_channel: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Device {
    pub kind: DeviceKind,
    #[serde(default)]
    pub cannot_carry_compressed: bool,
    #[serde(default)]
    pub multiblock: Option<Pos>,
    pub energy: f64,
    #[serde(default)]
    pub color: CableColor,
    /// Exposed sides bitmask: +X,-X,+Y,-Y,+Z,-Z.
    #[serde(default = "all_faces")]
    pub faces: u8,
    #[serde(default)]
    pub attached: Vec<AttachedPart>,
    #[serde(default)]
    pub idle_override: Option<f64>,
    #[serde(default)]
    pub capacity_override: Option<usize>,
    #[serde(default)]
    pub channel_override: Option<bool>,
    /// A persisted forced channel mode from this node, if any.
    #[serde(default)]
    pub channel_mode_override: Option<ChannelMode>,
    /// Forced modes owned by attached face nodes, separate from the host node.
    #[serde(default)]
    pub part_channel_modes: BTreeMap<u8, ChannelMode>,
}
fn requires_part_channel() -> bool {
    true
}
fn all_faces() -> u8 {
    63
}
impl Device {
    pub fn new(kind: DeviceKind) -> Self {
        Self {
            kind,
            cannot_carry_compressed: false,
            multiblock: None,
            energy: 0.0,
            color: CableColor::Transparent,
            faces: all_faces(),
            attached: Vec::new(),
            idle_override: None,
            capacity_override: None,
            channel_override: None,
            channel_mode_override: None,
            part_channel_modes: BTreeMap::new(),
        }
    }
    fn idle(&self) -> f64 {
        self.idle_override.unwrap_or_else(|| self.kind.idle())
    }
    fn capacity(&self) -> usize {
        self.capacity_override
            .unwrap_or_else(|| self.kind.capacity())
    }
    fn requires_channel(&self) -> bool {
        self.channel_override
            .unwrap_or_else(|| self.kind.requires_channel())
    }
}
/// Stable physical incarnation plus optional face-part UUID and node role.
/// Host identities must change when a physical node is replaced; synthetic
/// positions must never be used as identities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeIdentity {
    pub host: u64,
    pub part: Option<[i32; 4]>,
    pub outer: bool,
}
impl NodeIdentity {
    /// The Phantom block's second managed node shares its host incarnation,
    /// but has neither the main role nor a multipart identity.
    pub fn phantom_proxy(host: u64) -> Self {
        Self { host, part: None, outer: true }
    }
    pub fn is_phantom_proxy(self) -> bool {
        self.part.is_none() && self.outer
    }
}
#[derive(Clone, Debug, Default)]
pub struct CommittedChannels {
    nodes: BTreeMap<NodeIdentity, (bool, usize)>,
    services: Vec<CommittedService>,
}
#[derive(Clone, Debug)]
struct CommittedService {
    powered: bool,
    members: BTreeSet<NodeIdentity>,
    channel_power: f64,
    channels: usize,
    controller: ControllerState,
    forced_mode: Option<ChannelMode>,
}
fn member_ids(network: &Network, positions: &[Pos]) -> BTreeSet<NodeIdentity> {
    positions
        .iter()
        .flat_map(|pos| {
            network.node_identities.get(pos).copied().into_iter().chain(
                network.nodes[pos].attached.iter().filter_map(move |part| {
                    network.part_identities.get(&(*pos, part.face)).copied()
                }),
            )
        })
        .collect()
}
fn base_idle(network: &Network, positions: &[Pos]) -> f64 {
    positions
        .iter()
        .map(|pos| {
            network.nodes[pos].idle()
                + network.nodes[pos]
                    .attached
                    .iter()
                    .map(|p| p.idle)
                    .sum::<f64>()
        })
        .sum()
}
impl CommittedChannels {
    /// Capture only a completed path calculation, never an overlaid analysis.
    pub fn capture(network: &Network, analysis: &Analysis) -> Self {
        let mut nodes = BTreeMap::new();
        for (pos, status) in &analysis.nodes {
            if let Some(id) = network.node_identities.get(pos) {
                nodes.insert(*id, (status.channel, status.channels_used));
            }
            for (face, part) in &status.parts {
                if let Some(id) = network.part_identities.get(&(*pos, *face)) {
                    nodes.insert(*id, (part.channel, usize::from(part.channel)));
                }
            }
        }
        let services = analysis
            .components
            .iter()
            .map(|component| CommittedService {
                powered: component
                    .positions
                    .first()
                    .and_then(|pos| analysis.nodes.get(pos))
                    .is_some_and(|node| node.powered),
                members: member_ids(network, &component.positions),
                channel_power: (component.idle_per_tick - base_idle(network, &component.positions))
                    .max(0.0),
                channels: component.channels_in_use,
                controller: component.controller,
                forced_mode: component
                    .positions
                    .iter()
                    .flat_map(|pos| {
                        network.nodes[pos]
                            .channel_mode_override
                            .into_iter()
                            .chain(network.nodes[pos].part_channel_modes.values().copied())
                    })
                    .max_by_key(|mode| mode.ad_hoc_limit()),
            })
            .collect();
        Self { nodes, services }
    }
    /// Transfer the just-committed physical service counters into an ordered
    /// lifecycle graph only when membership agrees exactly.
    pub fn seed_ordered(
        &self,
        ordered: &mut service_events::OrderedServices,
    ) -> Result<(), &'static str> {
        let mut candidate = ordered.clone();
        for service in &self.services {
            let first = *service
                .members
                .iter()
                .next()
                .ok_or("empty committed service")?;
            let target = candidate.service_mut(first).ok_or("ordered node missing")?;
            if target.members != service.members {
                return Err("ordered service membership differs");
            }
            target.powered = service.powered;
            target.channels = service.channels;
            target.channel_power = service.channel_power;
            target.controller = service.controller;
            target.forced_mode = service.forced_mode;
        }
        let committed: BTreeSet<_> = self
            .services
            .iter()
            .flat_map(|s| s.members.iter().copied())
            .collect();
        if committed != candidate.node_ids().collect() {
            return Err("ordered node set differs");
        }
        *ordered = candidate;
        Ok(())
    }
    /// Follow event-owned services between pathing commits. Reject incomplete
    /// shadow graphs rather than guessing which component retained the pivot.
    pub fn with_ordered(
        &self,
        ordered: &service_events::OrderedServices,
        network: &Network,
    ) -> Result<Self, &'static str> {
        let actual: Vec<_> = network
            .components()
            .iter()
            .map(|positions| member_ids(network, positions))
            .collect();
        let tracked: BTreeSet<_> = ordered.node_ids().collect();
        if actual
            .iter()
            .flat_map(|group| group.iter().copied())
            .collect::<BTreeSet<_>>()
            != tracked
        {
            return Err("ordered node set differs from live graph");
        }
        for group in &actual {
            let first = *group.iter().next().ok_or("empty live component")?;
            if ordered
                .service(first)
                .is_none_or(|service| &service.members != group)
            {
                return Err("ordered service differs from live graph");
            }
        }
        let mut next = self.clone();
        let mut seen = BTreeSet::new();
        next.services.clear();
        for node in ordered.node_ids() {
            let service = ordered.service(node).ok_or("ordered node missing")?;
            if seen.insert(service.pivot) {
                next.services.push(CommittedService {
                    powered: service.powered,
                    members: service.members.clone(),
                    channel_power: service.channel_power,
                    channels: service.channels,
                    controller: service.controller,
                    forced_mode: service.forced_mode,
                });
            }
        }
        Ok(next)
    }
    pub fn contains_identity(&self, identity: NodeIdentity) -> bool {
        self.nodes.contains_key(&identity)
    }
    /// GridNode.setGrid carries its old grid's locked service mode into the
    /// destination grid. Identity makes this survive a physical grid split.
    pub fn forced_mode(&self, identity: NodeIdentity) -> Option<ChannelMode> {
        self.services
            .iter()
            .find(|service| service.members.contains(&identity))
            .and_then(|service| service.forced_mode)
    }
    /// A source setForcedChannelMode(null) unlocks the whole live service.
    pub fn clear_forced_mode(&mut self, identity: NodeIdentity) {
        for service in &mut self.services {
            if service.members.contains(&identity) {
                service.forced_mode = None;
            }
        }
    }
    fn apply(&self, network: &Network, analysis: &mut Analysis) {
        let groups: Vec<_> = analysis
            .components
            .iter()
            .map(|c| member_ids(network, &c.positions))
            .collect();
        let overlaps: Vec<Vec<usize>> = groups
            .iter()
            .map(|group| {
                self.services
                    .iter()
                    .enumerate()
                    .filter_map(|(i, old)| (!group.is_disjoint(&old.members)).then_some(i))
                    .collect()
            })
            .collect();
        // One-to-one continuity preserves the existing grid service exactly.
        // A split or merge needs the source pivot/event history; keep the
        // newly calculated rate there until that history is implemented.
        for (i, component) in analysis.components.iter_mut().enumerate() {
            let prior = match overlaps[i].as_slice() {
                [] => Some((0.0, 0, ControllerState::None)),
                [old] if overlaps.iter().filter(|group| group.contains(old)).count() == 1 => {
                    Some((
                        self.services[*old].channel_power,
                        self.services[*old].channels,
                        self.services[*old].controller,
                    ))
                }
                _ => None,
            };
            if let Some((channel_power, channels, controller)) = prior {
                component.controller = controller;
                component.idle_per_tick = base_idle(network, &component.positions) + channel_power;
                component.channels_in_use = channels;
            }
        }
        for power in &mut analysis.power_components {
            power.idle_per_tick = 0.0;
        }
        for component in &analysis.components {
            analysis.power_components[component.power_component].idle_per_tick +=
                component.idle_per_tick;
        }
        for (pos, status) in &mut analysis.nodes {
            let component = &analysis.components[status.component];
            let power = &analysis.power_components[component.power_component];
            status.powered = power.energy > 0.0 && power.energy >= power.idle_per_tick;
            let device = &network.nodes[pos];
            let prior = network
                .node_identities
                .get(pos)
                .and_then(|id| self.nodes.get(id));
            status.channel =
                !device.requires_channel() || prior.is_some_and(|(channel, _)| *channel);
            status.channels_used = prior.map_or(0, |(_, used)| *used);
            status.online = status.powered && status.channel;
            for part in &device.attached {
                let Some(value) = status.parts.get_mut(&part.face) else {
                    continue;
                };
                let prior = network
                    .part_identities
                    .get(&(*pos, part.face))
                    .and_then(|id| self.nodes.get(id));
                value.channel =
                    !part.requires_channel || prior.is_some_and(|(channel, _)| *channel);
                value.online = status.powered && value.channel;
            }
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Network {
    #[serde(with = "node_entries")]
    pub nodes: BTreeMap<Pos, Device>,
    /// Explicit grid connections (quantum bridges and the outer nodes of ME tunnels).
    /// Keys are normalized by `connect`; local adjacency still obeys side masks.
    #[serde(default)]
    pub links: BTreeSet<(Pos, Pos)>,
    /// Energy-service overlays (quartz fiber), never channel or storage edges.
    #[serde(default)]
    pub energy_links: BTreeSet<(Pos, Pos)>,
    /// Live service buffers, each projected onto a node of its owning grid.
    /// Physical node save data, not this transient projection, owns custody.
    #[serde(skip)]
    pub energy_buffers: BTreeMap<Pos, f64>,
    /// Source EnergyService public state at the preceding end-tick callback.
    #[serde(skip)]
    pub energy_public_power: BTreeMap<Pos, bool>,
    #[serde(default)]
    pub default_channel_mode: ChannelMode,
    /// Host-owned ephemeral identities and last end-tick channel assignment.
    #[serde(skip)]
    pub node_identities: BTreeMap<Pos, NodeIdentity>,
    /// Host positions whose physical edges are supplied from the observed
    /// ordered topology, as with the two-node Phantom debug block.
    #[serde(skip)]
    pub physical_suppressed: BTreeSet<Pos>,
    /// The currently exposed node for a physical host with multiple nodes.
    /// Values are transient graph indices, never world positions or identity.
    #[serde(skip)]
    pub physical_exposed: BTreeMap<Pos, Pos>,
    #[serde(skip)]
    pub part_identities: BTreeMap<(Pos, u8), NodeIdentity>,
    /// A physical side part can own its own node without a cable center.
    /// These transient endpoint bindings are constructed by the host.
    #[serde(skip)]
    pub part_endpoints: BTreeMap<(Pos, u8), Pos>,
    /// Outward-facing source nodes, keyed by real host and face. Ports of
    /// different multipart families can physically face one another.
    #[serde(skip)]
    pub external_ports: BTreeMap<(Pos, u8), Pos>,
    /// Validated live GridNode connection order; identities include attached
    /// parts and virtual outer nodes, independently of coordinate ordering.
    #[serde(skip)]
    pub observed_connection_order: BTreeMap<NodeIdentity, Vec<NodeIdentity>>,
    #[serde(skip)]
    pub committed_channels: Option<CommittedChannels>,
}
mod node_entries {
    use super::*;
    use serde::{Deserializer, Serializer, de::Error};
    pub fn serialize<S: Serializer>(
        nodes: &BTreeMap<Pos, Device>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        nodes.iter().collect::<Vec<_>>().serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<Pos, Device>, D::Error> {
        let entries = Vec::<(Pos, Device)>::deserialize(deserializer)?;
        let mut nodes = BTreeMap::new();
        for (pos, device) in entries {
            if nodes.insert(pos, device).is_some() {
                return Err(D::Error::custom("duplicate position"));
            }
        }
        Ok(nodes)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControllerState {
    None,
    Online,
    Conflict,
}
#[derive(Clone, Debug)]
pub struct NodeStatus {
    pub component: usize,
    pub channel: bool,
    pub powered: bool,
    pub online: bool,
    pub channels_used: usize,
    /// Status of channel-requiring parts, indexed by host face.
    pub parts: BTreeMap<u8, PartStatus>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartStatus {
    pub channel: bool,
    pub online: bool,
}
#[derive(Clone, Debug)]
pub struct ComponentStatus {
    pub positions: Vec<Pos>,
    pub power_component: usize,
    pub controller: ControllerState,
    pub channel_mode: ChannelMode,
    pub channels_in_use: usize,
    pub energy: f64,
    pub idle_per_tick: f64,
}
#[derive(Clone, Debug)]
pub struct PowerComponentStatus {
    pub positions: Vec<Pos>,
    pub energy: f64,
    pub idle_per_tick: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub nodes: BTreeMap<Pos, NodeStatus>,
    pub components: Vec<ComponentStatus>,
    pub power_components: Vec<PowerComponentStatus>,
}
impl Network {
    /// Source node idle draw before pathing channel power.
    pub fn component_base_idle(&self, positions:&[Pos])->f64 { base_idle(self,positions) }
    pub fn connect_facing_ports(&mut self) {
        let ports: Vec<_> = self.external_ports.iter().map(|(key, value)| (*key, *value)).collect();
        for ((host, face), node) in ports {
            let delta = match face {
                0 => (1,0,0), 1 => (-1,0,0), 2 => (0,1,0),
                3 => (0,-1,0), 4 => (0,0,1), 5 => (0,0,-1), _ => continue,
            };
            let Some(x) = host.x.checked_add(delta.0) else { continue };
            let Some(y) = host.y.checked_add(delta.1) else { continue };
            let Some(z) = host.z.checked_add(delta.2) else { continue };
            if let Some(other) = self.external_ports.get(&(Pos { x,y,z }, face ^ 1)).copied() {
                let _ = self.connect(node, other);
            }
        }
    }
    /// Resolve a side part independently of whether its host has a center.
    pub fn part_status(&self, analysis: &Analysis, host: Pos, face: u8) -> Option<PartStatus> {
        if let Some(endpoint) = self.part_endpoints.get(&(host, face)) {
            let node = analysis.nodes.get(endpoint)?;
            Some(PartStatus { channel: node.channel, online: node.online })
        } else {
            analysis.nodes.get(&host)?.parts.get(&face).copied()
        }
    }
    /// Call after deserialization before using a saved network.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.nodes.values().any(|d| {
            !d.energy.is_finite()
                || d.energy < 0.0
                || d.energy > d.kind.energy_capacity()
                || d.faces & !63 != 0
                || d.idle_override.is_some_and(|v| !v.is_finite() || v < 0.0)
                || d.capacity_override.is_some_and(|v| v > 32)
                || (!d.attached.is_empty() && !d.kind.carries())
                || d.part_channel_modes
                    .keys()
                    .any(|face| !d.attached.iter().any(|part| part.face == *face))
                || d.attached
                    .iter()
                    .any(|p| p.face >= 6 || !p.idle.is_finite() || p.idle < 0.0)
                || d.attached
                    .iter()
                    .map(|p| p.face)
                    .collect::<BTreeSet<_>>()
                    .len()
                    != d.attached.len()
        }) {
            return Err("invalid device energy");
        }
        if self.energy_buffers.iter().any(|(p, value)| !self.nodes.contains_key(p) || !value.is_finite() || *value < 0.0) {
            return Err("invalid service energy buffer");
        }
        if self.energy_public_power.keys().any(|p| !self.nodes.contains_key(p)) {
            return Err("invalid service public power node");
        }
        if self
            .links
            .iter()
            .any(|(a, b)| a >= b || !self.nodes.contains_key(a) || !self.nodes.contains_key(b))
        {
            return Err("invalid grid connection");
        }
        if self
            .energy_links
            .iter()
            .any(|(a, b)| a >= b || !self.nodes.contains_key(a) || !self.nodes.contains_key(b))
        {
            return Err("invalid energy overlay");
        }
        Ok(())
    }
    pub fn connect(&mut self, a: Pos, b: Pos) -> Result<(), &'static str> {
        if a == b || !self.nodes.contains_key(&a) || !self.nodes.contains_key(&b) {
            return Err("invalid grid endpoints");
        }
        self.links.insert(if a < b { (a, b) } else { (b, a) });
        Ok(())
    }
    pub fn connect_energy(&mut self, a: Pos, b: Pos) -> Result<(), &'static str> {
        if a == b || !self.nodes.contains_key(&a) || !self.nodes.contains_key(&b) {
            return Err("invalid energy endpoints");
        }
        self.energy_links
            .insert(if a < b { (a, b) } else { (b, a) });
        Ok(())
    }
    pub fn record_connection_order(&mut self, ordered: &service_events::OrderedServices) -> Result<(), &'static str> {
        let mut actual = BTreeMap::<NodeIdentity,BTreeSet<NodeIdentity>>::new();
        for (pos,device) in &self.nodes {
            let host=*self.node_identities.get(pos).ok_or("pathing host identity missing")?;
            let peers=actual.entry(host).or_default();
            for other in self.neighbours(*pos) {
                peers.insert(*self.node_identities.get(&other).ok_or("pathing peer identity missing")?);
            }
            for part in &device.attached {
                let id=*self.part_identities.get(&(*pos,part.face)).ok_or("pathing part identity missing")?;
                peers.insert(id);
            }
            for part in &device.attached {
                let id=self.part_identities[&(*pos,part.face)];
                actual.entry(id).or_default().insert(host);
            }
        }
        let mut observed=BTreeMap::new();
        for (id,peers) in actual {
            let connections=ordered.connections(id);
            if connections.iter().copied().collect::<BTreeSet<_>>() != peers {
                return Err("ordered pathing edges differ from live graph");
            }
            observed.insert(id,connections);
        }
        self.observed_connection_order=observed;
        Ok(())
    }
    fn neighbours(&self, p: Pos) -> Vec<Pos> {
        let mut out = p
            .neighbours()
            .filter(|q| self.nodes.contains_key(q) && self.connects(p, *q))
            .collect::<BTreeSet<_>>();
        for (a, b) in &self.links {
            if *a == p {
                out.insert(*b);
            } else if *b == p {
                out.insert(*a);
            }
        }
        out.into_iter().collect()
    }
    /// Physical adjacent connection, with the graph's face and color policy.
    /// Nonlocal quantum/P2P edges are not presented as visible cable arms.
    pub fn physical_connection(&self, a: Pos, b: Pos) -> bool {
        if !self.nodes.contains_key(&a) || !self.nodes.contains_key(&b)
            || !a.neighbours().any(|p| p == b) { return false; }
        let left = self.physical_exposed.get(&a).copied().unwrap_or(a);
        let right = self.physical_exposed.get(&b).copied().unwrap_or(b);
        self.connects(left, right)
    }
    /// Connected graph components, respecting exposed faces and grid colors.
    fn components(&self) -> Vec<Vec<Pos>> {
        let mut left: BTreeSet<_> = self.nodes.keys().copied().collect();
        let mut out = Vec::new();
        while let Some(start) = left.pop_first() {
            let mut queue = VecDeque::from([start]);
            let mut group = vec![];
            while let Some(p) = queue.pop_front() {
                group.push(p);
                for q in self.neighbours(p) {
                    if left.contains(&q) && self.connects(p, q) {
                        left.remove(&q);
                        queue.push_back(q);
                    }
                }
            }
            group.sort();
            out.push(group);
        }
        out
    }
    fn power_groups(&self, groups: &[Vec<Pos>]) -> Vec<Vec<Pos>> {
        fn root(parents: &mut [usize], mut at: usize) -> usize {
            while parents[at] != at {
                parents[at] = parents[parents[at]];
                at = parents[at];
            }
            at
        }
        let owner: BTreeMap<_, _> = groups
            .iter()
            .enumerate()
            .flat_map(|(i, g)| g.iter().map(move |p| (*p, i)))
            .collect();
        let mut parents: Vec<_> = (0..groups.len()).collect();
        for (a, b) in &self.energy_links {
            if let (Some(a), Some(b)) = (owner.get(a), owner.get(b)) {
                let a = root(&mut parents, *a);
                let b = root(&mut parents, *b);
                if a != b {
                    parents[b] = a;
                }
            }
        }
        let mut merged: BTreeMap<usize, Vec<Pos>> = BTreeMap::new();
        for (i, group) in groups.iter().enumerate() {
            merged
                .entry(root(&mut parents, i))
                .or_default()
                .extend(group.iter().copied());
        }
        merged
            .into_values()
            .map(|mut group| {
                group.sort();
                group
            })
            .collect()
    }
    /// Test actual exposed in-world sides and color without inventing a link.
    pub fn physically_connects(&self, a: Pos, b: Pos) -> bool {
        if !self.nodes.contains_key(&a) || !self.nodes.contains_key(&b) {
            return false;
        }
        if self.physical_suppressed.contains(&a) || self.physical_suppressed.contains(&b) {
            return false;
        }
        let (da, db) = (&self.nodes[&a], &self.nodes[&b]);
        let (dx, dy, dz) = (
            i64::from(b.x) - i64::from(a.x),
            i64::from(b.y) - i64::from(a.y),
            i64::from(b.z) - i64::from(a.z),
        );
        let (af, bf) = match (dx, dy, dz) {
            (1, 0, 0) => (1, 2),
            (-1, 0, 0) => (2, 1),
            (0, 1, 0) => (4, 8),
            (0, -1, 0) => (8, 4),
            (0, 0, 1) => (16, 32),
            (0, 0, -1) => (32, 16),
            _ => return false,
        };
        da.faces & af != 0
            && db.faces & bf != 0
            && (da.color == CableColor::Transparent
                || db.color == CableColor::Transparent
                || da.color == db.color)
    }
    fn connects(&self, a: Pos, b: Pos) -> bool {
        if self.links.contains(&if a < b { (a, b) } else { (b, a) }) {
            return true;
        }
        self.physically_connects(a, b)
    }
    fn controller_state(&self, group: &[Pos]) -> ControllerState {
        let controllers: BTreeSet<_> = group
            .iter()
            .copied()
            .filter(|p| self.nodes[p].kind == DeviceKind::Controller)
            .collect();
        let Some(&first) = controllers.first() else {
            return ControllerState::None;
        };
        for coord in [|p: Pos| p.x, |p: Pos| p.y, |p: Pos| p.z] as [fn(Pos) -> i32; 3] {
            let min = controllers
                .iter()
                .map(|&p| i64::from(coord(p)))
                .min()
                .unwrap();
            let max = controllers
                .iter()
                .map(|&p| i64::from(coord(p)))
                .max()
                .unwrap();
            if max - min >= 7 {
                return ControllerState::Conflict;
            }
        }
        let mut seen = BTreeSet::from([first]);
        let mut q = VecDeque::from([first]);
        while let Some(p) = q.pop_front() {
            for n in p.neighbours() {
                if controllers.contains(&n) && self.connects(p, n) && seen.insert(n) {
                    q.push_back(n);
                }
            }
        }
        if seen.len() != controllers.len() {
            return ControllerState::Conflict;
        }
        for p in &controllers {
            let axis = [(1, 0, 0), (0, 1, 0), (0, 0, 1)]
                .into_iter()
                .filter(|&(x, y, z)| {
                    let plus =
                        p.x.checked_add(x)
                            .zip(p.y.checked_add(y))
                            .zip(p.z.checked_add(z))
                            .map(|((x, y), z)| Pos { x, y, z });
                    let minus =
                        p.x.checked_sub(x)
                            .zip(p.y.checked_sub(y))
                            .zip(p.z.checked_sub(z))
                            .map(|((x, y), z)| Pos { x, y, z });
                    plus.is_some_and(|v| controllers.contains(&v))
                        && minus.is_some_and(|v| controllers.contains(&v))
                })
                .count();
            if axis > 1 {
                return ControllerState::Conflict;
            }
        }
        ControllerState::Online
    }
    /// Energy is explicitly injected by the host. Returns the amount accepted.
    pub fn insert_energy(&mut self, at: Pos, amount: f64) -> Result<f64, &'static str> {
        self.validate()?;
        if !amount.is_finite() || amount < 0.0 {
            return Err("invalid energy amount");
        }
        if !self.nodes.contains_key(&at) {
            return Err("no device at position");
        }
        let group = self
            .power_groups(&self.components())
            .into_iter()
            .find(|c| c.contains(&at))
            .unwrap();
        let mut remaining = amount;
        for p in group {
            let d = self.nodes.get_mut(&p).unwrap();
            let accepted = remaining.min(d.kind.energy_capacity() - d.energy);
            d.energy += accepted;
            remaining -= accepted;
        }
        Ok(amount - remaining)
    }
    pub fn analyze(&self) -> Result<Analysis, &'static str> {
        self.validate()?;
        let mut out = Analysis::default();
        let groups = self.components();
        let power_groups = self.power_groups(&groups);
        let power_owner: BTreeMap<_, _> = power_groups
            .iter()
            .enumerate()
            .flat_map(|(i, g)| g.iter().map(move |p| (*p, i)))
            .collect();
        out.power_components = power_groups
            .into_iter()
            .map(|positions| {
                let energy = if positions
                    .iter()
                    .any(|p| self.nodes[p].kind == DeviceKind::CreativeEnergyCell)
                {
                    f64::MAX
                } else {
                    positions.iter().map(|p| self.nodes[p].energy + self.energy_buffers.get(p).copied().unwrap_or(0.0)).sum()
                };
                PowerComponentStatus {
                    positions,
                    energy,
                    idle_per_tick: 0.0,
                }
            })
            .collect();
        for group in groups {
            let index = out.components.len();
            let power_component = power_owner[&group[0]];
            let controller = self.controller_state(&group);
            // Merged node overrides win over the default; the most relaxed
            // saved mode wins among them (PathingService.restoreChannelMode).
            let channel_mode = group
                .iter()
                .flat_map(|p| {
                    self.nodes[p]
                        .channel_mode_override
                        .into_iter()
                        .chain(self.nodes[p].part_channel_modes.values().copied())
                })
                .max_by_key(|mode| mode.ad_hoc_limit())
                .unwrap_or(self.default_channel_mode);
            let energy = out.power_components[power_component].energy;
            let demands: Vec<_> = group
                .iter()
                .copied()
                .filter(|p| self.nodes[p].requires_channel())
                .collect();
            let parts: Vec<_> = group
                .iter()
                .flat_map(|p| {
                    self.nodes[p]
                        .attached
                        .iter()
                        .filter(|part| part.requires_channel)
                        .map(move |part| (*p, part.face))
                })
                .collect();
            let demand_key = |p: Pos| self.nodes[&p].multiblock.unwrap_or(p);
            let demand_count = demands
                .iter()
                .map(|p| demand_key(*p))
                .collect::<BTreeSet<_>>()
                .len();
            // PathingService.calculateAdHocChannels fails the entire ad-hoc
            // network when a compressed demand shares it with a node that
            // cannot carry compressed channels (nested ME P2P).
            let nested_compressed = group.iter().any(|p| self.nodes[p].cannot_carry_compressed)
                && group.iter().any(|p| {
                    self.nodes[p]
                        .attached
                        .iter()
                        .any(|part| part.requires_channel && part.compressed)
                });
            let mut assigned_groups = BTreeSet::new();
            let mut controller_channel_blocks = 0usize;
            let mut used: BTreeMap<Pos, usize> = BTreeMap::new();
            let mut assigned = BTreeSet::new();
            let mut assigned_parts = BTreeSet::new();
            match controller {
                ControllerState::None
                    if demand_count + parts.len() <= channel_mode.ad_hoc_limit()
                        && !nested_compressed =>
                {
                    assigned.extend(demands.iter().copied());
                    assigned_groups.extend(demands.iter().map(|p| demand_key(*p)));
                    assigned_parts.extend(parts.iter().copied().filter(|(p, face)| {
                        let compressed = self.nodes[p]
                            .attached
                            .iter()
                            .any(|part| part.face == *face && part.compressed);
                        !compressed
                            || !group
                                .iter()
                                .any(|node| self.nodes[node].cannot_carry_compressed)
                    }));
                    for p in &group {
                        used.insert(*p, demand_count + assigned_parts.len());
                    }
                }
                ControllerState::Online => {
                    let allocation = pathing::controller(self, &group, channel_mode);
                    used = allocation.used;
                    assigned = allocation.hosts;
                    assigned_parts = allocation.parts;
                    assigned_groups = allocation.groups;
                    controller_channel_blocks = allocation.channel_blocks;
                }
                _ => {}
            }
            // AE2's PathingService charges channels-by-block / 128 AE/tick.
            let channel_blocks = if controller == ControllerState::None {
                // Grid.size() includes each real multipart grid node as well
                // as its host cable. Attached parts represent those nodes here.
                (group.len()
                    + group
                        .iter()
                        .map(|p| self.nodes[p].attached.len())
                        .sum::<usize>())
                    * (assigned_groups.len() + assigned_parts.len())
            } else {
                controller_channel_blocks
            };
            let idle: f64 = group
                .iter()
                .map(|p| {
                    self.nodes[p].idle()
                        + self.nodes[p]
                            .attached
                            .iter()
                            .map(|part| part.idle)
                            .sum::<f64>()
                })
                .sum::<f64>()
                + channel_blocks as f64 / 128.0;
            let powered = energy >= idle && energy > 0.0;
            for p in &group {
                let channel = !self.nodes[p].requires_channel() || assigned.contains(p);
                out.nodes.insert(
                    *p,
                    NodeStatus {
                        component: index,
                        channel,
                        powered,
                        online: powered && channel,
                        channels_used: used.get(p).copied().unwrap_or(0),
                        parts: self.nodes[p]
                            .attached
                            .iter()
                            .map(|part| {
                                let channel = !part.requires_channel
                                    || assigned_parts.contains(&(*p, part.face));
                                (
                                    part.face,
                                    PartStatus {
                                        channel,
                                        online: powered && channel,
                                    },
                                )
                            })
                            .collect(),
                    },
                );
            }
            out.components.push(ComponentStatus {
                positions: group,
                power_component,
                controller,
                channel_mode,
                channels_in_use: assigned_groups.len() + assigned_parts.len(),
                energy,
                idle_per_tick: idle,
            });
            out.power_components[power_component].idle_per_tick += idle;
        }
        // One shared budget powers the independent grids. Channel allocation,
        // controller validation and storage membership remain component-local.
        for node in out.nodes.values_mut() {
            let component = &out.components[node.component];
            let power = &out.power_components[component.power_component];
            node.powered = power.energy > 0.0 && power.energy >= power.idle_per_tick;
            node.online = node.powered && node.channel;
            for part in node.parts.values_mut() {
                part.online = node.powered && part.channel;
            }
        }
        if let Some(committed) = &self.committed_channels {
            committed.apply(self, &mut out);
        }
        // Preserve source public-power callback timing rather than deriving
        // it directly from the current live physical energy sum.
        for (pos, powered) in &self.energy_public_power {
            if let Some(status)=out.nodes.get_mut(pos) {
                status.powered=*powered;status.online=*powered&&status.channel;
                for part in status.parts.values_mut(){part.online=*powered&&part.channel;}
            }
        }
        Ok(out)
    }
    /// Consume one tick of idle energy; insufficient power drains remaining energy.
    pub fn tick(&mut self) -> Result<Analysis, &'static str> {
        let analysis = self.analyze()?;
        for c in &analysis.power_components {
            if c.positions
                .iter()
                .any(|p| self.nodes[p].kind == DeviceKind::CreativeEnergyCell)
            {
                continue;
            }
            let mut remaining = c.idle_per_tick;
            for p in &c.positions {
                let d = self.nodes.get_mut(p).unwrap();
                let n = remaining.min(d.energy);
                d.energy -= n;
                remaining -= n;
                if remaining <= 0.0 {
                    break;
                }
            }
        }
        self.analyze()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_power_is_the_service_callback_not_the_instant_energy_sum(){
        let pos=Pos{x:0,y:4,z:0};let mut n=Network::default();
        n.nodes.insert(pos,Device::new(DeviceKind::EnergyCell));n.nodes.get_mut(&pos).unwrap().energy=50.0;
        n.energy_public_power.insert(pos,false);assert!(!n.analyze().unwrap().nodes[&pos].powered);
        n.energy_public_power.insert(pos,true);assert!(n.analyze().unwrap().nodes[&pos].powered);
        n.nodes.get_mut(&pos).unwrap().energy=0.0;assert!(n.analyze().unwrap().nodes[&pos].powered);
        n.energy_public_power.insert(pos,false);assert!(!n.analyze().unwrap().nodes[&pos].powered);
    }
    #[test]
    fn service_buffers_are_limited_to_their_power_overlay() {
        let a = Pos { x: 0, y: 4, z: 0 };
        let b = Pos { x: 10, y: 4, z: 0 };
        let mut network = Network::default();
        network.nodes.insert(a, Device::new(DeviceKind::GlassCable));
        network.nodes.insert(b, Device::new(DeviceKind::GlassCable));
        network.energy_buffers.insert(a, 5.0);
        let split = network.analyze().unwrap();
        assert!(split.nodes[&a].powered && !split.nodes[&b].powered);
        network.connect_energy(a, b).unwrap();
        let joined = network.analyze().unwrap();
        assert!(joined.nodes[&a].powered && joined.nodes[&b].powered);
        network.energy_buffers.insert(b, f64::NAN);
        assert!(network.analyze().is_err());
    }
    #[test]
    fn facing_external_ports_connect_across_multipart_families() {
        let a=Pos{x:0,y:0,z:0}; let b=Pos{x:1,y:0,z:0};
        let toggle=Pos{x:100,y:3,z:0}; let fiber=Pos{x:200,y:3,z:0};
        let mut network=Network::default();
        network.nodes.insert(toggle,Device::new(DeviceKind::PoweredMachine));
        network.nodes.insert(fiber,Device::new(DeviceKind::PoweredMachine));
        network.external_ports.insert((a,0),toggle);
        network.external_ports.insert((b,1),fiber);
        network.connect_facing_ports();
        let analysis=network.analyze().unwrap();
        assert_eq!(analysis.nodes[&toggle].component,analysis.nodes[&fiber].component);
        network.external_ports.remove(&(b,1));
        network.links.clear();
        network.external_ports.insert((b,0),fiber);
        network.connect_facing_ports();
        let analysis=network.analyze().unwrap();
        assert_ne!(analysis.nodes[&toggle].component,analysis.nodes[&fiber].component);
    }
    #[test]
    fn centerless_face_endpoints_report_independent_power() {
        let host = Pos { x: 0, y: 0, z: 0 };
        let east = Pos { x: 100, y: 0, z: 0 };
        let west = Pos { x: 200, y: 0, z: 0 };
        let source = Pos { x: 101, y: 0, z: 0 };
        let mut network = Network::default();
        network.nodes.insert(east, Device::new(DeviceKind::Terminal));
        network.nodes.insert(west, Device::new(DeviceKind::Terminal));
        network.nodes.insert(source, Device::new(DeviceKind::CreativeEnergyCell));
        network.part_endpoints.insert((host, 0), east);
        network.part_endpoints.insert((host, 1), west);
        let analysis = network.analyze().unwrap();
        assert!(network.part_status(&analysis, host, 0).unwrap().online);
        assert!(!network.part_status(&analysis, host, 1).unwrap().online);
        assert!(network.part_status(&analysis, host, 2).is_none());
    }
    #[test]
    fn ordered_service_ownership_carries_committed_counts_across_a_split() {
        let p = |x| Pos { x, y: 0, z: 0 };
        let id = |host| NodeIdentity {
            host,
            part: None,
            outer: false,
        };
        let mut n = Network::default();
        let mut events = service_events::OrderedServices::default();
        for (x, kind) in [
            (0, DeviceKind::Controller),
            (1, DeviceKind::GlassCable),
            (2, DeviceKind::GlassCable),
            (3, DeviceKind::Drive),
        ] {
            n.nodes.insert(p(x), Device::new(kind));
            n.node_identities.insert(p(x), id((x + 1) as u64));
            events.add_node(id((x + 1) as u64), None).unwrap();
            if x > 0 {
                events.connect(id(x as u64), id((x + 1) as u64)).unwrap();
            }
        }
        let ready = n.analyze().unwrap();
        let committed = CommittedChannels::capture(&n, &ready);
        committed.seed_ordered(&mut events).unwrap();
        events.orient_controller_paths(&[id(1)], |_| 2).unwrap();
        events.remove_node(id(3)).unwrap();
        n.nodes.remove(&p(2));
        n.node_identities.remove(&p(2));
        n.committed_channels = Some(committed.with_ordered(&events, &n).unwrap());
        let split = n.analyze().unwrap();
        assert_eq!(
            split.components[split.nodes[&p(0)].component].channels_in_use,
            1
        );
        assert_eq!(
            split.components[split.nodes[&p(3)].component].channels_in_use,
            0
        );
        assert!(split.nodes[&p(3)].channel); // last node assignment remains until tick end
    }
    #[test]
    fn committed_channels_follow_incarnation_not_coordinates() {
        let p = |x| Pos { x, y: 0, z: 0 };
        let id = |host| NodeIdentity {
            host,
            part: None,
            outer: false,
        };
        let mut n = Network::default();
        n.nodes
            .insert(p(0), Device::new(DeviceKind::CreativeEnergyCell));
        n.nodes.insert(p(1), Device::new(DeviceKind::Drive));
        n.nodes.insert(p(2), Device::new(DeviceKind::Drive));
        for x in 0..=2 {
            n.node_identities.insert(p(x), id(x as u64 + 1));
        }
        let ready = n.analyze().unwrap();
        assert!(ready.nodes[&p(1)].channel && ready.nodes[&p(2)].channel);
        let committed = CommittedChannels::capture(&n, &ready);
        n.node_identities.insert(p(2), id(42)); // same coordinate, new physical node
        n.nodes.insert(p(3), Device::new(DeviceKind::Drive));
        n.node_identities.insert(p(3), id(43));
        n.committed_channels = Some(committed);
        let pending = n.analyze().unwrap();
        let old_component = &ready.components[ready.nodes[&p(1)].component];
        let pending_component = &pending.components[pending.nodes[&p(1)].component];
        let old_channel_power = old_component.idle_per_tick
            - (n.nodes[&p(0)].idle() + n.nodes[&p(1)].idle() + n.nodes[&p(2)].idle());
        let pending_channel_power =
            pending_component.idle_per_tick - base_idle(&n, &pending_component.positions);
        assert!((pending_channel_power - old_channel_power).abs() < 1e-9);
        assert_eq!(
            pending_component.channels_in_use,
            old_component.channels_in_use
        );
        assert!(pending.nodes[&p(1)].channel);
        assert!(!pending.nodes[&p(2)].channel && !pending.nodes[&p(3)].channel);
        n.committed_channels = None;
        assert!(n.analyze().unwrap().nodes[&p(2)].channel);
    }
    #[test]
    fn observed_connections_control_channel_bottleneck_ties() {
        let p=|x|Pos{x,y:0,z:0};
        let id=|x|NodeIdentity{host:x as u64,part:None,outer:false};
        let mut n=Network::default();
        n.nodes.insert(p(0),Device::new(DeviceKind::Controller));
        n.nodes.insert(p(1),Device::new(DeviceKind::GlassCable));
        n.nodes.insert(p(-1),Device::new(DeviceKind::CreativeEnergyCell));
        for x in 10..19 {n.nodes.insert(p(x*10),Device::new(DeviceKind::Drive));n.connect(p(1),p(x*10)).unwrap();}
        let mut order=service_events::OrderedServices::default();
        for (index,pos) in n.nodes.keys().copied().enumerate(){
            let node=id(index+1); n.node_identities.insert(pos,node);order.add_node(node,None).unwrap();order.ensure_grid(node).unwrap();
        }
        let identities=n.node_identities.clone();
        let identity=|pos|identities[&p(pos)];
        order.connect(identity(0),identity(1)).unwrap();
        order.connect(identity(0),identity(-1)).unwrap();
        for x in (10..19).rev(){order.connect(identity(1),identity(x*10)).unwrap();}
        n.record_connection_order(&order).unwrap();
        let observed=n.analyze().unwrap();
        assert!(!observed.nodes[&p(100)].channel);
        assert!(observed.nodes[&p(180)].channel);
        n.observed_connection_order.clear();
        let coordinates=n.analyze().unwrap();
        assert!(coordinates.nodes[&p(100)].channel);
        assert!(!coordinates.nodes[&p(180)].channel);
        order.disconnect(identity(1),identity(100)).unwrap();
        assert_eq!(n.record_connection_order(&order),Err("ordered pathing edges differ from live graph"));
    }
    #[test]
    fn forced_mode_follows_each_grid_node_and_service_can_unlock() {
        let p = |x| Pos { x, y: 0, z: 0 };
        let id = |host| NodeIdentity {
            host,
            part: None,
            outer: false,
        };
        let mut n = Network::default();
        n.nodes
            .insert(p(0), Device::new(DeviceKind::CreativeEnergyCell));
        n.nodes.insert(p(1), Device::new(DeviceKind::Drive));
        n.nodes.get_mut(&p(1)).unwrap().channel_mode_override = Some(ChannelMode::X3);
        n.nodes.insert(p(2), Device::new(DeviceKind::Drive));
        for x in 0..=2 {
            n.node_identities.insert(p(x), id(x as u64 + 1));
        }
        let mut committed = CommittedChannels::capture(&n, &n.analyze().unwrap());
        assert_eq!(committed.forced_mode(id(3)), Some(ChannelMode::X3));
        committed.clear_forced_mode(id(1));
        assert_eq!(committed.forced_mode(id(3)), None);
    }
    #[test]
    fn committed_part_channel_tracks_uuid_on_same_face() {
        let p = Pos { x: 1, y: 0, z: 0 };
        let mut n = Network::default();
        n.nodes.insert(
            Pos { x: 0, y: 0, z: 0 },
            Device::new(DeviceKind::CreativeEnergyCell),
        );
        let mut cable = Device::new(DeviceKind::GlassCable);
        cable.attached.push(AttachedPart {
            face: 5,
            idle: 1.0,
            compressed: false,
            requires_channel: true,
        });
        n.nodes.insert(p, cable);
        let identity = |part| NodeIdentity {
            host: 9,
            part: Some([part, 1, 2, 3]),
            outer: false,
        };
        n.part_identities.insert((p, 5), identity(11));
        let initial = n.analyze().unwrap();
        assert!(initial.nodes[&p].parts[&5].channel);
        n.committed_channels = Some(CommittedChannels::capture(&n, &initial));
        n.part_identities.insert((p, 5), identity(12));
        assert!(!n.analyze().unwrap().nodes[&p].parts[&5].channel);
    }
    #[test]
    fn quartz_overlay_shares_energy_without_merging_grids_or_channels() {
        let mut network = Network::default();
        let a = Pos { x: 0, y: 0, z: 0 };
        let b = Pos { x: 100, y: 0, z: 0 };
        let mut battery = Device::new(DeviceKind::EnergyCell);
        battery.energy = 100.0;
        network.nodes.insert(a, battery);
        network.nodes.insert(b, Device::new(DeviceKind::Terminal));
        network.connect_energy(a, b).unwrap();
        let analysis = network.analyze().unwrap();
        assert_eq!(analysis.components.len(), 2);
        assert_eq!(analysis.power_components.len(), 1);
        assert_ne!(analysis.nodes[&a].component, analysis.nodes[&b].component);
        assert!(analysis.nodes[&b].online);
        assert_eq!(
            analysis.components[analysis.nodes[&b].component].channels_in_use,
            1
        );
        let idle = analysis.power_components[0].idle_per_tick;
        network.tick().unwrap();
        assert!((network.nodes[&a].energy - (100.0 - idle)).abs() < 1e-9);
        assert_eq!(network.insert_energy(b, 10.0).unwrap(), 10.0);
        assert!((network.nodes[&a].energy - (110.0 - idle)).abs() < 1e-9);
        network.energy_links.clear();
        assert!(!network.analyze().unwrap().nodes[&b].powered);
    }
    #[test]
    fn multiblock_uses_first_reached_member_and_shared_channel_status() {
        let p = |x| Pos { x, y: 0, z: 0 };
        let mut n = Network::default();
        let mut controller = Device::new(DeviceKind::Controller);
        controller.energy = 8000.0;
        n.nodes.insert(p(0), controller);
        for x in 1..=10 {
            let mut member = Device::new(DeviceKind::CraftingCpu);
            member.multiblock = Some(p(10));
            n.nodes.insert(p(x), member);
        }
        let a = n.analyze().unwrap();
        assert!(a.nodes[&p(10)].online);
        assert!(a.nodes[&p(1)].online);
        assert_eq!(a.components[0].channels_in_use, 1);
        assert_eq!(a.nodes[&p(1)].channels_used, 1);
        // PathingCalculation.propagateAssignments increments every other
        // member after the consuming member's path has been charged.
        assert_eq!(a.nodes[&p(10)].channels_used, 1);
        assert!((a.components[0].idle_per_tick - (13.0 + 2.0 / 128.0)).abs() < 1e-12);
        n.nodes.remove(&p(0));
        let mut power = Device::new(DeviceKind::CreativeEnergyCell);
        power.faces = 63;
        n.nodes.insert(Pos { x: 1, y: -1, z: 0 }, power);
        let a = n.analyze().unwrap();
        assert_eq!(a.components[0].channels_in_use, 1);
        assert!(a.nodes[&p(10)].online);
        // Without a shared cluster these ten separate devices exceed ad-hoc capacity.
        for d in n.nodes.values_mut() {
            d.multiblock = None;
        }
        assert!(!n.analyze().unwrap().nodes[&p(10)].online);
    }
    #[test]
    fn no_channel_parts_remain_online_in_a_full_adhoc_network() {
        let mut n = Network::default();
        let p = Pos { x: 0, y: 0, z: 0 };
        let mut cable = Device::new(DeviceKind::GlassCable);
        for face in 0..6 {
            cable.attached.push(AttachedPart {
                face,
                idle: 0.5,
                compressed: false,
                requires_channel: false,
            });
        }
        n.nodes.insert(p, cable);
        let mut demand = Device::new(DeviceKind::GlassCable);
        for face in 0..6 {
            demand.attached.push(AttachedPart {
                face,
                idle: 1.0,
                compressed: false,
                requires_channel: true,
            });
        }
        n.nodes.insert(Pos { x: 1, y: 0, z: 0 }, demand);
        let mut demand = Device::new(DeviceKind::GlassCable);
        for face in 0..2 {
            demand.attached.push(AttachedPart {
                face,
                idle: 1.0,
                compressed: false,
                requires_channel: true,
            });
        }
        n.nodes.insert(Pos { x: 2, y: 0, z: 0 }, demand);
        n.nodes.insert(
            Pos { x: 0, y: -1, z: 0 },
            Device::new(DeviceKind::CreativeEnergyCell),
        );
        let a = n.analyze().unwrap();
        assert_eq!(a.components[0].channels_in_use, 8);
        assert!(a.nodes[&p].parts.values().all(|s| s.online));
    }

    #[test]
    fn controller_conflict_only_disables_nodes_requiring_channels() {
        let mut network = Network::default();
        let p = |x| Pos { x, y: 0, z: 0 };
        network
            .nodes
            .insert(p(0), Device::new(DeviceKind::Controller));
        let mut cable = Device::new(DeviceKind::GlassCable);
        cable.attached.push(AttachedPart {
            face: 0,
            idle: 0.0,
            compressed: false,
            requires_channel: true,
        });
        cable.attached.push(AttachedPart {
            face: 1,
            idle: 0.0,
            compressed: false,
            requires_channel: false,
        });
        network.nodes.insert(p(1), cable);
        network
            .nodes
            .insert(p(2), Device::new(DeviceKind::Controller));
        network
            .nodes
            .insert(p(3), Device::new(DeviceKind::PoweredMachine));
        network
            .nodes
            .insert(p(4), Device::new(DeviceKind::CreativeEnergyCell));
        let analysis = network.analyze().unwrap();
        assert_eq!(analysis.components[0].controller, ControllerState::Conflict);
        assert!(!analysis.nodes[&p(1)].parts[&0].online);
        assert!(analysis.nodes[&p(1)].parts[&1].online);
        assert!(analysis.nodes[&p(3)].online);
    }
    #[test]
    fn adhoc_channel_energy_counts_attached_part_nodes() {
        let mut network = Network::default();
        let a = Pos { x: 0, y: 0, z: 0 };
        let b = Pos { x: 1, y: 0, z: 0 };
        let mut cable = Device::new(DeviceKind::GlassCable);
        cable.attached.push(AttachedPart {
            face: 0,
            idle: 0.0,
            compressed: false,
            requires_channel: true,
        });
        cable.attached.push(AttachedPart {
            face: 1,
            idle: 0.0,
            compressed: false,
            requires_channel: false,
        });
        network.nodes.insert(a, cable);
        network
            .nodes
            .insert(b, Device::new(DeviceKind::CreativeEnergyCell));
        let analysis = network.analyze().unwrap();
        assert_eq!(analysis.components[0].channels_in_use, 1);
        // Four grid nodes (cable, two parts, source) times one channel / 128.
        assert_eq!(analysis.components[0].idle_per_tick, 4.0 / 128.0);
    }
    #[test]
    fn nested_compressed_channel_invalidates_all_adhoc_demands_and_recovers() {
        let mut network = Network::default();
        let a = Pos { x: 0, y: 0, z: 0 };
        let b = Pos { x: 1, y: 0, z: 0 };
        let mut trunk = Device::new(DeviceKind::GlassCable);
        trunk.attached.push(AttachedPart {
            face: 0,
            idle: 1.0,
            compressed: true,
            requires_channel: true,
        });
        trunk.attached.push(AttachedPart {
            face: 1,
            idle: 1.0,
            compressed: false,
            requires_channel: true,
        });
        network.nodes.insert(a, trunk);
        let mut outer = Device::new(DeviceKind::CreativeEnergyCell);
        outer.cannot_carry_compressed = true;
        network.nodes.insert(b, outer);
        let invalid = network.analyze().unwrap();
        assert_eq!(invalid.components[0].channels_in_use, 0);
        assert!(!invalid.nodes[&a].parts[&0].online);
        assert!(!invalid.nodes[&a].parts[&1].online);
        network.nodes.get_mut(&b).unwrap().cannot_carry_compressed = false;
        let recovered = network.analyze().unwrap();
        assert_eq!(recovered.components[0].channels_in_use, 2);
        assert!(recovered.nodes[&a].parts[&0].online);
        assert!(recovered.nodes[&a].parts[&1].online);
    }
    #[test]
    fn compressed_parts_cannot_route_through_me_outer_nodes() {
        let mut n = Network::default();
        let root = Pos { x: 0, y: 0, z: 0 };
        let outer = Pos { x: 1, y: 0, z: 0 };
        let endpoint = Pos { x: 2, y: 0, z: 0 };
        let mut controller = Device::new(DeviceKind::Controller);
        controller.energy = 8000.0;
        n.nodes.insert(root, controller);
        let mut tunnel = Device::new(DeviceKind::DenseCable);
        tunnel.cannot_carry_compressed = true;
        n.nodes.insert(outer, tunnel);
        let mut cable = Device::new(DeviceKind::GlassCable);
        cable.attached.push(AttachedPart {
            face: 0,
            idle: 2.0,
            compressed: true,
            requires_channel: true,
        });
        cable.attached.push(AttachedPart {
            face: 2,
            idle: 0.5,
            compressed: false,
            requires_channel: true,
        });
        n.nodes.insert(endpoint, cable);
        let a = n.analyze().unwrap();
        assert!(!a.nodes[&endpoint].parts[&0].online);
        assert!(a.nodes[&endpoint].parts[&2].online);
    }

    #[test]
    fn explicit_quantum_link_routes_and_disconnects_without_moving_energy() {
        let mut n = Network::default();
        let a = Pos { x: 0, y: 0, z: 0 };
        let b = Pos { x: 100, y: 0, z: 0 };
        n.nodes.insert(a, Device::new(DeviceKind::Controller));
        n.nodes.insert(
            Pos { x: 1, y: 0, z: 0 },
            Device::new(DeviceKind::DenseCable),
        );
        n.nodes.insert(b, Device::new(DeviceKind::DenseCable));
        n.nodes
            .insert(Pos { x: 101, y: 0, z: 0 }, Device::new(DeviceKind::Drive));
        n.insert_energy(a, 8000.0).unwrap();
        assert!(!n.analyze().unwrap().nodes[&Pos { x: 101, y: 0, z: 0 }].online);
        n.connect(Pos { x: 1, y: 0, z: 0 }, b).unwrap();
        assert!(n.analyze().unwrap().nodes[&Pos { x: 101, y: 0, z: 0 }].online);
        n.links.clear();
        assert_eq!(n.nodes[&a].energy, 8000.0);
        assert!(!n.analyze().unwrap().nodes[&Pos { x: 101, y: 0, z: 0 }].online);
    }
    #[test]
    fn creative_source_and_dynamic_machine_flags_are_not_fake_buffers() {
        let mut n = Network::default();
        let a = Pos { x: 0, y: 0, z: 0 };
        let b = Pos { x: 1, y: 0, z: 0 };
        n.nodes
            .insert(a, Device::new(DeviceKind::CreativeEnergyCell));
        let mut d = Device::new(DeviceKind::Interface);
        d.channel_override = Some(false);
        d.idle_override = Some(8.0);
        n.nodes.insert(b, d);
        for _ in 0..20 {
            let state = n.tick().unwrap();
            assert!(state.nodes[&b].online);
            assert_eq!(state.components[0].channels_in_use, 0);
        }
        assert_eq!(n.nodes[&a].energy, 0.0);
    }
    fn p(x: i32, y: i32, z: i32) -> Pos {
        Pos { x, y, z }
    }
    fn put(n: &mut Network, pos: Pos, kind: DeviceKind) {
        n.nodes.insert(pos, Device::new(kind));
    }
    #[test]
    fn cable_parts_consume_channels_without_breaking_trunk() {
        let mut n = Network::default();
        put(&mut n, p(0, 0, 0), DeviceKind::Controller);
        put(&mut n, p(1, 0, 0), DeviceKind::GlassCable);
        put(&mut n, p(2, 0, 0), DeviceKind::GlassCable);
        put(&mut n, p(3, 0, 0), DeviceKind::Drive);
        n.nodes
            .get_mut(&p(1, 0, 0))
            .unwrap()
            .attached
            .push(AttachedPart {
                face: 2,
                idle: 0.5,
                compressed: false,
                requires_channel: true,
            });
        n.nodes
            .get_mut(&p(2, 0, 0))
            .unwrap()
            .attached
            .push(AttachedPart {
                face: 2,
                idle: 1.0,
                compressed: false,
                requires_channel: true,
            });
        n.insert_energy(p(0, 0, 0), 1000.0).unwrap();
        let a = n.analyze().unwrap();
        assert_eq!(a.components[0].channels_in_use, 3);
        assert!(a.nodes[&p(3, 0, 0)].online);
        assert!(a.nodes[&p(1, 0, 0)].parts[&2].online);
        assert!(a.nodes[&p(2, 0, 0)].parts[&2].online);
        assert_eq!(a.nodes[&p(1, 0, 0)].channels_used, 3);
    }
    #[test]
    fn attached_parts_obey_ad_hoc_limit_and_machine_uses_no_channel() {
        let mut n = Network::default();
        put(&mut n, p(0, 0, 0), DeviceKind::EnergyCell);
        for x in 1..=3 {
            put(&mut n, p(x, 0, 0), DeviceKind::GlassCable);
        }
        put(&mut n, p(4, 0, 0), DeviceKind::PoweredMachine);
        for face in 0..6 {
            n.nodes
                .get_mut(&p(1, 0, 0))
                .unwrap()
                .attached
                .push(AttachedPart {
                    face,
                    idle: 1.0,
                    compressed: false,
                    requires_channel: true,
                });
        }
        for face in 0..3 {
            n.nodes
                .get_mut(&p(2, 0, 0))
                .unwrap()
                .attached
                .push(AttachedPart {
                    face,
                    idle: 1.0,
                    compressed: false,
                    requires_channel: true,
                });
        }
        n.insert_energy(p(0, 0, 0), 1000.0).unwrap();
        let a = n.analyze().unwrap();
        assert_eq!(a.components[0].channels_in_use, 0);
        assert!(!a.nodes[&p(1, 0, 0)].parts[&0].online);
        assert!(a.nodes[&p(4, 0, 0)].online);
        n.nodes.get_mut(&p(2, 0, 0)).unwrap().attached.pop();
        let a = n.analyze().unwrap();
        assert_eq!(a.components[0].channels_in_use, 8);
        assert!(a.nodes[&p(2, 0, 0)].parts[&1].online);
    }
    #[test]
    fn channel_modes_scale_adhoc_and_choose_most_permissive_forced_mode() {
        for mode in [
            ChannelMode::Default,
            ChannelMode::X2,
            ChannelMode::X3,
            ChannelMode::X4,
            ChannelMode::Infinite,
        ] {
            assert_eq!(
                ChannelMode::from_source_name(mode.source_name()),
                Some(mode)
            );
        }
        assert_eq!(ChannelMode::from_source_name("unknown"), None);
        let mut n = Network::default();
        put(&mut n, p(0, 0, 0), DeviceKind::EnergyCell);
        for x in 1..=9 {
            put(&mut n, p(x, 0, 0), DeviceKind::Drive);
        }
        n.insert_energy(p(0, 0, 0), 1000.0).unwrap();
        assert_eq!(n.analyze().unwrap().components[0].channels_in_use, 0);
        n.default_channel_mode = ChannelMode::X2;
        assert_eq!(n.analyze().unwrap().components[0].channels_in_use, 9);
        n.nodes.get_mut(&p(2, 0, 0)).unwrap().channel_mode_override = Some(ChannelMode::Default);
        assert_eq!(n.analyze().unwrap().components[0].channels_in_use, 0);
        n.nodes.get_mut(&p(3, 0, 0)).unwrap().channel_mode_override = Some(ChannelMode::X3);
        let status = n.analyze().unwrap();
        assert_eq!(status.components[0].channel_mode, ChannelMode::X3);
        assert_eq!(status.components[0].channels_in_use, 9);
        let mut cable = Device::new(DeviceKind::GlassCable);
        cable.attached.push(AttachedPart {
            face: 2,
            idle: 0.0,
            compressed: false,
            requires_channel: true,
        });
        cable.part_channel_modes.insert(2, ChannelMode::Infinite);
        n.nodes.insert(p(10, 0, 0), cable);
        assert_eq!(
            n.analyze().unwrap().components[0].channel_mode,
            ChannelMode::Infinite
        );
        n.nodes
            .get_mut(&p(10, 0, 0))
            .unwrap()
            .part_channel_modes
            .clear();
        let restored: Network = serde_json::from_str(&serde_json::to_string(&n).unwrap()).unwrap();
        assert_eq!(
            restored.analyze().unwrap().components[0].channel_mode,
            ChannelMode::X3
        );
    }
    #[test]
    fn controller_trunk_channel_modes_scale_capacity() {
        let mut n = Network::default();
        put(&mut n, p(0, 0, 0), DeviceKind::Controller);
        for x in 1..=17 {
            put(&mut n, p(x, 0, 0), DeviceKind::GlassCable);
            n.nodes
                .get_mut(&p(x, 0, 0))
                .unwrap()
                .attached
                .push(AttachedPart {
                    face: 2,
                    idle: 0.0,
                    compressed: false,
                    requires_channel: true,
                });
        }
        n.insert_energy(p(0, 0, 0), 8000.0).unwrap();
        n.default_channel_mode = ChannelMode::X2;
        assert_eq!(n.analyze().unwrap().components[0].channels_in_use, 16);
        n.default_channel_mode = ChannelMode::X3;
        assert_eq!(n.analyze().unwrap().components[0].channels_in_use, 17);
        n.default_channel_mode = ChannelMode::Infinite;
        assert_eq!(n.analyze().unwrap().components[0].channels_in_use, 17);
        assert_eq!(ChannelMode::Infinite.capacity(0), 0);
    }
    #[test]
    fn saturated_controller_trunk_leaves_cable_online_and_overflow_part_offline() {
        let mut n = Network::default();
        put(&mut n, p(0, 0, 0), DeviceKind::Controller);
        put(&mut n, p(1, 0, 0), DeviceKind::GlassCable);
        put(&mut n, p(2, 0, 0), DeviceKind::GlassCable);
        for face in 0..6 {
            n.nodes
                .get_mut(&p(1, 0, 0))
                .unwrap()
                .attached
                .push(AttachedPart {
                    face,
                    idle: 1.0,
                    compressed: false,
                    requires_channel: true,
                });
        }
        for face in 0..3 {
            n.nodes
                .get_mut(&p(2, 0, 0))
                .unwrap()
                .attached
                .push(AttachedPart {
                    face,
                    idle: 1.0,
                    compressed: false,
                    requires_channel: true,
                });
        }
        n.insert_energy(p(0, 0, 0), 1000.0).unwrap();
        let a = n.analyze().unwrap();
        assert_eq!(a.components[0].channels_in_use, 8);
        assert!(a.nodes[&p(2, 0, 0)].online);
        assert!(a.nodes[&p(2, 0, 0)].parts[&0].online);
        assert!(!a.nodes[&p(2, 0, 0)].parts[&2].online);
    }
    #[test]
    fn invalid_or_duplicate_attached_faces_are_rejected() {
        let mut n = Network::default();
        put(&mut n, p(0, 0, 0), DeviceKind::GlassCable);
        let d = n.nodes.get_mut(&p(0, 0, 0)).unwrap();
        d.attached = vec![
            AttachedPart {
                face: 0,
                idle: 1.0,
                compressed: false,
                requires_channel: true,
            },
            AttachedPart {
                face: 0,
                idle: 1.0,
                compressed: false,
                requires_channel: true,
            },
        ];
        assert!(n.validate().is_err());
    }
    #[test]
    fn directly_adjacent_exposed_machines_connect_and_route_channels() {
        let mut n = Network::default();
        put(&mut n, p(0, 0, 0), DeviceKind::Controller);
        put(&mut n, p(1, 0, 0), DeviceKind::EnergyCell);
        put(&mut n, p(2, 0, 0), DeviceKind::Drive);
        put(&mut n, p(3, 0, 0), DeviceKind::Terminal);
        n.insert_energy(p(0, 0, 0), 1000.0).unwrap();
        let a = n.analyze().unwrap();
        assert_eq!(a.components.len(), 1);
        assert!(a.nodes[&p(2, 0, 0)].online);
        assert!(a.nodes[&p(3, 0, 0)].online);
        assert_eq!(a.nodes[&p(1, 0, 0)].channels_used, 2);
        // Host applies the drive's actual non-connectable front face.
        n.nodes.get_mut(&p(2, 0, 0)).unwrap().faces &= !2;
        let a = n.analyze().unwrap();
        assert_eq!(a.components.len(), 2);
        assert!(!a.nodes[&p(2, 0, 0)].online);
    }
    #[test]
    fn ad_hoc_ninth_channel_disables_all_and_recovers() {
        let mut n = Network::default();
        for x in 0..9 {
            put(&mut n, p(x, 0, 0), DeviceKind::NormalCable);
            put(&mut n, p(x, 1, 0), DeviceKind::Terminal);
        }
        put(&mut n, p(0, -1, 0), DeviceKind::EnergyCell);
        assert_eq!(n.insert_energy(p(0, -1, 0), 1000.0).unwrap(), 1000.0);
        let a = n.analyze().unwrap();
        assert_eq!(a.components[0].controller, ControllerState::None);
        assert_eq!(a.components[0].channels_in_use, 0);
        assert!(!a.nodes[&p(0, 1, 0)].online);
        n.nodes.remove(&p(8, 1, 0));
        let a = n.analyze().unwrap();
        assert_eq!(a.components[0].channels_in_use, 8);
        assert!(a.nodes[&p(0, 1, 0)].online);
    }
    #[test]
    fn normal_trunk_limits_channels_and_dense_trunk_expands_them() {
        let mut n = Network::default();
        put(&mut n, p(0, 0, 0), DeviceKind::Controller);
        for x in 1..=10 {
            put(&mut n, p(x, 0, 0), DeviceKind::NormalCable);
            put(&mut n, p(x, 1, 0), DeviceKind::Terminal);
        }
        n.insert_energy(p(0, 0, 0), 1000.0).unwrap();
        let a = n.analyze().unwrap();
        assert_eq!(a.components[0].channels_in_use, 8);
        assert_eq!(a.nodes[&p(1, 0, 0)].channels_used, 8);
        for x in 1..=10 {
            n.nodes.get_mut(&p(x, 0, 0)).unwrap().kind = DeviceKind::DenseCable;
        }
        assert_eq!(n.analyze().unwrap().components[0].channels_in_use, 10);
    }
    #[test]
    fn invalid_controller_cross_and_separated_controllers_conflict() {
        let mut n = Network::default();
        for (x, y) in [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)] {
            put(&mut n, p(x, y, 0), DeviceKind::Controller);
        }
        assert_eq!(
            n.analyze().unwrap().components[0].controller,
            ControllerState::Conflict
        );
        n.nodes.clear();
        put(&mut n, p(0, 0, 0), DeviceKind::Controller);
        put(&mut n, p(1, 0, 0), DeviceKind::NormalCable);
        put(&mut n, p(2, 0, 0), DeviceKind::Controller);
        assert_eq!(
            n.analyze().unwrap().components[0].controller,
            ControllerState::Conflict
        );
    }
    #[test]
    fn energy_is_conserved_and_round_trip_validated() {
        let mut n = Network::default();
        put(&mut n, p(0, 0, 0), DeviceKind::EnergyCell);
        put(&mut n, p(1, 0, 0), DeviceKind::NormalCable);
        put(&mut n, p(2, 0, 0), DeviceKind::Terminal);
        assert_eq!(n.insert_energy(p(0, 0, 0), 200001.0).unwrap(), 200000.0);
        let serialized = serde_json::to_string(&n).unwrap();
        let mut restored: Network = serde_json::from_str(&serialized).unwrap();
        let before = restored.analyze().unwrap().components[0].clone();
        assert!(restored.analyze().unwrap().nodes[&p(2, 0, 0)].online);
        restored.tick().unwrap();
        let after = restored.nodes[&p(0, 0, 0)].energy;
        assert!((before.energy - after - before.idle_per_tick).abs() < 1e-8);
        restored.nodes.get_mut(&p(0, 0, 0)).unwrap().energy = -1.0;
        assert!(restored.analyze().is_err());
    }
}

/// Source-derived adaptive vibration chamber fuel-rate state. The host owns the
/// actual fuel inventory and calls `add_fuel` only after consuming an item.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VibrationChamber {
    pub remaining_fuel_ticks: f64,
    pub current_fuel_ticks_per_tick: f64,
    pub energy_cards: u8,
    pub speed_cards: u8,
    pub base_energy_per_fuel_tick: f64,
    pub min_energy_per_tick: f64,
    pub base_max_energy_per_tick: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Generation {
    pub fuel_consumed: f64,
    pub generated: f64,
    pub accepted: f64,
}
impl Default for VibrationChamber {
    fn default() -> Self {
        Self {
            remaining_fuel_ticks: 0.0,
            current_fuel_ticks_per_tick: 1.0,
            energy_cards: 0,
            speed_cards: 0,
            base_energy_per_fuel_tick: 5.0,
            min_energy_per_tick: 4.0,
            base_max_energy_per_tick: 40.0,
        }
    }
}
impl VibrationChamber {
    fn rates(&self) -> Result<(f64, f64, f64), &'static str> {
        let per = self.base_energy_per_fuel_tick * (1.0 + f64::from(self.energy_cards) / 2.0);
        let max = self.base_max_energy_per_tick * (1.0 + f64::from(self.speed_cards) / 2.0);
        if !per.is_finite()
            || per <= 0.0
            || !max.is_finite()
            || !self.min_energy_per_tick.is_finite()
            || self.min_energy_per_tick < 0.0
            || self.base_max_energy_per_tick < self.min_energy_per_tick
            || max < self.min_energy_per_tick
            || !self.remaining_fuel_ticks.is_finite()
            || self.remaining_fuel_ticks < 0.0
            || !self.current_fuel_ticks_per_tick.is_finite()
            || self.current_fuel_ticks_per_tick < 0.0
            || self.energy_cards.saturating_add(self.speed_cards) > 3
        {
            return Err("invalid vibration chamber state");
        }
        Ok((per, self.min_energy_per_tick / per, max / per))
    }
    pub fn add_fuel(&mut self, burn_ticks: u32) -> Result<(), &'static str> {
        self.rates()?;
        let total = self.remaining_fuel_ticks + f64::from(burn_ticks);
        if !total.is_finite() {
            return Err("fuel tick overflow");
        }
        self.remaining_fuel_ticks = total;
        Ok(())
    }
    /// Advance a scheduled AE2 machine tick; overflow is voided as in AE2.
    /// `elapsed` is the actual elapsed game ticks, not a call frequency.
    pub fn advance(
        &mut self,
        network: &mut Network,
        at: Pos,
        elapsed: u32,
    ) -> Result<Generation, &'static str> {
        let (per, min, max) = self.rates()?;
        if network.nodes.get(&at).map(|d| d.kind) != Some(DeviceKind::VibrationChamber) {
            return Err("no vibration chamber at position");
        }
        network.validate()?;
        if self.remaining_fuel_ticks <= 0.0 {
            self.current_fuel_ticks_per_tick = self
                .base_energy_per_fuel_tick
                .clamp(self.min_energy_per_tick, self.base_max_energy_per_tick)
                / per;
            return Ok(Generation {
                fuel_consumed: 0.0,
                generated: 0.0,
                accepted: 0.0,
            });
        }
        let consumed =
            (f64::from(elapsed) * self.current_fuel_ticks_per_tick).min(self.remaining_fuel_ticks);
        let generated = consumed * per;
        // Test network state before mutating fuel, and leave it unchanged on errors.
        let accepted = network.insert_energy(
            at,
            if consumed.abs() < 0.01 {
                0.0
            } else {
                generated
            },
        )?;
        self.remaining_fuel_ticks -= consumed;
        let step = f64::from(elapsed) * (max - min) / 100.0;
        if consumed.abs() < 0.01 {
            // AE2 probes whether the network could accept 1 AE before speeding up.
            let group = network
                .components()
                .into_iter()
                .find(|g| g.contains(&at))
                .unwrap();
            let free: f64 = group
                .iter()
                .map(|p| {
                    let d = &network.nodes[p];
                    d.kind.energy_capacity() - d.energy
                })
                .sum();
            if free >= 1.0 {
                self.current_fuel_ticks_per_tick =
                    (self.current_fuel_ticks_per_tick + step).clamp(min, max);
            }
        } else {
            self.current_fuel_ticks_per_tick = (self.current_fuel_ticks_per_tick
                + if accepted < generated { -step } else { step })
            .clamp(min, max);
        }
        Ok(Generation {
            fuel_consumed: consumed,
            generated,
            accepted,
        })
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    fn p(x: i32) -> Pos {
        Pos { x, y: 0, z: 0 }
    }
    #[test]
    fn colored_faces_and_backward_compatibility() {
        let old = r#"{"kind":"normal_cable","energy":0.0}"#;
        let mut left: Device = serde_json::from_str(old).unwrap();
        assert_eq!(left.faces, 63);
        assert_eq!(left.color, CableColor::Transparent);
        let mut n = Network::default();
        left.color = CableColor::Red;
        n.nodes.insert(p(0), left);
        let mut right = Device::new(DeviceKind::SmartCable);
        right.color = CableColor::Blue;
        n.nodes.insert(p(1), right);
        assert_eq!(n.analyze().unwrap().components.len(), 2);
        n.nodes.get_mut(&p(1)).unwrap().color = CableColor::Transparent;
        assert_eq!(n.analyze().unwrap().components.len(), 1);
        n.nodes.get_mut(&p(1)).unwrap().faces &= !2;
        assert_eq!(n.analyze().unwrap().components.len(), 2);
    }
    #[test]
    fn chamber_consumes_fuel_and_adapts_when_buffer_fills() {
        let mut n = Network::default();
        n.nodes
            .insert(p(0), Device::new(DeviceKind::VibrationChamber));
        n.nodes.insert(p(1), Device::new(DeviceKind::NormalCable));
        n.nodes.insert(p(2), Device::new(DeviceKind::EnergyCell));
        let mut c = VibrationChamber::default();
        c.add_fuel(100).unwrap();
        let result = c.advance(&mut n, p(0), 1).unwrap();
        assert_eq!(result.fuel_consumed, 1.0);
        assert_eq!(result.accepted, 5.0);
        assert!(c.current_fuel_ticks_per_tick > 1.0);
        n.insert_energy(p(0), 200000.0).unwrap();
        let prev = c.current_fuel_ticks_per_tick;
        let result = c.advance(&mut n, p(0), 1).unwrap();
        assert_eq!(result.accepted, 0.0);
        assert!(c.current_fuel_ticks_per_tick < prev);
        assert!(c.remaining_fuel_ticks < 99.0);
        let saved = serde_json::to_string(&c).unwrap();
        let recovered: VibrationChamber = serde_json::from_str(&saved).unwrap();
        assert_eq!(recovered.remaining_fuel_ticks, c.remaining_fuel_ticks);
    }
}

/// A P2P service address on one *carrier grid*. This is a lookup primitive,
/// not the inner/outer ME grid connection or transport simulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum P2pKind {
    Me,
    Item,
    Fluid,
    Fe,
    Redstone,
    Light,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct P2pEndpoint {
    pub pos: Pos,
    pub frequency: u16,
    pub kind: P2pKind,
    pub output: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct P2pService {
    pub endpoints: Vec<P2pEndpoint>,
}
impl P2pService {
    /// Output endpoints linked to the sole matching input, filtered by tunnel type.
    /// The host must scope this service to a carrier grid and check live channel/power.
    pub fn outputs(&self, input: Pos) -> Result<Vec<Pos>, &'static str> {
        let Some(i) = self.endpoints.iter().find(|e| e.pos == input && !e.output) else {
            return Err("missing input");
        };
        if i.frequency == 0 {
            return Ok(Vec::new());
        }
        if self
            .endpoints
            .iter()
            .filter(|e| !e.output && e.frequency == i.frequency)
            .count()
            != 1
        {
            return Err("duplicate P2P input frequency");
        }
        let mut output: Vec<_> = self
            .endpoints
            .iter()
            .filter(|e| e.output && e.frequency == i.frequency && e.kind == i.kind)
            .map(|e| e.pos)
            .collect();
        output.sort();
        output.dedup();
        Ok(output)
    }
}

#[cfg(test)]
mod p2p_tests {
    use super::*;
    #[test]
    fn outputs_require_frequency_and_type_match() {
        let p = |x| Pos { x, y: 0, z: 0 };
        let mut service = P2pService {
            endpoints: vec![
                P2pEndpoint {
                    pos: p(0),
                    frequency: 42,
                    kind: P2pKind::Item,
                    output: false,
                },
                P2pEndpoint {
                    pos: p(1),
                    frequency: 42,
                    kind: P2pKind::Item,
                    output: true,
                },
                P2pEndpoint {
                    pos: p(2),
                    frequency: 42,
                    kind: P2pKind::Fluid,
                    output: true,
                },
            ],
        };
        assert_eq!(service.outputs(p(0)).unwrap(), vec![p(1)]);
        service.endpoints.push(P2pEndpoint {
            pos: p(3),
            frequency: 42,
            kind: P2pKind::Item,
            output: false,
        });
        assert!(service.outputs(p(0)).is_err());
    }
}
