//! Derived from AE2 19.2.18 TickTracker/TickManagerService. Runtime-only.
//! Copyright (c) 2021 TeamAppliedEnergistics. LGPL-3.0-or-later.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Modulation { Urgent, Faster, Slower, Same, Idle, Sleep }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Tracker { last:u64, rate:u64, min:u64, max:u64, awake:bool }
impl Tracker {
 pub fn new(now:u64,min:u64,max:u64,sleeping:bool)->Self {assert!(min>0 && min<=max);Self{last:now,rate:horizon_client::ae2_tick_config::initial_rate(min,max),min,max,awake:!sleeping}}
 pub fn due(&self,now:u64)->bool {self.awake && now>=self.last.saturating_add(self.rate)}
 pub fn last(&self)->u64 {self.last}
 pub fn rate(&self)->u64 {self.rate}
 pub fn is_awake(&self)->bool {self.awake}
 pub fn wake(&mut self){self.awake=true;}
 pub fn sleep(&mut self){if self.awake {self.awake=false;self.rate=self.max;}}
 pub fn alert(&mut self){self.awake=true;self.rate=1;}
 pub fn completed(&mut self,now:u64,modulation:Modulation){self.last=now;self.rate=match modulation {Modulation::Urgent=>self.min,Modulation::Faster=>self.rate.saturating_sub(2),Modulation::Slower=>self.rate.saturating_add(1),Modulation::Same=>self.rate,Modulation::Idle|Modulation::Sleep=>self.max}.clamp(self.min,self.max);if modulation==Modulation::Sleep {self.awake=false;}}
}
#[cfg(test)] mod tests {use super::*;
#[test] fn source_wake_preserves_deadline_while_alert_moves_it(){let mut t=Tracker::new(10,5,40,true);t.wake();assert!(!t.due(31));assert!(t.due(32));t.completed(32,Modulation::Sleep);t.wake();assert!(!t.due(71));assert!(t.due(72));t.sleep();t.alert();assert!(t.due(33));}
#[test] fn source_modulation_clamps_and_sleeps(){let mut t=Tracker::new(0,5,60,false);assert!(!t.due(31));assert!(t.due(32));t.completed(32,Modulation::Faster);assert_eq!(t.rate,30);t.completed(62,Modulation::Urgent);assert_eq!(t.rate,5);t.completed(67,Modulation::Faster);assert_eq!(t.rate,5);t.completed(72,Modulation::Sleep);assert!(!t.due(1000));assert_eq!(t.rate,60);t.wake();assert!(t.due(132));}}
