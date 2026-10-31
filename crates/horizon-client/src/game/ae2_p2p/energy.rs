//! FEP2PTunnelPart capability forwarding, AE2 19.2.18, LGPL-3.0-or-later.
//! This uses physical adjacent energy handlers; it never creates tunnel energy.
use super::{Game, Port, capabilities};
use crate::game::{ae2_global::GridAddress, ae2_power::FeStorage};
use ae2_network_core::Pos;
use horizon_core::geometry::Direction;
use std::collections::BTreeSet;
type Guard = BTreeSet<(Pos, u8, u8)>;

// FEP2PTunnelPart.InputEnergyStorage sums Java int readbacks, which wrap
// rather than clamp when multiple large adjacent handlers are present.
fn accumulate_readbacks(total: &mut FeStorage, handler: FeStorage) {
    total.stored = total.stored.wrapping_add(handler.stored);
    total.capacity = total.capacity.wrapping_add(handler.capacity);
}

impl Game<'_> {
    fn ae2_fe_receive_inner(
        &mut self,
        address: GridAddress,
        face: Direction,
        amount: u32,
        simulate: bool,
        guard: &mut Guard,
    ) -> u32 {
        let Some(port) = self.ae2_p2p_port(address, face, "ae2:fe_p2p_tunnel") else {
            return self.ae2_native_fe_receive(address, face, amount, simulate);
        };
        if port.output || amount == 0 {
            return 0;
        }
        let key = (port.pos, port.face, 0);
        if !guard.insert(key) {
            return 0;
        }
        let outputs = self.ae2_p2p_peers(&port, true);
        let mut left = amount;
        if !outputs.is_empty() {
            let share = amount / (outputs.len() as u32);
            let mut overflow = if share == 0 { amount } else { amount % share };
            for output in outputs {
                let offered = share.saturating_add(overflow).min(left);
                if offered == 0 {
                    break;
                }
                let Some(target) = capabilities::adjacent(&output) else {
                    continue;
                };
                let accepted = self
                    .ae2_fe_receive_inner(
                        target,
                        capabilities::direction(output.face ^ 1),
                        offered,
                        simulate,
                        guard,
                    )
                    .min(offered);
                overflow = offered - accepted;
                left -= accepted;
            }
        }
        guard.remove(&key);
        let accepted = amount - left;
        if !simulate && accepted > 0 {
            self.ae2_p2p_energy_tax(&port, f64::from(accepted));
        }
        accepted
    }

    fn ae2_fe_extract_inner(
        &mut self,
        address: GridAddress,
        face: Direction,
        amount: u32,
        simulate: bool,
        guard: &mut Guard,
    ) -> u32 {
        let Some(port) = self.ae2_p2p_port(address, face, "ae2:fe_p2p_tunnel") else {
            return self.ae2_native_fe_extract(address, face, amount, simulate);
        };
        if !port.output || amount == 0 {
            return 0;
        }
        let key = (port.pos, port.face, 1);
        if !guard.insert(key) {
            return 0;
        }
        let accepted = self
            .ae2_p2p_peers(&port, false)
            .first()
            .and_then(|input| {
                let target = capabilities::adjacent(input)?;
                Some(self.ae2_fe_extract_inner(
                    target,
                    capabilities::direction(input.face ^ 1),
                    amount,
                    simulate,
                    guard,
                ))
            })
            .unwrap_or(0)
            .min(amount);
        guard.remove(&key);
        if !simulate && accepted > 0 {
            self.ae2_p2p_energy_tax(&port, f64::from(accepted));
        }
        accepted
    }

    fn ae2_fe_storage_inner(
        &self,
        address: GridAddress,
        face: Direction,
        guard: &mut Guard,
    ) -> Option<FeStorage> {
        let Some(port) = self.ae2_p2p_port(address, face, "ae2:fe_p2p_tunnel") else {
            return self.ae2_native_fe_storage(address, face);
        };
        let key = (port.pos, port.face, 2);
        if !guard.insert(key) {
            return Some(FeStorage::default());
        }
        let out = if !self.ae2_fe_port_active(&port) {
            FeStorage::default()
        } else if port.output {
            let mut storage = self
                .ae2_p2p_peers(&port, false)
                .first()
                .and_then(|input| {
                    self.ae2_fe_storage_inner(
                        capabilities::adjacent(input)?,
                        capabilities::direction(input.face ^ 1),
                        guard,
                    )
                })
                .unwrap_or_default();
            storage.can_receive = false;
            storage
        } else {
            let mut storage = FeStorage {
                can_receive: true,
                ..Default::default()
            };
            for output in self.ae2_p2p_peers(&port, true) {
                if let Some(handler) = capabilities::adjacent(&output).and_then(|target| {
                    self.ae2_fe_storage_inner(
                        target,
                        capabilities::direction(output.face ^ 1),
                        guard,
                    )
                }) {
                    accumulate_readbacks(&mut storage, handler);
                }
            }
            storage
        };
        guard.remove(&key);
        Some(out)
    }

    fn ae2_fe_port_active(&self, port: &Port) -> bool {
        self.ae2_network()
            .analyze()
            .ok()
            .and_then(|a| {
                a.nodes
                    .get(&port.pos)
                    .and_then(|n| n.parts.get(&port.face))
                    .map(|p| p.online)
            })
            .unwrap_or(false)
    }
    pub(in crate::game) fn ae2_fe_receive(
        &mut self,
        address: GridAddress,
        face: Direction,
        amount: u32,
        simulate: bool,
    ) -> u32 {
        self.ae2_fe_receive_inner(
            address,
            face,
            amount.min(i32::MAX as u32),
            simulate,
            &mut Guard::new(),
        )
    }
    pub(in crate::game) fn ae2_fe_extract(
        &mut self,
        address: GridAddress,
        face: Direction,
        amount: u32,
        simulate: bool,
    ) -> u32 {
        self.ae2_fe_extract_inner(
            address,
            face,
            amount.min(i32::MAX as u32),
            simulate,
            &mut Guard::new(),
        )
    }
    pub(in crate::game) fn ae2_fe_storage(
        &self,
        address: GridAddress,
        face: Direction,
    ) -> Option<FeStorage> {
        self.ae2_fe_storage_inner(address, face, &mut Guard::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fe_p2p_input_readback_wraps_like_java_int() {
        let mut total = FeStorage::default();
        let large = FeStorage { stored: i32::MAX, capacity: i32::MAX, ..Default::default() };
        accumulate_readbacks(&mut total, large);
        assert_eq!((total.stored, total.capacity), (i32::MAX, i32::MAX));
        accumulate_readbacks(&mut total, FeStorage { stored: 1, capacity: i32::MAX, ..Default::default() });
        assert_eq!((total.stored, total.capacity), (i32::MIN, -2));
        // An already-wrapped nested tunnel contributes its signed result too.
        accumulate_readbacks(&mut total, FeStorage { stored: -1, capacity: 3, ..Default::default() });
        assert_eq!((total.stored, total.capacity), (i32::MAX, 1));
    }
}
