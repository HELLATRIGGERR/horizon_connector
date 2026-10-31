//! FluidP2PTunnelPart forwarding with actual fluid keys and native physical
//! tanks. AE2 19.2.18 / LGPL-3.0-or-later. Simulation never modifies a tank.
use super::{
    Game,
    capabilities::{adjacent, direction},
};
use crate::game::ae2_global::GridAddress;
use ae2_cell_core::ItemKey;
use horizon_core::geometry::Direction;
use std::collections::BTreeSet;
type Guard = BTreeSet<(ae2_network_core::Pos, u8, bool)>;
impl Game<'_> {
    fn ae2_fluid_p2p_supports_inner(&self,address:GridAddress,face:Direction,guard:&mut Guard)->Option<bool> {
        let port=self.ae2_p2p_port(address,face,"ae2:fluid_p2p_tunnel")?;
        if !port.output{return Some(true);}
        let marker=(port.pos,port.face,false);
        if !guard.insert(marker){return Some(false);}
        let supported=self.ae2_p2p_peers(&port,false).first().is_some_and(|peer|{
            let Some(target)=adjacent(peer) else{return false};
            let side=direction(peer.face^1);
            self.ae2_fluid_p2p_supports_inner(target,side,guard).unwrap_or_else(||self.ae2_native_fluid_supports_local(target,side))
        });
        guard.remove(&marker);Some(supported)
    }
    pub(in crate::game) fn ae2_fluid_p2p_supports(&self,address:GridAddress,face:Direction)->Option<bool> {
        self.ae2_fluid_p2p_supports_inner(address,face,&mut Guard::new())
    }
    fn ae2_fluid_p2p_fill_inner(
        &mut self,
        address: GridAddress,
        face: Direction,
        key: &ItemKey,
        amount: u64,
        simulate: bool,
        guard: &mut Guard,
        plan: &mut crate::game::ae2_buses::FormationPlan,
    ) -> Option<u64> {
        let port = self.ae2_p2p_port(address, face, "ae2:fluid_p2p_tunnel")?;
        if port.output || amount == 0 {
            return Some(0);
        }
        let marker = (port.pos, port.face, true);
        if !guard.insert(marker) {
            return Some(0);
        }
        let outputs = self.ae2_p2p_peers(&port, true);
        if outputs.is_empty() {
            guard.remove(&marker);
            return Some(0);
        }
        let share = amount / (outputs.len() as u64);
        let mut overflow = if share == 0 { amount } else { amount % share };
        let mut left = amount;
        for output in outputs {
            let offered = share.saturating_add(overflow).min(left);
            if offered == 0 {
                break;
            }
            let Some(target) = adjacent(&output) else {
                continue;
            };
            let face = direction(output.face ^ 1);
            let accepted = self
                .ae2_fluid_p2p_fill_inner(target, face, key, offered, simulate, guard, plan)
                .unwrap_or_else(|| {
                    if matches!(self.ae2_block_entity(target),Some(horizon_client::containers::BlockEntity::Ae2(machine)) if machine.kind==horizon_client::ae2_world::MachineKind::Condenser) {return self.ae2_condenser_fill_at(target,offered,simulate);}
                    self.ae2_interface_fluid_move_staged(target, face, key, offered, true, simulate, plan)
                        .or_else(|| {
                            self.ae2_provider_return_fluid_at(target, face, key, offered, simulate)
                        })
                        .unwrap_or_else(|| {
                            self.ae2_tank_fill_encoded(target, key, offered, simulate)
                                .unwrap_or(0)
                        })
                })
                .min(offered);
            overflow = offered - accepted;
            left -= accepted;
        }
        guard.remove(&marker);
        let accepted = amount - left;
        if !simulate && accepted > 0 {
            self.ae2_p2p_tax(&port, accepted as f64 / 125.0);
        }
        Some(accepted)
    }
    // Output tunnel exposes every tank of its input-side capability. Input
    // tunnel is a write-only synthetic tank and therefore enumerates empty.
    fn ae2_fluid_p2p_entries_inner(
        &self,
        address: GridAddress,
        face: Direction,
        guard: &mut Guard,
    ) -> Option<Vec<(ItemKey, u64)>> {
        let port = self.ae2_p2p_port(address, face, "ae2:fluid_p2p_tunnel")?;
        if !port.output {
            return Some(Vec::new());
        }
        let marker = (port.pos, port.face, false);
        if !guard.insert(marker) {
            return Some(Vec::new());
        }
        let result = if let Some(input) = self.ae2_p2p_peers(&port, false).first() {
            if let Some(target) = adjacent(input) {
                let face = direction(input.face ^ 1);
                self.ae2_fluid_p2p_entries_inner(target, face, guard)
                    .unwrap_or_else(|| {
                        self.ae2_interface_fluid_entries(target, face)
                            .unwrap_or_else(|| {
                                self.ae2_tank_contents_encoded(target)
                                    .ok()
                                    .flatten()
                                    .into_iter()
                                    .collect()
                            })
                    })
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        guard.remove(&marker);
        Some(result)
    }
    fn ae2_fluid_p2p_drain_inner(
        &mut self,
        address: GridAddress,
        face: Direction,
        key: &ItemKey,
        amount: u64,
        simulate: bool,
        guard: &mut Guard,
    ) -> Option<u64> {
        let port = self.ae2_p2p_port(address, face, "ae2:fluid_p2p_tunnel")?;
        if !port.output || amount == 0 {
            return Some(0);
        }
        let marker = (port.pos, port.face, false);
        if !guard.insert(marker) {
            return Some(0);
        }
        let moved = if let Some(input) = self.ae2_p2p_peers(&port, false).first() {
            if let Some(target) = adjacent(input) {
                let face = direction(input.face ^ 1);
                self.ae2_fluid_p2p_drain_inner(target, face, key, amount, simulate, guard)
                    .unwrap_or_else(|| {
                        self.ae2_interface_fluid_move(target, face, key, amount, false, simulate)
                            .unwrap_or_else(|| {
                                self.ae2_tank_drain_encoded(target, key, amount, simulate)
                                    .unwrap_or(0)
                            })
                    })
            } else {
                0
            }
        } else {
            0
        };
        guard.remove(&marker);
        if !simulate && moved > 0 {
            self.ae2_p2p_tax(&port, moved as f64 / 125.0);
        }
        Some(moved)
    }
    pub(in crate::game) fn ae2_fluid_p2p_fill(
        &mut self,
        address: GridAddress,
        face: Direction,
        key: &ItemKey,
        amount: u64,
        simulate: bool,
    ) -> Option<u64> {
        let before = self.ae2_snapshot_blocks();
        let mut plan = crate::game::ae2_buses::FormationPlan::default();
        let result = self.ae2_fluid_p2p_fill_staged(address, face, key, amount, simulate, &mut plan);
        if !simulate && result.is_some() && self.ae2_commit_formation(plan).is_err() {
            self.ae2_restore_blocks(before);
            return Some(0);
        }
        result
    }
    pub(in crate::game) fn ae2_fluid_p2p_fill_staged(
        &mut self, address: GridAddress, face: Direction, key: &ItemKey, amount: u64, simulate: bool,
        plan: &mut crate::game::ae2_buses::FormationPlan,
    ) -> Option<u64> {
        let savepoint = plan.len();
        let result = self.ae2_fluid_p2p_fill_inner(address, face, key, amount, simulate, &mut Guard::new(), plan);
        if simulate { plan.truncate(savepoint); }
        result
    }
    pub(in crate::game) fn ae2_fluid_p2p_entries(
        &self,
        address: GridAddress,
        face: Direction,
    ) -> Option<Vec<(ItemKey, u64)>> {
        self.ae2_fluid_p2p_entries_inner(address, face, &mut Guard::new())
    }
    pub(in crate::game) fn ae2_fluid_p2p_contents(
        &self,
        address: GridAddress,
        face: Direction,
    ) -> Option<Option<(ItemKey, u64)>> {
        Some(
            self.ae2_fluid_p2p_entries(address, face)?
                .into_iter()
                .next(),
        )
    }
    pub(in crate::game) fn ae2_fluid_p2p_drain(
        &mut self,
        address: GridAddress,
        face: Direction,
        key: &ItemKey,
        amount: u64,
        simulate: bool,
    ) -> Option<u64> {
        self.ae2_fluid_p2p_drain_inner(address, face, key, amount, simulate, &mut Guard::new())
    }
}
