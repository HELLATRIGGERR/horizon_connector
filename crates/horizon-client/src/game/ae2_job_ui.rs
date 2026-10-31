//! Client-owned crafting-job notices and automatic pins.
use super::{Game, Screen};
use ae2_cell_core::ItemKey;
use horizon_client::{ae2_job_status::{CraftingJobStatus,Status},hud::HudBatch,items::ItemStack};
use horizon_world::nbt::Tag;
use std::{collections::{BTreeMap,VecDeque},time::{Duration,Instant}};

const FLUID_MARKER:&str="horizon:crafting_fluid/";

/// Decode only the native typed key formats, without treating arbitrary event
/// data as an item. The key's component bytes remain part of its identity.
pub(super) fn parse_key(wire:&str)->Option<ItemKey> {
    if wire.len()>65_536 {return None;}
    if let Some(rest)=wire.strip_prefix("fluid|") {
        let (id,hex)=rest.split_once('|')?;
        if !super::ae2_fluids::valid_fluid_id(id) || hex.len()%2!=0 || !hex.is_ascii() {return None;}
        let bytes=hex.as_bytes();let mut components=Vec::with_capacity(bytes.len()/2);
        for pair in bytes.chunks_exact(2) {
            let digit=|c:u8| match c {b'0'..=b'9'=>Some(c-b'0'),b'a'..=b'f'=>Some(c-b'a'+10),b'A'..=b'F'=>Some(c-b'A'+10),_=>None};
            components.push(digit(pair[0])?*16+digit(pair[1])?);
        }
        return Some(ItemKey{item_id:format!("{FLUID_MARKER}{id}"),components});
    }
    let stack=horizon_client::item_data::parse_stack(wire).ok()??;
    super::ae2_lab::key(&stack).ok()
}

#[derive(Clone,Debug)] struct Pin { wire:String, key:ItemKey, prune:bool }
#[derive(Clone,Debug)] struct Toast { key:ItemKey, amount:u64, started:Option<Instant> }
#[derive(Default)] pub(super) struct JobUi {
    pending:BTreeMap<[i64;2],ItemKey>,
    pins:VecDeque<Pin>,
    toasts:VecDeque<Toast>,
    events:VecDeque<CraftingJobStatus>,
}
impl JobUi {
    fn apply(&mut self,message:&CraftingJobStatus,key:ItemKey,pin:bool,notify:bool) {
        self.events.push_back(message.clone());
        while self.events.len()>64 {self.events.pop_front();}
        match message.status {
            Status::Started=>{
                self.pending.entry(message.job_id).or_insert_with(||key.clone());
                if pin {
                    self.pins.retain(|p|p.key!=key);
                    self.pins.push_back(Pin{wire:message.key.clone(),key,prune:false});
                    while self.pins.len()>9 {self.pins.pop_front();}
                }
            }
            Status::Finished=>{
                self.pending.remove(&message.job_id);
                if notify {self.toasts.push_back(Toast{key,amount:message.requested,started:None});}
            }
            Status::Cancelled=>{self.pending.remove(&message.job_id);}
        }
    }
    pub(super) fn terminal_closed(&mut self) {
        for pin in &mut self.pins {if !self.pending.values().any(|key|*key==pin.key) {pin.prune=true;}}
    }
    pub(super) fn terminal_opened(&mut self) {self.pins.retain(|pin|!pin.prune);}
    pub(super) fn wires(&self)->Vec<String> {self.pins.iter().map(|pin|pin.wire.clone()).collect()}
    pub(super) fn keys(&self)->Vec<ItemKey> {self.pins.iter().map(|pin|pin.key.clone()).collect()}
    /// Read-only, bounded control-socket evidence for this client only.
    pub(super) fn snapshot(&self)->serde_json::Value {
        serde_json::json!({
            "events":&self.events,
            "pending":self.pending.keys().collect::<Vec<_>>(),
            "pins":self.pins.iter().map(|pin|serde_json::json!({"key":&pin.wire,"item":&pin.key.item_id,"prune":pin.prune})).collect::<Vec<_>>(),
            "toasts":self.toasts.iter().map(|toast|serde_json::json!({"item":&toast.key.item_id,"amount":toast.amount,"visible":toast.started.is_some()})).collect::<Vec<_>>(),
        })
    }
}

fn linked_and_powered(stack:&ItemStack)->bool {
    if !super::ae2_wireless::is_wireless(stack) || !super::ae2_wireless::energy(stack).is_ok_and(|(energy,_)|energy>0.0) {return false;}
    super::ae2_wireless::target(stack).is_some()
}

impl Game<'_> {
    pub(super) fn ae2_accept_job_status(&mut self,data:&[u8]) {
        if data.len()>131_072 {return;}
        let Ok(message)=serde_json::from_slice::<CraftingJobStatus>(data) else{return;};
        let Some(key)=parse_key(&message.key) else{return;};
        let settings=super::ae2_client_config::settings();
        let terminal_open=self.screen==Screen::Inventory && (
            self.remote.as_ref().is_some_and(|remote|remote.ae2_terminal_screen)
            || self.remote.is_none() && self.container.is_some_and(|pos|self.ae2_has_terminal(pos)));
        let notify=settings.notify_finished_jobs && !terminal_open && self.inventory.iter().flatten().any(linked_and_powered);
        self.ae2_job_ui.apply(&message,key,settings.pin_auto_crafted_items,notify);
        self.ae2_invalidate_local_terminal();
        self.ae2_sync_job_pins();
    }
    pub(super) fn ae2_draw_job_toast(&mut self,batch:&mut HudBatch) {
        let toast={
            let Some(toast)=self.ae2_job_ui.toasts.front_mut() else{return;};
            let started=*toast.started.get_or_insert_with(Instant::now);
            if started.elapsed()>=Duration::from_millis(2500) {self.ae2_job_ui.toasts.pop_front();return;}
            toast.clone()
        };
        let fluid=toast.key.item_id.strip_prefix(FLUID_MARKER);
        let id=fluid.unwrap_or(&toast.key.item_id);
        let name=self.pack.names.display_item(id);
        let icon=if let Some(id)=fluid {
            let mut icon=ItemStack::new("ae2:wrapped_generic_stack",1);
            icon.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("ae2:wrapped_stack",Tag::compound([
                ("#t",Tag::String("ae2:f".into())),("id",Tag::String(id.into())),("#",Tag::Long(toast.amount.min(i64::MAX as u64) as i64)),
            ]))])));Some(icon)
        } else {super::ae2_lab::prototype(&toast.key).ok()};
        let x=batch.width-160.0;
        batch.sprite(&self.gui.atlas,"gui/sprites/toast/recipe",x,0.0);
        batch.text(&self.gui.atlas,"Auto-Crafting Complete",x+30.0,7.0,0xFF50_0050,false);
        batch.text(&self.gui.atlas,&format!("{} {}",toast.amount,name),x+30.0,18.0,0xFF00_0000,false);
        if let Some(icon)=icon {self.draw_item_model(batch,&icon,x+8.0,8.0);}
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn owner_events_pin_until_closed_and_validate_typed_keys(){
        let wire=horizon_client::item_data::canon_stack(Some(&ItemStack::new("minecraft:diamond",1)));
        let key=parse_key(&wire).unwrap();
        let mut ui=JobUi::default();
        let mut event=CraftingJobStatus{job_id:[1,2],key:wire,requested:4,remaining:4,status:Status::Started};
        ui.apply(&event,key.clone(),true,false);assert_eq!(ui.keys(),vec![key.clone()]);
        ui.terminal_closed();ui.terminal_opened();assert_eq!(ui.keys(),vec![key.clone()]);
        event.status=Status::Finished;event.remaining=0;ui.apply(&event,key,false,true);
        assert_eq!(ui.toasts.len(),1);ui.terminal_closed();ui.terminal_opened();assert!(ui.keys().is_empty());
        assert!(parse_key("fluid|minecraft:water|0").is_none());
        assert!(parse_key("fluid|minecraft:water|0x").is_none());
        let fluid=parse_key("fluid|minecraft:water|00ff").unwrap();assert_eq!(fluid.components,vec![0,255]);
    }
}
