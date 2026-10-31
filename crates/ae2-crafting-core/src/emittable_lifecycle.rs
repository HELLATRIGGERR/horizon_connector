//! Per-service emittable-key membership for ordered crafting-provider events.
//! The host must supply actual node membership and post-commit refresh events.
use std::{collections::HashMap, hash::Hash};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmittableError { DuplicateProvider, UnknownProvider, UnexpectedService, StillMounted, CountOverflow, Unavailable(String) }
#[derive(Clone, Debug)]
struct State<K> { keys: Vec<K>, service: Option<u64> }
#[derive(Clone, Debug)]
pub struct EmittableLifecycle<P, K> {
    providers: HashMap<P, State<K>>,
    counts: HashMap<u64, Vec<(K, usize)>>,
    failure: Option<EmittableError>,
}
impl<P, K> Default for EmittableLifecycle<P, K> {
    fn default() -> Self { Self { providers: HashMap::new(), counts: HashMap::new(), failure: None } }
}
impl<P: Clone + Eq + Hash, K: Clone + Eq> EmittableLifecycle<P, K> {
    pub fn invalidate(&mut self, reason: String) {
        if self.failure.is_none() { self.failure = Some(EmittableError::Unavailable(reason)); }
    }
    fn transaction<T>(&mut self, change: impl FnOnce(&mut Self) -> Result<T, EmittableError>) -> Result<T, EmittableError> {
        if let Some(error) = &self.failure { return Err(error.clone()); }
        let mut staged = self.clone();
        match change(&mut staged) {
            Ok(value) => { *self = staged; Ok(value) }
            Err(error) => { self.failure = Some(error.clone()); Err(error) }
        }
    }
    fn unique(keys: Vec<K>) -> Vec<K> {
        let mut result = Vec::new();
        for key in keys { if !result.contains(&key) { result.push(key); } }
        result
    }
    fn mount(&mut self, service: u64, keys: &[K]) -> Result<(), EmittableError> {
        let counts = self.counts.entry(service).or_default();
        for key in keys {
            if let Some((_, count)) = counts.iter_mut().find(|(candidate, _)| candidate == key) {
                *count = count.checked_add(1).ok_or(EmittableError::CountOverflow)?;
            } else { counts.push((key.clone(), 1)); }
        }
        Ok(())
    }
    fn unmount(&mut self, service: u64, keys: &[K]) -> Result<(), EmittableError> {
        if keys.is_empty() { return Ok(()); }
        let counts = self.counts.get_mut(&service).ok_or(EmittableError::UnexpectedService)?;
        for key in keys {
            let index = counts.iter().position(|(candidate, _)| candidate == key).ok_or(EmittableError::UnexpectedService)?;
            if counts[index].1 == 1 { counts.remove(index); } else { counts[index].1 -= 1; }
        }
        Ok(())
    }
    pub fn add(&mut self, id: P, keys: Vec<K>) -> Result<(), EmittableError> {
        self.transaction(|next| {
            if next.providers.contains_key(&id) { return Err(EmittableError::DuplicateProvider); }
            next.providers.insert(id, State { keys: Self::unique(keys), service: None }); Ok(())
        })
    }
    pub fn refresh(&mut self, id: &P, keys: Vec<K>) -> Result<(), EmittableError> {
        self.transaction(|next| {
            let old = next.providers.get(id).ok_or(EmittableError::UnknownProvider)?.clone();
            let keys = Self::unique(keys);
            if let Some(service) = old.service { next.unmount(service, &old.keys)?; next.mount(service, &keys)?; }
            next.providers.get_mut(id).unwrap().keys = keys; Ok(())
        })
    }
    pub fn membership(&mut self, id: &P, from: Option<u64>, to: Option<u64>) -> Result<(), EmittableError> {
        self.transaction(|next| {
            let old = next.providers.get(id).ok_or(EmittableError::UnknownProvider)?.clone();
            if old.service != from { return Err(EmittableError::UnexpectedService); }
            if from != to {
                if let Some(service) = from { next.unmount(service, &old.keys)?; }
                if let Some(service) = to { next.mount(service, &old.keys)?; }
                next.providers.get_mut(id).unwrap().service = to;
            }
            Ok(())
        })
    }
    pub fn forget(&mut self, id: &P) -> Result<(), EmittableError> {
        self.transaction(|next| {
            if next.providers.get(id).ok_or(EmittableError::UnknownProvider)?.service.is_some() { return Err(EmittableError::StillMounted); }
            next.providers.remove(id); Ok(())
        })
    }
    pub fn count(&self, service: u64, key: &K) -> Result<usize, EmittableError> {
        if let Some(error) = &self.failure { return Err(error.clone()); }
        Ok(self.counts.get(&service).and_then(|counts| counts.iter().find(|(candidate, _)| candidate == key)).map_or(0, |(_, count)| *count))
    }
    /// Snapshot the registered keys for one actual service without consuming
    /// provider state or deriving membership from the physical world.
    pub fn keys(&self, service: u64) -> Result<Vec<K>, EmittableError> {
        if let Some(error) = &self.failure { return Err(error.clone()); }
        Ok(self.counts.get(&service).map_or_else(Vec::new, |counts| counts.iter().map(|(key, _)| key.clone()).collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_emitters_remount_and_move_without_losing_other_registration() {
        let mut e = EmittableLifecycle::default();
        e.add("a", vec!["iron", "iron"]).unwrap(); e.add("b", vec!["iron"]).unwrap();
        e.membership(&"a", None, Some(1)).unwrap(); e.membership(&"b", None, Some(1)).unwrap();
        assert_eq!(e.count(1, &"iron"), Ok(2));
        assert_eq!(e.keys(1), Ok(vec!["iron"]));
        e.refresh(&"a", vec!["gold"]).unwrap();
        assert_eq!(e.count(1, &"iron"), Ok(1)); assert_eq!(e.count(1, &"gold"), Ok(1));
        e.membership(&"b", Some(1), Some(2)).unwrap();
        assert_eq!(e.count(1, &"iron"), Ok(0)); assert_eq!(e.count(2, &"iron"), Ok(1));
        e.membership(&"a", Some(1), None).unwrap(); e.forget(&"a").unwrap();
        assert_eq!(e.count(1, &"gold"), Ok(0));
    }
    #[test]
    fn unexpected_service_poison_is_sticky() {
        let mut e = EmittableLifecycle::default(); e.add("a", vec!["iron"]).unwrap();
        assert_eq!(e.membership(&"a", Some(7), Some(1)), Err(EmittableError::UnexpectedService));
        assert_eq!(e.count(1, &"iron"), Err(EmittableError::UnexpectedService));
    }
}
