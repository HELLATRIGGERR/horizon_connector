//! Physical multipart cable center and color. AE2 19.2.18 CablePart and
//! InWorldGridNode: dye colors isolate different non-fluix colors; dense
//! covered/smart cables carry 32 channels, ordinary cables carry eight.
use super::Game;
use ae2_network_core::{CableColor, DeviceKind};
use horizon_client::{
    ae2_world::{Machine, MachineKind},
    containers::BlockEntity,
    items::ItemStack,
};
use horizon_world::nbt::Tag;

pub(super) fn cable_item(id: &str) -> Option<(DeviceKind, CableColor)> {
    let id = id.strip_prefix("ae2:")?;
    let (color, kind) = id.split_once('_')?;
    // light_gray/light_blue have two-word colors.
    let (color, kind) = if color == "light" {
        let (suffix, rest) = kind.split_once('_')?;
        (
            match suffix {
                "gray" => "light_gray",
                "blue" => "light_blue",
                _ => return None,
            },
            rest,
        )
    } else {
        (color, kind)
    };
    let color = match color {
        "fluix" => CableColor::Transparent,
        "white" => CableColor::White,
        "light_gray" => CableColor::LightGray,
        "gray" => CableColor::Gray,
        "black" => CableColor::Black,
        "lime" => CableColor::Lime,
        "yellow" => CableColor::Yellow,
        "orange" => CableColor::Orange,
        "brown" => CableColor::Brown,
        "red" => CableColor::Red,
        "pink" => CableColor::Pink,
        "magenta" => CableColor::Magenta,
        "purple" => CableColor::Purple,
        "blue" => CableColor::Blue,
        "light_blue" => CableColor::LightBlue,
        "cyan" => CableColor::Cyan,
        "green" => CableColor::Green,
        _ => return None,
    };
    let kind = match kind {
        "glass_cable" => DeviceKind::GlassCable,
        "covered_cable" => DeviceKind::CoveredCable,
        "smart_cable" => DeviceKind::SmartCable,
        "covered_dense_cable" => DeviceKind::DenseCoveredCable,
        "smart_dense_cable" => DeviceKind::DenseSmartCable,
        _ => return None,
    };
    Some((kind, color))
}
/// Legacy prototype cable buses had no stored center; retain transparent glass
/// for old worlds. Invalid explicitly saved centers fail closed.
pub(super) fn cable_device(machine: &Machine) -> Option<(DeviceKind, CableColor)> {
    match machine.config.get(&"cable".into()) {
        None => Some((DeviceKind::GlassCable, CableColor::Transparent)),
        Some(tag) if tag.get("count") == Some(&Tag::Int(1)) => {
            let item = horizon_client::save::item_from_nbt(tag)?;
            cable_item(&item.item)
        }
        _ => None,
    }
}
impl Game<'_> {
    /// Authoritative use-on-block hook. `face` is outward hit normal.
    /// Handles a real cable item only, on an empty adjacent block or an existing
    /// centerless multipart host; components are kept in the block entity.
    pub(super) fn ae2_try_place_cable(&mut self, hit: [i32; 3], face: [i32; 3]) -> bool {
        let Some(held) = self.held().cloned() else {
            return false;
        };
        if cable_item(&held.item).is_none() || held.count == 0 {
            return false;
        }
        let target = if matches!(self.world.block_entities.get(&(hit[0],hit[1],hit[2])),Some(BlockEntity::Ae2(m)) if m.kind==MachineKind::CableBus && !m.config.contains_key(&"cable".into()))
        {
            hit
        } else {
            [hit[0] + face[0], hit[1] + face[1], hit[2] + face[2]]
        };
        let pos = (target[0], target[1], target[2]);
        let existing = matches!(self.world.block_entities.get(&pos),Some(BlockEntity::Ae2(m)) if m.kind==MachineKind::CableBus);
        if !existing {
            let state = self.world.block(target[0], target[1], target[2]);
            let name = &self.res.facts().get(state).name;
            if !matches!(
                name.as_ref(),
                "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
            ) {
                return false;
            }
            if !self
                .set_state_named(target, "ae2:cable_bus", &[])
                .unwrap_or(false)
                || !self.ae2_ensure_machine(pos)
            {
                return false;
            }
        }
        let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&pos) else {
            return false;
        };
        if m.kind != MachineKind::CableBus || m.config.contains_key(&"cable".into()) {
            return false;
        }
        let one = ItemStack {
            count: 1,
            ..held.clone()
        };
        let cable = horizon_client::save::item_to_nbt(&one);
        m.config.insert("cable".into(), cable.clone());
        self.world.block_entities.record_ae2_center_change(pos, None, Some(cable));
        if !self.creative {
            let remaining = held.count - 1;
            *self.held_mut() = (remaining > 0).then_some(ItemStack {
                count: remaining,
                ..held
            });
        }
        self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registered_cable_names_keep_color_and_dense_capacity() {
        assert_eq!(
            cable_item("ae2:fluix_glass_cable"),
            Some((DeviceKind::GlassCable, CableColor::Transparent))
        );
        assert_eq!(
            cable_item("ae2:light_blue_smart_dense_cable"),
            Some((DeviceKind::DenseSmartCable, CableColor::LightBlue))
        );
        assert_eq!(
            cable_item("ae2:red_covered_cable"),
            Some((DeviceKind::CoveredCable, CableColor::Red))
        );
        assert_eq!(cable_item("ae2:bogus_glass_cable"), None);
    }
}

impl Game<'_> {
    /// Toggle bus has an isolated outer node exposed on its face and a
    /// redstone-controlled internal connection to the cable's main node.
    pub(super) fn ae2_connect_toggle_buses(&self, network: &mut ae2_network_core::Network) {
        use ae2_network_core::{Device, Pos};
        use std::collections::BTreeMap;
        let mut ports = Vec::new();
        for (dim, world) in self.ae2_loaded_worlds() {
            for (&local, be) in world.block_entities.iter() {
                let Some(address) = self.ae2_dimension_address(dim, local) else {
                    continue;
                };
                let pos = address.network_pos();
                let BlockEntity::Ae2(m) = be else { continue };
                if m.kind != MachineKind::CableBus {
                    continue;
                }
                let Some(Tag::List(parts)) = m.config.get(&"parts".into()) else {
                    continue;
                };
                for part in parts {
                    let Some(Tag::String(id)) = part.get("id") else {
                        continue;
                    };
                    let Ok(id) = id.to_string() else { continue };
                    if !matches!(id.as_str(), "ae2:toggle_bus" | "ae2:inverted_toggle_bus") {
                        continue;
                    }
                    let Some(Tag::String(face)) = part.get("face") else {
                        continue;
                    };
                    let Ok(face) = face.to_string() else { continue };
                    let Some((index, delta)) = (match face.as_str() {
                        "east" => Some((0, [1, 0, 0])),
                        "west" => Some((1, [-1, 0, 0])),
                        "up" => Some((2, [0, 1, 0])),
                        "down" => Some((3, [0, -1, 0])),
                        "south" => Some((4, [0, 0, 1])),
                        "north" => Some((5, [0, 0, -1])),
                        _ => None,
                    }) else {
                        continue;
                    };
                    let signal = part.get("toggle_signal") == Some(&Tag::Byte(1));
                    ports.push((
                        pos,
                        index,
                        delta,
                        if id == "ae2:toggle_bus" {
                            signal
                        } else {
                            !signal
                        },
                        match part { Tag::Compound(data) => super::ae2_channels::node_mode(data,"outer"), _ => None },
                    ));
                }
            }
        }
        ports.sort_by_key(|p| (p.0, p.1));
        let mut virtuals = BTreeMap::new();
        for (index, (pos, side, _, _, mode)) in ports.iter().enumerate() {
            let Some(x) = i32::try_from(index)
                .ok()
                .and_then(|i| i.checked_mul(2))
                .and_then(|i| i32::MIN.checked_add(i))
                .and_then(|i| i.checked_add(1))
            else {
                break;
            };
            let at = Pos { x, y: 0, z: 0 };
            virtuals.insert((*pos, *side), at);
            if let Some(cable) = network.nodes.get_mut(pos) {
                cable.faces &= !(1 << side);
            }
            let mut outer = Device::new(DeviceKind::GlassCable);
            outer.faces = 0;
            let identity = network.part_identities.get(&(*pos, *side)).copied()
                .map(|inner| ae2_network_core::NodeIdentity { outer: true, ..inner });
            outer.channel_mode_override = super::ae2_channels::merge_mode(*mode,
                identity.and_then(|id| self.ae2_committed_channels.forced_mode(id)));
            network.nodes.insert(at, outer);
            network.external_ports.insert((*pos, *side), at);
            if let Some(identity) = identity { network.node_identities.insert(at, identity); }
        }
        for (pos, side, delta, enabled, _) in ports {
            let Some(&at) = virtuals.get(&(pos, side)) else {
                continue;
            };
            let Some(x) = pos.x.checked_add(delta[0]) else {
                continue;
            };
            let Some(y) = pos.y.checked_add(delta[1]) else {
                continue;
            };
            let Some(z) = pos.z.checked_add(delta[2]) else {
                continue;
            };
            let front = Pos { x, y, z };
            if let Some(&other) = virtuals.get(&(front, side ^ 1)) {
                let _ = network.connect(at, other);
            } else if network
                .nodes
                .get(&front)
                .is_some_and(|d| d.faces & (1 << (side ^ 1)) != 0)
            {
                let _ = network.connect(at, front);
            }
            if enabled {
                let inner = network.part_endpoints.get(&(pos, side)).copied().unwrap_or(pos);
                let _ = network.connect(inner, at);
            }
        }
    }
}

impl Game<'_> {
    /// Physical facade item, with a source component or legacy native texture.
    pub(super) fn ae2_try_facade(&mut self, hit: [i32; 3], face: [i32; 3]) -> bool {
        let side = match face {
            [0, -1, 0] => "down",
            [0, 1, 0] => "up",
            [0, 0, -1] => "north",
            [0, 0, 1] => "south",
            [-1, 0, 0] => "west",
            [1, 0, 0] => "east",
            _ => return false,
        };
        let Some(held) = self.held().cloned() else {
            return false;
        };
        if held.item.as_ref() != "ae2:facade" || held.count == 0 {
            return false;
        }
        let Some(id)=horizon_client::item_tooltip::ae2_facade_texture(&held) else{return false;};
        // FacadeItem.createPartFromItemStack accepts any non-empty registered
        // item. Block.byItem maps non-block items to glass at placement time;
        // the crafting recipe's whitelist is not a placement restriction.
        if id == "minecraft:air" { return false; }
        let Some(material) = self.res.items().get(&id) else { return false; };
        let Some(block) = material.block
            .filter(|id| !self.res.facts().is_air(*id))
            .or_else(|| self.pack.assets.registry.default_state("minecraft:glass"))
        else { return false; };
        let Some(facade_state) = self.pack.assets.registry.state(block).map(|state|state.to_nbt()) else {return false;};
        // FacadePart.getItemStack reconstructs a fresh facade from the placed
        // block's item. Preserve that exact physical return, including glass
        // for non-block material and no unrelated held-item components.
        let material_id = self.res.facts().get(block).item.clone();
        let pos = (hit[0], hit[1], hit[2]);
        let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&pos) else {
            return false;
        };
        if machine.kind != MachineKind::CableBus || cable_device(machine).is_none()
            || (machine.config.get(&"ae2_centerless".into())==Some(&Tag::Byte(1)) && !machine.config.contains_key(&"cable".into())) {
            return false;
        }
        let facades = machine
            .config
            .entry("facades".into())
            .or_insert_with(|| Tag::List(Vec::new()));
        let Tag::List(facades) = facades else {
            return false;
        };
        if facades.len() >= 6
            || facades
                .iter()
                .any(|f| f.get("face") == Some(&Tag::String(side.into())))
        {
            return false;
        }
        let mut one = ItemStack::new("ae2:facade", 1);
        one.data_mut().set_other("ae2:facade_item", Some(Tag::String(material_id.as_ref().into())));
        facades.push(Tag::compound([
            ("face", Tag::String(side.into())),
            ("stack", horizon_client::save::item_to_nbt(&one)),
            ("state", facade_state),
        ]));
        if !self.creative {
            *self.held_mut() = (held.count > 1).then_some(ItemStack {
                count: held.count - 1,
                ..held
            });
        }
        self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
        true
    }
    /// FacadePart.handleInteraction: right-click cycles the selected block
    /// property; left-click selects the next property on the held wrench.
    pub(super) fn ae2_try_facade_cycle(
        &mut self,
        hit: [i32; 3],
        face: [i32; 3],
        cycle_state: bool,
    ) -> bool {
        let side = match face {
            [0, -1, 0] => "down",
            [0, 1, 0] => "up",
            [0, 0, -1] => "north",
            [0, 0, 1] => "south",
            [-1, 0, 0] => "west",
            [1, 0, 0] => "east",
            _ => return false,
        };
        if self.keys.contains(&super::KeyCode::ShiftLeft)
            || !self
                .held()
                .is_some_and(|item| self.ae2_wrench_can_rotate(item))
        {
            return false;
        }
        let pos = (hit[0], hit[1], hit[2]);
        let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get(&pos) else {
            return false;
        };
        if machine.kind != MachineKind::CableBus {
            return false;
        }
        let Some(Tag::List(facades)) = machine.config.get(&"facades".into()) else {
            return false;
        };
        let mut matches = facades
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.get("face") == Some(&Tag::String(side.into())));
        let Some((index, entry)) = matches.next() else {
            return false;
        };
        if matches.next().is_some() {
            return true;
        }
        let entry = entry.clone();
        let Some(stack) = entry
            .get("stack")
            .and_then(horizon_client::save::item_from_nbt)
        else {
            return true;
        };
        let Some(block) = horizon_client::item_tooltip::ae2_facade_texture(&stack) else {
            return true;
        };
        let registry = &self.pack.assets.registry;
        let Some(default) = self.res.items().get(&block)
            .and_then(|item| item.block)
            .filter(|id| !self.res.facts().is_air(*id))
            .or_else(|| registry.default_state("minecraft:glass"))
            .and_then(|id| registry.state(id))
            .cloned()
        else {
            return true;
        };
        let requested = entry
            .get("state")
            .and_then(|tag| horizon_world::chunk::BlockState::from_nbt(tag).ok())
            .unwrap_or_else(|| default.clone());
        if requested.name != default.name {
            return true;
        }
        let Some(state) = registry
            .resolve(&requested)
            .ok()
            .and_then(|id| registry.state(id))
            .cloned()
        else {
            return true;
        };
        let properties: Vec<_> = state.properties.keys().cloned().collect();
        let Some(first) = properties.first() else {
            return false;
        };
        let selected = self
            .held()
            .and_then(|item| item.data())
            .and_then(|data| data.other("ae2:facade_cycle_property"))
            .and_then(|tag| {
                if let Tag::String(value) = tag {
                    value.to_string().ok()
                } else {
                    None
                }
            });
        let property = selected
            .filter(|key| state.properties.contains_key(key))
            .unwrap_or_else(|| first.clone());
        if !cycle_state {
            let current = properties
                .iter()
                .position(|key| *key == property)
                .unwrap_or(0);
            let next = &properties[(current + 1) % properties.len()];
            let Some(mut wrench) = self.held().cloned() else {
                return true;
            };
            wrench.data_mut().set_other(
                "ae2:facade_cycle_property",
                if next == first {
                    None
                } else {
                    Some(Tag::String(next.as_str().into()))
                },
            );
            horizon_client::ae2_stack_identity::synchronize_tracked(&wrench);
            *self.held_mut() = Some(wrench);
            self.system_message(horizon_client::text::Text::literal(format!(
                "Facade property: {next}"
            )));
            return true;
        }
        let mut values = Vec::new();
        for i in 0..registry.len() {
            let Some(candidate) = registry.state(horizon_world::registry::BlockStateId(i as u32))
            else {
                continue;
            };
            if candidate.name != state.name
                || candidate.properties.iter().any(|(key, value)| {
                    key != &property && state.properties.get(key) != Some(value)
                })
            {
                continue;
            }
            if let Some(value) = candidate.properties.get(&property) {
                if !values.contains(value) {
                    values.push(value.clone());
                }
            }
        }
        let Some(current) = state
            .properties
            .get(&property)
            .and_then(|value| values.iter().position(|candidate| candidate == value))
        else {
            return true;
        };
        let mut next = state.clone();
        next.properties.insert(
            property.clone(),
            values[(current + 1) % values.len()].clone(),
        );
        if registry.id(&next).is_none() {
            return true;
        }
        let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&pos) else {
            return true;
        };
        let Some(Tag::List(facades)) = machine.config.get_mut(&"facades".into()) else {
            return true;
        };
        if facades.get(index) != Some(&entry) {
            return true;
        }
        let Some(Tag::Compound(target)) = facades.get_mut(index) else {
            return true;
        };
        target.insert("state".into(), next.to_nbt());
        self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
        if next.properties.get(&property) == default.properties.get(&property) {
            self.system_message(horizon_client::text::Text::literal(format!(
                "Facade property wrapped: {property}"
            )));
        }
        true
    }
    /// Remove the physically stored facade using a quartz wrench while sneaking.
    /// Any inventory overflow becomes the actual facade item in the world.
    pub(super) fn ae2_try_remove_facade(&mut self, hit: [i32; 3], face: [i32; 3]) -> bool {
        if self.hand != super::Hand::Main { return false; }
        let side = match face {
            [0, -1, 0] => "down",
            [0, 1, 0] => "up",
            [0, 0, -1] => "north",
            [0, 0, 1] => "south",
            [-1, 0, 0] => "west",
            [1, 0, 0] => "east",
            _ => return false,
        };
        if !self.keys.contains(&super::KeyCode::ShiftLeft)
            || !self.held().is_some_and(|s| self.ae2_wrench_can_disassemble(s))
        {
            return false;
        }
        let pos = (hit[0], hit[1], hit[2]);
        let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get(&pos) else {
            return false;
        };
        let Some(Tag::List(facades)) = machine.config.get(&"facades".into()) else {
            return false;
        };
        let Some(index) = facades
            .iter()
            .position(|f| f.get("face") == Some(&Tag::String(side.into())))
        else {
            return false;
        };
        let Some(tag) = facades[index].get("stack") else {
            return false;
        };
        let Some(item) = horizon_client::save::item_from_nbt(tag)
            .filter(|s| s.count == 1 && s.item.as_ref() == "ae2:facade")
        else {
            return false;
        };
        let before = self.inventory.clone();
        let overflow = self.give(item);
        let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&pos) else {
            self.inventory = before;
            return true;
        };
        let Some(Tag::List(facades)) = machine.config.get_mut(&"facades".into()) else {
            self.inventory = before;
            return true;
        };
        facades.remove(index);
        self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
        if let Some(stack) = overflow {
            self.pop_resource(hit, stack);
        }
        true
    }
}

/// A multipart owner is a cable address plus a distinct physical face. It is
/// not interchangeable with the host block address: six parts can own six
/// independent inventories and configuration records on the same cable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PartAddress {
    pub host: super::ae2_global::GridAddress,
    pub face: u8,
}
fn part_face(face: u8) -> Option<&'static str> {
    Some(match face {
        0 => "east",
        1 => "west",
        2 => "up",
        3 => "down",
        4 => "south",
        5 => "north",
        _ => return None,
    })
}
fn part_machine_kind(id: &str) -> Option<MachineKind> {
    match id {
        "ae2:cable_interface" => Some(MachineKind::Interface),
        "ae2:cable_pattern_provider" => Some(MachineKind::PatternProvider),
        "ae2:terminal" => Some(MachineKind::Terminal),
        "ae2:crafting_terminal" => Some(MachineKind::CraftingTerminal),
        "ae2:pattern_encoding_terminal" => Some(MachineKind::PatternEncodingTerminal),
        _ => None,
    }
}
pub(super) fn strict_part_items(tag: Option<&Tag>, size: usize) -> Result<Vec<Option<ItemStack>>, String> {
    let Some(tag) = tag else {
        return Ok(vec![None; size]);
    };
    let Tag::List(entries) = tag else {
        return Err("invalid part inventory".into());
    };
    let mut seen = vec![false; size];
    for entry in entries {
        let Some(Tag::Byte(slot)) = entry.get("Slot") else {
            return Err("invalid part slot".into());
        };
        let index = *slot as usize;
        if index >= size
            || seen[index]
            || !matches!(entry.get("count"), Some(Tag::Int(n)) if *n > 0)
            || horizon_client::save::item_from_nbt(entry).is_none()
        {
            return Err("invalid part item".into());
        }
        seen[index] = true;
    }
    Ok(horizon_client::save::decode_items(Some(tag), size))
}
/// Physical view cells belong to the terminal face, not the virtual grid.
pub(super) fn ae2_terminal_view_cells(machine: &Machine) -> Result<Vec<Option<ItemStack>>, String> {
    if !matches!(machine.kind, MachineKind::Terminal | MachineKind::CraftingTerminal | MachineKind::PatternEncodingTerminal) {
        return Err("not a storage terminal".into());
    }
    decode_terminal_view_cells(machine.config.get(&"ae2_view_cells".into()))
}

pub(super) fn decode_terminal_view_cells(tag: Option<&Tag>) -> Result<Vec<Option<ItemStack>>, String> {
    let slots = match tag {
        None => vec![None; 5],
        Some(Tag::List(entries)) => horizon_client::ae2_upgrade_inventory::decode(entries, 5)?,
        _ => return Err("invalid terminal view cells".into()),
    };
    if slots.iter().flatten().any(|stack| stack.count != 1 || stack.item.as_ref() != "ae2:view_cell") {
        return Err("invalid terminal view cell".into());
    }
    Ok(slots)
}

pub(super) fn ae2_set_terminal_view_cells(machine: &mut Machine, slots: &[Option<ItemStack>]) -> Result<(), String> {
    if !matches!(machine.kind, MachineKind::Terminal | MachineKind::CraftingTerminal | MachineKind::PatternEncodingTerminal)
        || slots.len() != 5 || slots.iter().flatten().any(|stack| stack.count != 1 || stack.item.as_ref() != "ae2:view_cell") {
        return Err("invalid terminal view cells".into());
    }
    let encoded = horizon_client::ae2_upgrade_inventory::encode(slots)?;
    machine.config.insert("ae2_view_cells".into(), Tag::List(encoded));
    Ok(())
}

impl Game<'_> {
    /// Read one interface/provider face into an owned snapshot; all mutation
    /// must be committed back to the same PartAddress, never the cable slots.
    pub(super) fn ae2_part_machine(&self, address: PartAddress) -> Result<Option<Machine>, String> {
        let face = part_face(address.face).ok_or("invalid part face")?;
        let Some(BlockEntity::Ae2(host)) = self.ae2_block_entity(address.host) else {
            return Ok(None);
        };
        if host.kind != MachineKind::CableBus {
            return Ok(None);
        }
        let Some(Tag::List(parts)) = host.config.get(&"parts".into()) else {
            return Ok(None);
        };
        let mut matches = parts
            .iter()
            .filter(|p| p.get("face") == Some(&Tag::String(face.into())));
        let Some(part) = matches.next() else {
            return Ok(None);
        };
        if matches.next().is_some() {
            return Err("duplicate part face".into());
        }
        let Some(Tag::String(id)) = part.get("id") else {
            return Err("invalid part id".into());
        };
        let id = id.to_string().map_err(|_| "invalid part id")?;
        let Some(kind) = part_machine_kind(&id) else {
            return Ok(None);
        };
        let mut machine = Machine::new(kind);
        if let Some(data) = part.get("machine") {
            let Tag::Compound(data) = data else {
                return Err("invalid part machine".into());
            };
            machine.slots = strict_part_items(data.get(&"slots".into()), kind.physical_slots())?;
            machine.upgrades =
                strict_part_items(data.get(&"upgrades".into()), kind.upgrade_slots())?;
            machine.config = match data.get(&"config".into()) {
                None => Default::default(),
                Some(Tag::Compound(config)) => config.clone(),
                _ => return Err("invalid part configuration".into()),
            };
        }
        Ok(Some(machine))
    }
    pub(super) fn ae2_store_part_machine(
        &mut self,
        address: PartAddress,
        machine: &Machine,
    ) -> Result<(), String> {
        let previous = self
            .ae2_part_machine(address)?
            .ok_or("missing machine part")?;
        if previous.kind != machine.kind
            || machine.slots.len() != machine.kind.physical_slots()
            || machine.upgrades.len() != machine.kind.upgrade_slots()
            || machine.slots.iter().flatten().any(|stack| stack.count == 0)
            || machine
                .upgrades
                .iter()
                .flatten()
                .any(|stack| stack.count != 1)
        {
            return Err("part machine type or size changed".into());
        }
        let face = part_face(address.face).ok_or("invalid part face")?;
        let Some(BlockEntity::Ae2(host)) = self.ae2_block_entity_mut(address.host) else {
            return Err("missing cable host".into());
        };
        let Some(Tag::List(parts)) = host.config.get_mut(&"parts".into()) else {
            return Err("missing part list".into());
        };
        let Some(Tag::Compound(part)) = parts
            .iter_mut()
            .find(|p| p.get("face") == Some(&Tag::String(face.into())))
        else {
            return Err("missing part".into());
        };
        part.insert(
            "machine".into(),
            Tag::compound([
                ("slots", horizon_client::save::encode_items(&machine.slots)),
                (
                    "upgrades",
                    horizon_client::save::encode_items(&machine.upgrades),
                ),
                ("config", Tag::Compound(machine.config.clone())),
            ]),
        );
        self.ae2_mark_modified(address.host);
        Ok(())
    }
}

#[cfg(test)]
mod machine_owner_tests {
    use super::*;
    #[test]
    fn terminal_view_cells_roundtrip_and_reject_invalid_physical_slots() {
        let mut cell = ItemStack::new("ae2:view_cell", 1);
        cell.data_mut().custom_name = Some("exact physical cell".into());
        for kind in [MachineKind::Terminal, MachineKind::CraftingTerminal, MachineKind::PatternEncodingTerminal] {
            let mut machine = Machine::new(kind);
            assert_eq!(ae2_terminal_view_cells(&machine).unwrap(), vec![None; 5]);
            let slots = vec![None, Some(cell.clone()), None, None, None];
            ae2_set_terminal_view_cells(&mut machine, &slots).unwrap();
            assert_eq!(ae2_terminal_view_cells(&machine).unwrap(), slots);
            let Some(Tag::List(entries)) = machine.config.get_mut(&"ae2_view_cells".into()) else { panic!("missing encoded cells") };
            entries.push(entries[0].clone());
            assert!(ae2_terminal_view_cells(&machine).is_err());
        }
        let mut machine = Machine::new(MachineKind::Terminal);
        assert!(ae2_set_terminal_view_cells(&mut machine, &[Some(ItemStack::new("minecraft:diamond", 1)), None, None, None, None]).is_err());
    }
    #[test]
    fn part_inventory_rejects_duplicate_slots_and_roundtrips_components() {
        let mut a = ItemStack::new("minecraft:diamond", 2);
        a.data_mut().custom_name = Some("Owned by east face".into());
        let tag = horizon_client::save::encode_items(&[Some(a.clone()), None]);
        assert_eq!(
            strict_part_items(Some(&tag), 2).unwrap()[0].as_ref(),
            Some(&a)
        );
        let Tag::List(mut entries) = tag else {
            panic!("expected list")
        };
        entries.push(entries[0].clone());
        assert!(strict_part_items(Some(&Tag::List(entries)), 2).is_err());
    }
}

impl Game<'_> {
    /// QuartzFiberPart's inner and outer CANNOT_CARRY nodes share energy only.
    /// Synthetic outer nodes use a namespace disjoint from P2P/toggle nodes.
    pub(super) fn ae2_connect_quartz_fibers(&self, network: &mut ae2_network_core::Network) {
        use ae2_network_core::{Device, DeviceKind, Pos};
        use std::collections::BTreeMap;
        let mut ports = Vec::new();
        for (dim, world) in self.ae2_loaded_worlds() {
            for (&local, be) in &world.block_entities {
                let Some(address) = self.ae2_dimension_address(dim, local) else {
                    continue;
                };
                let BlockEntity::Ae2(machine) = be else {
                    continue;
                };
                if machine.kind != MachineKind::CableBus {
                    continue;
                }
                let Some(Tag::List(parts)) = machine.config.get(&"parts".into()) else {
                    continue;
                };
                for part in parts {
                    if part.get("id") != Some(&Tag::String("ae2:quartz_fiber".into())) {
                        continue;
                    }
                    let Some(Tag::String(face)) = part.get("face") else {
                        continue;
                    };
                    let Ok(face) = face.to_string() else { continue };
                    let Some((side, delta)) = (match face.as_str() {
                        "east" => Some((0, [1, 0, 0])),
                        "west" => Some((1, [-1, 0, 0])),
                        "up" => Some((2, [0, 1, 0])),
                        "down" => Some((3, [0, -1, 0])),
                        "south" => Some((4, [0, 0, 1])),
                        "north" => Some((5, [0, 0, -1])),
                        _ => None,
                    }) else {
                        continue;
                    };
                    let mode = match part { Tag::Compound(data) => super::ae2_channels::node_mode(data,"outer"), _ => None };
                    ports.push((address.network_pos(), side, delta, mode));
                }
            }
        }
        ports.sort_by_key(|p| (p.0, p.1));
        let mut outer = BTreeMap::new();
        for (index, (pos, side, _, mode)) in ports.iter().enumerate() {
            let Some(x) = i32::try_from(index)
                .ok()
                .and_then(|i| i.checked_mul(2))
                .and_then(|i| i32::MIN.checked_add(i))
            else {
                break;
            };
            let virtual_pos = Pos { x, y: 1, z: 1 };
            if let Some(cable) = network.nodes.get_mut(pos) {
                cable.faces &= !(1 << side);
            } else if !network.part_endpoints.contains_key(&(*pos, *side)) {
                continue;
            }
            let mut node = Device::new(DeviceKind::PoweredMachine);
            node.faces = 0;
            let identity = network.part_identities.get(&(*pos, *side)).copied()
                .map(|inner| ae2_network_core::NodeIdentity { outer: true, ..inner });
            node.channel_mode_override = super::ae2_channels::merge_mode(*mode,
                identity.and_then(|id| self.ae2_committed_channels.forced_mode(id)));
            network.nodes.insert(virtual_pos, node);
            network.external_ports.insert((*pos, *side), virtual_pos);
            if let Some(identity) = identity { network.node_identities.insert(virtual_pos, identity); }
            outer.insert((*pos, *side), virtual_pos);
            let inner = network.part_endpoints.get(&(*pos, *side)).copied().unwrap_or(*pos);
            let _ = network.connect_energy(inner, virtual_pos);
        }
        for (pos, side, delta, _) in ports {
            let Some(&virtual_pos) = outer.get(&(pos, side)) else {
                continue;
            };
            let Some(x) = pos.x.checked_add(delta[0]) else {
                continue;
            };
            let Some(y) = pos.y.checked_add(delta[1]) else {
                continue;
            };
            let Some(z) = pos.z.checked_add(delta[2]) else {
                continue;
            };
            let front = Pos { x, y, z };
            if let Some(&other) = outer.get(&(front, side ^ 1)) {
                let _ = network.connect(virtual_pos, other);
            } else if network
                .nodes
                .get(&front)
                .is_some_and(|node| node.faces & (1 << (side ^ 1)) != 0)
            {
                let _ = network.connect(virtual_pos, front);
            }
        }
    }
}
