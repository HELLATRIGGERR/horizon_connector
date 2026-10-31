//! Wireless terminal item power, linking and access-point selection.
//! Derived from AE2 19.2.18 WirelessTerminalMenuHost, ItemMenuHost and
//! WirelessTerminalItem (LGPL-3.0-or-later).
use super::{Game, lan::LanHost};
use horizon_client::{ae2_world::MachineKind, containers::BlockEntity, items::ItemStack};
use horizon_world::nbt::Tag;
type Pos = (i32, i32, i32);
#[derive(Clone, Debug)]
pub(super) struct WirelessContext {
    pub source: usize,
    pub target_dim: String,
    pub target: Pos,
    pub item: String,
    pub idle: u8,
    // The menu belongs to the actual item, not merely a slot containing an
    // equal replacement. This identity survives changes to its battery.
    pub source_owner: horizon_client::ae2_stack_identity::StackRef,
}
pub(super) fn is_wireless(stack: &ItemStack) -> bool {
    stack.count == 1
        && matches!(
            stack.item.as_ref(),
            "ae2:wireless_terminal" | "ae2:wireless_crafting_terminal"
        )
}
/// KeyTypeSelection.forStack: absence enables every native type; an explicit
/// empty selection stays empty until the player selects a type.
pub(super) fn enabled_key_types(stack: &ItemStack) -> Result<(bool, bool), String> {
    if !is_wireless(stack) { return Err("not a wireless terminal".into()); }
    let Some(value) = stack.data().and_then(|data|data.other("ae2:enabled_key_types")) else {return Ok((true,true));};
    let Tag::List(entries)=value else {return Err("invalid enabled key types".into());};
    let mut enabled=(false,false);
    for entry in entries {
        let Tag::String(name)=entry else {return Err("invalid enabled key type".into());};
        match name.to_string().map_err(|e|e.to_string())?.as_str() {
            "ae2:i" => enabled.0=true,
            "ae2:f" => enabled.1=true,
            _ => {},
        }
    }
    Ok(enabled)
}
pub(super) fn store_enabled_key_types(stack: &mut ItemStack, enabled:(bool,bool)) -> Result<(),String> {
    if !is_wireless(stack) { return Err("not a wireless terminal".into()); }
    let mut values=Vec::new();
    if enabled.0 {values.push(Tag::String("ae2:i".into()));}
    if enabled.1 {values.push(Tag::String("ae2:f".into()));}
    stack.data_mut().set_other("ae2:enabled_key_types",Some(Tag::List(values)));
    horizon_client::ae2_stack_identity::synchronize_tracked(stack);
    Ok(())
}
/// Clicking a checkbox cannot disable the final selected type.
pub(super) fn toggle_key_type(enabled:(bool,bool), fluid:bool) -> (bool,bool) {
    let (item,liquid)=enabled;
    if fluid {(item, if liquid && !item {true} else {!liquid})}
    else {(if item && !liquid {true} else {!item},liquid)}
}
/// KeyTypeSelectionButton's shift-click cycles all, item, fluid, all.
pub(super) fn cycle_key_types(enabled:(bool,bool)) -> (bool,bool) {
    match enabled {(true,true)=>(true,false),(true,false)=>(false,true),(false,true)=>(true,true),(false,false)=>(true,false)}
}
fn custom(stack: &ItemStack) -> Result<horizon_world::nbt::Compound, String> {
    match stack.data().and_then(|d| d.other("minecraft:custom_data")) {
        Some(Tag::Compound(c)) => Ok(c.clone()),
        None => Ok(Default::default()),
        _ => Err("invalid wireless terminal data".into()),
    }
}

/// AE2 stores the nine physical crafting slots in ItemContainerContents; old
/// native saves also carry the legacy custom-data envelope.
const CRAFTING_GRID: &str = "ae2_wireless_crafting_grid";
pub(super) fn crafting_grid(stack: &ItemStack) -> Result<Vec<Option<ItemStack>>, String> {
    if stack.item.as_ref() != "ae2:wireless_crafting_terminal" || stack.count != 1 {
        return Err("wireless crafting terminal moved".into());
    }
    let mut slots = vec![None; 9];
    if let Some(source)=stack.data().and_then(|d|d.other("ae2:crafting_inv")) {
        let Tag::List(entries)=source else{return Err("invalid source wireless crafting grid".into());};
        for entry in entries {
            let Some(Tag::Int(index))=entry.get("slot") else{return Err("invalid source wireless crafting slot".into());};
            let index=usize::try_from(*index).ok().filter(|index|*index<slots.len()).ok_or("invalid source wireless crafting slot")?;
            let item=entry.get("item").and_then(horizon_client::save::item_from_nbt).filter(|item|item.count>0 && item.count<=99).ok_or("invalid source wireless crafting item")?;
            if slots[index].replace(item).is_some(){return Err("duplicate source wireless crafting slot".into());}
        }
        return Ok(slots);
    }
    let data = custom(stack)?;
    let Some(encoded) = data.get(&CRAFTING_GRID.into()) else { return Ok(slots); };
    let Tag::List(entries) = encoded else { return Err("invalid wireless crafting grid".into()); };
    for entry in entries {
        let index = match entry.get("Slot") {
            Some(Tag::Byte(n)) if (0..9).contains(n) => *n as usize,
            _ => return Err("invalid wireless crafting slot".into()),
        };
        if slots[index].is_some() || !matches!(entry.get("count"), Some(Tag::Int(n)) if *n > 0) {
            return Err("invalid wireless crafting item".into());
        }
        slots[index] = Some(horizon_client::save::item_from_nbt(entry).ok_or("invalid wireless crafting item")?);
    }
    Ok(slots)
}
pub(super) fn store_crafting_grid(stack: &mut ItemStack, slots: &[Option<ItemStack>]) -> Result<(), String> {
    if stack.item.as_ref() != "ae2:wireless_crafting_terminal" || stack.count != 1 || slots.len() != 9
        || slots.iter().flatten().any(|s| s.count == 0 || s.count > 99)
    {
        return Err("invalid wireless crafting owner or grid".into());
    }
    let mut data = custom(stack)?;
    data.insert(CRAFTING_GRID.into(), horizon_client::save::encode_items(slots));
    stack.data_mut().set_other("minecraft:custom_data", Some(Tag::Compound(data)));
    let source=slots.iter().enumerate().filter_map(|(slot,item)|item.as_ref().map(|item|Tag::compound([
        ("slot",Tag::Int(slot as i32)),("item",horizon_client::save::item_to_nbt(item)),
    ]))).collect();
    stack.data_mut().set_other("ae2:crafting_inv",Some(Tag::List(source)));
    horizon_client::ae2_stack_identity::synchronize_tracked(stack);
    Ok(())
}
pub(super) fn link(stack: &mut ItemStack, dim: &str, pos: Pos) -> Result<(), String> {
    if !is_wireless(stack) {
        return Err("not a wireless terminal".into());
    }
    let mut data = custom(stack)?;
    let target=Tag::compound([("dimension",Tag::String(dim.into())),("pos",Tag::IntArray(vec![pos.0,pos.1,pos.2]))]);
    data.insert("ae2_wireless_link".into(),target.clone());
    stack
        .data_mut()
        .set_other("minecraft:custom_data", Some(Tag::Compound(data)));
    stack.data_mut().set_other("ae2:wireless_link_target",Some(target));
    Ok(())
}
pub(super) fn target(stack: &ItemStack) -> Option<(String, Pos)> {
    let components=stack.data()?;
    let data=match components.other("ae2:wireless_link_target") {
        Some(value)=>value,
        None=>components.other("minecraft:custom_data")?.get("ae2_wireless_link")?,
    };
    let Tag::String(dim) = data.get("dimension")? else {
        return None;
    };
    let Tag::IntArray(pos) = data.get("pos")? else {
        return None;
    };
    let [x, y, z] = pos.as_slice() else {
        return None;
    };
    Some((dim.to_string().ok()?, (*x, *y, *z)))
}
fn cards(stack: &ItemStack) -> Result<u32, String> {
    if !is_wireless(stack) {
        return Err("not a wireless terminal".into());
    }
    let upgrades=horizon_client::ae2_upgrade_inventory::Inventory::read(stack)?;
    Ok(upgrades.slots.iter().flatten().count() as u32)
}
pub(super) fn energy(stack: &ItemStack) -> Result<(f64, f64), String> {
    let base = horizon_client::ae2_common_config::wireless().terminal_capacity;
    let legacy_max = base * (1.0 + f64::from(cards(stack)?));
    let default = if stack.data().and_then(|data|data.other("ae2:upgrades")).is_some() {base} else {legacy_max};
    let max = horizon_client::ae2_powered_items::component_power_value(stack,"ae2:energy_capacity","ae2:energy_capacity",default)?;
    let power = horizon_client::ae2_powered_items::component_power_value(stack,"ae2:stored_energy","ae2_wireless_energy",0.0)?;
    Ok((power, max))
}
pub(super) fn charge_rate(stack: &ItemStack) -> Result<f64, String> {
    Ok(800.0 * (1.0 + f64::from(cards(stack)?)))
}
pub(super) fn transfer_energy(
    stack: &mut ItemStack,
    amount: f64,
    insert: bool,
    simulate: bool,
) -> Result<f64, String> {
    if !amount.is_finite() || amount < 0.0 {
        return Err("invalid wireless power amount".into());
    }
    let (current, max) = energy(stack)?;
    let moved = amount.min(if insert {
        (max - current).max(0.0)
    } else {
        current
    });
    if !simulate && (moved > 0.0 || (insert && current > max)) {
        let next = (current + if insert {amount.min(max-current)} else {-moved}).max(0.0);
        horizon_client::ae2_powered_items::set_component_power_value(stack,"ae2:stored_energy","ae2_wireless_energy",(next>=0.0001).then_some(next))?;
    }
    Ok(moved)
}
/// ItemMenuHost charges CONFIG-scaled energy but converts back to nominal
/// energy when it computes the number of buffered menu ticks.
fn extract_menu_energy(stack: &mut ItemStack, amount: f64, multiplier: f64) -> Result<f64, String> {
    Ok(transfer_energy(stack, (amount * multiplier).min(f64::MAX), false, false)? / multiplier)
}
impl Game<'_> {
    pub(super) fn ae2_wireless_nearest(
        &self,
        host: &LanHost,
        conn: u32,
        ctx: &WirelessContext,
    ) -> Option<(Pos, f64)> {
        let guest = host
            .hub
            .guests
            .iter()
            .find(|g| g.conn == conn && !g.spectator() && g.dimension == self.dim.id())?;
        if !self.ae2_wireless_source_valid(host, conn, ctx) {
            return None;
        }
        let target_dim = super::Dim::from_id(&ctx.target_dim);
        if target_dim.id() != ctx.target_dim {
            return None;
        }
        let target = self.ae2_dimension_address(target_dim, ctx.target)?;
        if !matches!(self.ae2_block_entity(target),Some(BlockEntity::Ae2(m)) if m.kind==MachineKind::WirelessAccessPoint)
        {
            return None;
        }
        let net = self.ae2_network();
        let analysis = net.analyze().ok()?;
        let anchor = analysis.nodes.get(&target.network_pos())?;
        analysis.components[anchor.component]
            .positions
            .iter()
            .filter_map(|p| {
                if !analysis.nodes.get(p).is_some_and(|n| n.online) {
                    return None;
                }
                let address = super::ae2_global::GridAddress::from_network(*p)?;
                let (dim, local) = self.ae2_address_parts(address)?;
                if dim != self.dim {
                    return None;
                }
                let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity(address) else {
                    return None;
                };
                if m.kind != MachineKind::WirelessAccessPoint {
                    return None;
                }
                let boosters = m
                    .slots
                    .first()
                    .and_then(Option::as_ref)
                    .filter(|s| s.item.as_ref() == "ae2:wireless_booster")
                    .map_or(0, |s| s.count.min(64));
                let d = (f64::from(local.0) - guest.position[0]).powi(2)
                    + (f64::from(local.1) - guest.position[1]).powi(2)
                    + (f64::from(local.2) - guest.position[2]).powi(2);
                (d < horizon_client::ae2_common_config::wireless()
                    .link
                    .range(boosters)
                    .powi(2))
                .then_some((local, d.sqrt()))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }
    fn ae2_wireless_distance(
        &self,
        host: &LanHost,
        conn: u32,
        ctx: &WirelessContext,
    ) -> Option<f64> {
        self.ae2_wireless_nearest(host, conn, ctx)
            .map(|(_, distance)| distance)
    }
    pub(super) fn ae2_wireless_source_valid(
        &self,
        host: &LanHost,
        conn: u32,
        ctx: &WirelessContext,
    ) -> bool {
        let Some(g) = host
            .hub
            .guests
            .iter()
            .find(|g| g.conn == conn && !g.spectator())
        else {
            return false;
        };
        let stack = if ctx.source == super::shield::OFFHAND_SLOT {
            g.offhand.as_ref()
        } else {
            g.inventory.get(ctx.source).and_then(Option::as_ref)
        };
        stack.is_some_and(|stack| {
            is_wireless(stack)
                && stack.item.as_ref() == ctx.item
                && target(stack) == Some((ctx.target_dim.clone(), ctx.target))
                && ctx.source_owner.tracks(stack)
                && (ctx.item != "ae2:wireless_crafting_terminal" || crafting_grid(stack).is_ok())
        })
    }
    pub(super) fn ae2_wireless_validate(
        &self,
        host: &LanHost,
        conn: u32,
        ctx: &WirelessContext,
    ) -> bool {
        let Some(distance) = self.ae2_wireless_distance(host, conn, ctx) else {
            return false;
        };
        if horizon_client::ae2_common_config::wireless()
            .link
            .drain(distance)
            <= 0.0
        {
            return true;
        }
        let Some(g) = host.hub.guests.iter().find(|g| g.conn == conn) else {
            return false;
        };
        if g.creative() || ctx.idle > 0 {
            return true;
        }
        let stack = if ctx.source == super::shield::OFFHAND_SLOT {
            g.offhand.as_ref()
        } else {
            g.inventory.get(ctx.source).and_then(Option::as_ref)
        };
        stack
            .and_then(|s| energy(s).ok())
            .is_some_and(|(p, _)| p > 0.0)
    }
    pub(super) fn ae2_wireless_tick(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        ctx: &mut WirelessContext,
    ) -> bool {
        let Some(distance) = self.ae2_wireless_distance(host, conn, ctx) else {
            return false;
        };
        if host
            .hub
            .guests
            .iter()
            .find(|g| g.conn == conn)
            .is_some_and(|g| g.creative())
        {
            return true;
        }
        if ctx.idle > 0 {
            ctx.idle -= 1;
            return true;
        }
        let drain = horizon_client::ae2_common_config::wireless()
            .link
            .drain(distance);
        if drain <= 0.0 {
            return self.ae2_wireless_validate(host, conn, ctx);
        }
        let multiplier = horizon_client::ae2_common_config::gameplay().usage_multiplier;
        let extracted = self
            .as_guest(host, conn, |game| {
                let Some(stack) = game.stack_in_mut(ctx.source).as_mut() else {
                    return 0.0;
                };
                extract_menu_energy(stack, drain * 10.0, multiplier).unwrap_or(0.0)
            })
            .unwrap_or(0.0);
        ctx.idle = (extracted / drain).ceil().clamp(0.0, 10.0) as u8;
        ctx.idle > 0
    }
    pub(super) fn lan_open_ae2_wireless(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        hand: i32,
    ) -> bool {
        let Some(g) = host.hub.guests.iter().find(|g| g.conn == conn) else {
            return false;
        };
        if g.spectator() {
            return false;
        }
        let source = if hand == 1 {
            super::shield::OFFHAND_SLOT
        } else {
            g.selected
        };
        self.lan_open_ae2_wireless_source(host, conn, source)
    }
    /// Open the actual inventory item at its source slot, including main
    /// inventory and offhand. Never temporarily change the selected hotbar.
    pub(super) fn lan_open_ae2_wireless_source(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        source: usize,
    ) -> bool {
        let Some(g) = host.hub.guests.iter().find(|g| g.conn == conn) else {
            return false;
        };
        if g.spectator() {
            return false;
        }
        let stack = if source == super::shield::OFFHAND_SLOT {
            g.offhand.as_ref()
        } else {
            g.inventory.get(source).and_then(Option::as_ref)
        };
        let Some(stack) = stack.filter(|s| is_wireless(s)) else {
            return false;
        };
        let Some((target_dim, target)) = target(stack) else {
            self.as_guest(host, conn, |g| {
                g.system_message(horizon_client::text::Text::literal(
                    "Wireless terminal is not linked",
                ))
            });
            return true;
        };
        let item = stack.item.to_string();
        let Some(guest) = host.hub.guests.iter_mut().find(|g| g.conn == conn) else { return false; };
        let stack = if source == super::shield::OFFHAND_SLOT {
            guest.offhand.as_mut()
        } else {
            guest.inventory.get_mut(source).and_then(Option::as_mut)
        };
        let Some(stack) = stack else { return false; };
        let source_owner = horizon_client::ae2_stack_identity::StackRef::capture(stack);
        let ctx = WirelessContext {
            source,
            target_dim,
            target,
            item,
            idle: 0,
            source_owner,
        };
        if !self.ae2_wireless_validate(host, conn, &ctx) {
            self.as_guest(host, conn, |g| {
                g.system_message(horizon_client::text::Text::literal(
                    "Wireless terminal: network out of range or battery empty",
                ))
            });
            return true;
        }
        let Some((anchor, _)) = self.ae2_wireless_nearest(host, conn, &ctx) else {
            return true;
        };
        self.lan_open_ae2_wireless_terminal(host, conn, anchor, ctx)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_key_types_default_empty_toggle_cycle_and_persist() {
        let mut terminal=ItemStack::new("ae2:wireless_terminal",1);
        assert_eq!(enabled_key_types(&terminal).unwrap(),(true,true));
        let owner=horizon_client::ae2_stack_identity::StackRef::capture(&mut terminal);
        store_enabled_key_types(&mut terminal,(false,false)).unwrap();
        assert!(owner.tracks(&terminal));
        assert_eq!(enabled_key_types(&terminal).unwrap(),(false,false));
        assert_eq!(toggle_key_type((false,false),false),(true,false));
        assert_eq!(toggle_key_type((true,false),false),(true,false));
        assert_eq!(toggle_key_type((true,false),true),(true,true));
        assert_eq!(cycle_key_types((true,true)),(true,false));
        assert_eq!(cycle_key_types((true,false)),(false,true));
        assert_eq!(cycle_key_types((false,true)),(true,true));
        store_enabled_key_types(&mut terminal,(false,true)).unwrap();
        let saved=horizon_client::save::item_to_nbt(&terminal);
        let restored=horizon_client::save::item_from_nbt(&saved).unwrap();
        assert_eq!(enabled_key_types(&restored).unwrap(),(false,true));
        terminal.data_mut().set_other("ae2:enabled_key_types",Some(Tag::Int(2)));
        assert!(enabled_key_types(&terminal).is_err());
    }
    #[test]
    fn menu_buffer_debits_configured_energy_and_returns_nominal_ticks() {
        let mut terminal = ItemStack::new("ae2:wireless_terminal", 1);
        transfer_energy(&mut terminal, 25.0, true, false).unwrap();
        assert_eq!(extract_menu_energy(&mut terminal, 10.0, 3.0).unwrap(), 25.0 / 3.0);
        assert_eq!(energy(&terminal).unwrap().0, 0.0);
    }
    #[test]
    fn wireless_crafting_grid_keeps_real_owner_and_power_across_save() {
        let mut terminal = ItemStack::new("ae2:wireless_crafting_terminal", 1);
        link(&mut terminal, "minecraft:overworld", (1, 64, 2)).unwrap();
        transfer_energy(&mut terminal, 800.0, true, false).unwrap();
        let owner = horizon_client::ae2_stack_identity::StackRef::capture(&mut terminal);
        let mut marked = ItemStack::new("minecraft:oak_log", 3);
        marked.data_mut().custom_name = Some("physical ingredient".into());
        let mut grid = vec![None; 9];
        grid[7] = Some(marked);
        store_crafting_grid(&mut terminal, &grid).unwrap();
        transfer_energy(&mut terminal, 25.0, false, false).unwrap();
        assert!(owner.tracks(&terminal));
        assert_eq!(crafting_grid(&terminal).unwrap(), grid);
        assert_eq!(energy(&terminal).unwrap().0, 775.0);
        assert_eq!(target(&terminal), Some(("minecraft:overworld".into(), (1, 64, 2))));

        let saved = horizon_client::save::item_to_nbt(&terminal);
        let restored = horizon_client::save::item_from_nbt(&saved).unwrap();
        assert_eq!(crafting_grid(&restored).unwrap(), grid);
        assert_eq!(energy(&restored).unwrap().0, 775.0);
        assert!(!owner.tracks(&restored), "equal replacement is a different owner");
        assert!(crafting_grid(&ItemStack::new("ae2:wireless_terminal", 1)).is_err());

        let mut invalid = terminal.clone();
        let mut data = custom(&invalid).unwrap();
        data.insert(CRAFTING_GRID.into(), Tag::List(vec![Tag::compound([
            ("Slot", Tag::Byte(9)), ("id", Tag::String("minecraft:diamond".into())), ("count", Tag::Int(1))
        ])]));
        invalid.data_mut().set_other("minecraft:custom_data", Some(Tag::Compound(data)));
        assert_eq!(crafting_grid(&invalid).unwrap(),grid,"registered source inventory takes precedence over stale legacy data");
        invalid.data_mut().set_other("ae2:crafting_inv",None);
        assert!(crafting_grid(&invalid).is_err());
    }
    #[test]
    fn wireless_source_components_are_authoritative_and_survive_roundtrip() {
        let mut item=ItemStack::new("ae2:wireless_crafting_terminal",1);
        item.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("ae2_wireless_link",Tag::compound([
            ("dimension",Tag::String("minecraft:the_nether".into())),("pos",Tag::IntArray(vec![7,8,9]))
        ]))])));
        item.data_mut().set_other("ae2:wireless_link_target",Some(Tag::compound([
            ("dimension",Tag::String("minecraft:overworld".into())),("pos",Tag::IntArray(vec![1,64,2]))
        ])));
        item.data_mut().set_other("ae2:crafting_inv",Some(Tag::List(vec![Tag::compound([
            ("slot",Tag::Int(7)),("item",horizon_client::save::item_to_nbt(&ItemStack::new("minecraft:oak_log",3)))
        ])])));
        assert_eq!(target(&item),Some(("minecraft:overworld".into(),(1,64,2))));
        let grid=crafting_grid(&item).unwrap();assert_eq!(grid[7],Some(ItemStack::new("minecraft:oak_log",3)));
        let restored=horizon_client::save::item_from_nbt(&horizon_client::save::item_to_nbt(&item)).unwrap();
        assert_eq!(target(&restored),target(&item));assert_eq!(crafting_grid(&restored).unwrap(),grid);
        item.data_mut().set_other("ae2:wireless_link_target",Some(Tag::String("malformed".into())));
        assert!(target(&item).is_none(),"malformed source data cannot silently fall back to a different legacy target");
        item.data_mut().set_other("ae2:crafting_inv",Some(Tag::List(vec![Tag::compound([
            ("slot",Tag::Int(9)),("item",horizon_client::save::item_to_nbt(&ItemStack::new("minecraft:diamond",1)))
        ])])));
        assert!(crafting_grid(&item).is_err());
    }
    #[test]
    fn wireless_over_capacity_read_and_refill_match_source_power_sink() {
        let mut item = ItemStack::new("ae2:wireless_terminal", 1);
        let (_, cap) = energy(&item).unwrap();
        item.data_mut().set_other("minecraft:custom_data", Some(Tag::compound([("ae2_wireless_energy", Tag::Double((cap + 10.0).to_bits()))])));
        assert_eq!(energy(&item).unwrap(), (cap + 10.0, cap));
        assert_eq!(transfer_energy(&mut item, 5.0, true, true).unwrap(), 0.0);
        assert_eq!(energy(&item).unwrap().0, cap + 10.0);
        // AEBasePoweredItem.injectAEPower clamps an existing excess during
        // an actual refill, while accepting none of the newly supplied power.
        assert_eq!(transfer_energy(&mut item, 5.0, true, false).unwrap(), 0.0);
        assert_eq!(energy(&item).unwrap().0, cap);
        let restored = horizon_client::save::item_from_nbt(&horizon_client::save::item_to_nbt(&item)).unwrap();
        assert_eq!(energy(&restored).unwrap().0, cap);
    }
    #[test]
    fn source_wireless_energy_and_capacity_override_native_aliases() {
        let mut item=ItemStack::new("ae2:wireless_terminal",1);
        item.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("ae2_wireless_energy",Tag::Double(90.0f64.to_bits()))])));
        item.data_mut().set_other("ae2:stored_energy",Some(Tag::Double(20.0f64.to_bits())));
        item.data_mut().set_other("ae2:energy_capacity",Some(Tag::Double(50.0f64.to_bits())));
        assert_eq!(energy(&item).unwrap(),(20.0,50.0));
        transfer_energy(&mut item,10.0,true,false).unwrap();
        assert_eq!(energy(&item).unwrap(),(30.0,50.0));
        assert_eq!(item.data().unwrap().other("ae2:stored_energy"),Some(&Tag::Double(30.0f64.to_bits())));
        let restored=horizon_client::save::item_from_nbt(&horizon_client::save::item_to_nbt(&item)).unwrap();
        assert_eq!(energy(&restored).unwrap(),(30.0,50.0));
    }
    #[test]
    fn wireless_link_and_power_are_physical_components() {
        let mut item = ItemStack::new("ae2:wireless_terminal", 1);
        link(&mut item, "ae2:spatial_storage", (192, 64, 192)).unwrap();
        assert_eq!(
            target(&item),
            Some(("ae2:spatial_storage".into(), (192, 64, 192)))
        );
        assert_eq!(energy(&item).unwrap(), (0.0, 1_600_000.0));
        assert_eq!(
            transfer_energy(&mut item, 800.0, true, false).unwrap(),
            800.0
        );
        assert_eq!(transfer_energy(&mut item, 90.0, false, true).unwrap(), 90.0);
        assert_eq!(energy(&item).unwrap().0, 800.0);
        assert_eq!(
            transfer_energy(&mut item, 90.0, false, false).unwrap(),
            90.0
        );
        let restored =
            horizon_client::save::item_from_nbt(&horizon_client::save::item_to_nbt(&item)).unwrap();
        assert_eq!(energy(&restored).unwrap().0, 710.0);
        assert_eq!(target(&restored), target(&item));
    }
    #[test]
    fn wireless_energy_cards_use_source_multiplier() {
        let mut stack = ItemStack::new("ae2:wireless_terminal", 1);
        let card = Tag::compound([
            ("id", Tag::String("ae2:energy_card".into())),
            ("count", Tag::Int(1)),
        ]);
        stack.data_mut().set_other(
            "minecraft:custom_data",
            Some(Tag::compound([(
                "ae2_wireless_upgrades",
                Tag::List(vec![card.clone(), card]),
            )])),
        );
        assert_eq!(energy(&stack).unwrap(), (0.0, 4_800_000.0));
        assert_eq!(charge_rate(&stack).unwrap(), 2400.0);
    }
}
