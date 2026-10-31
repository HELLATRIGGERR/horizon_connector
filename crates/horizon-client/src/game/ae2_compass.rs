//! AE2 Meteorite Compass target tracking, based on ServerCompassService 19.2.18.
//! Chunk records persist independently of loaded chunks; targets are Mysterious Cubes.
use super::{Game,lan::LanHost};
use horizon_client::{ae2_world::MachineKind,containers::BlockEntity};
use horizon_protocol::packets::{configuration::CustomPayload,play::Clientbound};
use serde::{Deserialize,Serialize};
use std::{collections::{BTreeMap,BTreeSet},path::PathBuf};
const CHANNEL:&str="ae2:compass_target";
const MAX_RANGE:i32=174;
#[derive(Clone,Debug,Serialize,Deserialize)]
struct Record {dim:String,x:i32,z:i32,positions:Vec<[i32;3]>}
#[derive(Default)]
pub(super) struct CompassIndex {
    root:PathBuf,
    chunks:BTreeMap<(String,i32,i32),Vec<[i32;3]>>,
    dirty:bool,
    pub target:Option<[i32;3]>,
    pub target_dim:String,
}
impl CompassIndex {
    fn load(&mut self,root:&std::path::Path) {
        if self.root==root {return;}
        self.root=root.to_owned();self.chunks.clear();self.dirty=false;
        if root.as_os_str().is_empty(){return;}
        if let Ok(bytes)=std::fs::read(root.join("ae2-compass-index.json"))
            && let Ok(records)=serde_json::from_slice::<Vec<Record>>(&bytes) {
            for record in records {if !record.positions.is_empty(){self.chunks.insert((record.dim,record.x,record.z),record.positions);}}
        }
    }
    fn save(&self)->std::io::Result<()> {
        if self.root.as_os_str().is_empty(){return Ok(());}
        let records:Vec<_>=self.chunks.iter().map(|((dim,x,z),positions)|Record{dim:dim.clone(),x:*x,z:*z,positions:positions.clone()}).collect();
        let data=serde_json::to_vec(&records).map_err(std::io::Error::other)?;
        let temporary=self.root.join("ae2-compass-index.json.tmp");
        std::fs::write(&temporary,data)?;std::fs::rename(temporary,self.root.join("ae2-compass-index.json"))
    }
    fn closest(&self,dim:&str,origin:(i32,i32),loaded:&BTreeSet<(i32,i32)>)->Option<[i32;3]> {
        let (cx,cz)=origin;
        let candidates:BTreeSet<_>=self.chunks.keys().filter_map(|(d,x,z)|(d==dim).then_some((*x,*z))).collect();
        if candidates.is_empty(){return None;}
        let present=|x,z|candidates.contains(&(x,z));
        let mut chosen=present(cx,cz).then_some((cx,cz));
        if chosen.is_none() {for offset in 1..MAX_RANGE {
            let (minx,maxx,minz,maxz)=(cx-offset,cx+offset,cz-offset,cz+offset);
            let mut best:i64=i64::MAX;let mut candidate=None;
            let mut visit=|x:i32,z:i32| {if present(x,z){let dx=i64::from(x)-i64::from(cx);let dz=i64::from(z)-i64::from(cz);let d=dx*dx+dz*dz;if d<best{best=d;candidate=Some((x,z));}}};
            for z in minz..=maxz {visit(minx,z);visit(maxx,z);}
            for x in minx+1..maxx {visit(x,minz);visit(x,maxz);}
            if candidate.is_some(){chosen=candidate;break;}
        }}
        let (x,z)=chosen?;
        let center=[x*16+8,0,z*16+8];
        if !loaded.contains(&(x,z)){return Some(center);}
        let origin=[cx*16+8,cz*16+8];
        self.chunks.get(&(dim.to_owned(),x,z))?.iter().min_by_key(|p|{
            let dx=i64::from(p[0])-i64::from(origin[0]);let dz=i64::from(p[2])-i64::from(origin[1]);dx*dx+dz*dz
        }).copied().map(|p|[p[0],0,p[2]]).or(Some(center))
    }
}
#[derive(Serialize,Deserialize)]
struct Target {dim:String,position:Option<[i32;3]>}
impl Game<'_> {
    pub(super) fn ae2_compass_angle(&self,owner:Option<&horizon_assets::item_model::OwnerState>,display:horizon_assets::item_model::DisplayContext,matrix:glam::Mat4)->f32 {
        use std::f64::consts::PI;
        let seconds=self.started.elapsed().as_secs_f64();
        let position=owner.map(|o|o.position);
        let target=(self.ae2_compass_index.target_dim==self.dim.id()).then_some(self.ae2_compass_index.target).flatten();
        let rotation=match (position,target) {
            (Some(p),Some(t)) if (p[0]-t[0] as f64).powi(2)+(p[2]-t[2] as f64).powi(2)>36.0 => (-(t[2] as f64-p[2])).atan2(t[0] as f64-p[0])-PI/2.0,
            (Some(_),None)=>(seconds%0.5)/0.5*2.0*PI,
            _=>(seconds%3.0)/3.0*2.0*PI,
        };
        let direction=matrix.transform_vector3(glam::Vec3::Z);
        let mut offset=(direction.z as f64).atan2(direction.x as f64)-PI/2.0;
        if display==horizon_assets::item_model::DisplayContext::Gui {offset+=owner.map_or(0.0,|o|f64::from(o.visual_rotation_y_degrees)*PI/180.0+PI);}
        (rotation+offset) as f32
    }
    pub(super) fn ae2_compass_status(&mut self,args:&[&str])->Result<serde_json::Value,String> {
        self.ae2_compass_refresh();
        let player=self.player.as_ref().ok_or("no player")?;
        let dim=self.dim.id().to_owned();
        let origin=match args {
            []=>((player.position.x.floor() as i32)>>4,(player.position.z.floor() as i32)>>4),
            [x,z]=>(x.parse::<i32>().map_err(|_|"invalid chunk x")?,z.parse::<i32>().map_err(|_|"invalid chunk z")?),
            _=>return Err("usage: ae2 compass status [chunk_x chunk_z]".into()),
        };
        if origin.0.unsigned_abs()>1_875_000 || origin.1.unsigned_abs()>1_875_000 {return Err("chunk outside world border".into());}
        let loaded:BTreeSet<_>=self.world.chunks.keys().copied().collect();
        let target=self.ae2_compass_index.closest(&dim,origin,&loaded);
        let mut cubes:Vec<_>=self.ae2_compass_index.chunks.iter().filter(|((d,x,z),_)|d==&dim&&loaded.contains(&(*x,*z))).flat_map(|(_,p)|p.iter().copied()).collect();
        cubes.sort_by_key(|p|{let x=i64::from(p[0])-player.position.x.floor() as i64;let z=i64::from(p[2])-player.position.z.floor() as i64;x*x+z*z});
        cubes.truncate(32);
        Ok(serde_json::json!({"dimension":dim,"chunk":origin,"target":target,"loaded_cubes":cubes,"tracked_chunks":self.ae2_compass_index.chunks.len()}))
    }
    /// Refresh loaded chunks without forcing any terrain generation or loads.
    pub(super) fn ae2_compass_refresh(&mut self) {
        self.ae2_compass_index.load(&self.world_root);
        let dim=self.dim.id().to_owned();
        let mut observed:BTreeMap<(i32,i32),Vec<[i32;3]>>=self.world.chunks.keys().map(|p|(*p,Vec::new())).collect();
        for (&(x,y,z),entity) in self.world.block_entities.iter() {
            if matches!(entity,BlockEntity::Ae2(m) if m.kind==MachineKind::MysteriousCube)
                && self.res.facts().get(self.world.block(x,y,z)).name.as_ref()=="ae2:mysterious_cube"
                && let Some(positions)=observed.get_mut(&(x>>4,z>>4)) {positions.push([x,y,z]);}
        }
        let mut changed=false;
        for ((x,z),mut positions) in observed {
            positions.sort_unstable();let key=(dim.clone(),x,z);
            if positions.is_empty(){changed|=self.ae2_compass_index.chunks.remove(&key).is_some();}
            else if self.ae2_compass_index.chunks.get(&key)!=Some(&positions){self.ae2_compass_index.chunks.insert(key,positions);changed=true;}
        }
        self.ae2_compass_index.dirty|=changed;
        if self.ae2_compass_index.dirty {
            match self.ae2_compass_index.save() {
                Ok(())=>self.ae2_compass_index.dirty=false,
                Err(error)=>eprintln!("AE2 compass index save failed: {error}"),
            }
        }
    }
    pub(super) fn ae2_compass_sync(&mut self,host:&LanHost) {
        if self.world.game_time.rem_euclid(100)!=0 {return;}
        self.ae2_compass_refresh();
        let dim=self.dim.id().to_owned();let loaded:BTreeSet<_>=self.world.chunks.keys().copied().collect();
        for guest in &host.hub.guests {
            if guest.dimension!=dim || !guest.loaded {continue;}
            let origin=((guest.position[0].floor() as i32)>>4,(guest.position[2].floor() as i32)>>4);
            let target=Target{dim:dim.clone(),position:self.ae2_compass_index.closest(&dim,origin,&loaded)};
            if let Ok(data)=serde_json::to_vec(&target){guest.send(&Clientbound::CustomPayload(CustomPayload{channel:CHANNEL.into(),data}));}
        }
    }
    pub(super) fn ae2_compass_tick_local(&mut self) {
        if self.lan.is_some() || self.world.game_time.rem_euclid(100)!=0{return;}
        self.ae2_compass_refresh();
        let Some(player)=self.player.as_ref() else{return};
        let dim=self.dim.id().to_owned();let loaded=self.world.chunks.keys().copied().collect();
        let origin=((player.position.x.floor() as i32)>>4,(player.position.z.floor() as i32)>>4);
        self.ae2_compass_index.target=self.ae2_compass_index.closest(&dim,origin,&loaded);
        self.ae2_compass_index.target_dim=dim;
    }
    pub(super) fn ae2_accept_compass(&mut self,data:&[u8]) {
        if data.len()>512{return;}
        if let Ok(target)=serde_json::from_slice::<Target>(data) && target.dim==self.dim.id() {
            self.ae2_compass_index.target=target.position;self.ae2_compass_index.target_dim=target.dim;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spiral_respects_source_range_ties_and_unloaded_chunk_centers() {
        let mut index=CompassIndex::default();let d="minecraft:overworld".to_string();
        index.chunks.insert((d.clone(),-1,-1),vec![[-15,40,-14]]);
        index.chunks.insert((d.clone(),1,-1),vec![[17,60,-15]]);
        assert_eq!(index.closest(&d,(0,0),&BTreeSet::new()),Some([-8,0,-8]));
        assert_eq!(index.closest(&d,(0,0),&BTreeSet::from([(-1,-1)])),Some([-15,0,-14]));
        index.chunks.clear();index.chunks.insert((d.clone(),174,0),vec![[2784,60,0]]);
        assert_eq!(index.closest(&d,(0,0),&BTreeSet::new()),None);
        assert_eq!(index.closest("minecraft:the_nether",(0,0),&BTreeSet::new()),None);
    }
}
