//! Source-event provider lifecycle; not yet wired into Horizon scheduling.
use crate::provider_registry::{Definition, ProviderRegistry, RegistryError};
use std::collections::HashMap;
use std::hash::Hash;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifecycleError {
    DuplicateProvider,
    UnknownProvider,
    UnexpectedService,
    ProviderStillMounted,
    Registry(RegistryError),
    Unavailable(String),
}
impl From<RegistryError> for LifecycleError {
    fn from(value:RegistryError)->Self {Self::Registry(value)}
}
#[derive(Clone, Debug)]
struct ProviderState<D,K> {
    current_priority:i32,
    effective_patterns:Vec<Definition<D,K>>,
    service:Option<u64>,
}
#[derive(Clone, Debug)]
pub struct ProviderLifecycle<P,D,K> {
    providers:HashMap<P,ProviderState<D,K>>,
    services:HashMap<u64,ProviderRegistry<P,D,K>>,
    failure:Option<LifecycleError>,
}
impl<P,D,K> Default for ProviderLifecycle<P,D,K> {
    fn default()->Self {Self {providers:HashMap::new(),services:HashMap::new(),failure:None}}
}
impl<P:Clone+Eq+Hash,D:Clone+Eq+Hash,K:Clone+Eq> ProviderLifecycle<P,D,K> {
    fn transaction<T>(&mut self,change:impl FnOnce(&mut Self)->Result<T,LifecycleError>)->Result<T,LifecycleError> {
        if let Some(failure)=&self.failure {return Err(failure.clone());}
        let mut staged=self.clone();
        match change(&mut staged) {
            Ok(result)=>{*self=staged;Ok(result)}
            Err(error)=>{self.failure=Some(error.clone());Err(error)}
        }
    }
    pub fn invalidate(&mut self,reason:String) {
        if self.failure.is_none(){self.failure=Some(LifecycleError::Unavailable(reason));}
    }
    /// Constructor state: no decoded patterns have yet been updated.
    pub fn add_provider(&mut self,id:P,priority:i32)->Result<(),LifecycleError> {
        self.transaction(|next| {
            if next.providers.contains_key(&id){return Err(LifecycleError::DuplicateProvider)}
            next.providers.insert(id,ProviderState {current_priority:priority,effective_patterns:Vec::new(),service:None});Ok(())
        })
    }
    /// Direct NBT read changes priority without a registry request. It does
    /// not recompute the provider logic's effective pattern list.
    pub fn read_priority_without_refresh(&mut self,id:&P,priority:i32)->Result<(),LifecycleError> {
        self.transaction(|next| {next.providers.get_mut(id).ok_or(LifecycleError::UnknownProvider)?.current_priority=priority;Ok(())})
    }
    pub fn priority_setter(&mut self,id:&P,priority:i32)->Result<(),LifecycleError> {
        self.transaction(|next| {
            let provider=next.providers.get_mut(id).ok_or(LifecycleError::UnknownProvider)?;
            provider.current_priority=priority;
            if let Some(service)=provider.service {
                let registry=next.services.get_mut(&service).ok_or(LifecycleError::UnexpectedService)?;
                if !registry.contains_provider(id){return Err(LifecycleError::UnexpectedService)}
                registry.refresh(id,priority,provider.effective_patterns.clone())?;
            }
            Ok(())
        })
    }
    pub fn patterns_changed(&mut self,id:&P,patterns:Vec<Definition<D,K>>)->Result<(),LifecycleError> {
        self.transaction(|next| {
            let provider=next.providers.get_mut(id).ok_or(LifecycleError::UnknownProvider)?;
            provider.effective_patterns=patterns;
            if let Some(service)=provider.service {
                let registry=next.services.get_mut(&service).ok_or(LifecycleError::UnexpectedService)?;
                if !registry.contains_provider(id){return Err(LifecycleError::UnexpectedService)}
                registry.refresh(id,provider.current_priority,provider.effective_patterns.clone())?;
            }
            Ok(())
        })
    }
    /// Actual service callback. The old grid unmounts before the new one
    /// captures the current priority and the already-effective patterns.
    pub fn membership(&mut self,id:&P,from:Option<u64>,to:Option<u64>)->Result<(),LifecycleError> {
        self.transaction(|next| {
            let provider=next.providers.get_mut(id).ok_or(LifecycleError::UnknownProvider)?;
            if provider.service!=from{return Err(LifecycleError::UnexpectedService)}
            if from==to{return Ok(())}
            if let Some(old)=from {
                let registry=next.services.get_mut(&old).ok_or(LifecycleError::UnexpectedService)?;
                if !registry.remove(id)?{return Err(LifecycleError::UnexpectedService)}
            }
            if let Some(new)=to {
                next.services.entry(new).or_default().add(id.clone(),provider.current_priority,provider.effective_patterns.clone())?;
            }
            provider.service=to;Ok(())
        })
    }
    pub fn forget_provider(&mut self,id:&P)->Result<(),LifecycleError> {
        self.transaction(|next| {
            let provider=next.providers.get(id).ok_or(LifecycleError::UnknownProvider)?;
            if provider.service.is_some(){return Err(LifecycleError::ProviderStillMounted)}
            next.providers.remove(id);Ok(())
        })
    }
    pub fn service(&self,id:u64)->Result<Option<&ProviderRegistry<P,D,K>>,LifecycleError> {
        if let Some(error)=&self.failure{return Err(error.clone())}Ok(self.services.get(&id))
    }
    pub fn service_mut(&mut self,id:u64)->Result<Option<&mut ProviderRegistry<P,D,K>>,LifecycleError> {
        if let Some(error)=&self.failure{return Err(error.clone())}Ok(self.services.get_mut(&id))
    }
    /// The provider's last decoded pattern cache, which can intentionally
    /// differ from physical slots after a direct NBT read.
    pub fn effective_patterns(&self,id:&P)->Result<&[Definition<D,K>],LifecycleError> {
        if let Some(error)=&self.failure{return Err(error.clone())}
        Ok(&self.providers.get(id).ok_or(LifecycleError::UnknownProvider)?.effective_patterns)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_registry::PriorityGroup;
    fn pattern(key:&'static str)->Definition<&'static str,&'static str> {Definition{key,primary_output:"diamond"}}
    #[test]
    fn priority_setter_remounts_effective_patterns_not_unobserved_physical_slots() {
        let mut l=ProviderLifecycle::default();
        l.add_provider("a",10).unwrap();l.add_provider("b",0).unwrap();
        l.patterns_changed(&"a",vec![pattern("A")]).unwrap();l.patterns_changed(&"b",vec![pattern("B")]).unwrap();
        l.membership(&"a",None,Some(7)).unwrap();l.membership(&"b",None,Some(7)).unwrap();
        assert_eq!(l.service(7).unwrap().unwrap().ranked_definitions(&"diamond"),vec![PriorityGroup{priority:10,definitions:vec!["A"]},PriorityGroup{priority:0,definitions:vec!["B"]}]);
        l.read_priority_without_refresh(&"a",-10).unwrap();
        assert_eq!(l.effective_patterns(&"a").unwrap(), &[pattern("A")]);
        assert_eq!(l.service(7).unwrap().unwrap().ranked_definitions(&"diamond")[0].definitions,vec!["A"]);
        l.priority_setter(&"a",-10).unwrap();
        assert_eq!(l.service(7).unwrap().unwrap().ranked_definitions(&"diamond"),vec![PriorityGroup{priority:0,definitions:vec!["B"]},PriorityGroup{priority:-10,definitions:vec!["A"]}]);
        l.patterns_changed(&"a",vec![pattern("B")]).unwrap();
        assert_eq!(l.service(7).unwrap().unwrap().ranked_definitions(&"diamond"),vec![PriorityGroup{priority:0,definitions:vec!["B"]}]);
        let registry=l.service_mut(7).unwrap().unwrap();let mut pass=registry.media_pass(&"B").unwrap();
        assert_eq!(registry.next_medium(&mut pass).unwrap(),Some("b"));
        assert_eq!(registry.next_medium(&mut pass).unwrap(),Some("a"));
    }
    #[test]
    fn service_moves_retain_effective_patterns_and_reset_real_media_lists() {
        let mut l=ProviderLifecycle::default();
        for id in ["a","b","c"] {l.add_provider(id,0).unwrap();l.patterns_changed(&id,vec![pattern("A")]).unwrap();}
        l.membership(&"a",None,Some(1)).unwrap();l.membership(&"b",None,Some(1)).unwrap();l.membership(&"c",None,Some(2)).unwrap();
        let registry=l.service_mut(1).unwrap().unwrap();let mut pass=registry.media_pass(&"A").unwrap();assert_eq!(registry.next_medium(&mut pass).unwrap(),Some("a"));
        l.membership(&"a",Some(1),Some(2)).unwrap();
        let registry=l.service_mut(2).unwrap().unwrap();let mut pass=registry.media_pass(&"A").unwrap();
        assert_eq!(registry.next_medium(&mut pass).unwrap(),Some("c"));assert_eq!(registry.next_medium(&mut pass).unwrap(),Some("a"));
        l.membership(&"a",Some(2),None).unwrap();l.forget_provider(&"a").unwrap();
    }
    #[test]
    fn invalid_service_order_poisons_only_the_lifecycle_model() {
        let mut l=ProviderLifecycle::<&str,&str,&str>::default();l.add_provider("a",0).unwrap();
        assert_eq!(l.membership(&"a",Some(99),Some(3)),Err(LifecycleError::UnexpectedService));
        assert!(matches!(l.service(3),Err(LifecycleError::UnexpectedService)));
        assert_eq!(l.priority_setter(&"a",1),Err(LifecycleError::UnexpectedService));
    }
}
