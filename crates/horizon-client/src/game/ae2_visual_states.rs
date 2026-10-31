//! Source-backed AE2 block-state projections from a single network analysis.
use super::Game;
use ae2_links_core::{Pos as LinkPos, PylonLine};
use ae2_network_core::{Analysis, ControllerState, Network};
use horizon_client::{ae2_world::MachineKind, containers::BlockEntity};
use horizon_world::chunk::BlockState;
use std::collections::{BTreeMap, BTreeSet};

fn controller_type(
    position: (i32, i32, i32),
    mut is_controller: impl FnMut((i32, i32, i32)) -> bool,
) -> &'static str {
    let (x, y, z) = position;
    let axes = [
        is_controller((x - 1, y, z)) && is_controller((x + 1, y, z)),
        is_controller((x, y - 1, z)) && is_controller((x, y + 1, z)),
        is_controller((x, y, z - 1)) && is_controller((x, y, z + 1)),
    ];
    match axes {
        [true, false, false] => "column_x",
        [false, true, false] => "column_y",
        [false, false, true] => "column_z",
        _ if axes.iter().filter(|v| **v).count() >= 2 => {
            if (i64::from(x).abs() + i64::from(y).abs() + i64::from(z).abs()) % 2 == 0 {
                "inside_a"
            } else {
                "inside_b"
            }
        }
        _ => "block",
    }
}
impl Game<'_> {
    /// Caller supplies the network and analysis already computed for this tick.
    pub(super) fn ae2_update_visual_states(&mut self, network: &Network, analysis: &Analysis) {
        let machines: Vec<_> = self
            .world
            .block_entities
            .iter()
            .filter_map(|(&p, e)| match e {
                BlockEntity::Ae2(m) => Some((p, m.kind)),
                _ => None,
            })
            .collect();
        let pylons: BTreeSet<_> = machines
            .iter()
            .filter_map(|(p, k)| {
                (*k == MachineKind::SpatialPylon).then_some(LinkPos(p.0, p.1, p.2))
            })
            .collect();
        let mut lines = BTreeMap::new();
        for line in PylonLine::scan(&pylons) {
            for index in 0..line.count {
                let mut xyz = [line.min.0, line.min.1, line.min.2];
                xyz[line.axis] += index as i32;
                let position = if index == 0 {
                    "start"
                } else if index + 1 == line.count {
                    "end"
                } else {
                    "middle"
                };
                lines.insert(
                    (xyz[0], xyz[1], xyz[2]),
                    (["x", "y", "z"][line.axis], position),
                );
            }
        }
        for (p, kind) in machines {
            let old = self.world.block(p.0, p.1, p.2);
            let Some(state) = self.pack.assets.registry.state(old) else {
                continue;
            };
            let mut props = state.properties.clone();
            let status = self
                .ae2_local_address(p)
                .and_then(|address| analysis.nodes.get(&address.network_pos()));
            match kind {
                MachineKind::Controller => {
                    let visual = match status {
                        Some(s) if s.powered => {
                            if analysis.components[s.component].controller
                                == ControllerState::Conflict
                            {
                                "conflicted"
                            } else {
                                "online"
                            }
                        }
                        _ => "offline",
                    };
                    props.insert("state".into(), visual.into());
                    props.insert(
                        "type".into(),
                        controller_type(p, |q| {
                            self.res
                                .facts()
                                .get(self.world.block(q.0, q.1, q.2))
                                .name
                                .as_ref()
                                == "ae2:controller"
                        })
                        .into(),
                    );
                }
                MachineKind::WirelessAccessPoint => {
                    let visual = match status {
                        Some(s) if s.online => "has_channel",
                        Some(s) if s.powered => "on",
                        _ => "off",
                    };
                    props.insert("state".into(), visual.into());
                }
                MachineKind::GrowthAccelerator => {
                    let local=matches!(self.world.block_entities.get(&p),Some(BlockEntity::Ae2(m)) if m.energy>=8.0);
                    props.insert("powered".into(),(local || status.is_some_and(|s|s.powered)).to_string());
                }
                MachineKind::EnergyCell | MachineKind::DenseEnergyCell => {
                    let capacity=if kind==MachineKind::EnergyCell {200_000.0} else {1_600_000.0};
                    let energy=match self.world.block_entities.get(&p) {Some(BlockEntity::Ae2(m)) if m.energy.is_finite()=>m.energy,_=>0.0};
                    let fullness=(4.0*(energy/capacity+0.01).clamp(0.0,1.0)).floor() as u8;
                    props.insert("fullness".into(),fullness.to_string());
                }
                MachineKind::VibrationChamber => {
                    let active=matches!(self.world.block_entities.get(&p),Some(BlockEntity::Ae2(m)) if m.burn_remaining>0.0);
                    props.insert("active".into(),active.to_string());
                }
                MachineKind::MolecularAssembler => {
                    // Source isPowered requires node power plus a successful
                    // simulated 1 AE extraction, never a real debit.
                    let powered=status.is_some_and(|s|s.powered && analysis.power_components[analysis.components[s.component].power_component].energy > 0.0001);
                    props.insert("powered".into(),powered.to_string());
                }
                MachineKind::IoPort | MachineKind::SpatialIoPort | MachineKind::SpatialAnchor => {
                    props.insert("powered".into(),status.is_some_and(|s|s.online).to_string());
                }
                MachineKind::Chest => {
                    let (cell,local)=match self.world.block_entities.get(&p) {Some(BlockEntity::Ae2(m))=>(m.slots.first().and_then(Option::as_ref).is_some_and(super::ae2_storage_devices::is_valid_storage_cell),m.energy>1.0),_=>(false,false)};
                    props.insert("lights_on".into(),(cell&&(local||status.is_some_and(|s|s.powered))).to_string());
                }
                MachineKind::CraftingUnit | MachineKind::CraftingAccelerator | MachineKind::CraftingStorage1k | MachineKind::CraftingStorage4k | MachineKind::CraftingStorage16k | MachineKind::CraftingStorage64k | MachineKind::CraftingStorage256k => {
                    let formed=self.ae2_local_address(p).and_then(|address|network.nodes.get(&address.network_pos())).is_some_and(|device|device.multiblock.is_some());
                    props.insert("formed".into(),formed.to_string());
                    props.insert("powered".into(),status.is_some_and(|s|s.online).to_string());
                }
                MachineKind::CraftingMonitor => {
                    let formed=self.ae2_local_address(p)
                        .and_then(|address|network.nodes.get(&address.network_pos()))
                        .is_some_and(|device|device.multiblock.is_some());
                    props.insert("formed".into(),formed.to_string());
                    props.insert("powered".into(),status.is_some_and(|s|s.online).to_string());
                }
                MachineKind::SpatialPylon => {
                    props.insert(
                        "powered_on".into(),
                        status.is_some_and(|s| s.powered).to_string(),
                    );
                    let (axis, line) = lines.get(&p).copied().unwrap_or(("x", "none"));
                    props.insert("axis".into(), axis.into());
                    props.insert("line".into(), line.into());
                    props.insert(
                        "online".into(),
                        (line != "none" && status.is_some_and(|s| s.online)).to_string(),
                    );
                }
                MachineKind::QuantumRing | MachineKind::QuantumLink => {
                    // Loaded block entities start with no constructed cluster;
                    // leave their saved appearance until a real callback changes it.
                    let Some(formed)=self.ae2_quantum_visual_formed(p) else {continue};
                    props.insert(
                        "formed".into(),
                        formed.to_string(),
                    );
                }
                _ => continue,
            }
            let Ok(new) = self.pack.assets.registry.resolve(&BlockState {
                name: state.name.clone(),
                properties: props,
            }) else {
                continue;
            };
            if new != old {
                if matches!(kind,MachineKind::QuantumRing|MachineKind::QuantumLink) {
                    // AE2 guards recursive multiblock calculations while it
                    // updates members. Do not manufacture extra neighbor
                    // callbacks when applying this deferred visual projection.
                    self.with_level(|b,level,ticks| {b.set_block(level,ticks,horizon_core::position::BlockPos::new(p.0,p.1,p.2),new,horizon_world::level::flags::UPDATE_CLIENTS|horizon_world::level::flags::UPDATE_KNOWN_SHAPE);});
                    let _=self.process_level_events();
                } else {
                    let _ = self.level_set([p.0, p.1, p.2], new);
                }
            }
        }
        let _ = network; // retained in the API for subsequent connected-model projections
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controller_layout_matches_source() {
        let blocks = std::collections::HashSet::from([(0, 0, 0), (-1, 0, 0), (1, 0, 0)]);
        assert_eq!(
            controller_type((0, 0, 0), |p| blocks.contains(&p)),
            "column_x"
        );
        let blocks = std::collections::HashSet::from([
            (0, 0, 0),
            (-1, 0, 0),
            (1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
        ]);
        assert_eq!(
            controller_type((0, 0, 0), |p| blocks.contains(&p)),
            "inside_a"
        );
    }
}
