//! Physical EnergyService operations. A transaction stages every service and
//! verifies its real provider incarnations before changing any world storage.
use super::{Game, ae2_global::GridAddress};
use super::ae2_energy_observer::EnergyObserver;
use super::ae2_energy_service::EnergyOverlay;
use ae2_network_core::{Analysis, Network, NodeIdentity, Pos};
use horizon_client::containers::BlockEntity;
use std::collections::{BTreeMap, BTreeSet};

/// Require the current physical graph and the ordered service observer to
/// agree about every member of this one component. Other loading components do
/// not prevent a ready component from being used.
fn component_service(network: &Network, analysis: &Analysis, energy: &EnergyObserver, component: usize) -> Result<u64, String> {
    let group = analysis.components.get(component).ok_or("missing energy component")?;
    let positions: BTreeSet<Pos> = group.positions.iter().copied().collect();
    let mut identities = BTreeSet::<NodeIdentity>::new();
    for pos in &group.positions {
        identities.insert(*network.node_identities.get(pos).ok_or("unidentified energy node")?);
    }
    for ((host, face), id) in &network.part_identities {
        let endpoint = network.part_endpoints.get(&(*host, *face)).copied().unwrap_or(*host);
        if positions.contains(&endpoint) { identities.insert(*id); }
    }
    let mut services = identities.iter().map(|id| energy.lifecycle.service_of(*id).ok_or("energy node not ready"));
    let service = services.next().ok_or("empty energy component")??;
    for item in services { if item? != service { return Err("energy service differs from physical component".into()); } }
    for id in energy.physical.keys() {
        if energy.lifecycle.service_of(*id) == Some(service) && !identities.contains(id) {
            return Err("energy service has a missing physical node".into());
        }
    }
    if !energy.lifecycle.services.contains_key(&service) { return Err("energy service unavailable".into()); }
    Ok(service)
}

impl Game<'_> {
    /// Add each live internal grid buffer once, to a node of that grid. This
    /// is only a power-analysis view; EnergyLifecycle retains actual custody.
    pub(super) fn ae2_project_energy_buffers(&self, network: &mut Network) {
        let Ok(live) = self.ae2_live_events.try_borrow() else { return };
        if live.energy.failure.is_some() { return }
        let Ok(analysis) = network.analyze() else { return };
        let mut groups:BTreeMap<usize,Vec<(u64,usize)>>=BTreeMap::new();let mut bad=BTreeSet::new();
        let mut owner=BTreeMap::new();
        for (index,component) in analysis.components.iter().enumerate(){
            let power=component.power_component;
            match component_service(network,&analysis,&live.energy,index){
                Ok(service)=>{
                    if let Some((other,_))=owner.insert(service,(power,index)){bad.insert(other);bad.insert(power);}
                    groups.entry(power).or_default().push((service,index));
                }
                Err(_)=>{bad.insert(power);}
            }
        }
        for (power,components) in groups {
            if bad.contains(&power){continue}
            for (service,index) in components {
                let component=&analysis.components[index];
                if let (Some(pos),Some(state))=(component.positions.first(),live.energy.lifecycle.services.get(&service)){
                    if state.local()>0.0{network.energy_buffers.insert(*pos,state.local());}
                    for node in &component.positions{network.energy_public_power.insert(*node,state.public_power);}
                }
            }
        }
    }

    /// The returned amount is physically accepted/extracted. Simulation never
    /// mutates source service state or any world entity.
    pub(super) fn ae2_energy_transaction(&mut self, anchor: GridAddress, face: Option<u8>, amount: f64, simulate: bool, inject: bool) -> Result<f64, String> {
        self.ae2_energy_transaction_with(anchor,face,amount,simulate,inject,false)
    }
    fn ae2_energy_transaction_with(&mut self, anchor: GridAddress, face: Option<u8>, amount: f64, simulate: bool, inject: bool, exact:bool) -> Result<f64, String> {
        if !amount.is_finite() || amount < 0.0 { return Err("invalid energy transaction".into()); }
        if !self.ae2_sync_events() { return Err("energy lifecycle unavailable".into()); }
        let network = self.ae2_network();
        let analysis = network.analyze().map_err(str::to_owned)?;
        let host = anchor.network_pos();
        let at = if let Some(face) = face {
            network.part_endpoints.get(&(host, face)).copied().unwrap_or(host)
        } else if network.nodes.contains_key(&host) { host }
        else {
            let endpoints: BTreeSet<_> = network.part_endpoints.iter().filter_map(|((p, _), endpoint)| (*p == host).then_some(*endpoint)).collect();
            if endpoints.len() != 1 { return Err("ambiguous multipart energy owner".into()); }
            *endpoints.iter().next().unwrap()
        };
        self.ae2_energy_transaction_snapshot(&network, &analysis, at, amount, simulate, inject, exact)
    }

    fn ae2_energy_transaction_snapshot(&mut self, network: &Network, analysis: &Analysis, at: Pos, amount: f64, simulate: bool, inject: bool, exact:bool) -> Result<f64, String> {
        let status = analysis.nodes.get(&at).ok_or("energy owner unavailable")?;
        let power = &analysis.power_components[analysis.components[status.component].power_component];
        let original = {
            let live = self.ae2_live_events.borrow();
            if let Some(error) = &live.energy.failure { return Err(error.clone()); }
            live.energy.clone()
        };
        let mut seen_components = BTreeSet::new();
        let mut seen_services = BTreeSet::new();
        let mut order = Vec::new();
        for pos in &power.positions {
            let component = analysis.nodes.get(pos).ok_or("energy node disappeared")?.component;
            if !seen_components.insert(component) { continue }
            let service = component_service(network, analysis, &original, component)?;
            if !seen_services.insert(service) { return Err("energy service spans separate physical components".into()); }
            order.push(service);
        }
        let mut overlay = EnergyOverlay { services: BTreeMap::new() };
        for service in &order { overlay.services.insert(*service, original.lifecycle.services.get(service).ok_or("missing energy service")?.clone()); }
        let result = if inject {
            let left = overlay.inject(&order, amount, simulate).map_err(str::to_owned)?;
            amount - left
        } else { overlay.extract(&order, amount, simulate).map_err(str::to_owned)? };
        if exact && result != amount { return Err("energy provider accepted a partial whole-unit transfer".into()); }
        if simulate { return Ok(result); }

        let mut writes = Vec::new();
        let mut dirty = BTreeSet::new();
        for (service, next) in &overlay.services {
            let old = original.lifecycle.services.get(service).ok_or("missing prior energy service")?;
            if next.local() != old.local() {
                for (node, physical) in &original.physical {
                    if original.lifecycle.service_of(*node) == Some(*service) { dirty.insert(physical.address); }
                }
            }
            for (node, store) in &next.stores {
                let previous = old.stores.get(node).ok_or("missing prior energy store")?;
                if store.creative || store.amount == previous.amount { continue }
                let physical = original.physical.get(node).ok_or("missing physical energy store")?;
                let (dim, pos) = self.ae2_address_parts(physical.address).ok_or("invalid store address")?;
                let world = self.ae2_world_for(dim).ok_or("energy store world unavailable")?;
                if world.block_entities.incarnation(&pos) != Some(node.host) { return Err("energy store identity changed".into()); }
                let Some(BlockEntity::Ae2(machine)) = world.block_entities.get(&pos) else { return Err("energy store disappeared".into()) };
                if machine.energy != previous.amount { return Err("energy store changed during transaction".into()); }
                writes.push((physical.address, store.amount));
                dirty.insert(physical.address);
            }
        }
        // Server-thread ownership means every validation precedes every write.
        for (address, value) in writes {
            let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity_mut(address) else { return Err("energy store disappeared".into()) };
            machine.energy = value;
        }
        for address in dirty { self.ae2_mark_modified(address); }
        let mut live = self.ae2_live_events.borrow_mut();
        for (id, service) in overlay.services { live.energy.lifecycle.services.insert(id, service); }
        Ok(result)
    }

    /// Tick valid overlay groups independently, with watcher callback, real
    /// service-local idle draw, then public power update for each service.
    /// An incomplete unrelated group retains the bounded legacy path without
    /// preventing a ready group from advancing its real source service.
    pub(super) fn ae2_energy_services_tick(&mut self) -> bool {
        if !self.ae2_sync_events() { return false }
        let network=self.ae2_network();let Ok(analysis)=network.analyze() else{return false};
        let snapshot={let live=self.ae2_live_events.borrow();if live.energy.failure.is_some(){return false}live.energy.clone()};
        let mut plans=Vec::new();let mut bad=BTreeSet::new();let mut owner=BTreeMap::new();
        for (index,component) in analysis.components.iter().enumerate(){
            let power=component.power_component;
            let service=match component_service(&network,&analysis,&snapshot,index){Ok(v)=>v,Err(_)=>{bad.insert(power);continue}};
            if let Some(previous)=owner.insert(service,power){bad.insert(previous);bad.insert(power);}
            let Some(pos)=component.positions.first().copied() else{bad.insert(power);continue};
            let base=network.component_base_idle(&component.positions);
            let amount=if base>0.0001{component.idle_per_tick}else{0.1};
            plans.push((power,service,pos,base,amount));
        }
        let multiplier=horizon_client::ae2_common_config::gameplay().usage_multiplier;
        for (power,_,pos,_,amount) in &plans {
            let charged=(*amount*multiplier).min(f64::MAX);
            if !bad.contains(power)&&self.ae2_energy_transaction_snapshot(&network,&analysis,*pos,charged,true,false,false).is_err(){bad.insert(*power);}
        }
        // Source onServerStartTick selects one incumbent passive generator per
        // real energy overlay, including across quartz fiber. A group without
        // a complete ordered view gets no invented generation.
        let mut grouped:BTreeMap<usize,Vec<u64>>=BTreeMap::new();
        for (power,service,_,_,_) in &plans{if !bad.contains(power){grouped.entry(*power).or_default().push(*service);}}
        let mut groups=Vec::new();
        for services in grouped.values(){
            let members:BTreeSet<_>=services.iter().copied().collect();
            let nodes=snapshot.physical.iter().filter_map(|(id,physical)|
                (physical.passive&&snapshot.lifecycle.service_of(*id).is_some_and(|service|members.contains(&service))).then_some(*id)).collect();
            groups.push((services.clone(),nodes));
        }
        let selected=match self.ae2_live_events.borrow_mut().energy.select_passives(&groups){
            Ok(state)=>state,Err(error)=>{log::warn!("AE2 passive power selection stopped: {error}");return false}
        };
        let mut generators=BTreeMap::new();
        for (node,suppressed) in selected {
            if !suppressed {if let Some(service)=snapshot.lifecycle.service_of(node){generators.insert(service,node);}}
        }
        // A temporarily unavailable source view stays on the established
        // physical drain for that one overlay; update its observed mirrors.
        let mut fallback_seen=BTreeSet::new();
        for (power,service,_,_,_) in &plans {
            if bad.contains(power)&&fallback_seen.insert(*service){
                let mut live=self.ae2_live_events.borrow_mut();
                if live.energy.watcher_tick(*service).is_ok(){if let Some(s)=live.energy.lifecycle.services.get_mut(service){s.finish_end_tick();}}
            }
        }
        if !bad.is_empty(){
            let mut legacy=network.clone();
            for power in &bad {
                let Some(group)=analysis.power_components.get(*power) else{continue};
                if group.positions.iter().any(|pos|legacy.nodes[pos].kind==ae2_network_core::DeviceKind::CreativeEnergyCell){continue}
                let mut remaining=(group.idle_per_tick*multiplier).min(f64::MAX);
                for pos in &group.positions {
                    if let Some(device)=legacy.nodes.get_mut(pos){let used=remaining.min(device.energy);device.energy-=used;remaining-=used;if remaining<=0.0{break}}
                }
            }
            self.ae2_commit_power(&legacy);
        }
        // Service IDs originate in the ordered membership observer. Exact
        // registration order across independent source grids remains bounded.
        plans.sort_by_key(|plan|plan.1);
        for (power,service,pos,base,amount) in plans {
            if bad.contains(&power){continue}
            // Source injects the selected generator once, at its owning
            // service, before that service's watcher and idle consumption.
            if generators.contains_key(&service) {
                let rate=horizon_client::ae2_common_config::passive_power().resonance_rate;
                if let Err(error)=self.ae2_energy_transaction_snapshot(&network,&analysis,pos,rate,false,true,false){log::warn!("AE2 passive generation stopped: {error}");return true}
            }
            if let Err(error)=self.ae2_live_events.borrow_mut().energy.watcher_tick(service){log::warn!("AE2 energy watcher tick stopped: {error}");return true}
            let charged=(amount*multiplier).min(f64::MAX);
            let extracted=match self.ae2_energy_transaction_snapshot(&network,&analysis,pos,charged,base<=0.0001,false,false){Ok(value)=>value/multiplier,Err(error)=>{log::warn!("AE2 energy idle tick stopped: {error}");return true}};
            let powered=if base>0.0001{extracted>=base-0.001}else{extracted>0.0};
            if let Err(error)=self.ae2_live_events.borrow_mut().energy.finish_service_tick(service,powered){log::warn!("AE2 energy power tick stopped: {error}");return true}
        }
        true
    }
}
