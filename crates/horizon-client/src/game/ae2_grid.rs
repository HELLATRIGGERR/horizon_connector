//! Authoritative AE2 world integration. Storage lives in physical drive cells;
//! the graph is rebuilt from loaded persistent machines, not a global sidecar.
use super::{Game, ae2_lab};
use ae2_cell_core::ItemKey;
use ae2_network_core::{AttachedPart, Device, DeviceKind, Network, Pos};
use horizon_client::{
    ae2_world::{Machine, MachineKind},
    containers::BlockEntity,
    items::ItemStack,
};
use horizon_world::nbt::{JavaString, Tag};
use std::collections::BTreeMap;
type WorldPos = (i32, i32, i32);
fn np(p: WorldPos) -> Pos {
    Pos {
        x: p.0,
        y: p.1,
        z: p.2,
    }
}
fn wp(p: Pos) -> WorldPos {
    (p.x, p.y, p.z)
}
pub(super) fn face_index(face: &str) -> Option<u8> {
    Some(match face {
        "east" => 0,
        "west" => 1,
        "up" => 2,
        "down" => 3,
        "south" => 4,
        "north" => 5,
        _ => return None,
    })
}
pub(super) fn kind(m: &Machine) -> Option<DeviceKind> {
    Some(match m.kind {
        MachineKind::Controller => DeviceKind::Controller,
        MachineKind::EnergyAcceptor => DeviceKind::EnergyAcceptor,
        MachineKind::EnergyCell => DeviceKind::EnergyCell,
        MachineKind::DenseEnergyCell => DeviceKind::DenseEnergyCell,
        MachineKind::CreativeEnergyCell => DeviceKind::CreativeEnergyCell,
        MachineKind::VibrationChamber => DeviceKind::VibrationChamber,
        MachineKind::Drive | MachineKind::Chest => DeviceKind::Drive,
        MachineKind::CableBus => DeviceKind::GlassCable,
        MachineKind::Inscriber | MachineKind::Charger | MachineKind::MolecularAssembler => {
            DeviceKind::PoweredMachine
        }
        MachineKind::DebugPhantomNode => DeviceKind::PoweredMachine,
        MachineKind::GrowthAccelerator | MachineKind::CrystalResonanceGenerator => {
            DeviceKind::PoweredMachine
        }
        MachineKind::QuantumRing | MachineKind::QuantumLink => DeviceKind::DenseCable,
        MachineKind::Terminal | MachineKind::CraftingTerminal => DeviceKind::Terminal,
        MachineKind::CraftingUnit
        | MachineKind::CraftingAccelerator
        | MachineKind::CraftingStorage1k
        | MachineKind::CraftingStorage4k
        | MachineKind::CraftingStorage16k
        | MachineKind::CraftingStorage64k
        | MachineKind::CraftingStorage256k => DeviceKind::CraftingCpu,
        MachineKind::Interface
        | MachineKind::PatternProvider
        | MachineKind::WirelessAccessPoint
        | MachineKind::IoPort
        | MachineKind::SpatialIoPort
        | MachineKind::SpatialPylon
        | MachineKind::SpatialAnchor
        | MachineKind::CraftingMonitor => DeviceKind::Interface,
        _ => return None,
    })
}
impl Game<'_> {
    pub(super) fn ae2_try_attach_part(&mut self, pos: WorldPos, face: &str) -> bool {
        if !matches!(
            self.held().map(|s| s.item.as_ref()),
            Some(
                "ae2:terminal"
                    | "ae2:crafting_terminal"
                    | "ae2:pattern_encoding_terminal"
                    | "ae2:pattern_access_terminal"
                    | "ae2:import_bus"
                    | "ae2:export_bus"
                    | "ae2:storage_bus"
                    | "ae2:level_emitter"
                    | "ae2:energy_level_emitter"
                    | "ae2:toggle_bus"
                    | "ae2:inverted_toggle_bus"
                    | "ae2:annihilation_plane"
                    | "ae2:formation_plane"
                    | "ae2:me_p2p_tunnel"
                    | "ae2:item_p2p_tunnel"
                    | "ae2:fluid_p2p_tunnel"
                    | "ae2:fe_p2p_tunnel"
                    | "ae2:redstone_p2p_tunnel"
                    | "ae2:light_p2p_tunnel"
                    | "ae2:cable_energy_acceptor"
                    | "ae2:cable_anchor"
                    | "ae2:quartz_fiber"
                    | "ae2:monitor"
                    | "ae2:semi_dark_monitor"
                    | "ae2:dark_monitor"
                    | "ae2:storage_monitor"
                    | "ae2:conversion_monitor"
                    | "ae2:cable_interface"
                    | "ae2:cable_pattern_provider"
            )
        ) {
            return false;
        }
        let mut target = pos;
        let mut target_face = face;
        let mut created = false;
        if !matches!(self.world.block_entities.get(&pos),Some(BlockEntity::Ae2(m))if m.kind==MachineKind::CableBus) {
            let (delta, opposite) = match face {
                "east" => ([1,0,0], "west"), "west" => ([-1,0,0], "east"),
                "up" => ([0,1,0], "down"), "down" => ([0,-1,0], "up"),
                "south" => ([0,0,1], "north"), "north" => ([0,0,-1], "south"),
                _ => return false,
            };
            let Some(x) = pos.0.checked_add(delta[0]) else { return false };
            let Some(y) = pos.1.checked_add(delta[1]) else { return false };
            let Some(z) = pos.2.checked_add(delta[2]) else { return false };
            target = (x,y,z);
            target_face = opposite;
            if !matches!(self.world.block_entities.get(&target),Some(BlockEntity::Ae2(m))if m.kind==MachineKind::CableBus) {
                let name = &self.res.facts().get(self.world.block(x,y,z)).name;
                if !matches!(name.as_ref(), "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air") { return false; }
                if !self.set_state_named([x,y,z], "ae2:cable_bus", &[]).unwrap_or(false) { return false; }
                let mut machine = Machine::new(MachineKind::CableBus);
                machine.config.insert("ae2_centerless".into(), Tag::Byte(1));
                self.world.block_entities.insert(target, BlockEntity::Ae2(Box::new(machine)));
                self.world.modified.insert((x >> 4,z >> 4));
                created = true;
            }
        }
        let xyz = [target.0.to_string(), target.1.to_string(), target.2.to_string()];
        if let Err(error) = self.ae2_grid_command(&["part", &xyz[0], &xyz[1], &xyz[2], target_face]) {
            if created { let _ = self.set_state_named([target.0,target.1,target.2], "minecraft:air", &[]); }
            self.command_failure(horizon_client::text::Text::literal(format!("AE2 {error}")));
        }
        true
    }
    pub(super) fn ae2_has_terminal(&self, pos: WorldPos) -> bool {
        matches!(self.world.block_entities.get(&pos),Some(BlockEntity::Ae2(m)) if matches!(m.kind,MachineKind::Terminal|MachineKind::CraftingTerminal|MachineKind::Chest)||m.config.get(&JavaString::from("terminal_part"))==Some(&Tag::Byte(1)))
    }
    pub(super) fn ae2_network(&self) -> Network {
        let mut n = Network::default();
        // In the source-callback mode, gameplay uses the same CPU faces and
        // formed-cluster identity as the ordered service observer.
        let cpu_snapshot = self.ae2_cpu_network_snapshot();
        let mut centerless_index = 0i32;
        n.default_channel_mode = super::ae2_channels::common_channel_mode();
        // Even the intermediate carrier analysis used by P2P observes the
        // same last end-tick channel assignments as every grid consumer.
        n.committed_channels = Some(self.ae2_committed_channels.clone());
        for (dim, world) in self.ae2_loaded_worlds() {
            for (&p, be) in world.block_entities.iter() {
                if let BlockEntity::Ae2(m) = be {
                    let actual = &self.res.facts().get(world.block(p.0, p.1, p.2)).name;
                    if Machine::for_block(actual).is_none() {
                        continue;
                    }
                    let Some(device_kind) = kind(m) else {
                        continue;
                    };
                    let mut d = Device::new(device_kind);
                    let Some(address) = self.ae2_dimension_address(dim, p) else { continue };
                    let Some(host_identity) = world.block_entities.incarnation(&p) else { continue };
                    let network_pos = address.network_pos();
                    let identity = ae2_network_core::NodeIdentity {
                        host: host_identity, part: None, outer: false,
                    };
                    if m.kind == MachineKind::DebugPhantomNode {
                        // onReady creates both nodes before gameplay exposes the proxy.
                        if !self.ae2_phantom_is_ready(identity, network_pos) { continue; }
                        d.idle_override = Some(1.0);
                    }
                    if matches!(m.kind, MachineKind::CraftingUnit | MachineKind::CraftingAccelerator | MachineKind::CraftingMonitor
                        | MachineKind::CraftingStorage1k | MachineKind::CraftingStorage4k | MachineKind::CraftingStorage16k
                        | MachineKind::CraftingStorage64k | MachineKind::CraftingStorage256k) {
                        if let Some(snapshot) = &cpu_snapshot {
                            let Some((faces, group)) = snapshot.as_ref().ok().and_then(|nodes| nodes.get(&identity)) else {
                                // No initialized source node, or observation is
                                // unavailable: do not invent its connectivity.
                                continue;
                            };
                            d.faces = *faces; d.multiblock = *group;
                        }
                    }
                    n.node_identities.insert(network_pos, identity);
                    d.channel_mode_override = super::ae2_channels::merge_mode(
                        super::ae2_channels::node_mode(&m.config, if m.kind == MachineKind::DebugPhantomNode { "proxy" } else { "gn" }),
                        self.ae2_committed_channels.forced_mode(identity),
                    );
                    d.energy = m.energy.min(d.kind.energy_capacity()).max(0.0);
                    if matches!(m.kind, MachineKind::Drive | MachineKind::Chest) {
                        let cell_idle = m
                            .slots
                            .iter()
                            .flatten()
                            .filter_map(super::ae2_storage_devices::cell_idle_drain)
                            .sum::<f64>();
                        d.idle_override = Some(
                            if m.kind == MachineKind::Drive {
                                2.0
                            } else {
                                1.0
                            } + cell_idle,
                        );
                    }
                    if m.kind == MachineKind::GrowthAccelerator {
                        d.idle_override = Some(8.0);
                        // GrowthAcceleratorBlockEntity exposes its grid node on
                        // FRONT and BACK only. Older saves without a facing
                        // property retain the source default north/south axis.
                        let facing = self.pack.assets.registry.state(world.block(p.0, p.1, p.2))
                            .and_then(|state| state.properties.get("facing"))
                            .map(String::as_str);
                        d.faces = growth_accelerator_faces(facing);
                    }
                    if m.kind == MachineKind::CrystalResonanceGenerator {
                        d.faces = self.pack.assets.registry.state(world.block(p.0,p.1,p.2))
                            .map_or(0, |state| {
                                if state.properties.contains_key("facing") {
                                    super::ae2_placement::wap_connection_faces(state)
                                } else {
                                    // Legacy saved state: source default faces north.
                                    1 << 4
                                }
                            });
                    }
                    if m.kind == MachineKind::SpatialPylon {
                        d.idle_override = Some(0.5);
                    }
                    if m.kind == MachineKind::SpatialAnchor {
                        // The host retains AE2's last updated idle value;
                        // cleanup can release tickets without recalculating it.
                        d.idle_override = Some(self.ae2_anchor_idle(address));
                    }
                    if m.kind == MachineKind::PatternProvider {
                        let direction = self.pack.assets.registry.state(world.block(p.0, p.1, p.2))
                            .and_then(|state| state.properties.get("push_direction"))
                            .map(String::as_str);
                        d.faces = pattern_provider_faces(direction);
                    }
                    if m.kind == MachineKind::WirelessAccessPoint {
                        d.faces = self
                            .pack
                            .assets
                            .registry
                            .state(world.block(p.0, p.1, p.2))
                            .map_or(0, super::ae2_placement::wap_connection_faces);
                        let boosters = m
                            .slots
                            .first()
                            .and_then(Option::as_ref)
                            .filter(|s| s.item.as_ref() == "ae2:wireless_booster")
                            .map_or(0, |s| s.count);
                        d.idle_override =
                            Some(horizon_client::ae2_common_config::wireless().link.idle(boosters));
                    }
                    if m.kind == MachineKind::CableBus {
                        let Some((cable_kind, color)) = super::ae2_parts::cable_device(m) else {
                            continue;
                        };
                        d.kind = cable_kind;
                        d.color = color;
                        if let Some(Tag::List(parts)) = m.config.get(&"parts".into()) {
                            for part in parts {
                                let (Some(Tag::String(id)), Some(Tag::String(face))) =
                                    (part.get("id"), part.get("face"))
                                else {
                                    continue;
                                };
                                let (Ok(id), Ok(face)) = (id.to_string(), face.to_string()) else {
                                    continue;
                                };
                                let Some(face) = face_index(&face) else {
                                    continue;
                                };
                                // CableBusContainer.updateConnections excludes
                                // every occupied part face from the center node.
                                // External nodes (P2P, toggle, quartz fiber) are
                                // represented by their explicit port adapters.
                                d.faces &= !(1 << face);
                                if id == "ae2:cable_anchor" {
                                    // CableAnchorPart has no node and blocks this face.
                                    continue;
                                }
                                let idle = match id.as_str() {
                                    "ae2:terminal"
                                    | "ae2:crafting_terminal"
                                    | "ae2:pattern_encoding_terminal" => 0.5,
                                    "ae2:pattern_access_terminal" => 0.5,
                                    "ae2:cable_interface"
                                    | "ae2:cable_pattern_provider"
                                    | "ae2:import_bus"
                                    | "ae2:export_bus"
                                    | "ae2:storage_bus"
                                    | "ae2:level_emitter"
                                    | "ae2:energy_level_emitter"
                                    | "ae2:annihilation_plane"
                                    | "ae2:formation_plane" => 1.0,
                                    "ae2:monitor"
                                    | "ae2:semi_dark_monitor"
                                    | "ae2:dark_monitor"
                                    | "ae2:storage_monitor" => 1.0 / 16.0,
                                    "ae2:conversion_monitor" => 0.5,
                                    "ae2:toggle_bus"
                                    | "ae2:inverted_toggle_bus"
                                    | "ae2:quartz_fiber"
                                    | "ae2:cable_energy_acceptor" => 0.0,
                                    id if super::ae2_p2p::is_tunnel(id) => super::ae2_p2p::idle(id),
                                    _ => continue,
                                };
                                let saved_mode = match part { Tag::Compound(data) => super::ae2_channels::node_mode(data,"gn"), _ => None };
                                let identity = super::ae2_channels::part_identity(part).map(|part_identity| ae2_network_core::NodeIdentity {
                                    host: host_identity, part: Some(part_identity), outer: false,
                                });
                                if let Some(identity) = identity {
                                    n.part_identities.insert((network_pos, face), identity);
                                }
                                if let Some(mode) = super::ae2_channels::merge_mode(saved_mode, identity.and_then(|id| self.ae2_committed_channels.forced_mode(id))) {
                                    d.part_channel_modes.insert(face, mode);
                                }
                                d.attached.push(AttachedPart {
                                    face,
                                    idle,
                                    compressed: id == "ae2:me_p2p_tunnel",
                                    requires_channel: !matches!(
                                        id.as_str(),
                                        "ae2:level_emitter"
                                        | "ae2:energy_level_emitter"
                                            | "ae2:monitor"
                                            | "ae2:semi_dark_monitor"
                                            | "ae2:dark_monitor"
                                            | "ae2:storage_monitor"
                                            | "ae2:toggle_bus"
                                            | "ae2:inverted_toggle_bus"
                                            | "ae2:quartz_fiber"
                                            | "ae2:cable_energy_acceptor"
                                    ),
                                });
                            }
                        }
                    }
                    let centerless = m.kind == MachineKind::CableBus
                        && m.config.get(&"ae2_centerless".into()) == Some(&Tag::Byte(1))
                        && !m.config.contains_key(&"cable".into());
                    if centerless {
                        // Each side part has a separate source GridNode until
                        // a physical center cable is installed. Synthetic
                        // positions are transient; UUIDs own the identity.
                        n.node_identities.remove(&network_pos);
                        for part in &d.attached {
                            let Some(identity) = n.part_identities.get(&(network_pos, part.face)).copied() else { continue };
                            let Some(x) = centerless_index.checked_mul(2).and_then(|i| i32::MIN.checked_add(i)) else { break };
                            centerless_index = centerless_index.saturating_add(1);
                            let endpoint = Pos { x, y: 2, z: 0 };
                            let mut node = Device::new(DeviceKind::PoweredMachine);
                            node.faces = 0;
                            node.idle_override = Some(part.idle);
                            node.channel_override = Some(part.requires_channel);
                            node.capacity_override = Some(8);
                            node.channel_mode_override = d.part_channel_modes.get(&part.face).copied();
                            n.nodes.insert(endpoint, node);
                            n.node_identities.insert(endpoint, identity);
                            n.part_endpoints.insert((network_pos, part.face), endpoint);
                        }
                    } else {
                        n.nodes.insert(network_pos, d);
                    }
                }
            }
        }
        self.ae2_connect_quantum(&mut n);
        self.ae2_spatial_graph(&mut n);
        if cpu_snapshot.is_none() { self.ae2_crafting_graph(&mut n); }
        self.ae2_connect_toggle_buses(&mut n);
        self.ae2_connect_quartz_fibers(&mut n);
        n.connect_facing_ports();
        self.ae2_connect_me_p2p(&mut n);
        n.connect_facing_ports();
        if let Err(error) = self.ae2_project_phantom_nodes(&mut n) {
            // A phantom's old connections cannot be reconstructed by a
            // position-only snapshot. Never silently coalesce its services.
            log::warn!("AE2 phantom topology unavailable: {error}");
            return Network::default();
        }
        if let Some(ordered) = self.ae2_event_overlay(&n) {
            n.committed_channels = Some(ordered);
        }
        self.ae2_event_connection_order(&mut n);
        self.ae2_project_energy_buffers(&mut n);
        n
    }
    /// Project PathingService.saveNodeData into the save batch's cloned
    /// machines; never mutate the live graph merely to serialize a chunk.
    pub(super) fn ae2_project_saved_node_modes(&self, chunk: &mut horizon_client::chunk::Chunk, modes: &ae2_network_core::CommittedChannels) {
        for (pos, entity) in &mut chunk.block_entities {
            let BlockEntity::Ae2(machine) = entity else { continue };
            let Some(host) = self.world.block_entities.incarnation(pos) else { continue };
            let identity = ae2_network_core::NodeIdentity { host, part: None, outer: false };
            if modes.contains_identity(identity) {
                let tag = if machine.kind == MachineKind::DebugPhantomNode { "proxy" } else { "gn" };
                if let Err(error) = super::ae2_channels::set_node_mode(&mut machine.config, tag, modes.forced_mode(identity)) {
                    log::warn!("cannot project AE2 node save data at {:?}: {}", pos, error);
                }
            }
            if let Some(Tag::List(parts)) = machine.config.get_mut(&"parts".into()) {
                for part in parts {
                    let Some(part_id) = super::ae2_channels::part_identity(part) else { continue };
                    let Tag::Compound(data) = part else { continue };
                    for outer in [false, true] {
                        let identity = ae2_network_core::NodeIdentity { host, part: Some(part_id), outer };
                        if modes.contains_identity(identity) {
                            if let Err(error) = super::ae2_channels::set_node_mode(data, if outer { "outer" } else { "gn" }, modes.forced_mode(identity)) {
                                log::warn!("cannot project AE2 part node save data at {:?}: {}", pos, error);
                            }
                        }
                    }
                }
            }
        }
    }
    /// Unlock the entire live grid service before a diagnostic forced-mode
    /// change, mirroring PathingService.setForcedChannelMode. Stage all NBT
    /// changes first so malformed node data cannot partially erase settings.
    fn ae2_clear_grid_forced_mode(&mut self, identity: ae2_network_core::NodeIdentity) -> Result<(), String> {
        let network = self.ae2_network();
        let analysis = network.analyze().map_err(str::to_owned)?;
        let pos = network.node_identities.iter().find_map(|(p,id)| (*id == identity).then_some(*p))
            .or_else(|| network.part_identities.iter().find_map(|((p,_),id)| (*id == identity).then_some(*p)))
            .ok_or("grid node identity is unavailable")?;
        let group = &analysis.components[analysis.nodes.get(&pos).ok_or("missing grid node")?.component];
        let positions: std::collections::BTreeSet<_> = group.positions.iter().copied().collect();
        if group.positions.iter().any(|pos| network.nodes[pos].attached.iter().any(|part| !network.part_identities.contains_key(&(*pos, part.face)))) {
            return Err("a grid part identity is unavailable; wait for its level to tick".into());
        }
        let identities: std::collections::BTreeSet<_> = network.node_identities.iter()
            .filter_map(|(pos,id)| positions.contains(pos).then_some(*id))
            .chain(network.part_identities.iter().filter_map(|((pos,_),id)| positions.contains(pos).then_some(*id))).collect();
        let mut hosts = BTreeMap::new();
        for (dim, world) in self.ae2_loaded_worlds() {
            for (&pos, _) in world.block_entities.iter() {
                if let (Some(host), Some(address)) = (world.block_entities.incarnation(&pos), self.ae2_dimension_address(dim, pos)) {
                    hosts.insert(host, address);
                }
            }
        }
        let mut configs = BTreeMap::new();
        for id in identities.iter().copied() {
            // Phantom's second managed node is never saved by its block entity.
            if id.is_phantom_proxy() { continue; }
            let address = *hosts.get(&id.host).ok_or("grid node host unloaded")?;
            if !configs.contains_key(&address) {
                let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(address) else { return Err("grid node host disappeared".into()) };
                configs.insert(address, machine.config.clone());
            }
            let config = configs.get_mut(&address).ok_or("missing staged node config")?;
            if let Some(part_id) = id.part {
                let Some(Tag::List(parts)) = config.get_mut(&"parts".into()) else { return Err("missing physical part list".into()) };
                let part = parts.iter_mut().find(|part| super::ae2_channels::part_identity(part) == Some(part_id)).ok_or("physical part disappeared")?;
                let Tag::Compound(data) = part else { return Err("invalid physical part".into()) };
                super::ae2_channels::set_node_mode(data, if id.outer { "outer" } else { "gn" }, None).map_err(str::to_owned)?;
            } else {
                let tag = if matches!(self.ae2_block_entity(address),Some(BlockEntity::Ae2(m)) if m.kind==MachineKind::DebugPhantomNode) { "proxy" } else { "gn" };
                super::ae2_channels::set_node_mode(config, tag, None).map_err(str::to_owned)?;
            }
        }
        for (address, config) in configs {
            let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity_mut(address) else { return Err("grid node host disappeared".into()) };
            machine.config = config;
            self.ae2_mark_modified(address);
        }
        for id in identities { self.ae2_committed_channels.clear_forced_mode(id); }
        Ok(())
    }
    /// Source PathingService finalizes the channel calculation at server tick
    /// end. The physical identities in the snapshot reject replacements.
    pub(super) fn ae2_commit_pathing(&mut self) {
        let mut network = self.ae2_network();
        network.committed_channels = None;
        if let Ok(analysis) = network.analyze() {
            let next = ae2_network_core::CommittedChannels::capture(&network, &analysis);
            self.ae2_event_commit(&network, &analysis, &next);
            let changed_hosts: std::collections::BTreeSet<_> = network.node_identities.values()
                .chain(network.part_identities.values())
                .filter(|id| self.ae2_committed_channels.forced_mode(**id) != next.forced_mode(**id))
                .map(|id| id.host).collect();
            let changed: Vec<_> = self.ae2_loaded_worlds().into_iter().flat_map(|(dim, world)| {
                let changed_hosts = &changed_hosts;
                world.block_entities.iter().filter_map(move |(&pos, _)| {
                    world.block_entities.incarnation(&pos).filter(|host| changed_hosts.contains(host))
                        .and_then(|_| super::ae2_global::GridAddress::from_local(dim, pos))
                }).collect::<Vec<_>>()
            }).collect();
            self.ae2_committed_channels = next;
            for address in changed { self.ae2_mark_modified(address); }
        }
    }
    pub(super) fn ae2_commit_power(&mut self, n: &Network) {
        let observed = self.ae2_sync_events();
        for (p, d) in &n.nodes {
            if d.kind.energy_capacity() == 0.0 {
                continue;
            }
            let Some(address) = super::ae2_global::GridAddress::from_network(*p) else {
                continue;
            };
            let mut before = None;
            if let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity_mut(address) {
                if m.energy != d.energy {
                    before = Some(m.energy);
                    m.energy = d.energy;
                    self.ae2_mark_modified(address);
                }
            }
            if observed { if let Some(old) = before { self.ae2_energy_store_changed(address,Some(old)); } }
        }
    }
    pub(super) fn ae2_online(&self, pos: WorldPos) -> bool {
        let Some(address) = self.ae2_local_address(pos) else {
            return false;
        };
        self.ae2_online_encoded(address)
    }
    pub(super) fn ae2_online_encoded(&self, address: super::ae2_global::GridAddress) -> bool {
        self.ae2_network()
            .analyze()
            .ok()
            .and_then(|a| a.nodes.get(&address.network_pos()).map(|s| s.online))
            .unwrap_or(false)
    }
    pub(super) fn ae2_part_online(&self, pos: WorldPos, face: &str) -> bool {
        let Some(address) = self.ae2_local_address(pos) else {
            return false;
        };
        let Some(face) = face_index(face) else {
            return false;
        };
        let network = self.ae2_network();
        network.analyze().ok()
            .and_then(|analysis| network.part_status(&analysis, address.network_pos(), face))
            .is_some_and(|status| status.online)
    }
    pub(super) fn ae2_terminal_online(&self, pos: WorldPos) -> bool {
        let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get(&pos) else {
            return false;
        };
        if m.kind != MachineKind::CableBus {
            return self.ae2_online(pos);
        }
        let Some(Tag::List(parts)) = m.config.get(&"parts".into()) else {
            return false;
        };
        let mut found = false;
        for part in parts {
            let (Some(Tag::String(id)), Some(Tag::String(face))) =
                (part.get("id"), part.get("face"))
            else {
                continue;
            };
            if id.to_string().is_ok_and(|id| matches!(id.as_str(),"ae2:terminal"|"ae2:crafting_terminal"|"ae2:pattern_encoding_terminal")) {
                found = true;
                if !face
                    .to_string()
                    .is_ok_and(|face| self.ae2_part_online(pos, &face))
                {
                    return false;
                }
            }
        }
        found
    }
    pub(super) fn ae2_add_power(&mut self, pos: WorldPos, amount: f64) -> f64 {
        let Some(address) = self.ae2_local_address(pos) else {
            return 0.0;
        };
        self.ae2_add_power_encoded(address, amount)
    }
    pub(super) fn ae2_power_room(&mut self, pos: WorldPos) -> f64 {
        let Some(address)=self.ae2_local_address(pos) else {return 0.0;};
        let network=self.ae2_network();
        let upper = network.nodes.values().map(|node|node.kind.energy_capacity()).sum::<f64>()
            + self.ae2_live_events.borrow().energy.lifecycle.services.values().map(|service|service.local_capacity()).sum::<f64>()
            + if network.nodes.values().any(|node|node.kind==DeviceKind::CreativeEnergyCell) { super::ae2_energy_service::CREATIVE_STORED } else {0.0};
        if upper.is_finite() && let Ok(room)=self.ae2_energy_transaction(address,None,upper,true,true) {return room;}
        let Ok(analysis)=network.analyze() else {return 0.0;};
        let Some(status)=analysis.nodes.get(&address.network_pos()) else {return 0.0;};
        let power=&analysis.power_components[analysis.components[status.component].power_component];
        power.positions.iter().filter_map(|p|network.nodes.get(p)).map(|node|(node.kind.energy_capacity()-node.energy).max(0.0)).sum()
    }
    pub(super) fn ae2_add_power_encoded(
        &mut self,
        address: super::ae2_global::GridAddress,
        amount: f64,
    ) -> f64 {
        if let Ok(accepted)=self.ae2_energy_transaction(address,None,amount,false,true){return accepted;}
        let mut n = self.ae2_network();
        let accepted = n
            .insert_energy(address.network_pos(), amount)
            .unwrap_or(0.0);
        self.ae2_commit_power(&n);
        accepted
    }
    pub(super) fn ae2_take_power(&mut self, pos: WorldPos, amount: f64, simulate: bool) -> f64 {
        let Some(address) = self.ae2_local_address(pos) else {
            return 0.0;
        };
        self.ae2_take_power_encoded(address, amount, simulate)
    }
    pub(super) fn ae2_take_power_encoded(
        &mut self,
        address: super::ae2_global::GridAddress,
        amount: f64,
        simulate: bool,
    ) -> f64 {
        // PowerMultiplier.CONFIG charges real storage in scaled AE, while the
        // source API returns the corresponding unscaled requested amount.
        if !amount.is_finite() || amount <= 0.0 { return 0.0; }
        let multiplier = horizon_client::ae2_common_config::gameplay().usage_multiplier;
        self.ae2_take_power_encoded_raw(address, (amount * multiplier).min(f64::MAX), simulate) / multiplier
    }
    /// Source operations using PowerMultiplier.ONE, such as charger grid
    /// refill, deliberately bypass the configured consumption multiplier.
    pub(super) fn ae2_take_power_raw(&mut self, pos: WorldPos, amount: f64, simulate: bool) -> f64 {
        let Some(address) = self.ae2_local_address(pos) else { return 0.0; };
        self.ae2_take_power_encoded_raw(address, amount, simulate)
    }
    pub(super) fn ae2_take_power_encoded_raw(
        &mut self,
        address: super::ae2_global::GridAddress,
        amount: f64,
        simulate: bool,
    ) -> f64 {
        if !amount.is_finite() || amount <= 0.0 {
            return 0.0;
        }
        if let Some(source) = self.ae2_wireless_power {
            return self
                .stack_in_mut(source)
                .as_mut()
                .and_then(|stack| {
                    super::ae2_wireless::transfer_energy(stack, amount, false, simulate).ok()
                })
                .unwrap_or(0.0);
        }
        if let Ok(extracted)=self.ae2_energy_transaction(address,None,amount,simulate,false){return extracted;}
        let mut n = self.ae2_network();
        let Ok(a) = n.analyze() else { return 0.0 };
        let Some(status) = a.nodes.get(&address.network_pos()) else {
            return 0.0;
        };
        let power = &a.power_components[a.components[status.component].power_component];
        if power
            .positions
            .iter()
            .any(|p| n.nodes[p].kind == DeviceKind::CreativeEnergyCell)
        {
            return amount;
        }
        let mut left = amount;
        for p in &power.positions {
            if let Some(d) = n.nodes.get_mut(p) {
                let got = d.energy.min(left);
                d.energy -= got;
                left -= got;
                if left <= 0.0 {
                    break;
                }
            }
        }
        if !simulate {
            self.ae2_commit_power(&n);
        }
        amount - left
    }
    pub(super) fn ae2_drives(
        &self,
        pos: WorldPos,
    ) -> Result<Vec<super::ae2_global::GridAddress>, String> {
        self.ae2_drives_encoded(self.ae2_local_address(pos).ok_or("invalid AE2 anchor")?)
    }
    pub(super) fn ae2_drives_encoded(
        &self,
        anchor: super::ae2_global::GridAddress,
    ) -> Result<Vec<super::ae2_global::GridAddress>, String> {
        let n = self.ae2_network();
        let a = n.analyze().map_err(str::to_owned)?;
        let s = a
            .nodes
            .get(&anchor.network_pos())
            .ok_or("no AE2 device here")?;
        if !s.online {
            return Err("network is offline: check power and channels".into());
        }
        Ok(a.components[s.component].positions.iter().filter(|p|a.nodes.get(p).is_some_and(|s|s.online)).filter_map(|p|{
   let address=super::ae2_global::GridAddress::from_network(*p)?;matches!(self.ae2_block_entity(address),Some(BlockEntity::Ae2(m))if matches!(m.kind,MachineKind::Drive|MachineKind::Chest)).then_some(address)
  }).collect())
    }
    pub(super) fn ae2_grid_inventory(&self, pos: WorldPos) -> Result<Vec<(ItemKey, u64)>, String> {
        self.ae2_grid_inventory_encoded(self.ae2_local_address(pos).ok_or("invalid AE2 anchor")?)
    }
    pub(super) fn ae2_grid_inventory_encoded(
        &self,
        pos: super::ae2_global::GridAddress,
    ) -> Result<Vec<(ItemKey, u64)>, String> {
        let mut total = BTreeMap::<ItemKey, u64>::new();
        for p in self.ae2_drives_encoded(pos)? {
            if let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity(p) {
                for stack in m.slots.iter().flatten() {
                    if let Ok(keys) = self.ae2_creative_cell_keys(stack) {
                        for key in keys {
                            if let super::ae2_storage_devices::CreativeCellKey::Item(k) = key {
                                let n = total.entry(k).or_default();
                                *n = n
                                    .checked_add(i32::MAX as u64)
                                    .ok_or("network item count overflow")?;
                            }
                        }
                    }
                    if let Ok(c) = self.ae2_read_cell(stack) {
                        for (k, v) in c.entries_for(ae2_cell_core::Channel::Item) {
                            let n = total.entry(k.clone()).or_default();
                            *n = n.checked_add(*v).ok_or("network item count overflow")?;
                        }
                    }
                }
            }
        }
        for mount in self.ae2_storage_mounts_encoded(pos) {
            for (k, v) in self.ae2_storage_inventory(&mount) {
                let n = total.entry(k).or_default();
                *n = n.checked_add(v).ok_or("network item count overflow")?;
            }
        }
        Ok(total.into_iter().collect())
    }
    pub(super) fn ae2_grid_insert(
        &mut self,
        pos: WorldPos,
        input: &ItemStack,
        limit: u64,
    ) -> Result<u64, String> {
        let address = self.ae2_local_address(pos).ok_or("invalid AE2 anchor")?;
        self.ae2_grid_insert_encoded(address, input, limit)
    }
    pub(super) fn ae2_grid_insert_encoded(
        &mut self,
        pos: super::ae2_global::GridAddress,
        input: &ItemStack,
        limit: u64,
    ) -> Result<u64, String> {
        let snapshot = self.ae2_snapshot_blocks();
        let mut plan = super::ae2_buses::FormationPlan::default();
        let result = self
            .ae2_grid_insert_inner(pos, input, limit, false, true, true, &mut plan)
            .and_then(|moved| self.ae2_commit_formation(plan).map(|_| moved));
        if result.is_err() {
            self.ae2_restore_blocks(snapshot);
        }
        result
    }
    /// Preview insertion without world placement, container writes or power
    /// debit. Crafting receipt previews are rolled back from all loaded worlds.
    pub(super) fn ae2_grid_insert_simulate(
        &mut self,
        pos: WorldPos,
        input: &ItemStack,
        limit: u64,
    ) -> Result<u64, String> {
        let address = self.ae2_local_address(pos).ok_or("invalid AE2 anchor")?;
        self.ae2_grid_insert_simulate_encoded(address, input, limit)
    }
    pub(super) fn ae2_grid_insert_simulate_encoded(
        &mut self,
        pos: super::ae2_global::GridAddress,
        input: &ItemStack,
        limit: u64,
    ) -> Result<u64, String> {
        let snapshot = self.ae2_snapshot_blocks();
        let mut plan = super::ae2_buses::FormationPlan::default();
        let result = self.ae2_grid_insert_inner(pos, input, limit, true, true, true, &mut plan);
        self.ae2_restore_blocks(snapshot);
        result
    }
    /// Insert into rollback-safe inventories only, never into world-placement
    /// providers; callers may compose several steps under a BE snapshot.
    pub(super) fn ae2_grid_insert_storage_only(
        &mut self,
        pos: WorldPos,
        input: &ItemStack,
        limit: u64,
    ) -> Result<u64, String> {
        let address = self.ae2_local_address(pos).ok_or("invalid AE2 anchor")?;
        self.ae2_grid_insert_storage_only_encoded(address, input, limit)
    }
    pub(super) fn ae2_grid_insert_storage_only_encoded(
        &mut self,
        pos: super::ae2_global::GridAddress,
        input: &ItemStack,
        limit: u64,
    ) -> Result<u64, String> {
        let snapshot = self.ae2_snapshot_blocks();
        let mut plan = super::ae2_buses::FormationPlan::default();
        let result = self.ae2_grid_insert_inner(pos, input, limit, false, false, true, &mut plan)
            .and_then(|moved| self.ae2_commit_formation(plan).map(|_| moved));
        if result.is_err() {
            self.ae2_restore_blocks(snapshot);
        }
        result
    }
    /// Raw storage-only interface capability; simulation restores all BE state.
    pub(super) fn ae2_grid_insert_uncharged_storage_encoded(
        &mut self,
        pos: super::ae2_global::GridAddress,
        input: &ItemStack,
        limit: u64,
        simulate: bool,
    ) -> Result<u64, String> {
        let snapshot = self.ae2_snapshot_blocks();
        let mut plan = super::ae2_buses::FormationPlan::default();
        let result = self.ae2_grid_insert_uncharged_storage_staged_encoded(pos, input, limit, simulate, &mut plan)
            .and_then(|moved| if simulate { Ok(moved) } else { self.ae2_commit_formation(plan).map(|_| moved) });
        if result.is_err() { self.ae2_restore_blocks(snapshot); }
        result
    }
    pub(super) fn ae2_grid_insert_uncharged_storage_staged_encoded(
        &mut self, pos: super::ae2_global::GridAddress, input: &ItemStack,
        limit: u64, simulate: bool, plan: &mut super::ae2_buses::FormationPlan,
    ) -> Result<u64, String> {
        let snapshot = self.ae2_snapshot_blocks();
        let previous = plan.len();
        let result = self.ae2_grid_insert_inner(pos, input, limit, simulate, false, false, plan);
        if simulate || result.is_err() {
            self.ae2_restore_blocks(snapshot);
            plan.truncate(previous);
        }
        result
    }
    /// Reserve formation writes while performing the rollback-safe inventory
    /// part. Caller snapshots BEs, commits plan only after all transfer steps.
    pub(super) fn ae2_grid_insert_staged(
        &mut self,
        pos: WorldPos,
        input: &ItemStack,
        limit: u64,
        plan: &mut super::ae2_buses::FormationPlan,
    ) -> Result<u64, String> {
        let address = self.ae2_local_address(pos).ok_or("invalid AE2 anchor")?;
        self.ae2_grid_insert_staged_encoded(address, input, limit, plan)
    }
    pub(super) fn ae2_grid_insert_staged_encoded(
        &mut self,
        pos: super::ae2_global::GridAddress,
        input: &ItemStack,
        limit: u64,
        plan: &mut super::ae2_buses::FormationPlan,
    ) -> Result<u64, String> {
        let snapshot = self.ae2_snapshot_blocks();
        let previous = plan.len();
        let result = self.ae2_grid_insert_inner(pos, input, limit, false, true, true, plan);
        if result.is_err() {
            self.ae2_restore_blocks(snapshot);
            plan.truncate(previous);
        }
        result
    }
    /// Raw MEStorage insertion for machine-owned provider return buffers.
    /// The caller snapshots all block entities, updates its physical source,
    /// and commits this formation plan only after all steps succeed.
    pub(super) fn ae2_grid_insert_uncharged_staged_encoded(
        &mut self,
        pos: super::ae2_global::GridAddress,
        input: &ItemStack,
        limit: u64,
        plan: &mut super::ae2_buses::FormationPlan,
    ) -> Result<u64, String> {
        let snapshot = self.ae2_snapshot_blocks();
        let previous = plan.len();
        let result = self.ae2_grid_insert_inner(pos, input, limit, false, true, false, plan);
        if result.is_err() {
            self.ae2_restore_blocks(snapshot);
            plan.truncate(previous);
        }
        result
    }
    fn ae2_grid_insert_inner(
        &mut self,
        pos: super::ae2_global::GridAddress,
        input: &ItemStack,
        limit: u64,
        simulate: bool,
        allow_formation: bool,
        charge_power: bool,
        formation: &mut super::ae2_buses::FormationPlan,
    ) -> Result<u64, String> {
        let cells = self.ae2_cell_mounts_encoded(pos)?;
        let k = ae2_lab::key(input)?;
        let mut left = limit.min(input.count.into());
        // StorageHelper uses 1 AE/item for an AEItemKey (amountPerOperation=1).
        if charge_power {
            left = left
                .min((self.ae2_take_power_encoded(pos, left as f64, true) + 0.9).floor() as u64);
        }
        let wanted = left;
        let received = self.ae2_crafting_receive_output_encoded(pos, input, left)?;
        if received > left {
            return Err("crafting receipt exceeds supplied quantity".into());
        }
        left -= received;
        let mounts=self.ae2_storage_mounts_encoded(pos);
        let mut priorities=std::collections::BTreeSet::new();
        priorities.extend(mounts.iter().map(|mount|mount.priority));
        priorities.extend(cells.iter().map(|mount|mount.priority));
        if allow_formation{priorities.extend(self.ae2_formation_priorities(pos));}
        let mut changes=Vec::new();
        // NetworkStorage: descending priority, then preferred storage before
        // the remaining storage within EACH priority, not globally at zero.
        for priority in priorities.iter().rev().copied(){
            let mut late_buses=Vec::new();let mut late_cells=Vec::new();
            for mount in mounts.iter().filter(|mount|mount.priority==priority){
                if left==0{break}
                if self.ae2_item_bus_preferred(mount,&k)?{left-=self.ae2_storage_insert(mount,input,left,simulate,formation)?;}else{late_buses.push(mount);}
            }
            for mount in cells.iter().filter(|mount|mount.priority==priority){
                if left==0{break}
                if self.ae2_cell_preferred(mount,&k,false)?{
                    let (moved,updated)=self.ae2_cell_storage_change(mount,&k,left,false,true,simulate)?;left-=moved;
                    if let Some(updated)=updated{changes.push((mount.address,mount.slot,updated));}
                }else{late_cells.push(mount);}
            }
            for mount in late_buses{if left==0{break}left-=self.ae2_storage_insert(mount,input,left,simulate,formation)?;}
            for mount in late_cells{
                if left==0{break}
                let (moved,updated)=self.ae2_cell_storage_change(mount,&k,left,false,true,simulate)?;left-=moved;
                if let Some(updated)=updated{changes.push((mount.address,mount.slot,updated));}
            }
            // FormationPlane's InWorldStorage has the default nonpreferred
            // behavior. Existing placement staging/rollback remains intact.
            if left>0&&allow_formation{left-=self.ae2_form_simple_blocks(pos,input,left,priority,simulate,formation);}
            if left==0{break}
        }
        let moved = wanted - left;
        if moved > 0 && !simulate {
            if charge_power {
                self.ae2_take_power_encoded(pos, moved as f64, false);
            }
            for (p, i, s) in changes {
                if let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity_mut(p) {
                    m.slots[i] = Some(s);
                }
                self.ae2_mark_modified(p);
            }
        }
        Ok(moved)
    }
    pub(super) fn ae2_grid_extract(
        &mut self,
        pos: WorldPos,
        k: &ItemKey,
        limit: u64,
    ) -> Result<Option<ItemStack>, String> {
        let address = self.ae2_local_address(pos).ok_or("invalid AE2 anchor")?;
        self.ae2_grid_extract_encoded(address, k, limit)
    }
    pub(super) fn ae2_grid_extract_encoded(
        &mut self,
        pos: super::ae2_global::GridAddress,
        k: &ItemKey,
        limit: u64,
    ) -> Result<Option<ItemStack>, String> {
        let snapshot = self.ae2_snapshot_blocks();
        let mut plan = super::ae2_buses::FormationPlan::default();
        let result = self.ae2_grid_extract_staged_encoded(pos, k, limit, &mut plan)
            .and_then(|out| self.ae2_commit_formation(plan).map(|_| out));
        if result.is_err() { self.ae2_restore_blocks(snapshot); }
        result
    }
    pub(super) fn ae2_grid_extract_staged(
        &mut self, pos: WorldPos, k: &ItemKey, limit: u64,
        plan: &mut super::ae2_buses::FormationPlan,
    ) -> Result<Option<ItemStack>, String> {
        let address = self.ae2_local_address(pos).ok_or("invalid AE2 anchor")?;
        self.ae2_grid_extract_staged_encoded(address, k, limit, plan)
    }
    pub(super) fn ae2_grid_extract_staged_encoded(
        &mut self, pos: super::ae2_global::GridAddress, k: &ItemKey, limit: u64,
        plan: &mut super::ae2_buses::FormationPlan,
    ) -> Result<Option<ItemStack>, String> {
        let snapshot = self.ae2_snapshot_blocks();
        let previous = plan.len();
        let result = self.ae2_grid_extract_inner(pos, k, limit, true, plan);
        if result.is_err() { self.ae2_restore_blocks(snapshot); plan.truncate(previous); }
        result
    }
    pub(super) fn ae2_grid_extract_uncharged_encoded(
        &mut self,
        pos: super::ae2_global::GridAddress,
        k: &ItemKey,
        limit: u64,
        simulate: bool,
    ) -> Result<Option<ItemStack>, String> {
        let snapshot = self.ae2_snapshot_blocks();
        let mut plan = super::ae2_buses::FormationPlan::default();
        let result = self.ae2_grid_extract_uncharged_staged_encoded(pos, k, limit, simulate, &mut plan)
            .and_then(|out| if simulate { Ok(out) } else { self.ae2_commit_formation(plan).map(|_| out) });
        if result.is_err() { self.ae2_restore_blocks(snapshot); }
        result
    }
    pub(super) fn ae2_grid_extract_uncharged_staged_encoded(
        &mut self, pos: super::ae2_global::GridAddress, k: &ItemKey, limit: u64,
        simulate: bool, plan: &mut super::ae2_buses::FormationPlan,
    ) -> Result<Option<ItemStack>, String> {
        let snapshot = self.ae2_snapshot_blocks();
        let previous = plan.len();
        let result = self.ae2_grid_extract_inner(pos, k, limit, false, plan);
        if simulate || result.is_err() { self.ae2_restore_blocks(snapshot); plan.truncate(previous); }
        result
    }
    /// Reserve an aggregate quantity without rebuilding the grid and cloning
    /// all block entities once per physical stack. The caller owns the outer
    /// transaction across keys and commits the shared formation plan.
    pub(super) fn ae2_grid_extract_bulk_staged(
        &mut self, pos: WorldPos, k: &ItemKey, limit: u64,
        plan: &mut super::ae2_buses::FormationPlan,
    ) -> Result<u64, String> {
        let address = self.ae2_local_address(pos).ok_or("invalid AE2 anchor")?;
        self.ae2_grid_extract_bulk_staged_encoded(address, k, limit, true, false, plan)
    }
    pub(super) fn ae2_grid_extract_bulk_staged_encoded(
        &mut self, pos: super::ae2_global::GridAddress, k: &ItemKey, limit: u64,
        charge_power: bool, simulate: bool,
        plan: &mut super::ae2_buses::FormationPlan,
    ) -> Result<u64, String> {
        // Validate the exact component-bearing key before any mutation.
        ae2_lab::prototype(k)?;
        let snapshot = self.ae2_snapshot_blocks();
        let previous = plan.len();
        let result = self.ae2_grid_extract_quantity_inner(pos, k, limit, charge_power, true, plan);
        if simulate || result.is_err() { self.ae2_restore_blocks(snapshot); plan.truncate(previous); }
        result
    }
    fn ae2_grid_extract_inner(
        &mut self,
        pos: super::ae2_global::GridAddress,
        k: &ItemKey,
        limit: u64,
        charge_power: bool,
        formation: &mut super::ae2_buses::FormationPlan,
    ) -> Result<Option<ItemStack>, String> {
        let mut out = ae2_lab::prototype(k)?;
        let registry_max = self.res.items().get(&out.item).map_or(64, |i| i.max_stack).max(1);
        let cap = u64::from(super::ae2_formation::max_stack(&out, registry_max));
        let got = self.ae2_grid_extract_quantity_inner(pos, k, limit.min(cap), charge_power, false, formation)?;
        if got == 0 { return Ok(None); }
        out.count = u32::try_from(got).map_err(|_| "physical extraction exceeds stack count")?;
        Ok(Some(out))
    }
    fn ae2_grid_extract_quantity_inner(
        &mut self,
        pos: super::ae2_global::GridAddress,
        k: &ItemKey,
        limit: u64,
        charge_power: bool,
        bulk: bool,
        formation: &mut super::ae2_buses::FormationPlan,
    ) -> Result<u64, String> {
        let cells = self.ae2_cell_mounts_encoded(pos)?;
        let mut left = limit;
        if charge_power {
            left = left
                .min((self.ae2_take_power_encoded(pos, left as f64, true) + 0.9).floor() as u64);
        }
        let wanted = left;
        let mut changes = Vec::new();
        let mounts=self.ae2_storage_mounts_encoded(pos);
        let mut priorities=std::collections::BTreeSet::new();
        priorities.extend(mounts.iter().map(|mount|mount.priority));priorities.extend(cells.iter().map(|mount|mount.priority));
        // Source extraction uses ascending priority and no preferred pass.
        for priority in priorities{
            for mount in mounts.iter().filter(|mount|mount.priority==priority){
                if left==0{break}
                let moved = if bulk {
                    self.ae2_storage_extract_bulk(mount,k,left,formation)?
                } else {
                    self.ae2_storage_extract(mount,k,left,false,formation)?.map_or(0, |stack| u64::from(stack.count))
                };
                if moved > left { return Err("storage mount exceeded requested extraction".into()); }
                left -= moved;
            }
            for mount in cells.iter().filter(|mount|mount.priority==priority){
                if left==0{break}
                let (moved,updated)=self.ae2_cell_storage_change(mount,k,left,false,false,false)?;left-=moved;
                if let Some(updated)=updated{changes.push((mount.address,mount.slot,updated));}
            }
            if left==0{break}
        }
        let got = wanted - left;
        if got == 0 {
            return Ok(0);
        }
        if charge_power {
            self.ae2_take_power_encoded(pos, got as f64, false);
        }
        for (p, i, s) in changes {
            if let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity_mut(p) {
                m.slots[i] = Some(s);
            }
            self.ae2_mark_modified(p);
        }
        Ok(got)
    }
    /// Give old physical part records an identity once, without replacing
    /// malformed existing IDs. This runs in each loaded level before grid work.
    fn ae2_migrate_part_identities(&mut self) {
        let mut changed = Vec::new();
        let enabled = self.world.block_entities.ae2_journal_enabled();
        let mut events = Vec::new();
        for (&pos, entity) in self.world.block_entities.iter_mut() {
            let BlockEntity::Ae2(machine) = entity else { continue };
            if machine.kind != MachineKind::CableBus { continue; }
            let Some(Tag::List(parts)) = machine.config.get_mut(&"parts".into()) else { continue };
            for part in parts {
                let before = enabled.then(|| part.clone());
                if super::ae2_channels::ensure_part_identity(part) {
                    changed.push(pos);
                    if let Some(before) = before { events.push((pos, before, part.clone())); }
                }
            }
        }
        for (pos, before, after) in events { self.world.block_entities.record_ae2_part_change(pos,before,after); }
        for (x, _, z) in changed { self.world.modified.insert((x >> 4, z >> 4)); }
    }
    pub(super) fn tick_ae2_grid(&mut self) {
        if self.remote.is_some() {
            return;
        }
        self.ae2_migrate_part_identities();
        self.ae2_tick_external_power();
        self.ae2_tick_anchors();
        self.ae2_tick_cranks();
        self.ae2_tick_debug_item_generators();
        self.ae2_tick_light_detectors();
        self.tick_ae2_processing();
        self.tick_ae2_providers();
        self.tick_ae2_recursive_crafting();
        self.tick_ae2_growth_accelerators();
        self.tick_ae2_storage_devices();
        self.ae2_tick_spatial();
        self.tick_ae2_autocrafting();
        let energy_ticked = self.level_pass.last && self.ae2_energy_services_tick();
        if self.level_pass.last && !energy_ticked { self.ae2_energy_service_end_tick(); }
        let mut n = self.ae2_network();
        if self.level_pass.last && !energy_ticked && n.tick().is_ok() {
            self.ae2_commit_power(&n);
        }
        if let Ok(analysis) = n.analyze() {
            self.ae2_update_visual_states(&n, &analysis);
        }
        self.tick_ae2_buses();
        self.ae2_tick_storage_monitors();
        self.ae2_tick_redstone_p2p();
        self.ae2_tick_light_p2p();
    }
    /// Grid services receive server callbacks while vanilla block-entity and
    /// entity ticks are frozen. Do not run the ordinary BE work here.
    pub(super) fn ae2_tick_frozen_grid_service(&mut self) {
        self.ae2_migrate_part_identities();
        // SpatialAnchor is an IGridTickable, not an ordinary BE ticker.
        self.ae2_tick_anchors();
        self.tick_ae2_grid_processing();
        self.tick_ae2_storage_devices();
        self.tick_ae2_providers();
        self.tick_ae2_recursive_crafting();
        self.tick_ae2_autocrafting();
        self.tick_ae2_growth_accelerators();
        let energy_ticked = self.level_pass.last && self.ae2_energy_services_tick();
        if self.level_pass.last && !energy_ticked { self.ae2_energy_service_end_tick(); }
        let mut network = self.ae2_network();
        if self.level_pass.last && !energy_ticked && network.tick().is_ok() {
            self.ae2_commit_power(&network);
        }
        if let Ok(analysis) = network.analyze() {
            self.ae2_update_visual_states(&network, &analysis);
        }
        self.tick_ae2_frozen_buses();
        self.ae2_tick_storage_monitors();
        self.tick_ae2_interfaces();
        self.ae2_tick_light_p2p();
    }
    /// Register a newly placed AE2 machine using real block state; called by the
    /// normal interaction path and the test fixture, never invents its storage.
    pub(super) fn ae2_ensure_machine(&mut self, pos: WorldPos) -> bool {
        let name = self
            .res
            .facts()
            .get(self.world.block(pos.0, pos.1, pos.2))
            .name
            .clone();
        let Some(m) = Machine::for_block(&name) else {
            return false;
        };
        self.world
            .block_entities
            .get_or_insert_with(pos, || BlockEntity::Ae2(Box::new(m)));
        self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
        true
    }
}
impl Game<'_> {
    pub(super) fn ae2_grid_command(&mut self, w: &[&str]) -> Result<serde_json::Value, String> {
        use serde_json::json;
        if w.len() < 4 {
            return Err(
                "usage: /ae2 grid status|install|part|deposit|withdraw x y z [args]".into(),
            );
        }
        let p = (
            w[1].parse().map_err(|_| "x coordinate")?,
            w[2].parse().map_err(|_| "y coordinate")?,
            w[3].parse().map_err(|_| "z coordinate")?,
        );
        if !self.ae2_ensure_machine(p) {
            return Err("there is no AE2 machine at that position".into());
        }
        match w[0] {
            "part-status" => {
                let face = face_index(w.get(4).copied().ok_or("missing face")?).ok_or("invalid face")?;
                let address = self.ae2_local_address(p).ok_or("invalid AE2 anchor")?;
                let network = self.ae2_network();
                let analysis = network.analyze().map_err(str::to_owned)?;
                let host = address.network_pos();
                let part = network.part_status(&analysis, host, face).ok_or("no grid part on this face")?;
                let endpoint = network.part_endpoints.get(&(host, face)).copied().unwrap_or(host);
                let node = analysis.nodes.get(&endpoint).ok_or("part node unavailable")?;
                let service = network.part_identities.get(&(host, face)).and_then(|identity|
                    self.ae2_event_service_ids(&network).and_then(|ids| ids.get(identity).copied()));
                Ok(json!({"online":part.online,"channel":part.channel,"powered":node.powered,
                    "nodes":analysis.components[node.component].positions.len(),
                    "ordered_service":service,"running_tick":self.ae2_running_tick()}))
            }
            "status" => {
                let n = self.ae2_network();
                let a = n.analyze().map_err(str::to_owned)?;
                let address = self
                    .ae2_local_address(p)
                    .ok_or("invalid local grid position")?;
                let s = a
                    .nodes
                    .get(&address.network_pos())
                    .ok_or("missing graph node")?;
                let c = &a.components[s.component];
                let entries = self
                    .ae2_grid_inventory(p)
                    .unwrap_or_default()
                    .iter()
                    .map(|(k, v)| json!({"item":k.item_id,"count":v}))
                    .collect::<Vec<_>>();
                Ok(
                    json!({"position":[p.0,p.1,p.2],"online":s.online,"powered":s.powered,"channel":s.channel,"energy":c.energy,"channels":c.channels_in_use,"channel_mode":c.channel_mode.source_name(),"nodes":c.positions.len(),"items":entries}),
                )
            }
            "mode" if w.len() == 5 => {
                let mode = if w[4] == "CLEAR" { None } else {
                    Some(ae2_network_core::ChannelMode::from_source_name(w[4]).ok_or("invalid channel mode")?)
                };
                let address = self.ae2_local_address(p).ok_or("invalid grid position")?;
                let identity = *self.ae2_network().node_identities.get(&address.network_pos()).ok_or("no physical grid node")?;
                self.ae2_clear_grid_forced_mode(identity)?;
                let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&p) else { return Err("machine disappeared".into()); };
                super::ae2_channels::set_node_mode(&mut machine.config,"gn",mode).map_err(str::to_owned)?;
                self.world.modified.insert((p.0 >> 4, p.2 >> 4));
                self.world.block_entities.record_ae2_forced_mode(p, identity, mode);
                Ok(json!({"forced_mode":mode.map(|m|m.source_name())}))
            }
            "modepart" if w.len() == 7 => {
                let face = w[4];
                let node = w[5];
                let index = face_index(face).ok_or("invalid part face")?;
                if !matches!(node,"gn"|"outer") { return Err("invalid part node".into()); }
                let mode = if w[6] == "CLEAR" { None } else {
                    Some(ae2_network_core::ChannelMode::from_source_name(w[6]).ok_or("invalid channel mode")?)
                };
                let address = self.ae2_local_address(p).ok_or("invalid grid position")?;
                let network = self.ae2_network();
                let inner = *network.part_identities.get(&(address.network_pos(),index)).ok_or("part identity unavailable")?;
                let identity = ae2_network_core::NodeIdentity { outer: node == "outer", ..inner };
                if node == "outer" && !network.node_identities.values().any(|id| *id == identity) {
                    return Err("part has no such grid node".into());
                }
                self.ae2_clear_grid_forced_mode(identity)?;
                let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&p) else { return Err("machine disappeared".into()); };
                let Some(Tag::List(parts)) = machine.config.get_mut(&"parts".into()) else { return Err("no cable parts".into()); };
                let part = parts.iter_mut().find(|part| super::ae2_channels::part_identity(part) == inner.part).ok_or("part disappeared")?;
                let Tag::Compound(data) = part else { return Err("invalid part data".into()) };
                super::ae2_channels::set_node_mode(data,node,mode).map_err(str::to_owned)?;
                self.world.modified.insert((p.0 >> 4, p.2 >> 4));
                self.world.block_entities.record_ae2_forced_mode(p, identity, mode);
                Ok(json!({"face":face,"node":node,"forced_mode":mode.map(|m|m.source_name())}))
            }
            "install" => {
                let slot: usize = w.get(4).unwrap_or(&"0").parse().map_err(|_| "slot index")?;
                let held = self.inventory[self.selected]
                    .clone()
                    .ok_or("hold a cell in your main hand")?;
                if !super::ae2_storage_devices::is_valid_storage_cell(&held) {
                    return Err("invalid storage cell".into());
                }
                let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&p) else {
                    return Err("missing machine".into());
                };
                if !matches!(m.kind, MachineKind::Drive | MachineKind::Chest) {
                    return Err("this machine is not a drive or ME chest".into());
                }
                let dest = m.slots.get_mut(slot).ok_or("invalid cell slot")?;
                if dest.is_some() {
                    return Err("cell slot is occupied".into());
                }
                *dest = Some(held);
                self.inventory[self.selected] = None;
                self.world.modified.insert((p.0 >> 4, p.2 >> 4));
                Ok(json!({"installed":true,"slot":slot}))
            }
            "part" => {
                let face = w.get(4).copied().unwrap_or("north");
                if !["north", "south", "east", "west", "up", "down"].contains(&face) {
                    return Err("face must name a block side".into());
                }
                let held = self.held().cloned().ok_or("hold an AE2 part")?;
                if !matches!(
                    &*held.item,
                    "ae2:terminal"
                        | "ae2:crafting_terminal"
                        | "ae2:pattern_encoding_terminal"
                        | "ae2:pattern_access_terminal"
                        | "ae2:import_bus"
                        | "ae2:export_bus"
                        | "ae2:storage_bus"
                        | "ae2:level_emitter"
                        | "ae2:energy_level_emitter"
                        | "ae2:toggle_bus"
                        | "ae2:inverted_toggle_bus"
                        | "ae2:annihilation_plane"
                        | "ae2:formation_plane"
                        | "ae2:me_p2p_tunnel"
                        | "ae2:item_p2p_tunnel"
                        | "ae2:fluid_p2p_tunnel"
                        | "ae2:fe_p2p_tunnel"
                        | "ae2:redstone_p2p_tunnel"
                        | "ae2:light_p2p_tunnel"
                        | "ae2:cable_energy_acceptor"
                        | "ae2:cable_anchor"
                        | "ae2:quartz_fiber"
                        | "ae2:monitor"
                        | "ae2:semi_dark_monitor"
                        | "ae2:dark_monitor"
                        | "ae2:storage_monitor"
                        | "ae2:conversion_monitor"
                        | "ae2:cable_interface"
                        | "ae2:cable_pattern_provider"
                ) {
                    return Err("unsupported part".into());
                }
                // ToggleBusPart.addToWorld samples the real host signal before
                // creating its internal connection.
                let toggle_signal = if matches!(held.item.as_ref(), "ae2:toggle_bus" | "ae2:inverted_toggle_bus") {
                    let at = horizon_core::position::BlockPos::new(p.0, p.1, p.2);
                    Some(self.with_level(|blocks, level, ticks| blocks.has_neighbor_signal(level, ticks, at)))
                } else { None };
                let player_owner=self.ae2_player_owner_tag();
                let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&p) else {
                    return Err("missing cable bus".into());
                };
                if m.kind != MachineKind::CableBus {
                    return Err("parts attach to cable buses".into());
                }
                let entry = m
                    .config
                    .entry("parts".into())
                    .or_insert_with(|| Tag::List(vec![]));
                let Tag::List(parts) = entry else {
                    return Err("invalid cable part data".into());
                };
                if parts
                    .iter()
                    .any(|t| t.get("face") == Some(&Tag::String(face.into())))
                {
                    return Err("this cable face already holds a part".into());
                }
                let mut attached = Tag::compound([
                    player_owner,
                    ("id", Tag::String(held.item.as_ref().into())),
                    ("face", Tag::String(face.into())),
                    ("ae2_part_identity", Tag::IntArray(horizon_client::save::random_uuid().to_vec())),
                    (
                        "stack",
                        horizon_client::save::item_to_nbt(&ItemStack {
                            count: 1,
                            ..held.clone()
                        }),
                    ),
                ]);
                if let (Some(signal), Tag::Compound(data)) = (toggle_signal, &mut attached) {
                    data.insert("toggle_signal".into(), Tag::Byte(i8::from(signal)));
                }
                parts.push(attached.clone());
                if held.item.ends_with("terminal") && held.item.as_ref()!="ae2:pattern_access_terminal" {
                    m.config.insert("terminal_part".into(), Tag::Byte(1));
                }
                self.world.block_entities.record_ae2_part(p, attached, true);
                if !self.creative {
                    *self.held_mut() = if held.count > 1 {
                        Some(ItemStack {
                            count: held.count - 1,
                            ..held
                        })
                    } else {
                        None
                    };
                }
                self.world.modified.insert((p.0 >> 4, p.2 >> 4));
                if self.ae2_has_anchor_owners() {
                    if let Err(error)=self.ae2_apply_anchor_callbacks(){log::debug!("AE2 anchor callbacks deferred: {error}");}
                }
                Ok(json!({"attached":true,"face":face}))
            }
            "simulate" => {
                let held=self.inventory[self.selected].clone().ok_or("hold an item to simulate")?;
                let wanted:u64=w.get(4).map_or(Ok(u64::from(held.count)),|s|s.parse()).map_err(|_|"item count")?;
                let n=self.ae2_grid_insert_simulate(p,&held,wanted)?;
                Ok(json!({"accepted":n}))
            }
            "deposit" => {
                if self.ae2_has_terminal(p) && !self.ae2_terminal_online(p) {
                    return Err("terminal is offline".into());
                }
                let held = self.inventory[self.selected]
                    .clone()
                    .ok_or("hold an item to deposit")?;
                let wanted: u64 = w
                    .get(4)
                    .map_or(Ok(u64::from(held.count)), |s| s.parse())
                    .map_err(|_| "item count")?;
                let n = self.ae2_grid_insert(p, &held, wanted)?;
                self.inventory[self.selected] = if n == u64::from(held.count) {
                    None
                } else {
                    Some(ItemStack {
                        count: held.count - n as u32,
                        ..held
                    })
                };
                Ok(json!({"inserted":n}))
            }
            "withdraw" => {
                if self.ae2_has_terminal(p) && !self.ae2_terminal_online(p) {
                    return Err("terminal is offline".into());
                }
                let index: usize = w
                    .get(4)
                    .ok_or("entry index")?
                    .parse()
                    .map_err(|_| "entry index")?;
                let wanted: u64 = w
                    .get(5)
                    .map_or(Ok(64), |s| s.parse())
                    .map_err(|_| "item count")?;
                let entries = self.ae2_grid_inventory(p)?;
                let (k, available) = entries.get(index).ok_or("entry out of range")?;
                let mut proto = ae2_lab::prototype(k)?;
                let max = u64::from(self.res.items().stack_max(&proto));
                let desired = wanted.min(*available).min(max);
                proto.count = desired as u32;
                let before = self.inventory.clone();
                let left = self.give(proto).map_or(0, |s| u64::from(s.count));
                self.inventory = before;
                let capacity = desired - left;
                if let Some(s) = self.ae2_grid_extract(p, k, capacity)? {
                    let n = s.count;
                    let left = self.give(s);
                    if left.is_some() {
                        return Err("inventory changed during transfer".into());
                    }
                    Ok(json!({"extracted":n}))
                } else {
                    Ok(json!({"extracted":0}))
                }
            }
            _ => Err("unknown grid operation".into()),
        }
    }
}

/// AE2 PatternProviderBlockEntity.getGridConnectableSides excludes its
/// dedicated push face; ALL exposes every side. Missing is the legacy default.
pub(super) fn pattern_provider_faces(direction: Option<&str>) -> u8 {
    match direction {
        None | Some("all") => 63,
        Some(face) => face_index(face).map_or(0, |index| 63 & !(1 << index)),
    }
}
/// AE2 GrowthAcceleratorBlockEntity exposes only its facing axis.
pub(super) fn growth_accelerator_faces(facing: Option<&str>) -> u8 {
    match facing {
        Some("east" | "west") => 0b000011,
        Some("up" | "down") => 0b001100,
        Some("north" | "south") | None => 0b110000,
        Some(_) => 0,
    }
}

#[cfg(test)]
mod growth_axis_tests {
    use super::growth_accelerator_faces;
    #[test]
    fn source_pattern_provider_excludes_only_the_selected_push_face() {
        use super::pattern_provider_faces;
        assert_eq!(pattern_provider_faces(None), 63);
        assert_eq!(pattern_provider_faces(Some("all")), 63);
        for (name, index) in [("east",0), ("west",1), ("up",2), ("down",3), ("south",4), ("north",5)] {
            assert_eq!(pattern_provider_faces(Some(name)), 63 & !(1 << index));
        }
        assert_eq!(pattern_provider_faces(Some("invalid")), 0);
    }
    #[test]
    fn source_growth_accelerator_exposes_only_front_and_back() {
        for (facing, expected) in [("east",3), ("west",3), ("up",12), ("down",12), ("north",48), ("south",48)] {
            assert_eq!(growth_accelerator_faces(Some(facing)), expected);
        }
        assert_eq!(growth_accelerator_faces(None), 48);
        assert_eq!(growth_accelerator_faces(Some("invalid")), 0);
    }
}
