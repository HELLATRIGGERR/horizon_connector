//! Server-authoritative, face-owned I/O bus settings and physical upgrades.
use super::{Game, lan::LanHost};
use ae2_machines_core::{Key, Redstone, Schedule};
use horizon_client::{
    containers::BlockEntity,
    items::ItemStack,
    menu::{Input, Kind, Menu},
};
use horizon_protocol::packets::play::{self, Clientbound as C};
use horizon_world::nbt::{Compound, Tag};

type Pos = (i32, i32, i32);
struct BusDrag {
    mode: i32,
    slots: Vec<usize>,
    carried: ItemStack,
}
pub(super) struct GuestBus {
    window: i32,
    state: i32,
    pos: Pos,
    face: u8,
    page: usize,
    dim: String,
    signature: Tag,
    menu: Menu,
    drag: Option<BusDrag>,
    fake_revisions: std::collections::VecDeque<(i32,usize,usize)>,
    owner_identity: (u64,[i32;4]),
    emitter_identity: Option<(u64,[i32;4])>,
    emitter_edit: Option<String>,
    energy_edit: Option<EnergyEdit>,
}
fn bus_fake_revision_matches(history:&std::collections::VecDeque<(i32,usize,usize)>,state:i32,page:usize,slots:usize)->bool {
    history.iter().rev().find(|(revision,_,_)|*revision==state).is_some_and(|(_,shown_page,shown_slots)|*shown_page==page && *shown_slots==slots)
}
fn face_name(face: u8) -> Option<&'static str> {
    ["east", "west", "up", "down", "south", "north"]
        .get(face as usize)
        .copied()
}
fn field(data: &Compound, name: &str) -> Option<String> {
    match data.get(&name.into()) {
        Some(Tag::String(s)) => s.to_string().ok(),
        _ => None,
    }
}
fn icon(text: String) -> Option<ItemStack> {
    let mut s = ItemStack::new("minecraft:paper", 1);
    s.data_mut().custom_name = Some(horizon_client::item_data::text_name(text));
    Some(s)
}
fn stable(data: &Compound) -> Option<Tag> {
    let mut copy = data.clone();
    match field(data,"id").as_deref() {
        Some("ae2:level_emitter") => {super::ae2_emitter::emitter_read(data).ok()?;},
        Some("ae2:energy_level_emitter") => {super::ae2_emitter::energy_emitter_read(data).ok()?;},
        _ => {}
    }
    if matches!(field(data,"id").as_deref(),Some("ae2:level_emitter"|"ae2:energy_level_emitter")) {
        copy.remove(&"emitter_count".into());copy.remove(&"emitter_active".into());
        return Some(Tag::Compound(copy));
    }
    copy.remove(&"bus_next_tick".into());
    copy.remove(&"bus_tick_rate".into());
    let mut state = super::ae2_buses::bus_state(data).ok()?;
    state.last_redstone = false;
    state.pending_pulse = false;
    state.next_slot = 0;
    copy.insert(
        "state_json".into(),
        Tag::String(serde_json::to_string(&state).ok()?.as_str().into()),
    );
    Some(Tag::Compound(copy))
}
impl Game<'_> {
    fn bus_ui_read(&self, pos: Pos, face: u8) -> Option<Compound> {
        let name = face_name(face)?;
        let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get(&pos) else {
            return None;
        };
        let Some(Tag::List(parts)) = m.config.get(&"parts".into()) else {
            return None;
        };
        let mut matches = parts
            .iter()
            .filter(|p| p.get("face") == Some(&Tag::String(name.into())));
        let Tag::Compound(data) = matches.next()? else {
            return None;
        };
        if matches.next().is_some()
            || !matches!(
                field(data, "id")?.as_str(),
                "ae2:import_bus" | "ae2:export_bus" | "ae2:storage_bus" | "ae2:formation_plane" | "ae2:level_emitter" | "ae2:energy_level_emitter"
            )
        {
            return None;
        }
        match field(data,"id").as_deref() {
            Some("ae2:level_emitter") => {super::ae2_emitter::emitter_read(data).ok()?;},
            Some("ae2:energy_level_emitter") => {super::ae2_emitter::energy_emitter_read(data).ok()?;},
            _ => {super::ae2_buses::bus_state(data).ok()?;}
        }
        Some(data.clone())
    }
    fn bus_ui_store(&mut self, pos: Pos, face: u8, data: Compound) -> bool {
        self.bus_ui_store_with_dirty(pos,face,data,true)
    }
    fn bus_ui_store_with_dirty(&mut self, pos: Pos, face: u8, data: Compound, dirty:bool) -> bool {
        let Some(name) = face_name(face) else {
            return false;
        };
        let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&pos) else {
            return false;
        };
        let Some(Tag::List(parts)) = m.config.get_mut(&"parts".into()) else {
            return false;
        };
        let ix: Vec<_> = parts
            .iter()
            .enumerate()
            .filter(|(_, p)| p.get("face") == Some(&Tag::String(name.into())))
            .map(|(i, _)| i)
            .collect();
        if ix.len() != 1 || parts[ix[0]].get("id") != data.get(&"id".into()) {
            return false;
        }
        parts[ix[0]] = Tag::Compound(data);
        if dirty {self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));}
        true
    }
    pub(super) fn ae2_local_energy_focused(&self)->bool {
        self.ae2_local_bus_open() && self.ae2_local_energy.as_ref().is_some_and(|field|field.focused)
    }
    fn ae2_apply_local_energy(&mut self) {
        if !self.ae2_local_bus_valid(){return;}
        let Some(text)=self.ae2_local_energy.as_ref().map(|field|field.value.clone()) else{return};
        let Some((pos,face))=self.ae2_local_bus.as_ref().map(|s|(s.pos,s.face)) else{return};
        let Some(mut data)=self.bus_ui_read(pos,face) else{return};
        let Ok(mut state)=super::ae2_emitter::energy_emitter_read(&data) else{return};
        if let Some(value)=super::ae2_number_entry::number_value(&text,1,0,i64::MAX,'.') {
            if value!=state.threshold {
                state.threshold=value;
                if super::ae2_emitter::energy_emitter_store(&mut data,&state).is_ok() && self.bus_ui_store_with_dirty(pos,face,data,false) {
                    if let Some(host)=self.ae2_local_address(pos) {self.ae2_energy_emitter_threshold_callback(super::ae2_parts::PartAddress{host,face});}
                }
            }
        }
        if let Some(session)=self.ae2_local_bus.as_mut() {
            if let Some(edit)=session.energy_edit.as_mut(){edit.text=text.clone();}
            if session.emitter_edit.is_some(){session.emitter_edit=Some(text);}
        }
    }
    pub(super) fn ae2_local_energy_input(&mut self,text:Option<&str>,key:Option<winit::keyboard::KeyCode>) {
        if !self.ae2_local_bus_valid(){return;}
        let ctrl=self.has_control_down();let shift=self.has_shift_down();
        if let Some(field)=self.ae2_local_energy.as_mut(){if !key.is_some_and(|key|field.key(key,ctrl,shift)){if let Some(text)=text{field.insert_text(text);}}}
        self.ae2_apply_local_energy();
    }
    pub(super) fn ae2_local_energy_shortcut(&mut self,shortcut:horizon_client::clipboard::EditShortcut) {
        if let Some(field)=self.ae2_local_energy.as_mut(){field.shortcut(shortcut,&mut self.clipboard);}
        self.ae2_apply_local_energy();
    }
    pub(super) fn ae2_local_energy_click_field(&mut self,ox:f32,oy:f32,right:bool)->bool {
        let (mx,my)=self.cursor;let hit=!right && mx>=ox+48.0 && mx<ox+114.0 && my>=oy+55.0 && my<oy+67.0;let shift=self.has_shift_down();
        let Some(field)=self.ae2_local_energy.as_mut() else{return false};field.focused=hit;
        if hit {field.x=ox+48.0;field.y=oy+55.0;field.width=66.0;field.height=12.0;field.click(&self.gui.atlas,mx,shift,false);}hit
    }
    pub(super) fn ae2_local_energy_draw(&self,batch:&mut horizon_client::hud::HudBatch,ox:f32,oy:f32) {
        if let Some(mut field)=self.ae2_local_energy.clone(){field.x=ox+48.0;field.y=oy+55.0;field.width=66.0;field.height=12.0;field.draw(batch,&self.gui.atlas,None);}
    }
    pub(super) fn ae2_local_bus_title(&self)->&'static str {
        match self.ae2_local_bus.as_ref().and_then(|s|self.bus_ui_read(s.pos,s.face)).and_then(|data|field(&data,"id")).as_deref() {
            Some("ae2:level_emitter")=>"AE2 Level Emitter",Some("ae2:energy_level_emitter")=>"AE2 Energy Level Emitter",_=>"AE2 Bus"
        }
    }
    /// Replay emitter physical slots and player gestures through the same
    /// persistent native Menu used by the server, committing effects only
    /// after the face-owned upgrade inventory is accepted.
    fn ae2_local_emitter_physical(&mut self,session:&mut GuestBus,mut data:Compound,slot:Option<usize>,button:i32,input:Input)->bool {
        let storage=field(&data,"id").as_deref()==Some("ae2:level_emitter");
        let mut state=if storage {super::ae2_emitter::emitter_read(&data).ok()}else{None};
        if storage && state.is_none(){return false;}
        let before_random=self.world.random.clone();
        let result={
            let env=self.menu_env();let menu=&mut session.menu;
            for (i,item) in self.inventory.iter().enumerate(){menu.set_player_item(i,item.clone());}
            for (i,slot) in [39,38,37,36].into_iter().enumerate(){menu.set_player_item(slot,self.armor[i].clone());}
            menu.set_player_item(40,self.offhand.clone());menu.selected=self.selected;menu.creative=self.creative;
            menu.carried=self.carried.clone();
            if let Some(state)=state.as_ref(){menu.set_slot(&env,36,state.upgrades[0].clone());}
            menu.recipe_durability_rng=Some(before_random.clone());
            let clicked=menu.click(&env,slot.map_or(-999,|slot|slot as i32),button,input);
            let random=menu.recipe_durability_rng.take();
            let dropped=std::mem::take(&mut menu.dropped);let equipped=std::mem::take(&mut menu.equipped);
            menu.drops.clear();menu.crafted.clear();
            clicked.map(|()|{
                let inventory=(0..36).map(|i|menu.player_item(i).cloned()).collect::<Vec<_>>();
                let armor=std::array::from_fn(|i|menu.player_item([39,38,37,36][i]).cloned());
                (inventory,armor,menu.player_item(40).cloned(),menu.carried.take(),menu.slot(36).cloned(),dropped,equipped,random)
            })
        };
        let Ok((inventory,armor,offhand,carried,upgrade,dropped,equipped,random))=result else {session.menu=emitter_session_menu(&data);return false};
        let mut upgrade_changed=false;
        if let Some(state)=state.as_mut() {
            if state.upgrades[0]!=upgrade {
                state.upgrades[0]=upgrade;upgrade_changed=true;
                if super::ae2_emitter::emitter_store(&mut data,state).is_err() || !self.bus_ui_store(session.pos,session.face,data.clone()) {
                    session.menu=emitter_session_menu(&data);return false;
                }
            }
            if emitter_card(state,"ae2:crafting_card") {session.emitter_edit=None;}
        }
        self.inventory=inventory;self.armor=armor;self.offhand=offhand;self.carried=carried;
        if let Some(random)=random {self.world.random=random;}
        for stack in dropped {self.player_drop(stack);}
        for (slot,stack) in equipped {self.player_equipped(slot,stack.as_ref());}
        if upgrade_changed {self.emitter_ui_provider_changed(session.pos,session.face);}
        true
    }
    /// Open the actual clicked bus part and bind its owner identity for the
    /// lifetime of this in-frame screen.
    pub(super) fn ae2_open_local_bus(&mut self,pos:Pos,face:[i32;3])->bool {
        if self.remote.is_some(){return false;}
        let face=match face {[1,0,0]=>0,[-1,0,0]=>1,[0,1,0]=>2,[0,-1,0]=>3,[0,0,1]=>4,[0,0,-1]=>5,_=>return false};
        let Some(data)=self.bus_ui_read(pos,face) else{return false};
        let kind=field(&data,"id").unwrap_or_default();let energy=kind=="ae2:energy_level_emitter";
        let emitter=energy || kind=="ae2:level_emitter";
        let Some(signature)=stable(&data) else{return false};let Some(owner_identity)=self.bus_ui_identity(pos,&data) else{return false};
        let energy_edit=if energy {let Ok(state)=super::ae2_emitter::energy_emitter_read(&data) else{return false};
            let text=energy_entry_text(state.threshold);let mut field=horizon_client::ui::EditBox::new(0.0,0.0,66.0,12.0);
            field.max_length=16;field.set_value(&text);field.highlight=0;field.focused=true;self.ae2_local_energy=Some(field);
            Some(EnergyEdit{token:0,text})
        }else{self.ae2_local_energy=None;None};
        self.container=Some(pos);self.ae2_local_bus=Some(GuestBus{
            window:0,state:0,pos,face,page:0,dim:self.dim.id().into(),signature,
            menu:if emitter {emitter_session_menu(&data)}else{Menu::new(Kind::Chest(6))},drag:None,fake_revisions:Default::default(),
            owner_identity,emitter_identity:emitter.then_some(owner_identity),emitter_edit:None,energy_edit,
        });
        self.screen=super::Screen::Inventory;self.capture(false);self.right=false;true
    }
    pub(super) fn ae2_local_bus_open(&self)->bool {
        self.remote.is_none() && self.ae2_local_bus.as_ref().is_some_and(|s|self.container==Some(s.pos))
    }
    pub(super) fn ae2_local_bus_valid(&self)->bool {
        self.ae2_local_bus_open() && self.ae2_local_bus.as_ref().is_some_and(|s|
            s.dim==self.dim.id() && self.bus_ui_read(s.pos,s.face).is_some_and(|data|self.bus_ui_identity(s.pos,&data)==Some(s.owner_identity))
                && self.eye_position().is_some_and(|eye|horizon_client::lan::guest::within_block_range(eye,self.reach()+4.0,[s.pos.0,s.pos.1,s.pos.2])))
    }
    pub(super) fn ae2_local_bus_slot_view(&self,slot:usize)->Option<ItemStack> {
        if !self.ae2_local_bus_valid(){return None;}let session=self.ae2_local_bus.as_ref()?;let data=self.bus_ui_read(session.pos,session.face)?;
        let items=match field(&data,"id").as_deref() {
            Some("ae2:level_emitter")=>self.ae2_emitter_menu_items(&data,session.emitter_edit.as_deref()),
            Some("ae2:energy_level_emitter")=>self.ae2_energy_emitter_menu_items(&data,session.emitter_edit.as_deref()),
            _=>self.ae2_bus_menu_items(&data,session.page),
        }?;
        items.get(slot.checked_sub(300)?).cloned().flatten()
    }
    pub(super) fn ae2_local_bus_fake_slot(&self,slot:usize)->bool {
        if !self.ae2_local_bus_valid() || !(300..336).contains(&slot){return false;}
        self.ae2_local_bus.as_ref().and_then(|s|self.bus_ui_read(s.pos,s.face).and_then(|data|match field(&data,"id").as_deref() {
            Some("ae2:level_emitter")=>Some(slot==300 && s.emitter_edit.is_none()),
            Some("ae2:energy_level_emitter")=>Some(false),
            _=>super::ae2_buses::bus_state(&data).ok().map(|state|s.page*36+slot-300<state.slots()),
        })).unwrap_or(false)
    }
    pub(super) fn ae2_local_bus_drag(&mut self,slot:usize,single:bool) {
        if !self.ae2_local_bus_fake_slot(slot) || self.carried.is_none(){return;}
        let actual=self.carried.clone();if single {if let Some(stack)=self.carried.as_mut(){stack.count=1;}}
        self.ae2_local_bus_click(Some(slot),0,Input::Pickup);self.carried=actual;
    }
    pub(super) fn ae2_local_bus_click(&mut self,slot:Option<usize>,button:i32,input:Input) {
        if !self.ae2_local_bus_valid(){self.close_container();self.screen=super::Screen::Playing;self.capture(true);return;}
        if input==Input::Clone && slot.is_some_and(|slot|self.slot_view(slot).is_some_and(|item|item.item.as_ref()=="ae2:wrapped_generic_stack")){return;}
        let (button,input)=if slot.is_some_and(|slot|self.ae2_local_bus_fake_slot(slot)) {
            if matches!(input,Input::Swap|Input::Throw) && self.carried.is_some(){return;}
            if input==Input::Swap && button!=40{return;}
            if input==Input::QuickMove {(button,Input::Pickup)}else if input==Input::Pickup {(button,input)}else{(i32::from(button==1),Input::Pickup)}
        }else{(button,input)};
        let Some(mut session)=self.ae2_local_bus.take() else{return};let Some(data)=self.bus_ui_read(session.pos,session.face) else{return};
        let wire=match slot {Some(slot @ 300..=353)=>Some(slot-300),Some(slot @ 0..=8)=>Some(81+slot),Some(slot @ 9..=35)=>Some(54+slot-9),_=>None};
        match field(&data,"id").as_deref() {
            Some("ae2:level_emitter")=>if !self.ae2_emitter_config_action(&mut session,data.clone(),wire,button,input) {
                self.ae2_local_emitter_physical(&mut session,data,wire,button,input);
            },
            Some("ae2:energy_level_emitter")=>{
                let (handled,echo)=self.ae2_energy_emitter_config_action(&mut session,data.clone(),wire,button,input);
                if !handled {self.ae2_local_emitter_physical(&mut session,data,wire,button,input);}
                if let Some((_,text))=echo {if let Some(field)=self.ae2_local_energy.as_mut(){field.set_value(&text);field.focused=true;}}
            },
            _=>{self.ae2_bus_action(&mut session,data,wire.unwrap_or(usize::MAX),button,input);}
        }
        self.ae2_local_bus=Some(session);
    }
    pub(super) fn lan_open_ae2_bus(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        pos: Pos,
        face: u8,
    ) -> bool {
        let Some(data) = self.bus_ui_read(pos, face) else {
            return false;
        };
        let Some(signature) = stable(&data) else {
            return false;
        };
        let energy=field(&data,"id").as_deref()==Some("ae2:energy_level_emitter");
        let emitter=energy || field(&data,"id").as_deref()==Some("ae2:level_emitter");
        let Some(owner_identity)=self.bus_ui_identity(pos,&data) else{return false};
        let emitter_identity=if emitter {Some(owner_identity)}else{None};
        let energy_edit=if energy {
            let Ok(state)=super::ae2_emitter::energy_emitter_read(&data) else{return false};
            static NEXT_ENERGY_EDITOR:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1);
            Some(EnergyEdit{token:NEXT_ENERGY_EDITOR.fetch_add(1,std::sync::atomic::Ordering::Relaxed),text:energy_entry_text(state.threshold)})
        }else{None};
        self.lan_ae2_bus_closed(host, conn, None);
        self.lan_ae2_network_tool_closed(host, conn, None);
        self.lan_ae2_anchor_closed(host, conn, None);
        self.lan_craft_confirm_closed(host, conn, None);
        self.lan_pattern_access_closed(host, conn, None);
        self.lan_ae2_portable_closed(host, conn, None);
        self.lan_ae2_terminal_closed(host, conn, None);
        self.lan_ae2_machine_closed(host, conn, None);
        self.lan_station_closed(host, conn, None);
        self.lan_crafting_closed(host, conn, None);
        self.guest_container_closed(host, conn);
        let Some(window) = host.hub.open_window(
            conn,
            "minecraft:generic_9x6",
            horizon_protocol::text::literal(if energy {"AE2 Energy Level Emitter"}else if emitter {"AE2 Level Emitter"}else{"AE2 Bus"}),
        ) else {
            return true;
        };
        host.hub.send_to(conn,&C::CustomPayload(horizon_protocol::packets::configuration::CustomPayload {
            channel:"ae2:bus_ack_enable".into(), data:window.to_be_bytes().to_vec(),
        }));
        host.play.entry(conn).or_default().ae2_bus = Some(GuestBus {
            window,
            state: 0,
            pos,
            face,
            page: 0,
            dim: self.dim.id().into(),
            signature,
            menu: if emitter {emitter_session_menu(&data)}else{Menu::new(Kind::Chest(6))},
            drag: None,
            fake_revisions: std::collections::VecDeque::new(),
            owner_identity,
            emitter_identity,
            emitter_edit:None,
            energy_edit:energy_edit.clone(),
        });
        self.lan_send_ae2_bus(host, conn);
        if let Some(edit)=energy_edit {Self::energy_payload(host,conn,window,edit.token,"open",&edit.text);}
        true
    }
    fn ae2_bus_menu_items(&self,data:&Compound,page:usize)->Option<Vec<Option<ItemStack>>> {
        let state=super::ae2_buses::bus_state(data).ok()?;
        let mut items = vec![None; 54];
        for i in 0..36 {
            let slot = page * 36 + i;
            if slot >= state.slots() {
                continue;
            }
            if let Some(key) = state.config.get(slot).and_then(Option::as_ref) {
                let key = ae2_cell_core::ItemKey::from(key);
                items[i] = super::ae2_lab::prototype(&key).ok().or_else(|| {
                    super::ae2_fluids::fluid_bucket(&key.item_id).map(|id| ItemStack::new(id, 1))
                });
            }
        }
        if let Ok(cards) = super::ae2_buses::bus_upgrade_slots(&data) {
            for (i, card) in cards.into_iter().enumerate() {
                items[36 + i] = card;
            }
        }
        let kind = field(&data, "id").unwrap_or_default();
        items[41] = icon(format!("Filter page {}/2", page + 1));
        if matches!(kind.as_str(), "ae2:import_bus" | "ae2:export_bus") {
            items[42] = icon(format!("Redstone: {:?}", state.redstone));
        }
        if kind == "ae2:export_bus" {
            items[43] = icon(format!("Schedule: {:?}", state.schedule));
        }
        items[44] = icon(format!(
            "Fuzzy mode: {}",
            field(&data, "fuzzy_mode").unwrap_or_else(|| "ignore_all".into())
        ));
        let priority = match data.get(&"priority".into()) {
            Some(Tag::Int(n)) => *n,
            _ => 0,
        };
        if matches!(kind.as_str(), "ae2:storage_bus" | "ae2:formation_plane") {
            for (i, n) in [1, 10, 100, 1000].into_iter().enumerate() {
                items[45 + i] = icon(format!("Priority: {priority}; left +{n}, right -{n}"));
            }
        }
        if kind == "ae2:storage_bus" {
            items[49] = icon(format!(
                "Access: {}",
                field(&data, "access").unwrap_or_else(|| "read_write".into())
            ));
            items[50] = icon(format!(
                "Filter on extraction: {}",
                !matches!(data.get(&"filter_on_extract".into()), Some(Tag::Byte(0)))
            ));
            items[52] = icon(format!(
                "Storage filter: {}",
                field(&data, "storage_filter").unwrap_or_else(|| "extractable_only".into())
            ));
        }
        if kind == "ae2:formation_plane" {
            let entity = matches!(data.get(&"formation_entity".into()), Some(Tag::Byte(1)));
            items[53] = icon(format!("Placement: {}", if entity { "item entity" } else { "block" }));
        }
        if kind == "ae2:export_bus"
            && matches!(data.get(&"upgrades".into()), Some(Tag::List(cards)) if cards.iter().any(|card| card.get("id") == Some(&Tag::String("ae2:crafting_card".into()))))
        {
            items[51] = icon(format!(
                "Craft only: {}",
                matches!(data.get(&"craft_only".into()), Some(Tag::Byte(1)))
            ));
        }
        Some(items)
    }
    fn lan_send_ae2_bus(&mut self, host: &mut LanHost, conn: u32) {
        let Some((pos, face, window, page)) = host
            .play
            .get(&conn)
            .and_then(|p| p.ae2_bus.as_ref())
            .map(|s| (s.pos, s.face, s.window, s.page))
        else {
            return;
        };
        let Some(data) = self.bus_ui_read(pos, face) else {
            self.lan_ae2_bus_closed(host, conn, None);
            host.hub.send_to(
                conn,
                &C::ContainerClose(play::ContainerClose { container: window }),
            );
            return;
        };
        if field(&data,"id").as_deref()==Some("ae2:energy_level_emitter") {self.lan_send_ae2_energy_emitter(host,conn);return;}
        if field(&data,"id").as_deref()==Some("ae2:level_emitter") {self.lan_send_ae2_emitter(host,conn);return;}
        let Ok(state) = super::ae2_buses::bus_state(&data) else {
            return;
        };
        let Some(g) = host.hub.guests.iter().find(|g| g.conn == conn) else {
            return;
        };
        let Some(mut items)=self.ae2_bus_menu_items(&data,page) else{return;};
        items.extend((9..36).chain(0..9).map(|i| g.inventory[i].clone()));
        let carried = g.carried.clone();
        let fake:Vec<u8>=(0..state.slots().saturating_sub(page*36).min(36)).map(|slot|slot as u8).collect();
        Self::lan_ae2_fake_slots(host,conn,window,&fake);
        let Some(s) = host.play.get_mut(&conn).and_then(|p| p.ae2_bus.as_mut()) else {
            return;
        };
        s.signature = stable(&data).unwrap();
        s.state = (s.state + 1) & 0x7fff;
        let next = s.state;
        s.fake_revisions.push_back((next,page,state.slots()));
        while s.fake_revisions.len()>64 {s.fake_revisions.pop_front();}
        host.hub.inventory_told_by(conn,&items);
        let items = items.iter().map(|v| host.hub.stack(v)).collect();
        let carried = host.hub.stack(&carried);
        host.hub.send_to(
            conn,
            &C::ContainerSetContent(play::ContainerSetContent {
                container: window,
                state: next,
                items,
                carried,
            }),
        );
    }
    /// Shared source bus ghost/config action. The actor's carried stack is
    /// authoritative in both the server and in-frame paths.
    fn ae2_bus_config_action(&mut self,session:&mut GuestBus,mut data:Compound,slot:usize,button:i32,input:Input)->bool {
        if !(slot<36 || (41..54).contains(&slot)) {return false;}
        if input!=Input::Pickup || (!(0..=1).contains(&button) && !(matches!(button,2|3) && slot<36 && self.carried.is_some())) {return true;}
        let kind=field(&data,"id").unwrap_or_default();
        let before=self.carried.clone();
        let Ok(mut state)=super::ae2_buses::bus_state(&data) else{return true;};
        let mut changed=false;
        if slot < 36 {
            let index = session.page * 36 + slot;
            if index < state.slots() {
                let held = self.carried.as_ref();
                let wrapped=held.map(|item|super::ae2_encoding::wrapped_ghost(item,&|id,fluid|fluid||self.res.items().get(id).is_some())).transpose();
                let selected=match wrapped {
                    Ok(Some(Some(super::ae2_encoding::WrappedGhost::Fluid(fluid))))=>Some(Key::from(&fluid.key)),
                    Ok(Some(Some(super::ae2_encoding::WrappedGhost::Item(item))))=>super::ae2_lab::key(&item).ok().as_ref().map(Key::from),
                    Err(_)=>return true,
                    _ if button==1=>held.and_then(|s|super::ae2_fluids::bucket_fluid(&s.item)).map(super::ae2_fluids::fluid_key).as_ref().map(Key::from),
                    _=>held.and_then(|s|super::ae2_lab::key(s).ok()).as_ref().map(Key::from),
                };
                state.config[index]=selected;
                changed = true;
            }
        } else if slot == 41 {
            session.page = (session.page + 1) % 2;
        } else if slot == 42 && matches!(kind.as_str(), "ae2:import_bus" | "ae2:export_bus") {
            let mode = match (state.redstone, button) {
                (Redstone::Ignore, 0) => Redstone::High,
                (Redstone::High, 0) => Redstone::Low,
                (Redstone::Low, 0) => Redstone::Pulse,
                (_, 0) => Redstone::Ignore,
                (Redstone::Ignore, _) => Redstone::Pulse,
                (Redstone::Pulse, _) => Redstone::Low,
                (Redstone::Low, _) => Redstone::High,
                _ => Redstone::Ignore,
            };
            let at =
                horizon_core::position::BlockPos::new(session.pos.0, session.pos.1, session.pos.2);
            let signal = self.with_level(|b, level, ticks| b.has_neighbor_signal(level, ticks, at));
            state.set_redstone_mode(mode, signal);
            changed = true;
        } else if slot == 43 && kind == "ae2:export_bus" {
            state.schedule = match (state.schedule, button) {
                (Schedule::Default, 0) => Schedule::RoundRobin,
                (Schedule::RoundRobin, 0) => Schedule::Random,
                (Schedule::Random, 0) => Schedule::Default,
                (Schedule::Default, _) => Schedule::Random,
                (Schedule::Random, _) => Schedule::RoundRobin,
                (Schedule::RoundRobin, _) => Schedule::Default,
            };
            changed = true;
        } else if slot == 44 {
            let choices = [
                "ignore_all",
                "percent_99",
                "percent_75",
                "percent_50",
                "percent_25",
            ];
            let current = field(&data, "fuzzy_mode").unwrap_or_default();
            let i = choices.iter().position(|v| *v == current).unwrap_or(0);
            let n = if button == 0 {
                (i + 1) % 5
            } else {
                (i + 4) % 5
            };
            data.insert("fuzzy_mode".into(), Tag::String(choices[n].into()));
            changed = true;
        } else if (45..49).contains(&slot)
            && matches!(kind.as_str(), "ae2:storage_bus" | "ae2:formation_plane")
        {
            let old = match data.get(&"priority".into()) {
                Some(Tag::Int(n)) => *n,
                _ => 0,
            };
            let n = [1i32, 10, 100, 1000][slot - 45];
            data.insert(
                "priority".into(),
                Tag::Int(if button == 0 {
                    old.saturating_add(n)
                } else {
                    old.saturating_sub(n)
                }),
            );
            changed = true;
        } else if slot == 49 && kind == "ae2:storage_bus" {
            let old = field(&data, "access").unwrap_or_else(|| "read_write".into());
            let next = match old.as_str() {
                "read_write" => "read_only",
                "read_only" => "write_only",
                _ => "read_write",
            };
            data.insert("access".into(), Tag::String(next.into()));
            changed = true;
        } else if slot == 50 && kind == "ae2:storage_bus" {
            let enabled = !matches!(data.get(&"filter_on_extract".into()), Some(Tag::Byte(0)));
            data.insert("filter_on_extract".into(), Tag::Byte(i8::from(!enabled)));
            changed = true;
        } else if slot == 52 && kind == "ae2:storage_bus" {
            let next = match field(&data, "storage_filter").as_deref() {
                None | Some("extractable_only") => Some("none"),
                Some("none") => Some("extractable_only"),
                _ => None,
            };
            if let Some(next) = next {
                data.insert("storage_filter".into(), Tag::String(next.into()));
                changed = true;
            }
        } else if slot == 53 && kind == "ae2:formation_plane" {
            let entity = matches!(data.get(&"formation_entity".into()), Some(Tag::Byte(1)));
            data.insert("formation_entity".into(), Tag::Byte(i8::from(!entity)));
            changed = true;
        } else if slot == 51
            && kind == "ae2:export_bus"
            && matches!(data.get(&"upgrades".into()), Some(Tag::List(cards)) if cards.iter().any(|card| card.get("id") == Some(&Tag::String("ae2:crafting_card".into()))))
        {
            let old = matches!(data.get(&"craft_only".into()), Some(Tag::Byte(1)));
            data.insert("craft_only".into(), Tag::Byte(i8::from(!old)));
            changed = true;

        }
        if changed {
            if let Ok(encoded) = serde_json::to_string(&state) {
                data.insert("state_json".into(), Tag::String(encoded.as_str().into()));
                data.insert("bus_next_tick".into(), Tag::Long(0));
                if !self.bus_ui_store(session.pos, session.face, data) {
                    self.carried = before;
                }
            } else {
                self.carried = before;
            }
        }
        true
    }
    /// Apply bus card and configuration operations to the acting player's
    /// real inventory and carried stack. Server and local sessions share it.
    fn ae2_bus_action(&mut self,session:&mut GuestBus,mut data:Compound,slot:usize,button:i32,input:Input)->bool {
        if input == Input::QuickCraft {
            let stage = button & 3;
            let mode = (button >> 2) & 3;
            match stage {
                0 => {
                    session.drag = self.carried.clone().and_then(|carried| {
                        (mode == 0 || mode == 1 || (mode == 2 && self.creative))
                            .then_some(BusDrag {
                                mode,
                                slots: Vec::new(),
                                carried,
                            })
                    });
                }
                1 => {
                    if let Some(drag) = session.drag.as_mut() {
                        if drag.mode == mode
                            && (36..41).contains(&slot)
                            && !drag.slots.contains(&(slot - 36))
                            && self.carried.as_ref() == Some(&drag.carried)
                            && (mode == 2 || drag.slots.len() < drag.carried.count as usize)
                        {
                            drag.slots.push(slot - 36);
                        }
                    }
                }
                2 => {
                    if let Some(mut drag) = session.drag.take() {
                        if drag.mode == mode
                            && self.carried.as_ref() == Some(&drag.carried)
                        {
                            let before = self.carried.clone();
                            let mut cards =
                                super::ae2_buses::bus_upgrade_slots(&data).unwrap_or_default();
                            drag.slots.sort_unstable();
                            let mut remaining = drag.carried.count;
                            let mut changed = false;
                            for index in drag.slots {
                                if index >= cards.len()
                                    || cards[index].is_some()
                                    || (mode != 2 && remaining == 0)
                                {
                                    continue;
                                }
                                let mut one = drag.carried.clone();
                                one.count = 1;
                                let mut trial = cards.clone();
                                trial[index] = Some(one);
                                let mut proposed = data.clone();
                                if super::ae2_buses::store_bus_upgrade_slots(&mut proposed, &trial)
                                    .is_ok()
                                    && super::ae2_buses::bus_state(&proposed).is_ok()
                                {
                                    cards = trial;
                                    if mode != 2 {
                                        remaining -= 1;
                                    }
                                    changed = true;
                                }
                            }
                            if changed {
                                let _ =
                                    super::ae2_buses::store_bus_upgrade_slots(&mut data, &cards);
                                data.insert("bus_next_tick".into(), Tag::Long(0));
                                if mode != 2 {
                                    self.carried =
                                        (remaining > 0).then_some(ItemStack {
                                            count: remaining,
                                            ..drag.carried
                                        });
                                }
                                if !self.bus_ui_store(session.pos, session.face, data) {
                                    self.carried = before;
                                }
                            }
                        }
                    }
                }
                _ => session.drag = None,
            }
            return true;
        }
        session.drag = None;
        if (54..90).contains(&slot) && matches!(input,Input::Swap|Input::Throw|Input::Clone) {
            let index=if slot<81 {slot-54+9}else{slot-81};
            self.ae2_machine_player_menu_click(index,button,input);return true;
        }
        if input==Input::Throw && (36..41).contains(&slot) && (0..=1).contains(&button) && self.carried.is_none() {
            if self.eye_position().is_none(){return true;}
            let mut cards=super::ae2_buses::bus_upgrade_slots(&data).unwrap_or_default();
            if let Some(item)=cards.get_mut(slot-36).and_then(Option::take) {
                if super::ae2_buses::store_bus_upgrade_slots(&mut data,&cards).is_ok() {
                    if item.item.as_ref()=="ae2:crafting_card" {super::ae2_buses::mark_export_card_removed(&mut data);}
                    data.insert("bus_next_tick".into(),Tag::Long(0));
                    if self.bus_ui_store(session.pos,session.face,data) {self.player_drop(item);}
                }
            }
            return true;
        }
        if input == Input::QuickMove || input == Input::Swap {
            let before_inventory = self.inventory.clone();let before_offhand=self.offhand.clone();
            let mut cards = super::ae2_buses::bus_upgrade_slots(&data).unwrap_or_default();
            let had_crafting = cards
                .iter()
                .flatten()
                .any(|v| v.item.as_ref() == "ae2:crafting_card");
            let mut changed = false;
            if input == Input::QuickMove && (36..41).contains(&slot) {
                let index = slot - 36;
                if let Some(item) = cards.get(index).and_then(Clone::clone) {
                    if self.give(item).is_none()
                    {
                        cards[index] = None;
                        changed = true;
                    }
                }
            } else if input == Input::QuickMove && (54..90).contains(&slot) {
                let index = if slot < 81 { slot - 54 + 9 } else { slot - 81 };
                if let Some(free) = cards.iter().position(Option::is_none) {
                    if let Some(mut card) = self.inventory[index].clone() {
                        card.count = 1;
                        let mut trial = cards.clone();
                        trial[free] = Some(card);
                        let mut proposed = data.clone();
                        if super::ae2_buses::store_bus_upgrade_slots(&mut proposed, &trial).is_ok()
                            && super::ae2_buses::bus_state(&proposed).is_ok()
                        {
                            cards = trial;
                            if let Some(stack) = self.inventory[index].as_mut() {
                                stack.count -= 1;
                                if stack.count == 0 {
                                    self.inventory[index] = None;
                                }
                            }
                            changed = true;
                        }
                    }
                }
            } else if input == Input::Swap
                && (36..41).contains(&slot)
                && ((0..=8).contains(&button) || button==40)
            {
                let index = slot - 36;
                let incoming=if button==40 {self.offhand.clone()}else{self.inventory[button as usize].clone()};
                if index < cards.len() && incoming.as_ref().is_none_or(|v| v.count == 1) {
                    let existing = cards[index].clone();
                    let mut trial = cards.clone();
                    trial[index] = incoming;
                    let mut proposed = data.clone();
                    if trial != cards
                        && super::ae2_buses::store_bus_upgrade_slots(&mut proposed, &trial).is_ok()
                        && super::ae2_buses::bus_state(&proposed).is_ok()
                    {
                        cards = trial;
                        if button==40 {self.offhand=existing;}else{self.inventory[button as usize]=existing;}
                        changed = true;
                    }
                }
            }
            if changed {
                let has_crafting = cards
                    .iter()
                    .flatten()
                    .any(|v| v.item.as_ref() == "ae2:crafting_card");
                let _ = super::ae2_buses::store_bus_upgrade_slots(&mut data, &cards);
                if had_crafting && !has_crafting {
                    super::ae2_buses::mark_export_card_removed(&mut data);
                }
                data.insert("bus_next_tick".into(), Tag::Long(0));
                if !self.bus_ui_store(session.pos, session.face, data) {
                    self.inventory = before_inventory;self.offhand=before_offhand;
                }
            }
            return true;
        }
        if input!=Input::Pickup || (!(0..=1).contains(&button)
            && !(matches!(button,2|3) && slot<36 && self.carried.is_some())) {return true;}
        let before=self.carried.clone();
        let Ok(state)=super::ae2_buses::bus_state(&data) else{return true;};
        let mut changed=false;
        if self.ae2_bus_config_action(session,data.clone(),slot,button,input) {return true;}
        if (36..41).contains(&slot) {
            let index = slot - 36;
            let mut cards = super::ae2_buses::bus_upgrade_slots(&data).unwrap_or_default();
            let held = self.carried.clone();
            if index < cards.len() {
                if held.is_none() {
                    if let Some(item) = cards[index].take() {
                        let removed_crafting = item.item.as_ref() == "ae2:crafting_card";
                        self.carried = Some(item);
                        let _ = super::ae2_buses::store_bus_upgrade_slots(&mut data, &cards);
                        if removed_crafting {
                            super::ae2_buses::mark_export_card_removed(&mut data);
                        }
                        changed = true;
                    }
                } else if cards[index].is_none() {
                    let mut item = held.unwrap();
                    item.count = 1;
                    let mut trial = cards.clone();
                    trial[index] = Some(item);
                    let mut proposed = data.clone();
                    if super::ae2_buses::store_bus_upgrade_slots(&mut proposed, &trial).is_ok()
                        && super::ae2_buses::bus_state(&proposed).is_ok()
                    {
                        data = proposed;
                        let carried = self.carried.as_mut().unwrap();
                        carried.count -= 1;
                        if carried.count == 0 {
                            self.carried = None;
                        }
                        changed = true;
                    }
                }
            }
        } else if (54..90).contains(&slot) {
            let index=if slot<81 {slot-54+9}else{slot-81};
            self.ae2_machine_player_menu_click(index,button,input);
        }
        if changed {
            if let Ok(encoded) = serde_json::to_string(&state) {
                data.insert("state_json".into(), Tag::String(encoded.as_str().into()));
                data.insert("bus_next_tick".into(), Tag::Long(0));
                if !self.bus_ui_store(session.pos, session.face, data) {
                    self.carried = before;
                }
            } else {
                self.carried = before;
            }
        }
        true
    }
    pub(super) fn lan_ae2_bus_click(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        p: &play::ContainerClick,
    ) -> bool {
        let Some(mut session) = host.play.get_mut(&conn).and_then(|p| p.ae2_bus.take()) else {
            return false;
        };
        if p.container != session.window {
            host.play.entry(conn).or_default().ae2_bus = Some(session);
            return false;
        }
        let Some(gi) = host
            .hub
            .guests
            .iter()
            .position(|g| g.conn == conn && host.hub.here(g))
        else {
            return true;
        };
        let Some(mut data) = self.bus_ui_read(session.pos, session.face) else {
            host.play.entry(conn).or_default().ae2_bus = Some(session);
            self.lan_ae2_bus_closed(host, conn, None);
            host.hub.send_to(
                conn,
                &C::ContainerClose(play::ContainerClose {
                    container: p.container,
                }),
            );
            return true;
        };
        if session.dim != self.dim.id()
            || !host.hub.guests[gi]
                .within_block_range([session.pos.0, session.pos.1, session.pos.2], 4.0)
        {
            host.play.entry(conn).or_default().ae2_bus = Some(session);
            self.lan_ae2_bus_closed(host, conn, None);
            host.hub.send_to(
                conn,
                &C::ContainerClose(play::ContainerClose {
                    container: p.container,
                }),
            );
            return true;
        }
        if self.bus_ui_identity(session.pos,&data)!=Some(session.owner_identity) {
            host.play.entry(conn).or_default().ae2_bus=Some(session);
            self.lan_ae2_bus_closed(host,conn,None);host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose{container:p.container}));return true;
        }
        if stable(&data).as_ref() != Some(&session.signature) {
            if session.emitter_identity.is_some() {session.menu=emitter_session_menu(&data);session.emitter_edit=None;}
            if let Some((token,text))=reset_energy_edit(&mut session,&data) {Self::energy_payload(host,conn,session.window,token,"set",&text);}
            session.drag = None;
            host.play.entry(conn).or_default().ae2_bus = Some(session);
            self.lan_send_ae2_bus(host, conn);
            return true;
        }
        if session.emitter_identity.is_some() {
            if field(&data,"id").as_deref()==Some("ae2:energy_level_emitter") {return self.lan_ae2_energy_emitter_click(host,conn,gi,p,session,data);}
            return self.lan_ae2_emitter_click(host,conn,gi,p,session,data);
        }
        let slot = p.slot as usize;
        let raw_input=super::lan_crafting::input(p.input);
        let input=if slot<36 && raw_input==Input::QuickMove {Input::Pickup}else{raw_input};
        if slot<36 && input==Input::Pickup {
            let Ok(state)=super::ae2_buses::bus_state(&data) else {host.play.entry(conn).or_default().ae2_bus=Some(session);return true;};
            if !bus_fake_revision_matches(&session.fake_revisions,p.state,session.page,state.slots()) {
                host.play.entry(conn).or_default().ae2_bus=Some(session);self.lan_send_ae2_bus(host,conn);return true;
            }
        }
        let stage=i32::from(p.button)&3;
        self.as_guest(host,conn,|game|game.ae2_bus_action(&mut session,data,slot,i32::from(p.button),input));
        host.play.entry(conn).or_default().ae2_bus=Some(session);
        // Do not reset the client's pointer drag while it is being assembled.
        if input!=Input::QuickCraft || stage>=2 {self.lan_send_ae2_bus(host,conn);}
        true
    }
    pub(super) fn lan_ae2_bus_closed(
        &mut self,
        host: &mut LanHost,
        conn: u32,
        window: Option<i32>,
    ) -> bool {
        let Some(_) = host
            .play
            .get_mut(&conn)
            .and_then(|p| p.ae2_bus.take_if(|v| window.is_none_or(|w| v.window == w)))
        else {
            return false;
        };
        if let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) {
            self.guest_carried_back(host, gi);
        }
        true
    }
    pub(super) fn lan_refresh_ae2_buses(&mut self, host: &mut LanHost) {
        let conns: Vec<_> = host
            .play
            .iter()
            .filter_map(|(&c, p)| {
                p.ae2_bus
                    .as_ref()
                    .map(|s| (c, s.pos, s.face, s.window, s.dim.clone()))
            })
            .collect();
        for (conn, pos, face, window, dim) in conns {
            if !host
                .hub
                .guests
                .iter()
                .any(|g| g.conn == conn && host.hub.here(g))
            {
                continue;
            }
            let in_range = host
                .hub
                .guests
                .iter()
                .find(|g| g.conn == conn)
                .is_some_and(|g| {
                    !g.spectator() && g.within_block_range([pos.0, pos.1, pos.2], 4.0)
                });
            let data=self.bus_ui_read(pos,face);
            let session_owner=host.play.get(&conn).and_then(|p|p.ae2_bus.as_ref()).map(|s|s.owner_identity);
            let identity=host.play.get(&conn).and_then(|p|p.ae2_bus.as_ref()).and_then(|s|s.emitter_identity);
            if dim != self.dim.id() || !in_range || data.is_none()
                || session_owner.is_none_or(|old|data.as_ref().and_then(|v|self.bus_ui_identity(pos,v))!=Some(old)) {
                self.lan_ae2_bus_closed(host, conn, None);
                host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose { container: window }));continue;
            }
            if identity.is_some() {
                let signature=data.as_ref().and_then(stable);
                let changed=host.play.get(&conn).and_then(|p|p.ae2_bus.as_ref()).is_some_and(|s|Some(&s.signature)!=signature.as_ref());
                if changed {
                    let mut reset=None;
                    if let Some(s)=host.play.get_mut(&conn).and_then(|p|p.ae2_bus.as_mut()) {
                        s.menu=data.as_ref().map(emitter_session_menu).unwrap_or_else(||Menu::new(Kind::Chest(6)));s.emitter_edit=None;
                        reset=data.as_ref().and_then(|data|reset_energy_edit(s,data)).map(|(token,text)|(s.window,token,text));
                    }
                    if let Some((window,token,text))=reset {Self::energy_payload(host,conn,window,token,"set",&text);}
                    self.lan_send_ae2_bus(host,conn);
                }
            }
        }
    }
}

// Staged additions to game/ae2_bus_ui.rs. Requires emitter-specific GuestBus
// fields, Menu restrictions, and the shared ae2_emitter state module.
fn emitter_session_menu(data:&Compound)->Menu {
    if field(data,"id").as_deref()==Some("ae2:energy_level_emitter") {Menu::new(Kind::Chest(6))} else {emitter_menu()}
}
fn emitter_menu() -> Menu {
    let mut menu=Menu::new(Kind::Chest(6));
    menu.emitter_upgrade_slot=Some(36);
    menu
}
fn emitter_card(state:&super::ae2_emitter::EmitterState,id:&str)->bool {
    state.upgrades.iter().flatten().any(|card|card.item.as_ref()==id)
}
impl Game<'_> {
    fn emitter_ui_provider_changed(&self,pos:Pos,face:u8) {
        if let Some(host)=self.ae2_local_address(pos) {
            self.ae2_emitter_provider_refresh(super::ae2_parts::PartAddress{host,face});
        }
    }
    fn bus_ui_identity(&self,pos:Pos,data:&Compound)->Option<(u64,[i32;4])> {
        let host=self.world.block_entities.incarnation(&pos)?;
        let part=super::ae2_channels::part_identity(&Tag::Compound(data.clone()))?;
        Some((host,part))
    }
    fn ae2_emitter_menu_items(&self,data:&Compound,edit:Option<&str>)->Option<Vec<Option<ItemStack>>> {
        use super::ae2_emitter::{EmitterFilter,emitter_read};
        let state=emitter_read(data).ok()?;
        let crafting=emitter_card(&state,"ae2:crafting_card");
        let fuzzy=emitter_card(&state,"ae2:fuzzy_card");
        let fluid=matches!(state.filter.as_ref(),Some(EmitterFilter::Fluid(_)));
        let mut items=vec![None;54];
        if let Some(edit)=edit {
            for digit in 0..10 {items[digit]=icon(format!("Threshold digit {digit}"));}
            items[10]=icon("Backspace threshold".into());
            items[11]=icon("Clear threshold".into());
            items[12]=icon("Done editing threshold".into());
            items[49]=icon(format!("Threshold entry: {edit}{}",if fluid {" mB"} else {""}));
        } else {
            items[0]=state.filter.as_ref().and_then(|filter|match filter {
                EmitterFilter::Item(item)=>Some(item.clone()),
                EmitterFilter::Fluid(key)=>super::ae2_fluids::fluid_bucket(&key.item_id).map(|id|{
                    let mut item=ItemStack::new(id,1);item.data_mut().custom_name=Some(horizon_client::item_data::text_name(format!("Fluid filter: {}",key.item_id)));item
                }),
            });
            items[1]=icon("Set fluid filter from held bucket".into());
            items[36]=state.upgrades[0].clone();
            if !crafting {
                items[42]=icon(format!("Redstone: {}",if state.low {"LOW_SIGNAL"} else {"HIGH_SIGNAL"}));
                let unit=if fluid {" mB"}else{""};
                for (i,base) in [1i64,10,100,1000].into_iter().enumerate() {
                    let step=if fluid {base.saturating_mul(1000)}else{base};
                    items[45+i]=icon(format!("Threshold: {}{unit}; left +{step}, right -{step}",state.threshold));
                }
                items[49]=icon(format!("Threshold: {}{unit}; click for digits",state.threshold));
            } else {
                items[43]=icon(format!("Craft via redstone: {}",state.craft_via_redstone));
            }
            if fuzzy {items[44]=icon(format!("Fuzzy mode: {}",state.fuzzy_mode));}
        }
        Some(items)
    }
    fn lan_send_ae2_emitter(&mut self,host:&mut LanHost,conn:u32) {
        let Some((pos,face,window,edit))=host.play.get(&conn).and_then(|p|p.ae2_bus.as_ref())
            .map(|s|(s.pos,s.face,s.window,s.emitter_edit.clone())) else{return};
        let Some(data)=self.bus_ui_read(pos,face) else{return};
        let Some(g)=host.hub.guests.iter().find(|g|g.conn==conn) else{return};
        let editing=edit.is_some();
        let Some(mut items)=self.ae2_emitter_menu_items(&data,edit.as_deref()) else{return};
        items.extend((9..36).chain(0..9).map(|i|g.inventory[i].clone()));
        let carried=g.carried.clone();
        Self::lan_ae2_fake_slots(host,conn,window,if editing {&[]}else{&[0]});
        let Some(session)=host.play.get_mut(&conn).and_then(|p|p.ae2_bus.as_mut()) else{return};
        let Some(signature)=stable(&data) else{return};
        session.signature=signature;session.state=(session.state+1)&0x7fff;
        let state_id=session.state;
        host.hub.inventory_told_by(conn,&items);
        let items=items.iter().map(|item|host.hub.stack(item)).collect();let carried=host.hub.stack(&carried);
        host.hub.send_to(conn,&C::ContainerSetContent(play::ContainerSetContent{container:window,state:state_id,items,carried}));
    }
    /// Source fake/filter and mode operations, shared by server and local
    /// emitter menus. Physical slots return false for the native menu engine.
    fn ae2_emitter_config_action(&mut self,session:&mut GuestBus,mut data:Compound,slot:Option<usize>,button:i32,raw_input:Input)->bool {
        use super::ae2_emitter::{EmitterFilter,emitter_read,emitter_store};
        let Ok(mut state)=emitter_read(&data) else{return true;};
        let input=if session.emitter_edit.is_none() && slot==Some(0) && raw_input==Input::QuickMove {Input::Pickup}else{raw_input};
        let crafting=emitter_card(&state,"ae2:crafting_card");
        let fuzzy=emitter_card(&state,"ae2:fuzzy_card");
        let mut changed=false;let mut refresh=false;
        if session.emitter_edit.is_some() && matches!(input,Input::Pickup) && matches!(button,0|1) {
            let mut entry=session.emitter_edit.take().unwrap();
            match slot {
                Some(digit @ 0..=9)=>{let mut candidate=entry.clone();candidate.push(char::from(b'0'+digit as u8));if candidate.len()<=19 && candidate.parse::<i64>().is_ok(){entry=candidate;}},
                Some(10)=>{entry.pop();},Some(11)=>entry.clear(),Some(12)=>{},_=>{}
            }
            if let Ok(value)=entry.parse::<i64>() {if value>=0 && value!=state.threshold {state.threshold=value;changed=true;}}
            if slot!=Some(12) {session.emitter_edit=Some(entry);}
        } else if session.emitter_edit.is_none() && input==Input::Pickup && matches!(button,0..=3) && slot==Some(0) {
            let held=self.carried.as_ref();
            if matches!(button,2|3) && held.is_none() {
                return true;
            }
            let wrapped=held.map(|item|super::ae2_encoding::wrapped_ghost(item,&|id,fluid|fluid||self.res.items().get(id).is_some())).transpose();
            let fluid=if button==1 {held.and_then(|held|super::ae2_fluids::bucket_fluid(&held.item)).map(super::ae2_fluids::fluid_key)}else{None};
            let next=if let Some(fluid)=fluid {Some(EmitterFilter::Fluid(fluid))}else{match wrapped {
                Ok(Some(Some(super::ae2_encoding::WrappedGhost::Fluid(fluid))))=>Some(EmitterFilter::Fluid(fluid.key)),
                Ok(Some(Some(super::ae2_encoding::WrappedGhost::Item(mut item))))=>{item.count=1;Some(EmitterFilter::Item(item))},
                Err(_)=>state.filter.clone(),
                _=>held.and_then(|held|{super::ae2_lab::key(held).ok()?;let mut one=held.clone();one.count=1;Some(EmitterFilter::Item(one))}),
            }};
            changed=state.filter!=next;refresh=changed;state.filter=next;
        } else if matches!(input,Input::Pickup) && matches!(button,0|1) && slot==Some(1) {
            if let Some(key)=self.carried.as_ref().and_then(|item|super::ae2_fluids::bucket_fluid(&item.item)).map(super::ae2_fluids::fluid_key) {
                let next=Some(EmitterFilter::Fluid(key));changed=state.filter!=next;refresh=changed;state.filter=next;
            }
        } else if matches!(input,Input::Pickup) && matches!(button,0|1) && slot==Some(42) && !crafting {
            state.low=!state.low;changed=true;refresh=true;
        } else if matches!(input,Input::Pickup) && matches!(button,0|1) && slot==Some(43) && crafting {
            state.craft_via_redstone=!state.craft_via_redstone;changed=true;refresh=true;
        } else if matches!(input,Input::Pickup) && matches!(button,0|1) && slot==Some(44) && fuzzy {
            let values=["ignore_all","percent_99","percent_75","percent_50","percent_25"];
            let current=values.iter().position(|v|*v==state.fuzzy_mode).unwrap_or(0);
            state.fuzzy_mode=values[if button==0 {(current+1)%5}else{(current+4)%5}].into();changed=true;refresh=true;
        } else if matches!(input,Input::Pickup|Input::QuickMove) && matches!(button,0|1)
            && slot.is_some_and(|slot|(45..49).contains(&slot)) && !crafting {
            let i=slot.unwrap()-45;let base=if input==Input::QuickMove {[1i64,16,32,64][i]}else{[1i64,10,100,1000][i]};
            let amount=if matches!(state.filter.as_ref(),Some(EmitterFilter::Fluid(_))) {base.saturating_mul(1000)} else {base};
            let old=state.threshold;state.threshold=if button==0 {old.saturating_add(amount).max(0)}else{old.saturating_sub(amount).max(0)};changed=old!=state.threshold;
        } else if matches!(input,Input::Pickup) && matches!(button,0|1) && slot==Some(49) && !crafting {
            session.emitter_edit=Some(state.threshold.to_string());
        } else if input==Input::QuickMove && slot.is_some_and(|slot|(54..90).contains(&slot)) {
            let index=if slot.unwrap()<81 {slot.unwrap()-54+9}else{slot.unwrap()-81};
            let value=self.inventory[index].clone();
            if state.filter.is_none() && value.as_ref().is_some_and(|v|!matches!(v.item.as_ref(),"ae2:fuzzy_card"|"ae2:crafting_card")) {
                if let Some(mut item)=value.filter(|item|super::ae2_lab::key(item).is_ok()) {item.count=1;state.filter=Some(EmitterFilter::Item(item));changed=true;refresh=true;}
            } else {
                return false;
            }
        } else if input==Input::QuickCraft || (slot.is_none() && input == Input::Pickup) || slot==Some(36) || slot.is_some_and(|slot|(54..90).contains(&slot)) {
            return false;
        }
        if changed && emitter_store(&mut data,&state).is_ok() && self.bus_ui_store(session.pos,session.face,data) && refresh {
            self.emitter_ui_provider_changed(session.pos,session.face);
        }
        true
    }
    fn lan_ae2_emitter_click(&mut self,host:&mut LanHost,conn:u32,gi:usize,p:&play::ContainerClick,
        mut session:GuestBus,data:Compound)->bool {
        let Ok(state)=super::ae2_emitter::emitter_read(&data) else {
            host.play.entry(conn).or_default().ae2_bus=Some(session);
            self.lan_ae2_bus_closed(host,conn,None);host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose{container:p.container}));return true;
        };
        let input=super::lan_crafting::input(p.input);let slot=usize::try_from(p.slot).ok();
        if self.as_guest(host,conn,|game|game.ae2_emitter_config_action(&mut session,data.clone(),slot,i32::from(p.button),input)).unwrap_or(false) {
            host.play.entry(conn).or_default().ae2_bus=Some(session);self.lan_send_ae2_bus(host,conn);return true;
        }
        self.lan_ae2_emitter_physical(host,conn,gi,p,session,data,state)
    }
    fn lan_ae2_emitter_physical(&mut self,host:&mut LanHost,conn:u32,gi:usize,p:&play::ContainerClick,
        mut session:GuestBus,mut data:Compound,mut state:super::ae2_emitter::EmitterState)->bool {
        let input=super::lan_crafting::input(p.input);let before=host.hub.guests[gi].inventory.clone();let old_upgrades=state.upgrades.clone();
        let armor=host.hub.guests[gi].armor.clone();let offhand=host.hub.guests[gi].offhand.clone();let carried=host.hub.guests[gi].carried.clone();let random=self.world.random.clone();
        let (result,effects)=self.guest_menu_op_deferred(host,gi,&mut session.menu,|menu,env|{
            menu.set_slot(env,36,state.upgrades[0].clone());menu.click(env,i32::from(p.slot),i32::from(p.button),input)?;Ok::<_,&'static str>(menu.slot(36).cloned())
        });
        let mut upgrade_changed=false;
        let committed=result.map_err(str::to_owned).and_then(|after|{
            if after!=state.upgrades[0] {
                upgrade_changed=true;state.upgrades[0]=after;super::ae2_emitter::emitter_store(&mut data,&state)?;
                if !self.bus_ui_store(session.pos,session.face,data) {return Err("emitter face changed".into());}
            }
            Ok(())
        });
        if committed.is_err() {
            let g=&mut host.hub.guests[gi];g.inventory=before;g.armor=armor;g.offhand=offhand;g.carried=carried;self.world.random=random;
            for item in old_upgrades.iter().chain(state.upgrades.iter()).chain(g.inventory.iter()).chain(g.armor.iter()).chain(std::iter::once(&g.offhand)).chain(std::iter::once(&g.carried)).flatten() {horizon_client::ae2_stack_identity::synchronize_tracked(item);}
            session.menu=emitter_menu();host.play.entry(conn).or_default().ae2_bus=Some(session);host.hub.send_inventory(gi);self.lan_send_ae2_bus(host,conn);return true;
        }
        self.apply_guest_menu_effects(host,gi,effects);
        if upgrade_changed {self.emitter_ui_provider_changed(session.pos,session.face);}
        if input==Input::Swap {host.hub.send_inventory(gi);}
        if emitter_card(&state,"ae2:crafting_card") {session.emitter_edit=None;}
        let quick_stage=i32::from(p.button)&3;
        host.play.entry(conn).or_default().ae2_bus=Some(session);
        if input!=Input::QuickCraft || quick_stage>=2 {self.lan_send_ae2_bus(host,conn);}
        true
    }
}

// Staged functions for ae2_bus_ui.rs. The source's real Energy Emitter has
// no filter or upgrade inventory; number controls use the existing bus keypad.
impl Game<'_> {
    fn ae2_energy_emitter_menu_items(&self,data:&Compound,edit:Option<&str>)->Option<Vec<Option<ItemStack>>> {
        let state=super::ae2_emitter::energy_emitter_read(data).ok()?;
        let mut items=vec![None;54];
        if let Some(edit)=edit {
            for digit in 0..10 {items[digit]=icon(format!("Energy threshold digit {digit}"));}
            items[10]=icon("Backspace threshold".into());items[11]=icon("Clear threshold".into());items[12]=icon("Done editing threshold".into());
            items[49]=icon(format!("Threshold entry: {edit} AE"));
        } else {
            items[42]=icon(format!("Redstone: {}",if state.low {"LOW_SIGNAL"}else{"HIGH_SIGNAL"}));
            for (i,step) in [1i64,10,100,1000].into_iter().enumerate() {
                items[45+i]=icon(format!("Threshold: {} AE; left +{step}, right -{step}",state.threshold.max(0)));
            }
            items[49]=icon(format!("Threshold: {} AE; click for digits",state.threshold.max(0)));
        }
        Some(items)
    }
    fn lan_send_ae2_energy_emitter(&mut self,host:&mut LanHost,conn:u32) {
        let Some((pos,face,window,edit))=host.play.get(&conn).and_then(|p|p.ae2_bus.as_ref())
            .map(|s|(s.pos,s.face,s.window,s.emitter_edit.clone())) else{return};
        let Some(data)=self.bus_ui_read(pos,face) else{return};
        let Some(g)=host.hub.guests.iter().find(|g|g.conn==conn) else{return};
        let Some(mut items)=self.ae2_energy_emitter_menu_items(&data,edit.as_deref()) else{return};
        items.extend((9..36).chain(0..9).map(|i|g.inventory[i].clone()));let carried=g.carried.clone();
        let Some(session)=host.play.get_mut(&conn).and_then(|p|p.ae2_bus.as_mut()) else{return};
        let Some(signature)=stable(&data) else{return};session.signature=signature;session.state=(session.state+1)&0x7fff;
        host.hub.inventory_told_by(conn,&items);
        let items=items.iter().map(|item|host.hub.stack(item)).collect();let carried=host.hub.stack(&carried);
        host.hub.send_to(conn,&C::ContainerSetContent(play::ContainerSetContent{container:window,state:session.state,items,carried}));
    }
    fn ae2_energy_emitter_config_action(&mut self,session:&mut GuestBus,mut data:Compound,slot:Option<usize>,button:i32,input:Input)->(bool,Option<(u64,String)>) {
        let Ok(mut state)=super::ae2_emitter::energy_emitter_read(&data) else{return (true,None);};
        let before_threshold=state.threshold;
        let mut changed=false;let mut mode_changed=false;let mut echo=None;
        if session.emitter_edit.is_some() && matches!(input,Input::Pickup) && matches!(button,0|1) && slot.is_some_and(|s|s<=12) {
            let mut entry=session.emitter_edit.take().unwrap();match slot {
                Some(digit @ 0..=9)=>{let mut candidate=entry.clone();candidate.push(char::from(b'0'+digit as u8));if candidate.len()<=16 && candidate.parse::<i64>().is_ok(){entry=candidate;}},
                Some(10)=>{entry.pop();},Some(11)=>entry.clear(),Some(12)=>{},_=>{}
            }
            if let Some(value)=super::ae2_number_entry::number_value(&entry,1,0,i64::MAX,'.') {if value!=state.threshold {state.threshold=value;changed=true;}}
            if let Some(edit)=session.energy_edit.as_mut() {edit.text=entry.clone();echo=Some((edit.token,entry.clone()));}
            if slot!=Some(12){session.emitter_edit=Some(entry);}
        } else if session.emitter_edit.is_none() && matches!(input,Input::Pickup) && matches!(button,0|1) && slot==Some(42) {
            state.low=!state.low;changed=true;mode_changed=true;
        } else if session.emitter_edit.is_none() && matches!(input,Input::Pickup|Input::QuickMove) && matches!(button,0|1) && slot.is_some_and(|s|(45..49).contains(&s)) {
            let i=slot.unwrap()-45;let amount=if input==Input::QuickMove {[1i64,16,32,64][i]}else{[1i64,10,100,1000][i]};
            let delta=if button==0 {amount}else{-amount};
            if let Some(edit)=session.energy_edit.as_mut() {
                if let Some(next)=super::ae2_number_entry::number_step(&edit.text,delta,0,i64::MAX,'.') {
                    if let Some(value)=super::ae2_number_entry::number_value(&next,1,0,i64::MAX,'.') {state.threshold=value;changed=state.threshold!=before_threshold;}
                    edit.text=next.clone();echo=Some((edit.token,next));
                }
            }
        } else if session.emitter_edit.is_none() && matches!(input,Input::Pickup) && matches!(button,0|1) && slot==Some(49) {
            session.emitter_edit=Some(state.threshold.max(0).to_string());
        } else if input==Input::QuickMove && slot.is_some_and(|s|(54..90).contains(&s)) {
            // No destination slots exist for an Energy Level Emitter.
        } else if input==Input::QuickCraft && button&3==1 && slot.is_some_and(|s|s<54) {
            // Virtual controls are not inventory destinations.
        } else if input==Input::QuickCraft || (slot.is_none() && input == Input::Pickup) || slot.is_some_and(|s|(54..90).contains(&s)) {
            return (false,None);
        }
        if changed && super::ae2_emitter::energy_emitter_store(&mut data,&state).is_ok()
            && self.bus_ui_store_with_dirty(session.pos,session.face,data,false) {
            if let Some(host_address)=self.ae2_local_address(session.pos) {
                let address=super::ae2_parts::PartAddress{host:host_address,face:session.face};
                if mode_changed {
                    if self.ae2_energy_emitter_setting_callback(address,before_threshold) {
                        self.world.modified.insert((session.pos.0>>4,session.pos.2>>4));
                    }
                } else {self.ae2_energy_emitter_threshold_callback(address);}
            }
        }
        (true,echo)
    }
    fn lan_ae2_energy_emitter_click(&mut self,host:&mut LanHost,conn:u32,gi:usize,p:&play::ContainerClick,
        mut session:GuestBus,data:Compound)->bool {
        if super::ae2_emitter::energy_emitter_read(&data).is_err() {
            host.play.entry(conn).or_default().ae2_bus=Some(session);self.lan_ae2_bus_closed(host,conn,None);
            host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose{container:p.container}));return true;
        }
        let input=super::lan_crafting::input(p.input);let slot=usize::try_from(p.slot).ok();
        let (handled,echo)=self.as_guest(host,conn,|game|game.ae2_energy_emitter_config_action(&mut session,data,slot,i32::from(p.button),input)).unwrap_or((true,None));
        if !handled {return self.lan_ae2_energy_emitter_player_click(host,conn,gi,p,session);}
        if let Some((token,text))=echo {Self::energy_payload(host,conn,session.window,token,"set",&text);}
        host.play.entry(conn).or_default().ae2_bus=Some(session);self.lan_send_ae2_bus(host,conn);true
    }
    fn lan_ae2_energy_emitter_player_click(&mut self,host:&mut LanHost,conn:u32,gi:usize,p:&play::ContainerClick,mut session:GuestBus)->bool {
        let input=super::lan_crafting::input(p.input);let before=host.hub.guests[gi].inventory.clone();
        let armor=host.hub.guests[gi].armor.clone();let offhand=host.hub.guests[gi].offhand.clone();let carried=host.hub.guests[gi].carried.clone();let random=self.world.random.clone();
        let (result,effects)=self.guest_menu_op_deferred(host,gi,&mut session.menu,|menu,env|menu.click(env,i32::from(p.slot),i32::from(p.button),input));
        if result.is_err() {
            let g=&mut host.hub.guests[gi];g.inventory=before;g.armor=armor;g.offhand=offhand;g.carried=carried;self.world.random=random;
            for item in g.inventory.iter().chain(g.armor.iter()).chain(std::iter::once(&g.offhand)).chain(std::iter::once(&g.carried)).flatten() {horizon_client::ae2_stack_identity::synchronize_tracked(item);}
            session.menu=Menu::new(Kind::Chest(6));host.play.entry(conn).or_default().ae2_bus=Some(session);host.hub.send_inventory(gi);self.lan_send_ae2_bus(host,conn);return true;
        }
        self.apply_guest_menu_effects(host,gi,effects);if input==Input::Swap {host.hub.send_inventory(gi);}
        let stage=i32::from(p.button)&3;host.play.entry(conn).or_default().ae2_bus=Some(session);
        if input!=Input::QuickCraft || stage>=2 {self.lan_send_ae2_bus(host,conn);}true
    }
}

// Internal text envelope cannot collide with ordinary editable text: EditBox
// removes the leading control character. The nonce identifies one menu open.
const ENERGY_TEXT_PREFIX:&str="\u{1f}ae2e:";
#[derive(Clone)]
struct EnergyEdit {token:u64,text:String}
pub(super) fn energy_transport_value(token:u64,text:&str)->String {format!("{ENERGY_TEXT_PREFIX}{token}:{text}")}
fn energy_transport_for(value:&str,expected:u64)->Option<&str> {
    let (token,text)=value.strip_prefix(ENERGY_TEXT_PREFIX)?.split_once(':')?;
    (token.parse::<u64>().ok()?==expected).then_some(text)
}
pub(super) fn energy_confirmable(text:&str)->bool {
    super::ae2_number_entry::number_value(text,1,0,i64::MAX,'.').is_some()
}
fn energy_entry_text(value:i64)->String {value.max(0).to_string().chars().take(16).collect()}
fn reset_energy_edit(session:&mut GuestBus,data:&Compound)->Option<(u64,String)> {
    let state=super::ae2_emitter::energy_emitter_read(data).ok()?;let edit=session.energy_edit.as_mut()?;
    edit.text=energy_entry_text(state.threshold);Some((edit.token,edit.text.clone()))
}
impl Game<'_> {
    fn energy_payload(host:&mut LanHost,conn:u32,window:i32,token:u64,mode:&str,text:&str) {
        if let Ok(data)=serde_json::to_vec(&serde_json::json!({"window":window,"token":token,"mode":mode,"value":text})) {
            host.hub.send_to(conn,&C::CustomPayload(horizon_protocol::packets::configuration::CustomPayload{channel:"ae2:energy_editor".into(),data}));
        }
    }
    pub(super) fn lan_ae2_energy_text(&mut self,host:&mut LanHost,conn:u32,value:&str)->bool {
        if !value.starts_with(ENERGY_TEXT_PREFIX){return false;}
        let Some(gi)=host.hub.guests.iter().position(|g|g.conn==conn) else{return true};
        let Some(mut session)=host.play.get_mut(&conn).and_then(|p|p.ae2_bus.take()) else{return true};
        let Some(edit)=session.energy_edit.as_ref() else{host.play.entry(conn).or_default().ae2_bus=Some(session);return true};
        let Some(text)=energy_transport_for(value,edit.token) else{host.play.entry(conn).or_default().ae2_bus=Some(session);return true};
        let data=self.bus_ui_read(session.pos,session.face);
        let valid=host.hub.here(&host.hub.guests[gi]) && !host.hub.guests[gi].spectator()
            && host.hub.guests[gi].within_block_range([session.pos.0,session.pos.1,session.pos.2],4.0)
            && session.dim==self.dim.id() && data.as_ref().is_some_and(|data|
                field(data,"id").as_deref()==Some("ae2:energy_level_emitter") && self.bus_ui_identity(session.pos,data)==session.emitter_identity);
        if !valid {
            let window=session.window;host.play.entry(conn).or_default().ae2_bus=Some(session);self.lan_ae2_bus_closed(host,conn,None);
            host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose{container:window}));return true;
        }
        let mut data=data.unwrap();let Ok(mut state)=super::ae2_emitter::energy_emitter_read(&data) else {
            let window=session.window;host.play.entry(conn).or_default().ae2_bus=Some(session);self.lan_ae2_bus_closed(host,conn,None);
            host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose{container:window}));return true;
        };
        if stable(&data).as_ref()!=Some(&session.signature) {
            let edit=session.energy_edit.as_mut().unwrap();edit.text=energy_entry_text(state.threshold);
            Self::energy_payload(host,conn,session.window,edit.token,"set",&edit.text);session.emitter_edit=None;
            host.play.entry(conn).or_default().ae2_bus=Some(session);self.lan_send_ae2_bus(host,conn);return true;
        }
        let text=text.chars().take(16).collect::<String>();
        if let Some(next)=super::ae2_number_entry::number_value(&text,1,0,i64::MAX,'.') {
            if next!=state.threshold {state.threshold=next;
                if super::ae2_emitter::energy_emitter_store(&mut data,&state).is_ok() && self.bus_ui_store_with_dirty(session.pos,session.face,data,false) {
                    if let Some(host_address)=self.ae2_local_address(session.pos) {
                        self.ae2_energy_emitter_threshold_callback(super::ae2_parts::PartAddress{host:host_address,face:session.face});
                    }
                }
            }
        }
        session.energy_edit.as_mut().unwrap().text=text.clone();if session.emitter_edit.is_some(){session.emitter_edit=Some(text);}
        host.play.entry(conn).or_default().ae2_bus=Some(session);self.lan_send_ae2_bus(host,conn);true
    }
}

#[cfg(test)]
mod emitter_menu_tests {
    #[test]
    fn fake_action_revision_checks_page_and_enabled_slot_count() {
        use super::bus_fake_revision_matches;
        let history=std::collections::VecDeque::from([(5,0,9),(6,0,9),(7,1,63)]);
        assert!(bus_fake_revision_matches(&history,5,0,9));
        assert!(!bus_fake_revision_matches(&history,5,1,9));
        assert!(!bus_fake_revision_matches(&history,5,0,18));
        assert!(!bus_fake_revision_matches(&history,4,0,9));
    }
    use super::*;
    #[test]
    fn number_entry_energy_wire_and_limits() {
        let old=energy_transport_value(7,"=2^3^4");
        assert_eq!(energy_transport_for(&old,7),Some("=2^3^4"));assert_eq!(energy_transport_for(&old,8),None);
        assert!(energy_confirmable("=2^3^4"));assert!(!energy_confirmable("-1"));assert!(!energy_confirmable("1/3"));
        assert!(energy_transport_for(&horizon_client::clipboard::filter_text(&old,false),7).is_none());
    }
    #[test]
    fn emitter_physical_upgrade_slot_filters_conserves_and_orders() {
        use horizon_client::{banner::Banners,brewing::Brewing,crafting::Recipes,items::Items,menu::Env};
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/resources/26.3");if !root.join("items.json").exists(){return;}
        let items=Items::catalog(&root, &horizon_world::loot::PackOverlay::NONE).unwrap();let recipes=Recipes::load(&root, &items, &horizon_world::loot::PackOverlay::NONE).unwrap();let brewing=Brewing::load(&root, &items, &horizon_world::loot::PackOverlay::NONE);let banners=Banners::load(&root);
        let stations=horizon_client::stations::StationRecipes::load(&root, &items, &horizon_world::loot::PackOverlay::NONE);let enchantments=horizon_client::enchant::Enchantments::default();let names=horizon_client::creative::Names::default();
        let env=Env{items:&items,recipes:&recipes,brewing:&brewing,banners:&banners,station_recipes:&stations,enchantments:&enchantments,names:&names,trade:None,maps:None};
        let card=|id:&str,n|Some(ItemStack::new(id,n));let mut menu=emitter_menu();
        menu.carried=card("ae2:speed_card",2);menu.click(&env,36,0,Input::Pickup).unwrap();assert!(menu.slot(36).is_none());assert_eq!(menu.carried.as_ref().map(|s|s.count),Some(2));
        menu.carried=card("ae2:fuzzy_card",2);menu.click(&env,36,0,Input::Pickup).unwrap();assert_eq!(menu.slot(36).map(|s|s.count),Some(1));assert_eq!(menu.carried.as_ref().map(|s|s.count),Some(1));
        menu.carried=None;menu.click(&env,36,0,Input::QuickMove).unwrap();assert!(menu.slot(36).is_none());assert_eq!(menu.player_item(9).map(|s|s.item.as_ref()),Some("ae2:fuzzy_card"));
        let mut gather=emitter_menu();gather.set_slot(&env,36,card("ae2:fuzzy_card",1));gather.set_player_item(5,card("ae2:fuzzy_card",3));gather.set_player_item(9,card("ae2:fuzzy_card",4));gather.carried=card("ae2:fuzzy_card",60);
        gather.click(&env,84,0,Input::PickupAll).unwrap();assert_eq!(gather.carried.as_ref().map(|s|s.count),Some(64));assert!(gather.slot(36).is_none());assert!(gather.player_item(5).is_none());assert_eq!(gather.player_item(9).map(|s|s.count),Some(4));
        let mut drag=emitter_menu();drag.carried=card("ae2:crafting_card",2);for (slot,button) in [(-999,0),(36,1),(-999,2)] {drag.click(&env,slot,button,Input::QuickCraft).unwrap();}
        assert_eq!(drag.slot(36).map(|s|s.count),Some(1));assert_eq!(drag.carried.as_ref().map(|s|s.count),Some(1));
        drag.carried=None;drag.click(&env,36,0,Input::Throw).unwrap();assert!(drag.slot(36).is_none());assert_eq!(drag.dropped.len(),1);assert_eq!(drag.dropped[0].item.as_ref(),"ae2:crafting_card");
    }
}

