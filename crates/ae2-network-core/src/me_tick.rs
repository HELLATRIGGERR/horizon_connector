//! AE2 19.2.18 ME P2P tick scheduling, based on TickingRequest and
//! TickManagerService. A tunnel starts asleep with a 12-tick initial rate;
//! wake preserves its last tick and rate, and a completed request sleeps at
//! the 20-tick maximum. Connection changes are supplied separately at the
//! actual deferred callback; this type does not infer them from graph snapshots.
use crate::NodeIdentity;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
struct Tracker { last: u64, rate: u64, max: u64, awake: bool, order: u64 }
#[derive(Clone, Debug, Default)]
pub struct MeTickSchedule { trackers: BTreeMap<NodeIdentity, Tracker>, next_order: u64 }
impl MeTickSchedule {
    pub fn register(&mut self, node: NodeIdentity, now: u64) -> Result<(), &'static str> {
        self.register_with_rates(node,now,5,20)
    }
    pub fn register_with_rates(&mut self, node: NodeIdentity, now: u64, min:u64, max:u64) -> Result<(), &'static str> {
        if self.trackers.contains_key(&node) { return Err("ME tunnel already registered"); }
        if min==0 || min>max || max>i32::MAX as u64 {return Err("unsupported ME tunnel tick-rate bounds");}
        let order=self.next_order; self.next_order=self.next_order.checked_add(1).ok_or("ME tick order overflow")?;
        let initial=((min as i32).wrapping_add(max as i32)/2) as i64;
        let rate=initial.clamp(min as i64,max as i64) as u64;
        self.trackers.insert(node, Tracker { last: now, rate, max, awake: false, order });
        Ok(())
    }
    pub fn remove(&mut self, node: NodeIdentity) { self.trackers.remove(&node); }
    pub fn contains(&self, node: NodeIdentity) -> bool { self.trackers.contains_key(&node) }
    pub fn wake(&mut self, node: NodeIdentity) -> Result<bool, &'static str> {
        let tracker=self.trackers.get_mut(&node).ok_or("ME tunnel not registered")?;
        let changed=!tracker.awake; tracker.awake=true; Ok(changed)
    }
    /// Nodes due at this level-end tick, in due-time/last-time/rate order.
    /// Equal comparator ties retain registration order; Java's priority queue
    /// does not promise a stable tie order, so callers must not claim that tie
    /// case is source-exact without an event-order oracle.
    pub fn due(&self, now: u64) -> Vec<NodeIdentity> {
        let mut due: Vec<_>=self.trackers.iter().filter_map(|(node,t)| {
            let next=t.last.saturating_add(t.rate);
            (t.awake && next<=now).then_some(((next,t.last,t.rate,t.order),*node))
        }).collect();
        due.sort_by_key(|(key,_)| *key); due.into_iter().map(|(_,node)|node).collect()
    }
    pub fn completed_sleep(&mut self, node: NodeIdentity, now: u64) -> Result<(), &'static str> {
        let tracker=self.trackers.get_mut(&node).ok_or("ME tunnel not registered")?;
        if !tracker.awake { return Err("ME tunnel was not awake"); }
        tracker.last=now; tracker.rate=tracker.max; tracker.awake=false; Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn id(n:u64)->NodeIdentity {NodeIdentity {host:n,part:Some([n as i32,0,0,0]),outer:false}}
    #[test]
    fn wake_uses_original_deadline_and_sleep_restores_max_rate() {
        let mut schedule=MeTickSchedule::default();let n=id(1);
        schedule.register(n,100).unwrap();assert!(schedule.wake(n).unwrap());assert!(!schedule.wake(n).unwrap());
        assert!(schedule.due(111).is_empty());assert_eq!(schedule.due(112),vec![n]);
        schedule.completed_sleep(n,112).unwrap();schedule.wake(n).unwrap();assert!(schedule.due(131).is_empty());
        assert_eq!(schedule.due(132),vec![n]);schedule.completed_sleep(n,132).unwrap();
        schedule.wake(n).unwrap();assert_eq!(schedule.due(200),vec![n]);
        schedule.remove(n);assert!(schedule.due(200).is_empty());
    }
    #[test]
    fn configured_bounds_control_initial_and_sleep_rates() {
        let mut s=MeTickSchedule::default();let n=id(2);
        s.register_with_rates(n,10,2,6).unwrap();s.wake(n).unwrap();
        assert!(s.due(13).is_empty());assert_eq!(s.due(14),vec![n]);
        s.completed_sleep(n,14).unwrap();s.wake(n).unwrap();
        assert!(s.due(19).is_empty());assert_eq!(s.due(20),vec![n]);
    }
}
