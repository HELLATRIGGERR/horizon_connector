//! Event-driven Crafting CPU formation and per-service publication.
//!
//! The host supplies actual resident block-entity additions/removals, managed
//! node membership callbacks, `onReady`, and neighbor callbacks in their
//! observed order. This model never invents those callbacks from a sorted
//! world scan. The rectangular calculator and reversed member iteration follow
//! AE2 19.2.18 `MBCalculator` / `CraftingCPUCalculator` / `CraftingCPUCluster`.
use std::collections::{BTreeSet, HashMap, HashSet};
use std::hash::Hash;

// These are unprocessed formation/break callbacks, not diagnostic history.
// Exceeding either bound poisons the transaction instead of discarding work.
const MAX_PENDING_EVENTS:usize=1024;
const MAX_PENDING_MEMBERS:usize=32_768;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CpuPos { pub dimension: i32, pub x: i32, pub y: i32, pub z: i32 }
impl CpuPos {
    fn offset(self, dx:i32, dy:i32, dz:i32) -> Result<Self, FormationError> {
        Ok(Self { dimension:self.dimension, x:self.x.checked_add(dx).ok_or(FormationError::CoordinateOverflow)?,
            y:self.y.checked_add(dy).ok_or(FormationError::CoordinateOverflow)?,
            z:self.z.checked_add(dz).ok_or(FormationError::CoordinateOverflow)? })
    }
    fn within(self, min:Self, max:Self) -> bool {
        self.dimension==min.dimension && self.x>=min.x && self.x<=max.x
            && self.y>=min.y && self.y<=max.y && self.z>=min.z && self.z<=max.z
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormationError {
    DuplicateBlock, UnknownBlock, UnexpectedService, StillRegistered, DanglingCluster,
    CoordinateOverflow, CapacityOverflow, ClusterIdExhausted, EventOverflow, Unavailable(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CpuCluster<I> {
    pub id:u64, pub min:CpuPos, pub max:CpuPos,
    /// Source `addBlockEntity` inserts at the front while visiting the region.
    pub members:Vec<I>, pub core:I, pub storage:u64, pub coprocessors:u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishedCpu<I> {
    pub service:u64, pub cluster:u64, pub core:I, pub storage:u64, pub coprocessors:u32,
    pub members:Vec<(I,CpuPos,bool)>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormationEvent<I> {
    Formed(CpuCluster<I>),
    /// The host must reconcile the real cluster/job custody on this callback.
    Broken(CpuCluster<I>),
}
#[derive(Clone, Debug)]
struct CpuBlock {
    position:CpuPos, storage:u64, coprocessors:u32, core:bool,
    service:Option<u64>, cluster:Option<u64>, resident:bool,
}
#[derive(Clone, Debug)]
pub struct CpuFormation<I> {
    blocks:HashMap<I,CpuBlock>, positions:HashMap<CpuPos,I>, clusters:HashMap<u64,CpuCluster<I>>,
    dirty:HashSet<u64>, published:HashMap<u64,BTreeSet<u64>>,
    events:Vec<FormationEvent<I>>, event_members:usize, next_cluster:u64, failure:Option<FormationError>,
}
impl<I> Default for CpuFormation<I> {
    fn default()->Self {Self {blocks:HashMap::new(),positions:HashMap::new(),clusters:HashMap::new(),
        dirty:HashSet::new(),published:HashMap::new(),events:Vec::new(),event_members:0,next_cluster:0,failure:None}}
}
impl<I:Clone+Eq+Hash> CpuFormation<I> {
    pub fn invalidate(&mut self,reason:String) {if self.failure.is_none(){self.failure=Some(FormationError::Unavailable(reason));}}
    /// Only resident entities receive new node and neighbor callbacks. A
    /// cluster may also retain an unloaded member as a source-style tombstone.
    pub fn contains(&self,id:&I)->bool {self.blocks.get(id).is_some_and(|block|block.resident)}
    pub fn formed(&self,id:&I)->Result<bool,FormationError> {
        if let Some(error)=&self.failure{return Err(error.clone());}
        Ok(self.blocks.get(id).ok_or(FormationError::UnknownBlock)?.cluster.is_some())
    }
    pub fn service_of(&self,id:&I)->Result<Option<u64>,FormationError> {
        if let Some(error)=&self.failure{return Err(error.clone());}
        Ok(self.blocks.get(id).ok_or(FormationError::UnknownBlock)?.service)
    }
    pub fn position_of(&self,id:&I)->Result<CpuPos,FormationError> {
        if let Some(error)=&self.failure{return Err(error.clone());}
        self.blocks.get(id).map(|block|block.position).ok_or(FormationError::UnknownBlock)
    }
    /// Read only actual resident, initialized nodes. The cluster's source
    /// core is a stable grouping key; this snapshot does not order callbacks.
    pub fn ready_network_nodes(&self)->Result<Vec<(I,CpuPos,Option<CpuPos>)>,FormationError> {
        if let Some(error)=&self.failure{return Err(error.clone());}
        self.blocks.iter().filter(|(_,block)|block.resident && block.service.is_some()).map(|(id,block)| {
            let core=match block.cluster {
                None=>None,
                Some(cluster)=>{
                    let cluster=self.clusters.get(&cluster).ok_or(FormationError::DanglingCluster)?;
                    Some(self.blocks.get(&cluster.core).ok_or(FormationError::UnknownBlock)?.position)
                }
            };
            Ok((id.clone(),block.position,core))
        }).collect()
    }
    /// Independent services publish their own CPU lists at the end-tick
    /// boundary; their relative ordering is not used to order any callbacks.
    pub fn dirty_services(&self)->Result<Vec<u64>,FormationError> {
        if let Some(error)=&self.failure{return Err(error.clone());}
        Ok(self.dirty.iter().copied().collect())
    }
    fn transaction<T>(&mut self,change:impl FnOnce(&mut Self)->Result<T,FormationError>)->Result<T,FormationError> {
        if let Some(error)=&self.failure{return Err(error.clone());}
        let mut next=self.clone();
        match change(&mut next) {Ok(value)=>{*self=next;Ok(value)},Err(error)=>{self.failure=Some(error.clone());Err(error)}}
    }
    /// Register an actually resident CPU block entity before its node exists.
    pub fn add_block(&mut self,id:I,position:CpuPos,storage:u64,coprocessors:u32,core:bool)->Result<(),FormationError> {
        self.transaction(|next| {
            if next.blocks.contains_key(&id) || next.positions.contains_key(&position) {return Err(FormationError::DuplicateBlock);}
            next.positions.insert(position,id.clone());
            next.blocks.insert(id,CpuBlock {position,storage,coprocessors,core,service:None,cluster:None,resident:true});Ok(())
        })
    }
    /// `CraftingService.addNode/removeNode` requests CPU-list refresh on each
    /// actual node transition, even if the block has not formed yet.
    pub fn membership(&mut self,id:&I,from:Option<u64>,to:Option<u64>)->Result<(),FormationError> {
        self.transaction(|next| {
            let block=next.blocks.get_mut(id).ok_or(FormationError::UnknownBlock)?;
            if block.service!=from {return Err(FormationError::UnexpectedService);}
            block.service=to;
            if from!=to {if let Some(s)=from{next.dirty.insert(s);}if let Some(s)=to{next.dirty.insert(s);}}
            Ok(())
        })
    }
    /// A callback runs at each actual `updateStatus` point. The host stages
    /// its topology along with this model and forwards membership changes to
    /// `membership` before returning from the callback.
    pub fn ready(&mut self,id:&I)->Result<(),FormationError> {
        self.ready_with(id,&mut |_,_,_|Ok(()))
    }
    pub fn ready_with<F>(&mut self,id:&I,face:&mut F)->Result<(),FormationError>
    where F:FnMut(&mut Self,&I,bool)->Result<(),FormationError> {
        self.transaction(|next| {
            if next.blocks.get(id).ok_or(FormationError::UnknownBlock)?.service.is_none() {return Err(FormationError::UnexpectedService);}
            next.calculate_with(id,face)
        })
    }
    pub fn neighbor(&mut self,id:&I,changed:CpuPos)->Result<(),FormationError> {
        self.neighbor_with(id,changed,&mut |_,_,_|Ok(()))
    }
    pub fn neighbor_with<F>(&mut self,id:&I,changed:CpuPos,face:&mut F)->Result<(),FormationError>
    where F:FnMut(&mut Self,&I,bool)->Result<(),FormationError> {
        self.transaction(|next| {
            let block=next.blocks.get(id).ok_or(FormationError::UnknownBlock)?;
            if changed.dimension!=block.position.dimension {return Err(FormationError::UnknownBlock);}
            let recheck=match block.cluster {
                None=>true,
                Some(cluster)=>{
                    let cluster=next.clusters.get(&cluster).ok_or(FormationError::DanglingCluster)?;
                    changed.within(cluster.min,cluster.max) || next.positions.contains_key(&changed)
                }
            };
            if recheck {next.calculate_with(id,face)?;}Ok(())
        })
    }
    pub fn neighbor_unlocated(&mut self,id:&I)->Result<(),FormationError> {
        self.neighbor_unlocated_with(id,&mut |_,_,_|Ok(()))
    }
    pub fn neighbor_unlocated_with<F>(&mut self,id:&I,face:&mut F)->Result<(),FormationError>
    where F:FnMut(&mut Self,&I,bool)->Result<(),FormationError> {
        self.transaction(|next| {
            if next.blocks.get(id).ok_or(FormationError::UnknownBlock)?.cluster.is_some() {
                return Err(FormationError::Unavailable("formed CPU neighbor source position unavailable".into()));
            }
            next.calculate_with(id,face)
        })
    }
    pub fn before_remove(&mut self,id:&I)->Result<(),FormationError> {
        self.before_remove_with(id,&mut |_,_,_|Ok(()))
    }
    pub fn before_remove_with<F>(&mut self,id:&I,face:&mut F)->Result<(),FormationError>
    where F:FnMut(&mut Self,&I,bool)->Result<(),FormationError> {
        self.transaction(|next| {
            let cluster=next.blocks.get(id).ok_or(FormationError::UnknownBlock)?.cluster;
            if let Some(cluster)=cluster {next.break_cluster_with(cluster,face)?;}Ok(())
        })
    }
    pub fn after_remove(&mut self,id:&I)->Result<(),FormationError> {
        self.transaction(|next| {
            let block=next.blocks.get(id).ok_or(FormationError::UnknownBlock)?;
            if block.service.is_some() || block.cluster.is_some() {return Err(FormationError::StillRegistered);}
            let position=block.position;next.blocks.remove(id);next.positions.remove(&position);Ok(())
        })
    }
    /// `onChunkUnloaded` destroys the node but does not call the CPU block's
    /// `onRemove`/`breakCluster`. A surviving cluster keeps its old member
    /// object until a later real calculator callback replaces it.
    pub fn after_unload(&mut self,id:&I)->Result<(),FormationError> {
        self.transaction(|next| {
            let block=next.blocks.get_mut(id).ok_or(FormationError::UnknownBlock)?;
            if block.service.is_some() || !block.resident {return Err(FormationError::StillRegistered);}
            let position=block.position;let cluster=block.cluster;block.resident=false;
            if next.positions.remove(&position).as_ref()!=Some(id) {return Err(FormationError::UnknownBlock);}
            if let Some(cluster)=cluster {
                let members=next.clusters.get(&cluster).ok_or(FormationError::DanglingCluster)?.members.clone();
                if members.iter().all(|member|next.blocks.get(member).is_some_and(|block|!block.resident)) {
                    next.clusters.remove(&cluster);
                    for member in members {next.blocks.remove(&member);}
                }
            } else {next.blocks.remove(id);}
            Ok(())
        })
    }
    fn mark_first_service(&mut self,cluster:&CpuCluster<I>) {
        for id in &cluster.members {
            if let Some(service)=self.blocks.get(id).and_then(|block|block.service) {self.dirty.insert(service);break;}
        }
    }
    fn push_event(&mut self,event:FormationEvent<I>)->Result<(),FormationError> {
        let members=match &event {FormationEvent::Formed(cluster)|FormationEvent::Broken(cluster)=>cluster.members.len()};
        let total=self.event_members.checked_add(members).ok_or(FormationError::EventOverflow)?;
        if self.events.len()>=MAX_PENDING_EVENTS || total>MAX_PENDING_MEMBERS{return Err(FormationError::EventOverflow);}
        self.events.push(event);self.event_members=total;Ok(())
    }
    fn break_cluster_with<F>(&mut self,id:u64,face:&mut F)->Result<(),FormationError>
    where F:FnMut(&mut Self,&I,bool)->Result<(),FormationError> {
        let cluster=self.clusters.remove(&id).ok_or(FormationError::DanglingCluster)?;
        self.mark_first_service(&cluster);
        for member in &cluster.members {
            let block=self.blocks.get_mut(member).ok_or(FormationError::UnknownBlock)?;
            if block.cluster!=Some(id) {return Err(FormationError::DanglingCluster);}
            block.cluster=None;
            face(self,member,false)?;
        }
        for member in &cluster.members {
            if self.blocks.get(member).is_some_and(|block|!block.resident) {self.blocks.remove(member);}
        }
        self.push_event(FormationEvent::Broken(cluster))
    }
    fn adjacent_face_has_block(&self,min:CpuPos,max:CpuPos,axis:usize,positive:bool)->Result<bool,FormationError> {
        let mut lo=min;let mut hi=max;
        let delta=if positive{1}else{-1};
        match axis {
            0=>{let edge=if positive{max.offset(delta,0,0)?.x}else{min.offset(delta,0,0)?.x};lo.x=edge;hi.x=edge;},
            1=>{let edge=if positive{max.offset(0,delta,0)?.y}else{min.offset(0,delta,0)?.y};lo.y=edge;hi.y=edge;},
            _=>{let edge=if positive{max.offset(0,0,delta)?.z}else{min.offset(0,0,delta)?.z};lo.z=edge;hi.z=edge;},
        }
        for z in lo.z..=hi.z {for y in lo.y..=hi.y {for x in lo.x..=hi.x {
            if self.positions.contains_key(&CpuPos{dimension:min.dimension,x,y,z}) {return Ok(true);}
        }}}
        Ok(false)
    }
    fn calculate_with<F>(&mut self,id:&I,face:&mut F)->Result<(),FormationError>
    where F:FnMut(&mut Self,&I,bool)->Result<(),FormationError> {
        let target=self.blocks.get(id).ok_or(FormationError::UnknownBlock)?.clone();
        let mut min=target.position;let mut max=target.position;
        while self.positions.contains_key(&min.offset(-1,0,0)?) {min=min.offset(-1,0,0)?;}
        while self.positions.contains_key(&min.offset(0,-1,0)?) {min=min.offset(0,-1,0)?;}
        while self.positions.contains_key(&min.offset(0,0,-1)?) {min=min.offset(0,0,-1)?;}
        while self.positions.contains_key(&max.offset(1,0,0)?) {max=max.offset(1,0,0)?;}
        while self.positions.contains_key(&max.offset(0,1,0)?) {max=max.offset(0,1,0)?;}
        while self.positions.contains_key(&max.offset(0,0,1)?) {max=max.offset(0,0,1)?;}
        let scale_ok=[i64::from(max.x)-i64::from(min.x),i64::from(max.y)-i64::from(min.y),i64::from(max.z)-i64::from(min.z)].into_iter().all(|n|n<=16);
        let mut valid=scale_ok;
        if valid {for (axis,positive) in [(1,false),(1,true),(2,true),(2,false),(0,false),(0,true)] {
            if self.adjacent_face_has_block(min,max,axis,positive)? {valid=false;break;}
        }}
        let mut traversal=Vec::new();let mut storage=0u64;let mut coprocessors=0u32;
        if valid {
            'outer: for z in min.z..=max.z {for y in min.y..=max.y {for x in min.x..=max.x {
                let position=CpuPos{dimension:min.dimension,x,y,z};
                let Some(member)=self.positions.get(&position) else {valid=false;break 'outer;};
                let block=self.blocks.get(member).ok_or(FormationError::UnknownBlock)?;
                storage=storage.checked_add(block.storage).ok_or(FormationError::CapacityOverflow)?;
                coprocessors=coprocessors.checked_add(block.coprocessors).ok_or(FormationError::CapacityOverflow)?;
                traversal.push(member.clone());
            }}}
            if storage==0 {valid=false;}
        }
        if !valid {if let Some(cluster)=target.cluster {self.break_cluster_with(cluster,face)?;}return Ok(());}
        if let Some(cluster)=target.cluster {
            let current=self.clusters.get(&cluster).ok_or(FormationError::DanglingCluster)?;
            if current.min==min && current.max==max {return Ok(());}
        }
        let id_new=self.next_cluster;self.next_cluster=self.next_cluster.checked_add(1).ok_or(FormationError::ClusterIdExhausted)?;
        let mut core:Option<I>=None;
        for member in &traversal {
            let old=self.blocks.get(member).ok_or(FormationError::UnknownBlock)?.cluster;
            if let Some(old)=old {self.break_cluster_with(old,face)?;}
            let block=self.blocks.get_mut(member).ok_or(FormationError::UnknownBlock)?;
            if core.is_none() || block.core {core=Some(member.clone());}
            block.core=false;block.cluster=Some(id_new);
            face(self,member,true)?;
        }
        let core=core.ok_or(FormationError::UnknownBlock)?;
        self.blocks.get_mut(&core).ok_or(FormationError::UnknownBlock)?.core=true;
        traversal.reverse();
        let cluster=CpuCluster{id:id_new,min,max,members:traversal,core,storage,coprocessors};
        self.mark_first_service(&cluster);self.clusters.insert(id_new,cluster.clone());self.push_event(FormationEvent::Formed(cluster))
    }
    /// Source crafting-service end tick refreshes its cluster set after node
    /// and `GridCraftingCpuChange` callbacks. Until then the old set remains.
    pub fn service_tick(&mut self,service:u64)->Result<(),FormationError> {
        self.transaction(|next| {
            if next.dirty.remove(&service) {
                let clusters=next.blocks.values().filter(|block|block.service==Some(service)).filter_map(|block|block.cluster).collect();
                next.published.insert(service,clusters);
            }
            Ok(())
        })
    }
    pub fn published(&self,service:u64)->Result<Vec<u64>,FormationError> {
        if let Some(error)=&self.failure{return Err(error.clone());}
        Ok(self.published.get(&service).map_or_else(Vec::new,|set|set.iter().copied().collect()))
    }
    /// Snapshot the clusters published by actual service membership at the
    /// last end tick. Resident flags retain partial-unload uncertainty.
    pub fn published_clusters(&self)->Result<Vec<PublishedCpu<I>>,FormationError> {
        if let Some(error)=&self.failure{return Err(error.clone());}
        let mut result=Vec::new();
        for (service,ids) in &self.published {for id in ids {
            let cluster=self.clusters.get(id).ok_or(FormationError::DanglingCluster)?;
            let members=cluster.members.iter().map(|member| {
                let block=self.blocks.get(member).ok_or(FormationError::UnknownBlock)?;
                Ok((member.clone(),block.position,block.resident))
            }).collect::<Result<Vec<_>,FormationError>>()?;
            result.push(PublishedCpu {service:*service,cluster:*id,core:cluster.core.clone(),storage:cluster.storage,
                coprocessors:cluster.coprocessors,members});
        }}
        Ok(result)
    }
    pub fn cluster_of(&self,id:&I)->Result<Option<&CpuCluster<I>>,FormationError> {
        if let Some(error)=&self.failure{return Err(error.clone());}
        let block=self.blocks.get(id).ok_or(FormationError::UnknownBlock)?;
        block.cluster.map(|id|self.clusters.get(&id).ok_or(FormationError::DanglingCluster)).transpose()
    }
    pub fn take_events(&mut self)->Result<Vec<FormationEvent<I>>,FormationError> {
        if let Some(error)=&self.failure{return Err(error.clone());}
        self.event_members=0;Ok(std::mem::take(&mut self.events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(x:i32,y:i32,z:i32)->CpuPos {CpuPos{dimension:0,x,y,z}}
    #[test]
    fn cpu_faces_follow_each_source_status_callback_and_membership() {
        let mut c=CpuFormation::default();
        c.add_block(1,p(0,0,0),0,0,false).unwrap();
        c.membership(&1,None,Some(1)).unwrap();c.ready(&1).unwrap();
        assert!(!c.formed(&1).unwrap());
        c.add_block(2,p(1,0,0),1024,0,false).unwrap();
        let changes=std::cell::RefCell::new(Vec::new());
        let mut face=|cpu:&mut CpuFormation<u8>,id:&u8,formed:bool| {
            changes.borrow_mut().push((*id,formed));
            if let Some(from)=cpu.service_of(id)? {
                let to=if formed {2} else {if *id==1 {1}else{3}};
                cpu.membership(id,Some(from),Some(to))?;
            }
            Ok(())
        };
        c.neighbor_with(&1,p(1,0,0),&mut face).unwrap();
        assert_eq!(*changes.borrow(),vec![(1,true),(2,true)]);
        assert!(c.formed(&2).unwrap());assert_eq!(c.service_of(&1).unwrap(),Some(2));
        assert_eq!(c.ready_network_nodes().unwrap(),vec![(1,p(0,0,0),Some(p(0,0,0)))]);
        c.membership(&2,None,Some(2)).unwrap();c.ready_with(&2,&mut face).unwrap();
        assert_eq!(c.ready_network_nodes().unwrap().len(),2);
        assert_eq!(*changes.borrow(),vec![(1,true),(2,true)]); // unchanged bounds
        let cluster=c.cluster_of(&1).unwrap().unwrap().id;
        c.service_tick(2).unwrap();assert_eq!(c.published(2).unwrap(),vec![cluster]);
        c.before_remove_with(&2,&mut face).unwrap();
        assert_eq!(*changes.borrow(),vec![(1,true),(2,true),(2,false),(1,false)]);
        assert_eq!(c.service_of(&1).unwrap(),Some(1));assert_eq!(c.service_of(&2).unwrap(),Some(3));
        c.service_tick(2).unwrap();assert!(c.published(2).unwrap().is_empty());
    }
    #[test]
    fn cpu_formation_backlog_preserves_semantic_events_and_fails_closed() {
        let mut c=CpuFormation::default();
        c.add_block(1,p(0,0,0),1024,0,false).unwrap();
        c.membership(&1,None,Some(0)).unwrap();c.ready(&1).unwrap();
        let saved=c.events[0].clone();
        while c.events.len()<MAX_PENDING_EVENTS {c.push_event(saved.clone()).unwrap();}
        c.add_block(2,p(1,0,0),0,1,false).unwrap();
        assert_eq!(c.neighbor(&1,p(1,0,0)),Err(FormationError::EventOverflow));
        assert_eq!(c.events.len(),MAX_PENDING_EVENTS);
        assert_eq!(c.take_events(),Err(FormationError::EventOverflow));
        assert!(matches!(c.cluster_of(&1),Err(FormationError::EventOverflow)));

        let mut large=CpuFormation::<u8>::default();
        let cluster=CpuCluster{id:1,min:p(0,0,0),max:p(16,16,16),members:vec![1;17*17*17],core:1,storage:1024,coprocessors:0};
        let event=FormationEvent::Formed(cluster);
        for _ in 0..6 {large.push_event(event.clone()).unwrap();}
        assert_eq!(large.push_event(event.clone()),Err(FormationError::EventOverflow));
        assert_eq!(large.events.len(),6);assert_eq!(large.event_members,6*17*17*17);
        assert_eq!(large.take_events().unwrap().len(),6);
        assert_eq!(large.event_members,0);large.push_event(event).unwrap();
    }
    // Actual AE2 block entities + service callbacks recorded in
    // evidence/java-ae2-cpu-formation-oracle; this is not world-order proof.
    #[test]
    fn oracle_ready_neighbor_and_service_tick_keep_partial_members_visible() {
        let mut c=CpuFormation::default();
        c.add_block("a",p(1,120,1),1024,0,false).unwrap();
        c.add_block("b",p(2,120,1),0,1,false).unwrap();
        c.membership(&"a",None,Some(7)).unwrap();c.ready(&"a").unwrap();
        let first=c.cluster_of(&"a").unwrap().unwrap().clone();
        assert_eq!(first.members,vec!["b","a"]);assert_eq!(first.storage,1024);assert_eq!(first.coprocessors,1);
        assert_eq!(c.cluster_of(&"b").unwrap().unwrap().id,first.id);
        assert!(c.published(7).unwrap().is_empty());c.service_tick(7).unwrap();assert_eq!(c.published(7).unwrap(),vec![first.id]);
        c.membership(&"b",None,Some(7)).unwrap();c.ready(&"b").unwrap();assert_eq!(c.cluster_of(&"b").unwrap().unwrap().id,first.id);
        c.add_block("c",p(3,120,1),4096,0,false).unwrap();c.neighbor(&"b",p(3,120,1)).unwrap();
        let expanded=c.cluster_of(&"c").unwrap().unwrap().clone();assert_ne!(expanded.id,first.id);
        assert_eq!(expanded.members,vec!["c","b","a"]);assert_eq!(expanded.storage,5120);
        assert_eq!(c.published(7).unwrap(),vec![first.id]);c.service_tick(7).unwrap();assert_eq!(c.published(7).unwrap(),vec![expanded.id]);
        c.add_block("d",p(2,121,1),0,0,false).unwrap();c.neighbor(&"b",p(2,121,1)).unwrap();
        assert!(c.cluster_of(&"a").unwrap().is_none());c.service_tick(7).unwrap();assert!(c.published(7).unwrap().is_empty());
        c.before_remove(&"d").unwrap();c.after_remove(&"d").unwrap();c.neighbor(&"b",p(2,121,1)).unwrap();
        let restored=c.cluster_of(&"b").unwrap().unwrap().id;assert_ne!(restored,expanded.id);
        c.service_tick(7).unwrap();assert_eq!(c.published(7).unwrap(),vec![restored]);
        let events=c.take_events().unwrap();assert!(matches!(&events[0],FormationEvent::Formed(v) if v.id==first.id));
        assert!(events.iter().any(|event|matches!(event,FormationEvent::Broken(v) if v.id==expanded.id)));
    }
    #[test]
    fn source_region_order_core_and_service_migration_are_explicit() {
        let mut c=CpuFormation::default();
        for (id,pos,core) in [("a",p(0,0,0),false),("b",p(1,0,0),false),("c",p(0,1,0),true),("d",p(1,1,0),false)] {
            c.add_block(id,pos,if id=="a"{1024}else{0},0,core).unwrap();
        }
        c.membership(&"a",None,Some(1)).unwrap();c.ready(&"a").unwrap();
        let cluster=c.cluster_of(&"a").unwrap().unwrap().clone();
        assert_eq!(cluster.members,vec!["d","c","b","a"]);assert_eq!(cluster.core,"c");
        c.service_tick(1).unwrap();assert_eq!(c.published(1).unwrap(),vec![cluster.id]);
        c.membership(&"a",Some(1),Some(2)).unwrap();assert_eq!(c.published(1).unwrap(),vec![cluster.id]);
        c.service_tick(1).unwrap();c.service_tick(2).unwrap();assert!(c.published(1).unwrap().is_empty());assert_eq!(c.published(2).unwrap(),vec![cluster.id]);
        c.before_remove(&"a").unwrap();c.membership(&"a",Some(2),None).unwrap();c.after_remove(&"a").unwrap();
        c.service_tick(2).unwrap();assert!(c.published(2).unwrap().is_empty());
    }
    #[test]
    fn invalid_membership_poison_isolated_formation_model() {
        let mut c=CpuFormation::default();c.add_block("a",p(0,0,0),1024,0,false).unwrap();
        assert_eq!(c.membership(&"a",Some(4),Some(5)),Err(FormationError::UnexpectedService));
        assert_eq!(c.published(5),Err(FormationError::UnexpectedService));
    }
    #[test]
    fn observed_unload_detaches_node_without_inventing_a_block_removal_break() {
        let mut c=CpuFormation::default();
        c.add_block("a",p(15,64,0),1024,0,false).unwrap();c.add_block("b",p(16,64,0),0,1,false).unwrap();
        c.membership(&"a",None,Some(1)).unwrap();c.ready(&"a").unwrap();
        c.membership(&"b",None,Some(1)).unwrap();c.ready(&"b").unwrap();
        let old=c.cluster_of(&"a").unwrap().unwrap().id;c.take_events().unwrap();c.service_tick(1).unwrap();
        c.membership(&"b",Some(1),None).unwrap();c.after_unload(&"b").unwrap();
        assert_eq!(c.cluster_of(&"a").unwrap().unwrap().id,old);
        assert!(c.take_events().unwrap().is_empty());c.service_tick(1).unwrap();assert_eq!(c.published(1).unwrap(),vec![old]);
        // A new block entity at the reloaded position has a new identity and
        // rebinds the resident cluster only when its own onReady runs.
        c.add_block("b2",p(16,64,0),0,1,false).unwrap();c.membership(&"b2",None,Some(1)).unwrap();c.ready(&"b2").unwrap();
        let new=c.cluster_of(&"a").unwrap().unwrap().id;assert_ne!(new,old);
        assert!(matches!(&c.take_events().unwrap()[0],FormationEvent::Broken(cluster) if cluster.id==old));
    }
}
