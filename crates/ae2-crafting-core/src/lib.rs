//! Experimental crafting core inspired by AE2 v19.2.18, particularly
//! `appeng/crafting/CraftingCalculation.java` and `execution/CraftingCpuLogic.java`.
//! AE2 is LGPL-3.0-or-later, https://github.com/AppliedEnergistics/Applied-Energistics-2.
//! This is not yet behaviorally equivalent to AE2 and does not access a world.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub mod provider_registry;
pub mod provider_lifecycle;
pub mod emittable_lifecycle;
pub mod cpu_formation;
pub mod calculation;

pub type Inventory = BTreeMap<String, u64>;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pattern { pub id: String, pub output: String, pub output_count: u64, pub inputs: Inventory }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step { pub pattern: String, pub inputs: Inventory, pub output: String, pub count: u64 }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan { pub reserved: Inventory, pub steps: Vec<Step>, pub target: String, pub count: u64 }
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error { InvalidPattern, DuplicateOutput(String), Missing(String, u64), Cycle(String), Overflow, InventoryChanged(String), Busy, NotWaiting, NoSteps, WrongOutput, AlreadyFinished, SearchLimit, SimulatedPlan, Cancelled, WorkerPanicked, InsufficientCpuBytes { required:u64, available:u64 } }

fn add(map: &mut Inventory, key: &str, n: u64) -> Result<(), Error> {
    let value = map.get(key).copied().unwrap_or(0).checked_add(n).ok_or(Error::Overflow)?;
    if value > 0 { map.insert(key.to_owned(), value); }
    Ok(())
}
fn take(map: &mut Inventory, key: &str, n: u64) -> bool {
    let available = map.get(key).copied().unwrap_or(0);
    if available < n { return false; }
    if available == n { map.remove(key); } else { map.insert(key.to_owned(), available - n); }
    true
}
#[derive(Default)]
pub struct Registry { patterns: BTreeMap<String, Pattern>, ids: BTreeSet<String> }
impl Registry {
    pub fn register(&mut self, pattern: Pattern) -> Result<(), Error> {
        if pattern.id.is_empty() || pattern.output.is_empty() || pattern.output_count == 0 || pattern.inputs.is_empty() || pattern.inputs.iter().any(|(k,v)| k.is_empty() || *v == 0) || self.ids.contains(&pattern.id) { return Err(Error::InvalidPattern); }
        if self.patterns.contains_key(&pattern.output) { return Err(Error::DuplicateOutput(pattern.output)); }
        self.ids.insert(pattern.id.clone()); self.patterns.insert(pattern.output.clone(), pattern); Ok(())
    }
    /// Plans without mutating storage. A submission must recheck the reservation.
    pub fn plan(&self, inventory: &Inventory, target: &str, count: u64) -> Result<Plan, Error> {
        if target.is_empty() || count == 0 { return Err(Error::InvalidPattern); }
        // CraftingCalculation.runCraftAttempt ignores the output in the
        // network: the request is for newly crafted items, not existing stock.
        let mut original=inventory.clone();
        original.remove(target);
        let mut p = Planner { registry: self, original, generated: Inventory::new(), reserved: Inventory::new(), steps: vec![], visiting: BTreeSet::new() };
        p.require(target, count)?;
        Ok(Plan { reserved: p.reserved, steps: p.steps, target: target.into(), count })
    }
}
struct Planner<'a> { registry: &'a Registry, original: Inventory, generated: Inventory, reserved: Inventory, steps: Vec<Step>, visiting: BTreeSet<String> }
impl Planner<'_> {
    fn require(&mut self, key: &str, count: u64) -> Result<(), Error> {
        let mut needed = count;
        let from_generated = self.generated.get(key).copied().unwrap_or(0).min(needed);
        take(&mut self.generated, key, from_generated); needed -= from_generated;
        let from_original = self.original.get(key).copied().unwrap_or(0).min(needed);
        take(&mut self.original, key, from_original); add(&mut self.reserved, key, from_original)?; needed -= from_original;
        if needed == 0 { return Ok(()); }
        if !self.visiting.insert(key.into()) { return Err(Error::Cycle(key.into())); }
        let pattern = self.registry.patterns.get(key).ok_or_else(|| Error::Missing(key.into(), needed))?.clone();
        let runs = needed / pattern.output_count + u64::from(needed % pattern.output_count != 0);
        let mut inputs = Inventory::new();
        for (ingredient, amount) in &pattern.inputs {
            let required = amount.checked_mul(runs).ok_or(Error::Overflow)?;
            self.require(ingredient, required)?; inputs.insert(ingredient.clone(), required);
        }
        let produced = pattern.output_count.checked_mul(runs).ok_or(Error::Overflow)?;
        self.steps.push(Step { pattern: pattern.id, inputs, output: key.into(), count: produced });
        add(&mut self.generated, key, produced - needed)?;
        self.visiting.remove(key); Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job { plan: Plan, escrow: Inventory, next: usize, waiting: Option<Step>, finished: bool }
impl Job {
    /// Atomically checks and removes inputs; persist job and inventory atomically in the host.
    pub fn submit(plan: Plan, inventory: &mut Inventory) -> Result<Self, Error> {
        for (k,v) in &plan.reserved { if inventory.get(k).copied().unwrap_or(0) < *v { return Err(Error::InventoryChanged(k.clone())); } }
        for (k,v) in &plan.reserved { take(inventory, k, *v); }
        Ok(Self { escrow: plan.reserved.clone(), plan, next: 0, waiting: None, finished: false })
    }
    /// Remove ingredients from escrow and hand them to a machine. Persist before dispatch;
    /// after a crash reconcile the waiting step with the machine, never blindly dispatch twice.
    pub fn dispatch(&mut self) -> Result<Step, Error> {
        if self.finished { return Err(Error::AlreadyFinished); }
        if self.waiting.is_some() { return Err(Error::Busy); }
        let step = self.plan.steps.get(self.next).cloned().ok_or(Error::NoSteps)?;
        for (k,v) in &step.inputs { if self.escrow.get(k).copied().unwrap_or(0) < *v { return Err(Error::InventoryChanged(k.clone())); } }
        for (k,v) in &step.inputs { take(&mut self.escrow, k, *v); }
        self.waiting = Some(step.clone()); Ok(step)
    }
    /// Machine must confirm exact expected output; external inventories must be reconciled by host.
    pub fn complete_step(&mut self, output: &str, count: u64) -> Result<(), Error> {
        let step = self.waiting.as_ref().ok_or(Error::NotWaiting)?;
        if output != step.output || count != step.count { return Err(Error::WrongOutput); }
        add(&mut self.escrow, output, count)?; self.waiting = None; self.next += 1; Ok(())
    }
    /// Recover a rejected machine dispatch only after host confirms inputs were returned.
    pub fn reject_step(&mut self) -> Result<(), Error> {
        let step = self.waiting.as_ref().ok_or(Error::NotWaiting)?;
        let mut escrow = self.escrow.clone(); for (k,v) in &step.inputs { add(&mut escrow,k,*v)?; }
        self.escrow = escrow; self.waiting = None; Ok(())
    }
    /// Complete returns all produced and surplus items to the caller's storage.
    pub fn finish(&mut self, inventory: &mut Inventory) -> Result<(), Error> {
        if self.finished { return Err(Error::AlreadyFinished); }
        if self.waiting.is_some() { return Err(Error::Busy); }
        if self.next != self.plan.steps.len() { return Err(Error::NoSteps); }
        self.release(inventory)
    }
    /// Cancel only after the in-flight machine has been reconciled.
    pub fn cancel(&mut self, inventory: &mut Inventory) -> Result<(), Error> {
        if self.finished { return Err(Error::AlreadyFinished); }
        if self.waiting.is_some() { return Err(Error::Busy); }
        self.release(inventory)
    }
    fn release(&mut self, inventory: &mut Inventory) -> Result<(), Error> {
        let mut result = inventory.clone(); for (k,v) in &self.escrow { add(&mut result,k,*v)?; }
        *inventory = result; self.escrow.clear(); self.finished = true; Ok(())
    }
    pub fn waiting(&self) -> Option<&Step> { self.waiting.as_ref() }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inv(pairs: &[(&str,u64)]) -> Inventory { pairs.iter().map(|(k,v)| (k.to_string(),*v)).collect() }
    fn pattern(id: &str, out: &str, n: u64, ins: &[(&str,u64)]) -> Pattern { Pattern { id: id.into(), output: out.into(), output_count: n, inputs: inv(ins) } }
    #[test]
    fn nested_batches_surplus_and_persistence() {
        let mut r=Registry::default();
        r.register(pattern("wire", "wire", 4, &[("copper",1)])).unwrap();
        r.register(pattern("circuit", "circuit", 1, &[("wire",3),("iron",1)])).unwrap();
        let mut storage=inv(&[("copper",2),("iron",2)]);
        let plan=r.plan(&storage,"circuit",2).unwrap();
        assert_eq!(plan.reserved, inv(&[("copper",2),("iron",2)]));
        let mut job=Job::submit(plan,&mut storage).unwrap();
        let s=job.dispatch().unwrap(); assert_eq!(s.count,8);
        let encoded=serde_json::to_string(&job).unwrap();
        let mut job: Job=serde_json::from_str(&encoded).unwrap();
        assert_eq!(job.dispatch(), Err(Error::Busy));
        job.complete_step("wire",8).unwrap();
        let s=job.dispatch().unwrap(); assert_eq!(s.inputs,inv(&[("wire",6),("iron",2)]));
        job.complete_step("circuit",2).unwrap(); job.finish(&mut storage).unwrap();
        assert_eq!(storage,inv(&[("circuit",2),("wire",2)]));
        assert_eq!(job.finish(&mut storage),Err(Error::AlreadyFinished));
    }
    #[test]
    fn missing_cycle_and_atomic_submit() {
        let mut r=Registry::default(); r.register(pattern("a","a",1,&[("b",1)])).unwrap();
        assert_eq!(r.plan(&Inventory::new(),"a",1),Err(Error::Missing("b".into(),1)));
        r.register(pattern("b","b",1,&[("a",1)])).unwrap();
        assert_eq!(r.plan(&Inventory::new(),"a",1),Err(Error::Cycle("a".into())));
        let p=r.plan(&inv(&[("b",1)]),"a",1).unwrap();
        let mut changed=inv(&[("other",1)]); let before=changed.clone();
        assert_eq!(Job::submit(p,&mut changed),Err(Error::InventoryChanged("b".into()))); assert_eq!(changed,before);
    }
    #[test]
    fn cancellation_requires_machine_reconciliation() {
        let mut r=Registry::default(); r.register(pattern("p","out",1,&[("in",2)])).unwrap();
        let mut storage=inv(&[("in",2)]); let mut j=Job::submit(r.plan(&storage,"out",1).unwrap(),&mut storage).unwrap();
        j.dispatch().unwrap(); assert_eq!(j.cancel(&mut storage),Err(Error::Busy));
        assert_eq!(j.complete_step("out",2),Err(Error::WrongOutput));
        j.reject_step().unwrap(); j.cancel(&mut storage).unwrap(); assert_eq!(storage,inv(&[("in",2)]));
    }
    #[test]
    fn preexisting_target_and_overflow() {
        let mut r=Registry::default(); r.register(pattern("p","x",1,&[("y",2)])).unwrap();
        let mut storage=inv(&[("x",5),("y",6)]);
        let plan=r.plan(&storage,"x",3).unwrap();
        assert_eq!(plan.reserved,inv(&[("y",6)]));
        let mut j=Job::submit(plan,&mut storage).unwrap();
        j.dispatch().unwrap(); j.complete_step("x",3).unwrap(); j.finish(&mut storage).unwrap();
        assert_eq!(storage,inv(&[("x",8)]));
        assert_eq!(r.plan(&Inventory::new(),"x",u64::MAX),Err(Error::Overflow));
    }
}

/// Extended experimental planner and multi-operation CPU. Original API remains available.
pub mod extended {
    mod source_plan;
    use super::{add, take, Error, Inventory};
    use serde::{Deserialize, Serialize};
    use std::collections::{BTreeMap, BTreeSet};

    /// AE2's native item and fluid key types have different storage-byte scales.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum ResourceKind { Item, Fluid }
    type NodePath = usize;

    /// Source input identity and its physical remainder for each permitted key.
    /// `encoded` is the original pattern key used to condense equal slots;
    /// `primary` is the first source template after any fluid conversion.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct SourceInput {
        pub encoded:String,
        pub primary:String,
        pub remaining:BTreeMap<String,String>,
    }

    /// One input slot: any one option can satisfy the whole slot per run.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Ingredient { pub choices: Inventory }
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Pattern { pub id: String, pub inputs: Vec<Ingredient>, pub outputs: Inventory }
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Step { pub pattern: String, pub inputs: Inventory, pub outputs: Inventory, pub runs: u64 }
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Plan {
        pub target: String, pub amount: u64, pub reserved: Inventory,
        #[serde(default)] pub emitted: Inventory,
        #[serde(default)] pub simulated: bool,
        pub steps: Vec<Step>, pub estimated_bytes: u64,
    }
    #[derive(Clone, Default)]
    pub struct Registry {
        patterns: Vec<Pattern>,
        ids: BTreeSet<String>,
        primary_outputs: BTreeMap<String, String>,
        emittable: BTreeSet<String>,
        resource_classifier: Option<fn(&str)->ResourceKind>,
        source_inputs: BTreeMap<String,Vec<SourceInput>>,
        // Encoded per-slot GenericStack amounts. Source crafting patterns use
        // their sum as the condensed input multiplier independently from the
        // first slot's candidate template amount.
        source_multipliers: BTreeMap<String,Vec<u64>>,
        // Source possible-input arrays can repeat the same key at different
        // template amounts; a map of keys alone cannot represent this.
        source_templates: BTreeMap<String,Vec<Vec<(String,u64)>>>,
        source_outputs: BTreeMap<String,BTreeSet<String>>,
    }
    impl Registry {
        pub fn plan(&self,inventory:&Inventory,target:&str,amount:u64)->Result<Plan,Error> {
            self.source_plan_controlled(inventory,target,amount,&mut ||Ok(()))
        }
        pub fn report(&self,inventory:&Inventory,target:&str,amount:u64)->Result<(Plan,Inventory),Error> {
            self.source_report_controlled(inventory,target,amount,&mut ||Ok(()))
        }
        pub fn report_controlled(&self,inventory:&Inventory,target:&str,amount:u64,checkpoint:&mut dyn FnMut()->Result<(),Error>)->Result<(Plan,Inventory),Error> {
            self.source_report_controlled(inventory,target,amount,checkpoint)
        }
        pub fn report_less_controlled(&self,inventory:&Inventory,target:&str,amount:u64,checkpoint:&mut dyn FnMut()->Result<(),Error>)->Result<(Plan,Inventory),Error> {
            self.source_report_less_controlled(inventory,target,amount,checkpoint)
        }
        pub fn register(&mut self, p: Pattern) -> Result<(), Error> {
            if p.id.is_empty() || self.ids.contains(&p.id) || p.inputs.is_empty() || p.outputs.is_empty()
                || p.outputs.iter().any(|(k,v)| k.is_empty() || *v == 0)
                || p.inputs.iter().any(|i| i.choices.is_empty() || i.choices.iter().any(|(k,v)| k.is_empty() || *v == 0)) { return Err(Error::InvalidPattern); }
            self.ids.insert(p.id.clone()); self.patterns.push(p); Ok(())
        }
        /// Index a source definition only by its declared primary output. Its
        /// other outputs still enter the plan as physical byproducts. Existing
        /// callers of `register` retain the legacy all-output lookup behavior.
        pub fn register_primary(&mut self, p: Pattern, primary: String) -> Result<(), Error> {
            if !p.outputs.contains_key(&primary) { return Err(Error::InvalidPattern); }
            let id = p.id.clone();
            self.register(p)?;
            self.primary_outputs.insert(id, primary);
            Ok(())
        }
        /// The host identifies resource types from its canonical key codec.
        /// Without a classifier, standalone legacy callers use item units.
        pub fn set_resource_classifier(&mut self,classify:fn(&str)->ResourceKind) {
            self.resource_classifier=Some(classify);
        }
        /// Whether a previously calculated plan still names the same provider
        /// definitions, substitution choices, emitted keys and byte scales.
        /// Physical stock and CPU availability are rechecked separately.
        pub fn same_definitions(&self,other:&Self)->bool {
            let same_kind=|key:&str| {
                self.resource_classifier.map_or(ResourceKind::Item,|f|f(key))
                    ==other.resource_classifier.map_or(ResourceKind::Item,|f|f(key))
            };
            self.patterns==other.patterns && self.primary_outputs==other.primary_outputs
                && self.emittable==other.emittable && self.source_inputs==other.source_inputs
                && self.source_multipliers==other.source_multipliers && self.source_templates==other.source_templates
                && self.source_outputs==other.source_outputs
                && self.patterns.iter().all(|p|p.outputs.keys().all(|key|same_kind(key))
                    && p.inputs.iter().all(|i|i.choices.keys().all(|key|same_kind(key))))
                && self.source_inputs.values().flatten().all(|input|
                    input.remaining.iter().all(|(key,remainder)|same_kind(key) && same_kind(remainder)))
                && self.emittable.iter().all(|key|same_kind(key))
        }
        fn stack_millibytes(&self,key:&str,amount:u64)->u128 {
            let kind=self.resource_classifier.map_or(ResourceKind::Item,|classify|classify(key));
            u128::from(amount)*match kind {ResourceKind::Item=>1000,ResourceKind::Fluid=>1}
        }
        /// Attach source input identities without changing persisted jobs or
        /// their physical slot-level dispatch descriptions.
        pub fn set_source_inputs(&mut self,id:&str,inputs:Vec<SourceInput>)->Result<(),Error> {
            let pattern=self.patterns.iter().find(|p|p.id==id).ok_or(Error::InvalidPattern)?;
            if inputs.len()!=pattern.inputs.len() || inputs.iter().zip(&pattern.inputs).any(|(source,input)|
                source.encoded.is_empty() || source.primary.is_empty() || !input.choices.contains_key(&source.primary)
                    || source.remaining.iter().any(|(choice,remainder)|!input.choices.contains_key(choice)||remainder.is_empty())) {
                return Err(Error::InvalidPattern);
            }
            self.source_inputs.insert(id.to_owned(),inputs);Ok(())
        }
        /// Preserve the source per-slot amounts used to condense crafting
        /// inputs. These are independent of the candidate template amounts.
        pub fn set_source_multipliers(&mut self,id:&str,multipliers:Vec<u64>)->Result<(),Error> {
            let pattern=self.patterns.iter().find(|p|p.id==id).ok_or(Error::InvalidPattern)?;
            if multipliers.len()!=pattern.inputs.len() || multipliers.iter().any(|amount|*amount==0) {return Err(Error::InvalidPattern);}
            self.source_multipliers.insert(id.to_owned(),multipliers);Ok(())
        }
        /// Retain the source's ordered possible templates, including the
        /// same key at two different amounts (e.g. encoded item and substitute).
        pub fn set_source_templates(&mut self,id:&str,templates:Vec<Vec<(String,u64)>>)->Result<(),Error> {
            let pattern=self.patterns.iter().find(|p|p.id==id).ok_or(Error::InvalidPattern)?;
            let inputs=self.source_inputs.get(id).ok_or(Error::InvalidPattern)?;
            if templates.len()!=pattern.inputs.len() || templates.iter().zip(&pattern.inputs).zip(inputs).any(|((templates,input),source)| {
                templates.first().is_none_or(|(key,amount)|key!=&source.primary || input.choices.get(key)!=Some(amount))
                    || templates.iter().any(|(key,amount)|*amount==0 || !input.choices.contains_key(key))
            }) {return Err(Error::InvalidPattern);}
            self.source_templates.insert(id.to_owned(),templates);Ok(())
        }
        /// Source pattern outputs exclude crafting-container remainders,
        /// which the host tracks separately as physical returned items.
        pub fn set_source_outputs(&mut self,id:&str,keys:impl IntoIterator<Item=String>)->Result<(),Error> {
            let pattern=self.patterns.iter().find(|p|p.id==id).ok_or(Error::InvalidPattern)?;
            let keys:BTreeSet<_>=keys.into_iter().collect();
            if keys.is_empty() || keys.iter().any(|key|!pattern.outputs.contains_key(key)) {return Err(Error::InvalidPattern);}
            self.source_outputs.insert(id.to_owned(),keys);Ok(())
        }
        fn source_nonrecursive(&self,pattern:&Pattern,key:&str,visiting:&BTreeSet<String>)->bool {
            let ancestor=|candidate:&str|candidate!=key && visiting.contains(candidate);
            let output_matches=match self.source_outputs.get(&pattern.id) {
                Some(keys)=>keys.iter().any(|output|ancestor(output)),
                None=>pattern.outputs.keys().any(|output|ancestor(output)),
            };
            let input_matches=match self.source_inputs.get(&pattern.id) {
                Some(inputs)=>inputs.iter().enumerate().any(|(slot,input)|self.source_group(&pattern.id,slot)==slot && ancestor(&input.primary)),
                None=>pattern.inputs.iter().any(|input|input.choices.keys().next().is_some_and(|input|ancestor(input))),
            };
            !output_matches && !input_matches
        }
        fn source_group(&self,pattern_id:&str,slot:usize)->usize {
            let Some(inputs)=self.source_inputs.get(pattern_id) else{return slot;};
            let Some(input)=inputs.get(slot) else{return slot;};
            inputs.iter().position(|other|other.encoded==input.encoded).unwrap_or(slot)
        }
        fn has_container_items(&self,pattern:&Pattern)->bool {
            self.source_inputs.get(&pattern.id).is_some_and(|inputs|inputs.iter().enumerate().any(|(slot,input)|self.source_group(&pattern.id,slot)==slot && input.remaining.contains_key(&input.primary)))
        }
        fn limits_quantity(&self,pattern:&Pattern)->bool {
            let source_output=|key:&str|self.source_outputs.get(&pattern.id).map_or_else(||pattern.outputs.contains_key(key),|outputs|outputs.contains(key));
            self.has_container_items(pattern) || match self.source_inputs.get(&pattern.id) {
                Some(inputs)=>inputs.iter().enumerate().any(|(slot,input)|self.source_group(&pattern.id,slot)==slot && source_output(&input.primary)),
                None=>pattern.inputs.iter().any(|input|input.choices.keys().any(|key|source_output(key))),
            }
        }
        /// Snapshot keys from this crafting service's actual provider membership.
        /// Planning records shortages; it never creates or reserves physical items.
        pub fn set_emittable(&mut self, keys: impl IntoIterator<Item=String>) -> Result<(), Error> {
            let keys: BTreeSet<_> = keys.into_iter().collect();
            if keys.contains("") { return Err(Error::InvalidPattern); }
            self.emittable=keys; Ok(())
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Flight { pub step: Step, pub remaining: Inventory }
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Cpu {
        pub capacity_bytes: u64, pub coprocessors: u32,
        plan: Option<Plan>, escrow: Inventory, pending: Vec<Step>, flights: BTreeMap<u64,Flight>, next_id:u64,
        used_ops:[u32;3], cancelled:bool,
        #[serde(default)] emitted_waiting:Inventory,
        #[serde(default)] delivered_target:u64,
    }
    impl Cpu {
        pub fn new(capacity_bytes:u64, coprocessors:u32) -> Self { Self { capacity_bytes,coprocessors,plan:None,escrow:Inventory::new(),pending:vec![],flights:BTreeMap::new(),next_id:0,used_ops:[0;3],cancelled:false,emitted_waiting:Inventory::new(),delivered_target:0 } }
        /// Normalize persisted resource identities atomically across the plan,
        /// physical escrow accounting, pending work, and in-flight work.
        pub fn remap_keys(&mut self, map:impl Fn(&str)->Result<String,String>)->Result<(),String> {
            fn inventory(src:&Inventory,map:&impl Fn(&str)->Result<String,String>)->Result<Inventory,String> {
                let mut out=Inventory::new();for (key,count) in src {let key=map(key)?;let n=out.entry(key).or_default();*n=n.checked_add(*count).ok_or("resource count overflow")?;}Ok(out)
            }
            fn step(s:&mut Step,map:&impl Fn(&str)->Result<String,String>)->Result<(),String> {s.inputs=inventory(&s.inputs,map)?;s.outputs=inventory(&s.outputs,map)?;Ok(())}
            let mut next=self.clone();next.escrow=inventory(&next.escrow,&map)?;
            next.emitted_waiting=inventory(&next.emitted_waiting,&map)?;
            if let Some(plan)=next.plan.as_mut() {plan.target=map(&plan.target)?;plan.reserved=inventory(&plan.reserved,&map)?;plan.emitted=inventory(&plan.emitted,&map)?;for s in &mut plan.steps {step(s,&map)?;}}
            for s in &mut next.pending {step(s,&map)?;}
            for flight in next.flights.values_mut() {step(&mut flight.step,&map)?;flight.remaining=inventory(&flight.remaining,&map)?;}
            *self=next;Ok(())
        }
        pub fn submit(&mut self, mut plan:Plan, network:&mut Inventory) -> Result<(),Error> {
            if self.plan.is_some() { return Err(Error::Busy); }
            if plan.simulated {return Err(Error::SimulatedPlan);}
            if plan.estimated_bytes > self.capacity_bytes { return Err(Error::InsufficientCpuBytes { required:plan.estimated_bytes, available:self.capacity_bytes }); }
            for (k,v) in &plan.reserved { if network.get(k).copied().unwrap_or(0)<*v { return Err(Error::InventoryChanged(k.clone())); } }
            for (k,v) in &plan.reserved { take(network,k,*v); }
            self.escrow=plan.reserved.clone(); self.pending=std::mem::take(&mut plan.steps); self.emitted_waiting=plan.emitted.clone(); self.plan=Some(plan); self.cancelled=false; self.delivered_target=0; Ok(())
        }
        /// Rotate the AE2 three-tick operation budget once per server tick.
        pub fn tick(&mut self) { self.used_ops[2]=self.used_ops[1]; self.used_ops[1]=self.used_ops[0]; self.used_ops[0]=0; }
        /// Remaining successful pushes in the source's three-tick window.
        /// Inspecting the budget or inputs never reserves items or consumes a medium.
        pub fn remaining_operations(&self) -> u32 {
            if self.plan.is_none() || self.cancelled { return 0; }
            let used=self.used_ops.iter().fold(0u32,|a,b|a.saturating_add(*b));
            self.coprocessors.saturating_add(1).saturating_sub(used)
        }
        pub fn task_has_inputs(&self, index:usize) -> bool {
            let Some(s)=self.pending.get(index) else {return false;};
            s.runs > 0 && s.inputs.iter().all(|(k,n)| *n % s.runs == 0
                && self.escrow.get(k).copied().unwrap_or(0) >= *n / s.runs)
                && s.outputs.values().all(|n| *n % s.runs == 0)
        }
        /// Dispatches one individual runnable push from an aggregated task.
        pub fn dispatch(&mut self) -> Result<(u64,Step),Error> { self.dispatch_where(|_|true) }
        /// Select only a task whose provider has an available destination.
        pub fn dispatch_where(&mut self, mut ready:impl FnMut(&Step)->bool) -> Result<(u64,Step),Error> {
            if self.plan.is_none() || self.cancelled { return Err(Error::AlreadyFinished); }
            if self.remaining_operations()==0 { return Err(Error::Busy); }
            let i=self.pending.iter().enumerate().position(|(i,s)|ready(s) && self.task_has_inputs(i)).ok_or(Error::NoSteps)?;
            self.dispatch_task(i)
        }
        /// Commit the specific task selected by the real provider pass. The
        /// caller owns provider iteration; this method never advances it.
        pub fn dispatch_task(&mut self, i:usize) -> Result<(u64,Step),Error> {
            if self.plan.is_none() || self.cancelled { return Err(Error::AlreadyFinished); }
            if self.remaining_operations()==0 { return Err(Error::Busy); }
            if !self.task_has_inputs(i) { return Err(Error::NoSteps); }
            let id=self.next_id; self.next_id=self.next_id.checked_add(1).ok_or(Error::Overflow)?;
            let pending=&self.pending[i];
            let one=Step { pattern:pending.pattern.clone(), inputs:pending.inputs.iter().map(|(k,n)|(k.clone(),n/pending.runs)).collect(), outputs:pending.outputs.iter().map(|(k,n)|(k.clone(),n/pending.runs)).collect(), runs:1 };
            for (k,n) in &one.inputs { take(&mut self.escrow,k,*n); }
            if self.pending[i].runs==1 { self.pending.remove(i); } else {
                let task=&mut self.pending[i]; task.runs-=1;
                for (k,n) in &one.inputs { take(&mut task.inputs,k,*n); }
                for (k,n) in &one.outputs { take(&mut task.outputs,k,*n); }
            }
            self.flights.insert(id,Flight { remaining:one.outputs.clone(),step:one.clone() }); self.used_ops[0]+=1; Ok((id,one))
        }
        /// Accept only expected output, including partial returns; duplicate delivery is rejected.
        pub fn receive(&mut self, id:u64, key:&str, amount:u64) -> Result<(),Error> {
            let flight=self.flights.get(&id).ok_or(Error::NotWaiting)?;
            if amount==0 || flight.remaining.get(key).copied().unwrap_or(0)<amount { return Err(Error::WrongOutput); }
            let mut escrow=self.escrow.clone(); add(&mut escrow,key,amount)?;
            let f=self.flights.get_mut(&id).unwrap(); take(&mut f.remaining,key,amount); self.escrow=escrow;
            if f.remaining.is_empty() { self.flights.remove(&id); } Ok(())
        }
        /// Source `waitingFor`: emitted shortages plus expected physical
        /// outputs of dispatched patterns. Canceled flights stay in native
        /// custody tracking but no longer count as crafting requests.
        pub fn waiting_for(&self) -> Result<Inventory,Error> {
            let mut result=Inventory::new();
            if self.plan.is_none() || self.cancelled {return Ok(result);}
            for (key,count) in &self.emitted_waiting {add(&mut result,key,*count)?;}
            for flight in self.flights.values() {for (key,count) in &flight.remaining {add(&mut result,key,*count)?;}}
            Ok(result)
        }
        pub fn emitted_waiting(&self) -> &Inventory {&self.emitted_waiting}
        /// Accept only physically delivered items/fluids against an emitted
        /// shortage. The host must separately debit the real source once.
        pub fn receive_emitted(&mut self,key:&str,amount:u64) -> Result<(),Error> {
            if self.plan.is_none() || self.cancelled {return Err(Error::AlreadyFinished);}
            if amount==0 || self.emitted_waiting.get(key).copied().unwrap_or(0)<amount {return Err(Error::WrongOutput);}
            let mut escrow=self.escrow.clone();add(&mut escrow,key,amount)?;
            take(&mut self.emitted_waiting,key,amount);self.escrow=escrow;Ok(())
        }
        /// Request cancellation. In-flight materials stay tracked until reconciled; no further dispatch.
        pub fn cancel(&mut self) -> Result<(),Error> { if self.plan.is_none() { return Err(Error::AlreadyFinished); } self.cancelled=true; self.pending.clear(); self.emitted_waiting.clear(); Ok(()) }
        /// Record target items actually accepted by a physical requester.
        /// Intermediate ingredients or in-flight materials may never be debited.
        pub fn deliver_target(&mut self,key:&str,amount:u64)->Result<(),Error> {
            let plan=self.plan.as_ref().ok_or(Error::AlreadyFinished)?;
            if self.cancelled || !self.pending.is_empty() || !self.flights.is_empty() || !self.emitted_waiting.is_empty() {return Err(Error::Busy);}
            if key!=plan.target || amount==0 || amount>plan.amount.saturating_sub(self.delivered_target) || self.escrow.get(key).copied().unwrap_or(0)<amount {return Err(Error::WrongOutput);}
            self.delivered_target=self.delivered_target.checked_add(amount).ok_or(Error::Overflow)?;
            take(&mut self.escrow,key,amount);
            Ok(())
        }
        /// Recover inputs only after the host proves a dispatch was rejected. Must precede outputs.
        pub fn reject(&mut self,id:u64) -> Result<(),Error> {
            let f=self.flights.get(&id).ok_or(Error::NotWaiting)?; if f.remaining!=f.step.outputs { return Err(Error::WrongOutput); }
            let mut escrow=self.escrow.clone(); for (k,n) in &f.step.inputs { add(&mut escrow,k,*n)?; }
            let f=self.flights.remove(&id).unwrap(); self.escrow=escrow; if !self.cancelled { self.pending.push(f.step); } Ok(())
        }
        /// Transfer all local materials atomically after every in-flight operation is reconciled.
        pub fn finish(&mut self,network:&mut Inventory) -> Result<(),Error> {
            if self.plan.is_none() { return Err(Error::AlreadyFinished); }
            if !self.flights.is_empty() || (!self.cancelled && !self.emitted_waiting.is_empty()) { return Err(Error::Busy); }
            if !self.cancelled && !self.pending.is_empty() { return Err(Error::NoSteps); }
            let mut out=network.clone(); for (k,v) in &self.escrow { add(&mut out,k,*v)?; }
            *network=out; self.plan=None; self.escrow.clear(); self.pending.clear(); self.emitted_waiting.clear(); Ok(())
        }
        pub fn flights(&self)->&BTreeMap<u64,Flight> { &self.flights }
        pub fn escrow(&self)->&Inventory { &self.escrow }
        pub fn pending(&self)->&[Step] { &self.pending }
        /// Remaining final output shown by a crafting monitor. Actual output
        /// already received into physical CPU escrow is no longer pending.
        pub fn display_progress(&self)->Option<(&str,u64)> {
            if self.cancelled {return None;}
            let plan=self.plan.as_ref()?;
            let made=self.escrow.get(&plan.target).copied().unwrap_or(0)
                .saturating_add(self.delivered_target);
            let remaining=plan.amount.saturating_sub(made);
            (remaining>0).then_some((plan.target.as_str(),remaining))
        }
        /// Stable job output and outstanding quantity for owner lifecycle
        /// messages, including after cancellation has been requested.
        pub fn requested_output(&self)->Option<(&str,u64,u64)> {
            let plan=self.plan.as_ref()?;
            let made=self.escrow.get(&plan.target).copied().unwrap_or(0).saturating_add(self.delivered_target);
            Some((plan.target.as_str(),plan.amount,plan.amount.saturating_sub(made)))
        }
        pub fn is_cancelled(&self)->bool { self.cancelled }
        pub fn is_active(&self)->bool { self.plan.is_some() }
    }
}

#[cfg(test)]
mod extended_tests {
    use super::{extended::*, Error, Inventory};
    fn inv(pairs:&[(&str,u64)])->Inventory { pairs.iter().map(|(k,v)|(k.to_string(),*v)).collect() }
    fn ing(p:&[(&str,u64)])->Ingredient { Ingredient { choices:inv(p) } }
    #[test]
    fn source_primary_output_indexes_byproducts_only_after_the_primary_is_planned() {
        let pattern=Pattern{id:"source".into(),inputs:vec![ing(&[("ore",1)])],outputs:inv(&[("ingot",1),("slag",1)])};
        let mut r=Registry::default();
        assert_eq!(r.register_primary(pattern.clone(),"absent".into()),Err(Error::InvalidPattern));
        r.register_primary(pattern.clone(),"ingot".into()).unwrap();
        assert_eq!(r.plan(&inv(&[("ore",1)]),"slag",1),Err(Error::Missing("slag".into(),1)));
        r.register_primary(Pattern{id:"combine".into(),inputs:vec![ing(&[("ingot",1)]),ing(&[("slag",1)])],outputs:inv(&[("assembly",1)])},"assembly".into()).unwrap();
        let plan=r.plan(&inv(&[("ore",1)]),"assembly",1).unwrap();
        assert_eq!(plan.steps.iter().map(|s|s.pattern.as_str()).collect::<Vec<_>>(),vec!["source","combine"]);
        let mut legacy=Registry::default();legacy.register(pattern).unwrap();
        assert!(legacy.plan(&inv(&[("ore",1)]),"slag",1).is_ok());
    }
    #[test]
    fn request_crafts_new_output_while_existing_output_remains_in_storage() {
        let mut r=Registry::default();
        r.register(Pattern{id:"p".into(),inputs:vec![ing(&[("ore",1)])],outputs:inv(&[("part",1)])}).unwrap();
        let mut network=inv(&[("part",5),("ore",2)]);
        let (preview,missing)=r.report(&network,"part",2).unwrap();
        assert!(missing.is_empty());
        assert_eq!(preview.reserved,inv(&[("ore",2)]));
        let mut cpu=Cpu::new(1000,1);
        cpu.submit(r.plan(&network,"part",2).unwrap(),&mut network).unwrap();
        assert_eq!(network,inv(&[("part",5)]));
        let (a,_)=cpu.dispatch().unwrap();let (b,_)=cpu.dispatch().unwrap();
        cpu.receive(a,"part",1).unwrap();cpu.receive(b,"part",1).unwrap();
        cpu.finish(&mut network).unwrap();
        assert_eq!(network,inv(&[("part",7)]));
    }
    #[test]
    fn cpu_capacity_counts_source_tree_requested_items_and_operations() {
        let mut r=Registry::default();
        r.register(Pattern{id:"smelt".into(),inputs:vec![ing(&[("ore",1)])],outputs:inv(&[("ingot",1)])}).unwrap();
        let mut network=inv(&[("ore",64)]);
        let plan=r.plan(&network,"ingot",64).unwrap();
        // Source: two tree nodes * 8 + 64 requested output + 64 input + 64 pushes.
        assert_eq!(plan.estimated_bytes,208);
        let mut too_small=Cpu::new(207,0);
        assert_eq!(too_small.submit(plan.clone(),&mut network),Err(Error::InsufficientCpuBytes{required:208,available:207}));
        assert_eq!(network,inv(&[("ore",64)]));
        Cpu::new(208,0).submit(plan,&mut network).unwrap();
        assert!(network.is_empty());
    }
    #[test]
    fn cpu_capacity_sums_fractional_fluid_bytes_before_rounding() {
        let mut r=Registry::default();
        r.set_resource_classifier(|key|if key.starts_with("fluid|"){ResourceKind::Fluid}else{ResourceKind::Item});
        r.register(Pattern{id:"mixed".into(),inputs:vec![ing(&[("iron",2)]),ing(&[("fluid|water|",750)])],outputs:inv(&[("fluid|oil|",200)])}).unwrap();
        let mut network=inv(&[("iron",2),("fluid|water|",750)]);
        let plan=r.plan(&network,"fluid|oil|",200).unwrap();
        // Three nodes * 8 + one push + two items + .75 + .2 fluid bytes = 27.95.
        assert_eq!(plan.estimated_bytes,28);
        assert_eq!(Cpu::new(27,0).submit(plan.clone(),&mut network),Err(Error::InsufficientCpuBytes{required:28,available:27}));
        assert_eq!(network,inv(&[("iron",2),("fluid|water|",750)]));
        Cpu::new(28,0).submit(plan,&mut network).unwrap();
        assert!(network.is_empty());
    }
    #[test]
    fn source_tree_counts_candidate_nodes_once_across_pattern_pushes() {
        let mut r=Registry::default();
        for (id,input) in [("iron_path","iron"),("gold_path","gold")] {
            r.register(Pattern{id:id.into(),inputs:vec![ing(&[(input,1)])],outputs:inv(&[("part",1)])}).unwrap();
        }
        let plan=r.plan(&inv(&[("iron",1),("gold",1)]),"part",2).unwrap();
        // One root + both candidate inputs, not one node per search attempt.
        assert_eq!(plan.estimated_bytes,3*8+2+2+2);
        let mut nested=Registry::default();
        nested.register(Pattern{id:"component".into(),inputs:vec![ing(&[("raw",1)])],outputs:inv(&[("middle",1)])}).unwrap();
        nested.register(Pattern{id:"assembly".into(),inputs:vec![ing(&[("middle",1)])],outputs:inv(&[("final",1)])}).unwrap();
        assert_eq!(nested.plan(&inv(&[("raw",1)]),"final",1).unwrap().estimated_bytes,3*8+3+2);
    }
    #[test]
    fn source_tree_does_not_expand_a_later_path_after_first_path_succeeds() {
        let mut r=Registry::default();
        for (id,input,output) in [("make_a","raw_a","a"),("make_b","raw_b","b"),("first","a","target"),("second","b","target")] {
            r.register(Pattern{id:id.into(),inputs:vec![ing(&[(input,1)])],outputs:inv(&[(output,1)])}).unwrap();
        }
        let plan=r.plan(&inv(&[("raw_a",1),("raw_b",1)]),"target",1).unwrap();
        assert_eq!(plan.reserved,inv(&[("raw_a",1)]));
        // Source builds root and both candidate input nodes, then expands only
        // the first candidate's raw input before returning successfully.
        assert_eq!(plan.estimated_bytes,4*8+3+2);
    }
    #[test]
    fn source_excludes_a_nested_pattern_that_also_outputs_an_ancestor() {
        let mut r=Registry::default();
        r.register_primary(Pattern{id:"recursive".into(),inputs:vec![ing(&[("raw",1)])],outputs:inv(&[("a",1),("target",1)])},"a".into()).unwrap();
        r.register_primary(Pattern{id:"safe".into(),inputs:vec![ing(&[("raw",1)])],outputs:inv(&[("a",1)])},"a".into()).unwrap();
        r.register_primary(Pattern{id:"final".into(),inputs:vec![ing(&[("a",1)])],outputs:inv(&[("target",1)])},"target".into()).unwrap();
        let plan=r.plan(&inv(&[("raw",1)]),"target",1).unwrap();
        assert_eq!(plan.steps.iter().map(|s|s.pattern.as_str()).collect::<Vec<_>>(),vec!["safe","final"]);
        assert_eq!(plan.estimated_bytes,3*8+3+2);
    }
    #[test]
    fn source_recursion_filter_does_not_treat_container_returns_as_pattern_outputs() {
        let mut r=Registry::default();
        r.register_primary(Pattern{id:"make_a".into(),inputs:vec![ing(&[("raw",1)])],outputs:inv(&[("a",1),("target",1)])},"a".into()).unwrap();
        r.set_source_outputs("make_a",["a".into()]).unwrap();
        r.set_source_inputs("make_a",vec![SourceInput{encoded:"raw".into(),primary:"raw".into(),remaining:std::collections::BTreeMap::from([("raw".into(),"target".into())])}]).unwrap();
        r.register_primary(Pattern{id:"final".into(),inputs:vec![ing(&[("a",1)])],outputs:inv(&[("target",1)])},"target".into()).unwrap();
        let plan=r.plan(&inv(&[("raw",1)]),"target",1).unwrap();
        assert_eq!(plan.steps.iter().map(|s|s.pattern.as_str()).collect::<Vec<_>>(),vec!["make_a","final"]);
        assert_eq!(plan.estimated_bytes,3*8+3+1+2);
    }
    #[test]
    fn source_tree_keeps_expanded_nodes_from_an_earlier_failed_path() {
        let mut r=Registry::default();
        for (id,input,output) in [("make_a","raw_a","a"),("make_b","raw_b","b"),("make_c","raw_c","c")] {
            r.register(Pattern{id:id.into(),inputs:vec![ing(&[(input,1)])],outputs:inv(&[(output,1)])}).unwrap();
        }
        r.register(Pattern{id:"first".into(),inputs:vec![ing(&[("a",1)]),ing(&[("b",1)])],outputs:inv(&[("target",1)])}).unwrap();
        r.register(Pattern{id:"second".into(),inputs:vec![ing(&[("c",1)])],outputs:inv(&[("target",1)])}).unwrap();
        let plan=r.plan(&inv(&[("raw_a",1),("raw_c",1)]),"target",1).unwrap();
        assert_eq!(plan.reserved,inv(&[("raw_c",1)]));
        // Root, three candidate inputs and the three lazily expanded raw nodes.
        assert_eq!(plan.estimated_bytes,7*8+3+2);
    }
    #[test]
    fn repeated_source_inputs_share_a_tree_node_without_merging_physical_slots() {
        let mut r=Registry::default();
        r.register(Pattern{id:"p".into(),inputs:vec![ing(&[("ore",1)]),ing(&[("ore",1)])],outputs:inv(&[("plate",1)])}).unwrap();
        r.set_source_inputs("p",vec![SourceInput{encoded:"ore".into(),primary:"ore".into(),remaining:Default::default()};2]).unwrap();
        let plan=r.plan(&inv(&[("ore",2)]),"plate",1).unwrap();
        // AEPatternHelper condenses the two identical slots: two nodes, three
        // requested items and one pattern operation.
        assert_eq!(plan.estimated_bytes,20);
        assert_eq!(plan.reserved,inv(&[("ore",2)]));
        assert_eq!(plan.steps[0].inputs,inv(&[("ore",2)]));
    }
    #[test]
    fn physical_recipe_remainders_are_charged_per_actual_push() {
        let mut r=Registry::default();
        r.register_primary(Pattern{id:"fill".into(),inputs:vec![ing(&[("filled_bucket",1)])],outputs:inv(&[("product",1),("bucket",1)])},"product".into()).unwrap();
        r.set_source_inputs("fill",vec![SourceInput{encoded:"filled_bucket".into(),primary:"filled_bucket".into(),remaining:std::collections::BTreeMap::from([("filled_bucket".into(),"bucket".into())])}]).unwrap();
        let mut network=inv(&[("filled_bucket",2)]);
        let plan=r.plan(&network,"product",2).unwrap();
        // Two tree nodes, two outputs, two inputs, two remainder items, two pushes.
        assert_eq!(plan.estimated_bytes,24);
        assert_eq!(plan.steps.iter().map(|step|step.runs).sum::<u64>(),2);
        let mut cpu=Cpu::new(24,0);cpu.submit(plan,&mut network).unwrap();
        for n in 0..2 {
            if n>0 {for _ in 0..3 {cpu.tick();}}
            let (id,_)=cpu.dispatch().unwrap();cpu.receive(id,"product",1).unwrap();cpu.receive(id,"bucket",1).unwrap();
        }
        cpu.finish(&mut network).unwrap();
        assert_eq!(network,inv(&[("product",2),("bucket",2)]));
    }
    #[test]
    fn repeated_cake_inputs_and_bucket_returns_use_source_bytes() {
        let grid=["milk","milk","milk","sugar","egg","sugar","wheat","wheat","wheat"];
        let mut r=Registry::default();
        r.register_primary(Pattern{id:"cake".into(),inputs:grid.iter().map(|key|ing(&[(key,1)])).collect(),outputs:inv(&[("cake",1),("bucket",3)])},"cake".into()).unwrap();
        r.set_source_outputs("cake",["cake".into()]).unwrap();
        r.set_source_inputs("cake",grid.iter().map(|key|SourceInput{encoded:(*key).into(),primary:(*key).into(),remaining:if *key=="milk" {std::collections::BTreeMap::from([("milk".into(),"bucket".into())])} else {Default::default()}}).collect()).unwrap();
        let plan=r.plan(&inv(&[("milk",6),("sugar",4),("egg",2),("wheat",6)]),"cake",2).unwrap();
        assert_eq!(plan.estimated_bytes,68);
        assert_eq!(plan.steps.iter().map(|step|step.runs).sum::<u64>(),2);
        assert_eq!(plan.reserved,inv(&[("milk",6),("sugar",4),("egg",2),("wheat",6)]));
    }
    #[test]
    fn repeated_pushes_reuse_a_physically_returned_input() {
        let mut r=Registry::default();
        r.register_primary(Pattern{id:"reusable".into(),inputs:vec![ing(&[("container",1)])],outputs:inv(&[("product",1),("container",1)])},"product".into()).unwrap();
        let mut network=inv(&[("container",1)]);
        let plan=r.plan(&network,"product",2).unwrap();
        assert_eq!(plan.reserved,inv(&[("container",1)]));
        assert_eq!(plan.steps.len(),2);
        let mut cpu=Cpu::new(1000,0);cpu.submit(plan,&mut network).unwrap();
        let (first,_)=cpu.dispatch().unwrap();cpu.receive(first,"product",1).unwrap();cpu.receive(first,"container",1).unwrap();
        for _ in 0..3 {cpu.tick();}
        let (second,_)=cpu.dispatch().unwrap();cpu.receive(second,"product",1).unwrap();cpu.receive(second,"container",1).unwrap();
        cpu.finish(&mut network).unwrap();assert_eq!(network,inv(&[("product",2),("container",1)]));
    }
    #[test]
    fn cpu_task_inspection_is_non_consuming_and_exact_dispatch_honors_budget() {
        let mut r=Registry::default();
        r.register(Pattern{id:"first".into(),inputs:vec![ing(&[("iron",1)])],outputs:inv(&[("part",1)])}).unwrap();
        r.register(Pattern{id:"second".into(),inputs:vec![ing(&[("part",1)])],outputs:inv(&[("result",1)])}).unwrap();
        let mut network=inv(&[("iron",1)]);let plan=r.plan(&network,"result",1).unwrap();
        let mut cpu=Cpu::new(1024,0);cpu.submit(plan,&mut network).unwrap();
        let before=cpu.clone();assert_eq!(cpu.remaining_operations(),1);assert!(cpu.task_has_inputs(0));assert!(!cpu.task_has_inputs(1));
        assert_eq!(cpu,before);assert_eq!(cpu.dispatch_task(1),Err(Error::NoSteps));assert_eq!(cpu,before);
        let (id,step)=cpu.dispatch_task(0).unwrap();assert_eq!(step.pattern,"first");assert_eq!(cpu.remaining_operations(),0);
        cpu.receive(id,"part",1).unwrap();assert_eq!(cpu.dispatch_task(0),Err(Error::Busy));
        for _ in 0..3 {cpu.tick();}assert!(cpu.task_has_inputs(0));assert_eq!(cpu.dispatch_task(0).unwrap().1.pattern,"second");
    }
    #[test]
    fn emitted_shortage_waits_for_real_receipt_and_survives_reload_and_cancel() {
        let mut r=Registry::default();
        r.register_primary(Pattern{id:"p".into(),inputs:vec![ing(&[("ore",2)])],outputs:inv(&[("diamond",1)])},"diamond".into()).unwrap();
        r.set_emittable(vec!["ore".into()]).unwrap();
        let mut network=inv(&[("ore",1)]);let plan=r.plan(&network,"diamond",2).unwrap();
        assert_eq!(plan.reserved,inv(&[("ore",1)]));assert_eq!(plan.emitted,inv(&[("ore",3)]));
        let mut cpu=Cpu::new(1024,1);cpu.submit(plan,&mut network).unwrap();
        assert!(network.is_empty());assert_eq!(cpu.waiting_for().unwrap(),inv(&[("ore",3)]));
        assert_eq!(cpu.dispatch_task(0),Err(Error::NoSteps));
        assert_eq!(cpu.receive_emitted("ore",4),Err(Error::WrongOutput));
        cpu.receive_emitted("ore",2).unwrap();assert_eq!(cpu.emitted_waiting(),&inv(&[("ore",1)]));
        cpu=serde_json::from_str(&serde_json::to_string(&cpu).unwrap()).unwrap();
        assert_eq!(cpu.waiting_for().unwrap(),inv(&[("ore",1)]));cpu.receive_emitted("ore",1).unwrap();
        let (a,_)=cpu.dispatch_task(0).unwrap();let (b,_)=cpu.dispatch_task(0).unwrap();
        assert_eq!(cpu.waiting_for().unwrap(),inv(&[("diamond",2)]));
        cpu.cancel().unwrap();assert!(cpu.waiting_for().unwrap().is_empty());
        assert_eq!(cpu.finish(&mut network),Err(Error::Busy));
        cpu.receive(a,"diamond",1).unwrap();cpu.receive(b,"diamond",1).unwrap();cpu.finish(&mut network).unwrap();
        assert_eq!(network,inv(&[("diamond",2)]));
    }
    #[test]
    fn emitted_target_is_not_fabricated_and_old_jobs_deserialize() {
        let mut r=Registry::default();r.set_emittable(vec!["diamond".into()]).unwrap();
        let mut network=Inventory::new();let plan=r.plan(&network,"diamond",2).unwrap();
        assert!(plan.steps.is_empty());assert_eq!(plan.emitted,inv(&[("diamond",2)]));
        let mut cpu=Cpu::new(1024,0);cpu.submit(plan,&mut network).unwrap();
        assert_eq!(cpu.finish(&mut network),Err(Error::Busy));assert!(network.is_empty());
        cpu.receive_emitted("diamond",2).unwrap();cpu.finish(&mut network).unwrap();assert_eq!(network,inv(&[("diamond",2)]));
        let old=r.plan(&inv(&[("diamond",1)]),"diamond",1).unwrap();let mut value=serde_json::to_value(old).unwrap();
        value.as_object_mut().unwrap().remove("emitted");
        let restored:Plan=serde_json::from_value(value).unwrap();assert!(restored.emitted.is_empty());
    }
    #[test]
    fn cpu_key_migration_preserves_pending_flights_and_rolls_back_errors() {
        let mut r=Registry::default();r.register(Pattern{id:"one".into(),inputs:vec![ing(&[("old-input",1)])],outputs:inv(&[("old-output",1)])}).unwrap();
        let mut network=inv(&[("old-input",1)]);let plan=r.plan(&network,"old-output",1).unwrap();let mut cpu=Cpu::new(1024,1);cpu.submit(plan,&mut network).unwrap();
        let before=cpu.clone();assert!(cpu.remap_keys(|_|Err("invalid".into())).is_err());assert_eq!(cpu,before);
        let mut in_flight=before.clone();let (flight,_)=in_flight.dispatch().unwrap();in_flight.remap_keys(|s|Ok(s.replace("old-","new-"))).unwrap();in_flight.receive(flight,"new-output",1).unwrap();assert_eq!(in_flight.escrow(),&inv(&[("new-output",1)]));
        cpu.remap_keys(|s|Ok(s.replace("old-","new-"))).unwrap();assert_eq!(cpu.escrow(),&inv(&[("new-input",1)]));
        let (id,step)=cpu.dispatch().unwrap();assert_eq!(step.inputs,inv(&[("new-input",1)]));cpu.receive(id,"new-output",1).unwrap();assert_eq!(cpu.escrow(),&inv(&[("new-output",1)]));
    }
    #[test]
    fn report_explains_multiple_missing_inputs_without_mutating_inventory() {
        let mut r=Registry::default();
        r.register(Pattern{id:"circuit".into(),inputs:vec![ing(&[("iron",2)]),ing(&[("redstone",3)])],outputs:inv(&[("circuit",1)])}).unwrap();
        let network=inv(&[("iron",1)]);
        let (plan,missing)=r.report(&network,"circuit",1).unwrap();
        assert_eq!(network,inv(&[("iron",1)]));
        assert_eq!(missing,inv(&[("iron",1),("redstone",3)]));
        assert_eq!(plan.reserved,inv(&[("iron",1)]));
        assert!(plan.simulated);
        let mut physical=network.clone();
        assert_eq!(Cpu::new(1024,0).submit(plan,&mut physical),Err(Error::SimulatedPlan));
        assert_eq!(physical,network);
        assert!(r.plan(&network,"circuit",1).is_err());
        let (unregistered,missing)=r.report(&network,"unregistered",1).unwrap();
        assert!(unregistered.reserved.is_empty());assert_eq!(missing,inv(&[("unregistered",1)]));
    }
    #[test]
    fn report_uses_source_simulated_first_pattern_after_real_attempt_fails() {
        let mut r=Registry::default();
        for (id,input) in [("iron_path","iron"),("gold_path","gold")] {
            r.register_primary(Pattern{id:id.into(),inputs:vec![ing(&[(input,1)])],outputs:inv(&[("part",1)])},"part".into()).unwrap();
        }
        let network=inv(&[("iron",1),("gold",1)]);
        let (plan,missing)=r.report(&network,"part",3).unwrap();
        assert_eq!(missing,inv(&[("iron",2)]));
        assert_eq!(plan.reserved,inv(&[("iron",1)]));
        assert_eq!(network,inv(&[("iron",1),("gold",1)]));
        assert!(r.plan(&network,"part",3).is_err());
    }
    #[test]
    fn report_prefers_real_alternative_and_reports_pruned_cycle_shortage() {
        let mut r=Registry::default();
        r.register(Pattern{id:"target".into(),inputs:vec![ing(&[("a",1),("b",1)])],outputs:inv(&[("target",1)])}).unwrap();
        let network=inv(&[("b",1)]);
        let (_,missing)=r.report(&network,"target",1).unwrap();
        assert!(missing.is_empty());
        assert_eq!(network,inv(&[("b",1)]));
        let mut cyclic=Registry::default();
        cyclic.register(Pattern{id:"one".into(),inputs:vec![ing(&[("two",1)])],outputs:inv(&[("one",1)])}).unwrap();
        cyclic.register(Pattern{id:"two".into(),inputs:vec![ing(&[("one",1)])],outputs:inv(&[("two",1)])}).unwrap();
        let (_,missing)=cyclic.report(&Inventory::new(),"one",1).unwrap();
        assert_eq!(missing,inv(&[("two",1)]));
        assert!(cyclic.plan(&Inventory::new(),"one",1).is_err());
    }
    #[test]
    fn target_delivery_debits_real_escrow_across_reload() {
        let mut r=Registry::default();
        r.register(Pattern{id:"planks".into(),inputs:vec![ing(&[("log",1)])],outputs:inv(&[("plank",4)])}).unwrap();
        let mut net=inv(&[("log",1)]);let plan=r.plan(&net,"plank",4).unwrap();
        let mut cpu=Cpu::new(1000,0);cpu.submit(plan,&mut net).unwrap();
        assert_eq!(cpu.display_progress(),Some(("plank",4)));
        assert_eq!(cpu.deliver_target("plank",1),Err(Error::Busy));
        let (flight,_)=cpu.dispatch().unwrap();cpu.receive(flight,"plank",4).unwrap();
        assert_eq!(cpu.display_progress(),None);
        cpu.deliver_target("plank",1).unwrap();assert_eq!(cpu.escrow(),&inv(&[("plank",3)]));
        let mut cpu:Cpu=serde_json::from_str(&serde_json::to_string(&cpu).unwrap()).unwrap();
        cpu.deliver_target("plank",2).unwrap();assert_eq!(cpu.deliver_target("plank",2),Err(Error::WrongOutput));
        cpu.deliver_target("plank",1).unwrap();cpu.finish(&mut net).unwrap();assert!(net.is_empty());
    }
    #[test]
    fn alternatives_byproducts_partial_outputs_and_recovery() {
        let mut r=Registry::default();
        r.register(Pattern{id:"process".into(),inputs:vec![ing(&[("ore",1)])],outputs:inv(&[("plate",1),("dust",1)])}).unwrap();
        r.register(Pattern{id:"circuit".into(),inputs:vec![ing(&[("plate",1)]),ing(&[("dust",1),("expensive",1)])],outputs:inv(&[("circuit",1)])}).unwrap();
        let mut network=inv(&[("ore",2)]); let p=r.plan(&network,"circuit",2).unwrap();
        assert_eq!(p.reserved,inv(&[("ore",2)])); assert_eq!(p.steps.len(),2);
        let mut cpu=Cpu::new(1000,1); cpu.submit(p,&mut network).unwrap();
        let (a,s)=cpu.dispatch().unwrap(); assert_eq!(s.outputs,inv(&[("plate",1),("dust",1)]));
        let (b,_)=cpu.dispatch().unwrap(); cpu.receive(a,"plate",1).unwrap();
        let snap=serde_json::to_string(&cpu).unwrap(); let mut cpu:Cpu=serde_json::from_str(&snap).unwrap();
        cpu.receive(a,"dust",1).unwrap(); cpu.receive(b,"plate",1).unwrap(); cpu.receive(b,"dust",1).unwrap();
        for _ in 0..3 { cpu.tick(); } let (c,_)=cpu.dispatch().unwrap(); let (d,_)=cpu.dispatch().unwrap();
        cpu.receive(c,"circuit",1).unwrap(); cpu.receive(d,"circuit",1).unwrap();
        cpu.finish(&mut network).unwrap(); assert_eq!(network,inv(&[("circuit",2)]));
    }
    #[test]
    fn fallback_cycles_capacity_and_machine_cancellation() {
        let mut r=Registry::default();
        r.register(Pattern{id:"cycle".into(),inputs:vec![ing(&[("a",1)])],outputs:inv(&[("a",1)])}).unwrap();
        r.register(Pattern{id:"valid".into(),inputs:vec![ing(&[("raw",1)])],outputs:inv(&[("a",1),("waste",1)])}).unwrap();
        let mut net=inv(&[("raw",1)]); let p=r.plan(&net,"a",1).unwrap();
        let mut small=Cpu::new(0,0); assert!(matches!(small.submit(p.clone(),&mut net),Err(Error::InsufficientCpuBytes {..}))); assert_eq!(net,inv(&[("raw",1)]));
        let mut cpu=Cpu::new(1000,0); cpu.submit(p,&mut net).unwrap(); let (id,_)=cpu.dispatch().unwrap();
        cpu.cancel().unwrap(); assert_eq!(cpu.finish(&mut net),Err(Error::Busy));
        cpu.receive(id,"a",1).unwrap(); assert_eq!(cpu.reject(id),Err(Error::WrongOutput));
        cpu.receive(id,"waste",1).unwrap(); cpu.finish(&mut net).unwrap(); assert_eq!(net,inv(&[("a",1),("waste",1)]));
    }
    #[test]
    fn independent_flights_and_per_cpu_jobs() {
        let mut r=Registry::default();
        r.register(Pattern{id:"a".into(),inputs:vec![ing(&[("x",1)])],outputs:inv(&[("a",1)])}).unwrap();
        r.register(Pattern{id:"b".into(),inputs:vec![ing(&[("y",1)])],outputs:inv(&[("b",1)])}).unwrap();
        r.register(Pattern{id:"c".into(),inputs:vec![ing(&[("a",1)]),ing(&[("b",1)])],outputs:inv(&[("c",1)])}).unwrap();
        let mut net=inv(&[("x",2),("y",2)]); let p=r.plan(&net,"c",1).unwrap();
        let mut c1=Cpu::new(1000,1); let mut c2=Cpu::new(1000,0); c1.submit(p.clone(),&mut net).unwrap(); c2.submit(p,&mut net).unwrap();
        let (a,_)=c1.dispatch().unwrap(); let (b,_)=c1.dispatch().unwrap(); assert_ne!(a,b); assert_eq!(c1.dispatch(),Err(Error::Busy));
        c1.receive(b,"b",1).unwrap(); c1.receive(a,"a",1).unwrap();
        for _ in 0..3 { c1.tick(); } let (id,_)=c1.dispatch().unwrap(); c1.receive(id,"c",1).unwrap(); c1.finish(&mut net).unwrap();
        assert_eq!(net,inv(&[("c",1)])); c2.cancel().unwrap(); c2.finish(&mut net).unwrap(); assert_eq!(net,inv(&[("c",1),("x",1),("y",1)]));
    }
}

#[cfg(test)]
mod choice_backtracking_test {
    use super::{extended::*, Error, Inventory};
    fn inv(p:&[(&str,u64)])->Inventory { p.iter().map(|(k,v)|(k.to_string(),*v)).collect() }
    #[test]
    fn combines_multiple_patterns_when_neither_can_make_the_full_request() {
        let mut r=Registry::default();
        for (id,input) in [("iron_path","iron"),("gold_path","gold")] {
            r.register(Pattern{id:id.into(),inputs:vec![Ingredient{choices:inv(&[(input,1)])}],outputs:inv(&[("part",1)])}).unwrap();
        }
        let mut network=inv(&[("iron",1),("gold",1)]);
        let plan=r.plan(&network,"part",2).unwrap();
        assert_eq!(plan.reserved,network);
        assert_eq!(plan.steps.iter().map(|step|step.pattern.as_str()).collect::<Vec<_>>(),vec!["iron_path","gold_path"]);
        let mut cpu=Cpu::new(1000,1);
        cpu.submit(plan,&mut network).unwrap();
        let (first,_)=cpu.dispatch().unwrap();
        let (second,_)=cpu.dispatch().unwrap();
        cpu.receive(first,"part",1).unwrap();
        cpu.receive(second,"part",1).unwrap();
        cpu.finish(&mut network).unwrap();
        assert_eq!(network,inv(&[("part",2)]));
    }
    #[test]
    fn earlier_pattern_is_used_before_a_later_full_batch() {
        let mut r=Registry::default();
        for (id,input) in [("iron_path","iron"),("gold_path","gold")] {
            r.register(Pattern{id:id.into(),inputs:vec![Ingredient{choices:inv(&[(input,1)])}],outputs:inv(&[("part",1)])}).unwrap();
        }
        let plan=r.plan(&inv(&[("iron",1),("gold",2)]),"part",2).unwrap();
        assert_eq!(plan.reserved,inv(&[("iron",1),("gold",1)]));
        assert_eq!(plan.steps.iter().map(|step|step.pattern.as_str()).collect::<Vec<_>>(),vec!["iron_path","gold_path"]);
    }
    #[test]
    fn incomplete_mixed_request_still_reports_missing_ingredients() {
        let mut r=Registry::default();
        for (id,input) in [("iron_path","iron"),("gold_path","gold")] {
            r.register(Pattern{id:id.into(),inputs:vec![Ingredient{choices:inv(&[(input,1)])}],outputs:inv(&[("part",1)])}).unwrap();
        }
        let (_,missing)=r.report(&inv(&[("gold",1)]),"part",3).unwrap();
        assert!(!missing.is_empty());
        assert_eq!(missing.get("part"),None);
    }
    #[test]
    fn mixed_pattern_amounts_follow_source_greedy_order_before_later_ingredient() {
        let mut r=Registry::default();
        for (id,input) in [("iron_path","iron"),("gold_path","gold")] {
            r.register(Pattern{id:id.into(),inputs:vec![Ingredient{choices:inv(&[(input,1)])}],outputs:inv(&[("part",1)])}).unwrap();
        }
        r.register(Pattern{id:"assembly".into(),inputs:vec![Ingredient{choices:inv(&[("part",2)])},Ingredient{choices:inv(&[("iron",1)])}],outputs:inv(&[("assembly",1)])}).unwrap();
        let stock=inv(&[("iron",2),("gold",1)]);
        assert_eq!(r.plan(&stock,"assembly",1),Err(Error::Missing("iron".into(),1)));
        let (simulated,missing)=r.report(&stock,"assembly",1).unwrap();
        assert!(simulated.simulated);assert_eq!(missing,inv(&[("iron",1)]));
        assert!(r.plan(&inv(&[("iron",3),("gold",1)]),"assembly",1).is_ok());
    }
    #[test]
    fn combines_pattern_paths_with_per_run_input_substitutions() {
        let mut r=Registry::default();
        r.register(Pattern{id:"wood_path".into(),inputs:vec![Ingredient{choices:inv(&[("oak",1),("birch",1)])}],outputs:inv(&[("part",1)])}).unwrap();
        r.register(Pattern{id:"gold_path".into(),inputs:vec![Ingredient{choices:inv(&[("gold",1)])}],outputs:inv(&[("part",1)])}).unwrap();
        let plan=r.plan(&inv(&[("oak",1),("birch",1),("gold",1)]),"part",3).unwrap();
        assert_eq!(plan.reserved,inv(&[("oak",1),("birch",1),("gold",1)]));
        assert_eq!(plan.steps.iter().map(|s|s.runs).sum::<u64>(),3);
    }
    #[test]
    fn source_choice_is_not_backtracked_for_a_mandatory_sibling() {
        let mut r=Registry::default();
        r.register(Pattern{id:"result".into(),inputs:vec![Ingredient{choices:inv(&[("a",1),("b",1)])},Ingredient{choices:inv(&[("a",1)])}],outputs:inv(&[("out",1)])}).unwrap();
        let stock=inv(&[("a",1),("b",1)]);
        assert_eq!(r.plan(&stock,"out",1),Err(Error::Missing("a".into(),1)));
        let (simulated,missing)=r.report(&stock,"out",1).unwrap();
        assert!(simulated.simulated);assert_eq!(missing,inv(&[("a",1)]));
        assert!(r.plan(&inv(&[("a",2),("b",1)]),"out",1).is_ok());
    }
    #[test]
    fn source_does_not_revisit_pattern_after_later_sibling_shortage() {
        let mut r=Registry::default();
        for (id,input) in [("a_from_rare","rare"),("a_from_common","common")] {
            r.register(Pattern{id:id.into(),inputs:vec![Ingredient{choices:inv(&[(input,1)])}],outputs:inv(&[("a",1)])}).unwrap();
        }
        r.register(Pattern{id:"target".into(),inputs:vec![Ingredient{choices:inv(&[("a",1)])},Ingredient{choices:inv(&[("rare",1)])}],outputs:inv(&[("out",1)])}).unwrap();
        let stock=inv(&[("rare",1),("common",1)]);
        assert_eq!(r.plan(&stock,"out",1),Err(Error::Missing("rare".into(),1)));
        let (simulated,missing)=r.report(&stock,"out",1).unwrap();
        assert!(simulated.simulated);assert_eq!(missing,inv(&[("rare",1)]));
        let mut reversed=Registry::default();
        for (id,input) in [("a_from_common","common"),("a_from_rare","rare")] {
            reversed.register(Pattern{id:id.into(),inputs:vec![Ingredient{choices:inv(&[(input,1)])}],outputs:inv(&[("a",1)])}).unwrap();
        }
        reversed.register(Pattern{id:"target".into(),inputs:vec![Ingredient{choices:inv(&[("a",1)])},Ingredient{choices:inv(&[("rare",1)])}],outputs:inv(&[("out",1)])}).unwrap();
        let plan=reversed.plan(&stock,"out",1).unwrap();
        assert!(plan.steps.iter().any(|step|step.pattern=="a_from_common"));
        assert_eq!(plan.reserved,stock);
    }
    #[test]
    fn cyclic_pattern_does_not_hide_noncyclic_alternative() {
        let mut r=Registry::default();
        r.register(Pattern{id:"cycle".into(),inputs:vec![Ingredient{choices:inv(&[("a",1)])}],outputs:inv(&[("a",1)])}).unwrap();
        r.register(Pattern{id:"valid".into(),inputs:vec![Ingredient{choices:inv(&[("raw",1)])}],outputs:inv(&[("a",1)])}).unwrap();
        let plan=r.plan(&inv(&[("raw",1)]),"a",1).unwrap();
        assert_eq!(plan.steps[0].pattern,"valid");
    }
    #[test]
    fn source_mixed_batch_does_not_reassign_inputs_for_a_later_sibling() {
        let mut r=Registry::default();
        r.register(Pattern{id:"make_a".into(),inputs:vec![Ingredient{choices:inv(&[("a_rare",1),("b_common",1)])}],outputs:inv(&[("a",1)])}).unwrap();
        r.register(Pattern{id:"target".into(),inputs:vec![Ingredient{choices:inv(&[("a",2)])},Ingredient{choices:inv(&[("a_rare",1)])}],outputs:inv(&[("out",1)])}).unwrap();
        let stock=inv(&[("a_rare",2),("b_common",1)]);
        assert_eq!(r.plan(&stock,"out",1),Err(Error::Missing("a_rare".into(),1)));
        let (simulated,missing)=r.report(&stock,"out",1).unwrap();
        assert!(simulated.simulated);assert_eq!(missing,inv(&[("a_rare",1)]));
        assert!(r.plan(&inv(&[("a_rare",3),("b_common",1)]),"out",1).is_ok());
    }
    #[test]
    fn large_homogeneous_batch_stays_aggregated() {
        let mut r=Registry::default();
        r.register(Pattern{id:"bulk".into(),inputs:vec![Ingredient{choices:inv(&[("raw",1)])}],outputs:inv(&[("out",1)])}).unwrap();
        let p=r.plan(&inv(&[("raw",1_000_000)]),"out",1_000_000).unwrap();
        assert_eq!(p.steps.len(),1);
        assert_eq!(p.steps[0].runs,1_000_000);
    }
}

#[cfg(test)]
mod split_choice_tests {
    use super::{extended::*, Inventory};
    #[test]
    fn substitutions_can_differ_between_actual_pushes() {
        let mut r=Registry::default();
        r.register(Pattern{id:"planks".into(),inputs:vec![Ingredient{choices:Inventory::from([("birch".into(),1),("oak".into(),1)])}],outputs:Inventory::from([("plank".into(),1)])}).unwrap();
        let mut network=Inventory::from([("birch".into(),1),("oak".into(),1)]);
        let plan=r.plan(&network,"plank",2).unwrap();assert_eq!(plan.steps.len(),2);assert_eq!(plan.reserved,network);
        let mut cpu=Cpu::new(100,1);cpu.submit(plan,&mut network).unwrap();let (a,first)=cpu.dispatch().unwrap();let (b,second)=cpu.dispatch().unwrap();
        assert_ne!(first.inputs,second.inputs);cpu.receive(a,"plank",1).unwrap();cpu.receive(b,"plank",1).unwrap();cpu.finish(&mut network).unwrap();assert_eq!(network.get("plank"),Some(&2));
    }
}

#[cfg(test)]
mod push_budget_tests {
    use super::{extended::*,Error,Inventory};
    #[test]
    fn source_three_tick_budget_counts_each_push_and_skips_unavailable_provider() {
        let mut reg=Registry::default();
        reg.register(Pattern{id:"a".into(),inputs:vec![Ingredient{choices:Inventory::from([("x".into(),1)])}],outputs:Inventory::from([("a".into(),1)])}).unwrap();
        reg.register(Pattern{id:"b".into(),inputs:vec![Ingredient{choices:Inventory::from([("y".into(),1)])}],outputs:Inventory::from([("b".into(),1)])}).unwrap();
        reg.register(Pattern{id:"target".into(),inputs:vec![Ingredient{choices:Inventory::from([("a".into(),1)])},Ingredient{choices:Inventory::from([("b".into(),1)])}],outputs:Inventory::from([("z".into(),1)])}).unwrap();
        let mut net=Inventory::from([("x".into(),1),("y".into(),1)]);let plan=reg.plan(&net,"z",1).unwrap();
        let mut cpu=Cpu::new(100,0);cpu.submit(plan,&mut net).unwrap();
        let (id,step)=cpu.dispatch_where(|s|s.pattern=="b").unwrap();assert_eq!(step.pattern,"b");cpu.receive(id,"b",1).unwrap();
        assert_eq!(cpu.dispatch(),Err(Error::Busy));
        cpu.tick();assert_eq!(cpu.dispatch(),Err(Error::Busy));cpu.tick();assert_eq!(cpu.dispatch(),Err(Error::Busy));cpu.tick();
        assert_eq!(cpu.dispatch().unwrap().1.pattern,"a");
    }
}

#[cfg(test)]
mod mixed_output_tests {
    use super::{extended::*, Error, Inventory};
    #[test]
    fn partial_item_and_two_fluid_outputs_are_tracked_independently() {
        let mut r=Registry::default();
        r.register(Pattern{id:"process".into(), inputs:vec![Ingredient{choices:Inventory::from([("ore".into(),1)])}], outputs:Inventory::from([("ingot".into(),1),("fluid:water".into(),500),("fluid:oil".into(),250)])}).unwrap();
        let mut network=Inventory::from([("ore".into(),1)]);
        let mut cpu=Cpu::new(100,0);
        cpu.submit(r.plan(&network,"ingot",1).unwrap(),&mut network).unwrap();
        let (flight,_)=cpu.dispatch().unwrap();
        cpu.receive(flight,"ingot",1).unwrap();
        cpu.receive(flight,"fluid:water",200).unwrap();
        assert_eq!(cpu.finish(&mut network),Err(Error::Busy));
        assert_eq!(cpu.receive(flight,"fluid:water",301),Err(Error::WrongOutput));
        cpu.receive(flight,"fluid:oil",250).unwrap();
        cpu.receive(flight,"fluid:water",300).unwrap();
        cpu.finish(&mut network).unwrap();
        assert_eq!(network.get("fluid:water"),Some(&500));
        assert_eq!(network.get("fluid:oil"),Some(&250));
        assert_eq!(network.get("ingot"),Some(&1));
    }
}
