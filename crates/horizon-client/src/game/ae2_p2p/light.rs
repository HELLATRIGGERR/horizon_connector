//! LightP2PTunnelPart, AE2 19.2.18 (LGPL-3.0-or-later).
//! The output updates native block emission, including propagated light.
use super::{Game, Port, capabilities};
use crate::game::ae2_global::GridAddress;
use horizon_client::{ae2_world::MachineKind, containers::BlockEntity};
use horizon_world::{chunk::BlockState, nbt::Tag, registry::BlockStateId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(in crate::game) struct LightCache {
    neighbor: Option<BlockStateId>,
    opacity: Option<i32>,
    next_sample: i64,
    active: Option<bool>,
}
impl Game<'_> {
    fn ae2_light_value(&self, port: &Port) -> i32 {
        let Some(BlockEntity::Ae2(m)) =
            GridAddress::from_network(port.pos).and_then(|a| self.ae2_block_entity(a))
        else {
            return 0;
        };
        let Some(Tag::List(parts)) = m.config.get(&"parts".into()) else {
            return 0;
        };
        parts
            .iter()
            .find(|p| part_matches(p, port))
            .map_or(0, |p| match p.get("p2p_light") {
                Some(Tag::Int(n)) => (*n).clamp(0, 15),
                _ => 0,
            })
    }
    fn ae2_set_light_value(&mut self, port: &Port, value: i32) -> bool {
        let Some(address) = GridAddress::from_network(port.pos) else {
            return false;
        };
        let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity_mut(address) else {
            return false;
        };
        let Some(Tag::List(parts)) = m.config.get_mut(&"parts".into()) else {
            return false;
        };
        let Some(Tag::Compound(part)) = parts.iter_mut().find(|p| part_matches(p, port)) else {
            return false;
        };
        let value = Tag::Int(value.clamp(0, 15));
        if part.get(&"p2p_light".into()) == Some(&value) {
            return false;
        }
        part.insert("p2p_light".into(), value);
        self.ae2_mark_modified(address);
        true
    }
    fn ae2_front_brightness(&self, port: &Port) -> i32 {
        let Some((dim, p)) = capabilities::adjacent(port).and_then(|a| self.ae2_address_parts(a))
        else {
            return 0;
        };
        let Some(world) = self.ae2_world_for(dim).filter(|w| w.loaded(p.0, p.2)) else {
            return 0;
        };
        let packed = world.light(p.0, p.1, p.2);
        // This helper is called in the front's owning level pass, so the
        // native time/weather sky-darkening belongs to that same dimension.
        i32::from(packed & 15)
            .max(i32::from(packed >> 4) - self.sky_darken())
            .clamp(0, 15)
    }
    pub(in crate::game) fn ae2_tick_light_p2p(&mut self) {
        let all: Vec<_> = self
            .ae2_p2p_ports()
            .into_iter()
            .filter(|p| p.id == "ae2:light_p2p_tunnel")
            .collect();
        let keys: BTreeSet<_> = all
            .iter()
            .filter_map(|p| Some((GridAddress::from_network(p.pos)?, p.face)))
            .collect();
        self.ae2_p2p_light_cache.retain(|key, _| keys.contains(key));
        let ports: Vec<_> = all
            .into_iter()
            .filter(|p| {
                GridAddress::from_network(p.pos)
                    .and_then(|a| self.ae2_address_parts(a))
                    .is_some_and(|(dim, _)| dim == self.dim)
            })
            .collect();
        let tick=self.ae2_server_tick();
        let now=tick.min(i64::MAX as u64) as i64;
        let network=self.ae2_network();
        let analysis=network.analyze().ok();
        let services=self.ae2_event_service_ids(&network);
        for port in ports.iter().filter(|p| !p.output) {
            let Some(address) = GridAddress::from_network(port.pos) else {
                continue;
            };
            let front = capabilities::adjacent(port).and_then(|a| self.ae2_grid_block(a));
            let active=analysis.as_ref().and_then(|a|network.part_status(a,port.pos,port.face)).is_some_and(|s|s.online);
            let cache = self
                .ae2_p2p_light_cache
                .entry((address, port.face))
                .or_default();
            let neighbor_changed=cache.neighbor != front || cache.active != Some(active);
            cache.neighbor = front;
            cache.active=Some(active);
            let Some(identity)=network.part_identities.get(&(port.pos,port.face)).copied() else {continue};
            let service=services.as_ref().and_then(|s|s.get(&identity).copied());
            let (min,max)=horizon_client::ae2_tick_config::rates(horizon_client::ae2_tick_config::Kind::LightTunnel);
            let due=self.ae2_device_scheduler.borrow_mut().due(identity,service,tick,min,max,false,false);
            if !due && !neighbor_changed {continue;}
            let value = self.ae2_front_brightness(port);
            let changed = active && self.ae2_set_light_value(port, value);
            if due {self.ae2_device_scheduler.borrow_mut().completed(identity,tick,if changed {crate::game::ae2_tick_tracker::Modulation::Urgent} else {crate::game::ae2_tick_tracker::Modulation::Slower});}
        }
        let mut emissions: BTreeMap<_, i32> = BTreeMap::new();
        for port in ports.iter().filter(|p| p.output) {
            let Some(address) = GridAddress::from_network(port.pos) else {
                continue;
            };
            let input = self.ae2_p2p_peers(port, false).first().cloned();
            let value = input
                .as_ref()
                .map_or(0, |input| self.ae2_light_value(input));
            self.ae2_set_light_value(port, value);
            if !self.ae2_p2p_light_cache.contains_key(&(address, port.face)) {
                let old_emission = self
                    .ae2_grid_block(address)
                    .and_then(|state| self.pack.assets.registry.state(state))
                    .and_then(|state| state.properties.get("light"))
                    .and_then(|n| n.parse::<u8>().ok())
                    .unwrap_or(0);
                if old_emission > 0 {
                    // `light` is the native host projection of the dynamic
                    // part light, not source part state. Discard that saved
                    // projection before measuring ambient light on reload.
                    self.ae2_p2p_light_cache.insert(
                        (address, port.face),
                        LightCache {
                            next_sample: now.saturating_add(5),
                            ..Default::default()
                        },
                    );
                }
            }
            if self
                .ae2_p2p_light_cache
                .get(&(address, port.face))
                .is_some_and(|c| c.opacity.is_none() && now < c.next_sample)
            {
                continue;
            }
            let front = capabilities::adjacent(port).and_then(|a| self.ae2_grid_block(a));
            let refresh = self
                .ae2_p2p_light_cache
                .get(&(address, port.face))
                .is_none_or(|c| c.opacity.is_none() || c.neighbor != front);
            if refresh {
                let opacity = self.ae2_front_brightness(port);
                let cache = self
                    .ae2_p2p_light_cache
                    .entry((address, port.face))
                    .or_default();
                cache.neighbor = front;
                cache.opacity = Some(opacity);
            }
            let opacity = self
                .ae2_p2p_light_cache
                .get(&(address, port.face))
                .and_then(|c| c.opacity)
                .unwrap_or(0);
            let emission = if input.is_some() {
                (value - opacity).max(0)
            } else {
                0
            };
            if let Some((_, pos)) = self.ae2_address_parts(address) {
                emissions
                    .entry(pos)
                    .and_modify(|v| *v = (*v).max(emission))
                    .or_insert(emission);
            }
        }
        let buses: Vec<_> = self
            .world
            .block_entities
            .iter()
            .filter_map(|(&p, be)| {
                matches!(be,BlockEntity::Ae2(m) if m.kind==MachineKind::CableBus).then_some(p)
            })
            .collect();
        let analysis=self.ae2_network().analyze().ok();
        for p in buses {
            let panel_powered=self.ae2_local_address(p).and_then(|a|analysis.as_ref()?.nodes.get(&a.network_pos())).is_some_and(|status| {
                let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get(&p) else {return false};
                let Some(Tag::List(parts))=machine.config.get(&"parts".into()) else {return false};
                parts.iter().any(|part| {
                    let Some(id)=super::text(part,"id") else {return false};
                    if !matches!(id.as_str(),"ae2:monitor"|"ae2:semi_dark_monitor"|"ae2:dark_monitor") {return false}
                    super::text(part,"face").and_then(|f|super::face(&f)).and_then(|(face,_)|status.parts.get(&face)).is_some_and(|s|s.online)
                })
            });
            let old = self.world.block(p.0, p.1, p.2);
            let Some(state) = self.pack.assets.registry.state(old) else {
                continue;
            };
            if !state.properties.contains_key("light") {
                continue;
            }
            let mut properties = state.properties.clone();
            properties.insert(
                "light".into(),
                emissions.get(&p).copied().unwrap_or(0).max(if panel_powered {15}else{0}).to_string(),
            );
            let Ok(next) = self.pack.assets.registry.resolve(&BlockState {
                name: state.name.clone(),
                properties,
            }) else {
                continue;
            };
            if old != next {
                let _ = self.level_set([p.0, p.1, p.2], next);
            }
        }
    }
}
fn part_matches(part: &Tag, port: &Port) -> bool {
    super::text(part, "id").as_deref() == Some("ae2:light_p2p_tunnel")
        && super::text(part, "face")
            .and_then(|face| super::face(&face))
            .is_some_and(|(face, _)| face == port.face)
}
