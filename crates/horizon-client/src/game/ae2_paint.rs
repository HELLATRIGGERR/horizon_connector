//! AE2 PaintSplotchesBlockEntity and Splotch (19.2.18, LGPL-3.0-or-later).
use super::Game;
use horizon_client::{ae2_world::{Machine,MachineKind},containers::BlockEntity};
use horizon_core::{geometry::Direction,position::BlockPos};
use horizon_world::nbt::Tag;

fn side_index(v:[i32;3])->Option<u8>{match v {[0,-1,0]=>Some(0),[0,1,0]=>Some(1),[0,0,-1]=>Some(2),[0,0,1]=>Some(3),[-1,0,0]=>Some(4),[1,0,0]=>Some(5),_=>None}}
fn normal(i:u8)->[i32;3]{Direction::ALL[i as usize].normal()}
/// AE2 stores a count followed by position and packed side/color/lumen bytes.
fn decode(data:&[u8])->Option<Vec<[u8;2]>> {
    let count=*data.first()? as usize;
    if count>21 || data.len()<1+2*count {return None}
    let mut dots=Vec::with_capacity(count);
    for pair in data[1..1+2*count].chunks_exact(2) {
        if pair[1]&7>5 {return None} dots.push([pair[0],pair[1]]);
    }
    Some(dots)
}
fn encode(dots:&[[u8;2]])->Vec<u8>{let mut out=vec![dots.len() as u8];for dot in dots {out.extend(dot)}out}
fn packed(side:u8,hit:[f64;3],color:u8,lit:bool)->[u8;2] {
    let (x,y)=match side {2|3=>(hit[0],hit[1]),0|1=>(hit[0],hit[2]),_=>(hit[1],hit[2])};
    let coord=|v:f64| (v.clamp(0.,1.)*15.) as u8;
    [coord(x)|(coord(y)<<4),side|(color<<3)|if lit {128}else{0}]
}
impl Game<'_>{
    fn ae2_paint_supported(&mut self,p:[i32;3],side:u8)->bool {
        let d=normal(side);let q=[p[0]+d[0],p[1]+d[1],p[2]+d[2]];
        if !self.world.loaded(q[0],q[2]) {return false}
        let state=self.world.block(q[0],q[1],q[2]);
        self.with_level(|b,_,_|b.is_face_sturdy(state,Direction::ALL[side as usize].opposite()))
    }
    fn ae2_paint_dots(&self,p:[i32;3])->Option<Vec<[u8;2]>> {
        let BlockEntity::Ae2(m)=self.world.block_entities.get(&(p[0],p[1],p[2]))? else {return None};
        if m.kind!=MachineKind::Paint {return None}
        match m.config.get(&"dots".into()){Some(Tag::ByteArray(v))=>decode(v),None=>Some(vec![]),_=>None}
    }
    fn ae2_paint_commit(&mut self,p:[i32;3],dots:Vec<[u8;2]>)->Result<(),String>{
        let key=(p[0],p[1],p[2]);
        if dots.is_empty(){self.world.block_entities.remove(&key);let air=self.world.air;self.world.set(p[0],p[1],p[2],air).map_err(|e|e.to_string())?;return Ok(())}
        let light=dots.iter().filter(|d|d[1]&128!=0).count().min(2);
        let id=self.world.block(p[0],p[1],p[2]);
        let mut state=self.pack.assets.registry.state(id).filter(|s|s.name=="ae2:paint").ok_or("paint state missing")?.clone();
        state.properties.insert("light_level".into(),light.to_string());
        let target=self.pack.assets.registry.resolve(&state).map_err(|e|e.to_string())?;
        self.world.set(p[0],p[1],p[2],target).map_err(|e|e.to_string())?;
        let Some(BlockEntity::Ae2(m))=self.world.block_entities.get_mut(&key) else {return Err("paint block entity missing".into())};
        m.config.insert("dots".into(),Tag::ByteArray(encode(&dots)));self.world.modified.insert((p[0]>>4,p[2]>>4));Ok(())
    }
    /// `side` points from the paint cell toward its supporting block.
    pub(super) fn ae2_add_paint(&mut self,p:[i32;3],side:[i32;3],hit:[f64;3],color:u8,lit:bool)->Result<(),String>{
        let side=side_index(side).ok_or("invalid side")?;
        if color>15 || hit.iter().any(|v|!v.is_finite()) {return Err("invalid paint data".into())}
        if !self.world.loaded(p[0],p[2]) || !self.ae2_paint_supported(p,side){return Err("paint support unavailable".into())}
        let state=self.world.block(p[0],p[1],p[2]);
        let name=self.pack.assets.registry.state(state).map(|s|s.name.as_str()).unwrap_or("");
        if name!="ae2:paint" && !self.res.facts().is_air(state){return Err("paint cell occupied".into())}
        if name!="ae2:paint" {let id=self.pack.assets.registry.default_state("ae2:paint").ok_or("paint unavailable")?;self.world.set(p[0],p[1],p[2],id).map_err(|e|e.to_string())?;self.world.block_entities.insert((p[0],p[1],p[2]),BlockEntity::Ae2(Box::new(Machine::new(MachineKind::Paint))));}
        let mut dots=self.ae2_paint_dots(p).ok_or("invalid paint storage")?;
        if dots.len()>20 {dots.remove(0);} dots.push(packed(side,hit,color,lit));self.ae2_paint_commit(p,dots)
    }
    pub(super) fn ae2_clean_paint_side(&mut self,p:[i32;3],side:[i32;3])->Result<(),String>{
        let side=side_index(side).ok_or("invalid side")?;let mut dots=self.ae2_paint_dots(p).ok_or("paint missing")?;dots.retain(|d|d[1]&7!=side);self.ae2_paint_commit(p,dots)
    }
    /// Refresh supports and rain in loaded chunks. Source uses neighbor and precipitation callbacks;
    /// this bounded periodic scan is the current host integration.
    pub(super) fn ae2_tick_paint(&mut self){
        if self.world.game_time.rem_euclid(20)!=0 {return}
        let positions:Vec<_>=self.world.block_entities.iter().filter_map(|(&p,e)|matches!(e,BlockEntity::Ae2(m) if m.kind==MachineKind::Paint).then_some([p.0,p.1,p.2])).collect();
        for p in positions {
            let rain=self.with_level(|b,l,_|b.is_raining_at(l,BlockPos::new(p[0],p[1],p[2])));
            let Some(mut dots)=self.ae2_paint_dots(p) else {continue};let old=dots.len();
            if rain {dots.clear()} else {let supported:[bool;6]=std::array::from_fn(|i|self.ae2_paint_supported(p,i as u8));dots.retain(|d|supported[(d[1]&7) as usize]);}
            if dots.len()!=old {let _=self.ae2_paint_commit(p,dots);}
        }
    }
}
#[cfg(test)] mod tests {use super::*;#[test]fn source_packing_roundtrips_and_rejects_invalid(){let d=packed(2,[0.5,1.,0.],3,true);assert_eq!(d,[0xf7,0x9a]);assert_eq!(decode(&encode(&[d])),Some(vec![d]));assert!(decode(&[1,0,7]).is_none());assert!(decode(&[22]).is_none());}}
