//! Ordered StatisticsService membership for actual Spatial Anchor owners.
use std::collections::BTreeMap;
use ae2_links_core::anchor_grid::{ChunkObserver, Kind};
use ae2_network_core::{NodeIdentity, service_events::MembershipChange};
use horizon_client::{
    ae2_world::MachineKind,
    block_entities::{Ae2Mutation,Ae2MutationKind},
    containers::BlockEntity,
};
use horizon_world::nbt::Tag;
use super::{ae2_event_live::dimension, ae2_global::GridAddress};

#[derive(Clone,Copy,Debug)]
struct Physical {address:GridAddress}
#[derive(Clone,Copy,Debug)]
pub(super) enum AnchorCallback {
    Chunk {owner:NodeIdentity,address:GridAddress,chunk:(i32,i32),kind:Kind},
    Removed {owner:NodeIdentity,address:GridAddress},
}
#[derive(Clone,Debug,Default)]
pub(super) struct AnchorObserver {
    grids:ChunkObserver<NodeIdentity>,
    physical:BTreeMap<NodeIdentity,Physical>,
    pending:Vec<AnchorCallback>,
    failure:Option<String>,
}
impl AnchorObserver {
    fn insert(&mut self,id:NodeIdentity,address:GridAddress,in_world:bool,anchor:bool)->Result<(),String>{
        if self.physical.contains_key(&id){return Err("duplicate anchor-grid node".into())}
        let (dim,pos)=address.local_parts().ok_or("invalid anchor-grid position")?;
        self.grids.register(id,super::ae2_event_live::ordinal(dim),(pos.0>>4,pos.2>>4),in_world,anchor).map_err(str::to_owned)?;
        self.physical.insert(id,Physical{address});Ok(())
    }
    fn insert_part(&mut self,host:u64,address:GridAddress,part:&Tag)->Result<(),String>{
        let Some(id)=super::ae2_channels::part_identity(part) else{return Ok(())};
        let Some(Tag::String(kind))=part.get("id") else{return Err("anchor-grid part kind unavailable".into())};
        let kind=kind.to_string_lossy();
        let node=NodeIdentity{host,part:Some(id),outer:false};
        // AEBasePart main nodes are ordinary GridNodes, not InWorldGridNode.
        self.insert(node,address,false,false)?;
        if matches!(kind.as_str(),"ae2:quartz_fiber"|"ae2:toggle_bus"|"ae2:inverted_toggle_bus"|"ae2:me_p2p_tunnel") {
            self.insert(NodeIdentity{outer:true,..node},address,true,false)?;
        }
        Ok(())
    }
    pub fn before(&mut self,dim:i32,event:&Ae2Mutation){
        if self.failure.is_some(){return}
        let result=(||->Result<(),String>{
            let address=GridAddress::from_local(dimension(dim).ok_or("unknown anchor-grid dimension")?,event.position).ok_or("invalid anchor-grid position")?;
            match &event.kind {
                Ae2MutationKind::EntityAdded{incarnation,entity:BlockEntity::Ae2(machine),..}=>{
                    let id=NodeIdentity{host:*incarnation,part:None,outer:false};
                    // Networked blocks and the cable center are in-world.
                    self.insert(id,address,true,machine.kind==MachineKind::SpatialAnchor)?;
                    if machine.kind==MachineKind::DebugPhantomNode {self.insert(NodeIdentity::phantom_proxy(*incarnation),address,true,false)?;}
                    if let Some(Tag::List(parts))=machine.config.get(&"parts".into()){for part in parts{self.insert_part(*incarnation,address,part)?;}}
                }
                Ae2MutationKind::PartAdded{incarnation,part}=>self.insert_part(*incarnation,address,part)?,
                Ae2MutationKind::PartChanged{incarnation,before,after}=>{
                    if super::ae2_channels::part_identity(before).is_none()&&super::ae2_channels::part_identity(after).is_some(){self.insert_part(*incarnation,address,after)?;}
                }
                _=>{}
            }
            Ok(())
        })();if let Err(error)=result{self.failure=Some(error);}
    }
    pub fn membership(&mut self,change:Result<MembershipChange,String>){
        if self.failure.is_some(){return}
        let result=change.and_then(|change|{
            let events=self.grids.membership(change.node,change.from,change.to).map_err(str::to_owned)?;
            if self.pending.len().checked_add(events.len()).is_none_or(|n|n>100_000){return Err("anchor-grid callback backlog exceeded its bound".into())}
            for event in events {
                let address=self.physical.get(&event.anchor).ok_or("anchor owner identity unavailable")?.address;
                self.pending.push(AnchorCallback::Chunk{owner:event.anchor,address,chunk:event.chunk,kind:event.kind});
            }
            Ok(())
        });if let Err(error)=result{self.failure=Some(error);}
    }
    pub fn after(&mut self,dim:i32,event:&Ae2Mutation){
        if self.failure.is_some(){return}
        let result=(||->Result<(),String>{
            let address=GridAddress::from_local(dimension(dim).ok_or("unknown anchor-grid dimension")?,event.position).ok_or("invalid anchor-grid position")?;
            let ids:Vec<_>=match &event.kind{
                Ae2MutationKind::EntityRemoved{incarnation,entity,bulk_unload}=>{
                    if !*bulk_unload && matches!(entity,BlockEntity::Ae2(machine) if machine.kind==MachineKind::SpatialAnchor) {
                        if self.pending.len()>=100_000{return Err("anchor-grid callback backlog exceeded its bound".into())}
                        self.pending.push(AnchorCallback::Removed{owner:NodeIdentity{host:*incarnation,part:None,outer:false},address});
                    }
                    let phantom=matches!(entity,BlockEntity::Ae2(machine) if machine.kind==MachineKind::DebugPhantomNode);
                    self.physical.iter().filter_map(|(id,p)|{
                        let retained_proxy=phantom&&id.is_phantom_proxy()&&self.grids.service_of(*id).is_some();
                        (id.host==*incarnation&&p.address==address&&!retained_proxy).then_some(*id)
                    }).collect()
                }
                Ae2MutationKind::PartRemoved{incarnation,part}=>{
                    let Some(uuid)=super::ae2_channels::part_identity(part) else{return Ok(())};
                    self.physical.iter().filter_map(|(id,p)|(id.host==*incarnation&&id.part==Some(uuid)&&p.address==address).then_some(*id)).collect()
                }
                _=>Vec::new()
            };
            for id in ids {self.grids.forget(id).map_err(str::to_owned)?;self.physical.remove(&id);}
            Ok(())
        })();if let Err(error)=result{self.failure=Some(error);}
    }
    pub fn take_callbacks(&mut self)->Result<Vec<AnchorCallback>,String>{
        if let Some(error)=&self.failure{return Err(error.clone())}
        Ok(std::mem::take(&mut self.pending))
    }
    pub fn chunks(&self,id:NodeIdentity,dim:i32)->Result<Option<std::collections::BTreeSet<(i32,i32)>>,String>{
        if let Some(error)=&self.failure{return Err(error.clone())}
        Ok(self.grids.service_of(id).map(|service|self.grids.chunks(service,dim)))
    }
}
