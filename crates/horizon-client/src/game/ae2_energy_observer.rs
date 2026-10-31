//! Ordered EnergyService ownership, driven by actual node lifecycle events.
use std::collections::BTreeMap;
use ae2_network_core::{NodeIdentity,service_events::MembershipChange};
use horizon_client::{ae2_world::{Machine,MachineKind},block_entities::{Ae2Mutation,Ae2MutationKind},containers::BlockEntity};
use horizon_world::nbt::Tag;
use super::{Dim,ae2_global::GridAddress,ae2_energy_lifecycle::{EnergyLifecycle,EnergyNode},ae2_energy_service::{Store,PassiveSelector,CREATIVE_STORED}};

#[derive(Clone,Debug)]
pub(super) struct EnergyPhysical {pub address:GridAddress,pub face:Option<u8>,pub emitter:Option<super::ae2_emitter::EnergyEmitterState>,pub reported:i64,pub output:bool,pub online:Option<bool>,pub passive:bool}
#[derive(Clone,Debug,Default)]
pub(super) struct EnergyObserver {
    pub lifecycle:EnergyLifecycle,
    pub per_node_capacity:Option<f64>,
    pub physical:BTreeMap<NodeIdentity,EnergyPhysical>,
    pub passive_selector:PassiveSelector,
    pub passive_suppressed:BTreeMap<NodeIdentity,bool>,
    pub failure:Option<String>,
}
fn dimension(n:i32)->Option<Dim>{Some(match n{0=>Dim::Overworld,1=>Dim::Nether,2=>Dim::End,3=>Dim::Spatial,_=>return None})}
fn saved_share(tag:Option<&Tag>)->Result<f64,String>{
    let Some(tag)=tag else{return Ok(0.0)};
    let Some(Tag::Double(value))=tag.get("e") else{return Ok(0.0)};
    let value=f64::from_bits(*value);
    if !value.is_finite(){return Err("invalid saved grid energy".into())}Ok(value.max(0.0))
}
fn store(machine:&Machine,host:u64)->Result<Option<Store>,String>{
    let (amount,capacity,priority,creative)=match machine.kind {
        MachineKind::Controller=>(machine.energy,8000.0,0,false),
        MachineKind::EnergyCell=>(machine.energy,200000.0,200,false),
        MachineKind::DenseEnergyCell=>(machine.energy,1600000.0,1600,false),
        MachineKind::CreativeEnergyCell=>(CREATIVE_STORED,CREATIVE_STORED,i32::MAX,true),
        _=>return Ok(None),
    };
    Store::new(amount,capacity,priority,host,true,true,creative).map(Some).map_err(str::to_owned)
}
impl EnergyObserver {
    fn insert(&mut self,id:NodeIdentity,address:GridAddress,face:Option<u8>,share:f64,store:Option<Store>,emitter:Option<super::ae2_emitter::EnergyEmitterState>,reported:i64,output:bool)->Result<(),String>{
        if self.physical.contains_key(&id){return Err("duplicate energy node identity".into())}
        self.lifecycle.register(id,EnergyNode::new(store,share).map_err(str::to_owned)?).map_err(str::to_owned)?;
        self.physical.insert(id,EnergyPhysical{address,face,emitter,reported,output,online:None,passive:false});Ok(())
    }
    fn insert_part(&mut self,host:u64,address:GridAddress,part:&Tag)->Result<(),String>{
        let Some(id)=super::ae2_channels::part_identity(part) else{return Ok(())};
        let Some(Tag::String(face))=part.get("face") else{return Err("energy part face unavailable".into())};
        let face=super::ae2_grid::face_index(&face.to_string_lossy()).ok_or("invalid energy part face")?;
        let Some(Tag::String(kind))=part.get("id") else{return Err("energy part kind unavailable".into())};let kind=kind.to_string_lossy();
        let node=NodeIdentity{host,part:Some(id),outer:false};
        let emitter=if kind=="ae2:energy_level_emitter" {let Tag::Compound(data)=part else{return Err("invalid energy emitter".into())};Some(super::ae2_emitter::energy_emitter_read(data)?)}else{None};
        let reported=match part.get("emitter_count"){Some(Tag::Long(value))=>*value,_=>0};
        let output=part.get("emitter_active")==Some(&Tag::Byte(1));
        self.insert(node,address,Some(face),saved_share(part.get("gn"))?,None,emitter,reported,output)?;
        if matches!(kind.as_str(),"ae2:quartz_fiber"|"ae2:toggle_bus"|"ae2:inverted_toggle_bus"|"ae2:me_p2p_tunnel") {
            self.insert(NodeIdentity{outer:true,..node},address,Some(face),saved_share(part.get("outer"))?,None,None,0,false)?;
        }
        Ok(())
    }
    pub fn before(&mut self,dimension_id:i32,event:&Ae2Mutation){
        if self.failure.is_some(){return}
        let result=(||->Result<(),String>{
            let address=GridAddress::from_local(dimension(dimension_id).ok_or("unknown energy dimension")?,event.position).ok_or("invalid energy position")?;
            match &event.kind {
                Ae2MutationKind::EntityAdded{incarnation,entity:BlockEntity::Ae2(machine),..}=>{
                    let id=NodeIdentity{host:*incarnation,part:None,outer:false};
                    let tag=if machine.kind==MachineKind::DebugPhantomNode{"proxy"}else{"gn"};
                    self.insert(id,address,None,saved_share(machine.config.get(&tag.into()))?,store(machine,*incarnation)?,None,0,false)?;
                    if machine.kind==MachineKind::DebugPhantomNode {
                        // The second managed node is unsaved and has no store.
                        self.insert(NodeIdentity::phantom_proxy(*incarnation),address,None,0.0,None,None,0,false)?;
                    }
                    if machine.kind==MachineKind::CrystalResonanceGenerator {self.physical.get_mut(&id).unwrap().passive=true;}
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
            let state=self.physical.get(&change.node).and_then(|p|p.emitter.clone());let mut report=None;
            self.lifecycle.membership_with_capacity(change,self.per_node_capacity.unwrap_or(25.0),|service,node|{
                if let Some(state)=state.as_ref(){service.watch(node,state.threshold as f64,node.host)?;report=Some(service.stored() as i64);}Ok(())
            }).map_err(str::to_owned)?;
            if let Some(value)=report {if let Some(physical)=self.physical.get_mut(&change.node){physical.reported=value;}}
            Ok(())
        });
        if let Err(error)=result{self.failure=Some(error);}
    }
    pub fn setting(&mut self,node:NodeIdentity,mut state:super::ae2_emitter::EnergyEmitterState,previous_threshold:i64,online:bool)->Result<(i64,bool),String>{
        if let Some(error)=self.failure.as_ref(){return Err(error.clone())}
        let Some(physical)=self.physical.get_mut(&node) else{return Err("energy emitter owner unavailable".into())};
        state.threshold=previous_threshold;
        if let Some(service_id)=self.lifecycle.service_of(node){
            let service=self.lifecycle.services.get_mut(&service_id).ok_or("energy service unavailable")?;
            service.reset_watch(node);service.watch(node,state.threshold as f64,node.host).map_err(str::to_owned)?;
            physical.reported=service.stored() as i64;
        }
        let boundary=physical.reported.wrapping_add(1);physical.online=Some(online);physical.output=online&&if state.low{state.threshold>=boundary}else{state.threshold<boundary};
        physical.emitter=Some(state);Ok((physical.reported,physical.output))
    }
    pub fn threshold(&mut self,node:NodeIdentity,state:super::ae2_emitter::EnergyEmitterState,online:bool)->Result<(i64,bool),String>{
        if let Some(error)=self.failure.as_ref(){return Err(error.clone())}
        let physical=self.physical.get_mut(&node).ok_or("energy emitter owner unavailable")?;
        let boundary=physical.reported.wrapping_add(1);physical.online=Some(online);physical.output=online&&if state.low{state.threshold>=boundary}else{state.threshold<boundary};
        physical.emitter=Some(state);Ok((physical.reported,physical.output))
    }
    /// Deliver this source service's watcher notifications before its own
    /// idle extraction, not after every other service has already ticked.
    pub fn select_passives(&mut self,groups:&[(Vec<u64>,Vec<NodeIdentity>)])->Result<BTreeMap<NodeIdentity,bool>,String>{
        if let Some(error)=self.failure.as_ref(){return Err(error.clone())}
        let mut selector=self.passive_selector.clone();let result=selector.select(groups).map_err(str::to_owned)?;
        self.passive_selector=selector;self.passive_suppressed=result.clone();Ok(result)
    }
    pub fn watcher_tick(&mut self,id:u64)->Result<(),String>{
        if let Some(error)=self.failure.as_ref(){return Err(error.clone())}
        let service=self.lifecycle.services.get_mut(&id).ok_or("missing energy service")?;
        let changed=service.watcher_end_tick();
        if !changed.is_empty(){
            let reported=service.stored() as i64;
            for node in changed {if let Some(physical)=self.physical.get_mut(&node){
                physical.reported=reported;
                if let Some(state)=physical.emitter.as_ref(){
                    let boundary=reported.wrapping_add(1);
                    physical.output=physical.online.unwrap_or(false)&&if state.low{state.threshold>=boundary}else{state.threshold<boundary};
                }
            }}
        }
        Ok(())
    }
    pub fn finish_service_tick(&mut self,id:u64,has_power:bool)->Result<(),String>{
        if let Some(error)=self.failure.as_ref(){return Err(error.clone())}
        self.lifecycle.services.get_mut(&id).ok_or("missing energy service")?.finish_service_tick(has_power);Ok(())
    }
    pub fn end_tick(&mut self){
        if self.failure.is_some(){return}
        for service in self.lifecycle.services.values_mut(){
            let changed=service.watcher_end_tick();
            if !changed.is_empty(){let reported=service.stored() as i64;for node in changed{if let Some(physical)=self.physical.get_mut(&node){physical.reported=reported;if let Some(state)=physical.emitter.as_ref(){let boundary=reported.wrapping_add(1);physical.output=physical.online.unwrap_or(false)&&if state.low{state.threshold>=boundary}else{state.threshold<boundary};}}}}
            service.finish_end_tick();
        }
    }
    pub fn observation(&mut self,node:NodeIdentity,state:super::ae2_emitter::EnergyEmitterState,online:bool)->Option<(i64,bool)>{
        if self.failure.is_some()||self.lifecycle.service_of(node).is_none(){return None}
        let physical=self.physical.get_mut(&node)?;
        if physical.online!=Some(online){let boundary=physical.reported.wrapping_add(1);physical.online=Some(online);physical.output=online&&if state.low{state.threshold>=boundary}else{state.threshold<boundary};}
        physical.emitter=Some(state);Some((physical.reported,physical.output))
    }
    pub fn after(&mut self,dimension_id:i32,event:&Ae2Mutation){
        if self.failure.is_some(){return}
        let result=(||->Result<(),String>{
            let address=GridAddress::from_local(dimension(dimension_id).ok_or("unknown energy dimension")?,event.position).ok_or("invalid energy position")?;
            let ids:Vec<NodeIdentity>=match &event.kind {
                Ae2MutationKind::EntityRemoved{incarnation,entity,..}=>{
                    let phantom=matches!(entity,BlockEntity::Ae2(machine) if machine.kind==MachineKind::DebugPhantomNode);
                    self.physical.iter().filter_map(|(id,p)|{
                        let retained_proxy=phantom&&id.is_phantom_proxy()&&self.lifecycle.service_of(*id).is_some();
                        (id.host==*incarnation&&p.address==address&&!retained_proxy).then_some(*id)
                    }).collect()
                },
                Ae2MutationKind::PartRemoved{incarnation,part}=>{let Some(uuid)=super::ae2_channels::part_identity(part) else{return Ok(())};self.physical.iter().filter_map(|(id,p)|(id.host==*incarnation&&id.part==Some(uuid)&&p.address==address).then_some(*id)).collect()},
                _=>Vec::new(),
            };
            for id in ids{self.lifecycle.forget(id).map_err(str::to_owned)?;self.physical.remove(&id);self.passive_suppressed.remove(&id);}
            Ok(())
        })();if let Err(error)=result{self.failure=Some(error);}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn phantom_energy_owner_is_separate_and_survives_host_removal() {
        use horizon_client::block_entities::BlockEntities;
        let pos=(0,64,0);
        let mut blocks=BlockEntities::default();blocks.start_ae2_journal();
        blocks.insert(pos,BlockEntity::Ae2(Box::new(Machine::new(MachineKind::DebugPhantomNode))));
        let host=blocks.incarnation(&pos).unwrap();
        let main=NodeIdentity{host,part:None,outer:false};
        let proxy=NodeIdentity::phantom_proxy(host);
        let mut energy=EnergyObserver::default();
        for event in blocks.take_ae2_mutations().unwrap(){energy.before(0,&event);energy.after(0,&event);}
        energy.membership(Ok(MembershipChange{node:main,from:None,to:Some(1)}));
        energy.membership(Ok(MembershipChange{node:proxy,from:None,to:Some(2)}));
        blocks.remove(&pos);
        let event=blocks.take_ae2_mutations().unwrap().into_iter().next().unwrap();
        energy.before(0,&event);
        energy.membership(Ok(MembershipChange{node:main,from:Some(1),to:None}));
        energy.after(0,&event);
        assert!(energy.failure.is_none(),"{:?}",energy.failure);
        assert!(!energy.physical.contains_key(&main));
        assert!(energy.physical.contains_key(&proxy));
        assert_eq!(energy.lifecycle.service_of(proxy),Some(2));
    }
    #[test]
    fn saved_active_output_is_recomputed_on_the_first_offline_observation() {
        let mut observer=EnergyObserver::default();
        let id=NodeIdentity{host:1,part:Some([1,2,3,4]),outer:false};
        let state=super::super::ae2_emitter::EnergyEmitterState{threshold:0,low:false};
        let address=GridAddress::from_local(Dim::Overworld,(0,64,0)).unwrap();
        observer.insert(id,address,Some(5),0.0,None,Some(state.clone()),100,true).unwrap();
        observer.membership(Ok(MembershipChange{node:id,from:None,to:Some(3)}));
        assert_eq!(observer.observation(id,state,false),Some((0,false)));
    }
}
