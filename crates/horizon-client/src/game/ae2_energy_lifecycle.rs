//! Ordered source GridNode.setGrid energy custody. This model receives actual
//! membership events and host snapshots; it never discovers ownership by scan.
use std::collections::BTreeMap;
use ae2_network_core::{NodeIdentity,service_events::MembershipChange};
use super::ae2_energy_service::{EnergyService,Store};

#[derive(Clone,Debug)]
pub(super) struct EnergyNode {
    pub initial_store:Option<Store>,
    pub saved_share:f64,
    service:Option<u64>,
}
impl EnergyNode {
    pub fn new(initial_store:Option<Store>,saved_share:f64)->Result<Self,&'static str>{
        if !saved_share.is_finite()||saved_share<0.0{return Err("invalid saved energy share")}
        Ok(Self{initial_store,saved_share,service:None})
    }
}
#[derive(Clone,Debug,Default)]
pub(super) struct EnergyLifecycle {
    nodes:BTreeMap<NodeIdentity,EnergyNode>,
    pub services:BTreeMap<u64,EnergyService>,
}
#[derive(Clone,Copy,Debug,PartialEq)]
pub(super) struct MembershipEffect {pub transferred:f64,pub discarded:f64,pub rejected:f64}
impl EnergyLifecycle {
    pub fn register(&mut self,id:NodeIdentity,node:EnergyNode)->Result<(),&'static str>{
        if self.nodes.contains_key(&id){return Err("duplicate energy owner")}self.nodes.insert(id,node);Ok(())
    }
    pub fn forget(&mut self,id:NodeIdentity)->Result<(),&'static str>{
        if self.nodes.get(&id).ok_or("missing energy owner")?.service.is_some(){return Err("energy owner still belongs to a service")}
        self.nodes.remove(&id);Ok(())
    }
    /// Caller stages this object with topology and provider observers, then
    /// commits all only after replay succeeds. The registration callback runs
    /// before restoring saved energy, matching EnergyService.addNode order.
    pub fn membership(&mut self,change:MembershipChange,on_add:impl FnMut(&mut EnergyService,NodeIdentity)->Result<(),&'static str>)->Result<MembershipEffect,&'static str>{
        self.membership_with_capacity(change,25.0,on_add)
    }
    pub fn membership_with_capacity(&mut self,change:MembershipChange,per_node_capacity:f64,mut on_add:impl FnMut(&mut EnergyService,NodeIdentity)->Result<(),&'static str>)->Result<MembershipEffect,&'static str>{
        let mut trial=self.clone();let node=trial.nodes.get(&change.node).ok_or("membership for unknown energy owner")?;
        if node.service!=change.from{return Err("energy service ownership mismatch")}
        let mut store=node.initial_store.clone();let mut share=node.saved_share;
        if let Some(from)=change.from {
            let service=trial.services.get_mut(&from).ok_or("missing old energy service")?;
            store=service.stores.get(&change.node).cloned();share=service.remove_node(change.node)?;
            service.reconfigure_node_capacity(per_node_capacity)?;
        }
        let mut result=MembershipEffect{transferred:0.0,discarded:0.0,rejected:0.0};
        if let Some(to)=change.to {
            let service=trial.services.entry(to).or_default();service.add_node(change.node,store.clone())?;
            service.reconfigure_node_capacity(per_node_capacity)?;
            on_add(service,change.node)?;
            let accepted=service.restore_node_share(change.node,share)?;result.transferred=accepted;result.rejected=share-accepted;
        } else {result.discarded=share;}
        let node=trial.nodes.get_mut(&change.node).unwrap();node.service=change.to;node.saved_share=0.0;node.initial_store=store;
        *self=trial;Ok(result)
    }
    /// Apply a real storage's direct change without inventing a grid transfer.
    /// As in the source, it changes the provider flags, not the cached total.
    pub fn update_external(&mut self,id:NodeIdentity,amount:f64)->Result<(),&'static str>{
        let node=self.nodes.get_mut(&id).ok_or("unknown physical energy store")?;
        if let Some(service)=node.service {
            self.services.get_mut(&service).ok_or("missing energy service")?.update_store(id,amount)?;
        } else {
            let old=node.initial_store.as_ref().ok_or("node has no physical energy store")?;
            node.initial_store=Some(Store::new(amount,old.capacity,old.priority,old.identity_order,old.can_extract,old.can_insert,old.creative)?);
        }
        Ok(())
    }
    /// Keep an established legacy fallback's real write in the observer while
    /// an ordered component is not ready. Injection does not raise the cache;
    /// an actual grid extraction subtracts the energy physically removed.
    pub fn reconcile_legacy(&mut self,id:NodeIdentity,before:f64,after:f64)->Result<(),&'static str>{
        let node=self.nodes.get(&id).ok_or("unknown physical energy store")?;
        if let Some(service)=node.service {
            let state=self.services.get_mut(&service).ok_or("missing energy service")?;
            if state.stores.get(&id).ok_or("missing physical power store")?.amount!=before{return Err("physical energy mirror diverged")}
            state.update_store(id,after)?;
            state.cached-=(before-after).max(0.0);
            Ok(())
        } else { self.update_external(id,after) }
    }
    pub fn service_of(&self,id:NodeIdentity)->Option<u64>{self.nodes.get(&id)?.service}
    pub fn saved_share(&self,id:NodeIdentity)->Result<f64,&'static str>{
        let service=self.service_of(id).ok_or("node has no energy service")?;
        self.services.get(&service).ok_or("missing energy service")?.node_share(id)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn n(id:u64)->NodeIdentity{NodeIdentity{host:id,part:None,outer:false}}
    #[test]
    fn membership_transfers_saved_custody_once_and_rejects_stale_owner(){
        let mut x=EnergyLifecycle::default();x.register(n(1),EnergyNode::new(None,10.0).unwrap()).unwrap();
        let first=x.membership(MembershipChange{node:n(1),from:None,to:Some(7)},|_,_|Ok(())).unwrap();assert_eq!(first.transferred,10.0);
        x.register(n(2),EnergyNode::new(None,0.0).unwrap()).unwrap();x.membership(MembershipChange{node:n(2),from:None,to:Some(7)},|_,_|Ok(())).unwrap();
        let moved=x.membership(MembershipChange{node:n(1),from:Some(7),to:Some(8)},|_,_|Ok(())).unwrap();assert_eq!(moved.transferred,5.0);
        assert_eq!(x.services[&7].local()+x.services[&8].local(),10.0);
        assert!(x.membership(MembershipChange{node:n(1),from:Some(7),to:None},|_,_|Ok(())).is_err());
        assert_eq!(x.service_of(n(1)),Some(8));
    }
    #[test]
    fn registration_callback_sees_service_before_saved_share_is_restored(){
        let mut x=EnergyLifecycle::default();x.register(n(1),EnergyNode::new(None,9.0).unwrap()).unwrap();
        x.membership(MembershipChange{node:n(1),from:None,to:Some(3)},|service,_|{assert_eq!(service.local(),0.0);Ok(())}).unwrap();
        assert_eq!(x.saved_share(n(1)).unwrap(),9.0);
    }
    #[test]
    fn configured_grid_capacity_is_applied_on_each_membership_change(){
        let mut x=EnergyLifecycle::default();
        x.register(n(1),EnergyNode::new(None,0.0).unwrap()).unwrap();
        x.membership_with_capacity(MembershipChange{node:n(1),from:None,to:Some(3)},100.0,|_,_|Ok(())).unwrap();
        assert_eq!(x.services[&3].local_capacity(),100.0);
        assert_eq!(x.services.get_mut(&3).unwrap().inject(80.0,false).unwrap(),0.0);
        x.register(n(2),EnergyNode::new(None,0.0).unwrap()).unwrap();
        x.membership_with_capacity(MembershipChange{node:n(2),from:None,to:Some(3)},60.0,|_,_|Ok(())).unwrap();
        assert_eq!(x.services[&3].local_capacity(),120.0);
        assert_eq!(x.services[&3].local(),80.0);
        x.membership_with_capacity(MembershipChange{node:n(1),from:Some(3),to:None},10.0,|_,_|Ok(())).unwrap();
        assert_eq!(x.services[&3].local_capacity(),10.0);
        assert_eq!(x.services[&3].local(),10.0);
    }
    #[test]
    fn failed_callback_rolls_back_membership_and_buffer(){
        let mut x=EnergyLifecycle::default();x.register(n(1),EnergyNode::new(None,9.0).unwrap()).unwrap();
        assert!(x.membership(MembershipChange{node:n(1),from:None,to:Some(3)},|_,_|Err("failed")).is_err());
        assert_eq!(x.service_of(n(1)),None);assert!(x.services.is_empty());
    }
    #[test]
    fn real_direct_injection_and_fallback_extraction_preserve_cache_timing(){
        let mut x=EnergyLifecycle::default();
        let store=Store::new(100.0,1000.0,0,1,true,true,false).unwrap();
        x.register(n(1),EnergyNode::new(Some(store),0.0).unwrap()).unwrap();
        x.membership(MembershipChange{node:n(1),from:None,to:Some(3)},|_,_|Ok(())).unwrap();
        x.update_external(n(1),300.0).unwrap();
        assert_eq!(x.services[&3].cached,100.0);
        x.reconcile_legacy(n(1),300.0,280.0).unwrap();
        assert_eq!(x.services[&3].cached,80.0);
        assert!(x.reconcile_legacy(n(1),300.0,270.0).is_err());
        assert_eq!(x.services[&3].stores[&n(1)].amount,280.0);
    }
}
