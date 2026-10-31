//! Fluid storage in physical drive cells. Fluid quantities are mB (1000/bucket).
//! Mounted sky stone tanks and physical cells retain the actual fluid.
use super::{Game, ae2_fluids};
use ae2_cell_core::ItemKey;
use horizon_client::containers::BlockEntity;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use super::ae2_buses::FormationPlan;
type Pos = (i32, i32, i32);
type TankChange=(super::ae2_global::GridAddress,horizon_client::ae2_world::Machine,ae2_network_core::NodeIdentity);
const BUCKET_MB: u64 = 1000;
const MB_PER_AE: f64 = 125.0; // AEFluidKeys.getAmountPerOperation
impl Game<'_> {
    pub(super) fn ae2_grid_fluids(&self, pos: Pos) -> Result<Vec<(ItemKey, u64)>, String> {
        self.ae2_grid_fluids_encoded(self.ae2_local_address(pos).ok_or("invalid AE2 anchor")?)
    }
    pub(super) fn ae2_grid_fluids_encoded(
        &self,
        pos: super::ae2_global::GridAddress,
    ) -> Result<Vec<(ItemKey, u64)>, String> {
        let mut totals = BTreeMap::<ItemKey, u64>::new();
        for at in self.ae2_drives_encoded(pos)? {
            let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(at) else {
                continue;
            };
            for item in machine.slots.iter().flatten() {
                if let Ok(keys) = self.ae2_creative_cell_keys(item) {
                    for key in keys {
                        if let super::ae2_storage_devices::CreativeCellKey::Fluid(k) = key {
                            let total = totals.entry(k).or_default();
                            *total = total
                                .checked_add(i32::MAX as u64)
                                .ok_or("fluid amount overflow")?;
                        }
                    }
                }
                let Ok(cell) = self.ae2_read_cell(item) else { continue; };
                for (key, amount) in cell.entries_for(ae2_cell_core::Channel::Fluid) {
                    let total = totals.entry(key.clone()).or_default();
                    *total = total.checked_add(*amount).ok_or("fluid amount overflow")?;
                }
            }
        }
        for mount in self.ae2_fluid_storage_mounts_encoded(pos) {
            let entries =
                if matches!(self.ae2_block_entity(mount.target),Some(BlockEntity::Ae2(machine)) if machine.kind==horizon_client::ae2_world::MachineKind::Condenser) {
                    // CondenserMEStorage exposes output items, but no fluids.
                    Vec::new()
                } else if let Some(entries) = self.ae2_interface_fluid_entries(mount.target, mount.face) {
                    entries
                } else {
                    self.ae2_monitor_read(&mount)?.1
                };
            for (key, amount) in entries {
                if mount.visible(&ae2_machines_core::Key::from(&key)) {
                    let total = totals.entry(key).or_default();
                    *total = total.checked_add(amount).ok_or("fluid amount overflow")?;
                }
            }
        }
        Ok(totals.into_iter().collect())
    }
    /// The caller must transfer real fluid to/from its own storage atomically;
    /// simulate=true never modifies cells or power.
    pub(super) fn ae2_grid_fluid_insert(
        &mut self,
        pos: Pos,
        key: &ItemKey,
        amount: u64,
        simulate: bool,
    ) -> Result<u64, String> {
        self.ae2_grid_fluid_insert_encoded(
            self.ae2_local_address(pos).ok_or("invalid AE2 anchor")?,
            key,
            amount,
            simulate,
        )
    }
    pub(super) fn ae2_grid_fluid_insert_encoded(
        &mut self,
        pos: super::ae2_global::GridAddress,
        key: &ItemKey,
        amount: u64,
        simulate: bool,
    ) -> Result<u64, String> {
        self.ae2_grid_fluid_move(pos, key, amount, simulate, true, false)
    }
    pub(super) fn ae2_grid_fluid_insert_staged(
        &mut self, pos: Pos, key: &ItemKey, amount: u64, simulate: bool, plan: &mut FormationPlan,
    ) -> Result<u64, String> {
        self.ae2_grid_fluid_insert_staged_encoded(self.ae2_local_address(pos).ok_or("invalid AE2 anchor")?, key, amount, simulate, plan)
    }
    pub(super) fn ae2_grid_fluid_insert_staged_encoded(
        &mut self, pos: super::ae2_global::GridAddress, key: &ItemKey, amount: u64, simulate: bool, plan: &mut FormationPlan,
    ) -> Result<u64, String> {
        self.ae2_grid_fluid_move_staged(pos, key, amount, simulate, true, false, true, plan)
    }
    pub(super) fn ae2_grid_fluid_extract(
        &mut self,
        pos: Pos,
        key: &ItemKey,
        amount: u64,
        simulate: bool,
    ) -> Result<u64, String> {
        self.ae2_grid_fluid_extract_encoded(
            self.ae2_local_address(pos).ok_or("invalid AE2 anchor")?,
            key,
            amount,
            simulate,
        )
    }
    pub(super) fn ae2_grid_fluid_extract_encoded(
        &mut self,
        pos: super::ae2_global::GridAddress,
        key: &ItemKey,
        amount: u64,
        simulate: bool,
    ) -> Result<u64, String> {
        self.ae2_grid_fluid_move(pos, key, amount, simulate, false, false)
    }
    fn ae2_grid_fluid_move(
        &mut self,
        pos: super::ae2_global::GridAddress,
        key: &ItemKey,
        amount: u64,
        simulate: bool,
        insert: bool,
        require_full: bool,
    ) -> Result<u64, String> {
        let before = self.ae2_snapshot_blocks();
        let mut plan = FormationPlan::default();
        let result = self.ae2_grid_fluid_move_staged(pos, key, amount, simulate, insert, require_full, true, &mut plan);
        let result = if simulate { result } else { result.and_then(|n| self.ae2_commit_formation(plan).map(|()| n)) };
        if result.is_err() {
            self.ae2_restore_blocks(before);
        }
        result
    }
    /// Machine-owned MEStorage fluid insertion, without StorageHelper power.
    pub(super) fn ae2_grid_fluid_insert_uncharged_encoded(
        &mut self,
        pos: super::ae2_global::GridAddress,
        key: &ItemKey,
        amount: u64,
    ) -> Result<u64, String> {
        let before = self.ae2_snapshot_blocks();
        let mut plan=FormationPlan::default();
        let result = self.ae2_grid_fluid_insert_uncharged_staged_encoded(pos,key,amount,&mut plan)
            .and_then(|n| self.ae2_commit_formation(plan).map(|()| n));
        if result.is_err() {
            self.ae2_restore_blocks(before);
        }
        result
    }
    pub(super) fn ae2_grid_fluid_insert_uncharged_staged_encoded(
        &mut self, pos: super::ae2_global::GridAddress, key: &ItemKey, amount: u64, plan: &mut FormationPlan,
    ) -> Result<u64, String> {
        self.ae2_grid_fluid_move_staged(pos,key,amount,false,true,false,false,plan)
    }
    pub(super) fn ae2_grid_fluid_capability_encoded(
        &mut self,
        pos: super::ae2_global::GridAddress,
        key: &ItemKey,
        amount: u64,
        insert: bool,
        simulate: bool,
    ) -> Result<u64, String> {
        let before = self.ae2_snapshot_blocks();
        let mut plan=FormationPlan::default();
        let result = self.ae2_grid_fluid_capability_staged_encoded(pos,key,amount,insert,simulate,&mut plan);
        let result = if simulate {result} else {result.and_then(|n| self.ae2_commit_formation(plan).map(|()| n))};
        if result.is_err() {
            self.ae2_restore_blocks(before);
        }
        result
    }
    pub(super) fn ae2_grid_fluid_capability_staged_encoded(
        &mut self, pos: super::ae2_global::GridAddress, key: &ItemKey, amount: u64,
        insert: bool, simulate: bool, plan: &mut FormationPlan,
    ) -> Result<u64, String> {
        self.ae2_grid_fluid_move_staged(pos,key,amount,simulate,insert,false,false,plan)
    }
    fn ae2_grid_fluid_move_staged(
        &mut self, pos: super::ae2_global::GridAddress, key: &ItemKey, amount: u64,
        simulate: bool, insert: bool, require_full: bool, charge_power: bool, plan: &mut FormationPlan,
    ) -> Result<u64, String> {
        // Crafting reception and delegated storages may mutate before the
        // operation completes. Restore them and the plan on simulation/error.
        let before=self.ae2_snapshot_blocks();
        let savepoint=plan.len();
        let result=self.ae2_grid_fluid_move_inner(pos,key,amount,simulate,insert,require_full,charge_power,plan);
        if simulate || result.is_err() {
            self.ae2_restore_blocks(before);
            plan.truncate(savepoint);
        }
        result
    }
    fn ae2_grid_fluid_move_inner(
        &mut self,
        pos: super::ae2_global::GridAddress,
        key: &ItemKey,
        amount: u64,
        simulate: bool,
        insert: bool,
        require_full: bool,
        charge_power: bool,
        plan: &mut FormationPlan,
    ) -> Result<u64, String> {
        let cells = self.ae2_cell_mounts_encoded(pos)?;
        let mut left = if charge_power {
            let energy = self.ae2_take_power_encoded(pos, amount as f64 / MB_PER_AE, true);
            amount.min((energy * MB_PER_AE + 0.9).floor() as u64)
        } else {
            amount
        };
        let wanted = left;
        if insert && left > 0 {
            let received = self.ae2_crafting_receive_fluid_encoded(pos, key, left)?;
            if received > left {
                return Err("crafting accepted more fluid than supplied".into());
            }
            left -= received;
        }
        let mut changes=Vec::new();let mut tank_changes:Vec<TankChange>=Vec::new();
        let mounts=self.ae2_fluid_storage_mounts_encoded(pos);
        let mut priorities=std::collections::BTreeSet::new();priorities.extend(mounts.iter().map(|mount|mount.priority));priorities.extend(cells.iter().map(|mount|mount.priority));
        if insert { priorities.extend(self.ae2_formation_priorities(pos)); }
        let priorities:Vec<_>=if insert{priorities.into_iter().rev().collect()}else{priorities.into_iter().collect()};
        for priority in priorities{
            let mut late_buses=Vec::new();let mut late_cells=Vec::new();
            for mount in mounts.iter().filter(|mount|mount.priority==priority){
                if left==0{break}
                if insert&&!self.ae2_fluid_bus_preferred(mount,key,plan)?{late_buses.push(mount);continue}
                let (accepted,change)=self.ae2_fluid_mount_change(mount,key,left,insert,simulate,plan)?;left-=accepted;
                if let Some(change)=change{tank_changes.push(change);}
            }
            for mount in cells.iter().filter(|mount|mount.priority==priority){
                if left==0{break}
                if insert&&!self.ae2_cell_preferred(mount,key,true)?{late_cells.push(mount);continue}
                let (accepted,updated)=self.ae2_cell_storage_change(mount,key,left,true,insert,simulate)?;left-=accepted;
                if let Some(updated)=updated{changes.push((mount.address,mount.slot,updated));}
            }
            for mount in late_buses{
                if left==0{break}
                let (accepted,change)=self.ae2_fluid_mount_change(mount,key,left,insert,simulate,plan)?;left-=accepted;
                if let Some(change)=change{tank_changes.push(change);}
            }
            for mount in late_cells{
                if left==0{break}
                let (accepted,updated)=self.ae2_cell_storage_change(mount,key,left,true,insert,simulate)?;left-=accepted;
                if let Some(updated)=updated{changes.push((mount.address,mount.slot,updated));}
            }
            if insert && left>0 { left-=self.ae2_form_fluids(pos,key,left,priority,plan)?; }
            if left==0{break}
        }
        let moved = wanted - left;
        if require_full && moved != amount {
            return Err("fluid grid cannot complete transfer".into());
        }
        if !simulate && moved > 0 {
            // No other work can interleave on this authoritative game thread.
            // Validate staged target slots before any power or inventory mutation.
            for (at, slot, _) in &changes {
                if !matches!(self.ae2_block_entity(*at),Some(BlockEntity::Ae2(m)) if *slot < m.slots.len())
                {
                    return Err("fluid drive disappeared".into());
                }
            }
            for (at, _, _) in &tank_changes {
                if !matches!(self.ae2_block_entity(*at),Some(BlockEntity::Ae2(m)) if m.kind==horizon_client::ae2_world::MachineKind::SkyStoneTank)
                {
                    return Err("fluid tank disappeared".into());
                }
            }
            let before = self.ae2_snapshot_blocks();
            if charge_power {
                let charged = self.ae2_take_power_encoded(pos, moved as f64 / MB_PER_AE, false);
                if (charged * MB_PER_AE + 0.9).floor() < moved as f64 {
                    self.ae2_restore_blocks(before);
                    return Err("fluid power changed before commit".into());
                }
            }
            for (at, staged, identity) in tank_changes {
                if let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity_mut(at) {
                    **machine = staged;
                }
                self.ae2_mark_modified(at);
                self.ae2_monitor_invalidate(identity);
            }
            for (at, slot, item) in changes {
                if let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity_mut(at) {
                    machine.slots[slot] = Some(item);
                }
                self.ae2_mark_modified(at);
            }
        }
        Ok(moved)
    }
    fn ae2_fluid_bus_preferred(&mut self,mount:&super::ae2_buses::StorageMount,key:&ItemKey,plan:&mut FormationPlan)->Result<bool,String>{
        let mut delegate=mount.clone();delegate.access=ae2_machines_core::Access::ReadWrite;
        if !mount.inverted&&!mount.filter.is_empty()&&delegate.allows(&ae2_machines_core::Key::from(key),false){return Ok(true)}
        delegate.filter_on_extract=false;
        Ok(self.ae2_fluid_mount_change(&delegate,key,1,false,true,plan)?.0>0)
    }
    /// Keep the existing interface and tank transaction rules behind one
    /// storage operation while priority/preferred passes choose its order.
    fn ae2_fluid_mount_change(&mut self,mount:&super::ae2_buses::StorageMount,key:&ItemKey,amount:u64,insert:bool,simulate:bool,plan:&mut FormationPlan)->Result<(u64,Option<TankChange>),String>{
        if matches!(self.ae2_block_entity(mount.target),Some(BlockEntity::Ae2(machine)) if machine.kind==horizon_client::ae2_world::MachineKind::Condenser) {
            if !mount.allows(&ae2_machines_core::Key::from(key),!insert){return Ok((0,None))}
            let moved=if insert {self.ae2_condenser_me_fluid_insert_at(mount.target,amount,simulate)}else{0};
            if moved>0 && !simulate {self.ae2_monitor_invalidate(mount.identity);}
            Ok((if insert&&mount.void_overflow{amount}else{moved},None))
        }else if self.ae2_has_interface_face(mount.target,mount.face){
            if !mount.allows(&ae2_machines_core::Key::from(key),!insert)||!self.ae2_interface_mount_allowed(mount.target,mount.face,insert){return Ok((0,None))}
            let moved=self.ae2_interface_fluid_move_staged(mount.target,mount.face,key,amount,insert,simulate,plan).unwrap_or(0);
            Ok((if insert&&mount.void_overflow{amount}else{moved},None))
        }else{self.stage_fluid_mount(mount,key,amount,insert)}
    }
    fn stage_fluid_mount(
        &self,
        mount: &super::ae2_buses::StorageMount,
        key: &ItemKey,
        amount: u64,
        insert: bool,
    ) -> Result<
        (
            u64,
            Option<(
                super::ae2_global::GridAddress,
                horizon_client::ae2_world::Machine,
                ae2_network_core::NodeIdentity,
            )>,
        ),
        String,
    > {
        if !mount.allows(&ae2_machines_core::Key::from(key), !insert) {
            return Ok((0, None));
        }
        if !self.ae2_monitor_ready(mount) {return Ok((0,None));}
        let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(mount.target) else {
            return Err("fluid tank missing".into());
        };
        let current = read_tank(machine)?;
        let actual = if insert {
            if current.as_ref().is_some_and(|(stored, _)| stored != key) {
                0
            } else {
                amount.min(TANK_CAPACITY - current.as_ref().map_or(0, |(_, n)| *n))
            }
        } else {
            current
                .as_ref()
                .filter(|(stored, _)| stored == key)
                .map_or(0, |(_, n)| amount.min(*n))
        };
        let accepted = if insert && mount.void_overflow {
            amount
        } else {
            actual
        };
        if actual == 0 {
            return Ok((accepted, None));
        }
        let mut staged = (**machine).clone();
        let old = current.map_or(0, |(_, n)| n);
        set_tank(
            &mut staged,
            key,
            if insert { old + actual } else { old - actual },
        )?;
        Ok((accepted, Some((mount.target, staged, mount.identity))))
    }
    /// /ae2 fluid grid status|insert|extract X Y Z [entry-index]
    pub(super) fn ae2_fluid_grid_command(&mut self, args: &[&str]) -> Result<Value, String> {
        if self.player.is_none() {
            return Err("enter a world first".into());
        }
        if args.len() < 4 {
            return Err("usage: /ae2 fluid grid status|insert|extract x y z [entry]".into());
        }
        let pos = (
            args[1].parse().map_err(|_| "invalid x")?,
            args[2].parse().map_err(|_| "invalid y")?,
            args[3].parse().map_err(|_| "invalid z")?,
        );
        match args[0] {
            "status" if args.len() == 4 => {
                let entries=self.ae2_grid_fluids(pos)?.iter().enumerate().map(|(i,(key,amount))|json!({"index":i,"fluid":key.item_id,"amount_mb":amount,"has_components":!key.components.is_empty()})).collect::<Vec<_>>();
                Ok(json!({"entries":entries}))
            }
            "insert" if args.len() == 4 => {
                let bucket = self.inventory[self.selected]
                    .clone()
                    .ok_or("hold a water or lava bucket")?;
                let id = ae2_fluids::bucket_fluid(&bucket.item)
                    .ok_or("only water and lava buckets are supported")?;
                if !ae2_fluids::plain_bucket(&bucket) {
                    return Err("modified bucket cannot be safely converted".into());
                }
                let key = ae2_fluids::fluid_key(id);
                if self.ae2_grid_fluid_insert(pos, &key, BUCKET_MB, true)? != BUCKET_MB {
                    return Err("grid cannot accept a full bucket or lacks power".into());
                }
                let before = self.inventory.clone();
                self.exchange_bucket(&bucket, "minecraft:bucket")?;
                match self.ae2_grid_fluid_move(
                    self.ae2_local_address(pos).ok_or("invalid AE2 anchor")?,
                    &key,
                    BUCKET_MB,
                    false,
                    true,
                    true,
                ) {
                    Ok(BUCKET_MB) => Ok(json!({"fluid":id,"inserted_mb":BUCKET_MB})),
                    Ok(_) => {
                        self.inventory = before;
                        Err("fluid grid changed before transfer".into())
                    }
                    Err(e) => {
                        self.inventory = before;
                        Err(e)
                    }
                }
            }
            "extract" if args.len() == 5 => {
                let index: usize = args[4].parse().map_err(|_| "invalid fluid index")?;
                let entries = self.ae2_grid_fluids(pos)?;
                let (key, amount) = entries.get(index).ok_or("fluid entry out of range")?;
                if *amount < BUCKET_MB || !key.components.is_empty() {
                    return Err("fluid cannot fill a vanilla bucket".into());
                }
                let filled = ae2_fluids::fluid_bucket(&key.item_id)
                    .ok_or("fluid has no supported bucket")?;
                let bucket = self.inventory[self.selected]
                    .clone()
                    .ok_or("hold an empty bucket")?;
                if &*bucket.item != "minecraft:bucket" || !ae2_fluids::plain_bucket(&bucket) {
                    return Err("hold an unmodified empty bucket".into());
                }
                if self.ae2_grid_fluid_extract(pos, key, BUCKET_MB, true)? != BUCKET_MB {
                    return Err("grid cannot provide a full bucket or lacks power".into());
                }
                let before = self.inventory.clone();
                self.exchange_bucket(&bucket, filled)?;
                match self.ae2_grid_fluid_move(
                    self.ae2_local_address(pos).ok_or("invalid AE2 anchor")?,
                    key,
                    BUCKET_MB,
                    false,
                    false,
                    true,
                ) {
                    Ok(BUCKET_MB) => Ok(json!({"fluid":key.item_id,"extracted_mb":BUCKET_MB})),
                    Ok(_) => {
                        self.inventory = before;
                        Err("fluid grid changed before transfer".into())
                    }
                    Err(e) => {
                        self.inventory = before;
                        Err(e)
                    }
                }
            }
            _ => Err("usage: /ae2 fluid grid status|insert|extract x y z [entry]".into()),
        }
    }
}

const TANK_CAPACITY: u64 = 16_000;
/// Read one genuine sky-stone tank's fluid, preserving opaque components.
fn read_tank(
    machine: &horizon_client::ae2_world::Machine,
) -> Result<Option<(ItemKey, u64)>, String> {
    if machine.kind != horizon_client::ae2_world::MachineKind::SkyStoneTank {
        return Err("not a sky stone tank".into());
    }
    let Some(tag) = machine.config.get(&"ae2_tank".into()) else {
        return Ok(None);
    };
    let Some(horizon_world::nbt::Tag::String(id)) = tag.get("fluid") else {
        return Err("invalid tank fluid".into());
    };
    let id = id.to_string().map_err(|_| "invalid tank fluid")?;
    if !ae2_fluids::valid_fluid_id(&id) {
        return Err("invalid tank fluid".into());
    }
    let Some(horizon_world::nbt::Tag::ByteArray(components)) = tag.get("components") else {
        return Err("invalid tank components".into());
    };
    let Some(horizon_world::nbt::Tag::Long(amount)) = tag.get("amount") else {
        return Err("invalid tank amount".into());
    };
    if *amount <= 0 || *amount > TANK_CAPACITY as i64 {
        return Err("invalid tank amount".into());
    }
    Ok(Some((
        ItemKey {
            item_id: id,
            components: components.clone(),
        },
        *amount as u64,
    )))
}
fn set_tank(
    machine: &mut horizon_client::ae2_world::Machine,
    key: &ItemKey,
    amount: u64,
) -> Result<(), String> {
    use horizon_world::nbt::Tag;
    if amount > TANK_CAPACITY || !ae2_fluids::valid_fluid_id(&key.item_id) {
        return Err("invalid tank contents".into());
    }
    if amount == 0 {
        machine.config.remove(&"ae2_tank".into());
    } else {
        machine.config.insert(
            "ae2_tank".into(),
            Tag::compound([
                ("fluid", Tag::String(key.item_id.as_str().into())),
                ("components", Tag::ByteArray(key.components.clone())),
                ("amount", Tag::Long(amount as i64)),
            ]),
        );
    }
    Ok(())
}
impl Game<'_> {
    pub(super) fn ae2_tank_contents_encoded(
        &self,
        address: super::ae2_global::GridAddress,
    ) -> Result<Option<(ItemKey, u64)>, String> {
        let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(address) else {
            return Err("tank missing".into());
        };
        read_tank(machine)
    }
    pub(super) fn ae2_tank_contents(&self, pos: Pos) -> Result<Option<(ItemKey, u64)>, String> {
        let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get(&pos) else {
            return Err("tank missing".into());
        };
        read_tank(machine)
    }
    pub(super) fn ae2_tank_fill(
        &mut self,
        pos: Pos,
        key: &ItemKey,
        amount: u64,
        simulate: bool,
    ) -> Result<u64, String> {
        self.ae2_tank_fill_encoded(
            self.ae2_local_address(pos).ok_or("invalid tank address")?,
            key,
            amount,
            simulate,
        )
    }
    pub(super) fn ae2_tank_fill_encoded(
        &mut self,
        pos: super::ae2_global::GridAddress,
        key: &ItemKey,
        amount: u64,
        simulate: bool,
    ) -> Result<u64, String> {
        let current = self.ae2_tank_contents_encoded(pos)?;
        if current.as_ref().is_some_and(|(stored, _)| stored != key) {
            return Ok(0);
        }
        let old = current.map_or(0, |(_, n)| n);
        let moved = amount.min(TANK_CAPACITY - old);
        if !simulate && moved > 0 {
            let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity_mut(pos) else {
                return Err("tank missing".into());
            };
            set_tank(machine, key, old + moved)?;
            self.ae2_mark_modified(pos);
        }
        Ok(moved)
    }
    pub(super) fn ae2_tank_drain(
        &mut self,
        pos: Pos,
        key: &ItemKey,
        amount: u64,
        simulate: bool,
    ) -> Result<u64, String> {
        self.ae2_tank_drain_encoded(
            self.ae2_local_address(pos).ok_or("invalid tank address")?,
            key,
            amount,
            simulate,
        )
    }
    pub(super) fn ae2_tank_drain_encoded(
        &mut self,
        pos: super::ae2_global::GridAddress,
        key: &ItemKey,
        amount: u64,
        simulate: bool,
    ) -> Result<u64, String> {
        let Some((stored, old)) = self.ae2_tank_contents_encoded(pos)? else {
            return Ok(0);
        };
        if stored != *key {
            return Ok(0);
        }
        let moved = amount.min(old);
        if !simulate && moved > 0 {
            let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity_mut(pos) else {
                return Err("tank missing".into());
            };
            set_tank(machine, key, old - moved)?;
            self.ae2_mark_modified(pos);
        }
        Ok(moved)
    }
    /// /ae2 tank status|insert|extract x y z using a real selected bucket.
    pub(super) fn ae2_tank_command(&mut self, args: &[&str]) -> Result<Value, String> {
        if args.len() != 4 {
            return Err("usage: /ae2 tank status|insert|extract x y z".into());
        }
        let pos = (
            args[1].parse().map_err(|_| "invalid x")?,
            args[2].parse().map_err(|_| "invalid y")?,
            args[3].parse().map_err(|_| "invalid z")?,
        );
        match args[0] {
            "status" => Ok(match self.ae2_tank_contents(pos)? {
                Some((key, n)) => {
                    json!({"fluid":key.item_id,"amount_mb":n,"capacity_mb":TANK_CAPACITY,"has_components":!key.components.is_empty()})
                }
                None => json!({"amount_mb":0,"capacity_mb":TANK_CAPACITY}),
            }),
            "insert" | "extract" => {
                let source = self.inventory[self.selected]
                    .clone()
                    .ok_or("hold a bucket")?;
                if !ae2_fluids::plain_bucket(&source) {
                    return Err("modified bucket cannot be safely converted".into());
                }
                let (key, result, insert) = if args[0] == "insert" {
                    let fluid = ae2_fluids::bucket_fluid(&source.item)
                        .ok_or("hold an ordinary water or lava bucket")?;
                    (ae2_fluids::fluid_key(fluid), "minecraft:bucket", true)
                } else {
                    if &*source.item != "minecraft:bucket" {
                        return Err("hold an empty bucket".into());
                    }
                    let (key, amount) = self.ae2_tank_contents(pos)?.ok_or("tank is empty")?;
                    if amount < BUCKET_MB || !key.components.is_empty() {
                        return Err("tank cannot fill a vanilla bucket".into());
                    }
                    let result = ae2_fluids::fluid_bucket(&key.item_id)
                        .ok_or("fluid has no supported bucket")?;
                    (key, result, false)
                };
                let available = if insert {
                    self.ae2_tank_fill(pos, &key, BUCKET_MB, true)?
                } else {
                    self.ae2_tank_drain(pos, &key, BUCKET_MB, true)?
                };
                if available != BUCKET_MB {
                    return Err("tank cannot transfer a full bucket".into());
                }
                let before = self.inventory.clone();
                let old = self
                    .world
                    .block_entities
                    .get(&pos)
                    .cloned()
                    .ok_or("tank missing")?;
                self.exchange_bucket(&source, result)?;
                let applied = if insert {
                    self.ae2_tank_fill(pos, &key, BUCKET_MB, false)
                } else {
                    self.ae2_tank_drain(pos, &key, BUCKET_MB, false)
                };
                match applied {
                    Ok(BUCKET_MB) => {
                        Ok(json!({"fluid":key.item_id,"amount_mb":BUCKET_MB,"insert":insert}))
                    }
                    other => {
                        self.inventory = before;
                        if let Some(entity) = self.world.block_entities.get_mut(&pos) {
                            *entity = old;
                            self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
                        }
                        Err(match other {
                            Err(e) => e,
                            _ => "tank changed during transfer".into(),
                        })
                    }
                }
            }
            _ => Err("usage: /ae2 tank status|insert|extract x y z".into()),
        }
    }
}
#[cfg(test)]
mod tank_tests {
    use super::*;
    use horizon_client::ae2_world::{Machine, MachineKind};
    #[test]
    fn sky_tank_capacity_identity_and_nbt_roundtrip() {
        let mut machine = Machine::new(MachineKind::SkyStoneTank);
        let water = ae2_fluids::fluid_key("minecraft:water");
        set_tank(&mut machine, &water, 16_000).unwrap();
        assert_eq!(read_tank(&machine).unwrap(), Some((water.clone(), 16_000)));
        assert!(set_tank(&mut machine, &water, 16_001).is_err());
        let be = BlockEntity::Ae2(Box::new(machine));
        let tag = horizon_client::save::encode_block_entity((0, 1, 2), &be, "ae2:sky_stone_tank");
        let (_, back) = horizon_client::save::decode_block_entity(&tag).unwrap();
        let BlockEntity::Ae2(back) = back else {
            panic!()
        };
        assert_eq!(read_tank(&back).unwrap(), Some((water, 16_000)));
    }
}
