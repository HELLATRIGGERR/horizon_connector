//! Cooperative worker for a crafting calculation.
//!
//! AE2's CraftingCalculation.handlePausing and TickHandler.simulateCraftingJobs
//! keep the recursive stack on a worker, and give each calculation a share of
//! the configured server-tick time. The worker owns its immutable snapshot;
//! planning never mutates a world's inventory.
use crate::{extended::{Plan,Registry},Error,Inventory};
use std::{
    panic::{catch_unwind,AssertUnwindSafe},
    sync::{Arc,Condvar,Mutex},
    thread,
    time::{Duration,Instant},
};

type Outcome = Result<(Plan,Inventory),Error>;

struct State {
    running: bool,
    cancelled: bool,
    done: bool,
    deadline: Instant,
    result: Option<Outcome>,
}

struct Shared { state:Mutex<State>, wake:Condvar }

/// A simulation awaiting server-tick time slices.
///
/// Dropping a calculation wakes a suspended worker and requests cancellation.
/// It never waits for a worker to finish while the server is closing a menu.
pub struct Calculation { shared:Arc<Shared> }

#[derive(Clone,Copy)]
pub enum Strategy { Full, CraftLess }

struct Checkpoint { shared:Arc<Shared>, calls:u32 }
impl Checkpoint {
    fn wait(&mut self)->Result<(),Error> {
        // Source checks its clock after every 101 recursive requests.
        if self.calls<=100 {self.calls+=1;return Ok(());}
        self.calls=1;
        let mut state=self.shared.state.lock().unwrap_or_else(|e|e.into_inner());
        if state.running && Instant::now()>=state.deadline {
            state.running=false;
            self.shared.wake.notify_all();
        }
        while !state.running && !state.cancelled {
            state=self.shared.wake.wait(state).unwrap_or_else(|e|e.into_inner());
        }
        if state.cancelled {Err(Error::Cancelled)} else {Ok(())}
    }
}

impl Calculation {
    /// Begin paused with a private snapshot. No worker touches host state.
    pub fn new(registry:Registry,inventory:Inventory,target:String,amount:u64)->Self {
        Self::with_strategy(registry,inventory,target,amount,Strategy::Full)
    }
    pub fn with_strategy(registry:Registry,inventory:Inventory,target:String,amount:u64,strategy:Strategy)->Self {
        let shared=Arc::new(Shared {state:Mutex::new(State {
            running:false,cancelled:false,done:false,deadline:Instant::now(),result:None,
        }),wake:Condvar::new()});
        let worker=shared.clone();
        thread::spawn(move || {
            let outcome=catch_unwind(AssertUnwindSafe(|| {
                let mut checkpoint=Checkpoint {shared:worker.clone(),calls:101};
                checkpoint.wait()?;
                match strategy {
                    Strategy::Full=>registry.report_controlled(&inventory,&target,amount,&mut ||checkpoint.wait()),
                    Strategy::CraftLess=>registry.report_less_controlled(&inventory,&target,amount,&mut ||checkpoint.wait()),
                }
            })).unwrap_or(Err(Error::WorkerPanicked));
            let mut state=worker.state.lock().unwrap_or_else(|e|e.into_inner());
            state.result=Some(outcome);
            state.done=true;
            state.running=false;
            worker.wake.notify_all();
        });
        Self {shared}
    }

    /// Run one slice, waiting only until this worker yields or finishes.
    /// The caller divides the configured total budget across active jobs.
    /// Returns true while further slices are required.
    pub fn run_slice(&mut self,micros:u64)->bool {
        let mut state=self.shared.state.lock().unwrap_or_else(|e|e.into_inner());
        if state.done || state.cancelled {return false;}
        state.deadline=Instant::now()+Duration::from_micros(micros.max(1));
        state.running=true;
        self.shared.wake.notify_all();
        while state.running && !state.done && !state.cancelled {
            state=self.shared.wake.wait(state).unwrap_or_else(|e|e.into_inner());
        }
        !state.done && !state.cancelled
    }

    /// Retrieve the result once, only after the calculation has completed.
    pub fn take_result(&mut self)->Option<Outcome> {
        let mut state=self.shared.state.lock().unwrap_or_else(|e|e.into_inner());
        if state.done {state.result.take()} else {None}
    }

    pub fn cancel(&mut self) {
        let mut state=self.shared.state.lock().unwrap_or_else(|e|e.into_inner());
        state.cancelled=true;
        state.running=false;
        self.shared.wake.notify_all();
    }
}

impl Drop for Calculation {
    fn drop(&mut self) {self.cancel();}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extended::{Ingredient,Pattern};

    #[test]
    fn calculation_starts_paused_and_returns_the_same_snapshot_report() {
        let mut registry=Registry::default();
        registry.register(Pattern {
            id:"board".into(),
            inputs:vec![Ingredient {choices:Inventory::from([("log".into(),1)])}],
            outputs:Inventory::from([("board".into(),4)]),
        }).unwrap();
        let inventory=Inventory::from([("log".into(),2)]);
        let expected=registry.report(&inventory,"board",8).unwrap();
        let mut work=Calculation::new(registry,inventory.clone(),"board".into(),8);
        assert!(work.take_result().is_none());
        while work.run_slice(10_000) {}
        assert_eq!(work.take_result().unwrap().unwrap(),expected);
        assert_eq!(inventory,Inventory::from([("log".into(),2)]));
        assert!(work.take_result().is_none());
    }

    #[test]
    fn cancellation_propagates_through_alternative_branches() {
        let mut registry=Registry::default();
        for id in ["first","second"] {
            registry.register(Pattern {
                id:id.into(),
                inputs:vec![Ingredient {choices:Inventory::from([("log".into(),1),("wood".into(),1)])}],
                outputs:Inventory::from([("board".into(),1)]),
            }).unwrap();
        }
        let mut calls=0;
        let result=registry.report_controlled(&Inventory::new(),"board",2,&mut || {
            calls+=1;
            if calls==4 {Err(Error::Cancelled)} else {Ok(())}
        });
        assert_eq!(result,Err(Error::Cancelled));
    }

    #[test]
    fn definition_recheck_detects_changed_patterns_and_resource_scales() {
        let mut registry=Registry::default();
        registry.register(Pattern {
            id:"fill".into(),
            inputs:vec![Ingredient {choices:Inventory::from([("water".into(),1000)])}],
            outputs:Inventory::from([("filled".into(),1)]),
        }).unwrap();
        let snapshot=registry.clone();
        assert!(snapshot.same_definitions(&registry));
        let mut changed=registry.clone();
        changed.register(Pattern {
            id:"other".into(),
            inputs:vec![Ingredient {choices:Inventory::from([("ore".into(),1)])}],
            outputs:Inventory::from([("ingot".into(),1)]),
        }).unwrap();
        assert!(!snapshot.same_definitions(&changed));
        registry.set_resource_classifier(|key|if key=="water" {
            crate::extended::ResourceKind::Fluid
        } else {crate::extended::ResourceKind::Item});
        assert!(!snapshot.same_definitions(&registry));
    }

    #[test]
    fn craft_less_returns_largest_real_amount_or_a_non_submittable_simulation() {
        let mut registry=Registry::default();
        registry.register(Pattern {
            id:"board".into(),
            inputs:vec![Ingredient {choices:Inventory::from([("log".into(),1)])}],
            outputs:Inventory::from([("board".into(),4)]),
        }).unwrap();
        let inventory=Inventory::from([("log".into(),3)]);
        let mut work=Calculation::with_strategy(registry.clone(),inventory,"board".into(),16,Strategy::CraftLess);
        while work.run_slice(10_000) {}
        let (plan,missing)=work.take_result().unwrap().unwrap();
        assert_eq!(plan.amount,12);
        assert_eq!(plan.reserved,Inventory::from([("log".into(),3)]));
        assert!(!plan.simulated && missing.is_empty());
        let mut no_inputs=Calculation::with_strategy(registry,Inventory::new(),"board".into(),16,Strategy::CraftLess);
        while no_inputs.run_slice(10_000) {}
        let (plan,missing)=no_inputs.take_result().unwrap().unwrap();
        assert!(plan.simulated);
        assert_eq!(plan.amount,16);
        assert_eq!(missing,Inventory::from([("log".into(),4)]));
        assert_eq!(crate::extended::Cpu::new(1024,0).submit(plan,&mut Inventory::new()),Err(Error::SimulatedPlan));
    }

    #[test]
    fn long_calculation_can_yield_and_resume_without_losing_state() {
        let mut registry=Registry::default();
        registry.register(Pattern {
            id:"many".into(),
            inputs:(0..80).map(|_|Ingredient {choices:Inventory::from([("ore".into(),1)])}).collect(),
            outputs:Inventory::from([("result".into(),1)]),
        }).unwrap();
        let inventory=Inventory::from([("ore".into(),80)]);
        let expected=registry.report(&inventory,"result",1).unwrap();
        let mut work=Calculation::new(registry,inventory,"result".into(),1);
        assert!(work.run_slice(1),"the worker should yield between recursive requests");
        while work.run_slice(10_000) {}
        assert_eq!(work.take_result().unwrap().unwrap(),expected);
    }
}
