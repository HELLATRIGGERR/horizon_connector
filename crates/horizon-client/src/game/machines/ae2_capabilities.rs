//! Sided native inventories exposed to AE2's P2P capability adapters. Reuses
//! vanilla container policy rather than treating every block as a plain chest.
use super::{Containers, Game, Kind, View, bone_meal};
use crate::game::ae2_global::GridAddress;
use horizon_client::{ae2_world::MachineKind, containers::BlockEntity, items::ItemStack};
use horizon_core::geometry::Direction;
use horizon_world::nbt::Tag;

#[derive(Clone, Copy)]
enum ProviderBufferOwner {
    Block(GridAddress),
    Part(super::super::ae2_parts::PartAddress),
}
/// NeoForge's SidedInvWrapper uses the native container's limit and has
/// special new-stack rules for furnace buckets and brewing bottle slots.
fn native_sided_slot_limit(kind: Kind, slot: usize, item: &str, occupied: bool, item_max: u32) -> u32 {
    let max = item_max.min(99);
    if kind == Kind::Brewing && slot < 3 { return max.min(1); }
    if kind == Kind::Furnace && slot == 1 && item == "minecraft:bucket" && !occupied { return max.min(1); }
    max
}
/// `BlockOrientation.getSide(TOP)` of a block with `facing` and `spin`
/// (`OrientationStrategies.full`).
pub(super) fn orientation_up(facing: &str, spin: usize) -> Option<Direction> {
    match facing {
        "down"=>[Direction::North,Direction::West,Direction::South,Direction::East],
        "up"=>[Direction::North,Direction::East,Direction::South,Direction::West],
        "north"=>[Direction::Up,Direction::West,Direction::Down,Direction::East],
        "south"=>[Direction::Up,Direction::East,Direction::Down,Direction::West],
        "west"=>[Direction::Up,Direction::South,Direction::Down,Direction::North],
        "east"=>[Direction::Up,Direction::North,Direction::Down,Direction::South],_=>return None,
    }.get(spin).copied()
}
/// The AE2 inventory filter a hopper, dropper or crafter meets on a
/// machine's own slot `index`: whether `stack` may go in, and how many the
/// slot holds. `None` leaves the plain container rules.
pub(super) fn ae2_slot_insert(kind: MachineKind, index: usize, stack: &ItemStack) -> Option<(bool, u32)> {
    let cell = crate::game::ae2_storage_devices::is_valid_storage_cell;
    match kind {
        MachineKind::QuantumLink => Some((index == 0 && crate::game::ae2_quantum::frequency(stack).is_some(), 1)),
        // DriveBlockEntity.CellValidInventoryFilter; AppEngCellInventory holds one cell a bay.
        MachineKind::Drive => Some((cell(stack), 1)),
        // IOPortBlockEntity: inputCellsExt is INSERT_ONLY, outputCellsExt EXTRACT_ONLY.
        MachineKind::IoPort => Some((index < 6 && cell(stack), 1)),
        _ => None,
    }
}
/// Whether automation may take from a machine's own slot `index`; `None`
/// leaves the plain container rules.
pub(super) fn ae2_slot_extract(kind: MachineKind, index: usize) -> Option<bool> {
    // IOPortBlockEntity: only the output cells come out.
    (kind == MachineKind::IoPort).then_some(index >= 6)
}

#[derive(Clone)]
struct NativeSlotChange {
    owner: GridAddress,
    incarnation: u64,
    slot: usize,
    before: Option<ItemStack>,
    after: Option<ItemStack>,
}
impl ProviderBufferOwner {
    fn host(self) -> GridAddress {
        match self {
            Self::Block(p) => p,
            Self::Part(p) => p.host,
        }
    }
}
impl Game<'_> {
    /// Read-only capability presence, including empty inventories. This does
    /// not simulate an insertion or infer support from the current contents.
    pub(in crate::game) fn ae2_native_item_supports(&self,address:GridAddress,face:Direction)->bool {
        self.ae2_item_p2p_supports(address,face).unwrap_or_else(||self.ae2_native_item_supports_local(address,face))
    }
    pub(in crate::game) fn ae2_native_item_supports_local(&self,address:GridAddress,face:Direction)->bool {
        if matches!(self.ae2_block_entity(address), Some(BlockEntity::Ae2(m)) if matches!(m.kind,MachineKind::DebugItemGenerator|MachineKind::Condenser|MachineKind::Charger|MachineKind::VibrationChamber|MachineKind::Inscriber|MachineKind::MolecularAssembler)) { return true; }
        if self.ae2_interface_supports(address,face) || self.ae2_provider_part_at(address,face).is_some()
            || matches!(self.ae2_block_entity(address),Some(BlockEntity::Ae2(m)) if m.kind==MachineKind::PatternProvider)
            {return true;}
        self.ae2_native_view(address).is_some_and(|(_,view,_)|!view.slots_for_face(Some(face)).is_empty())
    }
    /// The native fluid adapters currently expose tanks, interfaces, provider
    /// returns and fluid P2P; all are discoverable even when empty.
    pub(in crate::game) fn ae2_native_fluid_supports(&self,address:GridAddress,face:Direction)->bool {
        self.ae2_fluid_p2p_supports(address,face).unwrap_or_else(||self.ae2_native_fluid_supports_local(address,face))
    }
    pub(in crate::game) fn ae2_native_fluid_supports_local(&self,address:GridAddress,face:Direction)->bool {
        self.ae2_interface_supports(address,face) || self.ae2_provider_part_at(address,face).is_some()
            || matches!(self.ae2_block_entity(address),Some(BlockEntity::Ae2(m)) if matches!(m.kind,MachineKind::PatternProvider|MachineKind::SkyStoneTank|MachineKind::Condenser))
    }
    fn ae2_native_view(
        &self,
        address: GridAddress,
    ) -> Option<(Containers<'_>, View, super::super::Dim)> {
        let (dimension, local) = self.ae2_address_parts(address)?;
        let world = if dimension == self.dim {
            &self.world
        } else {
            self.levels.ae2_world(dimension)?
        };
        // These AE2 inventories have well-defined native physical slots. Other
        // AE2 inventories need their own capability adapter (e.g. condenser
        // input is a void handler, not the machine's output/component slots).
        if let Some(BlockEntity::Ae2(m)) = world.block_entities.get(&local) {
            if !matches!(
                m.kind,
                MachineKind::Drive
                    | MachineKind::Chest
                    | MachineKind::SkyStoneChest
                    | MachineKind::SmoothSkyStoneChest
                    | MachineKind::IoPort
                    | MachineKind::QuantumLink
            ) {
                return None;
            }
        }
        let c = Containers {
            world,
            behaviours: self.res.behaviours(),
            items: self.res.items(),
            registry: self.pack.assets.registry,
            brewing: Some(&self.res.stations().brewing),
            bone_meal: bone_meal(),
        };
        let view = c.view(local, false)?;
        if matches!(view.kind, Kind::Composter(_)) {
            return None;
        }
        Some((c, view, dimension))
    }
    /// A concrete sided native container, including an empty one. Specialized
    /// AE2/P2P capabilities keep their existing separate adapters.
    pub(in crate::game) fn ae2_native_container_supports(&self, address: GridAddress, face: Direction) -> bool {
        self.ae2_native_view(address).is_some_and(|(_, view, _)| !view.slots_for_face(Some(face)).is_empty())
    }
    /// ExternalStorageFacade lists the handler's visible slots. Only its
    /// EXTRACTABLE_ONLY mode suppresses a stack that the sided handler cannot
    /// actually extract; NONE still shows it without changing custody.
    pub(in crate::game) fn ae2_native_container_candidates(&self, address: GridAddress, face: Direction, extractable_only: bool) -> Option<Vec<ItemStack>> {
        let (c, view, _) = self.ae2_native_view(address)?;
        Some(view.slots_for_face(Some(face)).into_iter().filter_map(|slot| {
            let stack = c.get(&view, slot).filter(|s| s.count > 0)?;
            let (owner, index) = view.locate(slot)?;
            let io_input = matches!(c.world.block_entities.get(&owner), Some(BlockEntity::Ae2(m)) if m.kind == MachineKind::IoPort && index < 6);
            if extractable_only && (io_input || !c.can_take(&view, slot, stack, face)) { return None; }
            Some(stack.clone())
        }).collect())
    }
    fn ae2_native_changes_match(&self, changes: &[NativeSlotChange]) -> bool {
        changes.iter().all(|change| {
            let Some((dim, pos)) = self.ae2_address_parts(change.owner) else { return false };
            let Some(world) = self.ae2_world_for(dim) else { return false };
            world.block_entities.incarnation(&pos) == Some(change.incarnation)
                && world.block_entities.get(&pos).and_then(|entity| entity.slots().get(change.slot)) == Some(&change.before)
        })
    }
    fn ae2_apply_native_changes(&mut self, changes: Vec<NativeSlotChange>) -> Option<Vec<(GridAddress, u64)>> {
        if !self.ae2_native_changes_match(&changes) { return None; }
        let mut changed = Vec::new();
        for change in changes {
            *self.ae2_block_entity_mut(change.owner)?.slots_mut().get_mut(change.slot)? = change.after;
            self.ae2_mark_modified(change.owner);
            if !changed.iter().any(|(address, _)| *address == change.owner) { changed.push((change.owner, change.incarnation)); }
        }
        Some(changed)
    }
    /// Stage an actual sided-handler insertion; notifications are returned to
    /// the enclosing grid transaction, never emitted during simulation.
    pub(in crate::game) fn ae2_native_container_insert_staged(&mut self, address: GridAddress, face: Direction, stack: &ItemStack, limit: u32, simulate: bool) -> Option<(u32, Vec<(GridAddress, u64)>)> {
        let (c, view, dim) = self.ae2_native_view(address)?;
        let mut left = stack.count.min(limit); let requested = left; let mut changes = Vec::new();
        for slot in view.slots_for_face(Some(face)) {
            if left == 0 { break; }
            if !c.can_place(&view, slot, stack) { continue; }
            let (owner, index) = view.locate(slot)?;
            let owner_address = self.ae2_dimension_address(dim, owner)?;
            let incarnation = c.world.block_entities.incarnation(&owner)?;
            let before = c.get(&view, slot).cloned();
            let item_max = c.max_stack(stack);
            let mut max = native_sided_slot_limit(view.kind, index, &stack.item, before.is_some(), item_max);
            let state = c.world.block(owner.0, owner.1, owner.2);
            if c.name(state).ends_with("shulker_box") && stack.item.ends_with("shulker_box") { continue; }
            if let Some(BlockEntity::Ae2(m)) = c.world.block_entities.get(&owner) {
                match m.kind {
                    MachineKind::Drive | MachineKind::Chest => {
                        if !crate::game::ae2_storage_devices::is_valid_storage_cell(stack) { continue; }
                        max = 1;
                    }
                    MachineKind::IoPort => {
                        if index >= 6 || !crate::game::ae2_storage_devices::is_valid_storage_cell(stack) { continue; }
                        max = 1;
                    }
                    MachineKind::QuantumLink => {
                        if index != 0 || crate::game::ae2_quantum::frequency(stack).is_none() { continue; }
                        max = 1;
                    }
                    _ => {}
                }
            }
            let count = match &before {
                None => left.min(max),
                Some(old) if old.same_item_same_components(stack) => left.min(max.saturating_sub(old.count)),
                _ => 0,
            };
            if count == 0 { continue; }
            let after = Some(match &before {
                None => ItemStack { count, ..stack.clone() },
                Some(old) => ItemStack { count: old.count + count, ..old.clone() },
            });
            changes.push(NativeSlotChange { owner: owner_address, incarnation, slot: index, before, after });
            left -= count;
        }
        let moved = requested - left;
        if simulate || moved == 0 { return Some((moved, Vec::new())); }
        self.ae2_apply_native_changes(changes).map(|changed| (moved, changed))
    }
    /// Extract a matching key across the actual handler slots as one atomic
    /// change. Source ExternalStorageFacade continues past the first slot.
    pub(in crate::game) fn ae2_native_container_extract_staged(&mut self, address: GridAddress, face: Direction, prototype: &ItemStack, limit: u32, simulate: bool) -> Option<(Option<ItemStack>, Vec<(GridAddress, u64)>)> {
        let (c, view, dim) = self.ae2_native_view(address)?;
        let mut left = limit; let mut changes = Vec::new();
        for slot in view.slots_for_face(Some(face)) {
            if left == 0 { break; }
            let Some(stack) = c.get(&view, slot).filter(|s| s.count > 0 && s.same_item_same_components(prototype)) else { continue };
            if !c.can_take(&view, slot, stack, face) { continue; }
            let (owner, index) = view.locate(slot)?;
            if matches!(c.world.block_entities.get(&owner), Some(BlockEntity::Ae2(m)) if m.kind == MachineKind::IoPort && index < 6) { continue; }
            let owner_address = self.ae2_dimension_address(dim, owner)?;
            let incarnation = c.world.block_entities.incarnation(&owner)?;
            let count = stack.count.min(left);
            let after = (count < stack.count).then(|| ItemStack { count: stack.count - count, ..stack.clone() });
            changes.push(NativeSlotChange { owner: owner_address, incarnation, slot: index, before: Some(stack.clone()), after });
            left -= count;
        }
        let moved = limit - left;
        if moved == 0 { return Some((None, Vec::new())); }
        let out = ItemStack { count: moved, ..prototype.clone() };
        if simulate { return Some((Some(out), Vec::new())); }
        self.ae2_apply_native_changes(changes).map(|changed| (Some(out), changed))
    }
    pub(in crate::game) fn ae2_native_notify(&mut self, changed: &[GridAddress]) {
        let back = self.dim;
        for address in changed
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
        {
            let Some((dim, pos)) = self.ae2_address_parts(address) else {
                continue;
            };
            if self.enter_level(None, dim).is_ok() {
                if matches!(self.world.block_entities.get(&pos), Some(BlockEntity::Ae2(machine)) if machine.kind == MachineKind::QuantumLink) {
                    self.ae2_quantum_inventory_changed(pos);
                }
                self.container_changed(pos);
            }
        }
        let _ = self.enter_level(None, back);
    }
    pub(in crate::game) fn ae2_provider_part_at(
        &self,
        host: GridAddress,
        face: Direction,
    ) -> Option<super::super::ae2_parts::PartAddress> {
        let face = match face {
            Direction::East => 0,
            Direction::West => 1,
            Direction::Up => 2,
            Direction::Down => 3,
            Direction::South => 4,
            Direction::North => 5,
        };
        let address = super::super::ae2_parts::PartAddress { host, face };
        self.ae2_part_machine(address)
            .ok()
            .flatten()
            .filter(|m| m.kind == MachineKind::PatternProvider)
            .map(|_| address)
    }
    pub(in crate::game) fn ae2_provider_return_fluid_at(
        &mut self,
        host: GridAddress,
        face: Direction,
        key: &ae2_cell_core::ItemKey,
        amount: u64,
        simulate: bool,
    ) -> Option<u64> {
        if let Some(part) = self.ae2_provider_part_at(host, face) {
            return Some(self.ae2_part_provider_return_fluid(part, key, amount, simulate));
        }
        self.ae2_provider_return_fluid(host, key, amount, simulate)
    }
    fn ae2_provider_buffer_snapshot(
        &self,
        owner: ProviderBufferOwner,
    ) -> Option<horizon_client::ae2_world::Machine> {
        match owner {
            ProviderBufferOwner::Block(address) => match self.ae2_block_entity(address) {
                Some(BlockEntity::Ae2(m)) if m.kind == MachineKind::PatternProvider => {
                    Some((**m).clone())
                }
                _ => None,
            },
            ProviderBufferOwner::Part(address) => self
                .ae2_part_machine(address)
                .ok()
                .flatten()
                .filter(|m| m.kind == MachineKind::PatternProvider),
        }
    }
    fn ae2_provider_buffer_commit(
        &mut self,
        owner: ProviderBufferOwner,
        machine: &horizon_client::ae2_world::Machine,
    ) -> bool {
        let updated = match owner {
            ProviderBufferOwner::Block(address) => match self.ae2_block_entity_mut(address) {
                Some(BlockEntity::Ae2(m)) if m.kind == MachineKind::PatternProvider => {
                    **m = machine.clone();
                    self.ae2_mark_modified(address);
                    true
                }
                _ => false,
            },
            ProviderBufferOwner::Part(address) => {
                self.ae2_store_part_machine(address, machine).is_ok()
            }
        };
        if updated {
            self.ae2_native_notify(&[owner.host()]);
        }
        updated
    }
    pub(in crate::game) fn ae2_part_provider_return_item(
        &mut self,
        address: super::super::ae2_parts::PartAddress,
        stack: &ItemStack,
        limit: u32,
        simulate: bool,
    ) -> u32 {
        self.ae2_provider_return_item_owner(
            ProviderBufferOwner::Part(address),
            stack,
            limit,
            simulate,
        )
    }
    pub(in crate::game) fn ae2_part_provider_return_fluid(
        &mut self,
        address: super::super::ae2_parts::PartAddress,
        key: &ae2_cell_core::ItemKey,
        amount: u64,
        simulate: bool,
    ) -> u64 {
        self.ae2_provider_return_fluid_owner(
            ProviderBufferOwner::Part(address),
            key,
            amount,
            simulate,
        )
        .unwrap_or(0)
    }
    /// PatternProviderReturnInventory: nine physical, insertion-only slots.
    /// Network reinjection and its result callback are handled by provider tick.
    fn ae2_provider_return_item(
        &mut self,
        address: GridAddress,
        stack: &ItemStack,
        limit: u32,
        simulate: bool,
    ) -> u32 {
        self.ae2_provider_return_item_owner(
            ProviderBufferOwner::Block(address),
            stack,
            limit,
            simulate,
        )
    }
    fn ae2_provider_return_item_owner(
        &mut self,
        owner: ProviderBufferOwner,
        stack: &ItemStack,
        limit: u32,
        simulate: bool,
    ) -> u32 {
        let Some(mut machine) = self.ae2_provider_buffer_snapshot(owner) else {
            return 0;
        };
        if machine.kind != MachineKind::PatternProvider
            || machine.config.get(&"ae2_provider_injecting".into()) == Some(&Tag::Byte(1))
        {
            return 0;
        }
        let mut slots: Vec<Option<ItemStack>> = vec![None; 9];
        match machine.config.get(&"ae2_provider_return_items".into()) {
            None => {}
            Some(Tag::List(entries)) => {
                for entry in entries {
                    let Some(Tag::Byte(slot)) = entry.get("Slot") else {
                        return 0;
                    };
                    let slot = *slot as usize;
                    let Some(item) = horizon_client::save::item_from_nbt(entry) else {
                        return 0;
                    };
                    if slot >= slots.len()
                        || slots[slot].is_some()
                        || !matches!(entry.get("count"), Some(Tag::Int(n)) if *n > 0)
                        || item.count == 0
                    {
                        return 0;
                    }
                    slots[slot] = Some(item);
                }
            }
            _ => return 0,
        }
        let mut fluid_slots = [false; 9];
        match machine.config.get(&"ae2_provider_return_fluids".into()) {
            None => {}
            Some(Tag::List(entries)) => {
                for entry in entries {
                    let (
                        Some(Tag::Byte(slot)),
                        Some(Tag::String(_)),
                        Some(Tag::ByteArray(_)),
                        Some(Tag::Long(amount)),
                    ) = (
                        entry.get("Slot"),
                        entry.get("fluid"),
                        entry.get("components"),
                        entry.get("amount"),
                    )
                    else {
                        return 0;
                    };
                    let index = *slot as usize;
                    if index >= 9
                        || *amount <= 0
                        || *amount > 4000
                        || fluid_slots[index]
                        || slots[index].is_some()
                    {
                        return 0;
                    }
                    fluid_slots[index] = true;
                }
            }
            _ => return 0,
        }
        let mut left = limit.min(stack.count);
        let wanted = left;
        let max = self.res.items().stack_max(stack);
        for slot in &mut slots {
            if left == 0 {
                break;
            }
            if let Some(existing) = slot {
                if existing.same_item_same_components(stack) {
                    let n = left.min(max.saturating_sub(existing.count));
                    existing.count += n;
                    left -= n;
                }
            }
        }
        for (index, slot) in slots.iter_mut().enumerate() {
            if left == 0 {
                break;
            }
            if slot.is_none() && !fluid_slots[index] {
                let n = left.min(max);
                *slot = Some(ItemStack {
                    count: n,
                    ..stack.clone()
                });
                left -= n;
            }
        }
        let accepted = wanted - left;
        if accepted == 0 || simulate {
            return accepted;
        }
        let mut entries = Vec::new();
        for (slot, item) in slots.into_iter().enumerate() {
            if let Some(item) = item {
                let mut tag = horizon_client::save::item_to_nbt(&item);
                let Tag::Compound(ref mut map) = tag else {
                    return 0;
                };
                map.insert("Slot".into(), Tag::Byte(slot as i8));
                entries.push(tag);
            }
        }
        machine
            .config
            .insert("ae2_provider_return_items".into(), Tag::List(entries));
        if self.ae2_provider_buffer_commit(owner, &machine) {
            accepted
        } else {
            0
        }
    }
    /// Insertion-only shared GenericStack return buffer; fluid slots hold 4 buckets.
    pub(in crate::game) fn ae2_provider_return_fluid(
        &mut self,
        address: GridAddress,
        key: &ae2_cell_core::ItemKey,
        amount: u64,
        simulate: bool,
    ) -> Option<u64> {
        self.ae2_provider_return_fluid_owner(
            ProviderBufferOwner::Block(address),
            key,
            amount,
            simulate,
        )
    }
    fn ae2_provider_return_fluid_owner(
        &mut self,
        owner: ProviderBufferOwner,
        key: &ae2_cell_core::ItemKey,
        amount: u64,
        simulate: bool,
    ) -> Option<u64> {
        let Some(mut machine) = self.ae2_provider_buffer_snapshot(owner) else {
            return None;
        };
        if machine.config.get(&"ae2_provider_injecting".into()) == Some(&Tag::Byte(1)) {
            return Some(0);
        }
        if !super::super::ae2_fluids::valid_fluid_id(&key.item_id) {
            return Some(0);
        }
        let mut occupied = [false; 9];
        match machine.config.get(&"ae2_provider_return_items".into()) {
            None => {}
            Some(Tag::List(items)) => {
                for entry in items {
                    let Some(Tag::Byte(slot)) = entry.get("Slot") else {
                        return Some(0);
                    };
                    let i = *slot as usize;
                    if i >= 9 || occupied[i] || horizon_client::save::item_from_nbt(entry).is_none()
                    {
                        return Some(0);
                    }
                    occupied[i] = true;
                }
            }
            _ => return Some(0),
        }
        let mut slots: Vec<Option<(ae2_cell_core::ItemKey, u64)>> = vec![None; 9];
        match machine.config.get(&"ae2_provider_return_fluids".into()) {
            None => {}
            Some(Tag::List(fluids)) => {
                for entry in fluids {
                    let (
                        Some(Tag::Byte(slot)),
                        Some(Tag::String(id)),
                        Some(Tag::ByteArray(components)),
                        Some(Tag::Long(n)),
                    ) = (
                        entry.get("Slot"),
                        entry.get("fluid"),
                        entry.get("components"),
                        entry.get("amount"),
                    )
                    else {
                        return Some(0);
                    };
                    let i = *slot as usize;
                    let Ok(id) = id.to_string() else {
                        return Some(0);
                    };
                    if i >= 9
                        || occupied[i]
                        || *n <= 0
                        || *n > 4000
                        || !super::super::ae2_fluids::valid_fluid_id(&id)
                    {
                        return Some(0);
                    }
                    occupied[i] = true;
                    slots[i] = Some((
                        ae2_cell_core::ItemKey {
                            item_id: id,
                            components: components.clone(),
                        },
                        *n as u64,
                    ));
                }
            }
            _ => return Some(0),
        }
        let mut left = amount;
        for slot in &mut slots {
            if let Some((stored, n)) = slot {
                if stored == key {
                    let moved = left.min(4000 - *n);
                    *n += moved;
                    left -= moved;
                }
            }
        }
        for (i, slot) in slots.iter_mut().enumerate() {
            if left == 0 {
                break;
            }
            if !occupied[i] {
                let n = left.min(4000);
                *slot = Some((key.clone(), n));
                left -= n;
            }
        }
        let accepted = amount - left;
        if accepted == 0 || simulate {
            return Some(accepted);
        }
        let entries = slots
            .into_iter()
            .enumerate()
            .filter_map(|(i, v)| {
                v.map(|(key, n)| {
                    Tag::compound([
                        ("Slot", Tag::Byte(i as i8)),
                        ("fluid", Tag::String(key.item_id.as_str().into())),
                        ("components", Tag::ByteArray(key.components)),
                        ("amount", Tag::Long(n as i64)),
                    ])
                })
            })
            .collect();
        machine
            .config
            .insert("ae2_provider_return_fluids".into(), Tag::List(entries));
        Some(if self.ae2_provider_buffer_commit(owner, &machine) {
            accepted
        } else {
            0
        })
    }
    /// Preserve the enclosing grid transaction when an external capability is
    /// an unconfigured Interface forwarding into another storage service.
    pub(in crate::game) fn ae2_native_item_insert_staged(
        &mut self, address: GridAddress, face: Direction, stack: &ItemStack,
        limit: u32, simulate: bool, plan: &mut crate::game::ae2_buses::FormationPlan,
    ) -> u32 {
        if matches!(self.ae2_block_entity(address), Some(BlockEntity::Ae2(machine)) if machine.kind == MachineKind::QuantumLink) {
            let Some((moved, changed)) = self.ae2_native_container_insert_staged(address, face, stack, limit, simulate) else { return 0 };
            plan.actions.extend(changed.into_iter().map(|(address, incarnation)| crate::game::ae2_formation::FormationAction::ContainerChanged { address, incarnation }));
            return moved;
        }
        if self.ae2_has_interface_face(address, face) {
            return self.ae2_interface_item_insert_staged(address, face, stack, limit, simulate, plan).unwrap_or(0);
        }
        self.ae2_native_item_insert(address, face, stack, limit, simulate)
    }
    pub(in crate::game) fn ae2_native_item_extract_staged(
        &mut self, address: GridAddress, face: Direction, limit: u32,
        filter: Option<&ItemStack>, simulate: bool, plan: &mut crate::game::ae2_buses::FormationPlan,
    ) -> Option<ItemStack> {
        if matches!(self.ae2_block_entity(address), Some(BlockEntity::Ae2(machine)) if machine.kind == MachineKind::QuantumLink) {
            let prototype = self.ae2_native_container_candidates(address, face, true)?
                .into_iter().find(|stack| filter.is_none_or(|f| f.same_item_same_components(stack)))?;
            let (out, changed) = self.ae2_native_container_extract_staged(address, face, &prototype, limit, simulate)?;
            plan.actions.extend(changed.into_iter().map(|(address, incarnation)| crate::game::ae2_formation::FormationAction::ContainerChanged { address, incarnation }));
            return out;
        }
        if self.ae2_has_interface_face(address, face) {
            return self.ae2_interface_item_extract_staged(address, face, limit, filter, simulate, plan).flatten();
        }
        self.ae2_native_item_extract(address, face, limit, filter, simulate)
    }
    pub(in crate::game) fn ae2_native_item_insert(
        &mut self,
        address: GridAddress,
        face: Direction,
        stack: &ItemStack,
        limit: u32,
        simulate: bool,
    ) -> u32 {
        if let Some(accepted)=self.ae2_assembler_insert(address,stack,limit,simulate) {return accepted;}
        if let Some(accepted)=self.ae2_inscriber_insert(address,face,stack,limit,simulate) {return accepted;}
        if let Some(accepted)=self.ae2_processor_item_insert(address,stack,limit,simulate) {return accepted;}
        if matches!(self.ae2_block_entity(address),Some(BlockEntity::Ae2(machine)) if machine.kind==MachineKind::Condenser) {return self.ae2_condenser_insert_at(address,stack,limit,simulate);}
        if let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(address) {
            if machine.kind == MachineKind::DebugItemGenerator {
                let max = self.res.items().stack_max(stack).min(64);
                let mut slots = machine.slots.clone();
                let moved = crate::game::ae2_debug::itemgen_insert(&mut slots, stack, limit, max);
                if !simulate && moved > 0 {
                    if let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity_mut(address) { machine.slots = slots; }
                    else { return 0; }
                }
                return moved;
            }
        }
        if let Some(moved) = self.ae2_interface_item_insert(address, face, stack, limit, simulate) {
            return moved;
        }
        if let Some(part) = self.ae2_provider_part_at(address, face) {
            return self.ae2_part_provider_return_item(part, stack, limit, simulate);
        }
        if matches!(self.ae2_block_entity(address), Some(BlockEntity::Ae2(m)) if m.kind == MachineKind::PatternProvider)
        {
            return self.ae2_provider_return_item(address, stack, limit, simulate);
        }
        let Some((c, view, dim)) = self.ae2_native_view(address) else {
            return 0;
        };
        let mut left = stack.count.min(limit);
        let wanted = left;
        let mut changes = Vec::new();
        for slot in view.slots_for_face(Some(face)) {
            if left == 0 {
                break;
            }
            if !c.can_place(&view, slot, stack) {
                continue;
            }
            let Some((owner, index)) = view.locate(slot) else {
                continue;
            };
            let Some(owner_address) = self.ae2_dimension_address(dim, owner) else {
                continue;
            };
            let mut max = c.max_stack(stack);
            let state = c.world.block(owner.0, owner.1, owner.2);
            let name = c.name(state);
            if name.ends_with("shulker_box") && stack.item.ends_with("shulker_box") {
                continue;
            }
            if let Some(BlockEntity::Ae2(m)) = c.world.block_entities.get(&owner) {
                match m.kind {
                    MachineKind::Drive | MachineKind::Chest => {
                        if !crate::game::ae2_storage_devices::is_valid_storage_cell(stack) {
                            continue;
                        }
                        max = 1;
                    }
                    MachineKind::IoPort => {
                        if index >= 6
                            || !crate::game::ae2_storage_devices::is_valid_storage_cell(stack)
                        {
                            continue;
                        }
                        max = 1;
                    }
                    MachineKind::QuantumLink => {
                        if index != 0 || crate::game::ae2_quantum::frequency(stack).is_none() { continue; }
                        max = 1;
                    }
                    _ => {}
                }
            }
            let old = c.get(&view, slot).cloned();
            let accepted = match &old {
                None => left.min(max),
                Some(old) if old.same_item_same_components(stack) => {
                    left.min(max.saturating_sub(old.count))
                }
                _ => 0,
            };
            if accepted == 0 {
                continue;
            }
            let new = Some(match &old {
                None => ItemStack {
                    count: accepted,
                    ..stack.clone()
                },
                Some(old) => ItemStack {
                    count: old.count + accepted,
                    ..old.clone()
                },
            });
            changes.push((owner_address, index, old, new));
            left -= accepted;
        }
        let accepted = wanted - left;
        if simulate || accepted == 0 {
            return accepted;
        }
        if changes.iter().any(|(owner, slot, old, _)| {
            self.ae2_block_entity(*owner)
                .and_then(|b| b.slots().get(*slot))
                != Some(old)
        }) {
            return 0;
        }
        let mut changed = Vec::new();
        for (owner, slot, _, new) in changes {
            if let Some(value) = self
                .ae2_block_entity_mut(owner)
                .and_then(|b| b.slots_mut().get_mut(slot))
            {
                *value = new;
                self.ae2_mark_modified(owner);
                changed.push(owner);
            }
        }
        self.ae2_native_notify(&changed);
        accepted
    }
    pub(in crate::game) fn ae2_native_item_candidates(
        &self,
        address: GridAddress,
        face: Direction,
    ) -> Vec<ItemStack> {
        if let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(address) {
            if machine.kind==MachineKind::MolecularAssembler {return machine.slots.get(9).and_then(Option::as_ref).filter(|s|s.count>0).cloned().into_iter().collect();}
            if machine.kind==MachineKind::Inscriber {
                let Some(slots)=self.ae2_inscriber_faces(address,face,machine) else{return Vec::new()};
                let separate=slots.len()!=4;let smash=matches!(machine.config.get(&"ae2_smash".into()),Some(Tag::Int(n)) if *n>0);
                return slots.into_iter().filter(|n|*n==3||(separate&&!smash&&*n<2)).filter_map(|n|machine.slots.get(n)?.as_ref().filter(|s|s.count>0).cloned()).collect();
            }
            if matches!(machine.kind,MachineKind::Charger|MachineKind::VibrationChamber) {
                return machine.slots.first().and_then(Option::as_ref).filter(|s|s.count>0 && self.ae2_processor_slot_policy(address,s,false)==Some(true)).cloned().into_iter().collect();
            }
            if machine.kind == MachineKind::DebugItemGenerator { return machine.slots.iter().flatten().cloned().collect(); }
            if machine.kind==MachineKind::Condenser {return machine.slots.first().and_then(Option::as_ref).filter(|stack|stack.count>0).cloned().into_iter().collect();}
        }
        if let Some(items) = self.ae2_interface_item_candidates(address, face) {
            return items;
        }
        let Some((c, view, _)) = self.ae2_native_view(address) else {
            return Vec::new();
        };
        view.slots_for_face(Some(face)).into_iter().filter_map(|slot|{
            let stack=c.get(&view,slot).filter(|s|s.count>0)?;
            if !c.can_take(&view,slot,stack,face){return None;}
            let(owner,index)=view.locate(slot)?;
            if matches!(c.world.block_entities.get(&owner),Some(BlockEntity::Ae2(m))if m.kind==MachineKind::IoPort&&index<6){return None;}
            Some(stack.clone())
        }).collect()
    }
    pub(in crate::game) fn ae2_native_item_extract(
        &mut self,
        address: GridAddress,
        face: Direction,
        limit: u32,
        filter: Option<&ItemStack>,
        simulate: bool,
    ) -> Option<ItemStack> {
        if let Some(extracted)=self.ae2_assembler_extract(address,limit,filter,simulate) {return extracted;}
        if let Some(extracted)=self.ae2_inscriber_extract(address,face,limit,filter,simulate) {return extracted;}
        if let Some(extracted)=self.ae2_processor_item_extract(address,limit,filter,simulate) {return extracted;}
        if matches!(self.ae2_block_entity(address),Some(BlockEntity::Ae2(machine)) if machine.kind==MachineKind::Condenser) {return self.ae2_condenser_extract_at(address,limit,filter,simulate);}
        if let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(address) {
            if machine.kind == MachineKind::DebugItemGenerator {
                if limit == 0 { return None; }
                let (slot, stack) = machine.slots.iter().enumerate().find_map(|(i,s)| s.as_ref().filter(|s| s.count > 0 && filter.is_none_or(|f| f.same_item_same_components(s))).map(|s| (i,s.clone())))?;
                let mut out = stack.clone();
                out.count = out.count.min(limit);
                if !simulate {
                    let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity_mut(address) else { return None };
                    if machine.slots.get(slot)?.as_ref() != Some(&stack) { return None; }
                    machine.slots[slot] = if out.count == stack.count { None } else { let mut remain = stack; remain.count -= out.count; Some(remain) };
                    self.ae2_debug_refill_slot(address, slot);
                }
                return Some(out);
            }
        }
        if let Some(item) = self.ae2_interface_item_extract(address, face, limit, filter, simulate)
        {
            return item;
        }
        if limit == 0 {
            return None;
        }
        let (c, view, dim) = self.ae2_native_view(address)?;
        let mut selected = None;
        for slot in view.slots_for_face(Some(face)) {
            let Some(stack) = c.get(&view, slot).filter(|s| s.count > 0) else {
                continue;
            };
            if !c.can_take(&view, slot, stack, face)
                || filter.is_some_and(|f| !f.same_item_same_components(stack))
            {
                continue;
            }
            let (owner, index) = view.locate(slot)?;
            if let Some(BlockEntity::Ae2(m)) = c.world.block_entities.get(&owner) {
                if m.kind == MachineKind::IoPort && index < 6 {
                    continue;
                }
            }
            selected = Some((
                self.ae2_dimension_address(dim, owner)?,
                index,
                stack.clone(),
            ));
            break;
        }
        let (owner, index, stack) = selected?;
        let out = ItemStack {
            count: stack.count.min(limit),
            ..stack.clone()
        };
        if simulate {
            return Some(out);
        }
        let value = self
            .ae2_block_entity_mut(owner)?
            .slots_mut()
            .get_mut(index)?;
        if value.as_ref() != Some(&stack) {
            return None;
        }
        *value = if out.count == stack.count {
            None
        } else {
            Some(ItemStack {
                count: stack.count - out.count,
                ..stack
            })
        };
        self.ae2_mark_modified(owner);
        self.ae2_native_notify(&[owner]);
        Some(out)
    }
}
// Source filters for the one-slot Charger and Vibration Chamber. Inscriber
// and Assembler use their own recipe/plan and side filters below.
impl Game<'_> {
    fn ae2_processor_slot_policy(&self, address: GridAddress, stack: &ItemStack, insertion: bool) -> Option<bool> {
        let BlockEntity::Ae2(machine)=self.ae2_block_entity(address)? else { return None };
        match machine.kind {
            MachineKind::Charger => {
                let recipe=super::super::ae2_processing::live_charger_recipe(stack,self.res.items(),self.res.recipes()).is_some();
                let chargeable=super::super::ae2_processing::charger_chargeable(stack);
                if insertion {return Some(chargeable||recipe)}
                let energy=if super::super::ae2_wireless::is_wireless(stack) {
                    super::super::ae2_wireless::energy(stack)
                } else if horizon_client::ae2_powered_items::profile(stack).is_some() {
                    horizon_client::ae2_powered_items::energy(stack)
                } else if super::super::ae2_processing::is_energy_cell_item(stack) {
                    super::super::ae2_processing::energy_cell_energy(stack)
                } else { super::super::ae2_interactions::portable_energy(stack) };
                Some(energy.is_ok_and(|(current,max)|current>=max) || !recipe)
            }
            MachineKind::VibrationChamber => {
                let fuel=self.res.providers().burn_time(self.res.items(),stack,false)>0;
                Some(if insertion {fuel} else {!fuel})
            }
            _ => None,
        }
    }
    fn ae2_processor_item_insert(&mut self,address:GridAddress,stack:&ItemStack,limit:u32,simulate:bool)->Option<u32> {
        if !self.ae2_processor_slot_policy(address,stack,true)? {return Some(0)}
        let BlockEntity::Ae2(machine)=self.ae2_block_entity(address)? else {return None};
        let old=machine.slots.first()?.clone();
        let max=if machine.kind==MachineKind::Charger {1} else {self.res.items().stack_max(stack).min(64)};
        let accepted=match old.as_ref(){None=>limit.min(stack.count).min(max),Some(current) if current.same_item_same_components(stack)=>limit.min(stack.count).min(max.saturating_sub(current.count)),_=>0};
        if simulate||accepted==0{return Some(accepted)}
        let BlockEntity::Ae2(machine)=self.ae2_block_entity_mut(address)? else{return Some(0)};
        if machine.slots.first()!=Some(&old){return Some(0)}
        machine.slots[0]=Some(match old {Some(mut current)=>{current.count+=accepted;current},None=>{let mut inserted=stack.clone();inserted.count=accepted;horizon_client::ae2_stack_identity::detach_if_tracked(&mut inserted);inserted}});
        self.ae2_mark_modified(address);self.ae2_native_notify(&[address]);Some(accepted)
    }
    fn ae2_processor_item_extract(&mut self,address:GridAddress,limit:u32,filter:Option<&ItemStack>,simulate:bool)->Option<Option<ItemStack>> {
        let BlockEntity::Ae2(machine)=self.ae2_block_entity(address)? else{return None};
        if !matches!(machine.kind,MachineKind::Charger|MachineKind::VibrationChamber){return None}
        let Some(old)=machine.slots.first().and_then(Option::as_ref).cloned() else{return Some(None)};
        if limit==0 || filter.is_some_and(|f|!f.same_item_same_components(&old)) || !self.ae2_processor_slot_policy(address,&old,false).unwrap_or(false){return Some(None)}
        let mut out=old.clone();out.count=out.count.min(limit);horizon_client::ae2_stack_identity::detach_if_tracked(&mut out);
        if simulate{return Some(Some(out))}
        let Some(BlockEntity::Ae2(machine))=self.ae2_block_entity_mut(address) else{return Some(None)};
        if machine.slots.first().and_then(Option::as_ref)!=Some(&old){return Some(None)}
        let mut remaining=old;remaining.count-=out.count;
        horizon_client::ae2_stack_identity::synchronize_tracked(&remaining);
        machine.slots[0]=(remaining.count>0).then_some(remaining);
        self.ae2_mark_modified(address);self.ae2_native_notify(&[address]);Some(Some(out))
    }
}

// Pinned InscriberBlockEntity BaseFilter/AutomationFilter. Input validation
// examines partial physical recipes, not a finished result or ghost filter.
impl Game<'_> {
    pub(in crate::game) fn ae2_inscriber_faces(&self,address:GridAddress,face:Direction,m:&horizon_client::ae2_world::Machine)->Option<Vec<usize>> {
        let separate=match m.config.get(&"inscriber_separate_sides".into()) {
            None=>false,Some(Tag::String(s))=>match s.to_string().ok()?.as_str(){"NO"=>false,"YES"=>true,_=>return None},_=>return None,
        };
        if !separate{return Some(vec![0,1,2,3])}
        let state=self.ae2_grid_block(address).and_then(|id|self.pack.assets.registry.state(id))?;
        let front=state.properties.get("facing")?.as_str();let spin:usize=state.properties.get("spin")?.parse().ok()?;
        let up=orientation_up(front,spin)?;
        Some(if face==up {vec![0]} else if face==up.opposite(){vec![1]} else{vec![2,3]})
    }
    pub(in crate::game) fn ae2_inscriber_accepts(&self,m:&horizon_client::ae2_world::Machine,slot:usize,stack:&ItemStack)->bool {
        if slot>=3 || m.slots.len()!=4 {return false}
        if slot<2 && stack.item.as_ref()=="ae2:name_press" {return true}
        let mut inputs=m.slots[..3].to_vec();inputs[slot]=Some(stack.clone());
        if slot==2 && inputs[..2].iter().flatten().any(|s|s.item.as_ref()=="ae2:name_press"){return true}
        let check=super::super::ae2_processing::json_ingredient;
        let (top,bottom,middle)=(&inputs[0],&inputs[1],&inputs[2]);
        self.res.recipes().ae2_custom.get("ae2:inscriber").is_some_and(|recipes|recipes.iter().any(|(_,v)|{
            let Some(ingredients)=v.get("ingredients") else{return false};
            if middle.is_some() && !check(self.res.items(),ingredients.get("middle"),middle.as_ref()){return false}
            match (top.as_ref(),bottom.as_ref()) {
                (None,None)=>true,
                (Some(a),None)|(None,Some(a))=>check(self.res.items(),ingredients.get("top"),Some(a))||check(self.res.items(),ingredients.get("bottom"),Some(a)),
                (Some(a),Some(b))=>(check(self.res.items(),ingredients.get("top"),Some(a))&&check(self.res.items(),ingredients.get("bottom"),Some(b)))||(check(self.res.items(),ingredients.get("bottom"),Some(a))&&check(self.res.items(),ingredients.get("top"),Some(b))),
            }
        }))
    }
    fn ae2_inscriber_insert(&mut self,address:GridAddress,face:Direction,stack:&ItemStack,limit:u32,simulate:bool)->Option<u32>{
        let BlockEntity::Ae2(m)=self.ae2_block_entity(address)? else{return None};if m.kind!=MachineKind::Inscriber{return None}if m.slots.len()!=4{return Some(0)}
        if matches!(m.config.get(&"ae2_smash".into()),Some(Tag::Int(n)) if *n>0){return Some(0)}
        let slots=self.ae2_inscriber_faces(address,face,m)?;let mut staged=m.slots.clone();let old=staged.clone();let mut left=limit.min(stack.count);
        let cap=match m.config.get(&"inscriber_input_capacity".into()){Some(Tag::String(s))=>match s.to_string().ok()?.as_str(){"ONE"=>1,"FOUR"=>4,"SIXTY_FOUR"=>64,_=>return None},None=>64,_=>return None};
        let max=cap.min(self.res.items().stack_max(stack));
        for slot in slots.into_iter().filter(|n|*n<3){if left==0{break}let mut current=(**m).clone();current.slots=staged.clone();if !self.ae2_inscriber_accepts(&current,slot,stack){continue}
            let moved=match staged[slot].as_ref(){None=>left.min(max),Some(s) if s.same_item_same_components(stack)=>left.min(max.saturating_sub(s.count)),_=>0};if moved==0{continue}
            if let Some(s)=staged[slot].as_mut(){s.count+=moved}else{let mut s=stack.clone();s.count=moved;horizon_client::ae2_stack_identity::detach_if_tracked(&mut s);staged[slot]=Some(s)}left-=moved;
        }
        let accepted=limit.min(stack.count)-left;if simulate||accepted==0{return Some(accepted)}
        let Some(BlockEntity::Ae2(m))=self.ae2_block_entity_mut(address) else{return Some(0)};if m.slots!=old{return Some(0)}m.slots=staged;self.ae2_mark_modified(address);self.ae2_native_notify(&[address]);Some(accepted)
    }
    fn ae2_inscriber_extract(&mut self,address:GridAddress,face:Direction,limit:u32,filter:Option<&ItemStack>,simulate:bool)->Option<Option<ItemStack>>{
        let BlockEntity::Ae2(m)=self.ae2_block_entity(address)? else{return None};if m.kind!=MachineKind::Inscriber{return None}
        let slots=self.ae2_inscriber_faces(address,face,m)?;let separate=slots.len()!=4;let smash=matches!(m.config.get(&"ae2_smash".into()),Some(Tag::Int(n)) if *n>0);
        let Some((slot,old))=slots.into_iter().filter(|n|*n==3||(separate&&!smash&&*n<2)).find_map(|i|m.slots.get(i)?.as_ref().filter(|s|s.count>0&&filter.is_none_or(|f|f.same_item_same_components(s))).map(|s|(i,s.clone()))) else{return Some(None)};
        if limit==0{return Some(None)}let mut out=old.clone();out.count=out.count.min(limit);horizon_client::ae2_stack_identity::detach_if_tracked(&mut out);if simulate{return Some(Some(out))}
        let Some(BlockEntity::Ae2(m))=self.ae2_block_entity_mut(address) else{return Some(None)};if m.slots.get(slot).and_then(Option::as_ref)!=Some(&old){return Some(None)}let mut remain=old;remain.count-=out.count;horizon_client::ae2_stack_identity::synchronize_tracked(&remain);m.slots[slot]=(remain.count>0).then_some(remain);self.ae2_mark_modified(address);self.ae2_native_notify(&[address]);Some(Some(out))
    }
}

// The Assembler exposes its nine physical recipe inputs and output, never its
// pattern slot or fluid/CPU escrow. A physical installed pattern is required.
impl Game<'_> {
    pub(in crate::game) fn ae2_assembler_pattern_valid(&self,pattern:&ItemStack)->bool {
        super::super::ae2_processing::assembler_pattern_valid(pattern,self.res.items(),self.res.recipes(),self.res.stations())
    }
    /// The menu uses the active forced definition when a Provider owns the
    /// plan. Older snapshots lack that metadata and remain exact-only.
    pub(in crate::game) fn ae2_assembler_menu_accepts(&self,m:&horizon_client::ae2_world::Machine,slot:usize,stack:&ItemStack)->bool {
        if slot>=9 || m.slots.len()!=11 || stack.count==0{return false;}
        if let Some(snapshot)=m.config.get(&"ae2_assembler_pattern".into()) {
            if let Some(definition)=snapshot.get("definition") {
                return horizon_client::save::item_from_nbt(definition).is_some_and(|pattern|
                    super::super::ae2_processing::assembler_pattern_accepts(&pattern,slot,stack,self.res.items(),self.res.recipes(),self.res.stations()));
            }
            return horizon_client::save::decode_items(snapshot.get("grid"),9).get(slot).and_then(Option::as_ref)
                .is_some_and(|expected|expected.same_item_same_components(stack));
        }
        self.ae2_assembler_accepts(m,slot,stack)
    }
    pub(in crate::game) fn ae2_assembler_menu_slot_enabled(&self,m:&horizon_client::ae2_world::Machine,slot:usize)->bool {
        if slot>=9 || m.slots.len()!=11{return false;}
        if m.slots[slot].as_ref().is_some_and(|stack|stack.count>0){return true;}
        if let Some(snapshot)=m.config.get(&"ae2_assembler_pattern".into()) {
            if let Some(definition)=snapshot.get("definition") {
                return horizon_client::save::item_from_nbt(definition).is_some_and(|pattern|
                    super::super::ae2_processing::assembler_pattern_slot_enabled(&pattern,slot,self.res.items(),self.res.recipes(),self.res.stations()));
            }
            return horizon_client::save::decode_items(snapshot.get("grid"),9).get(slot).is_some_and(Option::is_some);
        }
        m.slots[10].as_ref().is_some_and(|pattern|
            super::super::ae2_processing::assembler_pattern_slot_enabled(pattern,slot,self.res.items(),self.res.recipes(),self.res.stations()))
    }
    pub(in crate::game) fn ae2_assembler_accepts(&self,m:&horizon_client::ae2_world::Machine,slot:usize,stack:&ItemStack)->bool {
        if slot>=9 || m.slots.len()!=11 {return false;}
        let Some(pattern)=m.slots[10].as_ref() else{return false;};
        // CraftingGridFilter requires an installed pattern, but validates
        // against myPlan, which can still be a provider-forced definition.
        if m.config.contains_key(&"ae2_assembler_pattern".into()) {
            return self.ae2_assembler_menu_accepts(m,slot,stack);
        }
        super::super::ae2_processing::assembler_pattern_accepts(pattern,slot,stack,self.res.items(),self.res.recipes(),self.res.stations())
    }
    fn ae2_assembler_insert(&mut self,address:GridAddress,stack:&ItemStack,limit:u32,simulate:bool)->Option<u32>{
        let BlockEntity::Ae2(m)=self.ae2_block_entity(address)? else{return None};if m.kind!=MachineKind::MolecularAssembler{return None}if m.slots.len()!=11{return Some(0)}
        let old=m.slots.clone();let mut staged=old.clone();let mut left=limit.min(stack.count);let max=1;
        for slot in 0..9{if left==0{break}if !self.ae2_assembler_accepts(m,slot,stack){continue}let moved=match staged[slot].as_ref(){None=>left.min(max),Some(s) if s.same_item_same_components(stack)=>left.min(max.saturating_sub(s.count)),_=>0};if moved==0{continue}
            if let Some(s)=staged[slot].as_mut(){s.count+=moved}else{let mut s=stack.clone();s.count=moved;horizon_client::ae2_stack_identity::detach_if_tracked(&mut s);staged[slot]=Some(s)}left-=moved;
        }
        let accepted=limit.min(stack.count)-left;if simulate||accepted==0{return Some(accepted)}let Some(BlockEntity::Ae2(m))=self.ae2_block_entity_mut(address)else{return Some(0)};if m.slots!=old{return Some(0)}m.slots=staged;self.ae2_mark_modified(address);self.ae2_native_notify(&[address]);Some(accepted)
    }
    fn ae2_assembler_extract(&mut self,address:GridAddress,limit:u32,filter:Option<&ItemStack>,simulate:bool)->Option<Option<ItemStack>>{
        let BlockEntity::Ae2(m)=self.ae2_block_entity(address)? else{return None};if m.kind!=MachineKind::MolecularAssembler{return None}
        let Some(old)=m.slots.get(9).and_then(Option::as_ref).filter(|s|s.count>0&&filter.is_none_or(|f|f.same_item_same_components(s))).cloned() else{return Some(None)};if limit==0{return Some(None)}
        let mut out=old.clone();out.count=out.count.min(limit);horizon_client::ae2_stack_identity::detach_if_tracked(&mut out);if simulate{return Some(Some(out))}let Some(BlockEntity::Ae2(m))=self.ae2_block_entity_mut(address) else{return Some(None)};if m.slots.get(9).and_then(Option::as_ref)!=Some(&old){return Some(None)}let mut remain=old;remain.count-=out.count;horizon_client::ae2_stack_identity::synchronize_tracked(&remain);m.slots[9]=(remain.count>0).then_some(remain);self.ae2_mark_modified(address);self.ae2_native_notify(&[address]);Some(Some(out))
    }
}

#[cfg(test)]
mod native_sided_slot_tests {
    use super::*;
    #[test]
    fn brewing_bottles_and_new_furnace_buckets_obey_sided_limits() {
        assert_eq!(native_sided_slot_limit(Kind::Brewing,0,"minecraft:glass_bottle",false,64),1);
        assert_eq!(native_sided_slot_limit(Kind::Brewing,2,"minecraft:glass_bottle",true,64),1);
        assert_eq!(native_sided_slot_limit(Kind::Brewing,3,"minecraft:glass_bottle",false,64),64);
        assert_eq!(native_sided_slot_limit(Kind::Furnace,1,"minecraft:bucket",false,16),1);
        assert_eq!(native_sided_slot_limit(Kind::Furnace,1,"minecraft:bucket",true,16),16);
        assert_eq!(native_sided_slot_limit(Kind::Furnace,1,"minecraft:coal",false,64),64);
        assert_eq!(native_sided_slot_limit(Kind::Generic,0,"minecraft:stone",false,99),99);
    }
    #[test]
    fn io_port_top_and_bottom_take_cells_in_and_only_sides_give_them_out() {
        // IOPortBlockEntity.getExposedInventoryForSide, relative to getTop.
        let port=|up|View{parts:vec![((0,0,0),12)],kind:Kind::IoPort(up),container:None};
        let (inputs,outputs):(Vec<usize>,Vec<usize>)=((0..6).collect(),(6..12).collect());
        let standing=port(orientation_up("north",0).unwrap());
        for face in [Direction::Up,Direction::Down] {assert_eq!(standing.slots_for_face(Some(face)),inputs);}
        for face in [Direction::North,Direction::South,Direction::West,Direction::East] {assert_eq!(standing.slots_for_face(Some(face)),outputs);}
        // UP_NORTH: the top looks north.
        let lying=port(orientation_up("up",0).unwrap());
        for face in [Direction::North,Direction::South] {assert_eq!(lying.slots_for_face(Some(face)),inputs);}
        for face in [Direction::Up,Direction::Down,Direction::West,Direction::East] {assert_eq!(lying.slots_for_face(Some(face)),outputs);}
        // inputCellsExt is INSERT_ONLY, outputCellsExt EXTRACT_ONLY.
        let cell=ItemStack::new("ae2:item_storage_cell_1k",1);
        assert_eq!(ae2_slot_insert(MachineKind::IoPort,0,&cell),Some((true,1)));
        assert_eq!(ae2_slot_insert(MachineKind::IoPort,6,&cell),Some((false,1)));
        assert_eq!(ae2_slot_extract(MachineKind::IoPort,5),Some(false));
        assert_eq!(ae2_slot_extract(MachineKind::IoPort,6),Some(true));
    }
    #[test]
    fn drive_bays_take_only_storage_cells_one_to_a_bay() {
        // DriveBlockEntity.CellValidInventoryFilter over AppEngCellInventory (max 1).
        let cobblestone=ItemStack::new("minecraft:cobblestone",1);
        let cell=ItemStack::new("ae2:item_storage_cell_1k",1);
        assert_eq!(ae2_slot_insert(MachineKind::Drive,0,&cobblestone),Some((false,1)));
        assert_eq!(ae2_slot_insert(MachineKind::Drive,9,&cell),Some((true,1)));
        // allowExtract is always true.
        assert_eq!(ae2_slot_extract(MachineKind::Drive,0),None);
    }
}
