//! Runtime-only available-key cache, derived from AE2 19.2.18
//! CompositeStorage.InventoryCache. Copyright TeamAppliedEnergistics;
//! LGPL-3.0-or-later. The caller owns physical capability validation.
use std::collections::BTreeMap;
#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) struct Monitor<K:Ord> {
    items:BTreeMap<K,u64>, fluids:BTreeMap<K,u64>, dirty:bool,
}
impl<K:Ord> Default for Monitor<K>{fn default()->Self{Self{items:BTreeMap::new(),fluids:BTreeMap::new(),dirty:true}}}
impl<K:Ord+Clone> Monitor<K> {
    /// A dirty cache must be rebuilt from the physical handler before reading.
    pub fn snapshot(&self)->Option<(&BTreeMap<K,u64>,&BTreeMap<K,u64>)>{(!self.dirty).then_some((&self.items,&self.fluids))}
    /// Called only after a committed own transfer, not a simulation.
    pub fn invalidate(&mut self){self.dirty=true;}
    /// Source monitor callback or forced rebuild; zero entries are absent.
    pub fn update(&mut self,mut items:BTreeMap<K,u64>,mut fluids:BTreeMap<K,u64>)->bool {
        items.retain(|_,n|*n>0);fluids.retain(|_,n|*n>0);
        let changed=self.items!=items || self.fluids!=fluids;
        self.items=items;self.fluids=fluids;self.dirty=false;changed
    }
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn external_change_waits_for_tick_own_commit_forces_rebuild_and_rollback_restores_cache(){
  let mut m=Monitor::<&str>::default();assert!(m.snapshot().is_none());
  assert!(m.update(BTreeMap::from([("diamond",1)]),BTreeMap::new()));
  let mut external=BTreeMap::from([("gold",1)]);
  assert_eq!(m.snapshot().unwrap().0.get("diamond"),Some(&1));
  assert!(m.update(external.clone(),BTreeMap::new()));assert_eq!(m.snapshot().unwrap().0.get("gold"),Some(&1));
  let before=m.clone();external.insert("diamond",3);m.invalidate();assert!(m.snapshot().is_none());m.update(external,BTreeMap::new());assert_eq!(m.snapshot().unwrap().0.get("diamond"),Some(&3));
  m=before;assert_eq!(m.snapshot().unwrap().0.get("diamond"),None);assert_eq!(m.snapshot().unwrap().0.get("gold"),Some(&1));
 }
}
