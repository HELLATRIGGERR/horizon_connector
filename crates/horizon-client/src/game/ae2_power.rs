//! Native external FE adapters and AE2's source debug energy generator.
//! AE2 19.2.18 ForgeEnergyAdapter, EnergyGeneratorBlockEntity (LGPL-3.0+).
//! Default source ratio: 1 FE = 0.5 AE. Private machine buffers are kept
//! separate from the grid's public controller/cell storage.
use super::{Game, ae2_global::GridAddress, ae2_processing};
use horizon_client::{ae2_world::MachineKind, containers::BlockEntity};
use horizon_core::geometry::Direction;
use horizon_world::nbt::Tag;

#[derive(Clone, Copy, Default, Debug)]
pub(super) struct FeStorage {
    // IEnergyStorage returns signed Java ints, including wrapped P2P sums.
    pub stored: i32,
    pub capacity: i32,
    pub can_receive: bool,
    pub can_extract: bool,
}
const SIDES: [(Direction, [i32; 3]); 6] = [
    (Direction::East, [1, 0, 0]),
    (Direction::West, [-1, 0, 0]),
    (Direction::Up, [0, 1, 0]),
    (Direction::Down, [0, -1, 0]),
    (Direction::South, [0, 0, 1]),
    (Direction::North, [0, 0, -1]),
];
fn as_fe(ae: f64) -> u32 {
    as_fe_with_ratio(ae, horizon_client::ae2_common_config::gameplay().forge_energy)
}
fn as_fe_with_ratio(ae: f64, ratio: f64) -> u32 {
    // Java's narrowing double-to-int conversion saturates at Integer.MAX_VALUE
    // (including positive infinity) and truncates the fractional FE.
    (ae.max(0.0) / ratio).floor().min(f64::from(i32::MAX)) as u32
}
/// ForgeEnergyAdapter returns `(int)(maxReceive - overflowFE)`, after the
/// sink has accepted its full (possibly fractional-FE) amount in AE.
fn received_fe(offered: u32, offered_ae: f64, accepted_ae: f64, ratio: f64) -> u32 {
    let overflow = (offered_ae - accepted_ae).max(0.0) / ratio;
    (f64::from(offered) - overflow).max(0.0).min(f64::from(offered)) as u32
}
fn generation_rate(tag: Option<&Tag>) -> u32 {
    match tag {
        Some(Tag::Int(n)) => (*n).max(0) as u32,
        _ => 8,
    }
}
fn checked_generation(rate: u32, tier: u32) -> u32 {
    rate.checked_pow(tier)
        .unwrap_or(i32::MAX as u32)
        .min(i32::MAX as u32)
}
impl Game<'_> {
    /// Read-only native capability and propagated-light diagnostics.
    pub(super) fn ae2_power_command(&mut self, args: &[&str]) -> Result<serde_json::Value, String> {
        if args.len() != 4 {
            return Err("usage: /ae2 power x y z face".into());
        }
        let pos = (
            args[0].parse().map_err(|_| "x coordinate")?,
            args[1].parse().map_err(|_| "y coordinate")?,
            args[2].parse().map_err(|_| "z coordinate")?,
        );
        let face = match args[3] {
            "down" => Direction::Down,
            "up" => Direction::Up,
            "north" => Direction::North,
            "south" => Direction::South,
            "west" => Direction::West,
            "east" => Direction::East,
            _ => return Err("block side".into()),
        };
        self.ae2_ensure_machine(pos);
        let address = self.ae2_local_address(pos).ok_or("invalid position")?;
        let packed = self.world.light(pos.0, pos.1, pos.2);
        let energy = match self.ae2_block_entity(address) {
            Some(BlockEntity::Ae2(m)) => Some(m.energy),
            _ => None,
        };
        let fe=self.ae2_fe_storage(address,face).map(|s|serde_json::json!({"stored":s.stored,"capacity":s.capacity,"can_receive":s.can_receive,"can_extract":s.can_extract}));
        let state = self
            .pack
            .assets
            .registry
            .state(self.world.block(pos.0, pos.1, pos.2));
        Ok(
            serde_json::json!({"position":[pos.0,pos.1,pos.2],"energy":energy,"energy_service":self.ae2_energy_storage_diagnostic(address),"fe":fe,"block_light":packed&15,"sky_light":packed>>4,"properties":state.map(|s|&s.properties)}),
        )
    }
    fn ae2_energy_acceptor_face(&self, address: GridAddress, face: Direction) -> bool {
        let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity(address) else {
            return false;
        };
        if m.kind == MachineKind::EnergyAcceptor || m.kind == MachineKind::Controller {
            return true;
        }
        if m.kind != MachineKind::CableBus {
            return false;
        }
        let name = match face {
            Direction::Down => "down",
            Direction::Up => "up",
            Direction::North => "north",
            Direction::South => "south",
            Direction::West => "west",
            Direction::East => "east",
        };
        let Some(Tag::List(parts)) = m.config.get(&"parts".into()) else {
            return false;
        };
        parts.iter().any(|p| {
            p.get("id") == Some(&Tag::String("ae2:cable_energy_acceptor".into()))
                && p.get("face") == Some(&Tag::String(name.into()))
        })
    }
    pub(super) fn ae2_native_fe_storage(
        &self,
        address: GridAddress,
        face: Direction,
    ) -> Option<FeStorage> {
        let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity(address) else {
            return None;
        };
        if m.kind == MachineKind::DebugEnergyGenerator {
            return Some(FeStorage {
                stored: i32::MAX,
                capacity: i32::MAX,
                can_extract: true,
                can_receive: false,
            });
        }
        if self.ae2_energy_acceptor_face(address, face) {
            return Some(FeStorage {
                stored: if m.kind == MachineKind::Controller {
                    as_fe(m.energy) as i32
                } else {
                    0
                },
                capacity: if m.kind == MachineKind::Controller {
                    as_fe(8000.0) as i32
                } else {
                    0
                },
                can_receive: true,
                can_extract: false,
            });
        }
        let capacity = ae2_processing::ae2_private_power_capacity(m)?;
        let state = self
            .ae2_grid_block(address)
            .and_then(|s| self.pack.assets.registry.state(s))?;
        if !ae2_processing::ae2_private_power_face(m.kind, state, face) {
            return None;
        }
        Some(FeStorage {
            stored: as_fe(m.energy.min(capacity)) as i32,
            capacity: as_fe(capacity) as i32,
            can_receive: true,
            can_extract: false,
        })
    }
    pub(super) fn ae2_native_fe_extract(
        &mut self,
        address: GridAddress,
        face: Direction,
        amount: u32,
        _simulate: bool,
    ) -> u32 {
        if self
            .ae2_native_fe_storage(address, face)
            .is_some_and(|s| s.can_extract)
        {
            amount.min(i32::MAX as u32)
        } else {
            0
        }
    }
    pub(super) fn ae2_native_fe_receive(
        &mut self,
        address: GridAddress,
        face: Direction,
        amount: u32,
        simulate: bool,
    ) -> u32 {
        if amount == 0 {
            return 0;
        }
        if !self
            .ae2_native_fe_storage(address, face)
            .is_some_and(|s| s.can_receive)
        {
            return 0;
        }
        let fe_ratio = horizon_client::ae2_common_config::gameplay().forge_energy;
        let offered_ae = (f64::from(amount) * fe_ratio).min(f64::MAX);
        if self.ae2_energy_acceptor_face(address, face) {
            let part_face=matches!(self.ae2_block_entity(address),Some(BlockEntity::Ae2(machine)) if machine.kind==MachineKind::CableBus).then_some(match face {
                Direction::East=>0,Direction::West=>1,Direction::Up=>2,Direction::Down=>3,Direction::South=>4,Direction::North=>5,
            });
            if let Ok(accepted_ae)=self.ae2_energy_transaction(address,part_face,offered_ae,simulate,true) {
                return received_fe(amount,offered_ae,accepted_ae,fe_ratio);
            }
            let mut network = self.ae2_network();
            let Ok(accepted_ae) = network.insert_energy(address.network_pos(), offered_ae)
            else {
                return 0;
            };
            if !simulate && accepted_ae > 0.0 { self.ae2_commit_power(&network); }
            return received_fe(amount,offered_ae,accepted_ae,fe_ratio);
        }
        let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity_mut(address) else {return 0;};
        let old_energy = m.energy;
        let accepted_ae = m.private_energy_insert(offered_ae, !simulate);
        let changed = m.energy != old_energy;
        if changed { self.ae2_mark_modified(address); }
        received_fe(amount,offered_ae,accepted_ae,fe_ratio)
    }
    pub(super) fn ae2_tick_external_power(&mut self) {
        let sources: Vec<_> = self
            .world
            .block_entities
            .iter()
            .filter_map(|(&pos, be)| {
                let BlockEntity::Ae2(m) = be else {
                    return None;
                };
                (m.kind == MachineKind::DebugEnergyGenerator)
                    .then_some((pos, generation_rate(m.config.get(&"generationRate".into()))))
            })
            .collect();
        for (pos, rate) in sources {
            let tier=1+SIDES.iter().filter(|(_,d)|matches!(self.world.block_entities.get(&(pos.0+d[0],pos.1+d[1],pos.2+d[2])),Some(BlockEntity::Ae2(m)) if m.kind==MachineKind::DebugEnergyGenerator)).count() as u32;
            let amount = checked_generation(rate, tier);
            for (side, delta) in SIDES {
                let target = (pos.0 + delta[0], pos.1 + delta[1], pos.2 + delta[2]);
                let Some(address) = self.ae2_local_address(target) else {
                    continue;
                };
                let _ = self.ae2_fe_receive(address, side.opposite(), amount, false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_fe_scaling_and_integer_boundaries() {
        assert_eq!(as_fe_with_ratio(8.0, 0.5), 16);
        assert_eq!(as_fe_with_ratio(0.49, 0.5), 0);
        assert_eq!(as_fe_with_ratio(8.0, 2.0), 4);
        assert_eq!(as_fe_with_ratio(8.0, 1.5), 5);
        assert_eq!(as_fe_with_ratio(f64::MAX, f64::from_bits(1)), i32::MAX as u32);
        assert_eq!(received_fe(8, 4.0, 0.4, 0.5), 0);
        assert_eq!(received_fe(8, 4.0, 3.9, 0.5), 7);
        assert_eq!(received_fe(8, 4.0, 4.0, 0.5), 8);
        assert_eq!(checked_generation(8, 3), 512);
        assert_eq!(checked_generation(128, 6), i32::MAX as u32);
        assert_eq!(generation_rate(Some(&Tag::Int(-5))), 0);
    }
}
