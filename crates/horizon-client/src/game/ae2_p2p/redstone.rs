//! RedstoneP2PTunnelPart from AE2 19.2.18 (LGPL-3.0-or-later).
//! Values are persisted on the physical part; the native redstone engine
//! reads them through LevelAccess and receives normal neighbor updates.
use super::{Game, Port, capabilities};
use crate::game::ae2_global::GridAddress;
use horizon_client::containers::BlockEntity;
use horizon_core::{geometry::Direction, position::BlockPos};
use horizon_world::nbt::Tag;

impl Game<'_> {
    fn ae2_p2p_redstone_value(&self, port: &Port) -> i32 {
        let Some(address) = GridAddress::from_network(port.pos) else {
            return 0;
        };
        let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity(address) else {
            return 0;
        };
        let Some(Tag::List(parts)) = m.config.get(&"parts".into()) else {
            return 0;
        };
        parts
            .iter()
            .find(|part| part_matches(part, port))
            .map_or(0, |part| match part.get("p2p_power") {
                Some(Tag::Int(value)) => (*value).clamp(0, 15),
                _ => 0,
            })
    }

    fn ae2_p2p_set_redstone(&mut self, port: &Port, value: i32) -> bool {
        let Some(address) = GridAddress::from_network(port.pos) else {
            return false;
        };
        let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity_mut(address) else {
            return false;
        };
        let Some(Tag::List(parts)) = m.config.get_mut(&"parts".into()) else {
            return false;
        };
        let Some(Tag::Compound(part)) = parts.iter_mut().find(|part| part_matches(part, port))
        else {
            return false;
        };
        let next = Tag::Int(value.clamp(0, 15));
        if part.get(&"p2p_power".into()) == Some(&next) {
            return false;
        }
        part.insert("p2p_power".into(), next);
        self.ae2_mark_modified(address);
        true
    }

    pub(in crate::game) fn ae2_tick_redstone_p2p(&mut self) {
        // Each level pass samples its own actual block-state signal. Loaded
        // cross-dimensional peers read that cached value by stable address.
        let ports: Vec<_> = self
            .ae2_p2p_ports()
            .into_iter()
            .filter(|port| {
                port.id == "ae2:redstone_p2p_tunnel"
                    && GridAddress::from_network(port.pos)
                        .and_then(|a| self.ae2_address_parts(a))
                        .is_some_and(|(dim, _)| dim == self.dim)
            })
            .collect();
        for port in ports.iter().filter(|port| !port.output) {
            let Some((_, front)) =
                capabilities::adjacent(port).and_then(|a| self.ae2_address_parts(a))
            else {
                continue;
            };
            let at = BlockPos::new(front.0, front.1, front.2);
            let state = self.world.block(front.0, front.1, front.2);
            let side = if self.res.facts().get(state).name.as_ref() == "minecraft:redstone_wire" {
                Direction::Up
            } else {
                capabilities::direction(port.face)
            };
            let value = self.with_level(|b, level, ticks| b.block_signal(level, ticks, at, side));
            self.ae2_p2p_set_redstone(port, value);
        }
        for port in ports.iter().filter(|port| port.output) {
            let value = self
                .ae2_p2p_peers(port, false)
                .first()
                .map_or(0, |input| self.ae2_p2p_redstone_value(input));
            if !self.ae2_p2p_set_redstone(port, value) {
                continue;
            }
            let Some((_, local)) =
                GridAddress::from_network(port.pos).and_then(|a| self.ae2_address_parts(a))
            else {
                continue;
            };
            let at = BlockPos::new(local.0, local.1, local.2);
            let state = self.world.block(local.0, local.1, local.2);
            self.with_level(|b, level, ticks| {
                b.update_neighbors_at(level, ticks, at, b.block(state));
                for delta in [
                    [1, 0, 0],
                    [-1, 0, 0],
                    [0, 1, 0],
                    [0, -1, 0],
                    [0, 0, 1],
                    [0, 0, -1],
                ] {
                    b.update_neighbors_at(
                        level,
                        ticks,
                        BlockPos::new(at.x + delta[0], at.y + delta[1], at.z + delta[2]),
                        b.block(state),
                    );
                }
            });
        }
    }
}

fn part_matches(part: &Tag, port: &Port) -> bool {
    super::text(part, "id").as_deref() == Some("ae2:redstone_p2p_tunnel")
        && super::text(part, "face")
            .and_then(|face| super::face(&face))
            .is_some_and(|(face, _)| face == port.face)
}
