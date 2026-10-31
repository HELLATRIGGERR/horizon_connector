//! Spatial Anchor screen: statistics and its persisted overlay setting are
//! derived from the physical anchor and the authoritative ticket service.
use super::{Game, lan::LanHost};
use horizon_client::{ae2_world::MachineKind, containers::BlockEntity, items::ItemStack};
use horizon_protocol::packets::play::{self, Clientbound as C};
use horizon_world::nbt::Tag;

pub(super) struct GuestAnchor { window:i32, state:i32, pos:(i32,i32,i32), dim:String, incarnation:u64, refresh:u8 }
impl Game<'_> {
    pub(super) fn lan_open_ae2_anchor(&mut self,host:&mut LanHost,conn:u32,pos:(i32,i32,i32))->bool {
        if !matches!(self.world.block_entities.get(&pos),Some(BlockEntity::Ae2(m)) if m.kind==MachineKind::SpatialAnchor) {return false;}
        let Some(incarnation)=self.world.block_entities.incarnation(&pos) else{return false;};
        self.lan_ae2_anchor_closed(host,conn,None);
        self.lan_ae2_terminal_closed(host,conn,None);self.lan_ae2_machine_closed(host,conn,None);
        self.lan_ae2_bus_closed(host,conn,None);self.lan_ae2_portable_closed(host,conn,None);
        self.lan_ae2_network_tool_closed(host,conn,None);self.lan_craft_confirm_closed(host,conn,None);
        self.lan_pattern_access_closed(host,conn,None);self.lan_station_closed(host,conn,None);
        self.lan_crafting_closed(host,conn,None);self.guest_container_closed(host,conn);
        let Some(window)=host.hub.open_window(conn,"minecraft:generic_9x3",horizon_protocol::text::literal("AE2 Spatial Anchor")) else{return true;};
        host.play.entry(conn).or_default().ae2_anchor=Some(GuestAnchor{window,state:0,pos,dim:self.dim.id().into(),incarnation,refresh:0});
        self.lan_send_ae2_anchor(host,conn);true
    }
    fn lan_send_ae2_anchor(&mut self,host:&mut LanHost,conn:u32) {
        let Some((pos,window,state))=host.play.get(&conn).and_then(|p|p.ae2_anchor.as_ref()).map(|s|(s.pos,s.window,s.state)) else{return;};
        let snapshot=self.ae2_local_address(pos).and_then(|a|self.ae2_anchor_snapshot(a));
        let labels=if let Some(s)=snapshot {
            vec![format!("Overlay: {}",if s.overlay{"on"}else{"off"}),format!("Power: {:.0} AE/t",s.idle),format!("Loaded chunks: {}",s.owned_chunks.len()),format!("Loaded: {} chunks in {} worlds",s.anchor_max_by_dimension.iter().map(|(_,n)|n).sum::<usize>(),s.anchor_max_by_dimension.len()),format!("Total: {} chunks in {} worlds",s.grid_chunks_by_dimension.iter().map(|(_,v)|v.len()).sum::<usize>(),s.grid_chunks_by_dimension.len())]
        } else {vec!["Network unavailable".into()]};
        let mut items:Vec<Option<ItemStack>>=(0..27).map(|i|labels.get(i).map(|v|{let mut s=ItemStack::new("minecraft:paper",1);s.data_mut().custom_name=Some(horizon_client::item_data::text_name(v.clone()));s})).collect();
        let Some(g)=host.hub.guests.iter().find(|g|g.conn==conn) else{return;};
        items.extend((9..36).chain(0..9).map(|i|g.inventory[i].clone()));let carried=g.carried.clone();
        let Some(s)=host.play.get_mut(&conn).and_then(|p|p.ae2_anchor.as_mut()) else{return;};s.state=(state+1)&0x7fff;let state=s.state;
        let items=items.iter().map(|i|host.hub.stack(i)).collect();let carried=host.hub.stack(&carried);
        host.hub.send_to(conn,&C::ContainerSetContent(play::ContainerSetContent{container:window,state,items,carried}));
    }
    pub(super) fn lan_ae2_anchor_click(&mut self,host:&mut LanHost,conn:u32,p:&play::ContainerClick)->bool {
        let Some((pos,window,dim,incarnation))=host.play.get(&conn).and_then(|p|p.ae2_anchor.as_ref()).map(|s|(s.pos,s.window,s.dim.clone(),s.incarnation)) else{return false;};
        if p.container!=window{return false;}
        let valid=dim==self.dim.id() && self.world.block_entities.incarnation(&pos)==Some(incarnation) && host.hub.guests.iter().find(|g|g.conn==conn && host.hub.here(g)).is_some_and(|g|!g.spectator() && g.within_block_range([pos.0,pos.1,pos.2],4.0));
        if !valid {self.lan_ae2_anchor_closed(host,conn,None);host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose{container:window}));return true;}
        if p.slot==0 && super::lan_crafting::input(p.input)==horizon_client::menu::Input::Pickup {
            if let Some(BlockEntity::Ae2(m))=self.world.block_entities.get_mut(&pos) {
                if m.kind==MachineKind::SpatialAnchor {let enabled=matches!(m.config.get(&"overlay_mode".into()),Some(Tag::String(v)) if v.to_string_lossy()=="YES");m.config.insert("overlay_mode".into(),Tag::String(if enabled{"NO"}else{"YES"}.into()));self.world.modified.insert((pos.0>>4,pos.2>>4));}
            }
        }
        self.lan_send_ae2_anchor(host,conn);true
    }
    pub(super) fn lan_ae2_anchor_closed(&mut self,host:&mut LanHost,conn:u32,window:Option<i32>)->bool {
        let Some(_)=host.play.get_mut(&conn).and_then(|p|p.ae2_anchor.take_if(|s|window.is_none_or(|w|w==s.window))) else{return false;};
        if let Some(i)=host.hub.guests.iter().position(|g|g.conn==conn){self.guest_carried_back(host,i);}true
    }
    pub(super) fn lan_refresh_ae2_anchors(&mut self,host:&mut LanHost) {
        let viewers:Vec<_>=host.play.iter().filter_map(|(&c,p)|p.ae2_anchor.as_ref().map(|s|(c,s.pos,s.dim.clone(),s.incarnation,s.window))).collect();
        for (conn,pos,dim,incarnation,window) in viewers {
            let Some(g)=host.hub.guests.iter().find(|g|g.conn==conn && host.hub.here(g)) else{continue;};
            if dim!=self.dim.id() || self.world.block_entities.incarnation(&pos)!=Some(incarnation) || g.spectator() || !g.within_block_range([pos.0,pos.1,pos.2],4.0) {
                self.lan_ae2_anchor_closed(host,conn,None);host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose{container:window}));continue;
            }
            if let Some(s)=host.play.get_mut(&conn).and_then(|p|p.ae2_anchor.as_mut()) {s.refresh+=1;if s.refresh<20{continue;}s.refresh=0;}
            self.lan_send_ae2_anchor(host,conn);
        }
    }
}
