//! Source-ordered QuantumCluster signed-frequency registration and links.
//! Derived from AE2 19.2.18 QuantumCluster and Locatables (LGPL-3.0-or-later).
//! The host must deliver real formed, inventory-change, and teardown callbacks.
use crate::NodeIdentity;
use std::collections::BTreeMap;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct LinkEvent {pub a:NodeIdentity,pub b:NodeIdentity,pub added:bool}
#[derive(Clone,Copy,Debug,Default)]
struct Cluster { side:i64, other:i64, peer:Option<NodeIdentity>, node_live:bool }
#[derive(Clone,Debug,Default)]
pub struct QuantumRegistry {clusters:BTreeMap<NodeIdentity,Cluster>,keys:BTreeMap<i64,NodeIdentity>}
impl QuantumRegistry {
    /// A formed cluster receives its node before its first status callback.
    pub fn form(&mut self,id:NodeIdentity)->Result<(), &'static str>{
        self.form_with_node(id,true)
    }
    /// Formation and frequency registration may precede the center's onReady.
    pub fn form_with_node(&mut self,id:NodeIdentity,node_live:bool)->Result<(), &'static str>{
        if self.clusters.contains_key(&id){return Err("quantum cluster already formed")}
        self.clusters.insert(id,Cluster{node_live,..Cluster::default()});Ok(())
    }
    /// onReady changes availability only. AE2 does not recalculate its
    /// connection until an actual subsequent cluster status callback.
    pub fn set_node_live(&mut self,id:NodeIdentity,node_live:bool)->Result<(), &'static str>{
        self.clusters.get_mut(&id).ok_or("quantum cluster is not formed")?.node_live=node_live;
        Ok(())
    }
    fn disconnect(&mut self,id:NodeIdentity,events:&mut Vec<LinkEvent>){
        let Some(peer)=self.clusters.get(&id).and_then(|c|c.peer) else{return};
        self.clusters.get_mut(&id).unwrap().peer=None;
        if let Some(other)=self.clusters.get_mut(&peer){if other.peer==Some(id){other.peer=None;}}
        events.push(LinkEvent{a:id,b:peer,added:false});
    }
    /// Runs QuantumCluster.updateStatus with the physical center's frequency.
    /// All registered clusters here have live nodes; host unload must destroy them.
    pub fn update(&mut self,id:NodeIdentity,frequency:i64)->Result<Vec<LinkEvent>, &'static str>{
        let mut current=*self.clusters.get(&id).ok_or("quantum cluster is not formed")?;
        let neg=frequency.wrapping_neg();
        if current.side!=frequency && current.side!=neg {
            if frequency!=0 {
                if current.side!=0 {self.keys.remove(&current.side);}
                if !self.keys.contains_key(&neg) {current.other=frequency;current.side=neg;}
                else if !self.keys.contains_key(&frequency) {current.side=frequency;current.other=neg;}
                self.keys.insert(current.side,id);
            } else {
                self.keys.remove(&current.side);current.other=0;current.side=0;
            }
        }
        self.clusters.insert(id,current);
        let other=if current.other==0 {None}else{self.keys.get(&current.other).copied()};
        let mut events=Vec::new();
        if let Some(peer)=other.filter(|peer|*peer!=id && current.side!=0 && current.node_live && self.clusters.get(peer).is_some_and(|c|c.side!=0 && c.node_live)) {
            if self.clusters[&id].peer==Some(peer) && self.clusters[&peer].peer==Some(id){return Ok(events)}
            self.disconnect(id,&mut events);self.disconnect(peer,&mut events);
            self.clusters.get_mut(&id).unwrap().peer=Some(peer);self.clusters.get_mut(&peer).unwrap().peer=Some(id);
            events.push(LinkEvent{a:id,b:peer,added:true});
        } else {self.disconnect(id,&mut events);}
        Ok(events)
    }
    /// Source destroys the connection before unregistering its signed key.
    pub fn destroy(&mut self,id:NodeIdentity)->Result<Vec<LinkEvent>, &'static str>{
        let c=*self.clusters.get(&id).ok_or("quantum cluster is not formed")?;
        let mut events=Vec::new();self.disconnect(id,&mut events);
        if c.side!=0 {self.keys.remove(&c.side);}
        self.clusters.remove(&id);Ok(events)
    }
    pub fn sides(&self,id:NodeIdentity)->Option<(i64,i64)>{self.clusters.get(&id).map(|c|(c.side,c.other))}
    pub fn peer(&self,id:NodeIdentity)->Option<NodeIdentity>{self.clusters.get(&id).and_then(|c|c.peer)}
    /// Each actual connection exactly once, retaining its endpoints' identity.
    pub fn links(&self)->impl Iterator<Item=(NodeIdentity,NodeIdentity)> + '_ {
        self.clusters.iter().filter_map(|(&id,c)|c.peer.filter(|peer|id<*peer).map(|peer|(id,peer)))
    }
}
#[cfg(test)]mod tests{
 use super::*;
 fn id(n:u64)->NodeIdentity{NodeIdentity{host:n,part:None,outer:false}}
 #[test]fn three_real_source_callback_order_keeps_first_pair_and_does_not_promote_third(){
  let mut r=QuantumRegistry::default();for n in [20,0,10]{r.form(id(n)).unwrap();}
  assert!(r.update(id(20),42).unwrap().is_empty());assert_eq!(r.sides(id(20)),Some((-42,42)));
  assert_eq!(r.update(id(0),42).unwrap(),vec![LinkEvent{a:id(0),b:id(20),added:true}]);
  assert_eq!(r.update(id(10),42).unwrap(),vec![]);assert_eq!(r.sides(id(10)),Some((0,0)));
  assert_eq!(r.update(id(20),0).unwrap(),vec![LinkEvent{a:id(20),b:id(0),added:false}]);
  assert_eq!(r.peer(id(10)),None);assert_eq!(r.peer(id(0)),None);
 }
 #[test]fn registered_center_connects_only_after_ready_and_real_status_callback(){
  let mut r=QuantumRegistry::default();
  r.form_with_node(id(1),true).unwrap();r.form_with_node(id(2),false).unwrap();
  r.update(id(1),42).unwrap();r.update(id(2),42).unwrap();
  assert_eq!(r.sides(id(1)),Some((-42,42)));assert_eq!(r.sides(id(2)),Some((42,-42)));
  assert_eq!(r.peer(id(1)),None);r.set_node_live(id(2),true).unwrap();
  assert_eq!(r.peer(id(1)),None,"readiness alone must not invoke a cluster status callback");
  assert_eq!(r.update(id(2),42).unwrap(),vec![LinkEvent{a:id(2),b:id(1),added:true}]);
  assert_eq!(r.peer(id(1)),Some(id(2)));
 }
}
