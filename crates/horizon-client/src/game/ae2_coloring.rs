//! ColorApplicatorItem / BlockRecolorer, AE2 19.2.18, LGPL-3.0-or-later.
use super::{Game, GuestUse, Hit, KeyCode, ae2_lab};
use ae2_cell_core::{Action, ItemKey};
use horizon_client::{
    ae2_powered_items as power, ae2_world::MachineKind, containers::BlockEntity,
    first_person::UseResult, items::ItemStack, mobs::Species,
};
use horizon_world::{
    chunk::BlockState,
    nbt::{Compound, Tag},
};
const SELECTED: &str = "ae2:selected_color";
fn custom(stack: &ItemStack) -> Result<Compound, String> {
    match stack.data().and_then(|d| d.other("minecraft:custom_data")) {
        None => Ok(Default::default()),
        Some(Tag::Compound(c)) => Ok(c.clone()),
        _ => Err("invalid applicator custom data".into()),
    }
}
fn selected(stack: &ItemStack) -> Option<u8> {
    power::selected_color(stack)
}
fn set_selected(stack: &mut ItemStack, color: Option<u8>) -> Result<(), String> {
    let mut data = custom(stack)?;
    let source=color.filter(|n|*n<17).map(|n|Tag::String(power::COLORS[n as usize].into()));
    if let Some(color) = color.filter(|n| *n < 17) {
        data.insert(
            SELECTED.into(),
            Tag::String(power::COLORS[color as usize].into()),
        );
    } else {
        data.remove(&SELECTED.into());
    }
    stack
        .data_mut()
        .set_other("minecraft:custom_data", Some(Tag::Compound(data)));
    stack.data_mut().set_other(SELECTED, source);
    Ok(())
}
fn recolored(id: &str, color: u8) -> Option<String> {
    let color = power::COLORS.get(color as usize)?;
    for (suffix, uncolored) in [
        ("stained_glass", Some("glass")),
        ("stained_glass_pane", Some("glass_pane")),
        ("wool", Some("white_wool")),
        ("banner", Some("white_banner")),
        ("wall_banner", Some("white_wall_banner")),
        ("carpet", Some("white_carpet")),
        ("terracotta", Some("terracotta")),
        ("glazed_terracotta", None),
        ("concrete", None),
    ] {
        let is_plain = uncolored.is_some_and(|plain| id == format!("minecraft:{plain}"));
        if is_plain
            || power::COLORS
                .iter()
                .take(16)
                .any(|prefix| id == format!("minecraft:{prefix}_{suffix}"))
        {
            let next = if *color == "fluix" {
                format!("minecraft:{}", uncolored?)
            } else {
                format!("minecraft:{color}_{suffix}")
            };
            return (next != id).then_some(next);
        }
    }
    None
}
impl Game<'_> {
    /// Source Shift+wheel cycling prefers the actual main-hand tool, then offhand.
    pub(super) fn ae2_item_wheel(&mut self, delta: f32) -> bool {
        if !delta.is_finite() || delta == 0.0 || !self.keys.contains(&KeyCode::ShiftLeft) {
            return false;
        }
        let hand = if self.inventory[self.selected]
            .as_ref()
            .is_some_and(|s| s.item.as_ref() == "ae2:color_applicator")
        {
            super::Hand::Main
        } else if self
            .offhand
            .as_ref()
            .is_some_and(|s| s.item.as_ref() == "ae2:color_applicator")
        {
            super::Hand::Off
        } else {
            return false;
        };
        if self.remote.is_some() {
            self.send(horizon_protocol::packets::play::Serverbound::CustomPayload(
                horizon_protocol::packets::configuration::CustomPayload {
                    channel: "ae2:mouse_wheel".into(),
                    data: vec![u8::from(delta > 0.0)],
                },
            ));
        } else {
            let _ = self.with_hand(hand, |game| {
                game.ae2_cycle_applicator(if delta > 0.0 { 1 } else { -1 })
            });
        }
        true
    }
    pub(super) fn lan_ae2_item_wheel(
        &mut self,
        host: &mut super::lan::LanHost,
        conn: u32,
        data: &[u8],
    ) {
        if data.len() != 1 || data[0] > 1 {
            return;
        }
        self.as_guest(host, conn, |game| {
            let hand = if game.inventory[game.selected]
                .as_ref()
                .is_some_and(|s| s.item.as_ref() == "ae2:color_applicator")
            {
                super::Hand::Main
            } else if game
                .offhand
                .as_ref()
                .is_some_and(|s| s.item.as_ref() == "ae2:color_applicator")
            {
                super::Hand::Off
            } else {
                return;
            };
            let _ = game.with_hand(hand, |game| {
                game.ae2_cycle_applicator(if data[0] == 1 { 1 } else { -1 })
            });
        });
        if let Some(index) = host.hub.guests.iter().position(|guest| guest.conn == conn) {
            host.hub.send_inventory(index);
        }
    }
    pub(super) fn ae2_applicator_takes_mob(&self, stack: &ItemStack, index: usize) -> bool {
        if stack.item.as_ref() != "ae2:color_applicator"
            || self.mobs[index].species() != Species::Sheep
            || self.mobs[index].extra.sheared
        {
            return false;
        }
        let color = selected(stack).or_else(|| {
            self.ae2_applicator_entries(stack)
                .ok()?
                .first()
                .map(|(_, _, color)| *color)
        });
        color
            .filter(|n| *n < 16)
            .and_then(|color| {
                horizon_client::block_entity_render::DYES
                    .iter()
                    .position(|(name, _)| *name == power::COLORS[color as usize])
            })
            .is_some_and(|dye| dye as u8 != self.mobs[index].extra.color)
    }
    fn ae2_applicator_entries(&self, stack: &ItemStack) -> Result<Vec<(ItemKey, u64, u8)>, String> {
        power::applicator_entries(self.res.items(), stack)
    }
    fn ae2_applicator_color(&self, stack: &mut ItemStack) -> Result<Option<u8>, String> {
        let color = power::applicator_color(self.res.items(), stack)?;
        set_selected(stack, color)?;
        Ok(color)
    }
    pub(super) fn ae2_cycle_applicator(&mut self, offset: i32) -> Result<(), String> {
        let Some(mut stack) = self
            .held()
            .filter(|s| s.item.as_ref() == "ae2:color_applicator")
            .cloned()
        else {
            return Ok(());
        };
        let anchor = self.ae2_applicator_color(&mut stack)?;
        let mut entries = self.ae2_applicator_entries(&stack)?;
        entries.sort_by_key(|(_, _, color)| *color);
        if let Some(anchor) = anchor {
            if !entries.is_empty() {
                let at = entries
                    .iter()
                    .position(|(_, _, color)| *color == anchor)
                    .unwrap_or(0);
                let next = (at as i64 + i64::from(offset.signum())).rem_euclid(entries.len() as i64)
                    as usize;
                set_selected(&mut stack, Some(entries[next].2))?;
            } else {
                set_selected(&mut stack, None)?;
            }
        }
        *self.held_mut() = Some(stack);
        Ok(())
    }
    fn ae2_consume_color(
        &self,
        stack: &mut ItemStack,
        color: u8,
        simulate: bool,
    ) -> Result<bool, String> {
        let mut cell = ae2_lab::read_cell(stack)?;
        let Some(key) = cell.entries().find_map(|(key, _)| {
            ae2_lab::prototype(key)
                .ok()
                .and_then(|s| power::item_color(self.res.items(), &s))
                .filter(|n| *n == color)
                .map(|_| key.clone())
        }) else {
            return Ok(false);
        };
        if power::energy(stack)?.0 < 100.0 || cell.extract(&key, 1, Action::Simulate) < 1 {
            return Ok(false);
        }
        if !simulate {
            cell.extract(&key, 1, Action::Modulate);
            power::transfer(stack, 100.0, false, false)?;
            ae2_lab::write_cell(stack, &cell)?;
            if !cell.entries().any(|(k, _)| *k == key) && selected(stack) == Some(color) {
                set_selected(stack, None)?;
            }
        }
        Ok(true)
    }
    fn ae2_recolor_block(&mut self, hit: &Hit, color: u8) -> Result<bool, String> {
        let old = self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
        let Some(state) = self.pack.assets.registry.state(old) else {
            return Ok(false);
        };
        if let Some(id) = recolored(&state.name, color) {
            let Some(default) = self
                .pack
                .assets
                .registry
                .default_state(&id)
                .and_then(|s| self.pack.assets.registry.state(s))
            else {
                return Ok(false);
            };
            let mut properties = default.properties.clone();
            for (name, value) in &state.properties {
                if properties.contains_key(name) {
                    properties.insert(name.clone(), value.clone());
                }
            }
            let next = self
                .pack
                .assets
                .registry
                .resolve(&BlockState {
                    name: id,
                    properties,
                })
                .map_err(|e| e.to_string())?;
            self.level_set(hit.pos, next).map_err(|e| e.to_string())?;
            return Ok(true);
        }
        let pos = (hit.pos[0], hit.pos[1], hit.pos[2]);
        if let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&pos) {
            if machine.kind == MachineKind::CableBus {
                let Some(old) = machine.config.get(&"cable".into()) else {
                    return Ok(false);
                };
                let before = old.clone();
                let Some(mut cable) = horizon_client::save::item_from_nbt(old) else {
                    return Ok(false);
                };
                let Some((kind, _)) = super::ae2_parts::cable_item(&cable.item) else {
                    return Ok(false);
                };
                let suffix = match kind {
                    ae2_network_core::DeviceKind::GlassCable => "glass_cable",
                    ae2_network_core::DeviceKind::CoveredCable => "covered_cable",
                    ae2_network_core::DeviceKind::SmartCable => "smart_cable",
                    ae2_network_core::DeviceKind::DenseCoveredCable => "covered_dense_cable",
                    ae2_network_core::DeviceKind::DenseSmartCable => "smart_dense_cable",
                    _ => return Ok(false),
                };
                let id = format!("ae2:{}_{suffix}", power::COLORS[color as usize]);
                if cable.item.as_ref() == id {
                    return Ok(false);
                }
                cable.item = id.into();
                let after = horizon_client::save::item_to_nbt(&cable);
                machine.config.insert("cable".into(), after.clone());
                self.world.block_entities.record_ae2_center_change(pos, Some(before), Some(after));
                self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
                return Ok(true);
            }
            if matches!(
                machine.kind,
                MachineKind::Chest | MachineKind::CraftingMonitor
            ) {
                let next = Tag::String(power::COLORS[color as usize].into());
                let old = machine
                    .config
                    .get(&"painted_color".into())
                    .cloned()
                    .unwrap_or(Tag::String("fluix".into()));
                if old == next {
                    return Ok(false);
                }
                machine.config.insert("painted_color".into(), next);
                self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub(super) fn ae2_use_applicator(
        &mut self,
        stack: &ItemStack,
        allow_block_actions: bool,
    ) -> Result<UseResult, String> {
        let on_block = matches!(self.guest_use, Some(GuestUse::OnBlock(_)))
            || self.guest_use.is_none() && allow_block_actions;
        if !on_block {
            self.ae2_cycle_applicator(1)?;
            return Ok(UseResult::Fail);
        }
        let Some(hit) = self.pick() else {
            return Ok(UseResult::Fail);
        };
        if !self.may_build() {
            return Ok(UseResult::Fail);
        }
        let mut updated = stack.clone();
        let result = self.ae2_applicator_at(&mut updated, hit)?;
        *self.held_mut() = Some(updated);
        if matches!(result, UseResult::Fail) && self.keys.contains(&KeyCode::ShiftLeft) {
            self.ae2_cycle_applicator(1)?;
            return Ok(UseResult::Success { swing: false });
        }
        Ok(result)
    }
    /// IBlockTool use from a dispenser has a real stack and clicked face,
    /// but no player or alternate-use color cycling.
    pub(super) fn ae2_applicator_at(&mut self, stack: &mut ItemStack, hit: Hit) -> Result<UseResult, String> {

        let color = self.ae2_applicator_color(stack)?;
        if let Some(color) = color.filter(|_| power::can_use(stack, 100.0)) {
            if self.ae2_consume_color(stack, color, true)? {
                let mut changed = self.ae2_recolor_block(&hit, color)?;
                if !changed && color == 16 {
                    let p = [
                        hit.pos[0] + hit.face[0],
                        hit.pos[1] + hit.face[1],
                        hit.pos[2] + hit.face[2],
                    ];
                    if matches!(self.world.block_entities.get(&(p[0],p[1],p[2])),Some(BlockEntity::Ae2(m)) if m.kind==MachineKind::Paint)
                    {
                        self.ae2_clean_paint_side(p, hit.face.map(|n| -n))?;
                        changed = true;
                    }
                }
                if changed {
                    self.ae2_consume_color(stack, color, false)?;
                    
                    return Ok(UseResult::Success { swing: true });
                }
            }
        }
        
        Ok(UseResult::Fail)
    }
    pub(super) fn ae2_applicator_sheep(&mut self, index: usize) -> Result<bool, String> {
        let Some(mut stack) = self
            .held()
            .filter(|s| s.item.as_ref() == "ae2:color_applicator")
            .cloned()
        else {
            return Ok(false);
        };
        if self.mobs[index].species() != Species::Sheep
            || !self.mobs[index].alive()
            || self.mobs[index].extra.sheared
        {
            return Ok(false);
        }
        let Some(color) = self.ae2_applicator_color(&mut stack)? else {
            return Ok(false);
        };
        let dye = if color == 16 {
            None
        } else {
            horizon_client::block_entity_render::DYES
                .iter()
                .position(|(name, _)| *name == power::COLORS[color as usize])
        };
        let Some(dye) = dye.filter(|d| *d as u8 != self.mobs[index].extra.color) else {
            return Ok(false);
        };
        if power::can_use(&stack, 100.0) && self.ae2_consume_color(&mut stack, color, false)? {
            self.mobs[index].extra.color = dye as u8;
            let p = self.mobs[index].position;
            self.audio
                .play("minecraft:item.dye.use", Some([p.x, p.y, p.z]), 1.0, 1.0);
            *self.held_mut() = Some(stack);
        }
        Ok(true)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_selected_color_overrides_legacy_and_writes_source_component() {
        let mut item=ItemStack::new("ae2:color_applicator",1);
        item.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([(SELECTED,Tag::String("blue".into()))])));
        item.data_mut().set_other(SELECTED,Some(Tag::String("red".into())));
        assert_eq!(selected(&item),Some(8));
        set_selected(&mut item,Some(12)).unwrap();
        assert_eq!(item.data().unwrap().other(SELECTED),Some(&Tag::String("blue".into())));
        set_selected(&mut item,None).unwrap();
        assert_eq!(selected(&item),None);
    }
    #[test]
    fn source_block_recolor_families() {
        assert_eq!(
            recolored("minecraft:glass", 8),
            Some("minecraft:red_stained_glass".into())
        );
        assert_eq!(
            recolored("minecraft:red_stained_glass", 16),
            Some("minecraft:glass".into())
        );
        assert_eq!(
            recolored("minecraft:red_carpet", 16),
            Some("minecraft:white_carpet".into())
        );
        assert_eq!(recolored("minecraft:red_concrete", 16), None);
        assert_eq!(recolored("minecraft:stone", 8), None);
    }
}
