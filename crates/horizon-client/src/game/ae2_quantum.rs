//! Physical quantum bridges and explosion pairing. Rules derived from AE2
//! 19.2.18 QuantumCalculator, QuantumCluster and TransformLogic (LGPL-3.0+).
use super::{Dim,Game};
use horizon_core::random::RandomSource;
use ae2_links_core::{Pos, QuantumBlock, QuantumFrame};
use ae2_network_core::{Network, NodeIdentity, Pos as NetworkPos, quantum_registry::QuantumRegistry};
use horizon_client::{ae2_world::MachineKind, containers::BlockEntity, items::ItemStack};
use horizon_world::nbt::Tag;
use std::collections::{BTreeMap,BTreeSet,HashSet};

type FrameNodes = BTreeMap<NetworkPos, (NodeIdentity, u8)>;
type PhysicalNodes = BTreeMap<NetworkPos, FrameNodes>;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum QuantumEvent {
    Faces{pos:NetworkPos,id:NodeIdentity,faces:u8},
    Link{a:NodeIdentity,b:NodeIdentity,added:bool},
}

/// Runtime registrations are usable only while every physical frame and
/// frequency still matches the observed callback sequence. If a transition
/// was not observed, no coordinate-based pairing is manufactured.
#[derive(Clone, Default)]
pub(super) struct QuantumRuntime {
    frames: BTreeMap<NetworkPos,(NodeIdentity,i64)>,
    members: BTreeMap<NetworkPos,BTreeSet<NetworkPos>>,
    registry: QuantumRegistry,
    active: bool,
    ready_ticked: BTreeSet<NodeIdentity>,
    ready_order_unknown: bool,
    // Formation is transient, belongs to the exact nine BE incarnations,
    // and is established only by a real rescan or neighbor callback.
    formed: PhysicalNodes,
    faces: BTreeMap<NodeIdentity, u8>,
    visual: BTreeMap<NodeIdentity, bool>,
    // Unlike the old all-ready fallback, this registry follows each observed
    // source formation/status callback, including centers not yet onReady.
    source_registry: QuantumRegistry,
    source_frames: BTreeMap<NetworkPos,(NodeIdentity,i64)>,
    source_invalid: bool,
    ready_nodes: BTreeSet<NodeIdentity>,
    pending: Vec<QuantumEvent>,
}
impl QuantumRuntime {
    fn invalidate(&mut self, reason:&str) {
        if !self.source_invalid && std::env::var_os("HORIZON_AE2_QUANTUM_TRACE").is_some() {
            log::info!("AE2 quantum source state invalidated: {reason}");
        }
        self.source_invalid=true;
    }
    fn links(&mut self,events:Vec<ae2_network_core::quantum_registry::LinkEvent>) {
        self.pending.extend(events.into_iter().map(|event|QuantumEvent::Link{a:event.a,b:event.b,added:event.added}));
    }
    pub(super) fn node_ready(&mut self,id:NodeIdentity) {
        self.ready_nodes.insert(id);
        if !self.source_invalid && self.source_registry.sides(id).is_some()
            && self.source_registry.set_node_live(id,true).is_err() {self.invalidate("node_ready set_node_live");}
    }
    fn source_status(&mut self,center:NetworkPos,id:NodeIdentity,frequency:i64) {
        if self.source_invalid{return}
        match self.source_registry.update(id,frequency) {
            Ok(events)=>self.links(events),
            Err(_)=>{self.invalidate("source_status update");return}
        }
        self.source_frames.insert(center,(id,frequency));
    }
    fn forget_registration(&mut self,center:NetworkPos) {
        if let Some((id,_))=self.source_frames.remove(&center) {
            if !self.source_invalid {
                match self.source_registry.destroy(id) {Ok(events)=>self.links(events),Err(_)=>self.invalidate("forget_registration destroy")}
            }
        }
    }
    fn form_at(&mut self, pos:NetworkPos, nodes:&PhysicalNodes,physical:&BTreeMap<NetworkPos,(NodeIdentity,i64)>) {
        let Some((&center, frame))=nodes.iter().find(|(_,frame)|frame.contains_key(&pos)) else{return};
        let Some(&(id,frequency))=physical.get(&center) else{self.invalidate("form_at center unavailable");return};
        let newly_formed=self.formed.get(&center)!=Some(frame);
        if newly_formed {
            if self.formed.contains_key(&center) {self.destroy_formation(center);}
            if !self.source_invalid && self.source_registry.form_with_node(id,self.ready_nodes.contains(&id)).is_err() {self.invalidate("form_at duplicate");}
        }
        for (id,faces) in frame.values() {self.visual.insert(*id,true);self.faces.insert(*id,*faces);}
        if newly_formed {
            let mut ordered:Vec<_>=frame.iter().collect();
            ordered.sort_by_key(|(pos,_)|(pos.z,pos.y,pos.x));
            for (&pos,&(id,faces)) in ordered {self.pending.push(QuantumEvent::Faces{pos,id,faces});}
        }
        self.formed.insert(center,frame.clone());
        self.source_status(center,id,frequency);
    }
    fn destroy_formation(&mut self, center:NetworkPos) {
        if let Some(frame)=self.formed.remove(&center) {
            self.forget_registration(center);
            if let Some(&(id,_))=frame.get(&center) {self.pending.push(QuantumEvent::Faces{pos:center,id,faces:0});}
            let mut ring:Vec<_>=frame.iter().filter(|(pos,_)|**pos!=center).collect();
            ring.sort_by_key(|(pos,_)|(pos.z,pos.y,pos.x));
            for (&pos,&(id,_)) in ring {self.pending.push(QuantumEvent::Faces{pos,id,faces:0});}
            for (id,_) in frame.values() {self.visual.insert(*id,false);self.faces.remove(id);}
        }
    }
    fn neighbor_formation(&mut self, target:NetworkPos, nodes:&PhysicalNodes,physical:&BTreeMap<NetworkPos,(NodeIdentity,i64)>) {
        let stale:Vec<_>=self.formed.iter().filter_map(|(center,frame)|
            (frame.contains_key(&target) && nodes.get(center)!=Some(frame)).then_some(*center)).collect();
        for center in stale {self.destroy_formation(center);}
        self.form_at(target,nodes,physical);
    }
    /// A BE removal destroys its cluster even when neighboring block updates
    /// are suppressed. A later replacement needs its own actual callback.
    pub(super) fn removed(&mut self, pos:NetworkPos, incarnation:u64) {
        let id=NodeIdentity{host:incarnation,part:None,outer:false};
        let centers:Vec<_>=self.formed.iter().filter_map(|(center,frame)|
            frame.get(&pos).is_some_and(|(node,_)|*node==id).then_some(*center)).collect();
        for center in centers {
            self.destroy_formation(center);
            if self.active {
                if let Some((node,_))=self.frames.remove(&center) {
                    if self.registry.destroy(node).is_err(){self.active=false;}
                } else {self.active=false;}
                self.members.remove(&center);
            }
        }
        self.visual.remove(&id);
        self.faces.remove(&id);
        self.ready_nodes.remove(&id);
        self.ready_ticked.remove(&id);
    }
    /// Replay the observed onChunkUnloaded callback for this exact BE.
    /// disconnect(false) destroys the cluster and peer connection, but
    /// updateStatus(..., false) deliberately does not change member faces.
    pub(super) fn unloaded(&mut self, pos:NetworkPos, incarnation:u64) -> Vec<ae2_network_core::quantum_registry::LinkEvent> {
        let id=NodeIdentity{host:incarnation,part:None,outer:false};
        let centers:Vec<_>=self.formed.iter().filter_map(|(center,frame)|
            frame.get(&pos).is_some_and(|(node,_)|*node==id).then_some(*center)).collect();
        let mut links=Vec::new();
        for center in centers {
            self.formed.remove(&center);
            if let Some((node,_))=self.source_frames.remove(&center) {
                if !self.source_invalid {
                    match self.source_registry.destroy(node) {
                        Ok(events)=>links.extend(events),
                        Err(_)=>self.invalidate("unload destroy"),
                    }
                }
            } else if !self.source_invalid {self.invalidate("unload registration missing");}
            if self.active {
                if let Some((node,_))=self.frames.remove(&center) {
                    if self.registry.destroy(node).is_err(){self.active=false;}
                } else {self.active=false;}
                self.members.remove(&center);
            }
        }
        self.visual.remove(&id);
        self.faces.remove(&id);
        self.ready_nodes.remove(&id);
        self.ready_ticked.remove(&id);
        links
    }
    /// A chunk unload is not a world-affecting destroy. Drop its cluster and
    /// registrations without inventing an ordinary neighbor callback.
    fn forget_unloaded(&mut self,nodes:&PhysicalNodes,pending:&BTreeSet<NetworkPos>) {
        let stale:Vec<_>=self.formed.iter().filter_map(|(center,frame)|
            (nodes.get(center)!=Some(frame) && !pending.contains(center)).then_some(*center)).collect();
        // World streaming batches BE removal without source-observed cluster
        // callback order. Do not manufacture an ordered teardown sequence.
        if !stale.is_empty(){self.invalidate(&format!("unobserved physical removal: {stale:?}"));}
        for center in stale {self.formed.remove(&center);self.source_frames.remove(&center);}
    }
    pub(super) fn flush_events(&mut self,world:&horizon_client::world::World,dim:Dim,anchor:(i32,i32,i32)) {
        for event in std::mem::take(&mut self.pending) {
            match event {
                QuantumEvent::Faces{pos,id,faces}=>{
                    let Some((owner,local))=super::ae2_global::GridAddress::from_network(pos).and_then(|a|a.local_parts()) else{self.invalidate("flush invalid position");continue};
                    if owner!=dim || world.block_entities.incarnation(&local)!=Some(id.host) {self.invalidate(&format!("flush face identity mismatch {local:?} {id:?}"));continue}
                    world.block_entities.record_ae2_quantum_faces(local,id.host,faces);
                }
                QuantumEvent::Link{a,b,added}=>world.block_entities.record_ae2_quantum_link(anchor,a,b,added),
            }
        }
    }
    fn ready_rescan(&mut self,id:NodeIdentity,pos:NetworkPos,physical:BTreeMap<NetworkPos,(NodeIdentity,i64)>,members:BTreeMap<NetworkPos,BTreeSet<NetworkPos>>,nodes:&PhysicalNodes,all_ready:bool) {
        if !self.ready_ticked.insert(id){return}
        self.node_ready(id);
        self.visual.entry(id).or_insert(false);
        self.neighbor_formation(pos,nodes,&physical);
        let Some(center)=members.iter().find_map(|(center,part)|part.contains(&pos).then_some(*center)) else{return};
        if !all_ready {self.active=false;self.ready_order_unknown=true;return}
        if self.ready_order_unknown{return}
        if !self.active {
            let mut registry=QuantumRegistry::default();
            for (node,_) in physical.values(){if registry.form(*node).is_err(){self.ready_order_unknown=true;return}}
            self.registry=registry;self.frames=physical.clone();self.members=members;self.active=true;
        } else if self.frames!=physical {self.active=false;self.ready_order_unknown=true;return}
        let (center_id,value)=physical[&center];
        if self.registry.update(center_id,value).is_err(){self.active=false;self.ready_order_unknown=true;}
    }
    fn observe(&mut self,physical:BTreeMap<NetworkPos,(NodeIdentity,i64)>,target:NetworkPos) {
        let Some(&(id,value))=physical.get(&target) else{self.active=false;return};
        if !self.active {
            if physical.iter().any(|(p,(_,f))|*p!=target && *f!=0){return}
            let mut registry=QuantumRegistry::default();
            for (node,_) in physical.values(){if registry.form(*node).is_err(){return}}
            self.registry=registry;
        } else if physical.iter().any(|(p,value)| *p!=target && self.frames.get(p)!=Some(value))
            || self.frames.keys().any(|p|!physical.contains_key(p))
            || self.frames.get(&target).is_some_and(|(old,_)|*old!=id) {
            self.active=false;return
        }
        if self.registry.update(id,value).is_err(){self.active=false;return}
        self.frames=physical;self.active=true;
    }
    /// A real neighbor callback can destroy one previously observed cluster.
    /// Unknown source/order or a more complex change invalidates this bounded
    /// runtime so the caller does not manufacture source connections.
    pub(super) fn neighbor_changed(&mut self,world:&horizon_client::world::World,dim:Dim,facts:&horizon_client::blocks::BlockFacts,pos:(i32,i32,i32),source:Option<(i32,i32,i32)>) {
        let Some(target)=super::ae2_global::GridAddress::from_local(dim,pos).map(|a|a.network_pos()) else{self.active=false;return};
        let (local,_,nodes) = physical_frames(world,dim,facts);
        let existing=self.formed.values().find(|frame|frame.contains_key(&target));
        let changed=source.and_then(|p|super::ae2_global::GridAddress::from_local(dim,p).map(|a|a.network_pos()));
        let recheck=if let Some(frame)=existing {
            if let (Some(changed),Some(source))=(changed,source) {
                frame.contains_key(&changed) || matches!(world.block_entities.get(&source),Some(BlockEntity::Ae2(m)) if matches!(m.kind,MachineKind::QuantumRing|MachineKind::QuantumLink))
            } else {self.invalidate("neighbor source unavailable");false}
        } else {true};
        if recheck {self.neighbor_formation(target,&nodes,&local);}
        if !self.active{return}
        let Some(source)=source else{self.active=false;return};
        let Some(changed)=super::ae2_global::GridAddress::from_local(dim,source).map(|a|a.network_pos()) else{self.active=false;return};
        let Some(base)=super::ae2_global::GridAddress::from_local(dim,(0,0,0)).map(|a|a.network_pos().y) else{self.active=false;return};
        self.apply_neighbor(local,base,target,changed);
    }
    fn apply_neighbor(&mut self,local:BTreeMap<NetworkPos,(NodeIdentity,i64)>,base:i32,target:NetworkPos,changed:NetworkPos) {
        let old_local:BTreeMap<_,_>=self.frames.iter().filter(|(p,_)|p.y>=base-64 && p.y<=base+319).map(|(p,v)|(*p,*v)).collect();
        if local==old_local{return}
        let removed:Vec<_>=old_local.keys().filter(|p|!local.contains_key(p)).copied().collect();
        let added=local.keys().any(|p|!old_local.contains_key(p));
        if removed.len()!=1 || added || local.iter().any(|(p,v)|old_local.get(p)!=Some(v)) {self.active=false;return}
        let center=removed[0];
        let Some(members)=self.members.get(&center) else{self.active=false;return};
        if !members.contains(&target) || !members.contains(&changed) {self.active=false;return}
        let id=old_local[&center].0;
        if self.registry.destroy(id).is_err(){self.active=false;return}
        self.frames.remove(&center);self.members.remove(&center);
    }
    fn added(&mut self,physical:BTreeMap<NetworkPos,(NodeIdentity,i64)>,members:BTreeMap<NetworkPos,BTreeSet<NetworkPos>>,nodes:&PhysicalNodes,changed:NetworkPos) {
        // Horizon installs a just-placed BE after its real neighbor callbacks;
        // replay their formation result after installation, only for a frame
        // touched by that actual ordinary placement.
        let stale:Vec<_>=self.formed.iter().filter_map(|(center,frame)|
            (nodes.get(center)!=Some(frame) && frame.keys().any(|p|adjacent(*p,changed))).then_some(*center)).collect();
        for center in stale {self.destroy_formation(center);}
        if let Some((_,frame))=nodes.iter().find(|(_,frame)|frame.contains_key(&changed) && frame.keys().any(|p|adjacent(*p,changed))) {
            if let Some(target)=frame.keys().find(|p|adjacent(**p,changed)) {self.form_at(*target,nodes,&physical);}
        }
        if !self.active || self.frames==physical{return}
        let removed:Vec<_>=self.frames.keys().filter(|p|!physical.contains_key(p)).copied().collect();
        let added:Vec<_>=physical.keys().filter(|p|!self.frames.contains_key(p)).copied().collect();
        if physical.iter().any(|(p,v)|self.frames.get(p).is_some_and(|old|old!=v)){self.active=false;return}
        if removed.is_empty() && added.len()==1 && members.get(&added[0]).is_some_and(|m|m.contains(&changed)) {
            let (id,frequency)=physical[&added[0]];
            // A preconfigured center needs the source deferred-ready sequence;
            // do not invent an early node/connection during placement.
            if frequency!=0 {self.active=false;return}
            if self.registry.form(id).is_err() || self.registry.update(id,frequency).is_err(){self.active=false;return}
        } else if added.is_empty() && removed.len()==1 && self.members.get(&removed[0]).is_some_and(|m|m.iter().any(|p| (p.x-changed.x).abs()+(p.y-changed.y).abs()+(p.z-changed.z).abs()==1)) {
            if self.registry.destroy(self.frames[&removed[0]].0).is_err(){self.active=false;return}
        } else {self.active=false;return}
        self.frames=physical;self.members=members;
    }
}
/// Streaming drops a chunk's block states before it serializes the still
/// resident BEs and only then emits their bulk-unload journal. This frame is
/// pending that observed callback only if every original identity is still
/// present and at least one of its physical chunks has already been removed.
fn pending_chunk_unload(frame:&FrameNodes,mut state:impl FnMut(NetworkPos)->Option<(u64,bool)>)->bool {
    let mut missing_chunk=false;
    for (&pos,&(id,_)) in frame {
        let Some((incarnation,loaded))=state(pos) else{return false};
        if incarnation!=id.host{return false}
        missing_chunk|=!loaded;
    }
    missing_chunk
}
fn adjacent(a:NetworkPos,b:NetworkPos)->bool {
    i64::from(a.x).abs_diff(i64::from(b.x))+i64::from(a.y).abs_diff(i64::from(b.y))+i64::from(a.z).abs_diff(i64::from(b.z))==1
}
fn physical_frames(world:&horizon_client::world::World,dim:Dim,facts:&horizon_client::blocks::BlockFacts)->(BTreeMap<NetworkPos,(NodeIdentity,i64)>,BTreeMap<NetworkPos,BTreeSet<NetworkPos>>,PhysicalNodes) {
    let blocks=world.block_entities.iter().filter_map(|(&p,be)|{
        let BlockEntity::Ae2(m)=be else{return None};
        let (block,name)=match m.kind {MachineKind::QuantumRing=>(QuantumBlock::Ring,"ae2:quantum_ring"),MachineKind::QuantumLink=>(QuantumBlock::Link,"ae2:quantum_link"),_=>return None};
        (facts.get(world.block(p.0,p.1,p.2)).name.as_ref()==name).then_some((Pos(p.0,p.1,p.2),block))
    }).collect::<BTreeMap<_,_>>();
    let mut physical=BTreeMap::new();let mut members=BTreeMap::new();let mut all_nodes=BTreeMap::new();
    for frame in QuantumFrame::scan(&blocks) {
        let p=(frame.center.0,frame.center.1,frame.center.2);
        let (Some(address),Some(incarnation),Some(BlockEntity::Ae2(m)))=(super::ae2_global::GridAddress::from_local(dim,p),world.block_entities.incarnation(&p),world.block_entities.get(&p)) else{continue};
        let center=address.network_pos();let id=NodeIdentity{host:incarnation,part:None,outer:false};
        let value=m.slots.first().and_then(Option::as_ref).and_then(frequency).unwrap_or(0);
        let Some(nodes)=frame.members.iter().map(|p| {
            let local=(p.0,p.1,p.2);
            let pos=super::ae2_global::GridAddress::from_local(dim,local)?.network_pos();
            let host=world.block_entities.incarnation(&local)?;
            Some((pos,(NodeIdentity{host,part:None,outer:false},frame.faces(*p))))
        }).collect::<Option<FrameNodes>>() else{continue};
        physical.insert(center,(id,value));members.insert(center,nodes.keys().copied().collect());all_nodes.insert(center,nodes);
    }
    (physical,members,all_nodes)
}

pub(super) fn frequency(stack: &ItemStack) -> Option<i64> {
    let data = stack.data()?;
    // Older native saves stored this inside custom data. Prefer the real AE2
    // component when present, including zero: the source slot validator tests
    // component presence while a zero frequency simply has no connection.
    let value = data.other("ae2:entangled_singularity_id").or_else(|| {
        data.other("minecraft:custom_data").and_then(|tag| tag.get("ae2:entangled_singularity_id"))
    })?;
    // Codec.LONG reads NbtOps' numeric values through Number.longValue().
    match value {
        Tag::Byte(n) => Some(i64::from(*n)),
        Tag::Short(n) => Some(i64::from(*n)),
        Tag::Int(n) => Some(i64::from(*n)),
        Tag::Long(n) => Some(*n),
        Tag::Float(n) => Some(f32::from_bits(*n) as i64),
        Tag::Double(n) => Some(f64::from_bits(*n) as i64),
        _ => None,
    }
}
fn network_pos(p: Pos) -> NetworkPos {
    NetworkPos {
        x: p.0,
        y: p.1,
        z: p.2,
    }
}
impl Game<'_> {
    /// One source QuantumBridge serverTick after that physical node's
    /// onReady. Called in the engine's actual block-entity ticker order.
    pub(super) fn ae2_quantum_ready_tick(&self,local:(i32,i32,i32)) {
        let Some(incarnation)=self.world.block_entities.incarnation(&local) else{return};
        let id=NodeIdentity{host:incarnation,part:None,outer:false};
        if !self.ae2_event_node_is_ready(id){return}
        if self.ae2_quantum_runtime.borrow().ready_ticked.contains(&id){return}
        let Some(pos)=self.ae2_local_address(local).map(|a|a.network_pos()) else{return};
        let (physical,members,nodes)=self.ae2_quantum_physical_frames();
        // Interleaved readiness invalidates the older all-ready registry.
        // The source registry still follows each real callback individually.
        let all_ready=members.values().flatten().all(|pos|{
            let Some(address)=super::ae2_global::GridAddress::from_network(*pos) else{return false};
            let Some((dim,p))=self.ae2_address_parts(address) else{return false};
            let Some(host)=self.ae2_world_for(dim).and_then(|w|w.block_entities.incarnation(&p)) else{return false};
            self.ae2_event_node_is_ready(NodeIdentity{host,part:None,outer:false})
        });
        let center=members.iter().find_map(|(center,part)|part.contains(&pos).then_some(*center));
        let center_id=center.and_then(|p|physical.get(&p).map(|entry|entry.0));
        let mut runtime=self.ae2_quantum_runtime.borrow_mut();
        runtime.ready_rescan(id,pos,physical,members,&nodes,all_ready);
        runtime.flush_events(&self.world,self.dim,local);
        if std::env::var_os("HORIZON_AE2_QUANTUM_TRACE").is_some() {
            log::info!("AE2 quantum ready ticker dim={:?} pos={local:?} node={id:?} center={center:?} sides={:?} peer={:?} all_ready={all_ready} active={} unknown={} source_invalid={}",self.dim,center_id.and_then(|id|runtime.registry.sides(id)),center_id.and_then(|id|runtime.registry.peer(id)),runtime.active,runtime.ready_order_unknown,runtime.source_invalid);
        }
    }
    fn ae2_quantum_physical_frames(&self)->(BTreeMap<NetworkPos,(NodeIdentity,i64)>,BTreeMap<NetworkPos,BTreeSet<NetworkPos>>,PhysicalNodes) {
        let mut physical=BTreeMap::new();let mut members=BTreeMap::new();let mut nodes=BTreeMap::new();
        for (dim,world) in self.ae2_loaded_worlds() {
            let (found,parts,identities)=physical_frames(world,dim,self.res.facts());physical.extend(found);members.extend(parts);nodes.extend(identities);
        }
        (physical,members,nodes)
    }
    /// A placed quantum block's entity is now physically installed. The
    /// runtime only accepts a single unambiguous source-shaped frame change.
    pub(super) fn ae2_quantum_block_added(&self,local:(i32,i32,i32)) {
        let Some(changed)=self.ae2_local_address(local).map(|a|a.network_pos()) else{return};
        let (physical,members,nodes)=self.ae2_quantum_physical_frames();
        let mut runtime=self.ae2_quantum_runtime.borrow_mut();
        runtime.added(physical,members,&nodes,changed);
        runtime.flush_events(&self.world,self.dim,local);
    }
    /// Called immediately after a real link-center inventory commit. This
    /// handles the source registration ordering only when no earlier nonempty
    /// center was missed and no other frame changed since the last callback.
    pub(super) fn ae2_quantum_inventory_changed(&self, local:(i32,i32,i32)) {
        let Some(target)=self.ae2_local_address(local).map(|a|a.network_pos()) else{return};
        let (physical,members,_)=self.ae2_quantum_physical_frames();
        let mut runtime=self.ae2_quantum_runtime.borrow_mut();
        // Source inventory changes do nothing to the registry before a
        // physical cluster exists; its later rescan reads the live slot.
        if !runtime.formed.contains_key(&target){return}
        if let Some(&(id,value))=physical.get(&target) {
            if runtime.source_frames.get(&target).is_some_and(|(old,_)|*old==id) {runtime.source_status(target,id,value);}
            else {runtime.invalidate("inventory registration identity mismatch");}
        } else {runtime.invalidate("inventory center missing");}
        runtime.observe(physical,target);if runtime.active {runtime.members=members;}
        runtime.flush_events(&self.world,self.dim,local);
    }
    pub(super) fn ae2_quantum_removed(&self,local:(i32,i32,i32)) {
        let (Some(pos),Some(host))=(self.ae2_local_address(local).map(|a|a.network_pos()),self.world.block_entities.incarnation(&local)) else{return};
        let mut runtime=self.ae2_quantum_runtime.borrow_mut();
        runtime.removed(pos,host);
        runtime.flush_events(&self.world,self.dim,local);
    }
    pub(super) fn ae2_quantum_visual_formed(&self,local:(i32,i32,i32))->Option<bool> {
        let id=NodeIdentity{host:self.world.block_entities.incarnation(&local)?,part:None,outer:false};
        self.ae2_quantum_runtime.borrow().visual.get(&id).copied()
    }
    pub(super) fn ae2_connect_quantum(&self, network: &mut Network) {
        // Process ordered unloads before comparing the registry to the live
        // physical scan; otherwise a normal chunk unload looks unobserved.
        let _ = self.ae2_sync_events();
        let (physical,_,nodes)=self.ae2_quantum_physical_frames();
        let mut runtime=self.ae2_quantum_runtime.borrow_mut();
        let pending:BTreeSet<_>=runtime.formed.iter().filter_map(|(center,frame)|{
            (nodes.get(center)!=Some(frame) && pending_chunk_unload(frame,|pos|{
                let (dim,local)=super::ae2_global::GridAddress::from_network(pos)?.local_parts()?;
                let world=self.ae2_world_for(dim)?;
                Some((world.block_entities.incarnation(&local)?,world.chunks.contains_key(&(local.0>>4,local.2>>4))))
            })).then_some(*center)
        }).collect();
        runtime.forget_unloaded(&nodes,&pending);
        if let Some((center,recorded))=runtime.source_frames.iter().find(|(center,recorded)|!pending.contains(center) && physical.get(center)!=Some(recorded)) {
            let message=format!("physical source mismatch {center:?}: expected {recorded:?}, actual {:?}",physical.get(center));
            runtime.invalidate(&message);
        }
        for (dim, world) in self.ae2_loaded_worlds() {
            let blocks = world
                .block_entities
                .iter()
                .filter_map(|(&p, be)| {
                    let BlockEntity::Ae2(m) = be else {
                        return None;
                    };
                    let block = match m.kind {
                        MachineKind::QuantumRing => QuantumBlock::Ring,
                        MachineKind::QuantumLink => QuantumBlock::Link,
                        _ => return None,
                    };
                    let address = self.ae2_dimension_address(dim, p)?;
                    network
                        .nodes
                        .contains_key(&address.network_pos())
                        .then_some((Pos(p.0, p.1, p.2), block))
                })
                .collect::<BTreeMap<_, _>>();
            for p in blocks.keys() {
                if let Some(address) = self.ae2_dimension_address(dim, (p.0, p.1, p.2)) {
                    let position=address.network_pos();
                    let identity=world.block_entities.incarnation(&(p.0,p.1,p.2)).map(|host|NodeIdentity{host,part:None,outer:false});
                    if !identity.is_some_and(|id|runtime.ready_nodes.contains(&id)) {
                        // A loaded source BE has no GridNode until onReady.
                        // Keep it in the physical frame scan, but not in the
                        // live network or ordered-service comparison yet.
                        network.nodes.remove(&position);
                        network.node_identities.remove(&position);
                        continue;
                    }
                    if let Some(d) = network.nodes.get_mut(&position) {
                        d.faces = identity.and_then(|id|runtime.faces.get(&id).copied()).unwrap_or(0);
                        d.idle_override = Some(22.0);
                        d.capacity_override = Some(32);
                    }
                }
            }
        }
        if !runtime.source_invalid {
            for (a,b) in runtime.source_registry.links() {
                let a=physical.iter().find_map(|(p,(id,_))|(*id==a && runtime.formed.contains_key(p)).then_some(*p));
                let b=physical.iter().find_map(|(p,(id,_))|(*id==b && runtime.formed.contains_key(p)).then_some(*p));
                if let (Some(a),Some(b))=(a,b){let _=network.connect(a,b);}
            }
            return;
        }
        if runtime.active && runtime.frames!=physical {runtime.active=false;}
        if runtime.active {
            for (a,b) in runtime.registry.links() {
                let a=physical.iter().find_map(|(p,(id,_))|(*id==a && runtime.formed.contains_key(p)).then_some(*p));
                let b=physical.iter().find_map(|(p,(id,_))|(*id==b && runtime.formed.contains_key(p)).then_some(*p));
                if let (Some(a),Some(b))=(a,b){let _=network.connect(a,b);}
            }
            return;
        }
        // Unknown callback state cannot establish a source connection.
        // In particular, sorted physical endpoints invent the wrong pair
        // when three clusters share a frequency.
    }

    /// Called for the original explosion entity snapshot before vanilla item
    /// damage. A successful recipe intercepts the anchor's damage and creates
    /// a paired stack, preserving unconsumed inputs and real component data.
    pub(super) fn ae2_explosion_singularities(
        &mut self,
        center: [f64; 3],
        radius: f32,
    ) -> HashSet<u32> {
        let diameter = f64::from(radius) * 2.0;
        let candidates = self
            .dropped
            .iter()
            .filter(|e| {
                e.stack.count > 0
                    && (e.position.x - center[0]).powi(2)
                        + (e.position.y - center[1]).powi(2)
                        + (e.position.z - center[2]).powi(2)
                        <= diameter * diameter
            })
            .map(|e| e.id)
            .collect::<Vec<_>>();
        self.ae2_explosion_transform_items(&candidates)
    }
    /// ItemEntityMixin intercepts each original entity's explosion damage.
    /// Callers may supply a native blast's exact affected item IDs.
    pub(super) fn ae2_explosion_transform_items(&mut self, candidates: &[u32]) -> HashSet<u32> {
        let rules = horizon_client::ae2_transform::rules(self.res.recipes(), self.res.items());
        let mut protected = HashSet::new();
        for id in candidates {
            let Some(index) = self
                .dropped
                .iter()
                .position(|e| e.id == *id && e.stack.count > 0)
            else {
                continue;
            };
            let selected = rules.iter().filter(|r| r.explosion()).find_map(|rule| {
                self.res.items().get(&rule.output.item)?;
                let claims = rule.select(index, &self.dropped)?;
                let output = rule.assemble().ok()?;
                Some((output, claims))
            });
            let Some((output, claims)) = selected else {
                continue;
            };
            let p = self.dropped[index].position;
            let at = [
                p.x.floor() + 0.25 + self.world.random.next_double() * 0.5,
                p.y.floor() + 0.25 + self.world.random.next_double() * 0.5,
                p.z.floor() + 0.25 + self.world.random.next_double() * 0.5,
            ];
            let velocity = [
                self.world.random.next_double() * 0.25 - 0.125,
                self.world.random.next_double() * 0.25 - 0.125,
                self.world.random.next_double() * 0.25 - 0.125,
            ];
            for (index, count) in claims {
                self.dropped[index].stack.count -= count;
            }
            self.spawn_item(output, at, velocity, 0);
            protected.insert(*id);
            protected.insert(self.next_entity);
        }
        self.dropped.retain(|e| e.stack.count > 0);
        protected
    }
}
#[cfg(test)]mod runtime_tests {
 use super::*;
 fn p(x:i32)->NetworkPos{NetworkPos{x,y:0,z:0}}
 fn id(x:u64)->NodeIdentity{NodeIdentity{host:x,part:None,outer:false}}
 #[test]fn source_singularity_component_and_legacy_data_select_frequency(){
  let mut stack=ItemStack::new("minecraft:diamond",1);
  assert_eq!(frequency(&stack),None);
  stack.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("ae2:entangled_singularity_id",Tag::Long(17))])));
  assert_eq!(frequency(&stack),Some(17),"earlier native saves remain readable");
  stack.data_mut().set_other("ae2:entangled_singularity_id",Some(Tag::Long(0)));
  assert_eq!(frequency(&stack),Some(0),"a present zero component is insertable but does not connect");
  stack.data_mut().set_other("ae2:entangled_singularity_id",Some(Tag::Long(-42)));
  assert_eq!(frequency(&stack),Some(-42),"the actual AE2 component takes precedence");
  stack.data_mut().set_other("ae2:entangled_singularity_id",Some(Tag::Int(42)));
  assert_eq!(frequency(&stack),Some(42),"Codec.LONG accepts an unsuffixed integer argument");
  stack.data_mut().set_other("ae2:entangled_singularity_id",Some(Tag::Double(42.75f64.to_bits())));
  assert_eq!(frequency(&stack),Some(42),"Number.longValue truncates a floating value");
  stack.data_mut().set_other("ae2:entangled_singularity_id",Some(Tag::String("invalid".into())));
  assert_eq!(frequency(&stack),None,"an invalid source component must not select stale legacy data");
 }
 #[test]fn source_component_registration_suggests_and_rejects_invalid_item_arguments(){
  use horizon_client::command::{Registries,Reader,SuggestionsBuilder,item::{parse_stack,suggest_stack}};
  let mut reg=Registries::default();reg.items.push("ae2:quantum_entangled_singularity".into());
  horizon_client::ae2_components::register(&mut reg);horizon_client::ae2_components::register(&mut reg);
  assert_eq!(reg.persistent_components.len(),35,"registration is idempotent");
  assert_eq!(reg.ids("minecraft:data_component_type").len(),35);
  let prefix="ae2:quantum_entangled_singularity[ae2:entangled";
  let mut suggestions=SuggestionsBuilder::new(prefix,0);suggest_stack(&mut suggestions,&reg);
  assert!(suggestions.out.iter().any(|s|s.text=="ae2:entangled_singularity_id="));
  let mut valid=Reader::new("ae2:quantum_entangled_singularity[ae2:entangled_singularity_id=42]");
  assert!(parse_stack(&mut valid,&reg).is_ok());
  let mut malformed=Reader::new("ae2:quantum_entangled_singularity[ae2:entangled_singularity_id=\"bad\"]");
  assert!(parse_stack(&mut malformed,&reg).is_err());assert_eq!(malformed.cursor,0);
  let mut unknown=Reader::new("ae2:quantum_entangled_singularity[ae2:not_registered=1]");
  assert!(parse_stack(&mut unknown,&reg).is_err());assert_eq!(unknown.cursor,0);
 }
 #[test]fn callback_order_preserves_first_pair_and_unobserved_changes_fail_closed(){
  let mut runtime=QuantumRuntime::default();let mut frames=BTreeMap::from([(p(0),(id(1),0)),(p(10),(id(2),0)),(p(20),(id(3),42))]);
  runtime.observe(frames.clone(),p(20));assert!(runtime.active);
  frames.get_mut(&p(0)).unwrap().1=42;runtime.observe(frames.clone(),p(0));assert_eq!(runtime.registry.peer(id(1)),Some(id(3)));
  frames.get_mut(&p(10)).unwrap().1=42;runtime.observe(frames.clone(),p(10));assert_eq!(runtime.registry.peer(id(1)),Some(id(3)));assert_eq!(runtime.registry.peer(id(2)),None);
  frames.get_mut(&p(20)).unwrap().1=0;runtime.observe(frames.clone(),p(20));assert_eq!(runtime.registry.peer(id(1)),None);assert_eq!(runtime.registry.peer(id(2)),None);
  frames.get_mut(&p(10)).unwrap().1=0;runtime.observe(frames.clone(),p(0));assert!(!runtime.active,"unobserved other slot edit must not be treated as ordered");
 }
 #[test]fn known_ring_removal_destroys_only_its_cluster_without_promoting_third(){
  let mut runtime=QuantumRuntime::default();let mut frames=BTreeMap::from([(p(0),(id(1),0)),(p(10),(id(2),0)),(p(20),(id(3),42))]);
  runtime.observe(frames.clone(),p(20));frames.get_mut(&p(0)).unwrap().1=42;runtime.observe(frames.clone(),p(0));frames.get_mut(&p(10)).unwrap().1=42;runtime.observe(frames.clone(),p(10));
  runtime.members.insert(p(20),BTreeSet::from([p(19),p(20),p(21)]));frames.remove(&p(20));
  runtime.apply_neighbor(frames,0,p(21),p(19));
  assert!(runtime.active);assert_eq!(runtime.registry.peer(id(1)),None);assert_eq!(runtime.registry.peer(id(2)),None);assert_eq!(runtime.registry.sides(id(3)),None);
 }
 #[test]fn newly_formed_frame_keeps_the_already_registered_pair(){
  let mut runtime=QuantumRuntime::default();let mut frames=BTreeMap::from([(p(0),(id(1),0)),(p(20),(id(3),42))]);
  runtime.observe(frames.clone(),p(20));frames.get_mut(&p(0)).unwrap().1=42;runtime.observe(frames.clone(),p(0));
  let mut members=BTreeMap::from([(p(0),BTreeSet::from([p(0)])),(p(20),BTreeSet::from([p(20)]))]);runtime.members=members.clone();
  frames.insert(p(10),(id(2),0));members.insert(p(10),BTreeSet::from([p(9),p(10)]));runtime.added(frames.clone(),members,&PhysicalNodes::new(),p(9));
  assert!(runtime.active);assert_eq!(runtime.registry.peer(id(1)),Some(id(3)));
  frames.get_mut(&p(10)).unwrap().1=42;runtime.observe(frames,p(10));assert_eq!(runtime.registry.peer(id(2)),None);assert_eq!(runtime.registry.peer(id(1)),Some(id(3)));
 }
 #[test]fn adjacent_new_quantum_block_destroys_invalidated_existing_frame(){
  let mut runtime=QuantumRuntime::default();let mut frames=BTreeMap::from([(p(0),(id(1),0)),(p(20),(id(3),42))]);
  runtime.observe(frames.clone(),p(20));frames.get_mut(&p(0)).unwrap().1=42;runtime.observe(frames.clone(),p(0));
  runtime.members=BTreeMap::from([(p(0),BTreeSet::from([p(0)])),(p(20),BTreeSet::from([p(19),p(20),p(21)]))]);
  frames.remove(&p(20));let members=BTreeMap::from([(p(0),BTreeSet::from([p(0)]))]);runtime.added(frames,members,&PhysicalNodes::new(),p(22));
  assert!(runtime.active);assert_eq!(runtime.registry.peer(id(1)),None);assert_eq!(runtime.registry.sides(id(3)),None);
 }
 #[test]fn deferred_rescans_follow_actual_ticker_order_and_reject_partial_readiness(){
  let frames=BTreeMap::from([(p(0),(id(1),42)),(p(10),(id(2),42)),(p(20),(id(3),42))]);
  let members=BTreeMap::from([(p(0),BTreeSet::from([p(0)])),(p(10),BTreeSet::from([p(10)])),(p(20),BTreeSet::from([p(20)]))]);
  let nodes=PhysicalNodes::from([(p(0),FrameNodes::from([(p(0),(id(1),63))])),(p(10),FrameNodes::from([(p(10),(id(2),63))])),(p(20),FrameNodes::from([(p(20),(id(3),63))]))]);
  let mut runtime=QuantumRuntime::default();
  for (node,pos) in [(id(3),p(20)),(id(1),p(0)),(id(2),p(10))] {runtime.ready_rescan(node,pos,frames.clone(),members.clone(),&nodes,true);}
  assert!(runtime.active);assert_eq!(runtime.registry.peer(id(1)),Some(id(3)));assert_eq!(runtime.registry.peer(id(2)),None);
  let mut partial=QuantumRuntime::default();partial.ready_rescan(id(3),p(20),frames.clone(),members.clone(),&nodes,false);
  assert!(partial.formed.contains_key(&p(20)),"one actual rescan forms its frame even when other nodes are not ready");
  partial.ready_rescan(id(1),p(0),frames,members,&nodes,true);assert!(!partial.active,"do not guess a partially observed ready order");
 }
 #[test]fn formation_requires_callback_and_tracks_physical_incarnations(){
  let mut runtime=QuantumRuntime::default();
  let frame=FrameNodes::from([(p(0),(id(1),3)),(p(1),(id(2),63))]);
  let mut nodes=PhysicalNodes::from([(p(0),frame.clone())]);
  let physical=BTreeMap::from([(p(0),(id(1),0))]);
  runtime.forget_unloaded(&nodes,&BTreeSet::new());assert!(runtime.formed.is_empty());assert!(runtime.faces.is_empty());
  runtime.neighbor_formation(p(1),&nodes,&physical);assert_eq!(runtime.faces.get(&id(1)),Some(&3));assert_eq!(runtime.visual.get(&id(2)),Some(&true));
  runtime.removed(p(1),2);assert!(runtime.formed.is_empty());assert_eq!(runtime.visual.get(&id(1)),Some(&false));assert!(!runtime.faces.contains_key(&id(1)));
  nodes.get_mut(&p(0)).unwrap().insert(p(1),(id(3),63));
  runtime.forget_unloaded(&nodes,&BTreeSet::new());assert!(!runtime.faces.contains_key(&id(3)),"silent replacement must not invent formation");
  runtime.neighbor_formation(p(0),&nodes,&physical);assert_eq!(runtime.faces.get(&id(3)),Some(&63));
 }
 #[test]fn partial_ready_center_registers_without_connecting_until_its_actual_tick(){
  let physical=BTreeMap::from([(p(0),(id(1),42)),(p(10),(id(2),42))]);
  let members=BTreeMap::from([(p(0),BTreeSet::from([p(0)])),(p(10),BTreeSet::from([p(10),p(11)]))]);
  let nodes=PhysicalNodes::from([(p(0),FrameNodes::from([(p(0),(id(1),63))])),(p(10),FrameNodes::from([(p(10),(id(2),3)),(p(11),(id(3),63))]))]);
  let mut runtime=QuantumRuntime::default();runtime.node_ready(id(1));runtime.node_ready(id(3));
  runtime.ready_rescan(id(1),p(0),physical.clone(),members.clone(),&nodes,false);
  runtime.ready_rescan(id(3),p(11),physical.clone(),members.clone(),&nodes,false);
  assert_eq!(runtime.source_registry.sides(id(1)),Some((-42,42)));
  assert_eq!(runtime.source_registry.sides(id(2)),Some((42,-42)));
  assert_eq!(runtime.source_registry.peer(id(1)),None);
  runtime.node_ready(id(2));assert_eq!(runtime.source_registry.peer(id(1)),None,"onReady alone does not run cluster status");
  runtime.ready_rescan(id(2),p(10),physical,members,&nodes,true);
  assert_eq!(runtime.source_registry.peer(id(1)),Some(id(2)));
  assert!(!runtime.source_invalid);
 }
 #[test]fn live_formation_and_destruction_emit_source_member_order(){
  let mut runtime=QuantumRuntime::default();let center=p(0);let mut frame=FrameNodes::new();
  for y in -1..=1 {for x in -1..=1 {
   let pos=NetworkPos{x,y,z:0};let n=((y+1)*3+x+2) as u64;
   frame.insert(pos,(id(n),63));
  }}
  let nodes=PhysicalNodes::from([(center,frame.clone())]);let physical=BTreeMap::from([(center,(id(5),0))]);
  runtime.form_at(center,&nodes,&physical);
  let actual:Vec<_>=runtime.pending.iter().filter_map(|event|match event {QuantumEvent::Faces{pos,faces:63,..}=>Some(*pos),_=>None}).collect();
  let expected:Vec<_>=(-1..=1).flat_map(|y|(-1..=1).map(move|x|NetworkPos{x,y,z:0})).collect();
  assert_eq!(actual,expected,"QuantumCalculator visits X first, then Y, then Z");
  runtime.pending.clear();runtime.removed(NetworkPos{x:1,y:1,z:0},9);
  let destroyed:Vec<_>=runtime.pending.iter().filter_map(|event|match event {QuantumEvent::Faces{pos,faces:0,..}=>Some(*pos),_=>None}).collect();
  let mut expected_destroyed=vec![center];expected_destroyed.extend(expected.into_iter().filter(|pos|*pos!=center));
  assert_eq!(destroyed,expected_destroyed,"QuantumCluster destroys the center before its formation-ordered ring");
 }
 #[test]fn link_events_surround_real_face_changes_and_unobserved_unload_invalidates(){
  let mut runtime=QuantumRuntime::default();runtime.node_ready(id(1));runtime.node_ready(id(2));
  let physical=BTreeMap::from([(p(0),(id(1),42)),(p(10),(id(2),42))]);
  let nodes=PhysicalNodes::from([(p(0),FrameNodes::from([(p(0),(id(1),63))])),(p(10),FrameNodes::from([(p(10),(id(2),63))]))]);
  runtime.form_at(p(0),&nodes,&physical);runtime.form_at(p(10),&nodes,&physical);
  assert_eq!(runtime.pending,vec![QuantumEvent::Faces{pos:p(0),id:id(1),faces:63},QuantumEvent::Faces{pos:p(10),id:id(2),faces:63},QuantumEvent::Link{a:id(2),b:id(1),added:true}]);
  runtime.pending.clear();runtime.removed(p(0),1);
  assert_eq!(runtime.pending,vec![QuantumEvent::Link{a:id(1),b:id(2),added:false},QuantumEvent::Faces{pos:p(0),id:id(1),faces:0}]);
  runtime.pending.clear();runtime.forget_unloaded(&PhysicalNodes::new(),&BTreeSet::new());
  assert!(runtime.source_invalid);assert!(runtime.pending.is_empty(),"unobserved unload cannot invent source-ordered events");
 }
 #[test]fn observed_unload_disconnects_first_pair_without_promoting_third_or_changing_other_faces(){
  let mut runtime=QuantumRuntime::default();
  for n in [1,2,3,4] {runtime.node_ready(id(n));}
  let physical=BTreeMap::from([(p(0),(id(1),42)),(p(10),(id(2),42)),(p(20),(id(3),42))]);
  let nodes=PhysicalNodes::from([
   (p(0),FrameNodes::from([(p(0),(id(1),3)),(p(1),(id(4),63))])),
   (p(10),FrameNodes::from([(p(10),(id(2),63))])),
   (p(20),FrameNodes::from([(p(20),(id(3),63))])),
  ]);
  // Actual registration order, deliberately different from position order.
  for pos in [p(20),p(0),p(10)] {runtime.form_at(pos,&nodes,&physical);}
  assert_eq!(runtime.source_registry.peer(id(3)),Some(id(1)));
  assert_eq!(runtime.source_registry.peer(id(2)),None);
  runtime.pending.clear();
  let physical_without_first=PhysicalNodes::from([(p(10),nodes[&p(10)].clone()),(p(20),nodes[&p(20)].clone())]);
  runtime.forget_unloaded(&physical_without_first,&BTreeSet::from([p(0)]));
  assert!(!runtime.source_invalid,"saving a removed chunk before its BE journal is not an unknown transition");
  assert_eq!(runtime.source_registry.peer(id(3)),Some(id(1)));
  assert!(runtime.unloaded(p(1),999).is_empty(),"stale identity cannot tear down a replacement");
  let events=runtime.unloaded(p(1),4);
  assert_eq!(events,vec![ae2_network_core::quantum_registry::LinkEvent{a:id(1),b:id(3),added:false}]);
  assert!(!runtime.source_invalid);
  assert!(runtime.pending.is_empty(),"disconnect(false) emits no world-affecting face updates");
  assert_eq!(runtime.faces.get(&id(1)),Some(&3),"the still-loaded center keeps its constructed face state");
  assert_eq!(runtime.visual.get(&id(1)),Some(&true));
  assert_eq!(runtime.source_registry.peer(id(3)),None);
  assert_eq!(runtime.source_registry.peer(id(2)),None);
  assert!(runtime.unloaded(p(0),1).is_empty(),"remaining members cannot destroy the same cluster twice");
  let reloaded=BTreeMap::from([(p(0),(id(5),42)),(p(10),(id(2),42)),(p(20),(id(3),42))]);
  let mut reloaded_nodes=nodes.clone();
  reloaded_nodes.insert(p(0),FrameNodes::from([(p(0),(id(5),3)),(p(1),(id(6),63))]));
  runtime.node_ready(id(5));runtime.node_ready(id(6));
  runtime.form_at(p(0),&reloaded_nodes,&reloaded);
  assert_eq!(runtime.source_registry.peer(id(5)),Some(id(3)));
  assert_eq!(runtime.source_registry.peer(id(2)),None);
  assert!(runtime.unloaded(p(0),1).is_empty(),"the old unloaded incarnation cannot destroy the reloaded cluster");
  assert_eq!(runtime.source_registry.peer(id(5)),Some(id(3)));
 }
 #[test]fn pending_unload_requires_the_exact_resident_identities_and_a_missing_chunk(){
  let frame=FrameNodes::from([(p(0),(id(1),3)),(p(1),(id(2),63))]);
  assert!(pending_chunk_unload(&frame,|pos|Some((if pos==p(0){1}else{2},false))));
  assert!(!pending_chunk_unload(&frame,|pos|Some((if pos==p(0){1}else{2},true))));
  assert!(!pending_chunk_unload(&frame,|pos|Some((if pos==p(0){1}else{99},false))));
  assert!(!pending_chunk_unload(&frame,|pos|(pos==p(0)).then_some((1,false))));
 }
}
