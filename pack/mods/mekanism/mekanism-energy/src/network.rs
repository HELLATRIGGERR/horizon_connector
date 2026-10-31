//! Universal cable networks (`EnergyNetwork`, a `DynamicBufferedNetwork`):
//! touching cables form one network whose buffer holds as much as all its
//! cables together (`CableTier` capacity each). Energy put into any cable
//! goes into the buffer; each tick the buffer is split among the blocks the
//! cables touch (`tickEmit`). Cables keep their part of the buffer, their
//! share, for saving (`updateSaveShares`); a network made of cables that were
//! saved, merged or split takes their shares back (`absorbBuffer`).
use crate::split::send_to_acceptors;
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub type Pos = [i32; 3];

/// `Direction` order: down, up, north, south, west, east.
pub const SIDES: [Pos; 6] = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];

pub fn offset(pos: Pos, side: usize) -> Pos {
    let d = SIDES[side];
    [pos[0] + d[0], pos[1] + d[1], pos[2] + d[2]]
}

/// The side facing back: down and up, north and south, west and east.
pub fn opposite(side: usize) -> usize {
    side ^ 1
}

/// `ConnectionType` of one side of a cable, set with the Configurator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Connection {
    /// Takes energy in and gives it out.
    #[default]
    Normal,
    /// Only gives energy out.
    Push,
    /// Takes energy out of the block on that side (`pullFromAcceptors`).
    Pull,
    /// Not connected.
    None,
}

#[derive(Clone, Debug)]
pub struct Cable {
    pub capacity: u64,
    pub connections: [Connection; 6],
    /// The cable's part of its network's buffer as last written for saving
    /// (`lastWrite`), or the energy it brought in when it was loaded.
    pub share: u64,
    network: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Network {
    pub cables: BTreeSet<Pos>,
    pub buffer: u64,
    pub capacity: u64,
}

impl Network {
    pub fn needed(&self) -> u64 {
        self.capacity.saturating_sub(self.buffer)
    }
}

/// Every loaded cable of a level and the networks they make.
#[derive(Debug, Default)]
pub struct Networks {
    cables: HashMap<Pos, Cable>,
    networks: BTreeMap<u32, Network>,
    next: u32,
}

impl Networks {
    pub fn cable(&self, pos: Pos) -> Option<&Cable> {
        self.cables.get(&pos)
    }

    pub fn network_of(&self, pos: Pos) -> Option<(u32, &Network)> {
        let id = self.cables.get(&pos)?.network;
        Some((id, self.networks.get(&id)?))
    }

    pub fn networks(&self) -> impl Iterator<Item = (u32, &Network)> {
        self.networks.iter().map(|(id, n)| (*id, n))
    }

    /// Whether two neighbouring cables join: neither side is cut.
    fn joined(&self, a: Pos, side: usize) -> bool {
        let b = offset(a, side);
        match (self.cables.get(&a), self.cables.get(&b)) {
            (Some(x), Some(y)) => {
                x.connections[side] != Connection::None && y.connections[opposite(side)] != Connection::None
            }
            _ => false,
        }
    }

    /// A cable loaded or placed, with the energy its saved share holds: it
    /// joins the networks it touches, which become one.
    pub fn add(&mut self, pos: Pos, capacity: u64, connections: [Connection; 6], share: u64) {
        self.remove(pos);
        self.cables.insert(pos, Cable { capacity, connections, share, network: u32::MAX });
        let mut members = vec![pos];
        let mut buffer = share;
        let touching: BTreeSet<u32> = (0..6)
            .filter(|&s| self.joined(pos, s))
            .map(|s| self.cables[&offset(pos, s)].network)
            .collect();
        for id in touching {
            if let Some(old) = self.networks.remove(&id) {
                buffer = buffer.saturating_add(old.buffer);
                members.extend(old.cables);
            }
        }
        self.form(members, buffer);
        if let Some(c) = self.cables.get_mut(&pos) {
            c.share = 0;
        }
    }

    /// A cable broken or unloaded: what it held of the buffer goes with it,
    /// and the rest of its network splits into the parts still touching.
    pub fn remove(&mut self, pos: Pos) -> Option<Cable> {
        let cable = self.cables.get(&pos)?.clone();
        let Some(network) = self.networks.remove(&cable.network) else {
            return self.cables.remove(&pos);
        };
        // Save shares first, so each part takes back what its cables hold.
        let shares = self.shares(&network);
        self.cables.remove(&pos);
        let mut left: BTreeSet<Pos> = network.cables;
        left.remove(&pos);
        while let Some(&start) = left.iter().next() {
            let mut part = Vec::new();
            let mut stack = vec![start];
            left.remove(&start);
            while let Some(p) = stack.pop() {
                part.push(p);
                for s in 0..6 {
                    let q = offset(p, s);
                    if left.contains(&q) && self.joined(p, s) {
                        left.remove(&q);
                        stack.push(q);
                    }
                }
            }
            let buffer = part.iter().map(|p| shares.get(p).copied().unwrap_or(0)).sum();
            self.form(part, buffer);
        }
        Some(cable)
    }

    /// A side's connection changed (the Configurator): the networks are made again.
    pub fn set_connection(&mut self, pos: Pos, side: usize, connection: Connection) {
        let Some(cable) = self.cables.get(&pos) else { return };
        let (capacity, mut connections) = (cable.capacity, cable.connections);
        connections[side] = connection;
        let held = self.network_of(pos).map(|(_, n)| self.shares(n)).unwrap_or_default();
        self.remove(pos);
        self.add(pos, capacity, connections, held.get(&pos).copied().unwrap_or(0));
    }

    fn form(&mut self, cables: Vec<Pos>, buffer: u64) {
        let id = self.next;
        self.next = self.next.wrapping_add(1);
        let capacity = cables
            .iter()
            .map(|p| self.cables[p].capacity)
            .fold(0u64, u64::saturating_add);
        for p in &cables {
            if let Some(c) = self.cables.get_mut(p) {
                c.network = id;
            }
        }
        // clampBuffer: a buffer bigger than the network's capacity is cut.
        let network = Network { cables: cables.into_iter().collect(), buffer: buffer.min(capacity), capacity };
        self.networks.insert(id, network);
    }

    /// `EnergyTransmitterSaveTarget`: the buffer split evenly among the
    /// network's cables, none over its capacity.
    fn shares(&self, network: &Network) -> HashMap<Pos, u64> {
        let cables: Vec<Pos> = network.cables.iter().copied().collect();
        let mut held = vec![0u64; cables.len()];
        send_to_acceptors(cables.len(), network.buffer, |i, offered, simulate| {
            let room = self.cables[&cables[i]].capacity - held[i];
            let taken = offered.min(room);
            if !simulate {
                held[i] += taken;
            }
            offered - taken
        });
        cables.into_iter().zip(held).collect()
    }

    /// `updateSaveShares`: each cable's share of its network's buffer, the
    /// energy its block entity saves.
    pub fn save_shares(&mut self) {
        let shares: Vec<(Pos, u64)> = self.networks.values().flat_map(|n| self.shares(n)).collect();
        for (pos, share) in shares {
            if let Some(c) = self.cables.get_mut(&pos) {
                c.share = share;
            }
        }
    }

    /// Energy put into the cable at `pos` (its handler is its network's
    /// buffer): returns what did not fit.
    pub fn insert(&mut self, pos: Pos, amount: u64, simulate: bool) -> u64 {
        let Some(id) = self.cables.get(&pos).map(|c| c.network) else {
            return amount;
        };
        let Some(network) = self.networks.get_mut(&id) else {
            return amount;
        };
        let taken = amount.min(network.needed());
        if !simulate {
            network.buffer += taken;
        }
        amount - taken
    }

    /// Energy taken out of the cable at `pos`'s network: returns what was taken.
    pub fn extract(&mut self, pos: Pos, amount: u64, simulate: bool) -> u64 {
        let Some(network) = self.cables.get(&pos).and_then(|c| self.networks.get_mut(&c.network)) else {
            return 0;
        };
        let taken = amount.min(network.buffer);
        if !simulate {
            network.buffer -= taken;
        }
        taken
    }

    /// The blocks network `id` hands energy to: each (cable, side) whose side
    /// gives out (Normal or Push) and has no joined cable beyond it.
    pub fn outputs(&self, id: u32) -> Vec<(Pos, usize)> {
        let Some(network) = self.networks.get(&id) else { return Vec::new() };
        let mut out = Vec::new();
        for &pos in &network.cables {
            let cable = &self.cables[&pos];
            for side in 0..6 {
                let gives = matches!(cable.connections[side], Connection::Normal | Connection::Push);
                if gives && !self.cables.contains_key(&offset(pos, side)) {
                    out.push((pos, side));
                }
            }
        }
        out
    }

    /// The pull sides of network `id`'s cables (`pullFromAcceptors`).
    pub fn pulls(&self, id: u32) -> Vec<(Pos, usize)> {
        let Some(network) = self.networks.get(&id) else { return Vec::new() };
        let mut out = Vec::new();
        for &pos in &network.cables {
            for side in 0..6 {
                if self.cables[&pos].connections[side] == Connection::Pull
                    && !self.cables.contains_key(&offset(pos, side))
                {
                    out.push((pos, side));
                }
            }
        }
        out
    }

    /// `EnergyNetwork.onUpdate`'s emission: network `id`'s buffer split among
    /// `targets` (see `outputs`; those that take nothing are left out as in
    /// `tickEmit`). `insert(target, amount, simulate)` returns what the block
    /// did not take. Returns what was sent, which leaves the buffer.
    pub fn emit(
        &mut self,
        id: u32,
        targets: &[(Pos, usize)],
        mut insert: impl FnMut((Pos, usize), u64, bool) -> u64,
    ) -> u64 {
        let Some(buffer) = self.networks.get(&id).map(|n| n.buffer) else {
            return 0;
        };
        if buffer == 0 {
            return 0;
        }
        let takers: Vec<(Pos, usize)> = targets
            .iter()
            .copied()
            .filter(|t| insert(*t, buffer, true) < buffer)
            .collect();
        let sent = send_to_acceptors(takers.len(), buffer, |i, amount, simulate| insert(takers[i], amount, simulate));
        if let Some(n) = self.networks.get_mut(&id) {
            n.buffer -= sent.min(n.buffer);
        }
        sent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: [Connection; 6] = [Connection::Normal; 6];

    #[test]
    fn touching_cables_share_one_buffer_and_split_when_cut() {
        let mut nets = Networks::default();
        nets.add([0, 0, 0], 8_000, N, 0);
        nets.add([2, 0, 0], 8_000, N, 5_000);
        assert_eq!(nets.networks().count(), 2);
        // The middle cable joins them: one network of three cables.
        nets.add([1, 0, 0], 128_000, N, 1_000);
        assert_eq!(nets.networks().count(), 1);
        let (id, net) = nets.network_of([0, 0, 0]).unwrap();
        assert_eq!((net.capacity, net.buffer), (144_000, 6_000));
        assert_eq!(nets.insert([2, 0, 0], 200_000, false), 62_000);
        assert_eq!(nets.network_of([0, 0, 0]).unwrap().1.buffer, 144_000);
        // Breaking the middle loses its share (128_000 of the even split);
        // the ends keep 8_000 each.
        nets.remove([1, 0, 0]);
        assert_eq!(nets.networks().count(), 2);
        assert_eq!(nets.network_of([0, 0, 0]).unwrap().1.buffer, 8_000);
        assert_eq!(nets.network_of([2, 0, 0]).unwrap().1.buffer, 8_000);
        assert!(nets.network_of([0, 0, 0]).unwrap().0 != id);
    }

    #[test]
    fn a_cut_side_keeps_networks_apart_and_outputs_skip_cables() {
        let mut nets = Networks::default();
        let mut cut = N;
        cut[5] = Connection::None;
        nets.add([0, 0, 0], 8_000, cut, 0);
        nets.add([1, 0, 0], 8_000, N, 0);
        assert_eq!(nets.networks().count(), 2);
        nets.set_connection([0, 0, 0], 5, Connection::Normal);
        assert_eq!(nets.networks().count(), 1);
        let (id, _) = nets.network_of([0, 0, 0]).unwrap();
        // Ten free sides: six around two cables less the two facing each other.
        assert_eq!(nets.outputs(id).len(), 10);
    }

    #[test]
    fn emission_splits_the_buffer_among_takers() {
        let mut nets = Networks::default();
        nets.add([0, 0, 0], 8_000, N, 3_000);
        let (id, _) = nets.network_of([0, 0, 0]).unwrap();
        let outputs = nets.outputs(id);
        let mut room = HashMap::from([(([0, 0, 0], 4), 1_000u64), (([0, 0, 0], 5), 10_000)]);
        let sent = nets.emit(id, &outputs, |t, amount, simulate| {
            let r = room.get(&t).copied().unwrap_or(0);
            let taken = amount.min(r);
            if !simulate && taken > 0 {
                room.insert(t, r - taken);
            }
            amount - taken
        });
        assert_eq!(sent, 3_000);
        assert_eq!(room[&([0, 0, 0], 4)], 0);
        assert_eq!(room[&([0, 0, 0], 5)], 8_000);
        assert_eq!(nets.network_of([0, 0, 0]).unwrap().1.buffer, 0);
    }

    #[test]
    fn saved_shares_fill_cables_evenly() {
        let mut nets = Networks::default();
        nets.add([0, 0, 0], 8_000, N, 0);
        nets.add([1, 0, 0], 8_000, N, 0);
        nets.insert([0, 0, 0], 9_001, false);
        nets.save_shares();
        let shares = (nets.cable([0, 0, 0]).unwrap().share, nets.cable([1, 0, 0]).unwrap().share);
        assert_eq!(shares, (4_501, 4_500));
    }
}
