//! Ordered source-side lifecycle shadow; incomplete/unsupported sequences never
//! replace the established graph view with guessed ownership.
use super::{Dim, Game, ae2_global::GridAddress, ae2_provider_registry::ProviderObserver};
use ae2_network_core::{CommittedChannels, Device, DeviceKind, Network, NodeIdentity};
use horizon_client::{
    ae2_event_replay::EventReplay,
    ae2_world::{Machine, MachineKind},
    block_entities::{Ae2Mutation, Ae2MutationKind},
};
use horizon_world::{nbt::Tag, registry::BlockStateId};
use std::collections::{BTreeMap, BTreeSet};
use horizon_client::ae2_event_replay::ReplayCheckpoint;
use ae2_network_core::physical_events::PhysicalTopology;

/// The source-shaped runtime is the normal AE2 lab path. An explicit zero
/// keeps the old implementation available for bounded recovery/diagnostics.
fn source_runtime_setting(value:Option<&std::ffi::OsStr>) -> bool {
    value.is_none_or(|value|value!=std::ffi::OsStr::new("0"))
}
pub(super) fn cpu_formation_enabled() -> bool {
    source_runtime_setting(std::env::var_os("HORIZON_AE2_CPU_FORMATION").as_deref())
}
pub(super) fn ordered_provider_enabled() -> bool {
    source_runtime_setting(std::env::var_os("HORIZON_AE2_PROVIDER_ORDERED").as_deref())
}

pub(super) struct LiveEvents {
    replay: EventReplay,
    pending: Vec<(i32, Ae2Mutation)>,
    failure: Option<String>,
    poisoned: bool,
    provider_failure: Option<String>,
    provider_shadow: Option<ProviderObserver<String, String>>,
    cpu_requests: Option<BTreeMap<u64, Result<BTreeSet<String>, String>>>,
    cpu_request_failure: Option<String>,
    pub(super) energy: super::ae2_energy_observer::EnergyObserver,
    anchor: super::ae2_anchor_observer::AnchorObserver,
}
pub(super) struct OrderedProviderPass {
    service: u64,
    pass: ae2_crafting_core::provider_registry::MediaPass<String>,
}
impl Default for LiveEvents {
    fn default() -> Self {
        let mut replay = EventReplay::default();
        let cpu_enabled = cpu_formation_enabled();
        let enabled = std::env::var_os("HORIZON_AE2_PROVIDER_SHADOW").is_some()
            || ordered_provider_enabled() || cpu_enabled;
        let membership_failure = replay.topology.services.start_membership_recording().err().map(str::to_owned);
        let provider_failure = if enabled { membership_failure.clone() } else { None };
        let provider_shadow = (enabled && provider_failure.is_none()).then(|| {
            let mut observer = ProviderObserver::default();
            if cpu_enabled { observer.enable_cpu(); }
            observer
        });
        let energy=super::ae2_energy_observer::EnergyObserver{failure:membership_failure,..Default::default()};
        Self { replay, pending: Vec::new(), failure: None, poisoned: false, provider_failure, provider_shadow,
            cpu_requests: None, cpu_request_failure: None, energy, anchor: Default::default() }
    }
}
fn trace_energy_and_provider(
    game:&Game<'_>, energy:&mut super::ae2_energy_observer::EnergyObserver,
    anchor:&mut super::ae2_anchor_observer::AnchorObserver,
    provider:Option<&mut ProviderObserver<String,String>>, topology:&mut PhysicalTopology,
    checkpoint:ReplayCheckpoint<'_>,
) {
    // Source GridEnergyStorage reads the configured capacity on add/remove,
    // not continuously while its existing membership remains unchanged.
    energy.per_node_capacity=Some(horizon_client::ae2_common_config::gameplay().grid_energy_per_node);
    if let ReplayCheckpoint::BeforeMutation{dimension,event}=&checkpoint {energy.before(*dimension,event);anchor.before(*dimension,event);}
    let after=match &checkpoint {ReplayCheckpoint::AfterMutation{dimension,event}=>Some((*dimension,*event)),_=>None};
    if let Some(provider)=provider {
        provider.observe_with_membership(topology,checkpoint,&mut |snapshot|game.ae2_provider_definitions(snapshot),
            &mut super::ae2_provider_registry::emitter_definitions,&mut |change|{anchor.membership(change.clone());energy.membership(change)});
    } else {
        match topology.services.take_membership_changes() {
            Ok(changes)=>for change in changes {anchor.membership(Ok(change));energy.membership(Ok(change));},
            Err(error)=>{anchor.membership(Err(error.into()));energy.membership(Err(error.into()));},
        }
    }
    if let Some((dimension,event))=after {energy.after(dimension,event);anchor.after(dimension,event);}
}
impl LiveEvents {
    fn commit_provider_shadow(&mut self, staged: Option<ProviderObserver<String, String>>, checkpoint: &str) {
        if let Some(observer) = staged.as_ref() {
            if let Some(reason) = observer.failure() {
                if self.provider_failure.is_none() {
                    log::warn!("AE2 provider event stream unavailable: {reason}");
                    self.provider_failure = Some(reason.to_owned());
                }
            }
            if let Some(reason) = observer.cpu_failure() {
                if self.provider_shadow.as_ref().and_then(ProviderObserver::cpu_failure).is_none() {
                    log::warn!("AE2 CPU formation event stream unavailable: {reason}");
                }
            }
            if std::env::var_os("HORIZON_AE2_PROVIDER_SHADOW_TRACE").is_some() {
                use std::sync::atomic::{AtomicUsize, Ordering};
                static CHECKPOINTS: AtomicUsize = AtomicUsize::new(0);
                let number = CHECKPOINTS.fetch_add(1, Ordering::Relaxed);
                if number < 4096 {
                    log::info!("AE2 provider shadow checkpoint={} providers={} failure={:?}",
                        checkpoint, observer.provider_count(), observer.failure());
                } else if number == 4096 { log::warn!("AE2 provider shadow trace limit reached"); }
            }
        }
        self.provider_shadow = staged;
    }
}
pub(super) fn ordinal(dim: Dim) -> i32 {
    match dim {
        Dim::Overworld => 0,
        Dim::Nether => 1,
        Dim::End => 2,
        Dim::Spatial => 3,
    }
}
pub(super) fn dimension(n: i32) -> Option<Dim> {
    Some(match n {
        0 => Dim::Overworld,
        1 => Dim::Nether,
        2 => Dim::End,
        3 => Dim::Spatial,
        _ => return None,
    })
}
fn part_priority(part: &Tag, outer: bool) -> u8 {
    match part.get("id") {
        Some(Tag::String(id)) if matches!(id.to_string_lossy().as_str(), "ae2:toggle_bus" | "ae2:inverted_toggle_bus") => 1,
        Some(Tag::String(id)) if id.to_string_lossy() == "ae2:me_p2p_tunnel" && outer => 0,
        _ => 2,
    }
}
impl Game<'_> {
    /// A placed BE's construction snapshot escaped before item components
    /// were imported. Fall back from ordered observers rather than projecting
    /// stale topology, CPU, provider or power state. Sandboxed placements
    /// report this to their guard instead of touching live services.
    pub(super) fn ae2_fault_placement_import(&mut self, reason:String) {
        if self.ae2_formation_staging {
            if self.ae2_formation_import_error.is_none() {self.ae2_formation_import_error=Some(reason);}
            return;
        }
        log::warn!("AE2 ordered placement import unavailable: {reason}");
        let mut live=self.ae2_live_events.borrow_mut();
        live.poisoned=true;
        live.failure=Some(reason.clone());
        live.provider_failure=Some(reason.clone());
        live.energy.failure=Some(reason.clone());
        if let Some(observer)=live.provider_shadow.as_mut() {
            observer.invalidate(reason.clone());
            observer.invalidate_cpu(reason);
        }
    }
    pub(super) fn ae2_next_cpu_break(&self) -> Result<Option<super::ae2_provider_registry::CpuBreakNotice>, String> {
        if !self.ae2_sync_events() { return Err("ordered grid events unavailable".into()); }
        let live=self.ae2_live_events.borrow();
        live.provider_shadow.as_ref().ok_or("CPU formation observer disabled")?.cpu_break_for_dimension(ordinal(self.dim))
    }
    pub(super) fn ae2_ack_cpu_break(&self, sequence:u64) -> Result<(),String> {
        self.ae2_live_events.borrow_mut().provider_shadow.as_mut().ok_or("CPU formation observer disabled")?.acknowledge_cpu_break(sequence)
    }
    pub(super) fn ae2_fault_cpu_break(&self, error:String) {
        if let Some(observer)=self.ae2_live_events.borrow_mut().provider_shadow.as_mut() {
            observer.invalidate_cpu(error);
        }
    }
    pub(super) fn ae2_unloaded_cpu_waiting(&self,node:NodeIdentity) -> Result<Option<ae2_crafting_core::Inventory>,String> {
        self.ae2_live_events.borrow().provider_shadow.as_ref().ok_or("CPU formation observer disabled")?.unloaded_cpu_waiting(node)
    }
    /// Capture the actual cluster before the caller changes its block. No
    /// geometry scan is used to infer membership or replace an owner identity.
    pub(super) fn ae2_cpu_before_removal(&self, pos:(i32,i32,i32)) -> Result<Option<Vec<(NodeIdentity,ae2_crafting_core::cpu_formation::CpuPos,bool)>>,String> {
        if !self.ae2_sync_events() {return Err("ordered grid events unavailable".into());}
        let host=self.world.block_entities.incarnation(&pos).ok_or("CPU incarnation unavailable")?;
        let node=NodeIdentity{host,part:None,outer:false};
        let live=self.ae2_live_events.borrow();
        let cpu=live.provider_shadow.as_ref().ok_or("CPU formation observer disabled")?.cpu()?;
        let Some(cluster)=cpu.cluster_of(&node).map_err(|e|format!("CPU cluster unavailable: {e:?}"))? else{return Ok(None);};
        cluster.members.iter().map(|member|Ok((*member,cpu.position_of(member).map_err(|e|format!("CPU member unavailable: {e:?}"))?,cpu.contains(member)))).collect::<Result<Vec<_>,String>>().map(Some)
    }
    /// Snapshot source-ordered CPU masks and cluster identity for gameplay
    /// network construction. Absence means the opt-in is off; an error means
    /// CPU nodes must be excluded rather than given guessed connectivity.
    pub(super) fn ae2_cpu_network_snapshot(&self) -> Option<Result<BTreeMap<NodeIdentity, (u8, Option<ae2_network_core::Pos>)>, String>> {
        if !cpu_formation_enabled() { return None; }
        Some((|| {
            if !self.ae2_sync_events() { return Err("ordered grid events unavailable".into()); }
            let live = self.ae2_live_events.borrow();
            let observer = live.provider_shadow.as_ref().ok_or("CPU formation observer disabled")?;
            let cpu = observer.cpu()?;
            let mut result = BTreeMap::new();
            for (node, _, core) in cpu.ready_network_nodes().map_err(|e| format!("CPU network snapshot: {e:?}"))? {
                if !live.replay.node_is_ready(node) { return Err("CPU node readiness mismatch".into()); }
                let faces = live.replay.topology.node_faces(node).ok_or("CPU node face mask unavailable")?;
                let expected_faces = if core.is_some() { 63 } else { 0 };
                if faces != expected_faces { return Err("CPU formation face mask mismatch".into()); }
                let group = core.map(|core| {
                    let dim = dimension(core.dimension).ok_or("invalid CPU core dimension")?;
                    GridAddress::from_local(dim, (core.x,core.y,core.z)).map(GridAddress::network_pos).ok_or("invalid CPU core address")
                }).transpose()?;
                if result.insert(node, (faces, group)).is_some() { return Err("duplicate CPU node identity".into()); }
            }
            Ok(result)
        })())
    }
    /// Publish the actual registered CPU clusters once all loaded dimensions
    /// have delivered their world and ready callbacks for this server tick.
    pub(super) fn ae2_cpu_service_end_tick(&self) {
        if !cpu_formation_enabled() || !self.ae2_sync_events() { return; }
        let clusters = (|| {
            let mut live = self.ae2_live_events.borrow_mut();
            let observer = live.provider_shadow.as_mut().ok_or("CPU formation observer disabled")?;
            observer.cpu_service_end_tick();
            observer.cpu()?.published_clusters().map_err(|e| format!("CPU published clusters: {e:?}"))
        })();
        let requests = clusters.map(|clusters| {
            let mut services = BTreeMap::<u64, Result<BTreeSet<String>,String>>::new();
            for cluster in clusters {
                let entry=services.entry(cluster.service).or_insert_with(||Ok(BTreeSet::new()));
                if entry.is_err() { continue; }
                match self.ae2_cpu_waiting_owner(&cluster.members) {
                    Ok(Some((_,waiting))) => if let Ok(keys)=entry { keys.extend(waiting.into_iter().filter_map(|(key,count)|(count>0).then_some(key))); },
                    Ok(None) => {},
                    Err(error) => *entry=Err(error),
                }
            }
            services
        });
        let mut live=self.ae2_live_events.borrow_mut();
        match requests {
            Ok(requests)=>{live.cpu_requests=Some(requests);live.cpu_request_failure=None;},
            Err(error)=>{live.cpu_requests=None;live.cpu_request_failure=Some(error);},
        }
    }
    /// Provider registry faults do not poison the independent topology view.
    pub(super) fn ae2_provider_events_fault(&self, reason: String) {
        let mut live = self.ae2_live_events.borrow_mut();
        if live.provider_failure.is_none() {
            log::warn!("AE2 provider event stream unavailable: {reason}");
            live.provider_failure = Some(reason.clone());
        }
        if let Some(observer) = live.provider_shadow.as_mut() { observer.invalidate(reason); }
    }
    pub(super) fn ae2_provider_events_failure(&self) -> Option<String> {
        self.ae2_live_events.borrow().provider_failure.clone()
    }
    fn ae2_crafting_anchor_service(&self, address: GridAddress) -> Result<u64, String> {
        if !self.ae2_sync_events() { return Err("ordered grid events unavailable".into()); }
        let (dim,pos)=self.ae2_address_parts(address).ok_or("invalid crafting anchor")?;
        let world=self.ae2_world_for(dim).ok_or("crafting level unavailable")?;
        let host=world.block_entities.incarnation(&pos).ok_or("crafting node unavailable")?;
        let node=NodeIdentity{host,part:None,outer:false};
        self.ae2_live_events.borrow().replay.topology.services.service_id(node).ok_or("crafting service unavailable".into())
    }
    pub(super) fn ae2_crafting_emittable_keys(&self, pos: (i32,i32,i32)) -> Result<Vec<String>,String> {
        let service=self.ae2_ordered_provider_service(pos)?;
        self.ae2_live_events.borrow().provider_shadow.as_ref().ok_or("crafting providers unavailable")?.emittable_keys(service)
    }
    /// Last server-end-tick request state for the actual emitter face node.
    /// None means any key; errors remain distinct from a known false result.
    pub(super) fn ae2_crafting_is_requesting(&self, address: super::ae2_parts::PartAddress, key: Option<&str>) -> Result<bool,String> {
        if !self.ae2_sync_events() { return Err("ordered grid events unavailable".into()); }
        let (dim,pos)=self.ae2_address_parts(address.host).ok_or("invalid emitter address")?;
        let world=self.ae2_world_for(dim).ok_or("emitter level unavailable")?;
        let host=world.block_entities.incarnation(&pos).ok_or("emitter host unavailable")?;
        let Some(horizon_client::containers::BlockEntity::Ae2(machine))=world.block_entities.get(&pos) else {return Err("emitter host changed".into());};
        let face=["east","west","up","down","south","north"].get(usize::from(address.face)).ok_or("invalid emitter face")?;
        let Some(Tag::List(parts))=machine.config.get(&"parts".into()) else {return Err("emitter parts unavailable".into());};
        let mut matches=parts.iter().filter(|part|part.get("face")==Some(&Tag::String((*face).into())));
        let part=matches.next().ok_or("emitter face unavailable")?;
        if matches.next().is_some() || !matches!(part.get("id"),Some(Tag::String(id)) if id.to_string_lossy()=="ae2:level_emitter") {
            return Err("emitter face changed".into());
        }
        let identity=super::ae2_channels::part_identity(part).ok_or("emitter identity unavailable")?;
        let live=self.ae2_live_events.borrow();
        let service=live.replay.topology.services.service_id(NodeIdentity{host,part:Some(identity),outer:false}).ok_or("emitter service unavailable")?;
        if let Some(error)=&live.cpu_request_failure {return Err(error.clone());}
        let requests=live.cpu_requests.as_ref().ok_or("CPU requests not yet published")?;
        let Some(keys)=requests.get(&service) else {return Ok(false);};
        let keys=keys.as_ref().map_err(|error|error.clone())?;
        Ok(key.map_or(!keys.is_empty(),|key|keys.contains(key)))
    }
    fn ae2_published_cpus(&self) -> Result<Vec<ae2_crafting_core::cpu_formation::PublishedCpu<NodeIdentity>>,String> {
        if !self.ae2_sync_events() {return Err("ordered grid events unavailable".into());}
        let live=self.ae2_live_events.borrow();
        live.provider_shadow.as_ref().ok_or("CPU formation observer disabled")?.cpu()?.published_clusters()
            .map_err(|e|format!("CPU published clusters: {e:?}"))
    }
    pub(super) fn ae2_crafting_waiting_owners(&self, address: GridAddress, key:&str) -> Result<Vec<GridAddress>,String> {
        let service=self.ae2_crafting_anchor_service(address)?;
        let mut owners=Vec::new();
        for cluster in self.ae2_published_cpus()?.into_iter().filter(|cluster|cluster.service==service) {
            if let Some((owner,_))=self.ae2_cpu_waiting_owner_for_key(&cluster.members,Some(key))? {owners.push(owner);}
        }
        Ok(owners)
    }
    pub(super) fn ae2_observed_cpu_candidates(&self, pos:(i32,i32,i32), required:u64) -> Result<Vec<(GridAddress,u64,u32)>,String> {
        let anchor=self.ae2_local_address(pos).ok_or("invalid crafting anchor")?;
        let service=self.ae2_crafting_anchor_service(anchor)?;
        let mut candidates=Vec::new();
        for cluster in self.ae2_published_cpus()?.into_iter().filter(|cluster|cluster.service==service && cluster.storage>=required) {
            if self.ae2_cpu_waiting_owner(&cluster.members)?.is_some() {continue;}
            let (_,core,resident)=cluster.members.iter().find(|(node,_,_)|*node==cluster.core).ok_or("CPU core unavailable")?;
            if !*resident {return Err("CPU core not resident".into());}
            let dim=dimension(core.dimension).ok_or("invalid CPU core dimension")?;
            let address=GridAddress::from_local(dim,(core.x,core.y,core.z)).ok_or("invalid CPU core address")?;
            if self.ae2_online_encoded(address) {candidates.push((address,cluster.storage,cluster.coprocessors));}
        }
        Ok(candidates)
    }
    pub(super) fn ae2_observed_cpu_capacity(&self, owner:GridAddress) -> Result<(u64,u32),String> {
        let service=self.ae2_crafting_anchor_service(owner)?;
        let mut found=None;
        for cluster in self.ae2_published_cpus()?.into_iter().filter(|cluster|cluster.service==service) {
            if self.ae2_cpu_waiting_owner(&cluster.members)?.is_some_and(|(address,_)|address==owner) {
                if found.is_some() {return Err("CPU owner appears in multiple published clusters".into());}
                found=Some((cluster.storage,cluster.coprocessors));
            }
        }
        found.ok_or("CPU owner is not in a published formed cluster".into())
    }
    fn ae2_ordered_provider_service(&self, pos: (i32, i32, i32)) -> Result<u64, String> {
        if !self.ae2_sync_events() { return Err("ordered grid events unavailable".into()); }
        let host = self.world.block_entities.incarnation(&pos).ok_or("node incarnation unavailable")?;
        let node = NodeIdentity { host, part:None, outer:false };
        let live = self.ae2_live_events.borrow();
        if let Some(reason) = &live.provider_failure { return Err(reason.clone()); }
        if live.provider_shadow.is_none() { return Err("ordered provider runtime disabled".into()); }
        live.replay.topology.services.service_id(node).ok_or("node service unavailable".into())
    }
    pub(super) fn ae2_ordered_provider_definitions(&self, pos: (i32, i32, i32)) -> Result<Vec<super::ae2_provider_registry::PlanningDefinition>, String> {
        let service = self.ae2_ordered_provider_service(pos)?;
        let live = self.ae2_live_events.borrow();
        live.provider_shadow.as_ref().ok_or("ordered provider runtime disabled")?.planning_definitions(service)
    }
    pub(super) fn ae2_ordered_provider_pass(&self, pos: (i32, i32, i32), definition: &str) -> Result<Option<OrderedProviderPass>, String> {
        let service = self.ae2_ordered_provider_service(pos)?;
        let live = self.ae2_live_events.borrow();
        Ok(live.provider_shadow.as_ref().ok_or("ordered provider runtime disabled")?
            .media_pass(service, definition)?.map(|pass| OrderedProviderPass { service, pass }))
    }
    pub(super) fn ae2_ordered_provider_next(&self, pos: (i32, i32, i32), pass: &mut OrderedProviderPass) -> Result<Option<super::ae2_provider_registry::ProviderMedium>, String> {
        let service = self.ae2_ordered_provider_service(pos)?;
        if service != pass.service { return Err("provider service changed during media pass".into()); }
        let mut live = self.ae2_live_events.borrow_mut();
        live.provider_shadow.as_mut().ok_or("ordered provider runtime disabled")?.next_medium(service, &mut pass.pass)
    }
    pub(super) fn ae2_ordered_provider_contains(&self, pos: (i32, i32, i32), medium: &super::ae2_provider_registry::ProviderMedium,
        definition: &str, pass: &OrderedProviderPass) -> Result<bool, String> {
        let service = self.ae2_ordered_provider_service(pos)?;
        if service != pass.service { return Err("provider service changed during media pass".into()); }
        let live = self.ae2_live_events.borrow();
        live.provider_shadow.as_ref().ok_or("ordered provider runtime disabled")?.contains_medium(service, medium.node, definition, &pass.pass)
    }
    /// Read-only diagnostic for a real node's currently observed service.
    /// This does not inspect or consume the shared round-robin cursor.
    pub(super) fn ae2_provider_shadow_status(&self, pos: (i32, i32, i32), item: &str) -> Result<serde_json::Value, String> {
        if !self.ae2_sync_events() { return Err("ordered grid events unavailable".into()); }
        let live = self.ae2_live_events.borrow();
        if let Some(reason) = &live.provider_failure { return Err(reason.clone()); }
        let observer = live.provider_shadow.as_ref().ok_or("provider shadow is disabled")?;
        let host = self.world.block_entities.incarnation(&pos).ok_or("node incarnation unavailable")?;
        let node = NodeIdentity { host, part: None, outer: false };
        let service = live.replay.topology.services.service_id(node).ok_or("node service unavailable")?;
        let item = horizon_client::items::ItemStack::new(item, 1);
        let key = horizon_client::item_data::canon_stack(Some(&item));
        observer.summary(service, &key)
    }
    /// Bounded, non-consuming view of the actual callback observer. This is
    /// diagnostic only: jobs and emitter signals still use their existing path.
    pub(super) fn ae2_cpu_formation_status(&self, pos: (i32, i32, i32)) -> Result<serde_json::Value, String> {
        if !self.ae2_sync_events() { return Err("ordered grid events unavailable".into()); }
        let live = self.ae2_live_events.borrow();
        let observer = live.provider_shadow.as_ref().ok_or("CPU formation observer disabled")?;
        let cpu = observer.cpu()?;
        let host = self.world.block_entities.incarnation(&pos).ok_or("node incarnation unavailable")?;
        let node = NodeIdentity { host, part: None, outer: false };
        let service = live.replay.topology.services.service_id(node).ok_or("node service unavailable")?;
        let published = cpu.published(service).map_err(|e| format!("CPU service: {e:?}"))?;
        let cluster = cpu.cluster_of(&node).map_err(|e| format!("CPU cluster: {e:?}"))?;
        let format_pos = |p: ae2_crafting_core::cpu_formation::CpuPos| [p.dimension, p.x, p.y, p.z];
        let cluster = match cluster {
            None => serde_json::Value::Null,
            Some(cluster) => {
                let members = cluster.members.iter().map(|id| {
                    let position = cpu.position_of(id).map_err(|e| format!("CPU member: {e:?}"))?;
                    Ok(serde_json::json!({"host": id.host, "position": format_pos(position)}))
                }).collect::<Result<Vec<_>, String>>()?;
                let core = cpu.position_of(&cluster.core).map_err(|e| format!("CPU core: {e:?}"))?;
                serde_json::json!({"id": cluster.id, "min": format_pos(cluster.min), "max": format_pos(cluster.max),
                    "core": format_pos(core), "members": members, "storage": cluster.storage,
                    "coprocessors": cluster.coprocessors, "published": published.contains(&cluster.id)})
            }
        };
        Ok(serde_json::json!({"node": host, "ready": live.replay.node_is_ready(node), "service": service, "cluster": cluster}))
    }
    pub(super) fn ae2_event_node_is_ready(&self,id:NodeIdentity)->bool {
        let live=self.ae2_live_events.borrow();
        !live.poisoned && live.failure.is_none() && live.replay.node_is_ready(id)
    }
    pub(super) fn ae2_take_anchor_callbacks(&self)->Result<Vec<super::ae2_anchor_observer::AnchorCallback>,String> {
        if !self.ae2_sync_events(){return Err("ordered anchor lifecycle unavailable".into())}
        self.ae2_live_events.borrow_mut().anchor.take_callbacks()
    }
    pub(super) fn ae2_ordered_anchor_chunks(&self,id:NodeIdentity,dim:Dim)->Result<Option<BTreeSet<(i32,i32)>>,String> {
        if !self.ae2_sync_events(){return Err("ordered anchor lifecycle unavailable".into())}
        self.ae2_live_events.borrow().anchor.chunks(id,ordinal(dim))
    }
    pub(super) fn ae2_phantom_is_ready(&self, id: NodeIdentity, pos: ae2_network_core::Pos) -> bool {
        if !self.ae2_sync_events() { return false; }
        let live = self.ae2_live_events.borrow();
        live.replay.node_is_ready(id)
            && live.replay.topology.exposed_colocated(pos) == Some(NodeIdentity::phantom_proxy(id.host))
    }
    pub(super) fn ae2_project_phantom_nodes(&self, network: &mut Network) -> Result<(), &'static str> {
        if self.ae2_live_events.borrow().replay.topology.colocated_nodes().next().is_none() { return Ok(()); }
        let synced = self.ae2_sync_events();
        let live = self.ae2_live_events.borrow();
        if !synced { return Err("phantom node events unavailable"); }
        live.replay.topology.project_colocated(network)
    }
    /// useWithoutItem reasserts the already-ready proxy and all its default
    /// sides; before onReady the source's null proxy makes this a no-op.
    pub(super) fn ae2_phantom_use(&self, pos: (i32, i32, i32)) {
        if !matches!(self.world.block_entities.get(&pos),Some(horizon_client::containers::BlockEntity::Ae2(m)) if m.kind==MachineKind::DebugPhantomNode) { return; }
        let Some(host) = self.world.block_entities.incarnation(&pos) else { return };
        let Some(address) = self.ae2_local_address(pos) else { return };
        if !self.ae2_sync_events() { return; }
        let main = NodeIdentity { host, part: None, outer: false };
        let mut live = self.ae2_live_events.borrow_mut();
        if !live.replay.node_is_ready(main) { return; }
        let proxy = NodeIdentity::phantom_proxy(host);
        if let Err(error) = live.replay.topology.expose_colocated(address.network_pos(), proxy) {
            log::warn!("AE2 phantom use ignored: {error}");
        }
    }
    fn ae2_event_device(&self, machine: &Machine, state: u32) -> Option<Device> {
        if Machine::for_block(&self.res.facts().get(BlockStateId(state)).name).is_none() {
            return None;
        }
        let mut device = Device::new(super::ae2_grid::kind(machine)?);
        let properties = self.pack.assets.registry.state(BlockStateId(state));
        match machine.kind {
            MachineKind::DebugPhantomNode => device.idle_override = Some(1.0),
            MachineKind::QuantumRing | MachineKind::QuantumLink => {
                // The source node starts with no connectable faces before its
                // first successful multiblock calculation.
                device.faces = 0;
                device.idle_override = Some(22.0);
                device.capacity_override = Some(32);
            }
            MachineKind::GrowthAccelerator => {
                device.faces = super::ae2_grid::growth_accelerator_faces(
                    properties
                        .and_then(|s| s.properties.get("facing"))
                        .map(String::as_str),
                );
            }
            MachineKind::CrystalResonanceGenerator => {
                device.faces = properties.map_or(0, |s| {
                    if s.properties.contains_key("facing") {
                        super::ae2_placement::wap_connection_faces(s)
                    } else {
                        1 << 4
                    }
                });
            }
            MachineKind::PatternProvider => {
                device.faces = super::ae2_grid::pattern_provider_faces(
                    properties
                        .and_then(|s| s.properties.get("push_direction"))
                        .map(String::as_str),
                );
            }
            MachineKind::WirelessAccessPoint => {
                device.faces = properties.map_or(0, super::ae2_placement::wap_connection_faces);
            }
            MachineKind::CableBus => {
                let (kind, color) = super::ae2_parts::cable_device(machine)?;
                device.kind = kind;
                device.color = color;
                if let Some(Tag::List(parts)) = machine.config.get(&"parts".into()) {
                    for part in parts {
                        let Some(Tag::String(face)) = part.get("face") else {
                            return None;
                        };
                        let face = super::ae2_grid::face_index(&face.to_string().ok()?)?;
                        device.faces &= !(1 << face);
                    }
                }
            }
            _ => {}
        }
        Some(device)
    }
    /// Drain every live dimension in global mutation order. If replay fails,
    /// preserve the complete batch for diagnosis/retry and retain the prior
    /// analysis fallback, never an incomplete shadow graph.
    pub(super) fn ae2_sync_events(&self) -> bool {
        let mut live = self.ae2_live_events.borrow_mut();
        if live.poisoned {
            return false;
        }
        for (dim, world) in self.ae2_loaded_worlds() {
            match world.block_entities.take_ae2_mutations() {
                Ok(events) => live
                    .pending
                    .extend(events.into_iter().map(|event| (ordinal(dim), event))),
                Err(error) => {
                    live.failure = Some(error.into());
                    return false;
                }
            }
        }
        if live.pending.len() > 100_000 {
            live.poisoned = true;
            live.failure = Some("ordered event backlog exceeded its bound".into());
            log::warn!("AE2 event replay disabled: ordered event backlog exceeded its bound");
            return false;
        }
        live.pending.sort_by_key(|(_, event)| event.sequence);
        if live.pending.is_empty() {
            return live.failure.is_none();
        }
        // onChunkUnloaded calls disconnect(false) before its grid node is
        // removed. The journal keeps the exact former host incarnation and
        // actual bulk-removal order even after the chunks have been dropped.
        // Stage both the quantum registry and its synthetic link callbacks so
        // a failed topology replay cannot commit only half the transition.
        let has_quantum_unload = live.pending.iter().any(|(_, event)| matches!(&event.kind,
            Ae2MutationKind::EntityRemoved {
                entity: horizon_client::containers::BlockEntity::Ae2(machine), bulk_unload: true, ..
            } if matches!(machine.kind, MachineKind::QuantumRing | MachineKind::QuantumLink)));
        let mut staged_quantum = has_quantum_unload.then(|| self.ae2_quantum_runtime.borrow().clone());
        let mut batch = Vec::with_capacity(live.pending.len());
        for (dim, event) in live.pending.clone() {
            if let Ae2MutationKind::EntityRemoved {
                incarnation, entity: horizon_client::containers::BlockEntity::Ae2(machine), bulk_unload: true,
            } = &event.kind {
                if matches!(machine.kind, MachineKind::QuantumRing | MachineKind::QuantumLink) {
                    let Some(pos) = GridAddress::from_local(match dimension(dim) {
                        Some(value) => value,
                        None => { live.failure=Some("invalid quantum unload dimension".into()); return false; }
                    }, event.position).map(GridAddress::network_pos) else {
                        live.failure=Some("invalid quantum unload position".into()); return false;
                    };
                    for link in staged_quantum.as_mut().expect("quantum unload batch").unloaded(pos,*incarnation) {
                        batch.push((dim, Ae2Mutation {
                            sequence: event.sequence,
                            server_tick: event.server_tick,
                            position: event.position,
                            kind: Ae2MutationKind::QuantumLink { a: link.a, b: link.b, added: link.added },
                        }));
                    }
                }
            }
            batch.push((dim,event));
        }
        let mut staged_runtime = self.ae2_me_runtime.borrow().clone();
        let mut staged_devices = self.ae2_device_scheduler.borrow().clone();
        let mut staged_provider = live.provider_shadow.clone();
        let mut staged_energy = live.energy.clone();
        let mut staged_anchor = live.anchor.clone();
        if let (Some(observer), Some(reason)) = (staged_provider.as_mut(), live.provider_failure.as_ref()) {
            observer.invalidate(reason.clone());
        }
        let encode = |dim, pos| GridAddress::from_local(dimension(dim)?, pos).map(GridAddress::network_pos);
        let device = |m: &Machine, state| self.ae2_event_device(m, state);
        let is_ae2 = |state| self.res.facts().get(BlockStateId(state)).name.starts_with("ae2:");
        let observe = |replay: &EventReplay, tick| {
                staged_runtime.observe_services(&replay.topology.services, tick)?;
                staged_devices.observe_services(&replay.topology.services, tick);
                Ok(())
        };
        let result = live.replay.batch_observed_traced(batch, encode, device, is_ae2, observe,
            |topology,checkpoint|trace_energy_and_provider(self,&mut staged_energy,&mut staged_anchor,staged_provider.as_mut(),topology,checkpoint));
        match result {
            Ok(()) => {
                if let Some(quantum) = staged_quantum { *self.ae2_quantum_runtime.borrow_mut() = quantum; }
                *self.ae2_me_runtime.borrow_mut() = staged_runtime;
                *self.ae2_device_scheduler.borrow_mut() = staged_devices;
                live.energy = staged_energy;
                live.anchor = staged_anchor;
                live.commit_provider_shadow(staged_provider, "batch");
                live.pending.clear();
                live.failure = None;
                true
            }
            Err(error) => {
                if live.failure.as_deref() != Some(&error) {
                    log::warn!("AE2 event replay paused: {error}");
                }
                live.failure = Some(error);
                false
            }
        }
    }
    pub(super) fn ae2_ready_event_level(&self) {
        if !self.ae2_sync_events() {
            return;
        }
        let mut live = self.ae2_live_events.borrow_mut();
        let dim = ordinal(self.dim);
        for chunk in live.replay.ready_chunks(dim) {
            // AE2's TickHandler calls onReady only where vanilla block
            // entities can tick, not merely where the chunk is resident.
            if !super::ticking_chunk(&self.world, chunk.0, chunk.1)
                || !self.tickets.levels().block_ticking(chunk.0,chunk.1) {
                continue;
            }
            let before = live.replay.clone();
            let mut staged_provider = live.provider_shadow.clone();
            let mut staged_energy = live.energy.clone();
            let mut staged_anchor = live.anchor.clone();
            if let (Some(observer), Some(reason)) = (staged_provider.as_mut(), live.provider_failure.as_ref()) {
                observer.invalidate(reason.clone());
            }
            let ready = live.replay.ready_chunk_traced(dim, chunk, |m, state| self.ae2_event_device(m, state),
                |topology,checkpoint|trace_energy_and_provider(self,&mut staged_energy,&mut staged_anchor,staged_provider.as_mut(),topology,checkpoint));
            if let Err(error) = ready {
                if live.failure.as_deref() != Some(&error) {
                    log::warn!("AE2 event readiness paused: {error}");
                }
                break;
            }
            let mut staged_runtime = self.ae2_me_runtime.borrow().clone();
            let mut staged_devices = self.ae2_device_scheduler.borrow().clone();
            if let Err(error) = staged_runtime.observe_services(&live.replay.topology.services, staged_runtime.now()) {
                live.replay = before;
                live.failure = Some(error);
                break;
            }
            staged_devices.observe_services(&live.replay.topology.services, staged_runtime.now());
            *self.ae2_me_runtime.borrow_mut() = staged_runtime;
            *self.ae2_device_scheduler.borrow_mut() = staged_devices;
            live.energy = staged_energy;
            live.anchor = staged_anchor;
            live.commit_provider_shadow(staged_provider, "ready");
            let ready:Vec<_>=self.world.block_entities.iter().filter_map(|(&pos,be)| {
                if (pos.0>>4,pos.2>>4)!=chunk || !matches!(be,horizon_client::containers::BlockEntity::Ae2(m) if matches!(m.kind,MachineKind::QuantumRing|MachineKind::QuantumLink)) {return None}
                let id=NodeIdentity{host:self.world.block_entities.incarnation(&pos)?,part:None,outer:false};
                live.replay.node_is_ready(id).then_some((pos,id))
            }).collect();
            let mut quantum=self.ae2_quantum_runtime.borrow_mut();
            for (pos,id) in ready {
                quantum.node_ready(id);
                if std::env::var_os("HORIZON_AE2_QUANTUM_TRACE").is_some() {log::info!("AE2 quantum onReady dim={:?} pos={pos:?} node={id:?}",self.dim);}
            }
        }
    }
    pub(super) fn ae2_event_forget_level(&self) {
        let mut live = self.ae2_live_events.borrow_mut();
        live.poisoned = true;
        let reason = "loaded dimension was closed without ordered node teardown".to_owned();
        live.failure = Some(reason.clone());
        if live.provider_failure.is_none() { live.provider_failure = Some(reason.clone()); }
        if let Some(observer) = live.provider_shadow.as_mut() { observer.invalidate(reason); }
    }
    pub(super) fn ae2_event_overlay(&self, network: &Network) -> Option<CommittedChannels> {
        if !self.ae2_sync_events() {
            return None;
        }
        let live = self.ae2_live_events.borrow();
        match self
            .ae2_committed_channels
            .with_ordered(&live.replay.topology.services, network)
        {
            Ok(ordered) => Some(ordered),
            Err(error) => {
                if std::env::var_os("HORIZON_AE2_EVENT_TRACE").is_some() {
                    log::info!("AE2 event overlay unavailable: {error}");
                }
                None
            }
        }
    }
    pub(super) fn ae2_event_connection_order(&self, network: &mut Network) {
        if !self.ae2_sync_events() {return;}
        let live=self.ae2_live_events.borrow();
        if self.ae2_committed_channels.with_ordered(&live.replay.topology.services,network).is_err(){return;}
        if let Err(error)=network.record_connection_order(&live.replay.topology.services) {
            if std::env::var_os("HORIZON_AE2_EVENT_TRACE").is_some(){log::info!("AE2 ordered pathing unavailable: {error}");}
        }
    }
    pub(super) fn ae2_event_service_ids(&self, network: &Network) -> Option<BTreeMap<ae2_network_core::NodeIdentity,u64>> {
        if !self.ae2_sync_events() { return None; }
        let live=self.ae2_live_events.borrow();
        self.ae2_committed_channels.with_ordered(&live.replay.topology.services,network).ok()?;
        Some(live.replay.topology.services.node_ids().filter_map(|node|
            live.replay.topology.services.service_id(node).map(|id|(node,id))).collect())
    }
    pub(super) fn ae2_event_commit(
        &self,
        network: &Network,
        analysis: &ae2_network_core::Analysis,
        committed: &CommittedChannels,
    ) {
        if !self.ae2_sync_events() {
            return;
        }
        let mut live = self.ae2_live_events.borrow_mut();
        let mut candidate = live.replay.topology.services.clone();
        if let Err(error) = committed.seed_ordered(&mut candidate) {
            if std::env::var_os("HORIZON_AE2_EVENT_TRACE").is_some() {
                log::info!("AE2 event pathing commit deferred: {error}");
            }
            return;
        }
        let mut controllers = Vec::new();
        let mut priorities = BTreeMap::new();
        for (pos, device) in &network.nodes {
            if let Some(id) = network.node_identities.get(pos) {
                if device.kind == DeviceKind::Controller
                    && analysis.nodes.get(pos).is_some_and(|node| {
                        analysis.components[node.component].controller
                            == ae2_network_core::ControllerState::Online
                    })
                {
                    controllers.push(*id);
                }
                let priority = match device.kind {
                    DeviceKind::DenseCable
                    | DeviceKind::DenseCoveredCable
                    | DeviceKind::DenseSmartCable => 0,
                    DeviceKind::NormalCable
                    | DeviceKind::GlassCable
                    | DeviceKind::CoveredCable
                    | DeviceKind::SmartCable => 1,
                    _ => 2,
                };
                priorities.insert(*id, priority);
            }
        }
        for ((position, _), id) in &network.part_identities {
            let part = GridAddress::from_network(*position)
                .and_then(|address| self.ae2_block_entity(address))
                .and_then(|entity| match entity {
                    horizon_client::containers::BlockEntity::Ae2(machine) => machine.config.get(&"parts".into()),
                    _ => None,
                })
                .and_then(|parts| match parts { Tag::List(parts) => Some(parts), _ => None })
                .and_then(|parts| parts.iter().find(|part| super::ae2_channels::part_identity(part) == id.part));
            if let Some(part) = part {
                priorities.insert(*id, part_priority(part, false));
                priorities.insert(ae2_network_core::NodeIdentity { outer: true, ..*id }, part_priority(part, true));
            }
        }
        if candidate
            .orient_controller_paths(&controllers, |id| *priorities.get(&id).unwrap_or(&2))
            .is_ok()
        {
            live.replay.topology.services = candidate;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_runtime_defaults_on_and_explicit_zero_opts_out() {
        use std::ffi::OsStr;
        assert!(source_runtime_setting(None));
        assert!(!source_runtime_setting(Some(OsStr::new("0"))));
        assert!(source_runtime_setting(Some(OsStr::new("1"))));
        assert!(source_runtime_setting(Some(OsStr::new(""))));
    }
    #[test]
    fn source_virtual_node_flags_set_pathing_priority() {
        let part = |id: &str| Tag::compound([("id", Tag::String(id.into()))]);
        assert_eq!(part_priority(&part("ae2:toggle_bus"), false), 1);
        assert_eq!(part_priority(&part("ae2:toggle_bus"), true), 1);
        assert_eq!(part_priority(&part("ae2:quartz_fiber"), false), 2);
        assert_eq!(part_priority(&part("ae2:quartz_fiber"), true), 2);
        assert_eq!(part_priority(&part("ae2:me_p2p_tunnel"), false), 2);
        assert_eq!(part_priority(&part("ae2:me_p2p_tunnel"), true), 0);
    }
}
