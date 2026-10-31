//! Actual item capability forwarding for P2P tunnels (AE2 19.2.18,
//! ItemP2PTunnelPart and CapabilityP2PTunnelPart, LGPL-3.0-or-later).
use super::{Game, Port};
use crate::game::ae2_global::GridAddress;
use ae2_network_core::Pos;
use horizon_client::items::ItemStack;
use horizon_core::geometry::Direction;
use std::collections::BTreeSet;

pub(super) fn side(d: Direction) -> u8 {
    match d {
        Direction::East => 0,
        Direction::West => 1,
        Direction::Up => 2,
        Direction::Down => 3,
        Direction::South => 4,
        Direction::North => 5,
    }
}
pub(super) fn direction(i: u8) -> Direction {
    match i {
        0 => Direction::East,
        1 => Direction::West,
        2 => Direction::Up,
        3 => Direction::Down,
        4 => Direction::South,
        _ => Direction::North,
    }
}
pub(super) fn adjacent(port: &Port) -> Option<GridAddress> {
    GridAddress::from_network(Pos {
        x: port.pos.x.checked_add(port.direction[0])?,
        y: port.pos.y.checked_add(port.direction[1])?,
        z: port.pos.z.checked_add(port.direction[2])?,
    })
}
type Guard = BTreeSet<(Pos, u8, bool)>;
impl Game<'_> {
    fn ae2_item_p2p_supports_inner(&self,address:GridAddress,face:Direction,guard:&mut Guard)->Option<bool> {
        let port=self.ae2_p2p_port(address,face,"ae2:item_p2p_tunnel")?;
        if !port.output{return Some(true);}
        let marker=(port.pos,port.face,false);
        if !guard.insert(marker){return Some(false);}
        let supported=self.ae2_p2p_peers(&port,false).first().is_some_and(|peer|{
            let Some(target)=adjacent(peer) else{return false};
            let side=direction(peer.face^1);
            self.ae2_item_p2p_supports_inner(target,side,guard).unwrap_or_else(||self.ae2_native_item_supports_local(target,side))
        });
        guard.remove(&marker);Some(supported)
    }
    pub(in crate::game) fn ae2_item_p2p_supports(&self,address:GridAddress,face:Direction)->Option<bool> {
        self.ae2_item_p2p_supports_inner(address,face,&mut Guard::new())
    }
    pub(super) fn ae2_p2p_port(&self, address: GridAddress, face: Direction, kind: &str) -> Option<Port> {
        self.ae2_p2p_ports()
            .into_iter()
            .find(|p| p.pos == address.network_pos() && p.face == side(face) && p.id == kind)
    }
    pub(super) fn ae2_p2p_peers(&self, port: &Port, output: bool) -> Vec<Port> {
        if port.frequency == 0 {
            return Vec::new();
        }
        let net = self.ae2_network();
        let Ok(a) = net.analyze() else {
            return Vec::new();
        };
        let Some(node) = a.nodes.get(&port.pos) else {
            return Vec::new();
        };
        if !node.parts.get(&port.face).is_some_and(|p| p.online) {
            return Vec::new();
        }
        self.ae2_p2p_ports()
            .into_iter()
            .filter(|p| {
                p.frequency == port.frequency
                    && p.id == port.id
                    && p.output == output
                    && a.nodes.get(&p.pos).is_some_and(|n| {
                        n.component == node.component
                            && n.parts.get(&p.face).is_some_and(|p| p.online)
                    })
            })
            .collect()
    }
    pub(super) fn ae2_p2p_tax(&mut self, port: &Port, operations: f64) {
        let config = horizon_client::ae2_common_config::gameplay();
        self.ae2_p2p_debit(port, operations * config.p2p_transport_tax * config.usage_multiplier);
    }
    pub(super) fn ae2_p2p_energy_tax(&mut self, port: &Port, transported_fe: f64) {
        let config = horizon_client::ae2_common_config::gameplay();
        self.ae2_p2p_debit(port, transported_fe * config.forge_energy * config.p2p_energy_tax * config.usage_multiplier);
    }
    fn ae2_p2p_debit(&mut self, port: &Port, amount: f64) {
        let Some(address) = GridAddress::from_network(port.pos) else {
            return;
        };
        // P2P tax is paid by its carrier grid, even when the initiating
        // storage operation is powered by a wireless terminal item.
        let wireless = self.ae2_wireless_power.take();
        let _ = self.ae2_take_power_encoded_raw(address, amount, false);
        self.ae2_wireless_power = wireless;
    }
    fn ae2_item_p2p_insert_inner(
        &mut self,
        address: GridAddress,
        face: Direction,
        stack: &ItemStack,
        simulate: bool,
        guard: &mut Guard,
        mut plan: Option<&mut crate::game::ae2_buses::FormationPlan>,
    ) -> Option<u32> {
        let port = self.ae2_p2p_port(address, face, "ae2:item_p2p_tunnel")?;
        if port.output || stack.count == 0 {
            return Some(0);
        }
        let key = (port.pos, port.face, true);
        if !guard.insert(key) {
            return Some(0);
        }
        let outputs = self.ae2_p2p_peers(&port, true);
        if outputs.is_empty() {
            guard.remove(&key);
            return Some(0);
        }
        // Preserve upstream's exact sharing/overflow rule. One target refusing
        // its share adds that overflow to the next actual target handler.
        let share = stack.count / (outputs.len() as u32);
        let mut overflow = if share == 0 {
            stack.count
        } else {
            stack.count % share
        };
        let mut left = stack.count;
        for output in outputs {
            let offered = share.saturating_add(overflow).min(left);
            if offered == 0 {
                break;
            }
            let Some(target) = adjacent(&output) else {
                continue;
            };
            let copy = ItemStack {
                count: offered,
                ..stack.clone()
            };
            let face = direction(output.face ^ 1);
            let accepted = match self.ae2_item_p2p_insert_inner(target, face, &copy, simulate, guard, plan.as_deref_mut()) {
                Some(n) => n,
                None => match plan.as_deref_mut() {
                    Some(plan) => self.ae2_native_item_insert_staged(target, face, &copy, offered, simulate, plan),
                    None => self.ae2_native_item_insert(target, face, &copy, offered, simulate),
                },
            }.min(offered);
            overflow = offered - accepted;
            left -= accepted;
        }
        guard.remove(&key);
        let accepted = stack.count - left;
        if !simulate && accepted > 0 {
            self.ae2_p2p_tax(&port, f64::from(accepted));
        }
        Some(accepted)
    }
    fn ae2_item_p2p_extract_inner(
        &mut self,
        address: GridAddress,
        face: Direction,
        limit: u32,
        filter: Option<&ItemStack>,
        simulate: bool,
        guard: &mut Guard,
        mut plan: Option<&mut crate::game::ae2_buses::FormationPlan>,
    ) -> Option<Option<ItemStack>> {
        let port = self.ae2_p2p_port(address, face, "ae2:item_p2p_tunnel")?;
        if !port.output || limit == 0 {
            return Some(None);
        }
        let key = (port.pos, port.face, false);
        if !guard.insert(key) {
            return Some(None);
        }
        let result = if let Some(input) = self.ae2_p2p_peers(&port, false).first() {
            if let Some(target) = adjacent(input) {
                let face = direction(input.face ^ 1);
                match self.ae2_item_p2p_extract_inner(target, face, limit, filter, simulate, guard, plan.as_deref_mut()) {
                    Some(result) => result,
                    None => match plan.as_deref_mut() {
                        Some(plan) => self.ae2_native_item_extract_staged(target, face, limit, filter, simulate, plan),
                        None => self.ae2_native_item_extract(target, face, limit, filter, simulate),
                    },
                }
            } else {
                None
            }
        } else {
            None
        };
        guard.remove(&key);
        if !simulate {
            if let Some(stack) = &result {
                self.ae2_p2p_tax(&port, f64::from(stack.count));
            }
        }
        Some(result)
    }
    fn ae2_item_p2p_candidates_inner(
        &self,
        address: GridAddress,
        face: Direction,
        guard: &mut Guard,
    ) -> Option<Vec<ItemStack>> {
        let port = self.ae2_p2p_port(address, face, "ae2:item_p2p_tunnel")?;
        if !port.output {
            return Some(Vec::new());
        }
        let key = (port.pos, port.face, false);
        if !guard.insert(key) {
            return Some(Vec::new());
        }
        let out = if let Some(input) = self.ae2_p2p_peers(&port, false).first() {
            if let Some(target) = adjacent(input) {
                let face = direction(input.face ^ 1);
                self.ae2_item_p2p_candidates_inner(target, face, guard)
                    .unwrap_or_else(|| self.ae2_native_item_candidates(target, face))
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        guard.remove(&key);
        Some(out)
    }
    pub(in crate::game) fn ae2_item_p2p_candidates(
        &self,
        address: GridAddress,
        face: Direction,
    ) -> Option<Vec<ItemStack>> {
        self.ae2_item_p2p_candidates_inner(address, face, &mut Guard::new())
    }
    pub(in crate::game) fn ae2_item_p2p_insert(
        &mut self,
        address: GridAddress,
        face: Direction,
        stack: &ItemStack,
        simulate: bool,
    ) -> Option<u32> {
        self.ae2_item_p2p_insert_inner(address, face, stack, simulate, &mut Guard::new(), None)
    }
    pub(in crate::game) fn ae2_item_p2p_insert_staged(
        &mut self, address: GridAddress, face: Direction, stack: &ItemStack, simulate: bool,
        plan: &mut crate::game::ae2_buses::FormationPlan,
    ) -> Option<u32> {
        self.ae2_item_p2p_insert_inner(address, face, stack, simulate, &mut Guard::new(), Some(plan))
    }
    pub(in crate::game) fn ae2_item_p2p_extract(
        &mut self,
        address: GridAddress,
        face: Direction,
        limit: u32,
        filter: Option<&ItemStack>,
        simulate: bool,
    ) -> Option<Option<ItemStack>> {
        self.ae2_item_p2p_extract_inner(address, face, limit, filter, simulate, &mut Guard::new(), None)
    }
    pub(in crate::game) fn ae2_item_p2p_extract_staged(
        &mut self, address: GridAddress, face: Direction, limit: u32,
        filter: Option<&ItemStack>, simulate: bool, plan: &mut crate::game::ae2_buses::FormationPlan,
    ) -> Option<Option<ItemStack>> {
        self.ae2_item_p2p_extract_inner(address, face, limit, filter, simulate, &mut Guard::new(), Some(plan))
    }
}
