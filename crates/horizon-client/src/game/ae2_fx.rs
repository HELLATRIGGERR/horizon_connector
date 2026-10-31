//! Transient AE2 tool visuals; source LightningPacket / MatterCannonPacket.
//! These packets carry visuals only and never affect world entities or damage.
use super::{Game,lan::LanHost};
use horizon_client::mesher::Vertex;
use horizon_core::random::RandomSource;
use horizon_protocol::packets::{configuration::CustomPayload,play::Clientbound};
use serde::{Deserialize,Serialize};
const CHANNEL:&str="ae2:tool_fx";
#[derive(Clone,Debug,Serialize,Deserialize)]
pub(super) struct Event { dim:String,kind:String,origin:[f64;3],direction:[f64;3],len:u8 }
fn visible_to(event:&Event,dim:&str,position:[f64;3])->bool {
    if dim!=event.dim {return false}
    let distance=(0..3).map(|i|(position[i]-event.origin[i]).powi(2)).sum::<f64>();
    let radius=if event.kind=="matter" {256.} else {32.};
    distance<=radius*radius
}
#[derive(Clone,Debug)]
pub(super) struct Effect { event:Event, born:i64 }
impl Game<'_> {
    /// Emit the source's two random lightning effects within the target AABB.
    pub(super) fn ae2_staff_fx(&mut self,min:[f64;3],max:[f64;3]) {
        for _ in 0..2 {
            let pos=std::array::from_fn(|i|min[i]+(max[i]-min[i])*f64::from(self.world.random.next_float()));
            self.ae2_queue_fx(Event{dim:self.dim.id().into(),kind:"lightning".into(),origin:pos.map(|v|f64::from(v as f32)),direction:[0.;3],len:0});
        }
    }
    /// Source MatterCannonPacket: points from 1 through len-1 along a normalized ray.
    pub(super) fn ae2_matter_fx(&mut self,origin:[f64;3],direction:[f64;3],len:u8) {
        let magnitude=direction.iter().map(|x|x*x).sum::<f64>().sqrt();
        if !magnitude.is_finite() || magnitude<=f64::EPSILON {return}
        self.ae2_queue_fx(Event{dim:self.dim.id().into(),kind:"matter".into(),origin:origin.map(|v|f64::from(v as f32)),direction:direction.map(|x|f64::from((x/magnitude) as f32)),len});
    }
    fn ae2_queue_fx(&mut self,event:Event) {
        if self.remote.is_none() && self.lan.is_none() { let now=self.world.game_time; self.ae2_fx_active.retain(|e|now.saturating_sub(e.born)<20); if self.ae2_fx_active.len()<256 {self.ae2_fx_active.push(Effect{event:event.clone(),born:now});} }
        if self.ae2_fx_pending.len()<256 {self.ae2_fx_pending.push(event);}
    }
    pub(super) fn ae2_flush_fx(&mut self,host:&LanHost) {
        for event in self.ae2_fx_pending.drain(..) {
            let Ok(data)=serde_json::to_vec(&event) else {continue};
            for guest in &host.hub.guests {
                if visible_to(&event,&guest.dimension,guest.position) {guest.send(&Clientbound::CustomPayload(CustomPayload{channel:CHANNEL.into(),data:data.clone()}));}
            }
        }
    }
    pub(super) fn ae2_accept_fx(&mut self,channel:&str,data:&[u8]) {
        if channel!=CHANNEL || data.len()>1024 {return}
        let Ok(event)=serde_json::from_slice::<Event>(data) else {return};
        if event.dim!=self.dim.id() || !matches!(event.kind.as_str(),"lightning"|"matter") || event.origin.iter().chain(&event.direction).any(|v|!v.is_finite()) || event.len>128 {return}
        let now=self.world.game_time;
        self.ae2_fx_active.retain(|e|now.saturating_sub(e.born)<20);
        if self.ae2_fx_active.len()<256 {self.ae2_fx_active.push(Effect{event,born:now});}
    }
    pub(super) fn ae2_fx_geometry(&self,camera:glam::DVec3,out:&mut Vec<Vertex>) {
        let Some(white)=self.gui.atlas.sprites.get("horizon/white") else {return};
        let uv=[(white.u0+white.u1)*0.5,(white.v0+white.v1)*0.5];
        for fx in &self.ae2_fx_active {
            let age=self.world.game_time.saturating_sub(fx.born);
            if age<0 || age>=if fx.event.kind=="lightning" {7} else {10} || fx.event.dim!=self.dim.id(){continue}
            let start=glam::DVec3::from_array(fx.event.origin)-camera;
            let mut quad=|a:glam::DVec3,b:glam::DVec3,width:f64,color:u32| {
                let d=b-a;let mut n=d.cross(glam::DVec3::Y).normalize_or_zero();
                if n.length_squared()<0.01 {n=d.cross(glam::DVec3::X).normalize_or_zero();}
                for p in [a-n*width,a+n*width,b+n*width,b-n*width] {out.push(Vertex{glint_uv:[0.0,0.0],pos:p.as_vec3().to_array(),uv,layer:0,color,light:0xF0F0,normal:[0,0,0,0]});}
            };
            if fx.event.kind=="lightning" {
                // Five randomized, damped steps as in LightningFX.regen().
                let mut seed=(fx.event.origin[0].to_bits()^fx.event.origin[1].to_bits().rotate_left(17)^fx.event.origin[2].to_bits().rotate_left(31)) ^ ((age>=3) as u64);
                let mut rnd=||{seed^=seed<<13;seed^=seed>>7;seed^=seed<<17;((seed&0xffff) as f64/65535.-0.5)*0.9};
                let mut direction=glam::DVec3::new(rnd(),rnd(),rnd());let mut at=start;
                for i in 0..5 {direction=(direction+glam::DVec3::new(rnd(),rnd(),rnd()))*0.5;let next=at+direction;
                    let scale=(5-i) as f64/5.;quad(at,next,0.04*scale,0xff99472d);quad(at,next,0.02*scale,0xffffb776);at=next;}
            } else {
                let direction=glam::DVec3::from_array(fx.event.direction);
                let Some(sprite)=self.gui.atlas.sprites.get("ae2:particle/matter_cannon") else {continue};
                let forward=glam::DVec3::from_array(super::view_vector(self.yaw,self.pitch));
                let right=forward.cross(glam::DVec3::Y).normalize_or_zero();
                let up=right.cross(forward).normalize_or_zero();
                let size=1.1*1.19f64.powi(age as i32);
                let alpha=(1.4*0.59f64.powi(age as i32)).clamp(0.,1.);
                let color=((alpha*255.) as u32)<<24 | 0x00ffffff;
                for n in 1..(fx.event.len as i8).max(0) as u8 {
                    let p=start+direction*f64::from(n);
                    let points=[p-right*size-up*size,p+right*size-up*size,p+right*size+up*size,p-right*size+up*size];
                    let uvs=[[sprite.u0,sprite.v1],[sprite.u1,sprite.v1],[sprite.u1,sprite.v0],[sprite.u0,sprite.v0]];
                    for i in 0..4 {out.push(Vertex{glint_uv:[0.0,0.0],pos:points[i].as_vec3().to_array(),uv:uvs[i],layer:0,color,light:0xF0F0,normal:[0,0,0,0]});}
                }
            }
        }
    }
}

#[cfg(test)] mod tests {
 use super::*;
 #[test] fn effects_target_source_radius_and_dimension() {
  let mut event=Event{dim:"minecraft:overworld".into(),kind:"lightning".into(),origin:[0.;3],direction:[0.;3],len:0};
  assert!(visible_to(&event,"minecraft:overworld",[32.,0.,0.]));
  assert!(!visible_to(&event,"minecraft:overworld",[33.,0.,0.]));
  assert!(!visible_to(&event,"minecraft:the_nether",[0.,0.,0.]));
  event.kind="matter".into();assert!(visible_to(&event,"minecraft:overworld",[256.,0.,0.]));
  assert!(!visible_to(&event,"minecraft:overworld",[257.,0.,0.]));
 }
}
