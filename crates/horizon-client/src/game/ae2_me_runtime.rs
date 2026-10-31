//! Physical-identity ME P2P link scheduler. A successful callback changes
//! the real host's connection registry and emits ordered journal events.
use ae2_network_core::{me_tick::MeTickSchedule, NodeIdentity, Pos};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug)]
pub(super) struct MePort {
    pub identity: NodeIdentity,
    pub pos: Pos,
    pub dimension: i32,
    pub frequency: i16,
    pub output: bool,
    pub online: bool,
    pub carrier: Vec<NodeIdentity>,
    pub service: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Signature {
    frequency: i16, output: bool, online: bool,
    carrier: Vec<NodeIdentity>, service: Option<u64>, peers: Vec<(NodeIdentity, bool, bool)>,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct LinkChange { pub input: Pos, pub a: NodeIdentity, pub b: NodeIdentity, pub added: bool }
#[derive(Clone, Default)]
pub(super) struct MeRuntime {
    tick: std::sync::Arc<std::sync::atomic::AtomicU64>,
    schedule: MeTickSchedule,
    signatures: BTreeMap<NodeIdentity, Signature>,
    links: BTreeMap<NodeIdentity, BTreeSet<NodeIdentity>>,
}
impl MeRuntime {
    pub fn edges(&self) -> Vec<(NodeIdentity, NodeIdentity)> {
        self.links.iter().flat_map(|(a, outputs)| outputs.iter().map(move |b| (*a,*b))).collect()
    }
    pub fn advance(&mut self) { let _ = self.tick.fetch_update(std::sync::atomic::Ordering::Relaxed, std::sync::atomic::Ordering::Relaxed, |n| Some(n.saturating_add(1))); }
    pub fn now(&self) -> u64 { self.tick.load(std::sync::atomic::Ordering::Relaxed) }
    pub fn clock(&self) -> std::sync::Arc<std::sync::atomic::AtomicU64> { self.tick.clone() }
    /// A node changing grid is removed from the old TickManager and added to
    /// the new one at the mutation tick, even while world ticks are frozen.
    pub fn observe_services(&mut self, services: &ae2_network_core::service_events::OrderedServices, tick: u64) -> Result<(), String> {
        let (min,max)=horizon_client::ae2_tick_config::rates(horizon_client::ae2_tick_config::Kind::MeTunnel);
        for (node, signature) in &mut self.signatures {
            let Some(next) = services.service_id(*node) else { continue };
            if signature.service.is_some_and(|old| old != next) {
                self.schedule.remove(*node);
                self.schedule.register_with_rates(*node, tick,min,max).map_err(str::to_owned)?;
            }
            signature.service = Some(next);
        }
        Ok(())
    }
    pub fn level_end(&mut self, dimension: i32, ports: &[MePort]) -> Result<Vec<LinkChange>, &'static str> {
        let now = self.now();
        let (min,max)=horizon_client::ae2_tick_config::rates(horizon_client::ae2_tick_config::Kind::MeTunnel);
        let seen: BTreeSet<_>=ports.iter().map(|p|p.identity).collect();
        let gone:Vec<_>=self.signatures.keys().filter(|id|!seen.contains(id)).copied().collect();
        for id in gone { self.schedule.remove(id);self.signatures.remove(&id);self.links.remove(&id); }
        // Node destruction already tears down its physical connections; do
        // not emit a second disconnect for an absent endpoint.
        for outputs in self.links.values_mut() { outputs.retain(|id|seen.contains(id)); }
        for port in ports {
            if !self.schedule.contains(port.identity) { self.schedule.register_with_rates(port.identity,now,min,max)?; }
            if self.signatures.get(&port.identity).and_then(|old| old.service)
                .zip(port.service).is_some_and(|(old,new)| old != new) {
                // GridNode.setGrid removes/re-adds the tickable on the new
                // service, creating a fresh sleeping 12-tick tracker.
                self.schedule.remove(port.identity);
                self.schedule.register_with_rates(port.identity,now,min,max)?;
            }
            let mut peers:Vec<_>=ports.iter().filter(|other|other.identity!=port.identity && other.carrier==port.carrier && other.frequency==port.frequency)
                .map(|p|(p.identity,p.output,p.online)).collect();peers.sort();
            let signature=Signature {frequency:port.frequency,output:port.output,online:port.online,carrier:port.carrier.clone(),service:port.service,peers};
            if self.signatures.get(&port.identity)!=Some(&signature) {
                if !port.output || self.links.get(&port.identity).is_some_and(|links|!links.is_empty()) { self.schedule.wake(port.identity)?; }
                self.signatures.insert(port.identity,signature);
            }
        }
        let mut changes=Vec::new();
        for id in self.schedule.due(now) {
            let Some(port)=ports.iter().find(|p|p.identity==id && p.dimension==dimension) else {continue};
            let selected = ports.iter().find(|p| !p.output && p.frequency == port.frequency && p.carrier == port.carrier)
                .is_some_and(|p| p.identity == id);
            let desired:BTreeSet<_>=if selected && !port.output && port.online {
                ports.iter().filter(|p|p.output && p.online && p.frequency==port.frequency && p.carrier==port.carrier).map(|p|p.identity).collect()
            } else {BTreeSet::new()};
            let current=self.links.entry(id).or_default();
            for other in current.difference(&desired) {changes.push(LinkChange {input:port.pos,a:NodeIdentity {outer:true,..id},b:NodeIdentity {outer:true,..*other},added:false});}
            for other in desired.difference(current) {changes.push(LinkChange {input:port.pos,a:NodeIdentity {outer:true,..id},b:NodeIdentity {outer:true,..*other},added:true});}
            *current=desired;self.schedule.completed_sleep(id,now)?;
        }
        Ok(changes)
    }
}
impl super::Game<'_> {
    /// TickHandler's server clock advances even when vanilla game elements are frozen.
    pub(super) fn ae2_server_tick(&self) -> u64 { self.ae2_me_runtime.borrow().now() }
    // Compatibility for the Anchor until its diagnostics migrate to the server clock.
    pub(super) fn ae2_running_tick(&self) -> u64 { self.ae2_server_tick() }
    pub(super) fn ae2_advance_running_tick(&self) { self.ae2_me_runtime.borrow_mut().advance(); }
    /// Level-end callback mirrors ME tunnel TickManager requests and emits
    /// physical outer-node connection changes in the same committed order.
    pub(super) fn ae2_tick_me_level(&mut self) {
        let network = self.ae2_network();
        let Ok(analysis) = network.analyze() else { return };
        let mut ports = Vec::new();
        let service_ids=self.ae2_event_service_ids(&network);
        for port in self.ae2_p2p_ports().into_iter().filter(|p| p.id == "ae2:me_p2p_tunnel") {
            let Some(identity) = network.part_identities.get(&(port.pos,port.face)).copied() else { continue };
            let Some(address) = super::ae2_global::GridAddress::from_network(port.pos) else { continue };
            let Some((dim,_)) = self.ae2_address_parts(address) else { continue };
            let endpoint = network.part_endpoints.get(&(port.pos,port.face)).copied().unwrap_or(port.pos);
            let Some(node) = analysis.nodes.get(&endpoint) else { continue };
            let Some(status) = network.part_status(&analysis,port.pos,port.face) else { continue };
            let mut carrier = BTreeSet::new();
            for pos in &analysis.components[node.component].positions {
                if let Some(id) = network.node_identities.get(pos) { carrier.insert(*id); }
                if let Some(device) = network.nodes.get(pos) {
                    for part in &device.attached {
                        if let Some(id) = network.part_identities.get(&(*pos,part.face)) { carrier.insert(*id); }
                    }
                }
            }
            ports.push(MePort { identity, pos:port.pos, dimension:super::ae2_event_live::ordinal(dim),
                frequency:port.frequency, output:port.output, online:status.online,
                carrier:carrier.into_iter().collect(), service:service_ids.as_ref().and_then(|ids| ids.get(&identity).copied()) });
        }
        let changes = match self.ae2_me_runtime.borrow_mut().level_end(
            super::ae2_event_live::ordinal(self.dim), &ports) {
            Ok(changes) => changes,
            Err(error) => { log::warn!("AE2 ME tunnel scheduling failed: {error}"); return; }
        };
        for change in &changes {
            let Some(address) = super::ae2_global::GridAddress::from_network(change.input) else { continue };
            let Some((dim,pos)) = self.ae2_address_parts(address) else { continue };
            if dim != self.dim { continue; }
            self.world.block_entities.record_ae2_me_link(pos, change.a, change.b, change.added);
        }
        if !changes.is_empty() { let _ = self.ae2_apply_anchor_callbacks(); }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn id(n:u64)->NodeIdentity {NodeIdentity {host:n,part:Some([n as i32,0,0,0]),outer:false}}
    fn port(n:u64,output:bool)->MePort {MePort {identity:id(n),pos:Pos{x:n as i32,y:64,z:0},dimension:0,frequency:7,output,online:true,carrier:vec![id(1),id(2)],service:Some(1)}}
    #[test]
    fn link_changes_only_on_the_scheduled_running_tick() {
        let mut runtime=MeRuntime::default();let mut ports=vec![port(1,false),port(2,true)];
        for _ in 0..12 {runtime.advance();assert!(runtime.level_end(0,&ports).unwrap().is_empty());}
        runtime.advance();let added=runtime.level_end(0,&ports).unwrap();assert_eq!(added.len(),1);assert!(added[0].added);
        ports[0].online=false;
        // No level-end call corresponds to a frozen world: registry is stable.
        assert_eq!(runtime.edges().len(),1);
        for _ in 0..19 {runtime.advance();assert!(runtime.level_end(0,&ports).unwrap().is_empty());}
        runtime.advance();let removed=runtime.level_end(0,&ports).unwrap();assert_eq!(removed.len(),1);assert!(!removed[0].added);
        assert!(runtime.edges().is_empty());
    }
    #[test]
    fn actual_grid_service_change_recreates_the_source_tick_tracker() {
        let mut runtime=MeRuntime::default();let mut ports=vec![port(1,false),port(2,true)];
        for _ in 0..13 {runtime.advance();runtime.level_end(0,&ports).unwrap();}
        assert_eq!(runtime.edges().len(),1);
        ports[0].service=Some(2);ports[1].online=false;
        for _ in 0..12 {runtime.advance();assert!(runtime.level_end(0,&ports).unwrap().is_empty());}
        runtime.advance();let changes=runtime.level_end(0,&ports).unwrap();
        assert_eq!(changes.len(),1);assert!(!changes[0].added);
    }
    #[test]
    fn service_migration_uses_mutation_time_even_when_replayed_later() {
        let mut runtime=MeRuntime::default();
        let mut services=ae2_network_core::service_events::OrderedServices::default();
        services.add_node(id(1),None).unwrap(); services.ensure_grid(id(1)).unwrap();
        let old=services.service_id(id(1)).unwrap();
        let mut ports=vec![port(1,false),port(2,true)];
        ports[0].service=Some(old);
        for _ in 0..13 {runtime.advance();runtime.level_end(0,&ports).unwrap();}
        services.add_node(id(3),None).unwrap();services.ensure_grid(id(3)).unwrap();
        services.connect(id(3),id(1)).unwrap();
        let changed=services.service_id(id(1)).unwrap();assert_ne!(old,changed);
        // Physical mutation occurred at tick 13, but replay is delayed.
        runtime.advance(); runtime.observe_services(&services,13).unwrap();
        ports[0].service=Some(changed); ports[1].online=false;
        for _ in 0..10 {runtime.advance();assert!(runtime.level_end(0,&ports).unwrap().is_empty());}
        runtime.advance();let changes=runtime.level_end(0,&ports).unwrap();
        assert_eq!(changes.len(),1);assert!(!changes[0].added);
    }

}
