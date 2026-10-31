//! Source-shaped traversal of AE2 CraftingTreeNode and CraftingTreeProcess
//! at db17504a86128fdf3dae31f5fb7a112a646e0b93.
//! CPU tasks retain real selected inputs and physical container outputs.
use super::*;
use std::collections::HashMap;

#[derive(Clone,Debug,PartialEq,Eq)]
struct InventoryState {
    original:Inventory,
    available:Inventory,
    required:Inventory,
    emitted:Inventory,
    missing:Inventory,
    steps:Vec<Step>,
    milli:u128,
}
impl InventoryState {
    fn new(inventory:&Inventory,target:&str)->Self {
        let mut original=inventory.clone();original.remove(target);
        Self {available:original.clone(),original,required:Inventory::new(),
            emitted:Inventory::new(),missing:Inventory::new(),steps:Vec::new(),milli:0}
    }
    fn extract(&mut self,key:&str,n:u64)->Result<u64,Error> {
        let n=self.available.get(key).copied().unwrap_or(0).min(n);
        take(&mut self.available,key,n);
        let deficit=self.original.get(key).copied().unwrap_or(0)
            .saturating_sub(self.available.get(key).copied().unwrap_or(0));
        if deficit>self.required.get(key).copied().unwrap_or(0) {
            self.required.insert(key.to_owned(),deficit);
        }
        Ok(n)
    }
    fn insert(&mut self,key:&str,n:u64)->Result<(),Error>{add(&mut self.available,key,n)}
}

struct PendingNode {
    state:Option<InventoryState>, crafted:String, path:NodePath,
    used:Inventory, remainders:Inventory, remaining:u64,
    patterns:Vec<Pattern>, next:usize,
}
struct PendingProcess {
    state:Option<InventoryState>, pattern:Pattern, times:u64, path:NodePath,
    groups:Vec<(usize,u64)>, next:usize,
    inputs:Inventory, returns:Inventory, selections:Vec<(usize,u64,Inventory)>,
}
enum Continuation { Node(PendingNode), Process(PendingProcess) }
enum Outcome {
    Node(Result<(InventoryState,Inventory,Inventory),Error>),
    Process(Result<InventoryState,Error>),
}
enum Action {
    Node{state:InventoryState,key:String,count:u64,parent:Option<(Pattern,usize)>,path:NodePath},
    AdvanceNode(PendingNode),
    Process{state:InventoryState,pattern:Pattern,times:u64,path:NodePath},
    AdvanceProcess(PendingProcess),
    Deliver(Outcome),
}

struct SourcePlanner<'a,'b> {
    registry:&'a Registry,
    nodes:HashMap<(NodePath,String,usize),NodePath>,
    visited:BTreeSet<String>,
    simulated:bool,
    checkpoint:&'b mut dyn FnMut()->Result<(),Error>,
}
impl<'a,'b> SourcePlanner<'a,'b> {
    fn child(&mut self,parent:NodePath,pattern:&str,slot:usize)->Result<NodePath,Error> {
        let key=(parent,pattern.to_owned(),slot);
        if let Some(id)=self.nodes.get(&key) {return Ok(*id);}
        self.nodes.try_reserve(1).map_err(|_|Error::Overflow)?;
        let id=self.nodes.len().checked_add(1).ok_or(Error::Overflow)?;
        self.nodes.insert(key,id);Ok(id)
    }
    fn stack_bytes(&self,key:&str,amount:u64)->u128 {self.registry.stack_millibytes(key,amount)}
    fn add_bytes(&self,state:&mut InventoryState,amount:u128)->Result<(),Error> {
        state.milli=state.milli.checked_add(amount).ok_or(Error::Overflow)?;Ok(())
    }
    fn primary(&self,pattern:&Pattern,slot:usize)->String {
        self.registry.source_inputs.get(&pattern.id).and_then(|v|v.get(slot))
            .map(|v|v.primary.clone())
            .unwrap_or_else(||pattern.inputs[slot].choices.keys().next().unwrap().clone())
    }
    fn group_multiplier(&self,pattern:&Pattern,slot:usize)->u64 {
        if let Some(amount)=self.registry.source_multipliers.get(&pattern.id).and_then(|values|values.get(slot)) {return *amount;}
        let choices=&pattern.inputs[slot].choices;
        if choices.len()!=1 {return 1;}
        let Some((key,count))=choices.iter().next() else{return 1;};
        let is_template=self.registry.source_inputs.get(&pattern.id).and_then(|v|v.get(slot))
            .is_some_and(|meta|meta.encoded!=*key);
        if is_template {1} else {*count}
    }
    fn groups(&self,pattern:&Pattern)->Result<Vec<(usize,u64)>,Error> {
        let mut result=Vec::<(usize,u64)>::new();
        for slot in 0..pattern.inputs.len() {
            let first=self.registry.source_group(&pattern.id,slot);
            let multiplier=self.group_multiplier(pattern,slot);
            if let Some((_,count))=result.iter_mut().find(|(index,_)|*index==first) {*count=count.checked_add(multiplier).ok_or(Error::Overflow)?;}
            else {result.push((first,multiplier));}
        }
        Ok(result)
    }
    fn choices(&self,pattern:Option<(&Pattern,usize)>,key:&str)->Inventory {
        pattern.map(|(p,slot)| {
            let mut choices=p.inputs[slot].choices.clone();
            if !self.registry.source_multipliers.contains_key(&p.id) && self.group_multiplier(p,slot)!=1 {
                if let Some(value)=choices.get_mut(key) {*value=1;}
            }
            choices
        })
            .unwrap_or_else(||Inventory::from([(key.to_owned(),1)]))
    }
    fn ordered_choices(&self,parent:Option<(&Pattern,usize)>,choices:&Inventory,primary:&str)->Vec<(String,u64)> {
        if let Some(templates)=parent.and_then(|(pattern,slot)|self.registry.source_templates.get(&pattern.id).and_then(|slots|slots.get(slot))) {
            return templates.clone();
        }
        let mut ordered=Vec::with_capacity(choices.len());
        if let Some(amount)=choices.get(primary) {ordered.push((primary.to_owned(),*amount));}
        ordered.extend(choices.iter().filter(|(key,_)|key.as_str()!=primary).map(|(key,n)|(key.clone(),*n)));
        ordered
    }
    fn has_source_pattern(&self,key:&str)->Result<bool,Error> {
        for pattern in &self.registry.patterns {
            if self.registry.primary_outputs.get(&pattern.id).is_none_or(|primary|primary==key)
                && self.source_output_count(pattern,key)?>0 {return Ok(true);}
        }
        Ok(false)
    }
    /// CraftingTreeNode.findCraftedStack chooses a craftable substitute only
    /// when the encoded key is not emittable and has no exact pattern. A
    /// different input template amount cannot be used for this fallback.
    fn crafted_key(&self,parent:Option<(&Pattern,usize)>,primary:&str,choices:&Inventory)->Result<String,Error> {
        if parent.is_none() || self.registry.emittable.contains(primary) || self.has_source_pattern(primary)? {
            return Ok(primary.to_owned());
        }
        let amount=choices.get(primary).copied().ok_or(Error::InvalidPattern)?;
        for (candidate,per) in self.ordered_choices(parent,choices,primary) {
            if per==amount && self.has_source_pattern(&candidate)? {return Ok(candidate);}
        }
        Ok(primary.to_owned())
    }
    /// Evaluate the source's mutually recursive node/process requests with an
    /// explicit continuation stack, preserving snapshots for sibling branches.
    fn node_iter(&mut self,state:InventoryState,key:&str,count:u64)->Result<(InventoryState,Inventory,Inventory),Error> {
        let mut continuations=Vec::<Continuation>::new();
        let mut action=Action::Node{state,key:key.to_owned(),count,parent:None,path:0};
        loop {
            action=match action {
                Action::Node{mut state,key,count,parent,path}=>{
                    (self.checkpoint)()?;
                    let parent_ref=parent.as_ref().map(|(pattern,slot)|(pattern,*slot));
                    let choices=self.choices(parent_ref,&key);
                    let primary_amount=choices.get(&key).copied().ok_or(Error::InvalidPattern)?;
                    let crafted=self.crafted_key(parent_ref,&key,&choices)?;
                    self.add_bytes(&mut state,self.stack_bytes(&crafted,primary_amount.checked_mul(count).ok_or(Error::Overflow)?))?;
                    let mut need=count;let mut used=Inventory::new();let mut remainders=Inventory::new();
                    for (candidate,per) in self.ordered_choices(parent_ref,&choices,&key) {
                        let selected=(state.available.get(&candidate).copied().unwrap_or(0)/per).min(need);
                        if selected==0 {continue;}
                        let quantity=selected.checked_mul(per).ok_or(Error::Overflow)?;
                        state.extract(&candidate,quantity)?;add(&mut used,&candidate,quantity)?;
                        if let Some((pattern,slot))=parent_ref {
                            if let Some(remainder)=self.registry.source_inputs.get(&pattern.id).and_then(|v|v.get(slot)).and_then(|input|input.remaining.get(&candidate)) {
                                add(&mut remainders,remainder,selected)?;
                            }
                        }
                        need-=selected;if need==0 {break;}
                    }
                    if need==0 {Action::Deliver(Outcome::Node(Ok((state,used,remainders))))}
                    else {
                        if let Some((pattern,slot))=parent_ref {
                            if let Some(remainder)=self.registry.source_inputs.get(&pattern.id).and_then(|v|v.get(slot)).and_then(|input|input.remaining.get(&crafted)) {
                                add(&mut remainders,remainder,need)?;
                            }
                        }
                        let remaining=need.checked_mul(primary_amount).ok_or(Error::Overflow)?;
                        if self.registry.emittable.contains(&key) {
                            add(&mut state.emitted,&crafted,remaining)?;add(&mut used,&crafted,remaining)?;
                            Action::Deliver(Outcome::Node(Ok((state,used,remainders))))
                        } else if !self.visited.insert(crafted.clone()) {
                            Action::Deliver(Outcome::Node(Err(Error::Cycle(crafted))))
                        } else {
                            let mut patterns=Vec::new();
                            for pattern in &self.registry.patterns {
                                if self.registry.primary_outputs.get(&pattern.id).is_none_or(|primary|primary==&crafted)
                                    && self.registry.source_nonrecursive(pattern,&crafted,&self.visited)
                                    && self.source_output_count(pattern,&crafted)?>0 {
                                    patterns.try_reserve(1).map_err(|_|Error::Overflow)?;patterns.push(pattern.clone());
                                }
                            }
                            for pattern in &patterns {for (slot,_) in self.groups(pattern)? {self.child(path,&pattern.id,slot)?;}}
                            Action::AdvanceNode(PendingNode{state:Some(state),crafted,path,used,remainders,remaining,patterns,next:0})
                        }
                    }
                }
                Action::AdvanceNode(mut node)=>{
                    if node.remaining==0 {
                        self.visited.remove(&node.crafted);
                        Action::Deliver(Outcome::Node(Ok((node.state.take().ok_or(Error::InvalidPattern)?,node.used,node.remainders))))
                    } else if node.next>=node.patterns.len() {
                        self.visited.remove(&node.crafted);
                        if !self.simulated {Action::Deliver(Outcome::Node(Err(Error::Missing(node.crafted,node.remaining))))}
                        else {
                            let mut state=node.state.take().ok_or(Error::InvalidPattern)?;
                            add(&mut state.missing,&node.crafted,node.remaining)?;add(&mut node.used,&node.crafted,node.remaining)?;
                            Action::Deliver(Outcome::Node(Ok((state,node.used,node.remainders))))
                        }
                    } else {
                        let pattern=node.patterns[node.next].clone();
                        let single=node.patterns.len()==1;
                        let times=if !single || self.registry.limits_quantity(&pattern) {1} else {
                            let per=self.source_output_count(&pattern,&node.crafted)?;
                            node.remaining/per+u64::from(node.remaining%per!=0)
                        };
                        let state=if single {node.state.take().ok_or(Error::InvalidPattern)?}else{node.state.as_ref().ok_or(Error::InvalidPattern)?.clone()};
                        let path=node.path;continuations.try_reserve(1).map_err(|_|Error::Overflow)?;continuations.push(Continuation::Node(node));
                        Action::Process{state,pattern,times,path}
                    }
                }
                Action::Process{state,pattern,times,path}=>{
                    (self.checkpoint)()?;
                    let groups=self.groups(&pattern)?;
                    Action::AdvanceProcess(PendingProcess{state:Some(state),pattern,times,path,groups,next:0,inputs:Inventory::new(),returns:Inventory::new(),selections:Vec::new()})
                }
                Action::AdvanceProcess(mut process)=>{
                    if process.next==process.groups.len() {
                        let mut state=process.state.take().ok_or(Error::InvalidPattern)?;
                        self.finish_process_iter(&mut state,&process.pattern,process.times,process.inputs,process.returns,process.selections)?;
                        Action::Deliver(Outcome::Process(Ok(state)))
                    } else {
                        let (slot,multiplicity)=process.groups[process.next];process.next+=1;
                        let key=self.primary(&process.pattern,slot);
                        let count=process.times.checked_mul(multiplicity).ok_or(Error::Overflow)?;
                        let path=self.child(process.path,&process.pattern.id,slot)?;
                        let parent=Some((process.pattern.clone(),slot));
                        let state=process.state.take().ok_or(Error::InvalidPattern)?;
                        continuations.try_reserve(1).map_err(|_|Error::Overflow)?;continuations.push(Continuation::Process(process));
                        Action::Node{state,key,count,parent,path}
                    }
                }
                Action::Deliver(outcome)=>{
                    let Some(continuation)=continuations.pop() else {
                        return match outcome {Outcome::Node(result)=>result,Outcome::Process(_)=>Err(Error::InvalidPattern)};
                    };
                    match (continuation,outcome) {
                        (Continuation::Process(mut process),Outcome::Node(Ok((state,taken,remaining))))=>{
                            let (slot,multiplicity)=process.groups[process.next-1];
                            for (key,n) in &taken {add(&mut process.inputs,key,*n)?;}
                            for (key,n) in remaining {add(&mut process.returns,&key,n)?;}
                            process.selections.try_reserve(1).map_err(|_|Error::Overflow)?;
                            process.selections.push((slot,multiplicity,taken));process.state=Some(state);
                            Action::AdvanceProcess(process)
                        }
                        (Continuation::Process(_),Outcome::Node(Err(error)))=>Action::Deliver(Outcome::Process(Err(error))),
                        (Continuation::Node(mut node),Outcome::Process(Ok(mut state)))=>{
                            let extracted=state.extract(&node.crafted,node.remaining)?;
                            if extracted==0 {
                                if node.patterns.len()==1 {return Err(Error::InvalidPattern);}
                                node.next+=1;
                            } else {
                                add(&mut node.used,&node.crafted,extracted)?;node.remaining-=extracted;node.state=Some(state);
                            }
                            Action::AdvanceNode(node)
                        }
                        (Continuation::Node(mut node),Outcome::Process(Err(Error::Missing(..)|Error::Cycle(..)))) if node.patterns.len()>1=>{
                            node.next+=1;Action::AdvanceNode(node)
                        }
                        (Continuation::Node(node),Outcome::Process(Err(error)))=>{
                            self.visited.remove(&node.crafted);Action::Deliver(Outcome::Node(Err(error)))
                        }
                        _=>return Err(Error::InvalidPattern),
                    }
                }
            };
        }
    }
    /// Return the physical keys consumed by this node, and its remainders.
    // Retained only as a bounded differential reference for the continuation
    // machine; production requests use node_iter and do not recurse here.
    #[cfg(test)]
    fn node(&mut self,state:&mut InventoryState,key:&str,count:u64,
        parent:Option<(&Pattern,usize)>,path:&NodePath)->Result<(Inventory,Inventory),Error> {
        (self.checkpoint)()?;
        if self.visited.len()>=128 {return Err(Error::SearchLimit);}
        let choices=self.choices(parent,key);
        let primary_amount=choices.get(key).copied().ok_or(Error::InvalidPattern)?;
        let crafted=self.crafted_key(parent,key,&choices)?;
        self.add_bytes(state,self.stack_bytes(&crafted,primary_amount.checked_mul(count).ok_or(Error::Overflow)?))?;
        let mut need=count;let mut used=Inventory::new();let mut remainders=Inventory::new();
        for (candidate,per) in self.ordered_choices(parent,&choices,key) {
            let possible=state.available.get(&candidate).copied().unwrap_or(0)/per;
            let selected=possible.min(need);
            if selected==0 {continue;}
            let quantity=selected.checked_mul(per).ok_or(Error::Overflow)?;
            state.extract(&candidate,quantity)?;add(&mut used,&candidate,quantity)?;
            if let Some((p,slot))=parent {
                if let Some(remainder)=self.registry.source_inputs.get(&p.id)
                    .and_then(|v|v.get(slot)).and_then(|input|input.remaining.get(&candidate)) {
                    add(&mut remainders,remainder,selected)?;
                }
            }
            need-=selected;
            if need==0 {return Ok((used,remainders));}
        }
        if let Some((p,slot))=parent {
            if let Some(remainder)=self.registry.source_inputs.get(&p.id)
                .and_then(|v|v.get(slot)).and_then(|input|input.remaining.get(&crafted)) {
                add(&mut remainders,remainder,need)?;
            }
        }
        let mut remaining=need.checked_mul(primary_amount).ok_or(Error::Overflow)?;
        if self.registry.emittable.contains(key) {
            add(&mut state.emitted,&crafted,remaining)?;add(&mut used,&crafted,remaining)?;
            return Ok((used,remainders));
        }
        if !self.visited.insert(crafted.clone()) {return Err(Error::Cycle(crafted));}
        let mut patterns=Vec::new();
        for pattern in &self.registry.patterns {
            if self.registry.primary_outputs.get(&pattern.id).is_none_or(|primary|primary==&crafted)
                && self.registry.source_nonrecursive(pattern,&crafted,&self.visited)
                && self.source_output_count(pattern,&crafted)?>0 {
                patterns.push(pattern.clone());
            }
        }
        for pattern in &patterns {
            for (slot,_) in self.groups(pattern)? {
                self.child(*path,&pattern.id,slot)?;
            }
        }
        let result=(||->Result<(),Error>{
        if patterns.len()==1 {
            let pattern=&patterns[0];
            let per=self.source_output_count(pattern,&crafted)?;
            while remaining>0 {
                let times=if self.registry.limits_quantity(pattern) {1} else {remaining/per+u64::from(remaining%per!=0)};
                self.process(state,pattern,times,path)?;
                let extracted=state.extract(&crafted,remaining)?;
                if extracted==0 {return Err(Error::InvalidPattern);}
                add(&mut used,&crafted,extracted)?;remaining-=extracted;
            }
        } else {
            for pattern in &patterns {
                while remaining>0 {
                    let mut trial=state.clone();
                    match self.process(&mut trial,pattern,1,path) {
                        Ok(())=>{
                            let extracted=trial.extract(&crafted,remaining)?;
                            if extracted==0 {break;}
                            add(&mut used,&crafted,extracted)?;remaining-=extracted;*state=trial;
                        }
                        Err(Error::Missing(..)|Error::Cycle(..))=>break,
                        Err(error)=>return Err(error),
                    }
                }
                if remaining==0 {break;}
            }
        }
        Ok(())
        })();
        self.visited.remove(&crafted);
        result?;
        if remaining>0 {
            if !self.simulated {return Err(Error::Missing(crafted,remaining));}
            add(&mut state.missing,&crafted,remaining)?;add(&mut used,&crafted,remaining)?;
        }
        Ok((used,remainders))
    }
    #[cfg(test)]
    fn process(&mut self,state:&mut InventoryState,pattern:&Pattern,times:u64,path:&NodePath)->Result<(),Error> {
        (self.checkpoint)()?;
        let mut inputs=Inventory::new();let mut returns=Inventory::new();
        let mut selections=Vec::new();
        for (slot,multiplicity) in self.groups(pattern)? {
            let primary=self.primary(pattern,slot);
            let count=times.checked_mul(multiplicity).ok_or(Error::Overflow)?;
            let child=self.child(*path,&pattern.id,slot)?;
            let (taken,remaining)=self.node(state,&primary,count,Some((pattern,slot)),&child)?;
            for (key,n) in &taken {add(&mut inputs,key,*n)?;}
            for (key,n) in remaining {add(&mut returns,&key,n)?;}
            selections.push((slot,multiplicity,taken));
        }
        let mut outputs=Inventory::new();
        for (key,n) in &returns {
            // The physical CPU expects the actual selected input's remainder.
            // Source simulation only collects remainders if at least one
            // primary input has a remainder (CraftingTreeProcess.containerItems).
            if self.registry.has_container_items(pattern) {
                state.insert(key,*n)?;
                self.add_bytes(state,self.stack_bytes(key,*n))?;
            }
            add(&mut outputs,key,*n)?;
        }
        for key in pattern.outputs.keys() {
            let n=self.source_output_count(pattern,key)?.checked_mul(times).ok_or(Error::Overflow)?;
            if n==0 {continue;}
            state.insert(key,n)?;add(&mut outputs,key,n)?;
        }
        self.add_bytes(state,u128::from(times)*1000)?;
        let mixed=times>1 && selections.iter().any(|(slot,_,taken)|taken.len()>1
            || self.registry.source_templates.get(&pattern.id).and_then(|templates|templates.get(*slot)).is_some_and(|templates| {
                let mut seen=std::collections::HashMap::new();
                templates.iter().any(|(key,amount)|seen.insert(key,*amount).is_some_and(|previous|previous!=*amount))
            }));
        if mixed || inputs.values().any(|n|n%times!=0) || outputs.values().any(|n|n%times!=0) {
            // Preserve the selected physical keys per push, batching only
            // contiguous pushes with exactly the same key/remainder shape.
            // Work is proportional to changes in selection, not push count.
            let mut remaining_runs=times;
            let mut orders=Vec::with_capacity(selections.len());
            for (slot,_,_) in &selections {
                let primary=self.primary(pattern,*slot);
                let choices=self.choices(Some((pattern,*slot)),&primary);
                orders.push(self.ordered_choices(Some((pattern,*slot)),&choices,&primary));
            }
            let mut offsets=vec![0;selections.len()];
            while remaining_runs>0 {
                (self.checkpoint)()?;
                let mut per_input=Inventory::new();let mut per_output=Inventory::new();
                let mut consumed=Vec::<(usize,String,u64)>::new();
                let mut batch=remaining_runs;
                for (index,(slot,multiplicity,taken)) in selections.iter().enumerate() {
                    let mut needed=*multiplicity;let mut per_group=Inventory::new();
                    for (key,per) in orders[index].iter().skip(offsets[index]) {
                        if needed==0 {break;}
                        let available=taken.get(key).copied().unwrap_or(0).saturating_sub(per_group.get(key).copied().unwrap_or(0))/ *per;
                        if available==0 {offsets[index]+=1;continue;}
                        let selected=available.min(needed);
                        let quantity=selected.checked_mul(*per).ok_or(Error::Overflow)?;
                        add(&mut per_input,key,quantity)?;add(&mut per_group,key,quantity)?;
                        if let Some(remainder)=self.registry.source_inputs.get(&pattern.id)
                            .and_then(|v|v.get(*slot)).and_then(|input|input.remaining.get(key)) {
                            add(&mut per_output,remainder,selected)?;
                        }
                        batch=batch.min(available/selected);
                        consumed.push((index,key.clone(),quantity));
                        needed-=selected;
                    }
                    if needed!=0 {return Err(Error::InvalidPattern);}
                    // Repeated template keys share one real inventory balance.
                    for (key,quantity) in per_group {batch=batch.min(taken.get(&key).copied().unwrap_or(0)/quantity);}
                }
                if batch==0 {return Err(Error::InvalidPattern);}
                for (index,key,quantity) in consumed {
                    let amount=quantity.checked_mul(batch).ok_or(Error::Overflow)?;
                    if !take(&mut selections[index].2,&key,amount) {return Err(Error::InvalidPattern);}
                }
                for key in pattern.outputs.keys() {
                    let n=self.source_output_count(pattern,key)?;
                    if n>0 {add(&mut per_output,key,n)?;}
                }
                let scale=|inventory:Inventory|->Result<Inventory,Error>{
                    inventory.into_iter().map(|(key,n)|Ok((key,n.checked_mul(batch).ok_or(Error::Overflow)?))).collect()
                };
                self.append_step(state,pattern,scale(per_input)?,scale(per_output)?,batch)?;
                remaining_runs-=batch;
            }
            return Ok(());
        }
        self.append_step(state,pattern,inputs,outputs,times)
    }
    fn finish_process_iter(&mut self,state:&mut InventoryState,pattern:&Pattern,times:u64,inputs:Inventory,returns:Inventory,mut selections:Vec<(usize,u64,Inventory)>)->Result<(),Error> {
        let mut outputs=Inventory::new();
        for (key,n) in &returns {
            // The physical CPU expects the actual selected input's remainder.
            // Source simulation only collects remainders if at least one
            // primary input has a remainder (CraftingTreeProcess.containerItems).
            if self.registry.has_container_items(pattern) {
                state.insert(key,*n)?;
                self.add_bytes(state,self.stack_bytes(key,*n))?;
            }
            add(&mut outputs,key,*n)?;
        }
        for key in pattern.outputs.keys() {
            let n=self.source_output_count(pattern,key)?.checked_mul(times).ok_or(Error::Overflow)?;
            if n==0 {continue;}
            state.insert(key,n)?;add(&mut outputs,key,n)?;
        }
        self.add_bytes(state,u128::from(times)*1000)?;
        let mixed=times>1 && selections.iter().any(|(slot,_,taken)|taken.len()>1
            || self.registry.source_templates.get(&pattern.id).and_then(|templates|templates.get(*slot)).is_some_and(|templates| {
                let mut seen=std::collections::HashMap::new();
                templates.iter().any(|(key,amount)|seen.insert(key,*amount).is_some_and(|previous|previous!=*amount))
            }));
        if mixed || inputs.values().any(|n|n%times!=0) || outputs.values().any(|n|n%times!=0) {
            // Preserve the selected physical keys per push, batching only
            // contiguous pushes with exactly the same key/remainder shape.
            // Work is proportional to changes in selection, not push count.
            let mut remaining_runs=times;
            let mut orders=Vec::with_capacity(selections.len());
            for (slot,_,_) in &selections {
                let primary=self.primary(pattern,*slot);
                let choices=self.choices(Some((pattern,*slot)),&primary);
                orders.push(self.ordered_choices(Some((pattern,*slot)),&choices,&primary));
            }
            let mut offsets=vec![0;selections.len()];
            while remaining_runs>0 {
                (self.checkpoint)()?;
                let mut per_input=Inventory::new();let mut per_output=Inventory::new();
                let mut consumed=Vec::<(usize,String,u64)>::new();
                let mut batch=remaining_runs;
                for (index,(slot,multiplicity,taken)) in selections.iter().enumerate() {
                    let mut needed=*multiplicity;let mut per_group=Inventory::new();
                    for (key,per) in orders[index].iter().skip(offsets[index]) {
                        if needed==0 {break;}
                        let available=taken.get(key).copied().unwrap_or(0).saturating_sub(per_group.get(key).copied().unwrap_or(0))/ *per;
                        if available==0 {offsets[index]+=1;continue;}
                        let selected=available.min(needed);
                        let quantity=selected.checked_mul(*per).ok_or(Error::Overflow)?;
                        add(&mut per_input,key,quantity)?;add(&mut per_group,key,quantity)?;
                        if let Some(remainder)=self.registry.source_inputs.get(&pattern.id)
                            .and_then(|v|v.get(*slot)).and_then(|input|input.remaining.get(key)) {
                            add(&mut per_output,remainder,selected)?;
                        }
                        batch=batch.min(available/selected);
                        consumed.push((index,key.clone(),quantity));
                        needed-=selected;
                    }
                    if needed!=0 {return Err(Error::InvalidPattern);}
                    // Repeated template keys share one real inventory balance.
                    for (key,quantity) in per_group {batch=batch.min(taken.get(&key).copied().unwrap_or(0)/quantity);}
                }
                if batch==0 {return Err(Error::InvalidPattern);}
                for (index,key,quantity) in consumed {
                    let amount=quantity.checked_mul(batch).ok_or(Error::Overflow)?;
                    if !take(&mut selections[index].2,&key,amount) {return Err(Error::InvalidPattern);}
                }
                for key in pattern.outputs.keys() {
                    let n=self.source_output_count(pattern,key)?;
                    if n>0 {add(&mut per_output,key,n)?;}
                }
                let scale=|inventory:Inventory|->Result<Inventory,Error>{
                    inventory.into_iter().map(|(key,n)|Ok((key,n.checked_mul(batch).ok_or(Error::Overflow)?))).collect()
                };
                self.append_step(state,pattern,scale(per_input)?,scale(per_output)?,batch)?;
                remaining_runs-=batch;
            }
            return Ok(());
        }
        self.append_step(state,pattern,inputs,outputs,times)
    }
    fn append_step(&self,state:&mut InventoryState,pattern:&Pattern,inputs:Inventory,outputs:Inventory,times:u64)->Result<(),Error> {
        let same_per_run=|a:&Inventory,a_runs:u64,b:&Inventory,b_runs:u64| {
            a.len()==b.len() && a.iter().all(|(key,n)|
                n%a_runs==0 && b.get(key).is_some_and(|other|other%b_runs==0 && n/a_runs==other/b_runs))
        };
        let repeat=state.steps.len()>=2 && state.steps[state.steps.len()-2].pattern==pattern.id;
        if let Some(last)=state.steps.last_mut().filter(|last|repeat && last.pattern==pattern.id
            && same_per_run(&last.inputs,last.runs,&inputs,times)
            && same_per_run(&last.outputs,last.runs,&outputs,times)) {
            for (key,n) in inputs {add(&mut last.inputs,&key,n)?;}
            for (key,n) in outputs {add(&mut last.outputs,&key,n)?;}
            last.runs=last.runs.checked_add(times).ok_or(Error::Overflow)?;
        } else {
            state.steps.try_reserve(1).map_err(|_|Error::Overflow)?;
            state.steps.push(Step{pattern:pattern.id.clone(),inputs,outputs,runs:times});
        }
        Ok(())
    }
    fn source_output_count(&self,pattern:&Pattern,key:&str)->Result<u64,Error> {
        let Some(total)=pattern.outputs.get(key).copied() else{return Ok(0);};
        if self.registry.source_outputs.get(&pattern.id).is_some_and(|outputs|!outputs.contains(key)) {return Ok(0);}
        // Host pattern outputs include encoded default container returns so
        // that real CPU flights can track them. Source pattern outputs do
        // not: add the actually selected return separately.
        let returns=self.registry.source_inputs.get(&pattern.id).map_or(0usize,|inputs|
            inputs.iter().filter(|input|input.remaining.get(&input.primary).is_some_and(|r|r==key)).count());
        total.checked_sub(returns as u64).ok_or(Error::InvalidPattern)
    }
    fn attempt(&mut self,inventory:&Inventory,target:&str,amount:u64,simulated:bool)->Result<(Plan,Inventory),Error> {
        self.simulated=simulated;self.visited.clear();
        let state=InventoryState::new(inventory,target);
        let (mut state,_,_)=self.node_iter(state,target,amount)?;
        self.add_bytes(&mut state,((self.nodes.len() as u128)+1)*8000)?;
        let bytes=u64::try_from(state.milli.checked_add(999).ok_or(Error::Overflow)?/1000).map_err(|_|Error::Overflow)?;
        Ok((Plan {target:target.to_owned(),amount,reserved:state.required,emitted:state.emitted,
            simulated,steps:state.steps,estimated_bytes:bytes},state.missing))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inv(pairs:&[(&str,u64)])->Inventory {pairs.iter().map(|(k,v)|((*k).into(),*v)).collect()}
    fn ingredient(key:&str)->Ingredient {Ingredient{choices:inv(&[(key,1)])}}
    fn pattern(id:&str,out:&str,n:u64,inputs:&[&str])->Pattern {
        Pattern{id:id.into(),inputs:inputs.iter().map(|key|ingredient(key)).collect(),outputs:inv(&[(out,n)])}
    }
    #[test]
    fn source_crafting_distinguishes_condensed_multiplier_from_template_amount() {
        let metadata=SourceInput{encoded:"oak".into(),primary:"oak".into(),remaining:BTreeMap::new()};
        let mut registry=Registry::default();
        registry.register(Pattern{id:"wrapped".into(),inputs:vec![Ingredient{choices:inv(&[("oak",2)])}],outputs:inv(&[("board",4)])}).unwrap();
        registry.set_source_inputs("wrapped",vec![metadata.clone()]).unwrap();
        registry.set_source_multipliers("wrapped",vec![2]).unwrap();
        let plan=registry.plan(&inv(&[("oak",4)]),"board",4).unwrap();
        assert_eq!(plan.estimated_bytes,25);assert_eq!(plan.reserved,inv(&[("oak",4)]));
        assert_eq!(plan.steps[0].inputs,inv(&[("oak",4)]));
        assert_eq!(registry.plan(&inv(&[("oak",3)]),"board",4),Err(Error::Missing("oak".into(),2)));
        let mut grouped=Registry::default();
        grouped.register(Pattern{id:"grouped".into(),inputs:vec![Ingredient{choices:inv(&[("oak",2)])},Ingredient{choices:inv(&[("oak",1)])}],outputs:inv(&[("board",4)])}).unwrap();
        grouped.set_source_inputs("grouped",vec![metadata.clone(),metadata]).unwrap();
        grouped.set_source_multipliers("grouped",vec![2,1]).unwrap();
        let plan=grouped.plan(&inv(&[("oak",6)]),"board",4).unwrap();
        assert_eq!(plan.estimated_bytes,27);assert_eq!(plan.reserved,inv(&[("oak",6)]));assert_eq!(plan.steps[0].inputs,inv(&[("oak",6)]));
        let mut mixed=Registry::default();
        mixed.register(Pattern{id:"mixed".into(),inputs:vec![Ingredient{choices:inv(&[("oak",2),("birch",1)])}],outputs:inv(&[("board",4)])}).unwrap();
        mixed.set_source_inputs("mixed",vec![SourceInput{encoded:"oak".into(),primary:"oak".into(),remaining:BTreeMap::new()}]).unwrap();
        mixed.set_source_multipliers("mixed",vec![2]).unwrap();
        let plan=mixed.plan(&inv(&[("oak",2),("birch",1)]),"board",4).unwrap();
        assert_eq!(plan.estimated_bytes,25);assert_eq!(plan.reserved,inv(&[("oak",2),("birch",1)]));
        assert!(grouped.set_source_multipliers("grouped",vec![0,1]).is_err());
        assert!(grouped.set_source_multipliers("grouped",vec![1]).is_err());
    }
    #[test]
    fn duplicate_template_amounts_and_even_mixed_totals_retain_real_pushes() {
        let meta=SourceInput{encoded:"oak".into(),primary:"oak".into(),remaining:BTreeMap::new()};
        let mut duplicate=Registry::default();
        duplicate.register(Pattern{id:"duplicate".into(),inputs:vec![Ingredient{choices:inv(&[("oak",2),("birch",1)])}],outputs:inv(&[("board",4)])}).unwrap();
        duplicate.set_source_inputs("duplicate",vec![meta.clone()]).unwrap();duplicate.set_source_multipliers("duplicate",vec![2]).unwrap();
        duplicate.set_source_templates("duplicate",vec![vec![("oak".into(),2),("oak".into(),1),("birch".into(),1)]]).unwrap();
        let one=duplicate.plan(&inv(&[("oak",3)]),"board",4).unwrap();assert_eq!(one.estimated_bytes,25);assert_eq!(one.reserved,inv(&[("oak",3)]));
        let two=duplicate.plan(&inv(&[("oak",7)]),"board",8).unwrap();assert_eq!(two.estimated_bytes,34);assert_eq!(two.reserved,inv(&[("oak",7)]));
        assert_eq!(two.steps.len(),2);assert_eq!(two.steps[0].inputs,inv(&[("oak",4)]));assert_eq!(two.steps[1].inputs,inv(&[("oak",3)]));
        let mut different=duplicate.clone();different.set_source_templates("duplicate",vec![vec![("oak".into(),2),("birch".into(),1)]]).unwrap();
        assert!(!duplicate.same_definitions(&different));
        let mut even=Registry::default();even.register(Pattern{id:"even".into(),inputs:vec![Ingredient{choices:inv(&[("oak",1),("birch",1)])}],outputs:inv(&[("board",4)])}).unwrap();
        even.set_source_inputs("even",vec![meta]).unwrap();even.set_source_multipliers("even",vec![3]).unwrap();
        let two=even.plan(&inv(&[("oak",4),("birch",2)]),"board",8).unwrap();assert_eq!(two.estimated_bytes,32);
        assert_eq!(two.steps.len(),2);assert_eq!(two.steps[0].inputs,inv(&[("oak",3)]));assert_eq!(two.steps[1].inputs,inv(&[("oak",1),("birch",2)]));
    }
    #[test]
    fn duplicate_templates_batch_like_source_per_push_greedy_selection() {
        for multiplier in 1..=4 {for runs in 1..=5 {
            let mut registry=Registry::default();
            registry.register(Pattern{id:"p".into(),inputs:vec![Ingredient{choices:inv(&[("oak",2),("birch",1)])}],outputs:inv(&[("board",4)])}).unwrap();
            registry.set_source_inputs("p",vec![SourceInput{encoded:"oak".into(),primary:"oak".into(),remaining:BTreeMap::new()}]).unwrap();
            registry.set_source_multipliers("p",vec![multiplier]).unwrap();
            registry.set_source_templates("p",vec![vec![("oak".into(),2),("oak".into(),1),("birch".into(),1)]]).unwrap();
            for oak in 0..=24 {for birch in 0..=8 {
                let Ok(plan)=registry.plan(&inv(&[("oak",oak),("birch",birch)]),"board",4*runs) else{continue};
                let mut remaining=plan.reserved.clone();let mut expected=Vec::new();
                for _ in 0..runs {
                    let mut need=multiplier;let mut one=Inventory::new();
                    for (key,amount) in [("oak",2),("oak",1),("birch",1)] {
                        let selected=(remaining.get(key).copied().unwrap_or(0)/amount).min(need);
                        if selected>0 {let total=selected*amount;assert!(take(&mut remaining,key,total));add(&mut one,key,total).unwrap();need-=selected;}
                    }
                    assert_eq!(need,0,"source plan must support each push: multiplier={multiplier} runs={runs} oak={oak} birch={birch}");expected.push(one);
                }
                let mut actual=Vec::new();
                for step in plan.steps {for _ in 0..step.runs {actual.push(step.inputs.iter().map(|(key,value)|(key.clone(),value/step.runs)).collect::<Inventory>());}}
                assert_eq!(actual,expected,"multiplier={multiplier} runs={runs} oak={oak} birch={birch}");
                assert!(remaining.is_empty());
            }}
        }}
    }
    #[test]
    fn thousands_of_source_candidate_nodes_keep_their_exact_byte_charge() {
        let mut registry=Registry::default();
        for i in 0..4097 {
            registry.register(Pattern{id:format!("pattern-{i:04}"),inputs:vec![ingredient(&format!("input-{i:04}"))],outputs:inv(&[("result",1)])}).unwrap();
        }
        let plan=registry.source_plan_controlled(&inv(&[("input-0000",1)]),"result",1,&mut ||Ok(())).unwrap();
        assert_eq!(plan.estimated_bytes,32787);assert_eq!(plan.reserved,inv(&[("input-0000",1)]));
        assert_eq!(plan.steps.len(),1);assert_eq!(plan.steps[0].pattern,"pattern-0000");
    }
    #[test]
    fn thousands_of_distinct_physical_substitutes_keep_every_cpu_step() {
        let mut choices=Inventory::new();
        for i in 0..4097 {choices.insert(format!("input-{i:04}"),1);}
        let mut registry=Registry::default();
        registry.register(Pattern{id:"substitutes".into(),inputs:vec![Ingredient{choices:choices.clone()}],outputs:inv(&[("result",1)])}).unwrap();
        registry.set_source_inputs("substitutes",vec![SourceInput{encoded:"input-0000".into(),primary:"input-0000".into(),remaining:BTreeMap::new()}]).unwrap();
        let plan=registry.source_plan_controlled(&choices,"result",4097,&mut ||Ok(())).unwrap();
        assert_eq!(plan.estimated_bytes,12307);assert_eq!(plan.reserved,choices);assert_eq!(plan.steps.len(),4097);
        assert_eq!(plan.steps[0].inputs,inv(&[("input-0000",1)]));
        assert_eq!(plan.steps[4096].inputs,inv(&[("input-4096",1)]));
        assert!(plan.steps.iter().all(|step|step.outputs==inv(&[("result",1)]) && step.runs==1));
        let mut network=choices;let mut cpu=Cpu::new(32768,0);cpu.submit(plan,&mut network).unwrap();
        let persisted=serde_json::to_value(&cpu).unwrap();
        assert_eq!(persisted["plan"]["steps"].as_array().unwrap().len(),0,"pending work must not be duplicated in saved metadata");
        let mut restored:Cpu=serde_json::from_value(persisted).unwrap();assert_eq!(restored.pending().len(),4097);
        let (flight,step)=restored.dispatch().unwrap();assert_eq!(step.inputs,inv(&[("input-0000",1)]));
        restored.receive(flight,"result",1).unwrap();assert_eq!(restored.pending().len(),4096);
    }
    #[test]
    fn deep_source_chain_uses_continuations_instead_of_host_stack_depth() {
        for depth in [256,1024] {
            let mut registry=Registry::default();let mut previous="raw".to_string();
            for i in 0..depth {
                let output=format!("deep-{i:04}");
                registry.register(Pattern{id:format!("pattern-{i:04}"),inputs:vec![ingredient(&previous)],outputs:Inventory::from([(output.clone(),1)])}).unwrap();
                previous=output;
            }
            let plan=registry.source_plan_controlled(&inv(&[("raw",1)]),&previous,1,&mut ||Ok(())).unwrap();
            assert_eq!(plan.estimated_bytes,10*depth as u64+9);assert_eq!(plan.steps.len(),depth);assert_eq!(plan.reserved,inv(&[("raw",1)]));
            assert_eq!(plan.steps.first().unwrap().pattern,"pattern-0000");assert_eq!(plan.steps.last().unwrap().pattern,format!("pattern-{:04}",depth-1));
            let inventory=inv(&[("raw",1)]);let before=inventory.clone();let mut checkpoints=0;
            let cancelled=registry.report_controlled(&inventory,&previous,1,&mut ||{checkpoints+=1;if checkpoints==200 {Err(Error::Cancelled)} else {Ok(())}});
            assert_eq!(cancelled,Err(Error::Cancelled));assert_eq!(checkpoints,200);assert_eq!(inventory,before);
        }
    }
    #[test]
    fn continuations_match_the_prior_recursive_engine_on_mixed_small_trees() {
        fn rng(value:&mut u64)->u64 {*value=value.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);*value>>32}
        let mut seed=0x4a17e2_u64;
        for case in 0..800 {
            let mut registry=Registry::default();
            if rng(&mut seed)%2==0 {registry.set_resource_classifier(|key|if key=="k5"{ResourceKind::Fluid}else{ResourceKind::Item});}
            if rng(&mut seed)%3==0 {registry.set_emittable([format!("k{}",rng(&mut seed)%6)]).unwrap();}
            let count=3+(rng(&mut seed)%5) as usize;
            for index in 0..count {
                let output=format!("k{}",rng(&mut seed)%6);let slots=1+(rng(&mut seed)%2) as usize;
                let mut inputs=Vec::new();let mut metadata=Vec::new();
                for _ in 0..slots {
                    let primary=format!("k{}",rng(&mut seed)%6);
                    let mut choices=Inventory::from([(primary.clone(),1+rng(&mut seed)%3)]);
                    if rng(&mut seed)%2==0 {
                        let alternate=format!("k{}",rng(&mut seed)%6);
                        if alternate!=primary {choices.insert(alternate,1+rng(&mut seed)%3);}
                    }
                    let remaining=if rng(&mut seed)%4==0 {BTreeMap::from([(primary.clone(),format!("k{}",rng(&mut seed)%6))])} else {BTreeMap::new()};
                    inputs.push(Ingredient{choices});metadata.push(SourceInput{encoded:primary.clone(),primary,remaining});
                }
                let mut outputs=Inventory::from([(output.clone(),1+(rng(&mut seed)%2))]);
                // The host includes the encoded default returns in physical outputs.
                for input in &metadata {if let Some(key)=input.remaining.get(&input.primary) {add(&mut outputs,key,1).unwrap();}}
                let id=format!("pattern-{index}");registry.register(Pattern{id:id.clone(),inputs,outputs}).unwrap();
                registry.set_source_inputs(&id,metadata).unwrap();registry.set_source_outputs(&id,[output]).unwrap();
            }
            let target=format!("k{}",rng(&mut seed)%6);let amount=1+rng(&mut seed)%3;
            let mut inventory=Inventory::new();for i in 0..6 {let n=rng(&mut seed)%4;if n>0 {inventory.insert(format!("k{i}"),n);}}
            for simulated in [false,true] {
                let mut old_checks=0;let mut old_checkpoint=||{old_checks+=1;Ok(())};
                let mut old=SourcePlanner{registry:&registry,nodes:HashMap::new(),visited:BTreeSet::new(),simulated,checkpoint:&mut old_checkpoint};
                let mut old_state=InventoryState::new(&inventory,&target);
                let old_result=old.node(&mut old_state,&target,amount,None,&0);
                let old_nodes=old.nodes.clone();drop(old);drop(old_checkpoint);
                let mut new_checks=0;let mut new_checkpoint=||{new_checks+=1;Ok(())};
                let mut new=SourcePlanner{registry:&registry,nodes:HashMap::new(),visited:BTreeSet::new(),simulated,checkpoint:&mut new_checkpoint};
                let new_result=new.node_iter(InventoryState::new(&inventory,&target),&target,amount);
                let new_nodes=new.nodes.clone();drop(new);drop(new_checkpoint);
                match (old_result,new_result) {
                    (Ok((old_used,old_returns)),Ok((new_state,new_used,new_returns)))=>{
                        assert_eq!((old_state,old_used,old_returns),(new_state,new_used,new_returns),"case={case}, simulated={simulated}");
                    }
                    (Err(old),Err(new))=>assert_eq!(old,new,"case={case}, simulated={simulated}"),
                    (old,new)=>panic!("different outcomes case={case}, simulated={simulated}: old={old:?}, new={new:?}"),
                }
                assert_eq!(old_nodes,new_nodes,"tree nodes, case={case}, simulated={simulated}");
                assert_eq!(old_checks,new_checks,"checkpoints, case={case}, simulated={simulated}");
            }
        }
    }
    #[test]
    fn substitute_remainders_are_simulated_only_when_source_enables_containers() {
        let choice=Ingredient{choices:inv(&[("oak",1),("birch",1)])};
        let mut r=Registry::default();
        r.register(Pattern{id:"board".into(),inputs:vec![choice.clone()],outputs:inv(&[("board",4)])}).unwrap();
        r.set_source_inputs("board",vec![SourceInput{encoded:"oak".into(),primary:"oak".into(),remaining:BTreeMap::from([("birch".into(),"bottle".into())])}]).unwrap();
        r.set_source_outputs("board",["board".into()]).unwrap();
        let plan=r.source_plan_controlled(&inv(&[("birch",2)]),"board",8,&mut ||Ok(())).unwrap();
        assert_eq!(plan.estimated_bytes,28);assert_eq!(plan.reserved,inv(&[("birch",2)]));
        assert_eq!(plan.steps[0].outputs,inv(&[("board",8),("bottle",2)]));
        let mut stock=inv(&[("birch",2)]);let mut cpu=Cpu::new(1024,0);cpu.submit(plan,&mut stock).unwrap();
        for _ in 0..2 {
            let (flight,step)=cpu.dispatch().unwrap();assert_eq!(step.outputs,inv(&[("board",4),("bottle",1)]));
            cpu.receive(flight,"board",4).unwrap();cpu.receive(flight,"bottle",1).unwrap();
            for _ in 0..3 {cpu.tick();}
        }
        cpu.finish(&mut stock).unwrap();assert_eq!(stock,inv(&[("board",8),("bottle",2)]));

        let mut needs_return=Registry::default();
        needs_return.register(Pattern{id:"requires_bottle".into(),inputs:vec![choice,ingredient("bottle")],outputs:inv(&[("board",1)])}).unwrap();
        needs_return.set_source_inputs("requires_bottle",vec![SourceInput{encoded:"oak".into(),primary:"oak".into(),remaining:BTreeMap::from([("birch".into(),"bottle".into())])},SourceInput{encoded:"bottle".into(),primary:"bottle".into(),remaining:BTreeMap::new()}]).unwrap();
        assert_eq!(needs_return.source_plan_controlled(&inv(&[("birch",1)]),"board",1,&mut ||Ok(())),Err(Error::Missing("bottle".into(),1)));

        let mut primary_container=Registry::default();
        primary_container.register(Pattern{id:"primary_container".into(),inputs:vec![Ingredient{choices:inv(&[("oak",1),("birch",1)])}],outputs:inv(&[("board",4),("bucket",1)])}).unwrap();
        primary_container.set_source_inputs("primary_container",vec![SourceInput{encoded:"oak".into(),primary:"oak".into(),remaining:BTreeMap::from([("oak".into(),"bucket".into())])}]).unwrap();
        primary_container.set_source_outputs("primary_container",["board".into()]).unwrap();
        let plan=primary_container.source_plan_controlled(&inv(&[("oak",1),("birch",1)]),"board",8,&mut ||Ok(())).unwrap();
        assert_eq!(plan.estimated_bytes,29);
        assert_eq!(plan.steps[0].outputs,inv(&[("board",4),("bucket",1)]));
        assert_eq!(plan.steps[1].outputs,inv(&[("board",4)]));
    }
    #[test]
    fn fluid_template_is_primary_and_can_fall_back_to_a_real_bucket() {
        let classify=|key:&str|if key=="water" {ResourceKind::Fluid}else{ResourceKind::Item};
        let fluid_input=SourceInput{encoded:"water_bucket".into(),primary:"water".into(),remaining:BTreeMap::new()};
        let mut only=Registry::default();only.set_resource_classifier(classify);
        only.register(Pattern{id:"only".into(),inputs:vec![Ingredient{choices:inv(&[("water",1000)])}],outputs:inv(&[("result",1)])}).unwrap();
        only.set_source_inputs("only",vec![fluid_input.clone()]).unwrap();
        assert_eq!(only.source_plan_controlled(&inv(&[("water_bucket",1)]),"result",1,&mut ||Ok(())),Err(Error::Missing("water".into(),1000)));

        let mut first=Registry::default();first.set_resource_classifier(classify);
        first.register(Pattern{id:"first".into(),inputs:vec![Ingredient{choices:inv(&[("water",1000),("water_bucket",1)])}],outputs:inv(&[("result",1)])}).unwrap();
        first.set_source_inputs("first",vec![SourceInput{remaining:BTreeMap::from([("water_bucket".into(),"bucket".into())]),..fluid_input}]).unwrap();
        first.set_source_outputs("first",["result".into()]).unwrap();
        let plan=first.source_plan_controlled(&inv(&[("water",1000),("water_bucket",1)]),"result",1,&mut ||Ok(())).unwrap();
        assert_eq!(plan.estimated_bytes,19);assert_eq!(plan.reserved,inv(&[("water",1000)]));
        assert_eq!(plan.steps[0].outputs,inv(&[("result",1)]));
        let plan=first.source_plan_controlled(&inv(&[("water",1000),("water_bucket",1)]),"result",2,&mut ||Ok(())).unwrap();
        assert_eq!(plan.estimated_bytes,22);assert_eq!(plan.reserved,inv(&[("water",1000),("water_bucket",1)]));
        assert_eq!(plan.steps[0].inputs,inv(&[("water",1000)]));assert_eq!(plan.steps[0].outputs,inv(&[("result",1)]));
        assert_eq!(plan.steps[1].inputs,inv(&[("water_bucket",1)]));assert_eq!(plan.steps[1].outputs,inv(&[("result",1),("bucket",1)]));
    }
    #[test]
    fn actual_source_greedy_tree_does_not_backtrack_earlier_sibling() {
        let mut r=Registry::default();
        r.register(pattern("rare_first","middle",1,&["rare"])).unwrap();
        r.register(pattern("common_second","middle",1,&["common"])).unwrap();
        r.register(pattern("final","end",1,&["middle","rare"])).unwrap();
        let (plan,missing)=r.source_report(&inv(&[("rare",1),("common",1)]),"end",1).unwrap();
        assert!(plan.simulated);
        assert_eq!(missing,inv(&[("rare",1)]));
    }
    #[test]
    fn actual_source_tree_byte_examples() {
        let mut r=Registry::default();
        let grid=["milk","milk","milk","sugar","egg","sugar","wheat","wheat","wheat"];
        r.register_primary(Pattern{id:"cake".into(),inputs:grid.iter().map(|key|ingredient(key)).collect(),
            outputs:inv(&[("cake",1),("bucket",3)])},"cake".into()).unwrap();
        r.set_source_outputs("cake",["cake".into()]).unwrap();
        r.set_source_inputs("cake",grid.iter().map(|key|SourceInput{encoded:(*key).into(),primary:(*key).into(),
            remaining:if *key=="milk" {BTreeMap::from([("milk".into(),"bucket".into())])}else{BTreeMap::new()}}).collect()).unwrap();
        let (plan,missing)=r.source_report(&inv(&[("milk",6),("sugar",4),("egg",2),("wheat",6)]),"cake",2).unwrap();
        assert!(missing.is_empty());assert!(!plan.simulated);assert_eq!(plan.estimated_bytes,68);
        let mut r=Registry::default();
        r.register_primary(Pattern{id:"reuse".into(),inputs:vec![ingredient("milk"),ingredient("bucket")],
            outputs:inv(&[("clay",1),("bucket",1)])},"clay".into()).unwrap();
        r.set_source_outputs("reuse",["clay".into()]).unwrap();
        r.set_source_inputs("reuse",vec![
            SourceInput{encoded:"milk".into(),primary:"milk".into(),remaining:BTreeMap::from([("milk".into(),"bucket".into())])},
            SourceInput{encoded:"bucket".into(),primary:"bucket".into(),remaining:BTreeMap::new()},
        ]).unwrap();
        let (plan,missing)=r.source_report(&inv(&[("milk",2),("bucket",1)]),"clay",2).unwrap();
        assert!(missing.is_empty());assert_eq!(plan.estimated_bytes,34);assert_eq!(plan.reserved,inv(&[("milk",2),("bucket",1)]));
        let mut network=inv(&[("milk",2),("bucket",1)]);
        let mut cpu=Cpu::new(1024,0);cpu.submit(plan,&mut network).unwrap();
        for iteration in 0..2 {
            if iteration>0 {for _ in 0..3 {cpu.tick();}}
            let (flight,step)=cpu.dispatch().unwrap();
            for (key,count) in step.outputs {cpu.receive(flight,&key,count).unwrap();}
        }
        cpu.finish(&mut network).unwrap();
        assert_eq!(network,inv(&[("clay",2),("bucket",1)]));
    }
    #[test]
    fn actual_source_tree_keeps_lazy_failed_and_pruned_paths() {
        let mut lazy=Registry::default();
        lazy.register(pattern("make_a","a",1,&["raw_a"])).unwrap();
        lazy.register(pattern("make_b","b",1,&["raw_b"])).unwrap();
        lazy.register(pattern("first","target",1,&["a"])).unwrap();
        lazy.register(pattern("second","target",1,&["b"])).unwrap();
        assert_eq!(lazy.source_report(&inv(&[("raw_a",1),("raw_b",1)]),"target",1).unwrap().0.estimated_bytes,37);
        let mut failed=Registry::default();
        for (id,input,out) in [("make_a","raw_a","a"),("make_b","raw_b","b"),("make_c","raw_c","c")] {
            failed.register(pattern(id,out,1,&[input])).unwrap();
        }
        failed.register(pattern("first","target",1,&["a","b"])).unwrap();
        failed.register(pattern("second","target",1,&["c"])).unwrap();
        assert_eq!(failed.source_report(&inv(&[("raw_a",1),("raw_c",1)]),"target",1).unwrap().0.estimated_bytes,61);
        let mut recursion=Registry::default();
        recursion.register_primary(Pattern{id:"recursive".into(),inputs:vec![ingredient("raw_a")],outputs:inv(&[("a",1),("target",1)])},"a".into()).unwrap();
        recursion.register(pattern("safe","a",1,&["raw_a"])).unwrap();
        recursion.register(pattern("final","target",1,&["a"])).unwrap();
        assert_eq!(recursion.source_report(&inv(&[("raw_a",1)]),"target",1).unwrap().0.estimated_bytes,29);
    }
    #[test]
    fn source_multiple_branches_combine_real_inputs() {
        let mut r=Registry::default();
        r.register(pattern("oak","plank",4,&["oak_log"])).unwrap();
        r.register(pattern("stripped","plank",4,&["stripped_log"])).unwrap();
        let (plan,missing)=r.source_report(&inv(&[("oak_log",1),("stripped_log",1),("plank",4)]),"plank",8).unwrap();
        assert!(missing.is_empty());assert!(!plan.simulated);
        assert_eq!(plan.reserved,inv(&[("oak_log",1),("stripped_log",1)]));
        assert_eq!(plan.steps.iter().map(|step|step.runs).sum::<u64>(),2);
    }
    #[test]
    fn source_simulated_shortage_and_craft_less_match_actual_tree() {
        let mut r=Registry::default();
        r.register(pattern("iron","part",1,&["iron"])).unwrap();
        r.register(pattern("gold","part",1,&["gold"])).unwrap();
        let (plan,missing)=r.source_report(&inv(&[("iron",1),("gold",1)]),"part",3).unwrap();
        assert!(plan.simulated);assert_eq!(missing,inv(&[("iron",2)]));
        let mut boards=Registry::default();
        boards.register(pattern("boards","board",4,&["log"])).unwrap();
        let (plan,missing)=boards.source_report_less_controlled(&inv(&[("log",3)]),"board",16,&mut ||Ok(())).unwrap();
        assert!(!plan.simulated && missing.is_empty());assert_eq!(plan.amount,12);assert_eq!(plan.estimated_bytes,34);
    }
    #[test]
    fn source_substitutions_use_real_inputs_across_separate_pushes() {
        let mut r=Registry::default();
        r.register(Pattern{id:"boards".into(),inputs:vec![Ingredient{choices:inv(&[("oak",1),("birch",1)])}],
            outputs:inv(&[("board",4)])}).unwrap();
        r.set_source_inputs("boards",vec![SourceInput{encoded:"oak".into(),primary:"oak".into(),remaining:BTreeMap::new()}]).unwrap();
        let mut network=inv(&[("oak",1),("birch",1)]);
        let (plan,missing)=r.source_report(&network,"board",8).unwrap();
        assert!(missing.is_empty());assert_eq!(plan.reserved,network);
        assert_eq!(plan.estimated_bytes,28);
        assert_eq!(plan.steps.len(),2);
        assert_eq!(plan.steps[0].inputs,inv(&[("oak",1)]));
        let mut cpu=Cpu::new(1024,0);cpu.submit(plan,&mut network).unwrap();
        for n in 0..2 {
            if n>0 {for _ in 0..3 {cpu.tick();}}
            let (id,step)=cpu.dispatch().unwrap();assert_eq!(step.inputs.values().sum::<u64>(),1);
            cpu.receive(id,"board",4).unwrap();
        }
        cpu.finish(&mut network).unwrap();assert_eq!(network,inv(&[("board",8)]));
    }
    #[test]
    fn mixed_substitutions_batch_more_than_4096_pushes_without_losing_inputs() {
        let choice=Ingredient{choices:inv(&[("oak",1),("birch",1)])};
        let input=SourceInput{encoded:"oak".into(),primary:"oak".into(),remaining:BTreeMap::new()};
        let mut registry=Registry::default();
        registry.register(Pattern{id:"boards".into(),inputs:vec![choice.clone()],outputs:inv(&[("board",4)])}).unwrap();
        registry.set_source_inputs("boards",vec![input.clone()]).unwrap();
        let stock=inv(&[("oak",5001),("birch",4999)]);
        let plan=registry.source_plan_controlled(&stock,"board",40000,&mut ||Ok(())).unwrap();
        assert_eq!(plan.estimated_bytes,60016);assert_eq!(plan.reserved,stock);
        assert_eq!(plan.steps.len(),2);assert_eq!(plan.steps[0].runs,5001);assert_eq!(plan.steps[0].inputs,inv(&[("oak",5001)]));
        assert_eq!(plan.steps[1].runs,4999);assert_eq!(plan.steps[1].inputs,inv(&[("birch",4999)]));
        let mut differing=Registry::default();
        differing.register(Pattern{id:"differing".into(),inputs:vec![Ingredient{choices:inv(&[("oak",1),("birch",2)])}],outputs:inv(&[("board",4)])}).unwrap();
        differing.set_source_inputs("differing",vec![input.clone()]).unwrap();
        let stock=inv(&[("oak",4999),("birch",10002)]);
        let plan=differing.source_plan_controlled(&stock,"board",40000,&mut ||Ok(())).unwrap();
        assert_eq!(plan.estimated_bytes,60016);assert_eq!(plan.reserved,stock);
        assert_eq!((plan.steps[0].runs,plan.steps[0].inputs.clone()),(4999,inv(&[("oak",4999)])));
        assert_eq!((plan.steps[1].runs,plan.steps[1].inputs.clone()),(5001,inv(&[("birch",10002)])));
        let stock=inv(&[("oak",5001),("birch",4999)]);
        let mut returned=Registry::default();
        returned.register(Pattern{id:"returned".into(),inputs:vec![choice.clone()],outputs:inv(&[("board",4),("bucket",1)])}).unwrap();
        returned.set_source_inputs("returned",vec![SourceInput{encoded:"oak".into(),primary:"oak".into(),remaining:BTreeMap::from([("oak".into(),"bucket".into()),("birch".into(),"bottle".into())])}]).unwrap();
        returned.set_source_outputs("returned",["board".into()]).unwrap();
        let plan=returned.source_plan_controlled(&stock,"board",40000,&mut ||Ok(())).unwrap();
        assert_eq!(plan.estimated_bytes,70016);
        let mut output=Inventory::new();
        for step in &plan.steps {
            for (key,n) in &step.outputs {add(&mut output,key,*n).unwrap();}
            if step.inputs.contains_key("oak") {assert_eq!(step.outputs.get("bucket"),step.inputs.get("oak"));assert!(!step.outputs.contains_key("bottle"));}
            if step.inputs.contains_key("birch") {assert_eq!(step.outputs.get("bottle"),step.inputs.get("birch"));assert!(!step.outputs.contains_key("bucket"));}
        }
        assert_eq!(output,inv(&[("board",40000),("bucket",5001),("bottle",4999)]));
        let mut registry=Registry::default();
        registry.register(Pattern{id:"boards".into(),inputs:vec![choice.clone(),choice],outputs:inv(&[("board",4)])}).unwrap();
        registry.set_source_inputs("boards",vec![input.clone(),input]).unwrap();
        let stock=inv(&[("oak",5001),("birch",14999)]);
        let plan=registry.source_plan_controlled(&stock,"board",40000,&mut ||Ok(())).unwrap();
        assert_eq!(plan.estimated_bytes,70016);assert_eq!(plan.reserved,stock);
        assert_eq!(plan.steps.len(),3);
        assert_eq!((plan.steps[0].runs,plan.steps[0].inputs.clone()),(2500,inv(&[("oak",5000)])));
        assert_eq!((plan.steps[1].runs,plan.steps[1].inputs.clone()),(1,inv(&[("oak",1),("birch",1)])));
        assert_eq!((plan.steps[2].runs,plan.steps[2].inputs.clone()),(7499,inv(&[("birch",14998)])));
    }
    #[test]
    fn source_branching_propagates_cooperative_cancellation() {
        let mut r=Registry::default();
        r.register(pattern("first","board",1,&["oak"])).unwrap();
        r.register(pattern("second","board",1,&["birch"])).unwrap();
        let mut calls=0;
        let result=r.source_report_controlled(&inv(&[("oak",2),("birch",2)]),"board",4,&mut ||{
            calls+=1;if calls==4 {Err(Error::Cancelled)} else {Ok(())}
        });
        assert_eq!(result,Err(Error::Cancelled));
    }
    #[test]
    fn source_repeated_container_reuse_coalesces_over_4096_pushes() {
        let mut r=Registry::default();
        r.register_primary(Pattern{id:"reuse".into(),inputs:vec![ingredient("milk"),ingredient("bucket")],
            outputs:inv(&[("clay",1),("bucket",1)])},"clay".into()).unwrap();
        r.set_source_outputs("reuse",["clay".into()]).unwrap();
        r.set_source_inputs("reuse",vec![
            SourceInput{encoded:"milk".into(),primary:"milk".into(),remaining:BTreeMap::from([("milk".into(),"bucket".into())])},
            SourceInput{encoded:"bucket".into(),primary:"bucket".into(),remaining:BTreeMap::new()},
        ]).unwrap();
        let (plan,missing)=r.source_report(&inv(&[("milk",4097),("bucket",1)]),"clay",4097).unwrap();
        assert!(missing.is_empty());assert_eq!(plan.reserved,inv(&[("milk",4097),("bucket",1)]));
        assert_eq!(plan.estimated_bytes,20509);
        assert_eq!(plan.steps.len(),2);assert_eq!(plan.steps.iter().map(|step|step.runs).sum::<u64>(),4097);
    }
    #[test]
    fn source_output_that_is_also_a_remainder_is_counted_once_each() {
        let mut r=Registry::default();
        // The host pattern includes both the physical output and the default
        // container return under one key.
        r.register_primary(Pattern{id:"same".into(),inputs:vec![ingredient("milk")],
            outputs:inv(&[("bucket",2)])},"bucket".into()).unwrap();
        r.set_source_outputs("same",["bucket".into()]).unwrap();
        r.set_source_inputs("same",vec![SourceInput{encoded:"milk".into(),primary:"milk".into(),
            remaining:BTreeMap::from([("milk".into(),"bucket".into())])}]).unwrap();
        let mut network=inv(&[("milk",1)]);
        let (plan,missing)=r.source_report(&network,"bucket",2).unwrap();
        assert!(missing.is_empty());assert_eq!(plan.estimated_bytes,21);
        assert_eq!(plan.steps[0].runs,1);assert_eq!(plan.steps[0].outputs,inv(&[("bucket",2)]));
        let mut cpu=Cpu::new(1024,0);cpu.submit(plan,&mut network).unwrap();
        let (flight,step)=cpu.dispatch().unwrap();cpu.receive(flight,"bucket",step.outputs["bucket"]).unwrap();
        cpu.finish(&mut network).unwrap();assert_eq!(network,inv(&[("bucket",2)]));
    }
    #[test]
    fn source_crafts_a_valid_substitute_when_the_primary_has_no_pattern() {
        let mut r=Registry::default();
        r.register(pattern("birch","birch",1,&["raw_birch"])).unwrap();
        r.register(Pattern{id:"box".into(),inputs:vec![Ingredient{choices:inv(&[("oak",1),("birch",1)])}],outputs:inv(&[("box",1)])}).unwrap();
        r.set_source_inputs("box",vec![SourceInput{encoded:"oak".into(),primary:"oak".into(),remaining:BTreeMap::new()}]).unwrap();
        let mut stock=inv(&[("raw_birch",1)]);
        let (plan,missing)=r.source_report(&stock,"box",1).unwrap();
        assert!(missing.is_empty());assert!(!plan.simulated);assert_eq!(plan.estimated_bytes,29);
        assert_eq!(plan.steps.iter().map(|step|step.pattern.as_str()).collect::<Vec<_>>(),["birch","box"]);
        assert_eq!(plan.reserved,stock);
        let mut cpu=Cpu::new(1024,0);cpu.submit(plan,&mut stock).unwrap();
        let (id,step)=cpu.dispatch().unwrap();assert_eq!(step.inputs,inv(&[("raw_birch",1)]));cpu.receive(id,"birch",1).unwrap();
        for _ in 0..3 {cpu.tick();}
        let (id,step)=cpu.dispatch().unwrap();assert_eq!(step.inputs,inv(&[("birch",1)]));cpu.receive(id,"box",1).unwrap();
        cpu.finish(&mut stock).unwrap();assert_eq!(stock,inv(&[("box",1)]));
    }
    #[test]
    fn source_substitute_fallback_requires_no_primary_pattern_and_equal_template_amounts() {
        let mut r=Registry::default();
        r.register(pattern("oak","oak",1,&["missing_oak"])).unwrap();
        r.register(pattern("birch","birch",1,&["raw_birch"])).unwrap();
        r.register(Pattern{id:"box".into(),inputs:vec![Ingredient{choices:inv(&[("oak",1),("birch",1)])}],outputs:inv(&[("box",1)])}).unwrap();
        r.set_source_inputs("box",vec![SourceInput{encoded:"oak".into(),primary:"oak".into(),remaining:BTreeMap::new()}]).unwrap();
        assert_eq!(r.source_plan_controlled(&inv(&[("raw_birch",1)]),"box",1,&mut ||Ok(())),Err(Error::Missing("missing_oak".into(),1)));
        let mut r=Registry::default();
        r.register(pattern("birch","birch",1000,&["raw_birch"])).unwrap();
        r.register(Pattern{id:"box".into(),inputs:vec![Ingredient{choices:inv(&[("oak",1),("birch",1000)])}],outputs:inv(&[("box",1)])}).unwrap();
        r.set_source_inputs("box",vec![SourceInput{encoded:"oak".into(),primary:"oak".into(),remaining:BTreeMap::new()}]).unwrap();
        assert_eq!(r.source_plan_controlled(&inv(&[("raw_birch",1)]),"box",1,&mut ||Ok(())),Err(Error::Missing("oak".into(),1)));
    }
    #[test]
    fn source_crafted_substitute_uses_condensed_recipe_inputs() {
        let mut r=Registry::default();
        r.register(pattern("birch_planks","birch",4,&["raw_birch"])).unwrap();
        let choice=Ingredient{choices:inv(&[("oak",1),("birch",1)])};
        r.register(Pattern{id:"sticks".into(),inputs:vec![choice.clone(),choice],outputs:inv(&[("sticks",4)])}).unwrap();
        r.set_source_inputs("sticks",vec![SourceInput{encoded:"oak".into(),primary:"oak".into(),remaining:BTreeMap::new()};2]).unwrap();
        let plan=r.source_plan_controlled(&inv(&[("raw_birch",1)]),"sticks",4,&mut ||Ok(())).unwrap();
        assert_eq!(plan.estimated_bytes,33);assert_eq!(plan.reserved,inv(&[("raw_birch",1)]));
        assert_eq!(plan.steps[0].outputs,inv(&[("birch",4)]));assert_eq!(plan.steps[1].inputs,inv(&[("birch",2)]));
    }
}
impl Registry {
    pub(super) fn source_plan_controlled(&self,inventory:&Inventory,target:&str,amount:u64,checkpoint:&mut dyn FnMut()->Result<(),Error>)->Result<Plan,Error> {
        if target.is_empty() || amount==0 {return Err(Error::InvalidPattern);}
        let mut planner=SourcePlanner {registry:self,nodes:HashMap::new(),
            visited:BTreeSet::new(),simulated:false,checkpoint};
        planner.attempt(inventory,target,amount,false).map(|(plan,_)|plan)
    }
    #[cfg(test)]
    fn source_report(&self,inventory:&Inventory,target:&str,amount:u64)->Result<(Plan,Inventory),Error> {
        self.source_report_controlled(inventory,target,amount,&mut ||Ok(()))
    }
    pub(super) fn source_report_controlled(&self,inventory:&Inventory,target:&str,amount:u64,checkpoint:&mut dyn FnMut()->Result<(),Error>)->Result<(Plan,Inventory),Error> {
        if target.is_empty() || amount==0 {return Err(Error::InvalidPattern);}
        let mut planner=SourcePlanner {registry:self,nodes:HashMap::new(),
            visited:BTreeSet::new(),simulated:false,checkpoint};
        match planner.attempt(inventory,target,amount,false) {
            Ok(result)=>Ok(result),
            Err(Error::Missing(..)|Error::Cycle(..))=>planner.attempt(inventory,target,amount,true),
            Err(error)=>Err(error),
        }
    }
    pub(super) fn source_report_less_controlled(&self,inventory:&Inventory,target:&str,amount:u64,checkpoint:&mut dyn FnMut()->Result<(),Error>)->Result<(Plan,Inventory),Error> {
        if target.is_empty() || amount==0 {return Err(Error::InvalidPattern);}
        let mut planner=SourcePlanner {registry:self,nodes:HashMap::new(),
            visited:BTreeSet::new(),simulated:false,checkpoint};
        match planner.attempt(inventory,target,amount,false) {
            Ok(result)=>return Ok(result),
            Err(Error::Missing(..)|Error::Cycle(..))=>{},
            Err(error)=>return Err(error),
        }
        let mut bit=1u64 << (u64::BITS-1-amount.leading_zeros());
        let mut done=0u64;let mut best=None;
        while bit>0 {
            if let Some(test)=done.checked_add(bit).filter(|n|*n<amount) {
                match planner.attempt(inventory,target,test,false) {
                    Ok(result)=>{done=test;best=Some(result);},
                    Err(Error::Missing(..)|Error::Cycle(..))=>{},
                    Err(error)=>return Err(error),
                }
            }
            bit/=2;
        }
        if let Some(result)=best {Ok(result)} else {planner.attempt(inventory,target,amount,true)}
    }
}
