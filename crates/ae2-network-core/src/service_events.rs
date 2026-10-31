//! Ordered grid-service ownership, based on AE2 GridConnection and GridNode.
//! This models actual mutation events; callers must supply them in source order.
use crate::{ChannelMode, ControllerState, NodeIdentity};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

type Node = NodeIdentity;
type EdgeKey = (Node, Node);
fn edge_key(a: Node, b: Node) -> EdgeKey {
    if a < b { (a, b) } else { (b, a) }
}
fn relax(a: Option<ChannelMode>, b: Option<ChannelMode>) -> Option<ChannelMode> {
    a.into_iter().chain(b).max_by_key(|v| v.ad_hoc_limit())
}
#[derive(Clone, Debug)]
struct Edge {
    a: Node,
    b: Node,
}
#[derive(Clone, Debug)]
pub struct GridService {
    pub pivot: Node,
    pub members: BTreeSet<Node>,
    pub powered: bool,
    pub forced_mode: Option<ChannelMode>,
    pub channels: usize,
    pub channel_power: f64,
    pub controller: ControllerState,
}
/// Exact mutation sequence emitted by the host's physical/virtual node
/// lifecycle. `Connect` preserves AE2 GridConnection.create's side ordering.
#[derive(Clone, Copy, Debug)]
pub enum GridEvent {
    NodeReady {
        node: Node,
        saved_mode: Option<ChannelMode>,
    },
    EnsureGrid {
        node: Node,
    },
    Connect {
        a: Node,
        b: Node,
    },
    Orient {
        toward_controller: Node,
        other: Node,
    },
    Disconnect {
        a: Node,
        b: Node,
    },
    NodeDestroyed {
        node: Node,
    },
    SetForcedMode {
        node: Node,
        mode: Option<ChannelMode>,
    },
}
/// One `GridNode.setGrid` transition, in source visitor order. A transfer
/// corresponds to the old grid's remove callback followed by the new grid's
/// add callback; no coordinate-based ordering is implied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MembershipChange {
    pub node: NodeIdentity,
    pub from: Option<u64>,
    pub to: Option<u64>,
}
#[derive(Clone, Debug, Default)]
pub struct OrderedServices {
    nodes: BTreeMap<Node, u64>,
    pending: BTreeMap<Node, Option<ChannelMode>>,
    edges: BTreeMap<EdgeKey, Edge>,
    incident: BTreeMap<Node, Vec<EdgeKey>>,
    services: BTreeMap<u64, GridService>,
    next_service: u64,
    membership_recording: bool,
    membership_overflowed: bool,
    membership_failure: Option<&'static str>,
    membership_changes: Vec<MembershipChange>,
}
impl OrderedServices {
    /// Begin before the first node exists; a later snapshot cannot recover
    /// actual registration order. Overflow is sticky and fails closed.
    pub fn start_membership_recording(&mut self) -> Result<(), &'static str> {
        if self.membership_recording { return Ok(()); }
        if !self.nodes.is_empty() || !self.pending.is_empty() {
            return Err("membership recording began after node creation");
        }
        self.membership_recording = true;
        Ok(())
    }
    fn record_membership(&mut self, node: Node, from: Option<u64>, to: Option<u64>) {
        const LIMIT: usize = 100_000;
        if !self.membership_recording || from == to { return; }
        if self.membership_changes.len() == LIMIT {
            self.membership_overflowed = true;
            return;
        }
        self.membership_changes.push(MembershipChange { node, from, to });
    }
    pub fn take_membership_changes(&mut self) -> Result<Vec<MembershipChange>, &'static str> {
        if !self.membership_recording { return Err("membership recording is disabled"); }
        if let Some(error) = self.membership_failure { return Err(error); }
        if self.membership_overflowed { return Err("membership recording overflow"); }
        Ok(std::mem::take(&mut self.membership_changes))
    }
    pub fn apply(&mut self, event: GridEvent) -> Result<(), &'static str> {
        match event {
            GridEvent::NodeReady { node, saved_mode } => self.add_node(node, saved_mode),
            GridEvent::EnsureGrid { node } => self.ensure_grid(node),
            GridEvent::Connect { a, b } => self.connect(a, b),
            GridEvent::Orient {
                toward_controller,
                other,
            } => self.orient_toward(toward_controller, other),
            GridEvent::Disconnect { a, b } => self.disconnect(a, b),
            GridEvent::NodeDestroyed { node } => self.remove_node(node),
            GridEvent::SetForcedMode { node, mode } => {
                self.service_mut(node)
                    .ok_or("missing grid service")?
                    .forced_mode = mode;
                Ok(())
            }
        }
    }
    fn new_service(
        &mut self,
        pivot: Node,
        ordered_members: impl IntoIterator<Item = Node>,
        forced_mode: Option<ChannelMode>,
    ) -> u64 {
        let id = self.next_service;
        self.next_service += 1;
        let ordered_members: Vec<_> = ordered_members.into_iter().collect();
        let members: BTreeSet<_> = ordered_members.iter().copied().collect();
        if self.membership_recording {
            for node in ordered_members {
                let from = self.nodes.insert(node, id);
                self.record_membership(node, from, Some(id));
            }
        } else {
            for node in &members { self.nodes.insert(*node, id); }
        }
        self.services.insert(
            id,
            GridService {
                pivot,
                members,
                powered: false,
                forced_mode,
                channels: 0,
                channel_power: 0.0,
                controller: ControllerState::None,
            },
        );
        id
    }
    pub fn add_node(
        &mut self,
        node: Node,
        saved_mode: Option<ChannelMode>,
    ) -> Result<(), &'static str> {
        if self.nodes.contains_key(&node) || self.pending.contains_key(&node) {
            return Err("duplicate grid node");
        }
        // GridNode.updateState discovers neighbors before getInternalGrid.
        self.pending.insert(node, saved_mode);
        Ok(())
    }
    pub fn ensure_grid(&mut self, node: Node) -> Result<(), &'static str> {
        if self.nodes.contains_key(&node) {
            return Ok(());
        }
        let saved = self.pending.remove(&node).ok_or("missing node")?;
        self.new_service(node, [node], saved);
        Ok(())
    }
    pub fn node_ids(&self) -> impl Iterator<Item = Node> + '_ {
        self.nodes.keys().copied()
    }
    pub fn service(&self, node: Node) -> Option<&GridService> {
        self.services.get(self.nodes.get(&node)?)
    }
    /// Runtime-local service incarnation; preserved by source-style merge
    /// winner selection and replaced for newly split services.
    pub fn service_id(&self, node: Node) -> Option<u64> {
        self.nodes.get(&node).copied()
    }
    pub fn service_mut(&mut self, node: Node) -> Option<&mut GridService> {
        let id = *self.nodes.get(&node)?;
        self.services.get_mut(&id)
    }
    pub fn connected(&self, a: Node, b: Node) -> bool {
        self.edges.contains_key(&edge_key(a, b))
    }
    pub fn connections(&self, node: Node) -> Vec<Node> {
        self.incident
            .get(&node)
            .into_iter()
            .flatten()
            .filter_map(|key| self.edges.get(key))
            .map(|edge| if edge.a == node { edge.b } else { edge.a })
            .collect()
    }
    /// `a` and `b` are the actual GridConnection.create arguments. AE2 keeps
    /// the powered grid, then the larger grid; A wins an equal-size tie.
    pub fn connect(&mut self, a: Node, b: Node) -> Result<(), &'static str> {
        if a == b || self.edges.contains_key(&edge_key(a, b)) {
            return Err("invalid or duplicate connection");
        }
        if !self.nodes.contains_key(&a) && !self.pending.contains_key(&a) {
            return Err("missing node A");
        }
        if !self.nodes.contains_key(&b) && !self.pending.contains_key(&b) {
            return Err("missing node B");
        }
        match (self.nodes.get(&a).copied(), self.nodes.get(&b).copied()) {
            (None, None) => {
                let mode = relax(
                    self.pending.remove(&a).flatten(),
                    self.pending.remove(&b).flatten(),
                );
                self.new_service(a, [a, b], mode);
            }
            (Some(existing), None) | (None, Some(existing)) => {
                let added = if self.pending.contains_key(&a) { a } else { b };
                let mode = self.pending.remove(&added).flatten();
                let service = self
                    .services
                    .get_mut(&existing)
                    .ok_or("missing grid service")?;
                service.forced_mode = relax(service.forced_mode, mode);
                service.members.insert(added);
                self.nodes.insert(added, existing);
                self.record_membership(added, None, Some(existing));
            }
            (Some(left), Some(right)) if left != right => {
                let x = &self.services[&left];
                let y = &self.services[&right];
                let a_wins = (x.powered, x.members.len()) >= (y.powered, y.members.len());
                let (winner, loser) = if a_wins { (left, right) } else { (right, left) };
                // GridPropagator starts at the losing endpoint and visits its
                // existing connections breadth-first before the new edge is
                // inserted. Retain that order for service callbacks.
                let order = if self.membership_recording {
                    let start = if a_wins { b } else { a };
                    let order = self.ordered_reachable(start, loser);
                    if order.len() != self.services[&loser].members.len() {
                        // A provider observer must fail closed, but a broken
                        // observation stream must not strand the graph. Use
                        // the established graph-only fallback for ownership.
                        self.membership_failure = Some("incomplete grid propagation order");
                        self.services[&loser].members.iter().copied().collect()
                    } else {
                        order
                    }
                } else {
                    self.services[&loser].members.iter().copied().collect()
                };
                let moved = self.services.remove(&loser).ok_or("missing grid service")?;
                let retained = self.services.get_mut(&winner).ok_or("missing grid service")?;
                retained.forced_mode = relax(retained.forced_mode, moved.forced_mode);
                for node in order {
                    self.services.get_mut(&winner).expect("retained service").members.insert(node);
                    self.nodes.insert(node, winner);
                    self.record_membership(node, Some(loser), Some(winner));
                }
            }
            _ => {}
        }
        let key = edge_key(a, b);
        self.edges.insert(key, Edge { a, b });
        self.incident.entry(a).or_default().push(key);
        self.incident.entry(b).or_default().push(key);
        Ok(())
    }
    /// PathingCalculation points connection side A toward its controller.
    pub fn orient_toward(&mut self, node: Node, other: Node) -> Result<(), &'static str> {
        let edge = self
            .edges
            .get_mut(&edge_key(node, other))
            .ok_or("missing connection")?;
        edge.a = node;
        edge.b = other;
        // GridNode.setControllerRoute moves the parent connection to the
        // front of that node's own connection list.
        let key = edge_key(node, other);
        let order = self
            .incident
            .get_mut(&other)
            .ok_or("missing incident list")?;
        let index = order
            .iter()
            .position(|v| *v == key)
            .ok_or("missing incident connection")?;
        order.remove(index);
        order.insert(0, key);
        Ok(())
    }
    /// Reproduce PathingCalculation's node/connection BFS orientation pass.
    /// `controllers` must preserve Grid's controller-node iteration order;
    /// `priority` returns 0 for dense, 1 for preferred cable, 2 otherwise.
    /// This only orients connections and parent-list order; channel allocation
    /// is a separate calculation.
    pub fn orient_controller_paths(
        &mut self,
        controllers: &[Node],
        mut priority: impl FnMut(Node) -> u8,
    ) -> Result<(), &'static str> {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        enum PathItem {
            Node(Node),
            Edge(EdgeKey),
        }
        let mut visited = BTreeSet::new();
        let roots: BTreeSet<_> = controllers.iter().copied().collect();
        let mut queues: [VecDeque<PathItem>; 3] = Default::default();
        for &node in controllers {
            if !self.nodes.contains_key(&node) {
                return Err("missing controller node");
            }
            visited.insert(PathItem::Node(node));
            for other in self.connections(node) {
                if roots.contains(&other) {
                    continue;
                }
                let key = edge_key(node, other);
                // Source's constructor enqueues each outgoing controller edge.
                if visited.insert(PathItem::Edge(key)) {
                    let edge = self.edges.get_mut(&key).ok_or("missing connection")?;
                    edge.a = node;
                    edge.b = other;
                    queues[0].push_back(PathItem::Edge(key));
                }
            }
        }
        for level in 0..3 {
            while let Some(item) = queues[level].pop_front() {
                match item {
                    PathItem::Node(node) => {
                        let options = self
                            .incident
                            .get(&node)
                            .cloned()
                            .ok_or("missing incident list")?;
                        for key in options {
                            if !visited.insert(PathItem::Edge(key)) {
                                continue;
                            }
                            let edge = self.edges.get_mut(&key).ok_or("missing connection")?;
                            let other = if edge.a == node { edge.b } else { edge.a };
                            edge.a = node;
                            edge.b = other;
                            queues[level].push_back(PathItem::Edge(key));
                        }
                    }
                    PathItem::Edge(key) => {
                        let edge = self.edges.get(&key).ok_or("missing connection")?.clone();
                        for node in [edge.a, edge.b] {
                            if !visited.insert(PathItem::Node(node)) {
                                continue;
                            }
                            let order = self
                                .incident
                                .get_mut(&node)
                                .ok_or("missing incident list")?;
                            let index = order
                                .iter()
                                .position(|entry| *entry == key)
                                .ok_or("missing incident connection")?;
                            order.remove(index);
                            order.insert(0, key);
                            let target = level.max(usize::from(priority(node).min(2)));
                            queues[target].push_back(PathItem::Node(node));
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn ordered_reachable(&self, start: Node, service: u64) -> Vec<Node> {
        let mut seen = BTreeSet::from([start]);
        let mut queue = VecDeque::from([start]);
        let mut ordered = Vec::new();
        while let Some(node) = queue.pop_front() {
            ordered.push(node);
            for edge in self.incident.get(&node).into_iter().flatten().filter_map(|key| self.edges.get(key)) {
                let next = if edge.a == node {
                    Some(edge.b)
                } else if edge.b == node {
                    Some(edge.a)
                } else {
                    None
                };
                if let Some(next) = next {
                    if self.nodes.get(&next) == Some(&service) && seen.insert(next) {
                        queue.push_back(next);
                    }
                }
            }
        }
        ordered
    }
    // Preserve the existing graph-only path exactly until a consumer opts in.
    fn reachable(&self, start: Node, service: u64) -> BTreeSet<Node> {
        let mut seen = BTreeSet::from([start]);
        let mut queue = VecDeque::from([start]);
        while let Some(node) = queue.pop_front() {
            for edge in self.edges.values() {
                let next = if edge.a == node { Some(edge.b) }
                    else if edge.b == node { Some(edge.a) } else { None };
                if let Some(next) = next {
                    if self.nodes.get(&next) == Some(&service) && seen.insert(next) {
                        queue.push_back(next);
                    }
                }
            }
        }
        seen
    }
    fn validate_node_grid(&mut self, node: Node) -> Result<(), &'static str> {
        let old = *self.nodes.get(&node).ok_or("missing node")?;
        let reached: Vec<_> = if self.membership_recording {
            let ordered = self.ordered_reachable(node, old);
            let fallback = self.reachable(node, old);
            if ordered.iter().copied().collect::<BTreeSet<_>>() != fallback {
                self.membership_failure = Some("incomplete grid propagation order");
                fallback.into_iter().collect()
            } else {
                ordered
            }
        } else {
            self.reachable(node, old).into_iter().collect()
        };
        let pivot = self.services[&old].pivot;
        if reached.contains(&pivot) {
            return Ok(());
        }
        let mode = self.services[&old].forced_mode;
        let old_grid = self.services.get_mut(&old).ok_or("missing grid service")?;
        for member in &reached {
            old_grid.members.remove(member);
        }
        self.new_service(node, reached, mode);
        Ok(())
    }
    /// GridConnection.destroy validates A then B against the retained pivot.
    pub fn disconnect(&mut self, a: Node, b: Node) -> Result<(), &'static str> {
        let edge = self
            .edges
            .remove(&edge_key(a, b))
            .ok_or("missing connection")?;
        for node in [a, b] {
            if let Some(order) = self.incident.get_mut(&node) {
                order.retain(|v| *v != edge_key(a, b));
            }
        }
        self.validate_node_grid(edge.a)?;
        self.validate_node_grid(edge.b)
    }
    /// GridNode.destroy removes all edges, first moving the pivot toward the
    /// first side-A neighbor, then validates neighbors in connection order.
    pub fn remove_node(&mut self, node: Node) -> Result<(), &'static str> {
        if self.pending.remove(&node).is_some() {
            return Ok(());
        }
        let service = *self.nodes.get(&node).ok_or("missing node")?;
        let incident: Vec<_> = self
            .incident
            .get(&node)
            .into_iter()
            .flatten()
            .filter_map(|key| self.edges.get(key).cloned())
            .collect();
        let candidate = incident
            .iter()
            .find(|e| e.a != node)
            .map(|e| e.a)
            .or_else(|| {
                (self.services[&service].pivot == node)
                    .then(|| {
                        incident
                            .first()
                            .map(|e| if e.a == node { e.b } else { e.a })
                    })
                    .flatten()
            });
        if let Some(pivot) = candidate {
            self.services
                .get_mut(&service)
                .ok_or("missing grid service")?
                .pivot = pivot;
        }
        for edge in &incident {
            let key = edge_key(edge.a, edge.b);
            self.edges.remove(&key);
            let neighbor = if edge.a == node { edge.b } else { edge.a };
            if let Some(order) = self.incident.get_mut(&neighbor) {
                order.retain(|v| *v != key);
            }
        }
        self.incident.remove(&node);
        for edge in &incident {
            let neighbor = if edge.a == node { edge.b } else { edge.a };
            self.validate_node_grid(neighbor)?;
        }
        self.nodes.remove(&node);
        self.record_membership(node, Some(service), None);
        let grid = self
            .services
            .get_mut(&service)
            .ok_or("missing grid service")?;
        grid.members.remove(&node);
        if grid.members.is_empty() {
            self.services.remove(&service);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn node(n: u64) -> Node {
        Node {
            host: n,
            part: None,
            outer: false,
        }
    }
    #[test]
    fn controller_bfs_orients_cycle_edges_and_keeps_parent_first() {
        let mut g = OrderedServices::default();
        for i in 1..=4 {
            g.add_node(node(i), None).unwrap();
        }
        for (a, b) in [(1, 2), (1, 3), (2, 4), (3, 4)] {
            g.connect(node(a), node(b)).unwrap();
        }
        g.orient_controller_paths(&[node(1)], |_| 2).unwrap();
        assert_eq!(g.edges[&edge_key(node(3), node(4))].a, node(3));
        assert_eq!(g.connections(node(4))[0], node(2));
        assert_eq!(g.connections(node(2))[0], node(1));
    }
    #[test]
    fn oriented_node_removal_keeps_controller_service_and_starts_new_service() {
        let mut g = OrderedServices::default();
        for i in 1..=4 {
            g.add_node(node(i), None).unwrap();
        }
        g.connect(node(1), node(2)).unwrap();
        g.connect(node(2), node(3)).unwrap();
        g.connect(node(3), node(4)).unwrap();
        {
            let service = g.service_mut(node(1)).unwrap();
            service.channels = 1;
            service.channel_power = 0.125;
            service.controller = ControllerState::Online;
            service.forced_mode = Some(ChannelMode::X3);
        }
        g.orient_toward(node(1), node(2)).unwrap();
        g.orient_toward(node(2), node(3)).unwrap();
        g.orient_toward(node(3), node(4)).unwrap();
        g.remove_node(node(3)).unwrap();
        let retained = g.service(node(1)).unwrap();
        assert_eq!(retained.channels, 1);
        assert_eq!(retained.controller, ControllerState::Online);
        assert_eq!(retained.channel_power, 0.125);
        let detached = g.service(node(4)).unwrap();
        assert_eq!(detached.channels, 0);
        assert_eq!(detached.controller, ControllerState::None);
        assert_eq!(detached.forced_mode, Some(ChannelMode::X3));
    }
    #[test]
    fn pathing_moves_controller_parent_to_front_before_node_destruction() {
        let mut g = OrderedServices::default();
        for i in [1, 2, 4] {
            g.add_node(node(i), None).unwrap();
        }
        // Side 4 was connected first, but pathing subsequently chooses 1 as
        // node 2's controller parent and moves that connection to the front.
        g.connect(node(4), node(2)).unwrap();
        g.connect(node(1), node(2)).unwrap();
        g.orient_toward(node(1), node(2)).unwrap();
        g.service_mut(node(1)).unwrap().channels = 9;
        g.remove_node(node(2)).unwrap();
        assert_eq!(g.service(node(1)).unwrap().channels, 9);
        assert_eq!(g.service(node(4)).unwrap().channels, 0);
    }
    #[test]
    fn fresh_node_joins_existing_grid_before_any_priority_tie_break() {
        let mut g = OrderedServices::default();
        g.add_node(node(1), None).unwrap();
        g.ensure_grid(node(1)).unwrap();
        g.service_mut(node(1)).unwrap().channels = 6;
        g.add_node(node(2), Some(ChannelMode::X2)).unwrap();
        g.connect(node(2), node(1)).unwrap();
        assert_eq!(g.service(node(2)).unwrap().channels, 6);
        assert_eq!(
            g.service(node(2)).unwrap().forced_mode,
            Some(ChannelMode::X2)
        );
    }
    #[test]
    fn equally_powered_equal_grids_preserve_side_a_service() {
        let mut g = OrderedServices::default();
        g.add_node(node(1), None).unwrap();
        g.ensure_grid(node(1)).unwrap();
        g.add_node(node(2), None).unwrap();
        g.ensure_grid(node(2)).unwrap();
        g.service_mut(node(1)).unwrap().channels = 3;
        g.service_mut(node(2)).unwrap().channels = 7;
        g.connect(node(2), node(1)).unwrap();
        assert_eq!(g.service(node(1)).unwrap().channels, 7);
    }
    #[test]
    fn powered_then_size_then_a_tie_break_selects_existing_service_and_relaxes_mode() {
        let mut g = OrderedServices::default();
        for i in 1..=4 {
            g.add_node(node(i), None).unwrap();
        }
        g.connect(node(1), node(2)).unwrap();
        g.connect(node(3), node(4)).unwrap();
        g.service_mut(node(1)).unwrap().channels = 7;
        g.service_mut(node(1)).unwrap().forced_mode = Some(ChannelMode::X2);
        let right = g.service_mut(node(3)).unwrap();
        right.powered = true;
        right.channels = 3;
        right.forced_mode = Some(ChannelMode::X3);
        g.connect(node(2), node(3)).unwrap();
        let service = g.service(node(1)).unwrap();
        assert_eq!(service.channels, 3);
        assert_eq!(service.forced_mode, Some(ChannelMode::X3));
    }
    #[test]
    fn membership_events_follow_actual_endpoint_bfs_for_merge_and_split() {
        let mut g = OrderedServices::default();
        g.start_membership_recording().unwrap();
        for n in [1, 2, 3, 4, 10, 50, 20] { g.add_node(node(n), None).unwrap(); }
        for (a, b) in [(1, 2), (2, 3), (3, 4), (10, 50), (10, 20)] {
            g.connect(node(a), node(b)).unwrap();
        }
        g.take_membership_changes().unwrap();
        let winner = g.service_id(node(1)).unwrap();
        let loser = g.service_id(node(10)).unwrap();
        g.connect(node(1), node(10)).unwrap();
        let merged = g.take_membership_changes().unwrap();
        assert_eq!(merged.iter().map(|event| event.node).collect::<Vec<_>>(),
            vec![node(10), node(50), node(20)]);
        assert!(merged.iter().all(|event| event.from == Some(loser) && event.to == Some(winner)));

        g.disconnect(node(1), node(10)).unwrap();
        let split = g.take_membership_changes().unwrap();
        let detached = g.service_id(node(10)).unwrap();
        assert_ne!(detached, winner);
        assert_eq!(split.iter().map(|event| event.node).collect::<Vec<_>>(),
            vec![node(10), node(50), node(20)]);
        assert!(split.iter().all(|event| event.from == Some(winner) && event.to == Some(detached)));
        g.remove_node(node(50)).unwrap();
        assert_eq!(g.take_membership_changes().unwrap(), vec![MembershipChange {
            node: node(50), from: Some(detached), to: None,
        }]);
    }
    #[test]
    fn membership_recording_cannot_reconstruct_a_late_snapshot() {
        let mut g = OrderedServices::default();
        g.add_node(node(3), None).unwrap();
        assert_eq!(g.start_membership_recording(), Err("membership recording began after node creation"));
        assert_eq!(g.take_membership_changes(), Err("membership recording is disabled"));
    }
    #[test]
    fn invalid_observer_order_does_not_block_graph_merge() {
        let mut observed = OrderedServices::default();
        observed.start_membership_recording().unwrap();
        for n in 1..=4 { observed.add_node(node(n), None).unwrap(); }
        observed.connect(node(1), node(2)).unwrap();
        observed.connect(node(3), node(4)).unwrap();
        observed.take_membership_changes().unwrap();
        // Deliberately corrupt the callback ordering information while leaving
        // the graph edges intact. The established graph-only merge can still
        // preserve ownership, while the provider event stream is unusable.
        observed.incident.get_mut(&node(3)).unwrap().clear();
        let mut fallback = observed.clone();
        fallback.membership_recording = false;
        observed.connect(node(1), node(3)).unwrap();
        fallback.connect(node(1), node(3)).unwrap();
        assert_eq!(observed.nodes, fallback.nodes);
        assert_eq!(observed.service(node(1)).unwrap().members, fallback.service(node(1)).unwrap().members);
        assert_eq!(observed.take_membership_changes(), Err("incomplete grid propagation order"));
        assert_eq!(observed.take_membership_changes(), Err("incomplete grid propagation order"));
    }
    #[test]
    fn invalid_observer_order_does_not_change_graph_split() {
        let mut observed = OrderedServices::default();
        observed.start_membership_recording().unwrap();
        for n in 1..=3 { observed.add_node(node(n), None).unwrap(); }
        observed.connect(node(1), node(2)).unwrap();
        observed.connect(node(2), node(3)).unwrap();
        observed.take_membership_changes().unwrap();
        observed.incident.get_mut(&node(2)).unwrap().clear();
        let mut fallback = observed.clone();
        fallback.membership_recording = false;
        observed.disconnect(node(1), node(2)).unwrap();
        fallback.disconnect(node(1), node(2)).unwrap();
        assert_eq!(observed.nodes, fallback.nodes);
        assert_eq!(observed.service(node(2)).unwrap().members, fallback.service(node(2)).unwrap().members);
        assert_eq!(observed.take_membership_changes(), Err("incomplete grid propagation order"));
    }
}
