//! Block entities and the items that carry them
//! (`horizon_client::block_entity_item`): the item Pick Block gives, with the
//! block entity's data on Ctrl in creative, and the data and components a
//! placed item gives its block entity.
use super::Game;
use horizon_client::{
    Result, block_entity_item, block_entity_nbt, containers::BlockEntity, item_data,
    items::ItemStack, save,
};
use horizon_world::{
    level::classes::Class,
    nbt::{Compound, JavaString, Tag},
    registry::BlockStateId,
};

/// The component patch of `stack`.
fn components(stack: &ItemStack) -> Compound {
    match item_data::encode_components(stack) {
        Some(Tag::Compound(c)) => c,
        _ => Compound::new(),
    }
}

impl Game<'_> {
    /// `BlockItem.updateBlockStateFromTag`: apply each recognized, valid
    /// property from the item's block-state component and ignore the rest.
    /// The source updates the same block with client notification flags.
    #[cfg(feature = "ae2-lab")]
    pub(super) fn ae2_apply_item_block_state(&mut self,pos:[i32;3],expected:horizon_world::registry::BlockStateId,stack:&ItemStack)->horizon_world::registry::BlockStateId {
        let old=self.world.block(pos[0],pos[1],pos[2]);
        // BlockItem only updates a state when placement left the same block.
        if self.res.facts().get(old).name!=self.res.facts().get(expected).name{return old}
        let Some(Tag::Compound(properties))=stack.data().and_then(|data|data.other("minecraft:block_state")) else {return old};
        let Some(mut state)=self.pack.assets.registry.state(old).cloned() else{return old};
        let mut next=old;
        for (key,value) in properties {
            let Tag::String(value)=value else{continue};
            let key=key.to_string_lossy();
            if !state.properties.contains_key(&key){continue}
            let mut candidate=state.clone();
            candidate.properties.insert(key,value.to_string_lossy());
            if let Ok(id)=self.pack.assets.registry.resolve(&candidate){state=candidate;next=id;}
        }
        if next!=old {
            let at=horizon_core::position::BlockPos::new(pos[0],pos[1],pos[2]);
            self.with_level(|blocks,level,ticks| {
                blocks.set_block(level,ticks,at,next,horizon_world::level::flags::UPDATE_CLIENTS);
            });
        }
        self.world.block(pos[0],pos[1],pos[2])
    }

    /// The block entity at `key` as vanilla saves it, with its custom name.
    fn saved_block_entity(&self, key: (i32, i32, i32)) -> Option<Tag> {
        let entity = self.world.block_entities.get(&key)?;
        let block = &self
            .res
            .facts()
            .get(self.world.block(key.0, key.1, key.2))
            .name;
        let mut tag = save::encode_block_entity(key, entity, block);
        if let (Some(name), Tag::Compound(map)) = (self.world.block_names.get(&key), &mut tag) {
            map.insert("CustomName".into(), Tag::String(name.as_str().into()));
        }
        #[cfg(feature = "ae2-lab")]
        if let Some(original)=self.world.block_entities.vanilla_metadata(&key) {
            tag=save::live_vanilla_block_entity(key,entity,block,self.world.block_names.get(&key).map(String::as_str),Some(original));
            self.restore_loot_table(key,&mut tag);
        }
        block_entity_nbt::canonical(&tag)
    }

    /// `BlockState.getCloneItemStack` of the block at `pos`, then, with
    /// `include_data`, `ServerGamePacketListenerImpl.addBlockDataToItem`.
    pub(super) fn picked_stack(&self, pos: [i32; 3], include_data: bool) -> Option<ItemStack> {
        let key = (pos[0], pos[1], pos[2]);
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let facts = self.res.facts().get(state);
        // PistonHeadBlock.getCloneItemStack: the piston the head belongs to.
        let head = self
            .pack
            .assets
            .registry
            .state(state)
            .filter(|s| s.name.as_str() == "minecraft:piston_head")
            .map(|s| match s.properties.get("type") {
                Some(t) if t == "sticky" => "minecraft:sticky_piston",
                _ => "minecraft:piston",
            });
        let item = head.unwrap_or(&*facts.item);
        if item == "minecraft:air" {
            return None;
        }
        // FlowerPotBlock.getCloneItemStack: a potted plant picks as that plant
        // (`new ItemStack(potted)`), an empty pot as the pot itself.
        let behaviours = self.res.behaviours();
        let plant = behaviours.data(state);
        let plant_item = plant
            .is(Class::FlowerPot)
            .then_some(plant.link)
            .flatten()
            .map(|block| behaviours.block_data(block).item.clone())
            .filter(|item| &**item != "minecraft:air");
        let mut stack = ItemStack::new(plant_item.as_deref().unwrap_or(item), 1);
        let mut patch = Compound::new();
        // BeehiveBlock.getCloneItemStack: the honey level as the item's block state.
        if include_data
            && let Some(level) = self
                .pack
                .assets
                .registry
                .state(state)
                .filter(|s| matches!(s.name.as_str(), "minecraft:beehive" | "minecraft:bee_nest"))
                .and_then(|s| s.properties.get("honey_level"))
        {
            patch.insert(
                "minecraft:block_state".into(),
                Tag::compound([("honey_level", Tag::String(level.as_str().into()))]),
            );
        }
        // CopperGolemStatueBlock.getCloneItemStack: the statue's pose as the item's
        // block_state (`CopperGolemStatueBlockEntity.getItem`), data or not.
        if let Some(pose) = self
            .pack
            .assets
            .registry
            .state(state)
            .filter(|s| s.name.ends_with("copper_golem_statue"))
            .and_then(|s| s.properties.get("copper_golem_pose"))
        {
            patch.insert(
                "minecraft:block_state".into(),
                Tag::compound([("copper_golem_pose", Tag::String(pose.as_str().into()))]),
            );
        }
        // LightBlock.getCloneItemStack, setLightOnStack: the light's level as
        // the item's block state, whether or not the block data is picked.
        if let Some(level) = self
            .pack
            .assets
            .registry
            .state(state)
            .filter(|s| s.name.as_str() == "minecraft:light")
            .and_then(|s| s.properties.get("level"))
        {
            patch.insert(
                "minecraft:block_state".into(),
                Tag::compound([("level", Tag::String(level.as_str().into()))]),
            );
        }
        // AbstractBannerBlock.getCloneItemStack: the banner's own item
        // (BannerBlockEntity.getItem), data or not.
        let banner = matches!(
            self.world.block_entities.get(&key),
            Some(BlockEntity::Banner(_))
        );
        // DecoratedPotBlock.getCloneItemStack: createDecoratedPotInstance, the
        // sherds only; the pot's item comes with include_data.
        let pot = &*facts.name == "minecraft:decorated_pot";
        if (include_data || banner || pot)
            && let Some(saved) = self.saved_block_entity(key)
        {
            let mut picked = block_entity_item::pick(&saved);
            if pot && !include_data {
                picked.retain(|k, _| *k == JavaString::from("minecraft:pot_decorations"));
            }
            patch.extend(picked);
        }
        if !patch.is_empty() {
            let mut all = components(&stack);
            all.extend(patch);
            (stack.damage, stack.extra) = item_data::decode_components(Some(&Tag::Compound(all)));
        }
        Some(stack)
    }

    /// `BlockItem.updateBlockStateFromTag`: the properties of the item's
    /// `block_state` component (a hive's honey level, a statue's pose) set on
    /// the block just placed at `pos`. Returns the state it has now.
    pub(super) fn apply_item_block_state(
        &mut self,
        pos: [i32; 3],
        state: BlockStateId,
        stack: &ItemStack,
    ) -> Result<BlockStateId> {
        let Some(Tag::Compound(saved)) =
            stack.data().and_then(|d| d.other("minecraft:block_state"))
        else {
            return Ok(state);
        };
        let Some(current) = self.pack.assets.registry.state(state) else {
            return Ok(state);
        };
        // Only the properties the block has.
        let props: Vec<(String, String)> = saved
            .iter()
            .filter_map(|(k, v)| match v {
                Tag::String(v) => Some((k.to_string_lossy(), v.to_string_lossy())),
                _ => None,
            })
            .filter(|(k, v)| current.properties.get(k).is_some_and(|old| old != v))
            .collect();
        let props: Vec<(&str, &str)> = props.iter().map(|(k, v)| (&**k, &**v)).collect();
        match self.with_props(state, &props) {
            Some(new) if !props.is_empty() && new != state => {
                self.level_set(pos, new)?;
                Ok(new)
            }
            _ => Ok(state),
        }
    }

    /// `BlockItem.updateCustomBlockEntityTag` and `updateBlockEntityComponents`
    /// for the block entity just placed at `pos` from `stack`; `game_master`:
    /// the placer can use game master blocks (a dispenser can't).
    pub(super) fn apply_placed_item(
        &mut self,
        pos: [i32; 3],
        stack: &ItemStack,
        game_master: bool,
    ) {
        let key = (pos[0], pos[1], pos[2]);
        self.pending.banner.remove(&pos);
        let Some(saved) = self.saved_block_entity(key) else {
            return;
        };
        let placed = block_entity_item::place(&saved, &components(stack), game_master);
        if placed == saved {
            #[cfg(feature = "ae2-lab")]
            self.ae2_import_dismantled_energy(key, stack);
            return;
        }
        let Some((_, mut entity)) = save::decode_block_entity(&placed) else {
            return;
        };
        // The new crafter's state flag (CrafterBlock.newBlockEntity), which
        // its saved data has no part of.
        if let (BlockEntity::Crafter(new), Some(BlockEntity::Crafter(old))) =
            (&mut entity, self.world.block_entities.get(&key))
        {
            new.state_triggered = old.state_triggered;
        }
        // Banners and signs keep their name themselves.
        if !matches!(entity, BlockEntity::Banner(_) | BlockEntity::Sign(_)) {
            match placed.get("CustomName").map(|value|horizon_client::entity_nbt::name_text(value).plain()) {
                Some(name) => self.world.block_names.insert(key, name),
                None => self.world.block_names.remove(&key),
            };
        }
        #[cfg(feature = "ae2-lab")]
        let ae2_import=matches!(self.world.block_entities.get(&key),Some(BlockEntity::Ae2(_)));
        if let Err(error)=self.world.block_entities.import_placed_in_place(key, entity) {
            log::warn!("placed block entity import at {key:?}: {error}");
            #[cfg(feature = "ae2-lab")]
            if ae2_import {self.ae2_fault_placement_import(format!("placed block entity import at {key:?}: {error}"));}
            return;
        }
        self.update_spawner_display(key);
        #[cfg(feature = "ae2-lab")]
        self.world.block_entities.remember_vanilla_metadata(key,&placed);
        #[cfg(feature = "ae2-lab")]
        if matches!(self.world.block_entities.get(&key),Some(BlockEntity::Ae2(_))) {
            let name=super::ae2_memory_host::stack_name_component(stack);
            match name.as_ref() {
                Some(raw)=>{self.world.block_names.insert(key,horizon_client::entity_nbt::name_text(raw).plain());},
                None=>{self.world.block_names.remove(&key);},
            }
            if let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get_mut(&key) {
                super::ae2_memory_host::set_machine_name_component(machine,name.as_ref());
            }
            if self.world.block_entities.refresh_pending_placed_ae2(key,Some(&placed))==Some(false) {
                self.ae2_fault_placement_import(format!("placed AE2 entity {key:?} escaped before its item data"));
            }
        }
        #[cfg(feature = "ae2-lab")]
        self.ae2_import_dismantled_energy(key, stack);
    }
    /// AE2 energy cells retain stored AE as a physical item component on
    /// disassembly; restore it when that actual item is placed.
    #[cfg(feature = "ae2-lab")]
    pub(super) fn ae2_import_dismantled_energy(&mut self,key:(i32,i32,i32),stack:&ItemStack) {
        use horizon_client::ae2_world::MachineKind;
        let max=match stack.item.as_ref() {"ae2:energy_cell"=>200_000.0,"ae2:dense_energy_cell"=>1_600_000.0,_=>return};
        let energy=stack.data().and_then(|d|d.other("ae2:stored_energy").or_else(||
            d.other("minecraft:custom_data").and_then(|v|v.get("ae2:stored_energy"))))
            .and_then(|v|match v {Tag::Double(bits)=>Some(f64::from_bits(*bits)),_=>None})
            .filter(|v|v.is_finite());
        let Some(energy)=energy else{return};
        // EnergyCellBlockEntity.importSettings calls StoredEnergyAmount.setStored:
        // negative and sub-minimum values become zero; over-capacity clamps.
        let mut stored=ae2_machines_core::stored_energy::StoredEnergy::new(0.0,max);
        stored.set_stored(energy);
        let energy=stored.stored();
        let mut changed=false;
        if let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get_mut(&key) {
            if matches!(machine.kind,MachineKind::EnergyCell|MachineKind::DenseEnergyCell) {
                machine.energy=energy;self.world.modified.insert((key.0>>4,key.2>>4));changed=true;
            }
        }
        if !changed {return;}
        // Import happens before source onReady. Refresh the same construction
        // record before any observer sees it, including inside a sandbox.
        let pending=self.world.block_entities.refresh_pending_placed_ae2(key,None);
        if pending==Some(false) {self.ae2_fault_placement_import(format!("placed AE2 energy cell {key:?} escaped before import"));}
        if self.ae2_formation_staging {
            return;
        }
        let observed=self.ae2_sync_events();
        if observed {
            // If Added already carried the energy, registration observed it.
            // Otherwise this is the post-commit notification of an existing node.
            if pending!=Some(true) {if let Some(address)=self.ae2_local_address(key){self.ae2_energy_store_changed(address,None);}}
        } else {self.ae2_live_events.borrow_mut().energy.failure=Some("ordered placement energy event unavailable".into());}
    }
    /// Publish a committed staged placement's current energy, without writing
    /// the original item amount again after other callbacks have run.
    #[cfg(feature = "ae2-lab")]
    pub(super) fn ae2_publish_placed_energy(&self,key:(i32,i32,i32)) {
        use horizon_client::ae2_world::MachineKind;
        if !matches!(self.world.block_entities.get(&key),Some(BlockEntity::Ae2(machine))
            if matches!(machine.kind,MachineKind::EnergyCell|MachineKind::DenseEnergyCell)) {return;}
        if self.ae2_sync_events() {
            if let Some(address)=self.ae2_local_address(key) {self.ae2_energy_store_changed(address,None);}
        } else {
            self.ae2_live_events.borrow_mut().energy.failure=Some("ordered placement energy event unavailable".into());
        }
    }
}
