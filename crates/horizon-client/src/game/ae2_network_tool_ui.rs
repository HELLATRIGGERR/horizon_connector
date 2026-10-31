//! Server-authoritative Network Tool pocket. The cards remain in the held
//! item's physical minecraft:container component, including across reloads.
use super::{Game, lan::LanHost};
use horizon_client::{ae2_network_tool::{Inventory, TOOL}, items::ItemStack, menu::{Kind, Menu}};
use horizon_protocol::packets::play::{self, Clientbound as C};
use horizon_client::containers::BlockEntity;
use horizon_world::nbt::Tag;

pub(super) struct GuestNetworkTool {
    window: i32,
    state: i32,
    source: usize,
    snapshot: ItemStack,
    menu: Menu,
    status: Option<(i32,i32,i32)>,
    status_page: usize,
    status_pages: usize,
    refresh: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standalone_network_tool_source_order_and_filters() {
        use horizon_client::{banner::Banners, brewing::Brewing, crafting::Recipes, items::Items, menu::Input};
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/resources/26.3");
        if !root.join("items.json").exists() { return; }
        let items=Items::catalog(&root, &horizon_world::loot::PackOverlay::NONE).unwrap();let recipes=Recipes::load(&root, &items, &horizon_world::loot::PackOverlay::NONE).unwrap();
        let brewing=Brewing::load(&root, &items, &horizon_world::loot::PackOverlay::NONE);let banners=Banners::load(&root);
        let station_recipes=horizon_client::stations::StationRecipes::load(&root, &items, &horizon_world::loot::PackOverlay::NONE);
        let enchantments=horizon_client::enchant::Enchantments::default();let names=horizon_client::creative::Names::default();
        let env=horizon_client::menu::Env{items:&items,recipes:&recipes,brewing:&brewing,banners:&banners,
            station_recipes:&station_recipes,enchantments:&enchantments,names:&names,trade:None,maps:None};
        let card=|n|Some(ItemStack::new("ae2:speed_card",n));
        let mut menu=pocket_menu(0);menu.set_player_item(0,Some(ItemStack::new(TOOL,1)));
        menu.set_slot(&env,0,card(4));menu.click(&env,0,0,Input::QuickMove).unwrap();
        assert_eq!(menu.player_item(9).map(|s|s.count),Some(4));assert!(menu.player_item(8).is_none());
        let mut merge=pocket_menu(0);merge.set_player_item(0,Some(ItemStack::new(TOOL,1)));
        merge.set_player_item(5,card(62));merge.set_slot(&env,0,card(4));merge.click(&env,0,0,Input::QuickMove).unwrap();
        assert_eq!(merge.player_item(5).map(|s|s.count),Some(64));assert_eq!(merge.player_item(9).map(|s|s.count),Some(2));
        let mut filtered=pocket_menu(0);filtered.set_player_item(0,Some(ItemStack::new(TOOL,1)));
        filtered.set_slot(&env,0,Some(ItemStack::new("minecraft:dirt",4)));filtered.set_player_item(9,Some(ItemStack::new("minecraft:dirt",3)));
        filtered.click(&env,9,0,Input::QuickMove).unwrap();assert_eq!(filtered.slot(0).map(|s|s.count),Some(4));assert_eq!(filtered.player_item(9).map(|s|s.count),Some(3));
        for button in [0,1] {
            let mut gather=pocket_menu(0);gather.set_player_item(0,Some(ItemStack::new(TOOL,1)));
            gather.set_slot(&env,0,card(1));gather.set_player_item(5,card(3));gather.set_player_item(9,card(4));gather.carried=card(60);
            gather.click(&env,39,button,Input::PickupAll).unwrap();assert_eq!(gather.carried.as_ref().map(|s|s.count),Some(64));
            if button==0 {assert!(gather.slot(0).is_none());assert!(gather.player_item(5).is_none());assert_eq!(gather.player_item(9).map(|s|s.count),Some(4));}
            else {assert_eq!(gather.slot(0).map(|s|s.count),Some(1));assert_eq!(gather.player_item(5).map(|s|s.count),Some(3));assert!(gather.player_item(9).is_none());}
        }
    }
    #[test]
    fn network_tool_owner_commit_never_copies_a_pocket_to_a_different_tool() {
        let mut a=ItemStack::new(TOOL,1);
        let mut b=ItemStack::new(TOOL,1);
        let mut ai=Inventory {slots:std::array::from_fn(|_|None)};
        let mut bi=Inventory {slots:std::array::from_fn(|_|None)};
        ai.slots[0]=Some(ItemStack::new("ae2:speed_card",4));
        bi.slots[0]=Some(ItemStack::new("ae2:capacity_card",3));
        ai.store(&mut a).unwrap();bi.store(&mut b).unwrap();
        // A number-key exchange can move A while B occupies A's locator.
        assert_eq!(stage_owned_pocket(&a,Some(&b),&ai,&ai).unwrap(),None);
        assert_eq!(Inventory::read(&b).unwrap(),bi);
        let mut changed=ai.clone();changed.slots[0]=None;
        assert!(stage_owned_pocket(&a,Some(&b),&ai,&changed).is_err());
        let updated=stage_owned_pocket(&a,Some(&a),&ai,&changed).unwrap().unwrap();
        assert_eq!(Inventory::read(&updated).unwrap(),changed);
        assert_eq!(Inventory::read(&b).unwrap(),bi);
    }
}
fn held(g: &horizon_client::lan::guest::Guest, source: usize) -> Option<&ItemStack> {
    if source == 40 { g.offhand.as_ref() } else { g.inventory.get(source)?.as_ref() }
}
fn held_mut(g: &mut horizon_client::lan::guest::Guest, source: usize) -> Option<&mut ItemStack> {
    if source == 40 { g.offhand.as_mut() } else { g.inventory.get_mut(source)?.as_mut() }
}
fn pocket_menu(source: usize) -> Menu {
    let mut menu=Menu::new(Kind::Chest(1));
    menu.network_tool_slot=Some(source);
    menu.network_tool_source_order=true;
    menu
}
/// A pocket is written only to the same tool that supplied its contents.
/// A player-slot swap can move that tool and leave a different one at source;
/// an unchanged pocket needs no write in that case.
fn stage_owned_pocket(
    original: &ItemStack, current: Option<&ItemStack>, before: &Inventory, after: &Inventory,
) -> Result<Option<ItemStack>, String> {
    if before == after { return Ok(None); }
    if current != Some(original) { return Err("network tool owner changed during pocket edit".into()); }
    let mut updated=original.clone();
    after.store(&mut updated)?;
    Ok(Some(updated))
}
impl Game<'_> {
    pub(super) fn ae2_network_tool_monitor_face(&self,pos:(i32,i32,i32),face:&str)->bool {
        let is_monitor=|id:&str|matches!(id,"ae2:storage_monitor"|"ae2:conversion_monitor");
        if self.remote.is_some() {
            return self.ae2_visual_cache.get(&pos).is_some_and(|v|v.parts.iter().any(|p|p.face==face && is_monitor(&p.id)));
        }
        if let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get(&pos) {
            if let Some(Tag::List(parts))=machine.config.get(&"parts".into()) {
                return parts.iter().any(|part|part.get("face")==Some(&Tag::String(face.into())) && matches!(part.get("id"),Some(Tag::String(id)) if id.to_string().is_ok_and(|id|is_monitor(&id))));
            }
        }
        false
    }
    pub(super) fn lan_open_ae2_network_tool(&mut self, host: &mut LanHost, conn: u32, hand: i32) -> bool {
        self.lan_open_ae2_network_tool_inner(host,conn,hand,None)
    }
    pub(super) fn lan_open_ae2_network_tool_at(&mut self, host: &mut LanHost, conn: u32, hand: i32, pos: (i32,i32,i32), face: i32) -> bool {
        let Some(g)=host.hub.guests.iter().find(|g|g.conn==conn) else {return false;};
        if g.sneaking || !g.within_block_range([pos.0,pos.1,pos.2],0.0) {return false;}
        let source=if hand==1 {40} else {g.selected};
        if !held(g,source).is_some_and(|s|s.item.as_ref()==TOOL) {return false;}
        // AbstractMonitorPart.showNetworkInfo returns false; its own
        // interaction must run before the tool opens a status screen.
        let face=match face {0=>"down",1=>"up",2=>"north",3=>"south",4=>"west",5=>"east",_=>return false};
        if self.ae2_network_tool_monitor_face(pos,face) {return false;}
        let network=self.ae2_network();
        let status=self.ae2_local_address(pos).filter(|p|network.nodes.contains_key(&p.network_pos())).map(|_|pos);
        self.lan_open_ae2_network_tool_inner(host,conn,hand,status)
    }
    fn lan_open_ae2_network_tool_inner(&mut self, host: &mut LanHost, conn: u32, hand: i32, status: Option<(i32,i32,i32)>) -> bool {
        let Some(g) = host.hub.guests.iter().find(|g| g.conn == conn) else { return false; };
        let source = if hand == 1 { 40 } else { g.selected };
        let Some(tool) = held(g, source).filter(|s| s.item.as_ref() == TOOL).cloned() else { return false; };
        let Ok(_) = Inventory::read(&tool) else { return true; };
        self.lan_ae2_network_tool_closed(host, conn, None);
        self.lan_ae2_anchor_closed(host, conn, None);
        self.lan_craft_confirm_closed(host, conn, None);
        self.lan_pattern_access_closed(host, conn, None);
        self.lan_ae2_portable_closed(host, conn, None);
        self.lan_ae2_bus_closed(host, conn, None);
        self.lan_ae2_terminal_closed(host, conn, None);
        self.lan_ae2_machine_closed(host, conn, None);
        self.lan_station_closed(host, conn, None);
        self.lan_crafting_closed(host, conn, None);
        self.guest_container_closed(host, conn);
        let title=if status.is_some(){"AE2 Network Status"}else{"AE2 Network Tool"};
        let kind=if status.is_some(){"minecraft:generic_9x3"}else{"minecraft:generic_9x1"};
        let Some(window) = host.hub.open_window(conn, kind, horizon_protocol::text::literal(title)) else { return true; };
        let menu = pocket_menu(source);
        host.play.entry(conn).or_default().ae2_network_tool = Some(GuestNetworkTool { window, state: 0, source, snapshot: tool, menu, status, status_page:0, status_pages:1, refresh: 0 });
        self.lan_send_ae2_network_tool(host, conn);
        true
    }
    fn lan_send_ae2_network_tool(&mut self, host: &mut LanHost, conn: u32) {
        let Some((source, window, state, status, requested_page)) = host.play.get(&conn).and_then(|p| p.ae2_network_tool.as_ref()).map(|s| (s.source,s.window,s.state,s.status,s.status_page)) else { return; };
        let status_lines=status.and_then(|pos| {
            let network=self.ae2_network();let analysis=network.analyze().ok()?;
            let node=self.ae2_local_address(pos)?.network_pos();let state=analysis.nodes.get(&node)?;
            let component=analysis.components.get(state.component)?;
            let power=analysis.power_components.get(component.power_component)?;
            let capacity=power.positions.iter().filter_map(|p|network.nodes.get(p)).map(|d|d.kind.energy_capacity()).sum::<f64>();
            let mut groups=std::collections::BTreeMap::<String,usize>::new();
            for p in &component.positions {
                let Some(address)=super::ae2_global::GridAddress::from_network(*p) else {continue;};
                if let Some(block)=self.ae2_grid_block(address) {
                    let id=self.res.facts().get(block).name.to_string();
                    if id.starts_with("ae2:") {*groups.entry(id).or_default()+=1;}
                }
                if let Some(BlockEntity::Ae2(machine))=self.ae2_block_entity(address) {
                    if let Some(Tag::List(parts))=machine.config.get(&"parts".into()) {
                        for part in parts {if let Some(Tag::String(id))=part.get("id") {if let Ok(id)=id.to_string() {*groups.entry(id).or_default()+=1;}}}
                    }
                }
            }
            let available=if power.energy>=f64::MAX/2.0 {"Available: unlimited".into()} else {format!("Stored: {:.2e} AE",power.energy)};
            let mut lines=vec![format!("Channels: {}",component.channels_in_use),available,format!("Capacity: {:.2e} AE",capacity)];
            let pages=groups.len().max(1).div_ceil(8);
            let page=requested_page.min(pages-1);
            for (id,count) in groups.into_iter().skip(page*8).take(8) {lines.push(format!("{} x {}",count,id.strip_prefix("ae2:").unwrap_or(&id).replace('_'," ")));}
            while lines.len()<11 {lines.push(String::new());}
            lines.push(format!("Idle: {:.2} AE/t",component.idle_per_tick));
            lines.push(format!("Nodes: {}",component.positions.len()));
            lines.push(format!("State: {}",if state.online{"online"}else{"offline"}));
            Some((lines,page,pages))
        });
        let Some(g)=host.hub.guests.iter().find(|g|g.conn==conn) else {return;};
        let Some(tool)=held(g,source) else {return;};
        let Ok(inv)=Inventory::read(tool) else {return;};
        let (lines,page,pages)=status_lines.unwrap_or_else(||(vec!["Network unavailable".into()],0,1));
        let mut items:Vec<_>=if status.is_some() {
            (0..27).map(|i|lines.get(i).map(|line|{let mut s=ItemStack::new("minecraft:paper",1);s.data_mut().custom_name=Some(horizon_client::item_data::text_name(line.clone()));s})).collect()
        } else {inv.slots.into_iter().collect()};
        items.extend((9..36).chain(0..9).map(|i|g.inventory[i].clone()));
        let carried=g.carried.clone();
        let Some(s)=host.play.get_mut(&conn).and_then(|p|p.ae2_network_tool.as_mut()) else {return;};
        s.snapshot=tool.clone();s.state=(state+1)&0x7fff;s.status_page=page;s.status_pages=pages;
        let next_state=s.state;
        let wire_items=items.iter().map(|v|host.hub.stack(v)).collect();
        let wire_carried=host.hub.stack(&carried);
        host.hub.send_to(conn,&C::ContainerSetContent(play::ContainerSetContent {container:window,state:next_state,items:wire_items,carried:wire_carried}));
    }
    pub(super) fn lan_ae2_network_tool_click(&mut self,host:&mut LanHost,conn:u32,p:&play::ContainerClick)->bool {
        let Some(mut session)=host.play.get_mut(&conn).and_then(|v|v.ae2_network_tool.take()) else {return false;};
        if p.container!=session.window {host.play.entry(conn).or_default().ae2_network_tool=Some(session);return false;}
        let Some(gi)=host.hub.guests.iter().position(|g|g.conn==conn) else {return true;};
        let current=held(&host.hub.guests[gi],session.source).cloned();
        let Some(tool)=current.filter(|s|Inventory::read(s).is_ok()) else {
            host.play.entry(conn).or_default().ae2_network_tool=Some(session);
            self.lan_ae2_network_tool_closed(host,conn,None);
            host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose{container:p.container}));
            return true;
        };
        if session.snapshot!=tool {
            // StackDependentSupplier refreshes the inventory when the stack at
            // the locator changes; pending drags cannot own the old pocket.
            session.menu=pocket_menu(session.source);
            session.snapshot=tool.clone();
        }
        if session.status.is_some() {
            if super::lan_crafting::input(p.input)==horizon_client::menu::Input::Pickup {
                if p.slot==25 {session.status_page=session.status_page.saturating_sub(1);}
                if p.slot==26 {session.status_page=(session.status_page+1).min(session.status_pages.saturating_sub(1));}
            }
            host.play.entry(conn).or_default().ae2_network_tool=Some(session);
            self.lan_send_ae2_network_tool(host,conn);return true;
        }
        // ServerGamePacketListenerImpl applies the action to the current
        // authoritative menu even when the client's revision is stale, then
        // sends a full resync. Dropping the click makes rapid use unreliable.
        let inv=Inventory::read(&tool).expect("validated tool");
        let input=super::lan_crafting::input(p.input);
        if inv.slots.iter().flatten().any(|s|s.count==0 || s.count>self.res.items().stack_max(&s).min(99)) {
            host.play.entry(conn).or_default().ae2_network_tool=Some(session);
            host.hub.send_inventory(gi);self.lan_send_ae2_network_tool(host,conn);return true;
        }
        let before_inventory=host.hub.guests[gi].inventory.clone();
        let before_armor=host.hub.guests[gi].armor.clone();
        let before_offhand=host.hub.guests[gi].offhand.clone();
        let before_carried=host.hub.guests[gi].carried.clone();
        let random=self.world.random.clone();
        let (result,effects)=self.guest_menu_op_deferred(host,gi,&mut session.menu,|menu,env| {
            for (i,item) in inv.slots.iter().cloned().enumerate(){menu.set_slot(env,i,item);}
            menu.click(env,i32::from(p.slot),i32::from(p.button),input)?;
            Ok::<_,&'static str>(Inventory {slots:std::array::from_fn(|i|menu.slot(i).cloned())})
        });
        let result=result.map_err(str::to_owned).and_then(|after| {
            stage_owned_pocket(&tool,held(&host.hub.guests[gi],session.source),&inv,&after)
        });
        match result {
            Err(_) => {
                let g=&mut host.hub.guests[gi];
                g.inventory=before_inventory;g.armor=before_armor;g.offhand=before_offhand;g.carried=before_carried;
                self.world.random=random;
                for stack in inv.slots.iter().chain(g.inventory.iter()).chain(g.armor.iter())
                    .chain(std::iter::once(&g.offhand)).chain(std::iter::once(&g.carried)).flatten()
                {horizon_client::ae2_stack_identity::synchronize_tracked(stack);}
                session.menu=pocket_menu(session.source);
                host.play.entry(conn).or_default().ae2_network_tool=Some(session);
                host.hub.send_inventory(gi);self.lan_send_ae2_network_tool(host,conn);
                return true;
            }
            Ok(Some(updated)) => {
                *held_mut(&mut host.hub.guests[gi],session.source).expect("validated owner")=updated;
            }
            Ok(None) => {}
        }
        self.apply_guest_menu_effects(host,gi,effects);
        if input==horizon_client::menu::Input::Swap {host.hub.send_inventory(gi);}
        let current=held(&host.hub.guests[gi],session.source).cloned();
        if current.as_ref()!=Some(&tool) {session.menu=pocket_menu(session.source);}
        host.play.entry(conn).or_default().ae2_network_tool=Some(session);
        if current.as_ref().is_none_or(|s|Inventory::read(s).is_err()) {
            self.lan_ae2_network_tool_closed(host,conn,None);
            host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose{container:p.container}));
            return true;
        }
        self.lan_send_ae2_network_tool(host,conn);true
    }
    pub(super) fn lan_ae2_network_tool_closed(&mut self,host:&mut LanHost,conn:u32,window:Option<i32>)->bool {
        let Some(_)=host.play.get_mut(&conn).and_then(|p|p.ae2_network_tool.take_if(|v|window.is_none_or(|w|v.window==w))) else {return false;};
        if let Some(gi)=host.hub.guests.iter().position(|g|g.conn==conn){self.guest_carried_back(host,gi);}
        true
    }
    pub(super) fn lan_refresh_ae2_network_tools(&mut self,host:&mut LanHost) {
        let conns:Vec<_>=host.play.iter().filter_map(|(&conn,p)|p.ae2_network_tool.as_ref().map(|s|(conn,s.window,s.source,s.snapshot.clone(),s.status))).collect();
        for (conn,window,source,snapshot,status) in conns {
            let Some(g)=host.hub.guests.iter().find(|g|g.conn==conn && host.hub.here(g)) else {continue;};
            let current=held(g,source).cloned();
            if current.as_ref().is_none_or(|s|Inventory::read(s).is_err()) {
                self.lan_ae2_network_tool_closed(host,conn,None);
                host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose{container:window}));
                continue;
            }
            if let Some(pos)=status {
                let network=self.ae2_network();
                if !self.ae2_local_address(pos).is_some_and(|p|network.nodes.contains_key(&p.network_pos())) {
                    self.lan_ae2_network_tool_closed(host,conn,None);
                    host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose{container:window}));
                    continue;
                }
            }
            if current.as_ref()!=Some(&snapshot) {
                if let Some(session)=host.play.get_mut(&conn).and_then(|p|p.ae2_network_tool.as_mut()) {
                    session.menu=pocket_menu(source);
                }
                self.lan_send_ae2_network_tool(host,conn);
            } else if status.is_some() {
                if let Some(session)=host.play.get_mut(&conn).and_then(|p|p.ae2_network_tool.as_mut()) {
                    session.refresh=session.refresh.saturating_add(1);
                    if session.refresh>=15 {session.refresh=0;self.lan_send_ae2_network_tool(host,conn);}
                }
            }
        }
    }
}
