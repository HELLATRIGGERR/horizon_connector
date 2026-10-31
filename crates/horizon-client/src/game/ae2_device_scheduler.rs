//! Runtime-only AE2 automation trackers. Physical node incarnation prevents
//! stale state being inherited by a replacement at the same location.
use std::collections::BTreeMap;
use ae2_network_core::NodeIdentity;
use super::ae2_tick_tracker::{Tracker,Modulation};

#[derive(Clone)]
struct Entry { tracker:Tracker, service:Option<u64>, last_seen:u64, planned:Option<bool>, token:Option<Vec<u8>>, min:u64, max:u64, sleeping:bool, continuous:Option<bool>, generation_ticks:u64 }
#[derive(Clone,Default)]
pub(super) struct DeviceScheduler { entries:BTreeMap<NodeIdentity,Entry> }
impl DeviceScheduler {
    pub fn due(&mut self,id:NodeIdentity,service:Option<u64>,now:u64,min:u64,max:u64,sleeping:bool,alert:bool)->bool {
        let e=self.entries.entry(id).or_insert_with(||Entry{tracker:Tracker::new(now,min,max,sleeping),service,last_seen:now,planned:None,token:None,min,max,sleeping,continuous:None,generation_ticks:0});
        if e.service.zip(service).is_some_and(|(a,b)|a!=b) {e.tracker=Tracker::new(now,min,max,sleeping);}
        // Existing Java TickingRequests keep their original bounds. Remember
        // the latest configured bounds only for the next grid recreation.
        e.service=service.or(e.service);e.last_seen=now;e.sleeping=sleeping;e.min=min;e.max=max;
        if alert {e.tracker.alert();} else if sleeping {e.tracker.sleep();} else {e.tracker.wake();}
        e.tracker.due(now)
    }
    pub fn completed(&mut self,id:NodeIdentity,now:u64,modulation:Modulation){if let Some(e)=self.entries.get_mut(&id){e.tracker.completed(now,modulation);if modulation==Modulation::Sleep {e.sleeping=true;}}}
    /// AnnihilationPlanePart chooses generation mode on addToWorld and keeps
    /// its transient elapsed remainder on that physical part instance.
    pub fn continuous_mode(&mut self,id:NodeIdentity,initial:bool)->bool {
        *self.entries.get_mut(&id).expect("tracker was registered").continuous.get_or_insert(initial)
    }
    pub fn continuous_generate(&mut self,id:NodeIdentity,now:u64,interval:u64)->u64 {
        let entry=self.entries.get_mut(&id).expect("tracker was registered");
        entry.generation_ticks=entry.generation_ticks.saturating_add(now.saturating_sub(entry.tracker.last()));
        let amount=entry.generation_ticks/interval;
        entry.generation_ticks%=interval;
        amount
    }
    /// GridNode.setGrid transfers a tickable to a fresh tracker immediately,
    /// even when replay is deferred until a later server tick.
    pub fn observe_services(&mut self,services:&ae2_network_core::service_events::OrderedServices,now:u64) {
        for (id,e) in &mut self.entries {
            let Some(next)=services.service_id(*id) else {continue};
            if e.service.is_some_and(|old|old!=next) {e.tracker=Tracker::new(now,e.min,e.max,e.sleeping);}
            e.service=Some(next);
        }
    }
    /// Compare an exact bounded owner-state signature, without hash collisions.
    /// A first observation establishes the baseline; subsequent changes alert.
    pub fn due_with_token(&mut self,id:NodeIdentity,service:Option<u64>,now:u64,min:u64,max:u64,sleeping:bool,token:&[u8])->bool {
        self.due(id,service,now,min,max,sleeping,false);
        let e=self.entries.get_mut(&id).expect("tracker was registered");
        let changed=e.token.as_deref().is_some_and(|old|old!=token);
        e.token=Some(token.to_vec());
        if changed && !sleeping {e.tracker.alert();}
        e.tracker.due(now)
    }
    pub fn interface_due(&mut self,id:NodeIdentity,service:Option<u64>,now:u64,online:bool,work:bool,bounds:(u64,u64))->bool {
        let changed=self.entries.get(&id).and_then(|e|e.planned).is_some_and(|old|!old && work);
        let due=self.due(id,service,now,bounds.0,bounds.1,!online || !work,changed && online);
        if let Some(e)=self.entries.get_mut(&id){e.planned=Some(work);}
        due
    }
    /// Expire only long-unseen runtime records; unloaded physical owners have
    /// no runnable grid node and acquire a fresh tracker when reloaded.
    pub fn prune(&mut self,now:u64){self.entries.retain(|_,e|now.saturating_sub(e.last_seen)<1200);}
}
#[cfg(test)]
mod tests {
    use super::*;
    fn node(host:u64)->NodeIdentity {NodeIdentity {host,part:None,outer:false}}
    #[test]
    fn interface_work_transition_alerts_and_replacement_gets_fresh_tracker(){
        let mut s=DeviceScheduler::default();let a=node(1);
        assert!(!s.interface_due(a,Some(1),10,true,false,(5,120)));
        assert!(s.interface_due(a,Some(1),11,true,true,(5,120)));
        s.completed(a,11,Modulation::Sleep);
        assert!(!s.interface_due(node(2),Some(1),100,true,true,(5,120)));
        assert!(!s.interface_due(node(2),Some(1),161,true,true,(5,120)));
        assert!(s.interface_due(node(2),Some(1),162,true,true,(5,120)));
    }
    #[test]
    fn service_migration_restarts_rate_and_token_alerts_only_on_changes(){
        let mut s=DeviceScheduler::default();let id=node(3);
        assert!(!s.due_with_token(id,Some(1),0,5,40,false,b"first"));
        assert!(!s.due_with_token(id,Some(1),1,5,40,false,b"first"));
        assert!(s.due_with_token(id,Some(1),2,5,40,false,b"changed"));
        s.completed(id,2,Modulation::Slower);
        assert!(!s.due_with_token(id,Some(2),10,5,40,false,b"changed"));
        assert!(!s.due_with_token(id,Some(2),31,5,40,false,b"changed"));
        assert!(s.due_with_token(id,Some(2),32,5,40,false,b"changed"));
    }
    #[test]
    fn replayed_grid_change_uses_physical_event_tick() {
        let mut services=ae2_network_core::service_events::OrderedServices::default();
        let id=node(10);let other=node(11);
        services.add_node(id,None).unwrap();services.ensure_grid(id).unwrap();
        let old=services.service_id(id).unwrap();let mut scheduler=DeviceScheduler::default();
        assert!(!scheduler.due(id,Some(old),10,5,40,false,false));
        services.add_node(other,None).unwrap();services.ensure_grid(other).unwrap();services.connect(other,id).unwrap();
        let new=services.service_id(id).unwrap();assert_ne!(old,new);
        scheduler.observe_services(&services,11);
        assert!(!scheduler.due(id,Some(new),32,5,40,false,false));
        assert!(scheduler.due(id,Some(new),33,5,40,false,false));
    }
    #[test]
    fn updated_bounds_apply_only_after_node_moves_grid() {
        let mut scheduler=DeviceScheduler::default();let id=node(20);
        assert!(!scheduler.due(id,Some(1),0,5,40,false,false));
        assert!(!scheduler.due(id,Some(1),4,2,6,false,false));
        assert!(scheduler.due(id,Some(1),22,2,6,false,false));
        assert!(!scheduler.due(id,Some(2),30,2,6,false,false));
        assert!(!scheduler.due(id,Some(2),33,2,6,false,false));
        assert!(scheduler.due(id,Some(2),34,2,6,false,false));
    }
    #[test]
    fn sky_dust_elapsed_accumulates_and_mode_belongs_to_the_part_instance() {
        let mut scheduler=DeviceScheduler::default();let id=node(21);
        assert!(!scheduler.due(id,Some(1),0,5,60,false,false));
        assert!(scheduler.continuous_mode(id,true));
        assert!(scheduler.continuous_mode(id,false));
        for tick in [32,92,152] {
            assert!(scheduler.due(id,Some(1),tick,5,60,false,false));
            assert_eq!(scheduler.continuous_generate(id,tick,200),0);
            scheduler.completed(id,tick,Modulation::Idle);
        }
        assert!(scheduler.due(id,Some(1),212,5,60,false,false));
        assert_eq!(scheduler.continuous_generate(id,212,200),1);
        scheduler.completed(id,212,Modulation::Idle);
        let next=node(22);scheduler.due(next,Some(1),212,5,60,false,false);
        assert!(!scheduler.continuous_mode(next,false));
    }
}
