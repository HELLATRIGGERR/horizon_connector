//! P2P tunnel registration, physical memory cards and isolated ME outer grids.
//! Source: AE2 19.2.18 P2PTunnelPart, MEP2PTunnelPart and P2PService, LGPL-3.0+.
use super::Game;
mod capabilities;
mod fluid_capabilities;
mod redstone;
mod energy;
mod light;
pub(super) use light::LightCache;
use ae2_network_core::{Device, DeviceKind, Network, Pos};
use horizon_client::{ae2_world::MachineKind, containers::BlockEntity};
use horizon_core::random::RandomSource;
use horizon_world::nbt::Tag;
use std::collections::{BTreeMap, BTreeSet};
type WorldPos = (i32, i32, i32);
pub(super) fn is_tunnel(id: &str) -> bool {
    matches!(
        id,
        "ae2:me_p2p_tunnel"
            | "ae2:item_p2p_tunnel"
            | "ae2:fluid_p2p_tunnel"
            | "ae2:fe_p2p_tunnel"
            | "ae2:redstone_p2p_tunnel"
            | "ae2:light_p2p_tunnel"
    )
}
pub(super) fn idle(id: &str) -> f64 {
    if matches!(id, "ae2:redstone_p2p_tunnel" | "ae2:light_p2p_tunnel") {
        0.5
    } else {
        2.0
    }
}
fn text(tag: &Tag, key: &str) -> Option<String> {
    if let Tag::String(v) = tag.get(key)? {
        v.to_string().ok()
    } else {
        None
    }
}
/// Memory-card type changes use CableBusContainer.replacePart: the old
/// node is destroyed and the new physical part has its own incarnation.
fn replace_tunnel_type(part: &Tag, new_id: &str) -> Option<Tag> {
    if !is_tunnel(new_id) { return None; }
    let old_id = text(part, "id")?;
    let mut stack = part.get("stack").and_then(horizon_client::save::item_from_nbt)?;
    if stack.count != 1 || stack.item.as_ref() != old_id { return None; }
    stack.item = std::sync::Arc::from(new_id);
    let Tag::Compound(mut data) = part.clone() else { return None };
    data.insert("id".into(), Tag::String(new_id.into()));
    data.insert("stack".into(), horizon_client::save::item_to_nbt(&stack));
    data.insert("ae2_part_identity".into(), Tag::IntArray(horizon_client::save::random_uuid().to_vec()));
    for key in ["gn", "outer", "p2p_frequency", "p2p_output", "p2p_power", "p2p_light"] { data.remove(&key.into()); }
    Some(Tag::Compound(data))
}
fn frequency(tag: &Tag) -> i16 {
    match tag.get("p2p_frequency") {
        Some(Tag::Short(n)) => *n,
        _ => 0,
    }
}
/// P2PTunnelPart consumes registered components; old Horizon saves used an
/// envelope in custom data. A present source component always takes precedence.
fn card_target(card:&horizon_client::items::ItemStack)->Option<(String,Option<i16>)> {
    let components=card.data()?;
    let ty=components.other("ae2:exported_p2p_type");
    let freq=components.other("ae2:exported_p2p_frequency");
    if ty.is_some() || freq.is_some() {
        let Some(Tag::String(id))=ty else{return None;};
        let id=id.to_string().ok().filter(|id|is_tunnel(id))?;
        let frequency=match freq {None=>None,Some(Tag::Short(value))=>Some(*value),_=>return None};
        return Some((id,frequency));
    }
    let saved=components.other("minecraft:custom_data")?.get("ae2_p2p_card")?;
    let id=text(saved,"id").filter(|id|is_tunnel(id))?;
    let Some(Tag::Short(frequency))=saved.get("frequency") else{return None;};
    (*frequency!=0).then_some((id,Some(*frequency)))
}
fn card_colors(frequency:i16)->Tag {
    const COLORS:[&str;16]=["white","light_gray","gray","black","lime","yellow","orange","brown","red","pink","magenta","purple","blue","light_blue","cyan","green"];
    let value=frequency as u16;
    let mut result=Vec::with_capacity(8);
    for shift in [12,8,4,0] {
        let color=COLORS[((value>>shift)&15) as usize];
        result.push(Tag::String(color.into()));result.push(Tag::String(color.into()));
    }
    Tag::List(result)
}
fn face(face: &str) -> Option<(u8, [i32; 3])> {
    Some(match face {
        "east" => (0, [1, 0, 0]),
        "west" => (1, [-1, 0, 0]),
        "up" => (2, [0, 1, 0]),
        "down" => (3, [0, -1, 0]),
        "south" => (4, [0, 0, 1]),
        "north" => (5, [0, 0, -1]),
        _ => return None,
    })
}
#[derive(Clone)]
pub(super) struct Port {
    pub(super) pos: Pos,
    pub(super) face: u8,
    direction: [i32; 3],
    pub(super) id: String,
    pub(super) frequency: i16,
    pub(super) output: bool,
    outer_mode: Option<ae2_network_core::ChannelMode>,
}
impl Game<'_> {
    /// AE2 checks loaded item tags before source-ordered item capabilities.
    fn ae2_try_p2p_attunement(&mut self, pos: WorldPos, side: &str, trigger: &horizon_client::items::ItemStack) -> bool {
        let candidates = ["me_p2p_tunnel", "fe_p2p_tunnel", "redstone_p2p_tunnel", "fluid_p2p_tunnel", "item_p2p_tunnel", "light_p2p_tunnel"];
        let target = candidates.into_iter().find(|id| {
            let tag = format!("ae2:p2p_attunements/{id}");
            self.res.items().item_tags.get(tag.as_str()).is_some_and(|values| values.contains(trigger.item.as_ref()))
        }).map(|id| format!("ae2:{id}"))
        .or_else(|| match horizon_client::ae2_item_capabilities::first_attunement_capability(trigger) {
            Some(horizon_client::ae2_item_capabilities::ItemCapability::Energy) => Some("ae2:fe_p2p_tunnel".to_owned()),
            Some(horizon_client::ae2_item_capabilities::ItemCapability::Fluid) => Some("ae2:fluid_p2p_tunnel".to_owned()),
            None => None,
        });
        let Some(target) = target else { return false };
        let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get(&pos) else { return false };
        let Some(Tag::List(parts)) = machine.config.get(&"parts".into()) else { return false };
        let Some(index) = parts.iter().position(|part| text(part,"face").as_deref()==Some(side) && text(part,"id").is_some_and(|id| is_tunnel(&id))) else { return false };
        let before = parts[index].clone();
        if text(&before,"id").as_deref()==Some(target.as_str()) { return false; }
        let Some(mut after) = replace_tunnel_type(&before, &target) else { return true };
        let Tag::Compound(fields) = &mut after else { return true };
        if let Some(value) = before.get("p2p_frequency") { fields.insert("p2p_frequency".into(), value.clone()); }
        if let Some(value) = before.get("p2p_output") { fields.insert("p2p_output".into(), value.clone()); }
        let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&pos) else { return true };
        let Some(Tag::List(parts)) = machine.config.get_mut(&"parts".into()) else { return true };
        if parts.get(index) != Some(&before) { return true; }
        parts[index] = after.clone();
        self.world.modified.insert((pos.0 >> 4,pos.2 >> 4));
        self.world.block_entities.record_ae2_part(pos,before,false);
        self.world.block_entities.record_ae2_part(pos,after,true);
        let at = horizon_core::position::BlockPos::new(pos.0,pos.1,pos.2);
        let state = self.world.block(pos.0,pos.1,pos.2);
        self.with_level(|behaviours,level,ticks| behaviours.update_neighbors_at(level,ticks,at,behaviours.block(state)));
        true
    }
    pub(super) fn ae2_p2p_ports(&self) -> Vec<Port> {
        let mut ports = Vec::new();
        for (dimension, world) in self.ae2_loaded_worlds() {
            for (&local, be) in world.block_entities.iter() {
                let Some(address) = self.ae2_dimension_address(dimension, local) else {
                    continue;
                };
                let BlockEntity::Ae2(m) = be else {
                    continue;
                };
                if m.kind != MachineKind::CableBus {
                    continue;
                }
                let Some(Tag::List(parts)) = m.config.get(&"parts".into()) else {
                    continue;
                };
                for part in parts {
                    let Some(id) = text(part, "id").filter(|id| is_tunnel(id)) else {
                        continue;
                    };
                    let Some((face, direction)) = text(part, "face").and_then(|s| face(&s)) else {
                        continue;
                    };
                    ports.push(Port {
                        pos: address.network_pos(),
                        face,
                        direction,
                        id,
                        frequency: frequency(part),
                        output: part.get("p2p_output") == Some(&Tag::Byte(1)),
                        outer_mode: match part { Tag::Compound(data) => super::ae2_channels::node_mode(data,"outer"), _ => None },
                    });
                }
            }
        }
        ports.sort_by_key(|p| (p.pos, p.face));
        ports
    }
    pub(super) fn ae2_connect_me_p2p(&self, network: &mut Network) {
        let ports = self.ae2_p2p_ports();
        // The outer face belongs to the tunnel's isolated node; it must never
        // physically short the carrier cable into the transported network.
        for port in &ports {
            if let Some(cable) = network.nodes.get_mut(&port.pos) {
                cable.faces &= !(1 << port.face);
            }
        }
        let me = ports
            .iter()
            .filter(|p| p.id == "ae2:me_p2p_tunnel")
            .collect::<Vec<_>>();
        let virtuals = me
            .iter()
            .enumerate()
            .map(|(i, p)| {
                (
                    (p.pos, p.face),
                    Pos {
                        x: i32::MIN + (i as i32) * 2,
                        y: 0,
                        z: 0,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        for port in &me {
            let at = virtuals[&(port.pos, port.face)];
            let mut outer = Device::new(DeviceKind::DenseCable);
            outer.faces = 0;
            outer.cannot_carry_compressed = true;
            let identity = network.part_identities.get(&(port.pos, port.face)).copied()
                .map(|inner| ae2_network_core::NodeIdentity { outer: true, ..inner });
            outer.channel_mode_override = super::ae2_channels::merge_mode(port.outer_mode,
                identity.and_then(|id| self.ae2_committed_channels.forced_mode(id)));
            network.nodes.insert(at, outer);
            network.external_ports.insert((port.pos, port.face), at);
            if let Some(identity) = identity { network.node_identities.insert(at, identity); }
        }
        for port in &me {
            let at = virtuals[&(port.pos, port.face)];
            let Some(x) = port.pos.x.checked_add(port.direction[0]) else {
                continue;
            };
            let Some(y) = port.pos.y.checked_add(port.direction[1]) else {
                continue;
            };
            let Some(z) = port.pos.z.checked_add(port.direction[2]) else {
                continue;
            };
            let front = Pos { x, y, z };
            if let Some(other) = virtuals.get(&(front, port.face ^ 1)) {
                let _ = network.connect(at, *other);
            } else if network
                .nodes
                .get(&front)
                .is_some_and(|d| d.faces & (1 << (port.face ^ 1)) != 0)
            {
                let _ = network.connect(at, front);
            }
        }
        // Remote tunnel edges are physical scheduled connections, not a
        // projection of today's frequency/power snapshot.
        for (input, output) in self.ae2_me_runtime.borrow().edges() {
            let a = network.node_identities.iter().find_map(|(pos,id)| (*id == (ae2_network_core::NodeIdentity { outer: true, ..input })).then_some(*pos));
            let b = network.node_identities.iter().find_map(|(pos,id)| (*id == (ae2_network_core::NodeIdentity { outer: true, ..output })).then_some(*pos));
            if let (Some(a),Some(b)) = (a,b) { let _ = network.connect(a,b); }
        }
    }
    pub(super) fn ae2_try_p2p_card(&mut self, pos: WorldPos, side: &str) -> bool {
        let Some(held) = self.held().cloned() else { return false };
        if held.item.as_ref() != "ae2:memory_card" {
            return self.ae2_try_p2p_attunement(pos, side, &held);
        }
        if self.hand != super::Hand::Main || held.count != 1 { return false; }
        let mut card = held;
        let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get(&pos) else {
            return false;
        };
        let Some(Tag::List(parts)) = m.config.get(&"parts".into()) else {
            return false;
        };
        let Some(index) = parts.iter().position(|p| {
            text(p, "face").as_deref() == Some(side) && text(p, "id").is_some_and(|s| is_tunnel(&s))
        }) else {
            return false;
        };
        let part = parts[index].clone();
        let mut data = match card.data().and_then(|d| d.other("minecraft:custom_data")) {
            Some(Tag::Compound(c)) => c.clone(),
            None => Default::default(),
            _ => return true,
        };
        let save = self.keys.contains(&super::KeyCode::ShiftLeft);
        let mut updated = part.clone();
        let mut replaced = false;
        if save {
            let old = frequency(&part);
            let mut new = old;
            if old == 0 || part.get("p2p_output") == Some(&Tag::Byte(1)) {
                let used = self
                    .ae2_p2p_ports()
                    .into_iter()
                    .map(|p| p.frequency)
                    .collect::<BTreeSet<_>>();
                let start = self.world.random.next_int_bound(65535).unwrap_or(0) as u16 + 1;
                let Some(found) = (0..=65535u32)
                    .map(|i| start.wrapping_add(i as u16) as i16)
                    .find(|f| *f != 0 && !used.contains(f))
                else {
                    return true;
                };
                new = found;
            }
            let Tag::Compound(ref mut fields) = updated else {
                return true;
            };
            fields.insert("p2p_frequency".into(), Tag::Short(new));
            fields.insert("p2p_output".into(), Tag::Byte(0));
            if super::ae2_memory::clear(&mut card).is_err() {return true;}
            data.remove(&"ae2_memory_settings".into());
            let id=text(&part,"id").unwrap();
            data.insert(
                "ae2_p2p_card".into(),
                Tag::compound([
                    ("id",Tag::String(id.as_str().into())),
                    ("frequency", Tag::Short(new)),
                ]),
            );
            card.data_mut()
                .set_other("minecraft:custom_data", Some(Tag::Compound(data)));
            card.data_mut().set_other("ae2:exported_p2p_type",Some(Tag::String(id.as_str().into())));
            card.data_mut().set_other("ae2:exported_p2p_frequency",Some(Tag::Short(new)));
            card.data_mut().set_other("ae2:memory_card_colors",Some(card_colors(new)));
            let translation=format!("item.ae2.{}",id.strip_prefix("ae2:").unwrap_or(&id));
            card.data_mut().set_other("ae2:exported_settings_source",Some(Tag::compound([("translate",Tag::String(translation.as_str().into()))])));
            *self.held_mut() = Some(card);
        } else {
            let Some((id,new))=card_target(&card) else{return true;};
            if text(&part, "id").as_deref() != Some(id.as_str()) {
                let Some(replacement) = replace_tunnel_type(&part, &id) else { return true };
                updated = replacement;
                replaced = true;
            }
            let previous_frequency=frequency(&updated);
            let Tag::Compound(ref mut fields) = updated else {
                return true;
            };
            if let Some(new)=new {
                if previous_frequency != new {
                    fields.insert("p2p_frequency".into(), Tag::Short(new));
                    fields.insert("p2p_output".into(), Tag::Byte(1));
                }
            }
        }
        let mut changed = false;
        if let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&pos) {
            if let Some(Tag::List(parts)) = m.config.get_mut(&"parts".into()) {
                parts[index] = updated.clone();
                self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
                changed = true;
            }
        }
        if changed {
            if replaced {
                self.world.block_entities.record_ae2_part(pos, part, false);
                self.world.block_entities.record_ae2_part(pos, updated, true);
            } else {
                self.world.block_entities.record_ae2_part_change(pos, part, updated);
            }
            if self.ae2_has_anchor_owners() {
                if let Err(error)=self.ae2_apply_anchor_callbacks(){log::debug!("AE2 anchor callbacks deferred: {error}");}
            }
        }
        true
    }
}

#[cfg(test)]
mod replacement_tests {
    use super::*;
    use horizon_client::items::ItemStack;
    #[test]
    fn memory_card_reads_registered_p2p_components_before_legacy_data() {
        let mut card=ItemStack::new("ae2:memory_card",1);
        card.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("ae2_p2p_card",Tag::compound([
            ("id",Tag::String("ae2:me_p2p_tunnel".into())),("frequency",Tag::Short(11))
        ]))])));
        assert_eq!(card_target(&card),Some(("ae2:me_p2p_tunnel".into(),Some(11))));
        card.data_mut().set_other("ae2:exported_p2p_type",Some(Tag::String("ae2:item_p2p_tunnel".into())));
        card.data_mut().set_other("ae2:exported_p2p_frequency",Some(Tag::Short(-1234)));
        assert_eq!(card_target(&card),Some(("ae2:item_p2p_tunnel".into(),Some(-1234))));
        card.data_mut().set_other("ae2:exported_p2p_frequency",None);
        assert_eq!(card_target(&card),Some(("ae2:item_p2p_tunnel".into(),None)));
        card.data_mut().set_other("ae2:exported_p2p_frequency",Some(Tag::Int(7)));
        assert_eq!(card_target(&card),None);
        assert_eq!(card_colors(0x1234),Tag::List(["light_gray","light_gray","gray","gray","black","black","lime","lime"].into_iter().map(|s|Tag::String(s.into())).collect()));
    }
    #[test]
    fn type_change_replaces_physical_part_and_clears_old_nodes() {
        let stack=ItemStack::new("ae2:me_p2p_tunnel",1);
        let old=Tag::compound([("id",Tag::String("ae2:me_p2p_tunnel".into())),("face",Tag::String("north".into())),("stack",horizon_client::save::item_to_nbt(&stack)),("ae2_part_identity",Tag::IntArray(vec![1,2,3,4])),("gn",Tag::compound([("cm",Tag::String("X3".into()))])),("outer",Tag::compound([("cm",Tag::String("X4".into()))]))]);
        let new=replace_tunnel_type(&old,"ae2:item_p2p_tunnel").unwrap();
        assert_eq!(text(&new,"id").as_deref(),Some("ae2:item_p2p_tunnel"));
        assert_eq!(horizon_client::save::item_from_nbt(new.get("stack").unwrap()).unwrap().item.as_ref(),"ae2:item_p2p_tunnel");
        assert_ne!(super::super::ae2_channels::part_identity(&old),super::super::ae2_channels::part_identity(&new));
        assert!(new.get("gn").is_none() && new.get("outer").is_none());
        assert!(replace_tunnel_type(&Tag::compound([("id",Tag::String("ae2:me_p2p_tunnel".into()))]),"ae2:item_p2p_tunnel").is_none());
    }
}
