//! Source-shaped per-grid EnergyService accounting, staged before live hooks.
//! This model stages provider values; the world adapter must validate and
//! atomically commit their corresponding real block entities with this state.
use std::collections::{BTreeMap,BTreeSet};
use ae2_network_core::NodeIdentity;

const MIN_AMOUNT:f64=0.000001;
pub(super) const CREATIVE_STORED:f64=(i64::MAX / 10_000) as f64;
const PROVIDE_THRESHOLD:f64=0.1;
const RECEIVE_THRESHOLD:f64=0.1;

#[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord)]
pub(super) enum StoreKey { Local, Node(NodeIdentity) }
#[derive(Clone,Debug,PartialEq)]
pub(super) struct Store {
    pub amount:f64,
    pub capacity:f64,
    pub priority:i32,
    /// A caller-provided physical incarnation ordering token. Equal-priority
    /// Java ties use identity hashes and need not be identical across VMs.
    pub identity_order:u64,
    pub can_extract:bool,
    pub can_insert:bool,
    pub creative:bool,
    providing:bool,
    receiving:bool,
}
impl Store {
    pub fn new(amount:f64,capacity:f64,priority:i32,identity_order:u64,can_extract:bool,can_insert:bool,creative:bool)->Result<Self,&'static str>{
        if !amount.is_finite()||!capacity.is_finite()||amount<0.0||capacity<0.0||amount>capacity{return Err("invalid power store")}
        if creative && (amount!=CREATIVE_STORED||capacity!=CREATIVE_STORED){return Err("invalid creative power store")}
        Ok(Self{amount,capacity,priority,identity_order,can_extract,can_insert,creative,providing:amount>=PROVIDE_THRESHOLD,receiving:capacity-amount>=RECEIVE_THRESHOLD})
    }
}
#[derive(Clone,Debug)]
pub(super) struct EnergyService {
    members:BTreeSet<NodeIdentity>,
    pub stores:BTreeMap<NodeIdentity,Store>,
    providers:BTreeSet<StoreKey>,
    requesters:BTreeSet<StoreKey>,
    local:f64,
    local_providing:bool,
    local_receiving:bool,
    local_identity_order:u64,
    per_node_capacity:f64,
    pub cached:f64,
    pub age:u64,
    pub last_stored:f64,
    pub has_power:bool,
    pub public_power:bool,
    pub ticks_since_power_change:u64,
    watchers:BTreeMap<NodeIdentity,Vec<(f64,u64)>>,
}
impl Default for EnergyService {
    fn default()->Self {
        Self{members:BTreeSet::new(),stores:BTreeMap::new(),providers:BTreeSet::from([StoreKey::Local]),requesters:BTreeSet::from([StoreKey::Local]),local:0.0,local_providing:true,local_receiving:true,local_identity_order:0,per_node_capacity:25.0,cached:0.0,age:0,last_stored:-1.0,has_power:false,public_power:false,ticks_since_power_change:0,watchers:BTreeMap::new()}
    }
}
impl EnergyService {
    pub fn configured(per_node_capacity:f64,local_identity_order:u64)->Result<Self,&'static str>{
        if !per_node_capacity.is_finite()||!(1.0..=1_000_000.0).contains(&per_node_capacity){return Err("invalid grid storage per node")}
        Ok(Self{per_node_capacity,local_identity_order,..Self::default()})
    }
    pub fn local(&self)->f64{self.local}
    /// GridEnergyStorage updates its maximum when node membership changes.
    pub fn reconfigure_node_capacity(&mut self, per_node_capacity:f64)->Result<(),&'static str>{
        if !per_node_capacity.is_finite() || !(1.0..=1_000_000.0).contains(&per_node_capacity) {return Err("invalid grid storage per node")}
        self.per_node_capacity=per_node_capacity;
        self.local=self.local.min(self.local_capacity());
        self.update_local_flags();
        Ok(())
    }
    pub fn nodes(&self)->usize{self.members.len()}
    pub fn local_capacity(&self)->f64{(self.per_node_capacity*self.members.len() as f64).min(CREATIVE_STORED)}
    pub fn maximum(&self)->f64{self.local_capacity()+self.stores.values().filter(|s|s.can_extract).map(|s|s.capacity).sum::<f64>()}
    pub fn custody(&self)->f64{self.local+self.stores.values().filter(|s|!s.creative).map(|s|s.amount).sum::<f64>()}
    /// Source addNode: expand buffer, register the physical provider, then
    /// caller registers its watcher; only afterward restore saved node energy.
    pub fn add_node(&mut self,node:NodeIdentity,store:Option<Store>)->Result<(),&'static str>{
        if !self.members.insert(node){return Err("duplicate energy node")}
        self.update_local_flags();
        if let Some(store)=store {
            if store.can_extract {self.cached+=store.amount;self.providers.insert(StoreKey::Node(node));}
            if store.can_insert {self.requesters.insert(StoreKey::Node(node));}
            self.stores.insert(node,store);
        }
        Ok(())
    }
    /// The share is an ephemeral transfer on GridNode.setGrid. An actual node
    /// destruction discards its share, matching GridEnergyStorage.removeNode.
    pub fn remove_node(&mut self,node:NodeIdentity)->Result<f64,&'static str>{
        if !self.members.contains(&node){return Err("missing energy node")}
        let share=self.local/self.members.len() as f64;
        self.set_local((self.local-share).max(0.0));
        self.members.remove(&node);self.local=self.local.min(self.local_capacity());self.update_local_flags();
        if let Some(store)=self.stores.remove(&node){if store.can_extract {self.cached-=store.amount;}}
        self.providers.remove(&StoreKey::Node(node));self.requesters.remove(&StoreKey::Node(node));self.watchers.remove(&node);
        Ok(share)
    }
    /// Source saveNodeData reads this share without removing or duplicating it.
    pub fn node_share(&self,node:NodeIdentity)->Result<f64,&'static str>{
        if !self.members.contains(&node){return Err("missing energy node")}
        Ok(self.local/self.members.len() as f64)
    }
    pub fn restore_node_share(&mut self,node:NodeIdentity,share:f64)->Result<f64,&'static str>{
        if !self.members.contains(&node)||!share.is_finite()||share<0.0{return Err("invalid saved energy share")}
        if share<MIN_AMOUNT{return Ok(0.0)}
        let accepted=share.min((self.local_capacity()-self.local).max(0.0));self.set_local(self.local+accepted);Ok(accepted)
    }
    fn update_local_flags(&mut self){
        let providing=self.local>=PROVIDE_THRESHOLD;let receiving=self.local_capacity()-self.local>=RECEIVE_THRESHOLD;
        if !self.local_providing&&providing {self.providers.insert(StoreKey::Local);}
        if !self.local_receiving&&receiving {self.requesters.insert(StoreKey::Local);}
        self.local_providing=providing;self.local_receiving=receiving;
    }
    fn set_local(&mut self,after:f64){
        let capacity=self.local_capacity();self.local=if after<MIN_AMOUNT{0.0}else{after.min(capacity).max(0.0)};self.update_local_flags();
    }
    /// A physical storage's own update may re-enable it as a provider or
    /// requester; it does not itself raise the cached available amount.
    pub fn update_store(&mut self,node:NodeIdentity,amount:f64)->Result<(),&'static str>{
        let s=self.stores.get_mut(&node).ok_or("missing physical power store")?;
        if !amount.is_finite()||amount<0.0||amount>s.capacity||s.creative && amount!=CREATIVE_STORED{return Err("invalid physical power update")}
        let amount=if amount<MIN_AMOUNT{0.0}else{amount};
        let providing=amount>=PROVIDE_THRESHOLD;let receiving=s.capacity-amount>=RECEIVE_THRESHOLD;
        if !s.providing&&providing&&s.can_extract{self.providers.insert(StoreKey::Node(node));}
        if !s.receiving&&receiving&&s.can_insert{self.requesters.insert(StoreKey::Node(node));}
        s.providing=providing;s.receiving=receiving;s.amount=amount;Ok(())
    }
    fn order(&self,set:&BTreeSet<StoreKey>,extract:bool)->Vec<StoreKey>{
        let mut list:Vec<_>=set.iter().copied().collect();
        let key=|k:&StoreKey| match k {StoreKey::Local=>(i32::MIN,self.local_identity_order),StoreKey::Node(id)=>{let s=&self.stores[id];(s.priority,s.identity_order)}};
        if extract {list.sort_by(|a,b|key(b).cmp(&key(a)));} else {list.sort_by_key(key);}
        list
    }
    /// `simulate` leaves cache, membership and physical snapshots unchanged.
    /// The caller commits updated stores and service together after validating
    /// every captured host incarnation and value.
    pub fn extract(&mut self,amount:f64,simulate:bool)->Result<f64,&'static str>{
        if !amount.is_finite()||amount<0.0{return Err("invalid energy extraction")}
        let mut trial=self.clone();let mut done=0.0;
        for key in trial.order(&trial.providers,true){
            let req=amount-done;if req<=0.0{break}
            let got=match key {
                StoreKey::Local=>{let got=if req<MIN_AMOUNT{0.0}else{req.min(trial.local)};trial.set_local(trial.local-got);got},
                StoreKey::Node(id)=>{let s=&trial.stores[&id];let got=if s.creative{req}else if req<MIN_AMOUNT{0.0}else{req.min(s.amount)};let next=if s.creative{s.amount}else{s.amount-got};trial.update_store(id,next)?;got},
            };
            done+=got;
            if got<req {trial.providers.remove(&key);}
        }
        trial.cached-=done;
        if !simulate{*self=trial;}Ok(done)
    }
    /// Java injectProviderPower changes the real store but does not increment
    /// globalAvailablePower. It becomes visible when the cache refreshes.
    pub fn inject(&mut self,amount:f64,simulate:bool)->Result<f64,&'static str>{
        if !amount.is_finite()||amount<0.0{return Err("invalid energy injection")}
        let mut trial=self.clone();let mut remaining=amount;
        for key in trial.order(&trial.requesters,false){
            if remaining<=0.0{break}
            let accepted=match key {
                StoreKey::Local=>{let got=if remaining<MIN_AMOUNT{0.0}else{remaining.min((trial.local_capacity()-trial.local).max(0.0))};trial.set_local(trial.local+got);got},
                StoreKey::Node(id)=>{let s=&trial.stores[&id];let got=if s.creative{remaining}else if remaining<MIN_AMOUNT{0.0}else{remaining.min((s.capacity-s.amount).max(0.0))};let next=if s.creative{s.amount}else{s.amount+got};trial.update_store(id,next)?;got},
            };
            remaining-=accepted;
            if remaining>0.0{trial.requesters.remove(&key);}
        }
        if !simulate{*self=trial;}Ok(remaining.max(0.0))
    }
    pub fn stored(&mut self)->f64 {
        if self.age>90 {self.cached=self.order(&self.providers,true).into_iter().map(|key|match key{StoreKey::Local=>self.local,StoreKey::Node(id)=>self.stores[&id].amount}).sum();self.age=0;}
        self.cached.max(0.0)
    }
    pub fn reset_watch(&mut self,node:NodeIdentity){self.watchers.remove(&node);}
    pub fn watch(&mut self,node:NodeIdentity,threshold:f64,identity_order:u64)->Result<bool,&'static str>{
        if !self.members.contains(&node)||threshold.is_nan()||threshold<0.0{return Err("invalid energy watch")}
        let values=self.watchers.entry(node).or_default();
        if values.iter().any(|(v,_)|v.to_bits()==threshold.to_bits()){return Ok(false)}
        values.push((threshold,identity_order));Ok(true)
    }
    pub fn remove_watch(&mut self,node:NodeIdentity,threshold:f64)->bool {
        let Some(values)=self.watchers.get_mut(&node) else{return false};
        let count=values.len();values.retain(|(v,_)|v.to_bits()!=threshold.to_bits());let removed=count!=values.len();
        if values.is_empty(){self.watchers.remove(&node);}removed
    }
    /// Call before this service's idle extraction. Only registered watchers
    /// cause a cache read, as in EnergyService.onServerEndTick.
    pub fn watcher_end_tick(&mut self)->Vec<NodeIdentity> {
        if self.watchers.is_empty(){return Vec::new()}
        let value=self.stored();let low=self.last_stored.min(value);let high=self.last_stored.max(value);self.last_stored=value;
        let mut matches:Vec<_>=self.watchers.iter().flat_map(|(node,values)|values.iter().filter_map(move |(t,order)|(*t>=low&&*t<=high).then_some((*t,*order,*node)))).collect();
        matches.sort_by(|a,b|a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        matches.into_iter().map(|(_,_,node)|node).collect()
    }
    /// Source EnergyService delays only the public transition to on. A power
    /// loss is visible immediately; a fresh powered run takes 32 callbacks.
    pub fn finish_service_tick(&mut self,has_power:bool){
        if has_power==self.has_power {self.ticks_since_power_change=self.ticks_since_power_change.saturating_add(1);}
        else {self.ticks_since_power_change=0;}
        self.has_power=has_power;
        if self.has_power && self.ticks_since_power_change>30 {self.public_power=true;}
        else if !self.has_power {self.public_power=false;}
        self.age=self.age.saturating_add(1);
    }
    /// Legacy incomplete-topology fallback keeps time but cannot publish a
    /// source power transition without a verified physical service.
    pub fn finish_end_tick(&mut self){self.age=self.age.saturating_add(1);}
}

/// One incumbent passive generator per actual energy overlay. All currently
/// native resonance generators share the configured rate. Source tie order is
/// identity-based; this stable physical ordering is not a Java tie claim.
#[derive(Clone,Debug,Default)]
pub(super) struct PassiveSelector {selected:BTreeMap<Vec<u64>,NodeIdentity>}
impl PassiveSelector {
    pub fn select(&mut self,groups:&[(Vec<u64>,Vec<NodeIdentity>)])->Result<BTreeMap<NodeIdentity,bool>,&'static str>{
        let mut keys=BTreeSet::new();let mut seen=BTreeSet::new();let mut next=BTreeMap::new();let mut suppressed=BTreeMap::new();
        for (services,nodes) in groups {
            let mut key=services.clone();key.sort_unstable();key.dedup();
            if key.is_empty()||key.len()!=services.len()||!keys.insert(key.clone()){return Err("invalid passive energy overlay")}
            let mut candidates=nodes.clone();candidates.sort();candidates.dedup();
            if candidates.len()!=nodes.len()||candidates.iter().any(|n|!seen.insert(*n)){return Err("duplicate passive generator")}
            if candidates.is_empty(){continue}
            let winner=self.selected.get(&key).filter(|id|candidates.contains(id)).copied().unwrap_or(candidates[0]);
            for node in candidates {suppressed.insert(node,node!=winner);}
            next.insert(key,winner);
        }
        self.selected=next;Ok(suppressed)
    }
}

/// One actual source overlay traversal, with the source's descending service
/// capacity order. The caller supplies traversal order for equal capacities:
/// Java's reference-set iteration uses identity, not coordinates.
#[derive(Clone,Debug,Default)]
pub(super) struct EnergyOverlay {pub services:BTreeMap<u64,EnergyService>}
impl EnergyOverlay {
    fn order(&self,source_order:&[u64])->Result<Vec<u64>,&'static str>{
        let mut seen=BTreeSet::new();let mut ids=Vec::new();
        for id in source_order {if !seen.insert(*id)||!self.services.contains_key(id){return Err("invalid overlay membership")}ids.push(*id);}
        ids.sort_by(|a,b|self.services[b].maximum().total_cmp(&self.services[a].maximum()));
        Ok(ids)
    }
    pub fn extract(&mut self,source_order:&[u64],amount:f64,simulate:bool)->Result<f64,&'static str>{
        if !amount.is_finite()||amount<0.0{return Err("invalid overlay extraction")}
        let mut staged=self.clone();let mut received=0.0;
        for id in staged.order(source_order)? {if received>=amount{break}received+=staged.services.get_mut(&id).unwrap().extract(amount-received,false)?;}
        if !simulate{*self=staged;}Ok(received)
    }
    pub fn inject(&mut self,source_order:&[u64],amount:f64,simulate:bool)->Result<f64,&'static str>{
        if !amount.is_finite()||amount<0.0{return Err("invalid overlay injection")}
        let mut staged=self.clone();let mut remaining=amount;
        for id in staged.order(source_order)? {if remaining<=0.0{break}remaining=staged.services.get_mut(&id).unwrap().inject(remaining,false)?;}
        if !simulate{*self=staged;}Ok(remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn node(n:u64)->NodeIdentity{NodeIdentity{host:n,part:None,outer:false}}
    #[test]
    fn resonance_selector_preserves_one_incumbent_per_overlay_and_reselects_after_split(){
        let mut selector=PassiveSelector::default();
        let first=selector.select(&[(vec![1],vec![node(1),node(2)]),(vec![2],vec![node(3)])]).unwrap();
        assert_eq!(first.values().filter(|suppressed|!**suppressed).count(),2);
        let joined=selector.select(&[(vec![1,2],vec![node(1),node(2),node(3)])]).unwrap();
        assert_eq!(joined.values().filter(|suppressed|!**suppressed).count(),1);
        let incumbent=*joined.iter().find(|(_,suppressed)|!**suppressed).unwrap().0;
        let stable=selector.select(&[(vec![2,1],vec![node(3),node(2),node(1)])]).unwrap();assert_eq!(stable[&incumbent],false);
        let remaining:Vec<_>=[node(1),node(2),node(3)].into_iter().filter(|id|*id!=incumbent).collect();
        let after=selector.select(&[(vec![1,2],remaining)]).unwrap();assert_eq!(after.values().filter(|suppressed|!**suppressed).count(),1);
        let split=selector.select(&[(vec![1],vec![node(1),node(2)]),(vec![2],vec![node(3)])]).unwrap();
        assert_eq!(split.values().filter(|suppressed|!**suppressed).count(),2);
    }
    #[test]
    fn public_power_waits_for_stable_source_ticks_and_drops_immediately(){
        let mut service=EnergyService::default();
        for _ in 0..31 {service.finish_service_tick(true);assert!(!service.public_power);}
        service.finish_service_tick(true);assert!(service.public_power);
        service.finish_service_tick(false);assert!(!service.public_power);
        for _ in 0..31 {service.finish_service_tick(true);assert!(!service.public_power);}
        service.finish_service_tick(true);assert!(service.public_power);
    }
    fn cell(amount:f64,capacity:f64)->Store{Store::new(amount,capacity,0,1,true,true,false).unwrap()}
    #[test]
    fn next_service_watcher_observes_prior_service_idle_extraction(){
        let mut first=EnergyService::default();first.add_node(node(1),None).unwrap();
        let mut second=EnergyService::default();second.add_node(node(2),Some(cell(100.0,1000.0))).unwrap();
        second.watch(node(2),50.0,2).unwrap();assert_eq!(second.watcher_end_tick(),vec![node(2)]);
        let mut overlay=EnergyOverlay{services:BTreeMap::from([(1,first),(2,second)])};
        overlay.services.get_mut(&1).unwrap().watcher_end_tick();
        assert_eq!(overlay.extract(&[1,2],60.0,false).unwrap(),60.0);
        assert_eq!(overlay.services.get_mut(&2).unwrap().watcher_end_tick(),vec![node(2)]);
        assert_eq!(overlay.services.get_mut(&2).unwrap().stored(),40.0);
    }
    #[test]
    fn injection_is_physical_now_but_cached_only_after_source_refresh() {
        let mut s=EnergyService::default();s.add_node(node(1),Some(cell(100.0,1000.0))).unwrap();s.watch(node(1),200.0,1).unwrap();
        assert_eq!(s.watcher_end_tick(),Vec::<NodeIdentity>::new());s.finish_end_tick();
        assert_eq!(s.inject(200.0,false).unwrap(),0.0);assert_eq!(s.custody(),300.0);assert_eq!(s.stored(),100.0);
        for _ in 0..90 {assert!(s.watcher_end_tick().is_empty());s.finish_end_tick();}
        assert_eq!(s.watcher_end_tick(),vec![node(1)]);assert_eq!(s.stored(),300.0);
    }
    #[test]
    fn direct_cell_injection_is_not_reported_until_the_service_refresh() {
        let mut s=EnergyService::default();s.add_node(node(1),Some(cell(100.0,1000.0))).unwrap();
        s.watch(node(1),200.0,1).unwrap();s.watcher_end_tick();s.finish_end_tick();
        s.update_store(node(1),300.0).unwrap();assert_eq!(s.stored(),100.0);assert_eq!(s.custody(),300.0);
        for _ in 0..90 {assert!(s.watcher_end_tick().is_empty());s.finish_end_tick();}
        assert_eq!(s.watcher_end_tick(),vec![node(1)]);assert_eq!(s.stored(),300.0);
    }
    #[test]
    fn moving_a_node_transfers_only_its_real_buffer_share_and_destroy_discards_it() {
        let mut a=EnergyService::default();a.add_node(node(1),None).unwrap();a.add_node(node(2),None).unwrap();
        assert_eq!(a.restore_node_share(node(1),20.0).unwrap(),20.0);assert_eq!(a.node_share(node(1)).unwrap(),10.0);
        let share=a.remove_node(node(1)).unwrap();let mut b=EnergyService::default();b.add_node(node(1),None).unwrap();b.restore_node_share(node(1),share).unwrap();
        assert_eq!(a.local()+b.local(),20.0);let lost=b.remove_node(node(1)).unwrap();assert_eq!(lost,10.0);assert_eq!(b.local(),0.0);
    }
    #[test]
    fn five_node_source_split_preserves_four_shares_and_loses_destroyed_bridge_share() {
        let mut joined=EnergyService::default();for id in 1..=5{joined.add_node(node(id),None).unwrap();}
        assert_eq!(joined.inject(60.0,false).unwrap(),0.0);assert_eq!(joined.local(),60.0);
        let mut left=EnergyService::default();left.add_node(node(1),None).unwrap();
        let moved=joined.remove_node(node(1)).unwrap();left.restore_node_share(node(1),moved).unwrap();
        let destroyed_share=joined.remove_node(node(2)).unwrap();
        assert_eq!((left.nodes(),joined.nodes()),(1,3));assert_eq!((left.local(),joined.local(),destroyed_share),(12.0,36.0,12.0));
    }
    #[test]
    fn creative_is_finite_when_reported_but_never_depleted_on_extract() {
        let mut s=EnergyService::default();s.add_node(node(1),Some(Store::new(CREATIVE_STORED,CREATIVE_STORED,i32::MAX,1,true,true,true).unwrap())).unwrap();
        assert_eq!(s.stored(),CREATIVE_STORED);assert_eq!(s.extract(10.0,false).unwrap(),10.0);assert_eq!(s.stores[&node(1)].amount,CREATIVE_STORED);assert_eq!(s.stored(),CREATIVE_STORED-10.0);
    }
    #[test]
    fn overlay_transfers_are_ordered_by_service_capacity_without_minting_energy() {
        let mut a=EnergyService::default();a.add_node(node(1),Some(cell(100.0,1000.0))).unwrap();
        let mut b=EnergyService::default();b.add_node(node(2),Some(cell(200.0,2000.0))).unwrap();
        let mut overlay=EnergyOverlay::default();overlay.services.insert(1,a);overlay.services.insert(2,b);
        assert_eq!(overlay.extract(&[1,2],50.0,false).unwrap(),50.0);
        assert_eq!(overlay.services[&1].custody(),100.0);assert_eq!(overlay.services[&2].custody(),150.0);
        let before=overlay.services.values().map(EnergyService::custody).sum::<f64>();
        assert_eq!(overlay.inject(&[1,2],40.0,false).unwrap(),0.0);
        assert_eq!(overlay.services.values().map(EnergyService::custody).sum::<f64>(),before+40.0);
    }
    #[test]
    fn simulation_never_moves_physical_power_or_mutates_the_cache() {
        let mut s=EnergyService::default();s.add_node(node(1),Some(cell(10.0,100.0))).unwrap();let before=s.clone();
        assert_eq!(s.extract(5.0,true).unwrap(),5.0);assert_eq!(s.inject(20.0,true).unwrap(),0.0);assert_eq!(s.stores,before.stores);assert_eq!(s.cached,before.cached);assert_eq!(s.local,before.local);
    }
}
