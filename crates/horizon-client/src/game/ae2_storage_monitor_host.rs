//! Host adapter for AE2 CompositeStorage's runtime cache. Staged until the
//! plane/light acceptance cut is built; never serializes virtual counts.
use super::{Game,ae2_buses::StorageMount,ae2_storage_monitor::Monitor};
use ae2_cell_core::ItemKey;
use ae2_network_core::NodeIdentity;
use std::collections::{BTreeMap,BTreeSet};
#[derive(Clone)]
struct Entry { mount:StorageMount, signature:String, configuration:String, target_incarnation:u64, monitor:Monitor<ItemKey>, ready:bool, observed:(BTreeMap<ItemKey,u64>,BTreeMap<ItemKey,u64>) }
fn signature(mount:&StorageMount)->String {format!("{:?}:{:?}:{}",mount.target,mount.face,mount.fluid)}
fn configuration(mount:&StorageMount)->String {format!("{:?}:{:?}:{}:{}:{}:{}:{:?}:{}:{}",mount.access,mount.filter,mount.inverted,mount.filter_on_extract,mount.extractable_only,mount.fuzzy,mount.fuzzy_mode,mount.void_overflow,mount.monitor_configuration)}
#[derive(Clone,Default)]
pub(super) struct StorageMonitors { entries:BTreeMap<NodeIdentity,Entry>, recursive_mounts:BTreeMap<NodeIdentity,(super::ae2_global::GridAddress,String)> }
impl Game<'_> {
 fn ae2_monitor_target_incarnation(&self,mount:&StorageMount)->Option<u64> {
    let (dim,pos)=self.ae2_address_parts(mount.target)?;self.ae2_world_for(dim)?.block_entities.incarnation(&pos)
 }
 fn ae2_monitor_live(&self,mount:&StorageMount)->Result<(BTreeMap<ItemKey,u64>,BTreeMap<ItemKey,u64>),String> {
    let item=self.ae2_storage_inventory_raw(mount).into_iter().collect();
    let mut fluid=BTreeMap::new();
    if matches!(self.ae2_block_entity(mount.target),Some(horizon_client::containers::BlockEntity::Ae2(m)) if m.kind==horizon_client::ae2_world::MachineKind::SkyStoneTank) {
       if let Some((key,n))=self.ae2_tank_contents_encoded(mount.target)? {fluid.insert(key,n);}
    }
    Ok((item,fluid))
 }
 /// Reading a dirty cache forces a physical rebuild; external edits alone do
 /// not dirty it, matching CompositeStorage.getAvailableStacks.
 pub(super) fn ae2_monitor_read(&self,mount:&StorageMount)->Result<(Vec<(ItemKey,u64)>,Vec<(ItemKey,u64)>),String> {
    let incarnation=self.ae2_monitor_target_incarnation(mount).ok_or("storage target disappeared")?;
    let signature=signature(mount);
    let configuration=configuration(mount);
    let existing={let all=self.ae2_storage_monitors.borrow();all.entries.get(&mount.identity).filter(|e|e.target_incarnation==incarnation && e.signature==signature).cloned()};
    let config_changed=existing.as_ref().is_some_and(|e|e.configuration!=configuration);
    if existing.as_ref().is_some_and(|e|!e.ready) && !config_changed {return Ok((Vec::new(),Vec::new()))}
    let cached=existing.as_ref().filter(|_|!config_changed).and_then(|e|e.monitor.snapshot().map(|(i,f)|(i.iter().map(|(k,n)|(k.clone(),*n)).collect(),f.iter().map(|(k,n)|(k.clone(),*n)).collect())));
    if let Some(cached)=cached{return Ok(cached)}
    let (items,fluids)=self.ae2_monitor_live(mount)?;
    let mut all=self.ae2_storage_monitors.borrow_mut();
    if existing.is_none() {
       all.entries.insert(mount.identity,Entry{mount:mount.clone(),signature,configuration,target_incarnation:incarnation,monitor:Monitor::default(),ready:false,observed:(items,fluids)});
       return Ok((Vec::new(),Vec::new()));
    }
    let e=all.entries.get_mut(&mount.identity).expect("existing monitor");
    e.mount=mount.clone();e.signature=signature;e.configuration=configuration;e.target_incarnation=incarnation;e.ready=true;e.observed=(items.clone(),fluids.clone());e.monitor.update(items.clone(),fluids.clone());
    Ok((items.into_iter().collect(),fluids.into_iter().collect()))
 }
 pub(super) fn ae2_monitor_ready(&self,mount:&StorageMount)->bool {
    let incarnation=self.ae2_monitor_target_incarnation(mount);let signature=signature(mount);
    self.ae2_storage_monitors.borrow().entries.get(&mount.identity).is_some_and(|e|e.ready && Some(e.target_incarnation)==incarnation && e.signature==signature)
 }
 pub(super) fn ae2_monitor_invalidate(&self,id:NodeIdentity){if let Some(e)=self.ae2_storage_monitors.borrow_mut().entries.get_mut(&id){e.monitor.invalidate();}}
 /// ConfigManager.putSetting always notifies its listener, even if the enum
 /// value is unchanged. Invalidate the actual mounted handler on that event.
 pub(super) fn ae2_monitor_setting_changed(&self,cable:super::ae2_global::GridAddress,target:super::ae2_global::GridAddress){
    for entry in self.ae2_storage_monitors.borrow_mut().entries.values_mut(){
       if entry.mount.cable==cable && entry.mount.target==target {entry.monitor.invalidate();}
    }
 }
 /// Called per active level on the AE2 server clock, not vanilla world time.
 pub(super) fn ae2_tick_storage_monitors(&mut self){
    let now=self.ae2_server_tick();let network=self.ae2_network();let services=self.ae2_event_service_ids(&network);
    let owners=self.ae2_node_player_owners(&network);let mut seen_recursive=BTreeSet::new();let mut recursive_triggers=BTreeSet::new();
    // Register loaded external monitors even before a terminal first queries
    // them, just as the source storage provider is mounted at node readiness.
    if let Ok(analysis)=network.analyze(){
       for component in &analysis.components {
          let has_storage_bus=component.positions.iter().filter_map(|p|super::ae2_global::GridAddress::from_network(*p)).any(|address| {
              let Some(horizon_client::containers::BlockEntity::Ae2(machine))=self.ae2_block_entity(address) else{return false};
              let Some(horizon_world::nbt::Tag::List(parts))=machine.config.get(&"parts".into()) else{return false};
              parts.iter().any(|part|part.get("id")==Some(&horizon_world::nbt::Tag::String("ae2:storage_bus".into())))
          });
          if !has_storage_bus {continue}
          let Some(anchor)=component.positions.iter().find(|p|analysis.nodes.get(p).is_some_and(|n|n.online)).and_then(|p|super::ae2_global::GridAddress::from_network(*p)) else {continue};
          let mounts=self.ae2_storage_mounts_encoded(anchor).into_iter().chain(self.ae2_fluid_storage_mounts_encoded(anchor));
          for mount in mounts {
             if !self.ae2_address_parts(mount.cable).is_some_and(|(dim,_)|dim==self.dim){continue}
             if self.ae2_has_interface_face(mount.target,mount.face) {
                if let Some(incarnation)=self.ae2_monitor_target_incarnation(&mount) {
                   let side=match mount.face {horizon_core::geometry::Direction::East=>0,horizon_core::geometry::Direction::West=>1,horizon_core::geometry::Direction::Up=>2,horizon_core::geometry::Direction::Down=>3,horizon_core::geometry::Direction::South=>4,horizon_core::geometry::Direction::North=>5};
                   let target_part=network.part_identities.get(&(mount.target.network_pos(),side));
                   let target=format!("{:?}:{:?}:{incarnation}:{target_part:?}",mount.target,mount.face);
                   seen_recursive.insert(mount.identity);
                   let prior=self.ae2_storage_monitors.borrow_mut().recursive_mounts.insert(mount.identity,(mount.cable,target.clone()));
                   if prior.as_ref().is_none_or(|(_,old)|old!=&target) {if let Some(uuid)=owners.get(&mount.identity){recursive_triggers.insert(*uuid);}}
                }
                continue
             }
             let known=self.ae2_storage_monitors.borrow().entries.contains_key(&mount.identity);
             if !known {let _=self.ae2_monitor_read(&mount);}
          }
       }
    }
    let stale:Vec<_>=self.ae2_storage_monitors.borrow().recursive_mounts.iter().filter_map(|(id,(cable,_))|(!seen_recursive.contains(id) && self.ae2_address_parts(*cable).is_some_and(|(dim,_)|dim==self.dim)).then_some(*id)).collect();
    for id in stale {self.ae2_storage_monitors.borrow_mut().recursive_mounts.remove(&id);}
    for uuid in recursive_triggers {self.ae2_queue_advancement(uuid,"ae2:recursive_networking");}
    let list:Vec<_>=self.ae2_storage_monitors.borrow().entries.iter().map(|(id,e)|(*id,e.clone())).collect();
    for (id,entry) in list {
       if !self.ae2_address_parts(entry.mount.cable).is_some_and(|(dim,_)|dim==self.dim){continue}
       let current=self.ae2_storage_mounts_encoded(entry.mount.cable).into_iter().chain(self.ae2_fluid_storage_mounts_encoded(entry.mount.cable)).find(|m|m.identity==id);
       let Some(mount)=current else {self.ae2_storage_monitors.borrow_mut().entries.remove(&id);continue};
       if self.ae2_has_interface_face(mount.target,mount.face) {self.ae2_storage_monitors.borrow_mut().entries.remove(&id);continue}
       let service=services.as_ref().and_then(|s|s.get(&id).copied());
       let (min,max)=horizon_client::ae2_tick_config::rates(horizon_client::ae2_tick_config::Kind::StorageBus);
       let incarnation=self.ae2_monitor_target_incarnation(&mount);
       let result=self.ae2_monitor_live(&mount);
       let changed_target=incarnation.is_some_and(|n|n!=entry.target_incarnation);
       let changed_external=result.as_ref().is_ok_and(|pair|*pair!=entry.observed);
       let config_changed=entry.signature!=signature(&mount) || entry.configuration!=configuration(&mount);
       if !self.ae2_device_scheduler.borrow_mut().due(id,service,now,min,max,false,changed_target || changed_external || config_changed){continue}
       let changed=if let Ok((items,fluids))=result {let mut all=self.ae2_storage_monitors.borrow_mut();if let Some(e)=all.entries.get_mut(&id){e.mount=mount.clone();e.signature=signature(&mount);e.configuration=configuration(&mount);if let Some(n)=incarnation {e.target_incarnation=n;}e.observed=(items.clone(),fluids.clone());e.ready=true;e.monitor.update(items,fluids)}else{false}}else{false};
       self.ae2_device_scheduler.borrow_mut().completed(id,now,if changed{super::ae2_tick_tracker::Modulation::Urgent}else{super::ae2_tick_tracker::Modulation::Slower});
    }
 }
}
