//! Item stock maintenance for AE2 InterfaceLogic (AE2 19.2.18, LGPL-3.0-or-later).
//! Config entries are ghost stacks; slot contents are real owned items.
use super::ae2_global::GridAddress;
use super::{Game, ae2_lab, ae2_parts::PartAddress};
use ae2_cell_core::ItemKey;
use horizon_client::{
    ae2_world::{Machine, MachineKind},
    containers::BlockEntity,
    items::ItemStack,
    save,
};
use horizon_world::nbt::Tag;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Block(GridAddress),
    Part(PartAddress),
}
impl Owner {
    fn host(self) -> GridAddress {
        match self {
            Self::Block(a) => a,
            Self::Part(a) => a.host,
        }
    }
}
/// Persistent requester identity: a block or one face-owned interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum InterfaceCraftOwner {
    Block(GridAddress),
    Part { host: GridAddress, face: u8 },
}
impl InterfaceCraftOwner {
    pub(super) fn anchor(self) -> GridAddress {
        match self {
            Self::Block(host) | Self::Part { host, .. } => host,
        }
    }
    fn owner(self) -> Option<Owner> {
        match self {
            Self::Block(host) => Some(Owner::Block(host)),
            Self::Part { host, face } if face < 6 => Some(Owner::Part(PartAddress { host, face })),
            _ => None,
        }
    }
}
impl From<Owner> for InterfaceCraftOwner {
    fn from(owner: Owner) -> Self {
        match owner {
            Owner::Block(host) => Self::Block(host),
            Owner::Part(p) => Self::Part {
                host: p.host,
                face: p.face,
            },
        }
    }
}
const CRAFT_LINKS: &str = "ae2_interface_craft_links";
const CANCELLED_LINKS: &str = "ae2_interface_cancelled_links";
/// Persist the cancellation event even if a CPU's dimension is not loaded.
/// Reinstalling the card must not revive an old outstanding job.
pub(super) fn mark_card_removed(machine: &mut Machine) {
    if machine.kind != MachineKind::Interface {
        return;
    }
    super::ae2_autocrafting::advance_crafting_card_generation(&mut machine.config);
    let mask = craft_links(machine)
        .map(|links| {
            links.iter().enumerate().fold(0i32, |mask, (slot, link)| {
                mask | if link.is_some() { 1 << slot } else { 0 }
            })
        })
        .unwrap_or(0x1ff);
    let old = match machine.config.get(&CANCELLED_LINKS.into()) {
        Some(Tag::Int(value)) => *value,
        _ => 0,
    };
    machine
        .config
        .insert(CANCELLED_LINKS.into(), Tag::Int(old | mask));
}
fn craft_link_cancelled(machine: &Machine, slot: usize) -> bool {
    match machine.config.get(&CANCELLED_LINKS.into()) {
        None => false,
        Some(Tag::Int(mask)) => mask & (1 << slot) != 0,
        _ => true,
    }
}
fn craft_links(machine: &Machine) -> Result<[Option<GridAddress>; 9], String> {
    let mut out = [None; 9];
    let Some(tag) = machine.config.get(&CRAFT_LINKS.into()) else {
        return Ok(out);
    };
    let Tag::List(links) = tag else {
        return Err("invalid interface crafting links".into());
    };
    for entry in links {
        let (Some(Tag::Byte(slot)), Some(Tag::IntArray(pos))) =
            (entry.get("Slot"), entry.get("cpu"))
        else {
            return Err("invalid interface crafting link".into());
        };
        let slot = *slot as usize;
        if slot >= 9 || out[slot].is_some() || pos.len() != 3 {
            return Err("duplicate or invalid interface crafting link".into());
        }
        let address = GridAddress::from_network(ae2_network_core::Pos {
            x: pos[0],
            y: pos[1],
            z: pos[2],
        })
        .ok_or("invalid interface CPU address")?;
        out[slot] = Some(address);
    }
    Ok(out)
}
fn store_craft_links(machine: &mut Machine, links: &[Option<GridAddress>; 9]) {
    let entries = links
        .iter()
        .enumerate()
        .filter_map(|(i, address)| {
            let (x, y, z) = (*address)?.encoded();
            Some(Tag::compound([
                ("Slot", Tag::Byte(i as i8)),
                ("cpu", Tag::IntArray(vec![x, y, z])),
            ]))
        })
        .collect();
    machine
        .config
        .insert(CRAFT_LINKS.into(), Tag::List(entries));
}
fn has_crafting_card(machine: &Machine) -> bool {
    machine
        .upgrades
        .iter()
        .flatten()
        .any(|item| item.item.as_ref() == "ae2:crafting_card")
}
fn active_craft_link(machine: &Machine, slot: usize, cpu: GridAddress) -> bool {
    slot < 9
        && has_crafting_card(machine)
        && !craft_link_cancelled(machine, slot)
        && craft_links(machine).ok().and_then(|links| links[slot]) == Some(cpu)
}
const CONFIG: &str = "ae2_interface_config";
pub(super) fn interface_setting_click(machine: &mut Machine, slot: usize, button: i32) -> bool {
    if machine.kind != MachineKind::Interface || !(0..=1).contains(&button) {
        return false;
    }
    if (19..=22).contains(&slot) {
        let amount = [1i32, 10, 100, 1000][slot - 19];
        let old = interface_priority(machine);
        let next = if button == 0 {
            old.saturating_add(amount)
        } else {
            old.saturating_sub(amount)
        };
        machine.config.insert("priority".into(), Tag::Int(next));
    } else if slot == 23 {
        let choices = [
            "ignore_all",
            "percent_99",
            "percent_75",
            "percent_50",
            "percent_25",
        ];
        let current = match machine.config.get(&"ae2_fuzzy_mode".into()) {
            Some(Tag::String(s)) => s.to_string().unwrap_or_default(),
            _ => String::new(),
        };
        let index = choices
            .iter()
            .position(|value| *value == current)
            .unwrap_or(0);
        let next = if button == 0 {
            (index + 1) % choices.len()
        } else {
            (index + choices.len() - 1) % choices.len()
        };
        machine
            .config
            .insert("ae2_fuzzy_mode".into(), Tag::String(choices[next].into()));
    } else {
        return false;
    }
    wake_interface(machine);
    true
}
pub(super) fn wake_interface(machine: &mut Machine) {
    if machine.kind == MachineKind::Interface {
        machine
            .config
            .insert("ae2_interface_next_tick".into(), Tag::Long(0));
    }
}
pub(super) fn interface_config(machine: &Machine) -> Result<Vec<Option<ItemStack>>, String> {
    let Some(tag) = machine.config.get(&CONFIG.into()) else {
        return Ok(vec![None; 9]);
    };
    let Tag::List(entries) = tag else {
        return Err("invalid interface config".into());
    };
    let mut seen = [false; 9];
    for entry in entries {
        let Some(Tag::Byte(slot)) = entry.get("Slot") else {
            return Err("invalid interface config slot".into());
        };
        let i = *slot as usize;
        if i >= 9
            || seen[i]
            || !matches!(entry.get("count"),Some(Tag::Int(n)) if *n>0)
            || save::item_from_nbt(entry).is_none()
        {
            return Err("invalid interface config item".into());
        }
        seen[i] = true;
    }
    Ok(save::decode_items(Some(tag), 9))
}
// InterfaceLogic calls useRegisteredCapacities for both rows; AE2 registers
// four buckets for fluids in GenericSlotCapacities.
pub(super) const INTERFACE_FLUID_CAPACITY: u64 = 4000;
const FLUID_CONFIG: &str = "ae2_interface_fluid_config";
const FLUID_STOCK: &str = "ae2_interface_fluids";
// InterfaceLogic.isAllowedInStorageSlot uses AEKey.fuzzyEquals; fluid keys
// have no fuzzy range and therefore ignore components when a fuzzy card is installed.
fn allows_fluid_slot(
    machine: &Machine,
    configured: Option<&(ItemKey, u64)>,
    key: &ItemKey,
) -> bool {
    configured.is_none_or(|(wanted, _)| {
        wanted == key
            || (wanted.item_id == key.item_id
                && machine
                    .upgrades
                    .iter()
                    .flatten()
                    .any(|item| item.item.as_ref() == "ae2:fuzzy_card"))
    })
}
fn fluid_slots(machine: &Machine, name: &str) -> Result<Vec<Option<(ItemKey, u64)>>, String> {
    let mut slots = vec![None; 9];
    let Some(tag) = machine.config.get(&name.into()) else {
        return Ok(slots);
    };
    let Tag::List(entries) = tag else {
        return Err("invalid interface fluids".into());
    };
    for entry in entries {
        let (
            Some(Tag::Byte(slot)),
            Some(Tag::String(id)),
            Some(Tag::ByteArray(components)),
            Some(Tag::Long(amount)),
        ) = (
            entry.get("Slot"),
            entry.get("fluid"),
            entry.get("components"),
            entry.get("amount"),
        )
        else {
            return Err("invalid interface fluid".into());
        };
        let i = *slot as usize;
        let id = id.to_string().map_err(|_| "invalid fluid id")?;
        if i >= 9
            || slots[i].is_some()
            || *amount <= 0
            || *amount > INTERFACE_FLUID_CAPACITY as i64
            || !super::ae2_fluids::valid_fluid_id(&id)
        {
            return Err("invalid interface fluid slot".into());
        }
        slots[i] = Some((
            ItemKey {
                item_id: id,
                components: components.clone(),
            },
            *amount as u64,
        ));
    }
    Ok(slots)
}
fn store_fluids(machine: &mut Machine, name: &str, slots: &[Option<(ItemKey, u64)>]) {
    let entries = slots
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            s.as_ref().map(|(k, n)| {
                Tag::compound([
                    ("Slot", Tag::Byte(i as i8)),
                    ("fluid", Tag::String(k.item_id.as_str().into())),
                    ("components", Tag::ByteArray(k.components.clone())),
                    ("amount", Tag::Long(*n as i64)),
                ])
            })
        })
        .collect();
    machine.config.insert(name.into(), Tag::List(entries));
}
pub(super) fn interface_fluid_config(
    machine: &Machine,
) -> Result<Vec<Option<(ItemKey, u64)>>, String> {
    fluid_slots(machine, FLUID_CONFIG)
}
pub(super) fn interface_fluid_stock(
    machine: &Machine,
) -> Result<Vec<Option<(ItemKey, u64)>>, String> {
    fluid_slots(machine, FLUID_STOCK)
}
pub(super) fn set_interface_fluid_config(
    machine: &mut Machine,
    index: usize,
    key: Option<&ItemKey>,
) -> Result<(), String> {
    set_interface_fluid_config_amount(machine,index,key.map(|key|(key,1000)))
}
pub(super) fn set_interface_fluid_config_amount(
    machine:&mut Machine,index:usize,value:Option<(&ItemKey,u64)>,
)->Result<(),String> {
    if machine.kind != MachineKind::Interface || index >= 9 {
        return Err("invalid interface fluid config".into());
    }
    if value.is_some_and(|(_,amount)|amount==0) {return Err("invalid interface fluid amount".into());}
    let mut fluid = interface_fluid_config(machine)?;
    let mut item = interface_config(machine)?;
    fluid[index] = value.map(|(key,amount)| (key.clone(), amount.min(INTERFACE_FLUID_CAPACITY)));
    if value.is_some() {
        item[index] = None;
    }
    store_fluids(machine, FLUID_CONFIG, &fluid);
    machine
        .config
        .insert(CONFIG.into(), save::encode_items(&item));
    machine
        .config
        .insert("ae2_interface_next_tick".into(), Tag::Long(0));
    Ok(())
}
/// Real 1-bucket exchange against one fluid stock slot. Unknown fluid keys
/// and component-bearing buckets remain untouched.
pub(super) fn interface_bucket_exchange(
    machine: &mut Machine,
    index: usize,
    cursor: &ItemStack,
) -> Result<Option<ItemStack>, String> {
    if machine.kind != MachineKind::Interface
        || index >= 9
        || cursor.count != 1
        || cursor.damage != 0
        || cursor.data().is_some()
    {
        return Ok(None);
    }
    let config = interface_fluid_config(machine)?;
    let mut stock = interface_fluid_stock(machine)?;
    if machine.slots.get(index).and_then(Option::as_ref).is_some() {
        return Ok(None);
    }
    if let Some(id) = super::ae2_fluids::bucket_fluid(&cursor.item) {
        let key = super::ae2_fluids::fluid_key(id);
        if config[index].as_ref().is_some_and(|(want, _)| *want != key) {
            return Ok(None);
        }
        let n = match &stock[index] {
            Some((old, n)) if *old == key => *n,
            Some(_) => return Ok(None),
            None => 0,
        };
        if n > INTERFACE_FLUID_CAPACITY - 1000 {
            return Ok(None);
        }
        stock[index] = Some((key, n + 1000));
        store_fluids(machine, FLUID_STOCK, &stock);
        wake_interface(machine);
        return Ok(Some(ItemStack::new("minecraft:bucket", 1)));
    }
    if cursor.item.as_ref() == "minecraft:bucket" {
        if let Some((key, n)) = &stock[index] {
            if *n < 1000 || *key != super::ae2_fluids::fluid_key(&key.item_id) {
                return Ok(None);
            }
            let Some(bucket) = super::ae2_fluids::fluid_bucket(&key.item_id) else {
                return Ok(None);
            };
            let remain = n - 1000;
            stock[index] = (remain > 0).then_some((key.clone(), remain));
            store_fluids(machine, FLUID_STOCK, &stock);
            wake_interface(machine);
            return Ok(Some(ItemStack::new(bucket, 1)));
        }
    }
    Ok(None)
}
pub(super) fn set_interface_config(
    machine: &mut Machine,
    index: usize,
    stack: Option<&ItemStack>,
) -> Result<(), String> {
    if machine.kind != MachineKind::Interface || index >= 9 {
        return Err("invalid interface config".into());
    }
    let mut cfg = interface_config(machine)?;
    cfg[index] = stack.cloned().filter(|s| s.count > 0);
    let mut fluids = interface_fluid_config(machine)?;
    fluids[index] = None;
    store_fluids(machine, FLUID_CONFIG, &fluids);
    machine
        .config
        .insert(CONFIG.into(), save::encode_items(&cfg));
    machine
        .config
        .insert("ae2_interface_next_tick".into(), Tag::Long(0));
    Ok(())
}
impl Game<'_> {
    /// A calculation is valid only for the current card and ghost request.
    pub(super) fn ae2_interface_pending_craft_generation(
        &self, owner: InterfaceCraftOwner, slot: usize, key: &ItemKey, fluid: bool,
    ) -> Option<i64> {
        if slot >= 9 { return None; }
        let machine = self.interface_get(owner.owner()?)?;
        if !has_crafting_card(&machine) || craft_links(&machine).ok()?[slot].is_some() { return None; }
        let matches = if fluid {
            interface_fluid_config(&machine).ok()?.get(slot)?.as_ref().is_some_and(|(wanted, _)| wanted == key)
        } else {
            interface_config(&machine).ok()?.get(slot)?.as_ref()
                .and_then(|stack| ae2_lab::key(stack).ok()).is_some_and(|wanted| wanted == *key)
        };
        matches.then(|| super::ae2_autocrafting::crafting_card_generation(&machine.config)).flatten()
    }
    pub(super) fn ae2_interface_craft_link_valid(
        &self,
        owner: InterfaceCraftOwner,
        slot: usize,
        cpu: GridAddress,
        key: &ItemKey,
        fluid: bool,
    ) -> bool {
        let Some(owner) = owner.owner() else {
            return false;
        };
        let Some(machine) = self.interface_get(owner) else {
            return false;
        };
        if !active_craft_link(&machine, slot, cpu) {
            return false;
        }
        if fluid {
            interface_fluid_config(&machine)
                .ok()
                .and_then(|cfg| cfg[slot].clone())
                .is_some_and(|(wanted, _)| wanted == *key)
        } else {
            interface_config(&machine)
                .ok()
                .and_then(|cfg| cfg[slot].clone())
                .and_then(|stack| ae2_lab::key(&stack).ok())
                .is_some_and(|wanted| wanted == *key)
        }
    }
    pub(super) fn ae2_interface_craft_links(
        &self,
        owner: InterfaceCraftOwner,
    ) -> Result<[Option<GridAddress>; 9], String> {
        let owner = owner.owner().ok_or("invalid interface face")?;
        let machine = self.interface_get(owner).ok_or("interface missing")?;
        craft_links(&machine)
    }
    pub(super) fn ae2_interface_set_craft_link(
        &mut self,
        owner: InterfaceCraftOwner,
        slot: usize,
        old: Option<GridAddress>,
        new: Option<GridAddress>,
    ) -> Result<(), String> {
        let owner = owner.owner().ok_or("invalid interface face")?;
        let mut machine = self.interface_get(owner).ok_or("interface missing")?;
        let mut links = craft_links(&machine)?;
        if slot >= links.len() || links[slot] != old {
            return Err("interface crafting link changed".into());
        }
        if new.is_some() && !has_crafting_card(&machine) {
            return Err("interface crafting card missing".into());
        }
        links[slot] = new;
        let old_mask = match machine.config.get(&CANCELLED_LINKS.into()) {
            Some(Tag::Int(mask)) => *mask,
            _ => 0,
        };
        machine
            .config
            .insert(CANCELLED_LINKS.into(), Tag::Int(old_mask & !(1 << slot)));
        store_craft_links(&mut machine, &links);
        wake_interface(&mut machine);
        self.interface_store(owner, &machine)
    }
    /// Caller holds CPU escrow and snapshots all affected block entities. A
    /// simulated insertion changes no physical inventory or persisted link.
    pub(super) fn ae2_interface_deliver_item(
        &mut self,
        owner: InterfaceCraftOwner,
        slot: usize,
        cpu: GridAddress,
        stack: &ItemStack,
        simulate: bool,
    ) -> Result<u32, String> {
        let owner = owner.owner().ok_or("invalid interface face")?;
        let mut machine = self.interface_get(owner).ok_or("interface missing")?;
        if slot >= 9
            || craft_links(&machine)?[slot] != Some(cpu)
            || !has_crafting_card(&machine)
            || craft_link_cancelled(&machine, slot)
        {
            return Ok(0);
        }
        let items = interface_config(&machine)?;
        let fluids = interface_fluid_config(&machine)?;
        let fluid_stock = interface_fluid_stock(&machine)?;
        if machine.slots.len() != 9
            || fluids[slot].is_some()
            || fluid_stock[slot].is_some()
            || !self.interface_allowed(&machine, &items, slot, stack)
        {
            return Ok(0);
        }
        let max = self.res.items().stack_max(stack);
        let existing = machine.slots[slot].as_ref();
        if existing.is_some_and(|v| !v.same_item_same_components(stack)) {
            return Ok(0);
        }
        let old = existing.map_or(0, |v| v.count);
        let moved = stack.count.min(max.saturating_sub(old));
        if moved > 0 && !simulate {
            machine.slots[slot] = Some(ItemStack {
                count: old + moved,
                ..stack.clone()
            });
            wake_interface(&mut machine);
            self.interface_store(owner, &machine)?;
        }
        Ok(moved)
    }
    pub(super) fn ae2_interface_deliver_fluid(
        &mut self,
        owner: InterfaceCraftOwner,
        slot: usize,
        cpu: GridAddress,
        key: &ItemKey,
        amount: u64,
        simulate: bool,
    ) -> Result<u64, String> {
        let owner = owner.owner().ok_or("invalid interface face")?;
        let mut machine = self.interface_get(owner).ok_or("interface missing")?;
        if slot >= 9
            || craft_links(&machine)?[slot] != Some(cpu)
            || !has_crafting_card(&machine)
            || craft_link_cancelled(&machine, slot)
        {
            return Ok(0);
        }
        let items = interface_config(&machine)?;
        let config = interface_fluid_config(&machine)?;
        let mut stock = interface_fluid_stock(&machine)?;
        if machine.slots.len() != 9
            || items[slot].is_some()
            || machine.slots[slot].is_some()
            || !allows_fluid_slot(&machine, config[slot].as_ref(), key)
            || stock[slot]
                .as_ref()
                .is_some_and(|(stored, _)| stored != key)
        {
            return Ok(0);
        }
        let old = stock[slot].as_ref().map_or(0, |(_, n)| *n);
        let moved = amount.min(INTERFACE_FLUID_CAPACITY.saturating_sub(old));
        if moved > 0 && !simulate {
            stock[slot] = Some((key.clone(), old + moved));
            store_fluids(&mut machine, FLUID_STOCK, &stock);
            wake_interface(&mut machine);
            self.interface_store(owner, &machine)?;
        }
        Ok(moved)
    }
    fn interface_get(&self, owner: Owner) -> Option<Machine> {
        match owner {
            Owner::Block(a) => match self.ae2_block_entity(a) {
                Some(BlockEntity::Ae2(m)) if m.kind == MachineKind::Interface => {
                    Some((**m).clone())
                }
                _ => None,
            },
            Owner::Part(p) => self
                .ae2_part_machine(p)
                .ok()
                .flatten()
                .filter(|m| m.kind == MachineKind::Interface),
        }
    }
    fn interface_store(&mut self, owner: Owner, machine: &Machine) -> Result<(), String> {
        match owner {
            Owner::Block(a) => {
                let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity_mut(a) else {
                    return Err("interface disappeared".into());
                };
                if m.kind != MachineKind::Interface {
                    return Err("interface changed".into());
                }
                **m = machine.clone();
                self.ae2_mark_modified(a);
                Ok(())
            }
            Owner::Part(p) => self.ae2_store_part_machine(p, machine),
        }
    }
    /// Source 5–120 tick cadence: progress is urgent, blocked work slows.
    /// Configuration changes wake immediately; failures restore block entities.
    /// Mirror InterfaceLogic's planned-work state without performing a transfer.
    fn interface_has_work(&self, machine:&Machine) -> Result<bool,String> {
        if machine.slots.len()!=9 {return Err("invalid interface inventory".into())}
        let config=interface_config(machine)?;
        let fluid_config=interface_fluid_config(machine)?;
        let fluids=interface_fluid_stock(machine)?;
        let links=craft_links(machine)?;
        for i in 0..9 {
            if config[i].is_some() && fluid_config[i].is_some() || machine.slots[i].is_some() && fluids[i].is_some() {return Err("interface slot has conflicting item/fluid ownership".into())}
            if fluids[i].is_some() || (fluid_config[i].is_some() && machine.slots[i].is_none()) {
                let desired=fluid_config[i].as_ref();let existing=fluids[i].as_ref();
                let remove=matches!((desired,existing),(None,Some(_))) || matches!((desired,existing),(Some((a,w)),Some((b,n))) if a!=b || n>w);
                let add=matches!((desired,existing),(Some(_),None)) || matches!((desired,existing),(Some((a,w)),Some((b,n))) if a==b && n<w);
                if remove || (add && links[i].is_none()) {return Ok(true)}
            } else {
                let desired=config[i].as_ref();let existing=machine.slots[i].as_ref();
                let same=existing.is_some_and(|item|desired.is_some() && self.interface_allowed(machine,&config,i,item));
                let remove=existing.is_some_and(|item|desired.is_none() || !same || desired.is_some_and(|want|item.count>want.count));
                let add=desired.is_some_and(|want|existing.is_none() || (same && existing.is_some_and(|item|item.count<want.count)));
                if remove || (add && links[i].is_none()) {return Ok(true)}
            }
        }
        Ok(false)
    }
    pub(super) fn tick_ae2_interfaces(&mut self) {
        let now=self.ae2_server_tick();
        let network=self.ae2_network();
        let services=self.ae2_event_service_ids(&network);
        let mut owners = Vec::new();
        for (&pos, be) in &self.world.block_entities {
            let Some(host) = self.ae2_local_address(pos) else {
                continue;
            };
            let BlockEntity::Ae2(m) = be else { continue };
            if m.kind == MachineKind::Interface {
                owners.push((Owner::Block(host), pos, None));
            } else if m.kind == MachineKind::CableBus {
                if let Some(Tag::List(parts)) = m.config.get(&"parts".into()) {
                    for p in parts {
                        if p.get("id") != Some(&Tag::String("ae2:cable_interface".into())) {
                            continue;
                        }
                        let Some(Tag::String(face)) = p.get("face") else {
                            continue;
                        };
                        let Ok(face) = face.to_string() else { continue };
                        let idx = match face.as_str() {
                            "east" => 0,
                            "west" => 1,
                            "up" => 2,
                            "down" => 3,
                            "south" => 4,
                            "north" => 5,
                            _ => continue,
                        };
                        owners.push((
                            Owner::Part(PartAddress { host, face: idx }),
                            pos,
                            Some(face),
                        ));
                    }
                }
            }
        }
        for (owner, pos, face) in owners {
            let online = face.as_deref().map_or_else(
                || self.ae2_online_encoded(owner.host()),
                |f| self.ae2_part_online(pos, f),
            );
            let Some(machine) = self.interface_get(owner) else {
                continue;
            };
            let Ok(work)=self.interface_has_work(&machine) else {continue};
            let anchor=owner.host().network_pos();
            let identity=match owner {Owner::Block(_)=>network.node_identities.get(&anchor),Owner::Part(part)=>network.part_identities.get(&(anchor,part.face))}.copied();
            let Some(identity)=identity else {continue};
            let service=services.as_ref().and_then(|s|s.get(&identity).copied());
            let bounds=horizon_client::ae2_tick_config::rates(horizon_client::ae2_tick_config::Kind::Interface);
            if !self.ae2_device_scheduler.borrow_mut().interface_due(identity,service,now,online,work,bounds) {continue}
            let mut worked = false;
            for _ in 0..9 {
                match self.interface_tick_one(owner) {
                    Ok(true) => worked = true,
                    Ok(false) => break,
                    Err(e) => {
                        log::warn!("AE2 interface {pos:?}: {e}");
                        break;
                    }
                }
            }
            let remaining=self.interface_get(owner).and_then(|m|self.interface_has_work(&m).ok()).unwrap_or(false);
            let modulation=if !remaining {super::ae2_tick_tracker::Modulation::Sleep} else if worked {super::ae2_tick_tracker::Modulation::Urgent} else {super::ae2_tick_tracker::Modulation::Slower};
            self.ae2_device_scheduler.borrow_mut().completed(identity,now,modulation);
        }
    }
    fn interface_tick_one(&mut self, owner: Owner) -> Result<bool, String> {
        let Some(machine) = self.interface_get(owner) else {
            return Ok(false);
        };
        let _request = InterfaceRequestGuard::enter(owner.host(), interface_priority(&machine));
        let config = interface_config(&machine)?;
        let fluid_config = interface_fluid_config(&machine)?;
        let fluid_stock = interface_fluid_stock(&machine)?;
        let links = craft_links(&machine)?;
        if machine.slots.len() != 9 {
            return Err("invalid interface inventory".into());
        }
        for i in 0..9 {
            if config[i].is_some() && fluid_config[i].is_some()
                || machine.slots[i].is_some() && fluid_stock[i].is_some()
            {
                return Err("interface slot has conflicting item/fluid ownership".into());
            }
            if fluid_stock[i].is_some() || (fluid_config[i].is_some() && machine.slots[i].is_none())
            {
                let desired = fluid_config[i].as_ref();
                let existing = fluid_stock[i].as_ref();
                let same = matches!((desired,existing),(Some((a,_)),Some((b,_))) if a==b);
                let remove = match (desired, existing) {
                    (_, Some((_, n))) if desired.is_none() || !same => *n,
                    (Some((_, want)), Some((_, n))) if n > want => n - want,
                    _ => 0,
                };
                let add = match (desired, existing) {
                    (Some((_, want)), None) => *want,
                    (Some((_, want)), Some((_, n))) if same && n < want => want - n,
                    _ => 0,
                };
                if remove == 0 && (add == 0 || links[i].is_some()) {
                    continue;
                }
                let before = self.ae2_snapshot_blocks();
                let mut updated = machine.clone();
                let mut stock = fluid_stock.clone();
                let mut plan = super::ae2_buses::FormationPlan::default();
                let result = (|| -> Result<(), String> {
                    if remove > 0 {
                        let (key, n) = existing.ok_or("missing fluid stock")?;
                        let moved = self.ae2_grid_fluid_insert_staged_encoded(
                            owner.host(), key, remove, false, &mut plan)?;
                        if moved > remove {
                            return Err("invalid fluid insertion".into());
                        }
                        stock[i] = (*n > moved).then_some((key.clone(), n - moved));
                    } else {
                        let (key, _) = desired.ok_or("missing fluid request")?;
                        let moved =
                            self.ae2_grid_fluid_extract_encoded(owner.host(), key, add, false)?;
                        if moved > add {
                            return Err("invalid fluid extraction".into());
                        }
                        let n = existing.map_or(0, |(_, n)| *n) + moved;
                        stock[i] = (n > 0).then_some((key.clone(), n));
                    }
                    store_fluids(&mut updated, FLUID_STOCK, &stock);
                    self.interface_store(owner, &updated)?;
                    Ok(())
                })().and_then(|()| self.ae2_commit_formation(plan));
                if let Err(error) = result {
                    self.ae2_restore_blocks(before);
                    return Err(error);
                }
                if stock != fluid_stock {
                    return Ok(true);
                }
                if add > 0 && has_crafting_card(&machine) {
                    if let Some((key, _)) = desired {
                        if self
                            .ae2_begin_interface_craft(owner.into(), i, key, true, add)
                            .is_ok()
                        {
                            return Ok(true);
                        }
                    }
                }
                continue;
            }
            let desired = config[i].as_ref();
            let existing = machine.slots[i].as_ref();
            let same = match (desired, existing) {
                (Some(_), Some(b)) => self.interface_allowed(&machine, &config, i, b),
                _ => false,
            };
            let remove = match (desired, existing) {
                (_, Some(v)) if desired.is_none() || !same => v.count,
                (Some(d), Some(v)) if v.count > d.count => v.count - d.count,
                _ => 0,
            };
            let add = match (desired, existing) {
                (Some(d), None) => d.count,
                (Some(d), Some(v)) if same && v.count < d.count => d.count - v.count,
                _ => 0,
            };
            if remove == 0 && (add == 0 || links[i].is_some()) {
                continue;
            }
            let before = self.ae2_snapshot_blocks();
            let mut updated = machine.clone();
            let mut plan = super::ae2_buses::FormationPlan::default();
            let result = (|| -> Result<(), String> {
                if remove > 0 {
                    let input = existing.ok_or("missing interface stock")?;
                    let moved = self.ae2_grid_insert_staged_encoded(
                        owner.host(),
                        input,
                        remove as u64,
                        &mut plan,
                    )? as u32;
                    if moved > remove {
                        return Err("invalid interface insertion".into());
                    }
                    let left = input.count - moved;
                    updated.slots[i] = (left > 0).then_some(ItemStack {
                        count: left,
                        ..input.clone()
                    });
                } else {
                    let requested = desired.ok_or("missing interface request")?;
                    // Do not merge distinct components into an existing fuzzy
                    // variant; top it up using that actual key instead.
                    let input = existing
                        .filter(|v| !v.same_item_same_components(requested))
                        .unwrap_or(requested);
                    let key: ItemKey = ae2_lab::key(input)?;
                    let mut extracted =
                        self.ae2_grid_extract_staged_encoded(owner.host(), &key, add as u64, &mut plan)?;
                    if extracted.is_none()
                        && existing.is_none()
                        && machine
                            .upgrades
                            .iter()
                            .flatten()
                            .any(|s| s.item.as_ref() == "ae2:fuzzy_card")
                    {
                        for (candidate, _) in self.ae2_grid_inventory_encoded(owner.host())? {
                            let Ok(item) = ae2_lab::prototype(&candidate) else {
                                continue;
                            };
                            if !self.interface_allowed(&machine, &config, i, &item) {
                                continue;
                            }
                            extracted = self.ae2_grid_extract_staged_encoded(
                                owner.host(),
                                &candidate,
                                add as u64,
                                &mut plan,
                            )?;
                            if extracted.is_some() {
                                break;
                            }
                        }
                    }
                    if let Some(extracted) = extracted {
                        let count = extracted.count + existing.map_or(0, |v| v.count);
                        updated.slots[i] = Some(ItemStack { count, ..extracted });
                    }
                }
                self.interface_store(owner, &updated)?;
                self.ae2_commit_formation(plan)?;
                Ok(())
            })();
            if let Err(error) = result {
                self.ae2_restore_blocks(before);
                return Err(error);
            }
            if updated.slots != machine.slots {
                return Ok(true);
            }
            if add > 0 && has_crafting_card(&machine) {
                if let Some(requested) = desired {
                    if let Ok(key) = ae2_lab::key(requested) {
                        if self
                            .ae2_begin_interface_craft(owner.into(), i, &key, false, u64::from(add))
                            .is_ok()
                        {
                            return Ok(true);
                        }
                    }
                }
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fuzzy_fluid_slot_accepts_same_fluid_with_distinct_components() {
        let mut machine = Machine::new(MachineKind::Interface);
        let wanted = (
            ItemKey {
                item_id: "minecraft:water".into(),
                components: vec![1],
            },
            1000,
        );
        let different_components = ItemKey {
            item_id: "minecraft:water".into(),
            components: vec![2],
        };
        let different_fluid = ItemKey {
            item_id: "minecraft:lava".into(),
            components: vec![2],
        };
        assert!(!allows_fluid_slot(
            &machine,
            Some(&wanted),
            &different_components
        ));
        machine.upgrades = vec![Some(ItemStack::new("ae2:fuzzy_card", 1))];
        assert!(allows_fluid_slot(
            &machine,
            Some(&wanted),
            &different_components
        ));
        assert!(!allows_fluid_slot(
            &machine,
            Some(&wanted),
            &different_fluid
        ));
    }
    #[test]
    fn requester_links_preserve_dimension_and_reject_duplicate_slots() {
        let mut machine = Machine::new(MachineKind::Interface);
        let cpu = GridAddress::from_network(ae2_network_core::Pos {
            x: 7,
            y: 12340,
            z: -4,
        })
        .unwrap();
        let mut links = [None; 9];
        links[4] = Some(cpu);
        store_craft_links(&mut machine, &links);
        assert_eq!(craft_links(&machine).unwrap(), links);
        machine.upgrades = vec![Some(ItemStack::new("ae2:crafting_card", 1))];
        assert!(active_craft_link(&machine, 4, cpu));
        let original_generation = super::super::ae2_autocrafting::crafting_card_generation(&machine.config);
        machine.upgrades[0] = None;
        mark_card_removed(&mut machine);
        machine.upgrades[0] = Some(ItemStack::new("ae2:crafting_card", 1));
        assert_ne!(super::super::ae2_autocrafting::crafting_card_generation(&machine.config), original_generation);
        assert!(!active_craft_link(&machine, 4, cpu));
        assert!(craft_link_cancelled(&machine, 4));
        assert!(!craft_link_cancelled(&machine, 3));
        let Some(Tag::List(entries)) = machine.config.get_mut(&CRAFT_LINKS.into()) else {
            panic!("missing links")
        };
        entries.push(entries[0].clone());
        assert!(craft_links(&machine).is_err());
    }
    #[test]
    fn wrapped_fluid_config_keeps_identity_and_clamps_registered_capacity() {
        let mut machine=Machine::new(MachineKind::Interface);
        let key=ItemKey{item_id:"minecraft:water".into(),components:vec![1,2]};
        set_interface_fluid_config_amount(&mut machine,2,Some((&key,1250))).unwrap();
        assert_eq!(interface_fluid_config(&machine).unwrap()[2],Some((key.clone(),1250)));
        set_interface_fluid_config_amount(&mut machine,2,Some((&key,10_000))).unwrap();
        assert_eq!(interface_fluid_config(&machine).unwrap()[2],Some((key,4000)));
    }
    #[test]
    fn ghost_configuration_preserves_count_and_components_and_rejects_duplicate_slots() {
        let mut machine = Machine::new(MachineKind::Interface);
        let mut wanted = ItemStack::new("minecraft:diamond", 3);
        wanted.data_mut().custom_name = Some("Stock request".into());
        set_interface_config(&mut machine, 4, Some(&wanted)).unwrap();
        assert_eq!(
            interface_config(&machine).unwrap()[4].as_ref(),
            Some(&wanted)
        );
        let Some(Tag::List(entries)) = machine.config.get_mut(&CONFIG.into()) else {
            panic!("missing configuration")
        };
        entries.push(entries[0].clone());
        assert!(interface_config(&machine).is_err());
    }
}

// Source InterfaceLogic uses a machine action context when replenishing stock.
// Keep it scoped to this authoritative server thread, including nested grids.
thread_local! {
    static INTERFACE_REQUEST: std::cell::Cell<Option<(GridAddress, i32)>> = const { std::cell::Cell::new(None) };
    static INTERFACE_FORWARDING: std::cell::RefCell<Vec<Owner>> = const { std::cell::RefCell::new(Vec::new()) };
}
fn interface_priority(machine: &Machine) -> i32 {
    match machine.config.get(&"priority".into()) {
        Some(Tag::Int(n)) => *n,
        _ => 0,
    }
}
struct InterfaceRequestGuard(Option<(GridAddress, i32)>);
impl InterfaceRequestGuard {
    fn enter(host: GridAddress, priority: i32) -> Self {
        Self(INTERFACE_REQUEST.with(|v| v.replace(Some((host, priority)))))
    }
}
impl Drop for InterfaceRequestGuard {
    fn drop(&mut self) {
        INTERFACE_REQUEST.with(|v| v.set(self.0));
    }
}
struct ForwardGuard(Owner);
impl ForwardGuard {
    fn enter(owner: Owner) -> Option<Self> {
        INTERFACE_FORWARDING.with(|stack| {
            let mut stack = stack.borrow_mut();
            if stack.contains(&owner) {
                None
            } else {
                stack.push(owner);
                Some(Self(owner))
            }
        })
    }
}
impl Drop for ForwardGuard {
    fn drop(&mut self) {
        INTERFACE_FORWARDING.with(|stack| {
            let popped = stack.borrow_mut().pop();
            debug_assert_eq!(popped, Some(self.0));
        });
    }
}
impl Game<'_> {
    // AE2 InterfaceLogic.InterfaceInventory: same-grid interface stock insertion
    // is rejected; stock extraction requires a strictly higher requester priority.
    pub(super) fn ae2_interface_mount_allowed(
        &self,
        host: GridAddress,
        face: horizon_core::geometry::Direction,
        insert: bool,
    ) -> bool {
        let Some(owner) = self.interface_owner_at(host, face) else {
            return false;
        };
        let Some(machine) = self.interface_get(owner) else {
            return false;
        };
        // Unconfigured interfaces expose their grid directly, rather than the
        // local InterfaceInventory that applies these priority rules.
        let Ok(items) = interface_config(&machine) else {
            return false;
        };
        let Ok(fluids) = interface_fluid_config(&machine) else {
            return false;
        };
        if items.iter().all(Option::is_none) && fluids.iter().all(Option::is_none) {
            return true;
        }
        INTERFACE_REQUEST.with(|context| {
            let Some((source, priority)) = context.get() else {
                return true;
            };
            let Ok(analysis) = self.ae2_network().analyze() else {
                return false;
            };
            let same = match (
                analysis.nodes.get(&source.network_pos()),
                analysis.nodes.get(&host.network_pos()),
            ) {
                (Some(a), Some(b)) => a.component == b.component,
                _ => return false,
            };
            !same || (!insert && priority > interface_priority(&machine))
        })
    }
    pub(super) fn ae2_has_interface_face(
        &self,
        host: GridAddress,
        face: horizon_core::geometry::Direction,
    ) -> bool {
        self.interface_owner_at(host, face).is_some()
    }
    pub(super) fn ae2_interface_face_identity(
        &self,
        host: GridAddress,
        face: horizon_core::geometry::Direction,
    ) -> Option<u8> {
        match self.interface_owner_at(host, face) {
            Some(Owner::Part(part)) => Some(part.face),
            _ => None,
        }
    }
    pub(in crate::game) fn ae2_interface_supports(
        &self,host:GridAddress,face:horizon_core::geometry::Direction,
    )->bool {
        self.interface_owner_at(host,face).is_some()
    }
    fn interface_owner_at(
        &self,
        host: GridAddress,
        face: horizon_core::geometry::Direction,
    ) -> Option<Owner> {
        if matches!(self.ae2_block_entity(host),Some(BlockEntity::Ae2(m)) if m.kind==MachineKind::Interface)
        {
            return Some(Owner::Block(host));
        }
        use horizon_core::geometry::Direction as D;
        let face = match face {
            D::East => 0,
            D::West => 1,
            D::Up => 2,
            D::Down => 3,
            D::South => 4,
            D::North => 5,
        };
        let part = PartAddress { host, face };
        self.ae2_part_machine(part)
            .ok()
            .flatten()
            .filter(|m| m.kind == MachineKind::Interface)
            .map(|_| Owner::Part(part))
    }
    fn interface_allowed(
        &self,
        machine: &Machine,
        config: &[Option<ItemStack>],
        slot: usize,
        item: &ItemStack,
    ) -> bool {
        let Some(wanted) = config.get(slot).and_then(Option::as_ref) else {
            return true;
        };
        if wanted.same_item_same_components(item) {
            return true;
        }
        if !machine
            .upgrades
            .iter()
            .flatten()
            .any(|s| s.item.as_ref() == "ae2:fuzzy_card")
        {
            return false;
        }
        let Ok(a) = ae2_lab::key(wanted) else {
            return false;
        };
        let Ok(b) = ae2_lab::key(item) else {
            return false;
        };
        let mode = match machine.config.get(&"ae2_fuzzy_mode".into()) {
            Some(Tag::String(s)) => s.to_string().unwrap_or_else(|_| "ignore_all".into()),
            _ => "ignore_all".into(),
        };
        let max = self.res.items().get(&a.item_id).map_or(0, |v| v.max_damage);
        super::ae2_buses::fuzzy_key_match(&(&a).into(), &(&b).into(), &mode, max)
    }
    pub(super) fn ae2_interface_item_candidates(
        &self,
        host: GridAddress,
        face: horizon_core::geometry::Direction,
    ) -> Option<Vec<ItemStack>> {
        let owner = self.interface_owner_at(host, face)?;
        let machine = self.interface_get(owner)?;
        let config = interface_config(&machine).ok()?;
        let fluid_config = interface_fluid_config(&machine).ok()?;
        if config.iter().any(Option::is_some) || fluid_config.iter().any(Option::is_some) {
            return Some(machine.slots.into_iter().flatten().collect());
        }
        let _guard = ForwardGuard::enter(owner)?;
        Some(
            self.ae2_grid_inventory_encoded(host)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|(key, n)| {
                    let mut item = ae2_lab::prototype(&key).ok()?;
                    item.count = n.min(u32::MAX as u64) as u32;
                    Some(item)
                })
                .collect(),
        )
    }
    pub(super) fn ae2_interface_item_insert(
        &mut self, host: GridAddress, face: horizon_core::geometry::Direction,
        stack: &ItemStack, limit: u32, simulate: bool,
    ) -> Option<u32> {
        if !self.ae2_has_interface_face(host, face) { return None; }
        let snapshot = self.ae2_snapshot_blocks();
        let mut plan = super::ae2_buses::FormationPlan::default();
        let result = self.ae2_interface_item_insert_staged(host, face, stack, limit, simulate, &mut plan);
        if !simulate && result.is_some() && self.ae2_commit_formation(plan).is_err() {
            self.ae2_restore_blocks(snapshot);
            return Some(0);
        }
        result
    }
    pub(super) fn ae2_interface_item_insert_staged(
        &mut self, host: GridAddress, face: horizon_core::geometry::Direction,
        stack: &ItemStack, limit: u32, simulate: bool,
        plan: &mut super::ae2_buses::FormationPlan,
    ) -> Option<u32> {
        let owner = self.interface_owner_at(host, face)?;
        let mut machine = self.interface_get(owner)?;
        let config = interface_config(&machine).ok()?;
        let fluid_config = interface_fluid_config(&machine).ok()?;
        let fluid_stock = interface_fluid_stock(&machine).ok()?;
        let _guard = ForwardGuard::enter(owner)?;
        if config.iter().all(Option::is_none) && fluid_config.iter().all(Option::is_none) {
            return Some(
                self.ae2_grid_insert_uncharged_storage_staged_encoded(
                    host,
                    stack,
                    limit.min(stack.count) as u64,
                    simulate,
                    plan,
                )
                .unwrap_or(0) as u32,
            );
        }
        let mut left = limit.min(stack.count);
        let before = left;
        let max = self.res.items().stack_max(stack);
        for i in 0..machine.slots.len() {
            if left == 0 {
                break;
            }
            if fluid_config[i].is_some()
                || fluid_stock[i].is_some()
                || !self.interface_allowed(&machine, &config, i, stack)
            {
                continue;
            }
            match &mut machine.slots[i] {
                Some(v) if v.same_item_same_components(stack) => {
                    let n = left.min(max.saturating_sub(v.count));
                    v.count += n;
                    left -= n
                }
                None => {
                    let n = left.min(max);
                    machine.slots[i] = Some(ItemStack {
                        count: n,
                        ..stack.clone()
                    });
                    left -= n
                }
                _ => {}
            }
        }
        let moved = before - left;
        if moved > 0 && !simulate {
            wake_interface(&mut machine);
            if self.interface_store(owner, &machine).is_err() {
                return Some(0);
            }
        }
        Some(moved)
    }
    pub(super) fn ae2_interface_item_extract(
        &mut self, host: GridAddress, face: horizon_core::geometry::Direction,
        limit: u32, filter: Option<&ItemStack>, simulate: bool,
    ) -> Option<Option<ItemStack>> {
        if !self.ae2_has_interface_face(host, face) { return None; }
        let snapshot = self.ae2_snapshot_blocks();
        let mut plan = super::ae2_buses::FormationPlan::default();
        let result = self.ae2_interface_item_extract_staged(host, face, limit, filter, simulate, &mut plan);
        if !simulate && result.is_some() && self.ae2_commit_formation(plan).is_err() {
            self.ae2_restore_blocks(snapshot);
            return Some(None);
        }
        result
    }
    /// Aggregate variant for CPU reservations. The same forwarding guard
    /// protects pass-through networks; stocked interfaces retain exact keys.
    pub(super) fn ae2_interface_item_extract_bulk_staged(
        &mut self, host: GridAddress, face: horizon_core::geometry::Direction,
        key: &ItemKey, limit: u64, plan: &mut super::ae2_buses::FormationPlan,
    ) -> Option<Result<u64, String>> {
        let owner = self.interface_owner_at(host, face)?;
        let mut machine = self.interface_get(owner)?;
        let config = match interface_config(&machine) { Ok(v) => v, Err(e) => return Some(Err(e)) };
        let fluid_config = match interface_fluid_config(&machine) { Ok(v) => v, Err(e) => return Some(Err(e)) };
        let _guard = ForwardGuard::enter(owner)?;
        if config.iter().all(Option::is_none) && fluid_config.iter().all(Option::is_none) {
            return Some(self.ae2_grid_extract_bulk_staged_encoded(host, key, limit, false, false, plan));
        }
        let filter = match ae2_lab::prototype(key) { Ok(v) => v, Err(e) => return Some(Err(e)) };
        let mut left = limit;
        for slot in &mut machine.slots {
            let Some(item) = slot.as_mut() else { continue };
            if !filter.same_item_same_components(item) { continue; }
            let n = left.min(u64::from(item.count));
            item.count -= n as u32;
            left -= n;
            if item.count == 0 { *slot = None; }
            if left == 0 { break; }
        }
        if left < limit {
            wake_interface(&mut machine);
            if let Err(error) = self.interface_store(owner, &machine) { return Some(Err(error)); }
        }
        Some(Ok(limit - left))
    }
    pub(super) fn ae2_interface_item_extract_staged(
        &mut self, host: GridAddress, face: horizon_core::geometry::Direction,
        limit: u32, filter: Option<&ItemStack>, simulate: bool,
        plan: &mut super::ae2_buses::FormationPlan,
    ) -> Option<Option<ItemStack>> {
        let owner = self.interface_owner_at(host, face)?;
        let mut machine = self.interface_get(owner)?;
        let config = interface_config(&machine).ok()?;
        let fluid_config = interface_fluid_config(&machine).ok()?;
        let _guard = ForwardGuard::enter(owner)?;
        if config.iter().all(Option::is_none) && fluid_config.iter().all(Option::is_none) {
            let entries = self.ae2_grid_inventory_encoded(host).ok()?;
            for (key, _) in entries {
                let item = ae2_lab::prototype(&key).ok()?;
                if filter.is_some_and(|f| !f.same_item_same_components(&item)) {
                    continue;
                }
                if let Ok(Some(got)) =
                    self.ae2_grid_extract_uncharged_staged_encoded(host, &key, limit as u64, simulate, plan)
                {
                    return Some(Some(got));
                }
            }
            return Some(None);
        }
        for slot in &mut machine.slots {
            let Some(item) = slot.as_mut() else { continue };
            if filter.is_some_and(|f| !f.same_item_same_components(item)) {
                continue;
            }
            let n = limit.min(item.count);
            if n == 0 {
                return Some(None);
            }
            let result = ItemStack {
                count: n,
                ..item.clone()
            };
            item.count -= n;
            if item.count == 0 {
                *slot = None
            }
            if !simulate {
                wake_interface(&mut machine);
                if self.interface_store(owner, &machine).is_err() {
                    return Some(None);
                }
            }
            return Some(Some(result));
        }
        Some(None)
    }
}
impl Game<'_> {
    pub(super) fn ae2_interface_fluid_entries(
        &self,
        host: GridAddress,
        face: horizon_core::geometry::Direction,
    ) -> Option<Vec<(ItemKey, u64)>> {
        let owner = self.interface_owner_at(host, face)?;
        let machine = self.interface_get(owner)?;
        let items = interface_config(&machine).ok()?;
        let fluids = interface_fluid_config(&machine).ok()?;
        if items.iter().any(Option::is_some) || fluids.iter().any(Option::is_some) {
            return Some(
                interface_fluid_stock(&machine)
                    .ok()?
                    .into_iter()
                    .flatten()
                    .collect(),
            );
        }
        let _guard = ForwardGuard::enter(owner)?;
        Some(self.ae2_grid_fluids_encoded(host).unwrap_or_default())
    }
    pub(super) fn ae2_interface_fluid_contents(
        &self,
        host: GridAddress,
        face: horizon_core::geometry::Direction,
    ) -> Option<Option<(ItemKey, u64)>> {
        Some(
            self.ae2_interface_fluid_entries(host, face)?
                .into_iter()
                .next(),
        )
    }
    pub(super) fn ae2_interface_fluid_move(
        &mut self,
        host: GridAddress,
        face: horizon_core::geometry::Direction,
        key: &ItemKey,
        amount: u64,
        insert: bool,
        simulate: bool,
    ) -> Option<u64> {
        if !self.ae2_has_interface_face(host, face) { return None; }
        let before = self.ae2_snapshot_blocks();
        let mut plan = super::ae2_buses::FormationPlan::default();
        let result = self.ae2_interface_fluid_move_staged(host, face, key, amount, insert, simulate, &mut plan);
        if !simulate && result.is_some() && self.ae2_commit_formation(plan).is_err() {
            self.ae2_restore_blocks(before);
            return Some(0);
        }
        result
    }
    pub(super) fn ae2_interface_fluid_move_staged(
        &mut self, host: GridAddress, face: horizon_core::geometry::Direction, key: &ItemKey,
        amount: u64, insert: bool, simulate: bool, plan: &mut super::ae2_buses::FormationPlan,
    ) -> Option<u64> {
        let owner = self.interface_owner_at(host, face)?;
        let mut machine = self.interface_get(owner)?;
        let items = interface_config(&machine).ok()?;
        let config = interface_fluid_config(&machine).ok()?;
        let mut stock = interface_fluid_stock(&machine).ok()?;
        let _guard = ForwardGuard::enter(owner)?;
        if items.iter().all(Option::is_none) && config.iter().all(Option::is_none) {
            return Some(
                self.ae2_grid_fluid_capability_staged_encoded(host, key, amount, insert, simulate, plan)
                    .unwrap_or(0),
            );
        }
        let mut left = amount;
        for i in 0..9 {
            if left == 0 {
                break;
            }
            if insert {
                if items[i].is_some()
                    || machine.slots[i].is_some()
                    || !allows_fluid_slot(&machine, config[i].as_ref(), key)
                {
                    continue;
                }
                match &mut stock[i] {
                    Some((stored, n)) if stored == key => {
                        let moved = left.min(INTERFACE_FLUID_CAPACITY.saturating_sub(*n));
                        *n += moved;
                        left -= moved
                    }
                    None => {
                        let moved = left.min(INTERFACE_FLUID_CAPACITY);
                        stock[i] = Some((key.clone(), moved));
                        left -= moved
                    }
                    _ => {}
                }
            } else if let Some((stored, n)) = &mut stock[i] {
                if stored == key {
                    let moved = left.min(*n);
                    *n -= moved;
                    left -= moved;
                    if *n == 0 {
                        stock[i] = None
                    }
                }
            }
        }
        let moved = amount - left;
        if moved > 0 && !simulate {
            store_fluids(&mut machine, FLUID_STOCK, &stock);
            wake_interface(&mut machine);
            if self.interface_store(owner, &machine).is_err() {
                return Some(0);
            }
        }
        Some(moved)
    }
}
