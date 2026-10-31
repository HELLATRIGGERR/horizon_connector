//! Incremental in-world AE2 node connection events. This accepts each actual
//! node lifecycle mutation in order; it does not infer events from a final graph.
use crate::{ChannelMode, Device, Network, NodeIdentity, Pos, service_events::OrderedServices};
use std::collections::BTreeMap;

/// `InWorldGridNode.findInWorldConnections` iterates Direction's enum order.
fn neighbors(p: Pos) -> impl Iterator<Item = Pos> {
    [
        (0, -1, 0),
        (0, 1, 0),
        (0, 0, -1),
        (0, 0, 1),
        (-1, 0, 0),
        (1, 0, 0),
    ]
    .into_iter()
    .filter_map(move |(x, y, z)| {
        Some(Pos {
            x: p.x.checked_add(x)?,
            y: p.y.checked_add(y)?,
            z: p.z.checked_add(z)?,
        })
    })
}
#[derive(Clone, Default)]
pub struct PhysicalTopology {
    network: Network,
    nodes: BTreeMap<Pos, NodeIdentity>,
    // A Phantom Node's second managed in-world node has its own identity and
    // may outlive the block entity. The host's externally returned node is a
    // separate selection, installed only after proxy.create() returns.
    co_nodes: BTreeMap<NodeIdentity, (Pos, Device)>,
    exposed_co_nodes: BTreeMap<Pos, NodeIdentity>,
    parts: BTreeMap<(Pos, u8), NodeIdentity>,
    virtuals: std::collections::BTreeSet<NodeIdentity>,
    explicit_links: std::collections::BTreeSet<(NodeIdentity, NodeIdentity)>,
    ports: BTreeMap<(Pos, u8), (NodeIdentity, Device)>,
    pub services: OrderedServices,
}
impl PhysicalTopology {
    /// The last actually applied mask for a physical host node. Lookup by
    /// incarnation avoids rebinding an old callback to a replacement block.
    pub fn node_faces(&self, id: NodeIdentity) -> Option<u8> {
        let pos = self.nodes.iter().find_map(|(pos, node)| (*node == id).then_some(pos))?;
        self.network.nodes.get(pos).map(|device| device.faces)
    }
    /// Apply one real host-side connectable-sides callback without replacing
    /// its other device properties or changing callback order.
    pub fn update_node_faces(&mut self, id: NodeIdentity, faces: u8) -> Result<(), &'static str> {
        if faces & !63 != 0 { return Err("invalid node face mask"); }
        let pos = self.nodes.iter().find_map(|(pos, node)| (*node == id).then_some(*pos))
            .ok_or("missing physical node")?;
        let mut device = self.network.nodes.get(&pos).cloned().ok_or("missing physical device")?;
        if device.faces == faces { return Ok(()); }
        device.faces = faces;
        self.update_node(pos, device)
    }
    /// A real node becomes ready at this position. Side parts and virtual
    /// links must be supplied separately by their own ordered events.
    pub fn node_ready(
        &mut self,
        pos: Pos,
        id: NodeIdentity,
        device: Device,
        saved_mode: Option<ChannelMode>,
    ) -> Result<(), &'static str> {
        if self.nodes.contains_key(&pos) || !device.attached.is_empty() {
            return Err("occupied position or unsupported embedded parts");
        }
        self.services.add_node(id, saved_mode)?;
        self.network.nodes.insert(pos, device);
        self.nodes.insert(pos, id);
        for neighbor in neighbors(pos) {
            if let Some(other) = self.exposed_neighbor(pos, &self.network.nodes[&pos], neighbor) {
                self.services.connect(id, other)?;
            }
            for (port, _) in self
                .neighbor_ports(pos, &self.network.nodes[&pos])
                .into_iter()
                .filter(|(_, host)| *host == neighbor)
            {
                self.services.connect(id, port)?;
            }
        }
        self.services.ensure_grid(id)
    }

    /// Resolve the in-world node actually returned by the neighboring host.
    /// A Phantom Node switches this result to its second node after creation;
    /// the returned node still has to pass GridHelper's exposure/color check.
    fn exposed_neighbor(&self, pos: Pos, device: &Device, neighbor: Pos) -> Option<NodeIdentity> {
        if let Some(id) = self.exposed_co_nodes.get(&neighbor) {
            let (at, other) = self.co_nodes.get(id)?;
            return (*at == neighbor && Self::compatible(pos, device, neighbor, other)).then_some(*id);
        }
        let id = *self.nodes.get(&neighbor)?;
        Self::compatible(pos, device, neighbor, self.network.nodes.get(&neighbor)?).then_some(id)
    }

    fn replace_phantom_side(&mut self, node: NodeIdentity, neighbor: Pos, next: NodeIdentity) -> Result<(), &'static str> {
        // InWorldGridNode scans its existing connections in insertion order.
        // An old phantom connection on this side is replaced by the node the
        // host now returns, rather than joining both merely by proximity.
        for old in self.services.connections(node) {
            let edge = if node < old { (node, old) } else { (old, node) };
            if self.explicit_links.contains(&edge) { continue; }
            let old_at = self.co_nodes.get(&old).map(|(pos, _)| *pos)
                .or_else(|| self.nodes.iter().find_map(|(pos, id)| (*id == old).then_some(*pos)));
            if old_at == Some(neighbor) {
                // Java stops scanning the side as soon as it finds the node
                // already returned by the host, even if later edges exist.
                if old == next { return Ok(()); }
                if self.co_nodes.contains_key(&old) || self.co_nodes.contains_key(&next) {
                    self.services.disconnect(node, old)?;
                    // Do not suppress the source's duplicate-connection error
                    // if another edge already connects the newly returned node.
                    return self.services.connect(node, next);
                }
            }
        }
        if !self.services.connected(node, next) { self.services.connect(node, next)?; }
        Ok(())
    }

    /// Create a second real in-world node at an already occupied host, without
    /// automatically linking it to the main node or changing host lookup yet.
    pub fn colocated_ready(&mut self, pos: Pos, id: NodeIdentity, device: Device) -> Result<(), &'static str> {
        if !id.is_phantom_proxy() || self.nodes.get(&pos).is_none_or(|main| main.host != id.host)
            || self.co_nodes.contains_key(&id)
            || self.services.service_id(id).is_some() || !device.attached.is_empty()
            || self.co_nodes.len() >= 100_000
        {
            return Err("invalid or duplicate co-located node");
        }
        let before = self.clone();
        let result = (|| {
            self.services.add_node(id, None)?;
            for neighbor in neighbors(pos) {
                if let Some(other) = self.exposed_neighbor(pos, &device, neighbor) {
                    self.services.connect(id, other)?;
                }
                for (port, _) in self.neighbor_ports(pos, &device).into_iter().filter(|(_, host)| *host == neighbor) {
                    self.services.connect(id, port)?;
                }
            }
            self.services.ensure_grid(id)?;
            self.co_nodes.insert(id, (pos, device));
            Ok(())
        })();
        if result.is_err() { *self = before; }
        result
    }

    /// Called only after the second node's create/ready callback returns.
    pub fn expose_colocated(&mut self, pos: Pos, id: NodeIdentity) -> Result<(), &'static str> {
        if self.co_nodes.get(&id).is_none_or(|(at, _)| *at != pos)
            || self.nodes.get(&pos).is_none_or(|main| main.host != id.host)
        {
            return Err("co-located host or node changed");
        }
        self.exposed_co_nodes.insert(pos, id);
        Ok(())
    }

    /// Co-located nodes include unexposed, still-live proxies after host
    /// unload/removal; consumers must retain their distinct identities.
    pub fn colocated_nodes(&self) -> impl Iterator<Item = (NodeIdentity, Pos, &Device)> {
        self.co_nodes.iter().map(|(id, (pos, device))| (*id, *pos, device))
    }

    pub fn exposed_colocated(&self, pos: Pos) -> Option<NodeIdentity> {
        self.exposed_co_nodes.get(&pos).copied()
    }

    /// Project observed Phantom edges into the gameplay graph. Synthetic
    /// positions are only temporary indices; identity is the managed node.
    /// A failed projection leaves the caller's graph untouched.
    pub fn project_colocated(&self, network: &mut Network) -> Result<(), &'static str> {
        if self.co_nodes.is_empty() { return Ok(()); }
        let mut next = network.clone();
        let mut by_identity = BTreeMap::new();
        for (pos, id) in &next.node_identities {
            if by_identity.insert(*id, *pos).is_some() { return Err("duplicate gameplay node identity"); }
        }
        let mut affected = Vec::new();
        let mut main_positions = std::collections::BTreeSet::new();
        for (index, (id, (pos, device))) in self.co_nodes.iter().enumerate() {
            let x = i32::try_from(index).ok().and_then(|v| v.checked_mul(2))
                .and_then(|v| i32::MIN.checked_add(v)).ok_or("too many co-located nodes")?;
            // Y=32768 is outside every encoded world dimension.
            let index_pos = Pos { x, y: 32768, z: 0 };
            if next.nodes.contains_key(&index_pos) || by_identity.contains_key(id) {
                return Err("co-located projection collision");
            }
            let mut projected = device.clone();
            projected.faces = 0;
            next.nodes.insert(index_pos, projected);
            next.node_identities.insert(index_pos, *id);
            by_identity.insert(*id, index_pos);
            affected.push(*id);
            if self.exposed_co_nodes.get(pos) == Some(id) {
                next.physical_exposed.insert(*pos, index_pos);
            }
            if let Some(main) = self.nodes.get(pos).filter(|main| main.host == id.host) {
                if by_identity.get(main) != Some(pos) { return Err("co-located host projection missing"); }
                main_positions.insert(*pos);
                affected.push(*main);
            }
        }
        // A rebuilt adjacency snapshot cannot tell which of the two managed
        // nodes owns a side, or which old phantom edges still exist.
        next.physical_suppressed.extend(main_positions.iter().copied());
        next.links.retain(|(a, b)| !main_positions.contains(a) && !main_positions.contains(b));
        for id in affected {
            let at = *by_identity.get(&id).ok_or("co-located endpoint unavailable")?;
            for peer in self.services.connections(id) {
                let other = *by_identity.get(&peer).ok_or("co-located peer unavailable")?;
                next.connect(at, other)?;
            }
        }
        *network = next;
        Ok(())
    }
    /// Exposed sides/color changed on a still-live physical node. Source first
    /// cleans stale connections, then discovers new ones in Direction order.
    pub fn update_node(&mut self, pos: Pos, device: Device) -> Result<(), &'static str> {
        if !device.attached.is_empty() {
            return Err("unsupported embedded parts");
        }
        let id = *self.nodes.get(&pos).ok_or("missing physical node")?;
        let previous = self.network.nodes.get(&pos).ok_or("missing physical device")?;
        let near_colocated = self.co_nodes.values().any(|(at, _)| *at == pos || neighbors(pos).any(|p| p == *at));
        if near_colocated && previous.faces == device.faces && previous.color == device.color {
            // setExposedOnSides does not call updateState for the same sides.
            self.network.nodes.insert(pos, device);
            return Ok(());
        }
        let existing = self.services.connections(id);
        self.network.nodes.insert(pos, device);
        let next_ports = self.neighbor_ports(pos, &self.network.nodes[&pos]);
        // Iterate the actual connection list, including ports, so disconnects
        // retain source pivot order when several neighbors disappear at once.
        for other in existing {
            let edge = if id < other { (id, other) } else { (other, id) };
            if self.explicit_links.contains(&edge) {
                continue;
            }
            if self.parts.values().any(|part| *part == other) {
                continue;
            }
            if let Some((other_pos, other_device)) = self.co_nodes.get(&other) {
                if !Self::compatible(pos, &self.network.nodes[&pos], *other_pos, other_device) {
                    self.services.disconnect(id, other)?;
                }
            } else if self.ports.values().any(|(port, _)| *port == other) {
                if !next_ports.iter().any(|(port, _)| *port == other) {
                    self.services.disconnect(id, other)?;
                }
            } else if self.virtuals.contains(&other) {
                // An explicit nonphysical link has a separate lifecycle.
                continue;
            } else {
                let other_pos = self
                    .nodes
                    .iter()
                    .find_map(|(p, n)| (*n == other).then_some(*p))
                    .ok_or("external connection needs its own event")?;
                if !self.network.physically_connects(pos, other_pos) {
                    self.services.disconnect(id, other)?;
                }
            }
        }
        for other_pos in neighbors(pos) {
            if let Some(other) = self.exposed_neighbor(pos, &self.network.nodes[&pos], other_pos) {
                self.replace_phantom_side(id, other_pos, other)?;
            }
            for (port, _) in next_ports.iter().filter(|(_, host)| *host == other_pos) {
                if !self.services.connected(id, *port) {
                    self.services.connect(id, *port)?;
                }
            }
        }
        Ok(())
    }
    // A virtual outer node shares the host block coordinate but has its own
    // in-world side. Compare physical faces/colors using the regular policy.
    fn neighbor_ports(&self, pos: Pos, node: &Device) -> Vec<(NodeIdentity, Pos)> {
        let mut found = Vec::new();
        for other in neighbors(pos) {
            for ((host, _), (id, device)) in &self.ports {
                if *host == other && Self::compatible(pos, node, other, device) {
                    found.push((*id, other));
                }
            }
        }
        found
    }
    fn compatible(a: Pos, left: &Device, b: Pos, right: &Device) -> bool {
        let mut pair = Network::default();
        pair.nodes.insert(a, left.clone());
        pair.nodes.insert(b, right.clone());
        pair.physically_connects(a, b)
    }
    /// Create an actual in-world outer node and discover its neighboring node
    /// before allocating a standalone grid, as InWorldGridNode.updateState does.
    pub fn virtual_ready_at(
        &mut self,
        pos: Pos,
        face: u8,
        id: NodeIdentity,
        mut device: Device,
        saved_mode: Option<ChannelMode>,
    ) -> Result<(), &'static str> {
        if face >= 6 || self.ports.contains_key(&(pos, face)) || self.virtuals.contains(&id) {
            return Err("invalid or occupied virtual face");
        }
        let before = self.clone();
        let result = (|| {
            device.faces = 1 << face;
            self.services.add_node(id, saved_mode)?;
            for neighbor in neighbors(pos) {
                if let Some(other) = self.exposed_neighbor(pos, &device, neighbor) {
                    self.services.connect(id, other)?;
                }
                for ((host, _), (other, other_device)) in &self.ports {
                    if *host == neighbor && Self::compatible(pos, &device, neighbor, other_device) {
                        self.services.connect(id, *other)?;
                    }
                }
            }
            self.services.ensure_grid(id)?;
            self.virtuals.insert(id);
            self.ports.insert((pos, face), (id, device));
            Ok(())
        })();
        if result.is_err() {
            *self = before;
        }
        result
    }
    /// Virtual outer nodes and explicit links are reported by their owning
    /// source adapter in actual creation/destruction order.
    pub fn virtual_ready(
        &mut self,
        id: NodeIdentity,
        saved_mode: Option<ChannelMode>,
    ) -> Result<(), &'static str> {
        if self.virtuals.contains(&id) {
            return Err("duplicate virtual node");
        }
        self.services.add_node(id, saved_mode)?;
        self.services.ensure_grid(id)?;
        self.virtuals.insert(id);
        Ok(())
    }
    pub fn virtual_destroyed(&mut self, id: NodeIdentity) -> Result<(), &'static str> {
        if !self.virtuals.contains(&id) {
            return Err("missing virtual node");
        }
        self.services.remove_node(id)?;
        self.explicit_links.retain(|(a,b)| *a != id && *b != id);
        self.virtuals.remove(&id);
        self.ports.retain(|_, (port, _)| *port != id);
        Ok(())
    }
    pub fn explicit_connect(
        &mut self,
        a: NodeIdentity,
        b: NodeIdentity,
    ) -> Result<(), &'static str> {
        self.services.connect(a, b)?;
        self.explicit_links.insert(if a < b { (a,b) } else { (b,a) });
        Ok(())
    }
    pub fn explicit_disconnect(
        &mut self,
        a: NodeIdentity,
        b: NodeIdentity,
    ) -> Result<(), &'static str> {
        self.services.disconnect(a, b)?;
        self.explicit_links.remove(&if a < b { (a,b) } else { (b,a) });
        Ok(())
    }
    /// A part on a host without a center owns its own independent grid node.
    /// `add_to_world` may create its outward-facing virtual node and links.
    pub fn centerless_part_ready(
        &mut self,
        pos: Pos,
        face: u8,
        id: NodeIdentity,
        saved_mode: Option<ChannelMode>,
        add_to_world: impl FnOnce(&mut Self) -> Result<(), &'static str>,
    ) -> Result<(), &'static str> {
        if face >= 6 || self.nodes.contains_key(&pos) || self.parts.contains_key(&(pos, face)) {
            return Err("invalid centerless part face");
        }
        let before = self.clone();
        let result = (|| {
            self.services.add_node(id, saved_mode)?;
            self.services.ensure_grid(id)?;
            self.parts.insert((pos, face), id);
            add_to_world(self)
        })();
        if result.is_err() { *self = before; }
        result
    }
    /// CableBusContainer.addPart(center): ready the real center before
    /// connecting existing side nodes in Direction.values() order.
    pub fn center_added(
        &mut self,
        pos: Pos,
        id: NodeIdentity,
        device: Device,
        saved_mode: Option<ChannelMode>,
    ) -> Result<(), &'static str> {
        let before = self.clone();
        let result = (|| {
            self.node_ready(pos, id, device, saved_mode)?;
            for face in [3, 2, 5, 4, 1, 0] {
                if let Some(part) = self.parts.get(&(pos, face)) {
                    self.services.connect(id, *part)?;
                }
            }
            Ok(())
        })();
        if result.is_err() { *self = before; }
        result
    }
    /// Removing a center destroys only its node; side parts stay physical and
    /// their now-independent services are determined by the ordered edges.
    pub fn center_destroyed(&mut self, pos: Pos) -> Result<(), &'static str> {
        let id = *self.nodes.get(&pos).ok_or("missing center node")?;
        self.services.remove_node(id)?;
        self.explicit_links.retain(|(a,b)| *a != id && *b != id);
        self.nodes.remove(&pos);
        self.network.nodes.remove(&pos);
        Ok(())
    }
    /// A side part readies its own node, connects center-to-part, then the
    /// host updates the center's exposed sides (CableBusContainer.addPart).
    pub fn part_ready(
        &mut self,
        pos: Pos,
        face: u8,
        id: NodeIdentity,
        saved_mode: Option<ChannelMode>,
        center_after: Device,
    ) -> Result<(), &'static str> {
        self.part_ready_with(pos, face, id, saved_mode, center_after, |_| Ok(()))
    }

    /// `addToWorld` runs before the container connects the part to its center.
    /// Virtual-node adapters use this callback to create the outer node and
    /// any immediate internal connection in that source order. The whole part
    /// addition is rolled back if any phase fails.
    pub fn part_ready_with(
        &mut self,
        pos: Pos,
        face: u8,
        id: NodeIdentity,
        saved_mode: Option<ChannelMode>,
        center_after: Device,
        add_to_world: impl FnOnce(&mut Self) -> Result<(), &'static str>,
    ) -> Result<(), &'static str> {
        if face >= 6
            || self.parts.contains_key(&(pos, face))
            || !self.nodes.contains_key(&pos)
            || !center_after.attached.is_empty()
        {
            return Err("invalid or occupied part face");
        }
        let before = self.clone();
        let result = (|| {
            let center = self.nodes[&pos];
            self.services.add_node(id, saved_mode)?;
            self.services.ensure_grid(id)?;
            self.parts.insert((pos, face), id);
            add_to_world(self)?;
            self.services.connect(center, id)?;
            self.update_node(pos, center_after)
        })();
        if result.is_err() {
            *self = before;
        }
        result
    }
    /// CableBusContainer.removeFromWorld destroys side nodes without reopening
    /// exposed faces before it finally destroys the center.
    pub fn part_destroyed_with_host(&mut self, pos: Pos, face: u8) -> Result<(), &'static str> {
        self.part_destroyed_with_host_callback(pos, face, |_| Ok(()))
    }
    /// Virtual parts destroy their main node first and their outer node next,
    /// before the container proceeds to destroy its center node.
    pub fn part_destroyed_with_host_callback(
        &mut self,
        pos: Pos,
        face: u8,
        remove_from_world: impl FnOnce(&mut Self) -> Result<(), &'static str>,
    ) -> Result<(), &'static str> {
        let id = *self.parts.get(&(pos, face)).ok_or("missing part")?;
        let before = self.clone();
        let result = (|| {
            self.services.remove_node(id)?;
            self.explicit_links.retain(|(a,b)| *a != id && *b != id);
            self.parts.remove(&(pos, face));
            remove_from_world(self)
        })();
        if result.is_err() {
            *self = before;
        }
        result
    }
    /// Source destroys the side node first, then exposes that host face.
    pub fn part_destroyed(
        &mut self,
        pos: Pos,
        face: u8,
        center_after: Device,
    ) -> Result<(), &'static str> {
        self.part_destroyed_with(pos, face, center_after, |_| Ok(()))
    }
    /// Run the part's remaining removal steps before exposing its host face.
    pub fn part_destroyed_with(
        &mut self,
        pos: Pos,
        face: u8,
        center_after: Device,
        remove_from_world: impl FnOnce(&mut Self) -> Result<(), &'static str>,
    ) -> Result<(), &'static str> {
        if !center_after.attached.is_empty() {
            return Err("unsupported embedded parts");
        }
        let id = *self.parts.get(&(pos, face)).ok_or("missing part")?;
        let before = self.clone();
        let result = (|| {
            self.services.remove_node(id)?;
            self.explicit_links.retain(|(a,b)| *a != id && *b != id);
            self.parts.remove(&(pos, face));
            remove_from_world(self)?;
            self.update_node(pos, center_after)
        })();
        if result.is_err() {
            *self = before;
        }
        result
    }
    pub fn node_destroyed(&mut self, pos: Pos) -> Result<(), &'static str> {
        if self.parts.keys().any(|(host, _)| *host == pos) {
            return Err("side parts must be destroyed first");
        }
        let id = *self.nodes.get(&pos).ok_or("missing physical node")?;
        self.services.remove_node(id)?;
        self.explicit_links.retain(|(a,b)| *a != id && *b != id);
        self.nodes.remove(&pos);
        self.network.nodes.remove(&pos);
        self.exposed_co_nodes.remove(&pos);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ControllerState, DeviceKind};
    fn p(x: i32) -> Pos {
        Pos { x, y: 0, z: 0 }
    }
    fn id(host: u64) -> NodeIdentity {
        NodeIdentity {
            host,
            part: None,
            outer: false,
        }
    }
    #[test]
    fn phantom_colocated_nodes_follow_creation_exposure_and_removal() {
        let mut g = PhysicalTopology::default();
        let main = id(1);
        let proxy = NodeIdentity { outer: true, ..main };
        let neighbor = id(2);
        let mut phantom = Device::new(DeviceKind::PoweredMachine);
        phantom.idle_override = Some(1.0);
        g.node_ready(p(0), neighbor, Device::new(DeviceKind::GlassCable), None).unwrap();
        g.node_ready(p(1), main, phantom.clone(), None).unwrap();
        g.colocated_ready(p(1), proxy, phantom).unwrap();
        assert!(g.services.connected(neighbor, main));
        assert!(g.services.connected(neighbor, proxy));
        assert!(!g.services.connected(main, proxy));
        assert_eq!(g.exposed_colocated(p(1)), None);
        g.expose_colocated(p(1), proxy).unwrap();
        assert_eq!(g.exposed_colocated(p(1)), Some(proxy));
        let mut network = Network::default();
        network.nodes.insert(p(0), Device::new(DeviceKind::GlassCable));
        let mut active = Device::new(DeviceKind::PoweredMachine);
        active.idle_override = Some(1.0);
        network.nodes.insert(p(1), active);
        network.node_identities.insert(p(0), neighbor);
        network.node_identities.insert(p(1), main);
        g.project_colocated(&mut network).unwrap();
        let analysis = network.analyze().unwrap();
        assert_eq!(analysis.components.len(), 1);
        assert_eq!(analysis.components[0].positions.len(), 3);
        assert_eq!(analysis.components[0].idle_per_tick, 2.0);
        // A later source neighbor update removes the stale first edge, then
        // attempts to create an existing edge: the deliberate source error.
        let mut refreshed = g.clone();
        let mut changed = Device::new(DeviceKind::GlassCable);
        changed.faces = 1; // retain the east side while changing the mask
        assert_eq!(refreshed.update_node(p(0), changed),
            Err("invalid or duplicate connection"));
        assert!(!refreshed.services.connected(neighbor, main));
        assert!(refreshed.services.connected(neighbor, proxy));
        g.node_destroyed(p(1)).unwrap();
        assert_eq!(g.exposed_colocated(p(1)), None);
        assert_eq!(g.services.service_id(main), None);
        assert!(g.services.connected(neighbor, proxy));
        assert_eq!(g.colocated_nodes().map(|(id, _, _)| id).collect::<Vec<_>>(), vec![proxy]);
        let mut orphan_network = Network::default();
        orphan_network.nodes.insert(p(0), Device::new(DeviceKind::GlassCable));
        orphan_network.node_identities.insert(p(0), neighbor);
        g.project_colocated(&mut orphan_network).unwrap();
        let after = orphan_network.analyze().unwrap();
        assert_eq!(after.components.len(), 1);
        assert_eq!(after.components[0].positions.len(), 2);
        assert_eq!(after.components[0].idle_per_tick, 1.0);
    }

    #[test]
    fn phantom_ready_before_neighbor_selects_proxy_and_replacement_is_distinct() {
        let mut g = PhysicalTopology::default();
        let main = id(1);
        let proxy = NodeIdentity { outer: true, ..main };
        g.node_ready(p(1), main, Device::new(DeviceKind::PoweredMachine), None).unwrap();
        g.colocated_ready(p(1), proxy, Device::new(DeviceKind::PoweredMachine)).unwrap();
        g.expose_colocated(p(1), proxy).unwrap();
        g.node_ready(p(0), id(2), Device::new(DeviceKind::GlassCable), None).unwrap();
        assert!(g.services.connected(id(2), proxy));
        assert!(!g.services.connected(id(2), main));
        assert_ne!(g.services.service_id(main), g.services.service_id(proxy));
        let mut network = Network::default();
        network.nodes.insert(p(0), Device::new(DeviceKind::GlassCable));
        network.nodes.insert(p(1), Device::new(DeviceKind::PoweredMachine));
        network.node_identities.insert(p(0), id(2));
        network.node_identities.insert(p(1), main);
        g.project_colocated(&mut network).unwrap();
        let analysis = network.analyze().unwrap();
        assert_ne!(analysis.nodes[&p(0)].component, analysis.nodes[&p(1)].component);
        assert!(network.physical_connection(p(0), p(1)), "visual connectivity follows the exposed proxy");
        g.node_destroyed(p(1)).unwrap();
        let replacement = id(3);
        let replacement_proxy = NodeIdentity { outer: true, ..replacement };
        g.node_ready(p(1), replacement, Device::new(DeviceKind::PoweredMachine), None).unwrap();
        g.colocated_ready(p(1), replacement_proxy, Device::new(DeviceKind::PoweredMachine)).unwrap();
        g.expose_colocated(p(1), replacement_proxy).unwrap();
        assert_ne!(proxy, replacement_proxy);
        assert_eq!(g.colocated_nodes().count(), 2);
        assert_eq!(g.exposed_colocated(p(1)), Some(replacement_proxy));
        assert!(g.services.connected(id(2), proxy));
    }
    #[test]
    fn observed_cpu_face_changes_split_and_join_the_actual_node() {
        let mut g=PhysicalTopology::default();
        g.services.start_membership_recording().unwrap();
        g.node_ready(p(0),id(1),Device::new(DeviceKind::CreativeEnergyCell),None).unwrap();
        let mut cpu=Device::new(DeviceKind::CraftingCpu);cpu.faces=0;
        g.node_ready(p(1),id(2),cpu,None).unwrap();
        assert_ne!(g.services.service_id(id(1)),g.services.service_id(id(2)));
        g.services.take_membership_changes().unwrap();
        g.update_node_faces(id(2),63).unwrap();
        assert_eq!(g.node_faces(id(2)),Some(63));
        assert_eq!(g.services.service_id(id(1)),g.services.service_id(id(2)));
        assert!(!g.services.take_membership_changes().unwrap().is_empty());
        g.update_node_faces(id(2),0).unwrap();
        assert_eq!(g.node_faces(id(2)),Some(0));
        assert_ne!(g.services.service_id(id(1)),g.services.service_id(id(2)));
        assert!(!g.services.take_membership_changes().unwrap().is_empty());
        assert_eq!(g.update_node_faces(id(20),63),Err("missing physical node"));
    }
    #[test]
    fn actual_ordered_node_events_preserve_controller_service_on_split() {
        let mut g = PhysicalTopology::default();
        for (x, kind) in [
            (0, DeviceKind::Controller),
            (1, DeviceKind::GlassCable),
            (2, DeviceKind::GlassCable),
            (3, DeviceKind::Drive),
        ] {
            g.node_ready(p(x), id((x + 1) as u64), Device::new(kind), None)
                .unwrap();
        }
        {
            let service = g.services.service_mut(id(1)).unwrap();
            service.channels = 1;
            service.controller = ControllerState::Online;
        }
        for (parent, child) in [(1, 2), (2, 3), (3, 4)] {
            g.services.orient_toward(id(parent), id(child)).unwrap();
        }
        g.node_destroyed(p(2)).unwrap();
        assert_eq!(g.services.service(id(1)).unwrap().channels, 1);
        assert_eq!(g.services.service(id(4)).unwrap().channels, 0);
        // Replacing at the same coordinate makes an actual new node identity.
        g.node_ready(p(2), id(20), Device::new(DeviceKind::GlassCable), None)
            .unwrap();
        assert!(g.services.service(id(20)).unwrap().members.contains(&id(4)));
    }
    #[test]
    fn explicit_virtual_connection_is_not_misclassified_as_a_physical_edge() {
        let mut g = PhysicalTopology::default();
        g.node_ready(p(0), id(1), Device::new(DeviceKind::GlassCable), None)
            .unwrap();
        let outer = NodeIdentity {
            host: 1,
            part: Some([1, 2, 3, 4]),
            outer: true,
        };
        g.virtual_ready(outer, None).unwrap();
        g.explicit_connect(id(1), outer).unwrap();
        g.update_node(p(0), Device::new(DeviceKind::GlassCable))
            .unwrap();
        assert!(g.services.connected(id(1), outer));
        g.explicit_disconnect(id(1), outer).unwrap();
        assert!(!g.services.connected(id(1), outer));
        g.virtual_destroyed(outer).unwrap();
    }
    #[test]
    fn physical_side_update_keeps_remote_explicit_link_until_its_own_event() {
        let mut g = PhysicalTopology::default();
        for (pos, node) in [(p(0), id(1)), (p(1), id(2)), (p(20), id(3))] {
            g.node_ready(pos, node, Device::new(DeviceKind::GlassCable), None).unwrap();
        }
        g.explicit_connect(id(1), id(3)).unwrap();
        let mut isolated = Device::new(DeviceKind::GlassCable);
        isolated.faces = 0;
        g.update_node(p(0), isolated).unwrap();
        assert!(!g.services.connected(id(1), id(2)));
        assert!(g.services.connected(id(1), id(3)));
        g.explicit_disconnect(id(1), id(3)).unwrap();
        assert!(!g.services.connected(id(1), id(3)));
        g.explicit_connect(id(1), id(3)).unwrap();
        g.node_destroyed(p(20)).unwrap();
        assert!(g.explicit_links.is_empty());
    }
    #[test]
    fn centerless_faces_stay_separate_until_real_center_is_added() {
        let mut graph = PhysicalTopology::default();
        let west = NodeIdentity { host: 1, part: Some([1,0,0,0]), outer: false };
        let east = NodeIdentity { host: 1, part: Some([2,0,0,0]), outer: false };
        graph.centerless_part_ready(p(0), 1, west, None, |_| Ok(())).unwrap();
        graph.centerless_part_ready(p(0), 0, east, None, |_| Ok(())).unwrap();
        assert_ne!(graph.services.service(west).unwrap().pivot, graph.services.service(east).unwrap().pivot);
        let mut center = Device::new(DeviceKind::GlassCable);
        center.faces &= !3;
        graph.center_added(p(0), id(1), center, None).unwrap();
        assert!(graph.services.connected(id(1), west));
        assert!(graph.services.connected(id(1), east));
        assert_eq!(graph.services.service(west).unwrap().pivot, graph.services.service(east).unwrap().pivot);
        graph.center_destroyed(p(0)).unwrap();
        assert_ne!(graph.services.service(west).unwrap().pivot, graph.services.service(east).unwrap().pivot);
        assert!(graph.services.service(west).is_some());
        assert!(graph.services.service(east).is_some());
    }
    #[test]
    fn centerless_toggle_connects_only_its_own_node_to_external_neighbor() {
        let mut graph = PhysicalTopology::default();
        graph.node_ready(p(1), id(2), Device::new(DeviceKind::Drive), None).unwrap();
        let toggle = NodeIdentity { host: 1, part: Some([1,0,0,0]), outer: false };
        let outer = NodeIdentity { outer: true, ..toggle };
        let other = NodeIdentity { host: 1, part: Some([2,0,0,0]), outer: false };
        graph.centerless_part_ready(p(0), 1, other, None, |_| Ok(())).unwrap();
        graph.centerless_part_ready(p(0), 0, toggle, None, |graph| {
            graph.virtual_ready_at(p(0), 0, outer, Device::new(DeviceKind::GlassCable), None)?;
            graph.explicit_connect(toggle, outer)
        }).unwrap();
        assert!(graph.services.connected(outer, id(2)));
        assert_eq!(graph.services.service(toggle).unwrap().pivot, graph.services.service(id(2)).unwrap().pivot);
        assert_ne!(graph.services.service(toggle).unwrap().pivot, graph.services.service(other).unwrap().pivot);
    }
    #[test]
    fn side_part_connects_to_center_before_occupied_face_is_blocked() {
        let mut g = PhysicalTopology::default();
        for (x, kind) in [
            (0, DeviceKind::Controller),
            (1, DeviceKind::GlassCable),
            (2, DeviceKind::Drive),
        ] {
            g.node_ready(p(x), id((x + 1) as u64), Device::new(kind), None)
                .unwrap();
        }
        g.services.orient_toward(id(1), id(2)).unwrap();
        g.services.orient_toward(id(2), id(3)).unwrap();
        let part = NodeIdentity {
            host: 2,
            part: Some([9, 8, 7, 6]),
            outer: false,
        };
        let mut blocked = Device::new(DeviceKind::GlassCable);
        blocked.faces &= !1;
        g.part_ready(p(1), 0, part, None, blocked).unwrap();
        assert!(g.services.connected(id(2), part));
        assert!(!g.services.connected(id(2), id(3)));
        assert!(g.services.service(id(1)).unwrap().members.contains(&part));
        g.part_destroyed(p(1), 0, Device::new(DeviceKind::GlassCable))
            .unwrap();
        assert!(g.services.connected(id(2), id(3)));
        assert!(g.services.service(part).is_none());
    }
    #[test]
    fn virtual_part_adds_outer_and_internal_link_before_center_and_rolls_back_failure() {
        let mut g = PhysicalTopology::default();
        g.node_ready(p(0), id(1), Device::new(DeviceKind::GlassCable), None)
            .unwrap();
        let inner = NodeIdentity {
            host: 1,
            part: Some([3, 1, 4, 1]),
            outer: false,
        };
        let outer = NodeIdentity {
            outer: true,
            ..inner
        };
        let mut blocked = Device::new(DeviceKind::GlassCable);
        blocked.faces &= !1;
        let fail = g.part_ready_with(p(0), 0, inner, None, blocked.clone(), |topology| {
            topology.virtual_ready(outer, None)?;
            topology.explicit_connect(inner, outer)?;
            assert!(!topology.services.connected(id(1), inner));
            Err("failed after creating outer")
        });
        assert!(fail.is_err());
        assert!(g.services.service(inner).is_none());
        assert!(g.services.service(outer).is_none());
        g.part_ready_with(p(0), 0, inner, None, blocked, |topology| {
            topology.virtual_ready(outer, None)?;
            topology.explicit_connect(inner, outer)?;
            assert!(!topology.services.connected(id(1), inner));
            Ok(())
        })
        .unwrap();
        assert!(g.services.connected(inner, outer));
        assert!(g.services.connected(id(1), inner));
        let removal =
            g.part_destroyed_with(p(0), 0, Device::new(DeviceKind::GlassCable), |topology| {
                assert!(topology.services.service(inner).is_none());
                assert!(topology.services.service(outer).is_some());
                topology.virtual_destroyed(outer)?;
                Err("failed after destroying outer")
            });
        assert!(removal.is_err());
        assert!(g.services.connected(inner, outer));
        g.part_destroyed_with(p(0), 0, Device::new(DeviceKind::GlassCable), |topology| {
            assert!(topology.services.service(inner).is_none());
            topology.virtual_destroyed(outer)
        })
        .unwrap();
        assert!(g.services.service(outer).is_none());
    }
    #[test]
    fn virtual_outer_discovers_real_neighbor_in_either_ready_order() {
        for port_first in [true, false] {
            let mut g = PhysicalTopology::default();
            g.node_ready(p(0), id(1), Device::new(DeviceKind::GlassCable), None)
                .unwrap();
            let outer = NodeIdentity {
                host: 1,
                part: Some([1, 2, 3, 4]),
                outer: true,
            };
            let mut center = Device::new(DeviceKind::GlassCable);
            center.faces &= !1;
            g.update_node(p(0), center).unwrap();
            if !port_first {
                g.node_ready(p(1), id(2), Device::new(DeviceKind::Drive), None)
                    .unwrap();
            }
            g.virtual_ready_at(p(0), 0, outer, Device::new(DeviceKind::GlassCable), None)
                .unwrap();
            if port_first {
                g.node_ready(p(1), id(2), Device::new(DeviceKind::Drive), None)
                    .unwrap();
            }
            assert!(g.services.connected(outer, id(2)));
            assert!(!g.services.connected(id(1), id(2)));
            let mut disconnected = Device::new(DeviceKind::Drive);
            disconnected.faces &= !2;
            g.update_node(p(1), disconnected).unwrap();
            assert!(!g.services.connected(outer, id(2)));
            g.update_node(p(1), Device::new(DeviceKind::Drive)).unwrap();
            assert!(g.services.connected(outer, id(2)));
            g.virtual_destroyed(outer).unwrap();
            assert!(!g.services.connected(outer, id(2)));
        }
    }
    #[test]
    fn facing_outer_ports_connect_without_joining_their_centers() {
        let mut g = PhysicalTopology::default();
        for x in [0, 1] {
            let mut center = Device::new(DeviceKind::GlassCable);
            center.faces &= !(if x == 0 { 1 } else { 2 });
            g.node_ready(p(x), id((x + 1) as u64), center, None)
                .unwrap();
        }
        let a = NodeIdentity {
            host: 1,
            part: Some([1, 0, 0, 0]),
            outer: true,
        };
        let b = NodeIdentity {
            host: 2,
            part: Some([2, 0, 0, 0]),
            outer: true,
        };
        g.virtual_ready_at(p(0), 0, a, Device::new(DeviceKind::GlassCable), None)
            .unwrap();
        g.virtual_ready_at(p(1), 1, b, Device::new(DeviceKind::GlassCable), None)
            .unwrap();
        assert!(g.services.connected(a, b));
        assert!(!g.services.connected(id(1), a));
        assert!(!g.services.connected(id(2), b));
        g.virtual_destroyed(a).unwrap();
        assert!(!g.services.connected(a, b));
    }
    #[test]
    fn side_change_disconnects_before_reconnecting_in_source_direction_order() {
        let mut g = PhysicalTopology::default();
        for (x, kind) in [
            (0, DeviceKind::Controller),
            (1, DeviceKind::GlassCable),
            (2, DeviceKind::Drive),
        ] {
            g.node_ready(p(x), id((x + 1) as u64), Device::new(kind), None)
                .unwrap();
        }
        g.services.orient_toward(id(1), id(2)).unwrap();
        g.services.orient_toward(id(2), id(3)).unwrap();
        let mut middle = Device::new(DeviceKind::GlassCable);
        middle.faces &= !1;
        g.update_node(p(1), middle).unwrap();
        assert_ne!(
            g.services.service(id(1)).unwrap().pivot,
            g.services.service(id(3)).unwrap().pivot
        );
    }
}
