// Staged only; not included in the shared compiled source.
// Owner scan required for an AE2 Debug Cube live ItemStack reference.
// Do not convert an unseen owner to a value-equality match: that could consume
// an unrelated identical stack. Parked worlds and guest cursor/armor are real owners.
use super::Game;
use horizon_client::{ae2_stack_identity::StackRef, containers::BlockEntity, items::ItemStack};

impl Game<'_> {
    fn debug_cube_owned_stacks(&self, reference: &StackRef) -> Vec<ItemStack> {
        let mut found = Vec::new();
        let mut visit = |item: &Option<ItemStack>| {
            if let Some(item) = item { if reference.tracks(item) { found.push(item.clone()); } }
        };
        for item in &self.inventory { visit(item); }
        for item in &self.armor { visit(item); }
        visit(&self.offhand); visit(&self.carried);
        for item in &self.craft_grid { visit(item); }
        for item in &self.ender_items { visit(item); }
        if let Some(host) = &self.lan {
            for guest in &host.hub.guests {
                for item in &guest.inventory { visit(item); }
                for item in &guest.armor { visit(item); }
                visit(&guest.offhand); visit(&guest.carried);
                for item in &guest.craft_grid { visit(item); }
                for item in &guest.ender_items { visit(item); }
            }
        }
        for (dim, world) in self.ae2_loaded_worlds() {
            for (_, entity) in world.block_entities.iter() {
                for item in entity.slots() { visit(item); }
                if let BlockEntity::Ae2(machine) = entity { for item in &machine.upgrades { visit(item); } }
            }
            let dropped = if dim == self.dim { Some(self.dropped.as_slice()) } else { self.levels.ae2_dropped(dim) };
            if let Some(dropped) = dropped { for entity in dropped { visit(&Some(entity.stack.clone())); } }
        }
        found
    }
}

impl Game<'_> {
    fn debug_cube_write_owned_count(&mut self, reference: &StackRef, count: u32) {
        let update = |slot: &mut Option<ItemStack>| {
            if slot.as_ref().is_some_and(|item| reference.tracks(item)) {
                if count == 0 { *slot = None; } else if let Some(item)=slot { item.count=count; }
            }
        };
        for item in &mut self.inventory { update(item); }
        for item in &mut self.armor { update(item); }
        update(&mut self.offhand); update(&mut self.carried);
        for item in &mut self.craft_grid { update(item); }
        for item in &mut self.ender_items { update(item); }
        if let Some(host) = &mut self.lan {
            let mut changed = Vec::new();
            for (index, guest) in host.hub.guests.iter_mut().enumerate() {
                let had = guest.inventory.iter().chain(guest.armor.iter()).chain([&guest.offhand,&guest.carried]).chain(guest.craft_grid.iter()).chain(guest.ender_items.iter()).flatten().any(|item| reference.tracks(item));
                for item in &mut guest.inventory { update(item); }
                for item in &mut guest.armor { update(item); }
                update(&mut guest.offhand); update(&mut guest.carried);
                for item in &mut guest.craft_grid { update(item); }
                for item in &mut guest.ender_items { update(item); }
                if had { changed.push(index); }
            }
            for index in changed { host.hub.send_inventory(index); }
        }
        let dims:Vec<_> = self.ae2_loaded_worlds().iter().map(|(dim,_)|*dim).collect();
        for dim in dims {
            if let Some(world) = self.ae2_world_for_mut(dim) {
                let mut dirty = Vec::new();
                for (&pos, entity) in world.block_entities.iter_mut() {
                    let had = entity.slots().iter().flatten().any(|item| reference.tracks(item)) || matches!(entity,BlockEntity::Ae2(machine) if machine.upgrades.iter().flatten().any(|item|reference.tracks(item)));
                    for item in entity.slots_mut() { update(item); }
                    if let BlockEntity::Ae2(machine)=entity { for item in &mut machine.upgrades {update(item);} }
                    if had { dirty.push((pos.0>>4,pos.2>>4)); }
                }
                world.modified.extend(dirty);
            }
            let dropped = if dim == self.dim { Some(&mut self.dropped) } else { self.levels.ae2_dropped_mut(dim) };
            if let Some(dropped) = dropped {
                for entity in dropped.iter_mut() { if reference.tracks(&entity.stack) {entity.stack.count=count;} }
                dropped.retain(|entity| !reference.tracks(&entity.stack) || entity.stack.count>0);
            }
        }
    }
}

// Captured from the actual pinned Java 1.21.1 + AE2 19.2.18 item
// registry: these classes inherit Item.useOn unchanged, which returns PASS.
fn passive_item(id: &str) -> bool {
    static ITEMS: std::sync::OnceLock<std::collections::HashSet<String>> = std::sync::OnceLock::new();
    ITEMS.get_or_init(|| serde_json::from_str::<Vec<String>>(include_str!("ae2_cube_passive_items.json"))
        .expect("valid source item-use oracle").into_iter().collect()).contains(id)
}
#[cfg(test)]
mod passive_tests {
    #[test]
    fn source_passive_items_distinguish_overridden_use_on() {
        assert!(super::passive_item("minecraft:diamond"));
        assert!(super::passive_item("minecraft:water_bucket"));
        assert!(!super::passive_item("minecraft:bone_meal"));
        assert!(!super::passive_item("minecraft:flint_and_steel"));
    }
}

impl Game<'_> {
    pub(super) fn ae2_try_debug_cube_use(&mut self, pos: [i32;3]) -> bool {
        let at=(pos[0],pos[1],pos[2]);
        if !matches!(self.world.block_entities.get(&at),Some(BlockEntity::Ae2(m)) if m.kind==horizon_client::ae2_world::MachineKind::DebugCubeGenerator) { return false; }
        let Some(incarnation)=self.world.block_entities.incarnation(&at) else {return false};
        let alternate=self.keys.contains(&super::KeyCode::ShiftLeft);
        let selected=self.inventory.get_mut(self.selected).and_then(Option::as_mut);
        let empty=selected.as_ref().is_none_or(|stack|stack.count==0);
        let state=self.ae2_debug_cubes.entry((self.dim,at)).or_default();
        if state.incarnation != incarnation { *state=super::ae2_debug::CubeState {incarnation, ..Default::default()}; }
        state.click(selected,alternate);
        let size=state.size;
        if empty {self.system_message(horizon_client::text::Text::literal(format!("Size: {size}")));}
        true
    }
    pub(super) fn ae2_tick_debug_cubes(&mut self) {
        let dim=self.dim;
        let live:Vec<_>=self.ae2_debug_cubes.keys().filter_map(|(d,pos)| (*d==dim).then_some(*pos)).collect();
        for pos in live {
            if !matches!(self.world.block_entities.get(&pos),Some(BlockEntity::Ae2(m)) if m.kind==horizon_client::ae2_world::MachineKind::DebugCubeGenerator) {self.ae2_debug_cubes.remove(&(dim,pos));continue;}
            let Some(mut state)=self.ae2_debug_cubes.remove(&(dim,pos)) else {continue};
            if self.world.block_entities.incarnation(&pos) != Some(state.incarnation) {continue;}
            if let Some(reference)=state.stack.as_ref() {
                let found=self.debug_cube_owned_stacks(reference);
                // An absent/duplicated marker can arise from an owner path
                // not yet bridged. Do not mutate the world from stale state.
                if found.len()>1 || (found.is_empty() && !reference.is_known_detached()) {self.ae2_debug_cubes.insert((dim,pos),state);continue;}
                if let Some(owner)=found.first() {reference.synchronize(owner);}
            }
            let (message,spawn)=state.tick();
            if let Some(seconds)=message {
                let text=horizon_client::text::Text::literal(format!("Spawning in... {seconds}"));
                if let Some(host)=&self.lan { host.hub.system_message(horizon_client::lan::data::component_nbt(&text.component())); }
                if self.options.dedicated.is_none() { self.system_message(text); }
            }
            if !spawn {self.ae2_debug_cubes.insert((dim,pos),state);continue;}
            let Some(reference)=state.stack.as_ref().cloned() else {continue};
            let selected=reference.read().clone();
            let is_block=self.res.items().get(&selected.item).is_some_and(|info|info.block.is_some());
            let is_passive=passive_item(&selected.item);
            let is_bone_meal=selected.item.as_ref()=="minecraft:bone_meal";
            // Unknown overriding item classes retain the machine until their
            // source-backed useOn implementation exists.
            if !is_block && !is_passive && !is_bone_meal { self.ae2_debug_cubes.insert((dim,pos),state);continue; }
            if !matches!(self.set_state_named([pos.0,pos.1,pos.2],"minecraft:air",&[]), Ok(true)) {
                self.ae2_debug_cubes.insert((dim,pos),state);continue;
            }
            // The inherited source Item.useOn returns PASS and consumes nothing.
            if is_passive { continue; }
            let positions:Vec<_>=state.offsets().collect();
            for [dx,dy,dz] in positions {
                let at=[pos.0+dx,pos.1+dy,pos.2+dz];
                if is_bone_meal {
                    use horizon_world::level::LevelAccess;
                    use horizon_core::{geometry::Direction, position::BlockPos};
                    let target=BlockPos::new(at[0],at[1],at[2]);
                    let grew=self.with_level(|b,level,ticks| {
                        if b.grow_crop(level,ticks,target) { return true; }
                        let clicked=level.block_state(target);
                        if !b.is_face_sturdy(clicked,Direction::Down) { return false; }
                        b.grow_water_plant(level,ticks,BlockPos::new(at[0],at[1]-1,at[2]),Some(Direction::Down))
                    });
                    if grew {
                        reference.shrink(1);
                        self.debug_cube_write_owned_count(&reference,reference.read().count);
                        // The pinned Java source commits these effects and then
                        // throws on its null player, before growth particles.
                        // Stop this Cube's loop without crashing the server.
                        log::warn!("AE2 Cube bone meal encountered source null-player exception after committing growth");
                        break;
                    }
                    continue;
                }
                let ctx=horizon_world::level::PlaceContext {
                    clicked_pos:horizon_core::position::BlockPos::new(at[0],at[1],at[2]),
                    face:horizon_core::geometry::Direction::Down,
                    hit:horizon_core::geometry::Vec3::new(at[0] as f64+0.5,at[1] as f64,at[2] as f64+0.5),
                    inside:false,yaw:0.0,pitch:0.0,sneaking:false,item:selected.item.to_string(),can_use_game_master_blocks:false,
                };
                let result=self.with_level(|b,level,ticks|b.place_block(level,ticks,&ctx));
                if let horizon_world::level::PlaceOutcome::Placed {pos:placed,state} = result {
                    let target=[placed.x,placed.y,placed.z];
                    let state=self.ae2_apply_item_block_state(target,state,&selected);
                    if let Some(entity)=BlockEntity::for_block(&self.res.facts().get(state).name) {
                        self.world.block_entities.insert((placed.x,placed.y,placed.z),entity);
                        self.apply_placed_item(target,&selected,false);
                    }
                    reference.shrink(1);
                    self.debug_cube_write_owned_count(&reference,reference.read().count);
                }
            }
            let _=self.process_level_events();
        }
    }
}
