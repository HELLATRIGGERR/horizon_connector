// Derived from AE2 PathingCalculation, db17504a86128fdf3dae31f5fb7a112a646e0b93.
// Copyright AlgorithmX2 and AE2 contributors. LGPL-3.0-or-later; see LICENSE.
//! Controller allocation visits connections and nodes as separate path items.
use super::{ChannelMode, DeviceKind, Network, Pos};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Key { Host(Pos), Part(Pos, u8) }
#[derive(Clone)]
struct Node {
    key: Key,
    priority: usize,
    capacity: usize,
    requires_channel: bool,
    compressed: bool,
    allows_compressed: bool,
    group: Key,
    connections: Vec<usize>,
}
#[derive(Clone, Copy)]
struct Edge { near: usize, far: usize }
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Item { Node(usize), Edge(usize) }

pub(super) struct Assignment {
    pub used: BTreeMap<Pos, usize>,
    pub hosts: BTreeSet<Pos>,
    pub parts: BTreeSet<(Pos, u8)>,
    pub groups: BTreeSet<Pos>,
    pub channel_blocks: usize,
}

pub(super) fn controller(network: &Network, positions: &[Pos], mode: ChannelMode) -> Assignment {
    let mut nodes = Vec::new();
    let mut indices = BTreeMap::new();
    for &pos in positions {
        let device = &network.nodes[&pos];
        indices.insert(Key::Host(pos), nodes.len());
        nodes.push(Node {
            key: Key::Host(pos), priority: if device.kind.carries() { if device.capacity() == 32 { 0 } else { 1 } } else { 2 },
            capacity: mode.capacity(device.capacity()), requires_channel: device.requires_channel(),
            compressed: false, allows_compressed: !device.cannot_carry_compressed,
            group: Key::Host(device.multiblock.unwrap_or(pos)), connections: Vec::new(),
        });
        for part in &device.attached {
            let key = Key::Part(pos, part.face);
            indices.insert(key, nodes.len());
            nodes.push(Node {
                key, priority: 2, capacity: mode.capacity(8), requires_channel: part.requires_channel,
                compressed: part.compressed, allows_compressed: true, group: key, connections: Vec::new(),
            });
        }
    }
    let mut members = BTreeMap::<Key, Vec<usize>>::new();
    for (index, node) in nodes.iter().enumerate().filter(|(_, node)| node.requires_channel) {
        members.entry(node.group).or_default().push(index);
    }
    let mut edges = Vec::new();
    let mut edge_indices = BTreeMap::new();
    for &pos in positions {
        let index = indices[&Key::Host(pos)];
        let mut peers: Vec<_> = network.neighbours(pos).into_iter().map(Key::Host).collect();
        peers.extend(network.nodes[&pos].attached.iter().map(|part| Key::Part(pos, part.face)));
        for peer in peers {
            let Some(&other) = indices.get(&peer) else { continue };
            let key = if index < other { (index, other) } else { (other, index) };
            let edge = *edge_indices.entry(key).or_insert_with(|| {
                let edge = edges.len(); edges.push(Edge { near: index, far: other }); edge
            });
            nodes[index].connections.push(edge);
            if matches!(peer, Key::Part(_, _)) { nodes[other].connections.push(edge); }
        }
    }
    // Source node connections have a stable incident order, including the
    // previous controller route moved to the front. Use the observed order
    // only after the host has validated the complete logical edge set.
    let identity = |key:Key| match key {
        Key::Host(pos) => network.node_identities.get(&pos),
        Key::Part(pos,face) => network.part_identities.get(&(pos,face)),
    }.copied();
    let node_ids:Vec<_>=nodes.iter().map(|node|identity(node.key)).collect();
    for (index,node) in nodes.iter_mut().enumerate() {
        if let Some(order)=node_ids[index].and_then(|id|network.observed_connection_order.get(&id)) {
            node.connections.sort_by_key(|edge| {
                let edge=edges[*edge]; let other=if edge.near==index{edge.far}else{edge.near};
                node_ids[other].and_then(|id|order.iter().position(|peer|*peer==id)).unwrap_or(usize::MAX)
            });
        }
    }
    let roots: Vec<_> = positions.iter().filter(|pos| network.nodes[pos].kind == DeviceKind::Controller)
        .map(|pos| indices[&Key::Host(*pos)]).collect();
    let root_set: BTreeSet<_> = roots.iter().copied().collect();
    let mut visited = BTreeSet::new();
    let mut queues: [VecDeque<Item>; 3] = Default::default();
    let mut parents = vec![None; nodes.len()];
    for &root in &roots {
        visited.insert(Item::Node(root));
        for &edge in &nodes[root].connections {
            let other = if edges[edge].near == root { edges[edge].far } else { edges[edge].near };
            if !root_set.contains(&other) && visited.insert(Item::Edge(edge)) {
                edges[edge] = Edge { near: root, far: other }; queues[0].push_back(Item::Edge(edge));
            }
        }
    }
    let mut bottlenecks = vec![0usize; nodes.len()];
    let mut highest = vec![None; nodes.len()];
    let mut subtree_max = vec![0usize; nodes.len()];
    let mut subtree_compressed = vec![true; nodes.len()];
    let mut visit_order = Vec::new();
    let mut consuming = BTreeSet::new();
    let mut assigned_groups = BTreeSet::new();
    let mut extra_members = BTreeSet::new();
    for level in 0..3 {
        while let Some(item) = queues[level].pop_front() {
            match item {
                Item::Node(node) => {
                    for &edge in &nodes[node].connections {
                        if visited.insert(Item::Edge(edge)) {
                            let other = if edges[edge].near == node { edges[edge].far } else { edges[edge].near };
                            edges[edge] = Edge { near: node, far: other };
                            queues[level].push_back(Item::Edge(edge));
                        }
                    }
                }
                Item::Edge(edge) => {
                    for node in [edges[edge].near, edges[edge].far] {
                        if !visited.insert(Item::Node(node)) { continue; }
                        let parent = edges[edge].near;
                        parents[node] = Some(parent);
                        visit_order.push(node);
                        if root_set.contains(&parent) {
                            subtree_max[node] = nodes[node].capacity;
                            subtree_compressed[node] = nodes[node].allows_compressed;
                        } else {
                            highest[node] = match highest[parent] {
                                None => Some(parent),
                                Some(ancestor) if subtree_max[parent] == subtree_max[ancestor] => Some(ancestor),
                                Some(_) => Some(parent),
                            };
                            subtree_max[node] = subtree_max[parent].min(nodes[node].capacity);
                            subtree_compressed[node] = subtree_compressed[parent] && nodes[node].allows_compressed;
                        }
                        // GridNode.setControllerRoute moves its parent edge first.
                        let at = nodes[node].connections.iter().position(|entry| *entry == edge).expect("incident parent edge");
                        nodes[node].connections.remove(at); nodes[node].connections.insert(0, edge);
                        if nodes[node].requires_channel {
                            if assigned_groups.contains(&nodes[node].group) { extra_members.insert(node); }
                            else {
                                let mut available = !nodes[node].compressed || subtree_compressed[node];
                                let mut next = Some(node);
                                while available {
                                    let Some(at) = next else { break };
                                    if bottlenecks[at] >= nodes[at].capacity { available = false; break; }
                                    next = highest[at];
                                }
                                if available {
                                    consuming.insert(node); assigned_groups.insert(nodes[node].group);
                                    for &other in &members[&nodes[node].group] {
                                        if other != node { extra_members.insert(other); }
                                    }
                                    let mut next = Some(node);
                                    while let Some(at) = next { bottlenecks[at] += 1; next = highest[at]; }
                                }
                            }
                        }
                        queues[level.max(nodes[node].priority)].push_back(Item::Node(node));
                    }
                }
            }
        }
    }
    // Each non-controller node and its parent connection carries the same
    // propagated count. Multiblock-local flags are applied after this sum.
    let mut routed = vec![0usize; nodes.len()];
    for &node in &consuming { routed[node] = 1; }
    for &node in visit_order.iter().rev() {
        if let Some(parent) = parents[node] {
            if !root_set.contains(&parent) { routed[parent] += routed[node]; }
        }
    }
    let channel_blocks = routed.iter().sum::<usize>() * 2;
    for node in extra_members.iter().copied() { routed[node] += 1; }
    let mut assignment = Assignment { used: BTreeMap::new(), hosts: BTreeSet::new(), parts: BTreeSet::new(), groups: BTreeSet::new(), channel_blocks };
    for (index, node) in nodes.iter().enumerate() {
        match node.key {
            Key::Host(pos) => {
                assignment.used.insert(pos, routed[index]);
                if consuming.contains(&index) || extra_members.contains(&index) { assignment.hosts.insert(pos); }
                if consuming.contains(&index) { if let Key::Host(group) = node.group { assignment.groups.insert(group); } }
            }
            Key::Part(pos, face) => { if consuming.contains(&index) { assignment.parts.insert((pos, face)); } }
        }
    }
    assignment
}
