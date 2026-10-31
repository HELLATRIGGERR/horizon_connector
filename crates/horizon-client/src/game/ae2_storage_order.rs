//! AE2 storage priority and physical cell mounts shared by item/fluid routing.
use super::{Game,ae2_global::GridAddress,ae2_lab,ae2_fluids};
use ae2_cell_core::ItemKey;
use horizon_client::{ae2_world::MachineKind,containers::BlockEntity,items::ItemStack};
use horizon_world::nbt::Tag;

#[derive(Clone,Debug)]
pub(super) struct CellMount {
    pub address:GridAddress,
    pub slot:usize,
    pub priority:i32,
    pub kind:MachineKind,
}
impl Game<'_> {
    pub(super) fn ae2_storage_priority(&self,address:GridAddress)->Result<i32,String>{
        let Some(BlockEntity::Ae2(machine))=self.ae2_block_entity(address) else{return Err("storage host unavailable".into())};
        if !matches!(machine.kind,MachineKind::Drive|MachineKind::Chest){return Err("not a cell storage host".into())}
        match machine.config.get(&"priority".into()){None=>Ok(0),Some(Tag::Int(value))=>Ok(*value),_=>Err("invalid cell storage priority".into())}
    }
    /// Typed setter for real UI/Memory Card integrations. Routing reads the
    /// committed value for both item and fluid cells; native cell objects are
    /// decoded from their physical stacks rather than retained in a cache.
    pub(super) fn ae2_set_storage_priority_local(&mut self,pos:(i32,i32,i32),value:i32)->Result<(),String>{
        let address=self.ae2_local_address(pos).ok_or("invalid storage position")?;
        let Some(BlockEntity::Ae2(machine))=self.ae2_block_entity_mut(address) else{return Err("storage host disappeared".into())};
        if !matches!(machine.kind,MachineKind::Drive|MachineKind::Chest){return Err("not a cell storage host".into())}
        let mark_dirty=machine.kind==MachineKind::Drive;
        machine.config.insert("priority".into(),Tag::Int(value));
        // Source Drive.setPriority saves its block; ME Chest remounts its cell
        // but does not itself dirty the chunk. Never clear an earlier mark.
        if mark_dirty {self.ae2_mark_modified(address);}
        Ok(())
    }
    pub(super) fn ae2_cell_mounts_encoded(&self,anchor:GridAddress)->Result<Vec<CellMount>,String>{
        let mut mounts=Vec::new();
        for address in self.ae2_drives_encoded(anchor)?{
            let priority=self.ae2_storage_priority(address)?;
            let Some(BlockEntity::Ae2(machine))=self.ae2_block_entity(address) else{continue};
            for (slot,stack) in machine.slots.iter().enumerate(){if stack.is_some(){mounts.push(CellMount{address,slot,priority,kind:machine.kind});}}
        }
        Ok(mounts)
    }
    pub(super) fn ae2_mounted_cell_stack(&self,mount:&CellMount)->Result<ItemStack,String>{
        let Some(BlockEntity::Ae2(machine))=self.ae2_block_entity(mount.address) else{return Err("cell host disappeared".into())};
        if machine.kind!=mount.kind{return Err("cell host changed".into())}
        machine.slots.get(mount.slot).and_then(Option::as_ref).cloned().ok_or_else(||"mounted cell disappeared".into())
    }
    pub(super) fn ae2_cell_preferred(&self,mount:&CellMount,key:&ItemKey,fluid:bool)->Result<bool,String>{
        let stack=self.ae2_mounted_cell_stack(mount)?;
        if let Ok(keys)=self.ae2_creative_cell_keys(&stack){
            let key=if fluid{super::ae2_storage_devices::CreativeCellKey::Fluid(key.clone())}else{super::ae2_storage_devices::CreativeCellKey::Item(key.clone())};
            return Ok(keys.contains(&key));
        }
        // DriveWatcher is an MEInventoryHandler: its simulated extraction
        // makes existing keys preferred. ME Chest's ChestMonitorHandler is
        // only a delegating wrapper around BasicCellInventory, whose default
        // preference is false. Keep that source distinction.
        if mount.kind!=MachineKind::Drive{return Ok(false)}
        let cell=self.ae2_read_cell(&stack);
        Ok(cell.is_ok_and(|cell| {
            let primary=(cell.config().channel==ae2_cell_core::Channel::Fluid)==fluid;
            if primary {cell.count(key)>0} else {cell.other_count(key)>0}
        }))
    }
    pub(super) fn ae2_item_bus_preferred(&mut self,mount:&super::ae2_buses::StorageMount,key:&ItemKey)->Result<bool,String>{
        let mut delegate=mount.clone();delegate.access=ae2_machines_core::Access::ReadWrite;
        // MEInventoryHandler's whitelist test is independent of access mode.
        if !mount.inverted&&!mount.filter.is_empty()&&delegate.allows(&ae2_machines_core::Key::from(key),false){return Ok(true)}
        // Its super.extract check bypasses the wrapper's own extract filter
        // while retaining the real sided delegate's extraction constraints.
        delegate.filter_on_extract=false;
        let mut plan=super::ae2_buses::FormationPlan::default();
        Ok(self.ae2_storage_extract(&delegate,key,1,true,&mut plan)?.is_some_and(|stack|stack.count>0))
    }
    /// One physical cell operation. Only the enclosing transaction commits the
    /// returned stack; simulation never mutates the original item or power.
    pub(super) fn ae2_cell_storage_change(&self,mount:&CellMount,key:&ItemKey,amount:u64,fluid:bool,insert:bool,simulate:bool)->Result<(u64,Option<ItemStack>),String>{
        use ae2_cell_core::{Action,Channel};
        if amount==0{return Ok((0,None))}
        let stack=self.ae2_mounted_cell_stack(mount)?;
        if let Ok(keys)=self.ae2_creative_cell_keys(&stack){
            let wanted=if fluid{super::ae2_storage_devices::CreativeCellKey::Fluid(key.clone())}else{super::ae2_storage_devices::CreativeCellKey::Item(key.clone())};
            return Ok((if keys.contains(&wanted){amount}else{0},None))
        }
        let cell=self.ae2_read_cell(&stack);
        let Ok(mut cell)=cell else{return Ok((0,None))};
        let primary=(cell.config().channel==Channel::Fluid)==fluid;
        if insert && !primary{return Ok((0,None))}
        let action=if simulate{Action::Simulate}else{Action::Modulate};
        let moved=if insert{
            if fluid{cell.insert(key,amount,action)}else{self.ae2_cell_insert(&mut cell,key,amount,action)}
        }else if primary {cell.extract(key,amount,action)}else{cell.extract_other(key,amount,action)};
        let updated=if moved>0&&!simulate{
            let mut updated=stack;
            if cell.config().channel==Channel::Fluid{ae2_fluids::write_fluid_cell(&mut updated,&cell)?;}else{ae2_lab::write_cell(&mut updated,&cell)?;}
            Some(updated)
        }else{None};
        Ok((moved,updated))
    }

}
