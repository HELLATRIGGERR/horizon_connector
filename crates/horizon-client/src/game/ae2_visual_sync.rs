//! Compact clientbound AE2 render projection. No inventories, NBT or mutable
//! storage are replicated: the server remains authoritative.
use super::{Game, lan::LanHost};
use horizon_client::containers::BlockEntity;
use horizon_protocol::packets::{configuration::CustomPayload, play::Clientbound};
use horizon_world::nbt::Tag;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    hash::{Hash, Hasher},
};
const CHANNEL: &str = "ae2:visual";
// The upstream writes one packed long array. Bound the assembled count by
// the protocol's actual maximum frame size, not an arbitrary chunk limit.
const MAX_SOURCE_CHUNKS: usize = horizon_protocol::frame::MAX_FRAME / 8;
const ANCHOR_FRAGMENT_CHUNKS: usize = 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Visual {
    pub pos: [i32; 3],
    pub kind: String,
    pub slots: Vec<Option<CellVisual>>,
    pub parts: Vec<Part>,
    #[serde(default)]
    pub facades: Vec<Facade>,
    pub cable: Option<String>,
    pub powered: bool,
    #[serde(default)]
    pub connections: Vec<String>,
    #[serde(default)]
    pub channels: usize,
    #[serde(default)]
    pub paint_dots: Vec<u8>,
    #[serde(default)]
    pub charger_working: bool,
    #[serde(default)]
    pub crank_angle: i32,
    #[serde(default)]
    pub crafting_job: Option<MonitorVisual>,
    #[serde(default)]
    pub painted_color: Option<String>,
    /// Server-owned Spatial Anchor chunks; empty also means disabled.
    #[serde(default)]
    pub anchor_chunks: Vec<[i32; 2]>,
    #[serde(skip)]
    pub(super) anchor_pending: Option<AnchorTransfer>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct CellVisual {
    pub id: String,
    pub state: u8,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Part {
    pub id: String,
    pub face: String,
    #[serde(default)]
    pub online: bool,
    #[serde(default)]
    pub powered: bool,
    #[serde(default)]
    pub plane_edges: [bool; 4],
    #[serde(default)]
    pub monitor: Option<MonitorVisual>,
    #[serde(default)]
    pub monitor_locked: bool,
}
/// Only the configured display key and its public aggregate are replicated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MonitorVisual {
    pub item: String,
    pub components: Vec<u8>,
    pub fluid: bool,
    pub amount: u64,
    pub craftable: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Facade {
    pub face: String,
    pub block: String,
    /// The facade's public material item; a block item may use a distinct ID.
    #[serde(default)]
    pub material: String,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AnchorTransfer { expected: usize, chunks: Vec<[i32;2]> }
#[derive(Clone, Debug, Serialize, Deserialize)]
struct AnchorFragment { pos: [i32;3], total: usize, offset: usize, chunks: Vec<[i32;2]> }
fn apply_anchor_fragment(v:&mut Visual, f:AnchorFragment)->bool {
    if v.kind!="ae2:spatial_anchor" || f.pos!=v.pos || f.total==0 || f.total>MAX_SOURCE_CHUNKS
        || f.chunks.is_empty() || f.chunks.len()>ANCHOR_FRAGMENT_CHUNKS
        || f.offset.checked_add(f.chunks.len()).is_none_or(|n|n>f.total)
        || f.chunks.iter().any(|c|c.iter().any(|n|!(-1_875_000..1_875_000).contains(n))) {return false;}
    if f.offset==0 {v.anchor_pending=Some(AnchorTransfer{expected:f.total,chunks:Vec::new()});}
    let Some(pending)=v.anchor_pending.as_mut() else{return false};
    if pending.expected!=f.total || pending.chunks.len()!=f.offset {v.anchor_pending=None;return false;}
    pending.chunks.extend(f.chunks);
    if pending.chunks.len()==pending.expected {
        let Some(done)=v.anchor_pending.take() else{return false};
        if done.chunks.iter().copied().collect::<HashSet<_>>().len()!=done.expected {return false;}
        v.anchor_chunks=done.chunks;
    }
    true
}
#[derive(Serialize, Deserialize)]
struct Batch {
    dim: String,
    chunk: [i32; 2],
    reset: bool,
    entries: Vec<Visual>,
    #[serde(default)]
    fragment: Option<AnchorFragment>,
}
pub(super) type MonitorCache = BTreeMap<(usize,bool),(Vec<(ae2_cell_core::ItemKey,u64)>,Vec<ae2_cell_core::ItemKey>)>;
pub(super) type CpuJobCache = BTreeMap<ae2_network_core::Pos,Option<(ae2_cell_core::ItemKey,bool,u64)>>;
fn text(t: Option<&Tag>) -> Option<String> {
    match t? {
        Tag::String(s) => s.to_string().ok(),
        _ => None,
    }
}
pub(super) fn visual(pos: (i32, i32, i32), entity: &BlockEntity, powered: bool) -> Option<Visual> {
    let BlockEntity::Ae2(machine) = entity else {
        return None;
    };
    let slots = if machine.kind == horizon_client::ae2_world::MachineKind::Drive {
        machine
            .slots
            .iter()
            .take(10)
            .map(|s| {
                s.as_ref().map(|v| {
                    let cell = super::ae2_lab::read_cell(v)
                        .or_else(|_| super::ae2_fluids::read_fluid_cell(v));
                    let state = match cell {
                        Ok(c) if c.total_items() == 0 => 1,
                        Ok(c) if c.can_hold_new_type() => 2,
                        Ok(c) if c.remaining_items() > 0 => 3,
                        Ok(_) => 4,
                        Err(_) => 0,
                    };
                    CellVisual {
                        id: v.item.to_string(),
                        state,
                    }
                })
            })
            .collect()
    } else if machine.kind==horizon_client::ae2_world::MachineKind::Charger {
        machine.slots.iter().take(1).map(|s|s.as_ref().map(|stack|CellVisual{id:stack.item.to_string(),state:0})).collect()
    } else {
        Vec::new()
    };
    let mut parts = Vec::new();
    if let Some(Tag::List(list)) = machine.config.get(&"parts".into()) {
        for p in list.iter().take(6) {
            if let (Some(id), Some(face)) = (text(p.get("id")), text(p.get("face"))) {
                parts.push(Part { id, face, online: false, powered:false, plane_edges: [false; 4], monitor: None, monitor_locked:false });
            }
        }
    }
    parts.sort_by(|a, b| a.face.cmp(&b.face));
    let mut facades=Vec::new();
    if let Some(Tag::List(entries))=machine.config.get(&"facades".into()) {
        let mut seen=HashSet::new();
        for entry in entries.iter().take(6) {
            let Some(face)=text(entry.get("face")) else {continue};
            if !matches!(face.as_str(),"down"|"up"|"north"|"south"|"west"|"east") || seen.contains(&face) {continue;}
            let Some(stack)=entry.get("stack").filter(|t|t.get("count")==Some(&Tag::Int(1))).and_then(horizon_client::save::item_from_nbt) else {continue};
            if stack.count!=1 || stack.item.as_ref()!="ae2:facade" {continue;}
            let Some(material)=horizon_client::item_tooltip::ae2_facade_texture(&stack) else {continue};
            let (block, properties) = match entry.get("state") {
                Some(tag) => match horizon_world::chunk::BlockState::from_nbt(tag) {
                    Ok(state) if state.name.len() <= 128 && state.properties.len() <= 16
                        && state.properties.iter().all(|(k,v)| k.len() <= 64 && v.len() <= 64) => (state.name, state.properties),
                    _ => continue,
                },
                None => (material.clone(), BTreeMap::new()),
            };
            if material.len()>128 {continue} seen.insert(face.clone()); facades.push(Facade{face,block,material,properties});
        }
    }
    facades.sort_by(|a,b|a.face.cmp(&b.face));
    let cable = machine
        .config
        .get(&"cable".into())
        .and_then(|tag| text(tag.get("id")));
    let paint_dots=match machine.config.get(&"dots".into()) {Some(Tag::ByteArray(bytes)) if machine.kind==horizon_client::ae2_world::MachineKind::Paint && bytes.len()<=43=>bytes.clone(),_=>vec![]};
    Some(Visual {
        pos: [pos.0, pos.1, pos.2],
        kind: machine.kind.id().into(),
        slots,
        parts,
        facades,
        cable,
        powered,
        connections: Vec::new(),
        channels: 0,
        paint_dots,
        charger_working: machine.kind==horizon_client::ae2_world::MachineKind::Charger && machine.config.get(&"charger_working".into())==Some(&Tag::Byte(1)),
        crank_angle:0,
        crafting_job:None,
        anchor_chunks:Vec::new(),
        anchor_pending:None,
        painted_color:if machine.kind==horizon_client::ae2_world::MachineKind::CraftingMonitor {text(machine.config.get(&"painted_color".into()))} else {None},
    })
}
impl Game<'_> {
    pub(super) fn ae2_enrich_anchor_visual(&self,v:&mut Visual) {
        if v.kind!="ae2:spatial_anchor" {return;}
        let Some(address)=self.ae2_local_address((v.pos[0],v.pos[1],v.pos[2])) else{return};
        let Some((enabled,chunks))=self.ae2_anchor_overlay_snapshot(address) else{return};
        if enabled && chunks.len()<=MAX_SOURCE_CHUNKS {
            v.anchor_chunks=chunks.into_iter().map(|(x,z)|[x,z]).collect();
        }
    }
    pub(super) fn ae2_enrich_monitor_visual(&self,v:&mut Visual,network:&ae2_network_core::Network,analysis:&ae2_network_core::Analysis,cache:&mut MonitorCache,jobs:&mut CpuJobCache) {
        let pos=(v.pos[0],v.pos[1],v.pos[2]);
        let Some(address)=self.ae2_local_address(pos) else{return};
        let point=address.network_pos();
        let Some(node)=analysis.nodes.get(&point) else{return};
        if v.kind=="ae2:crafting_monitor" {
            if let Some(group)=network.nodes.get(&point).and_then(|d|d.multiblock) {
                let job=jobs.entry(group).or_insert_with(||self.ae2_crafting_monitor_job(pos));
                if let Some((key,fluid,amount))=job && key.item_id.len()<=512 && key.components.len()<=65536 {
                    v.crafting_job=Some(MonitorVisual{item:key.item_id.clone(),components:key.components.clone(),fluid:*fluid,amount:*amount,craftable:false});
                }
            }
            return;
        }
        for part in &mut v.parts {
            let Some(state)=self.ae2_monitor_state(pos,&part.face) else {continue};
            part.monitor_locked=state.locked;
            let Some((key,fluid))=state.key else {continue};
            // The watched prototype is visible on the monitor; bound the
            // clientbound payload even when an item has unusually large NBT.
            if key.item_id.len()>512 || key.components.len()>65536 {continue;}
            let (inventory,craftable)=cache.entry((node.component,fluid)).or_insert_with(||{
                if fluid {(self.ae2_grid_fluids(pos).unwrap_or_default(),self.ae2_craftable_fluids(pos))}
                else {(self.ae2_grid_inventory(pos).unwrap_or_default(),self.ae2_craftable_outputs(pos))}
            });
            let amount=inventory.iter().find_map(|(k,n)|(k==&key).then_some(*n)).unwrap_or(0);
            let can_craft=craftable.contains(&key);
            part.monitor=Some(MonitorVisual{item:key.item_id,components:key.components,fluid,amount,craftable:can_craft});
        }
    }
    pub(super) fn ae2_apply_dynamic_shape(&mut self, v: &Visual) {
        if v.kind != "ae2:cable_bus" { return; }
        let p=(v.pos[0],v.pos[1],v.pos[2]);
        let state=self.world.block(p.0,p.1,p.2);
        if self.pack.assets.registry.state(state).is_none_or(|s|s.name!="ae2:cable_bus") { self.world.dynamic_shapes.remove(&p); return; }
        let outline=super::ae2_geometry::boxes(v,false);
        if self.world.dynamic_shapes.get(&p).is_some_and(|(old,_,boxes)| *old==state && *boxes==outline) { return; }
        let mut shape=horizon_core::geometry::VoxelShape::empty();
        for b in super::ae2_geometry::boxes(v,true) {
            let a=horizon_core::geometry::Aabb::new(
                horizon_core::geometry::Vec3::new(b[0],b[1],b[2]),
                horizon_core::geometry::Vec3::new(b[3],b[4],b[5]));
            let Ok(part)=horizon_core::geometry::VoxelShape::from_box(a) else { return };
            let Ok(joined)=shape.join(&part,horizon_core::geometry::ShapeOp::OR) else { return };
            shape=joined;
        }
        self.world.dynamic_shapes.insert(p,(state,shape,outline));
    }
    pub(super) fn ae2_enrich_visual(&self, v: &mut Visual, network: &ae2_network_core::Network, analysis: &ae2_network_core::Analysis) {
        let p = (v.pos[0],v.pos[1],v.pos[2]);
        let Some(address) = self.ae2_local_address(p) else { return };
        let node = address.network_pos();
        if let Some(status) = analysis.nodes.get(&node) {
            v.channels = status.channels_used;
            for part in &mut v.parts {
                let face = match part.face.as_str() {"east"=>0,"west"=>1,"up"=>2,"down"=>3,"south"=>4,"north"=>5,_=>continue};
                part.online = status.parts.get(&face).is_some_and(|s| s.online);
                part.powered = status.powered;
            }
        }
        for part in &mut v.parts {
            if !matches!(part.id.as_str(), "ae2:annihilation_plane"|"ae2:formation_plane") { continue; }
            // Local +X/+Y directions transformed into the side's world frame.
            let (u,w)=match part.face.as_str() {
                "north"=>((-1,0,0),(0,1,0)), "south"=>((1,0,0),(0,1,0)),
                "east"=>((0,0,-1),(0,1,0)), "west"=>((0,0,1),(0,1,0)),
                "up"=>((1,0,0),(0,0,-1)), "down"=>((1,0,0),(0,0,1)), _=>continue,
            };
            for (i,(dx,dy,dz)) in [(-u.0,-u.1,-u.2),u,(-w.0,-w.1,-w.2),w].into_iter().enumerate() {
                let q=(p.0+dx,p.1+dy,p.2+dz);
                part.plane_edges[i]=self.world.block_entities.get(&q).and_then(|e|visual(q,e,false))
                    .is_some_and(|n|n.parts.iter().any(|other|other.id==part.id && other.face==part.face));
            }
        }
        for (face, (dx,dy,dz)) in [("down",(0,-1,0)),("up",(0,1,0)),("north",(0,0,-1)),("south",(0,0,1)),("west",(-1,0,0)),("east",(1,0,0))] {
            if let Some(other) = self.ae2_local_address((p.0+dx,p.1+dy,p.2+dz)) {
                let other=other.network_pos();
                let connected=if matches!(v.kind.as_str(), "ae2:crafting_monitor" | "ae2:crafting_unit" | "ae2:crafting_accelerator" | "ae2:1k_crafting_storage" | "ae2:4k_crafting_storage" | "ae2:16k_crafting_storage" | "ae2:64k_crafting_storage" | "ae2:256k_crafting_storage") {
                    network.nodes.get(&node).and_then(|d|d.multiblock).is_some_and(|group|network.nodes.get(&other).and_then(|d|d.multiblock)==Some(group))
                } else {network.physical_connection(node,other)};
                if connected {v.connections.push(face.to_owned());}
            }
        }
    }
    pub(super) fn ae2_sync_visuals(&mut self, host: &mut LanHost) {
        let dim = self.dim.id().to_owned();
        let network = self.ae2_network();
        let analysis = network.analyze().ok();
        let mut monitor_cache=MonitorCache::new();
        let mut cpu_jobs=CpuJobCache::new();
        let mut grouped: BTreeMap<(i32, i32), Vec<Visual>> = BTreeMap::new();
        for (&p, e) in self.world.block_entities.iter() {
            let powered = self
                .ae2_local_address(p)
                .and_then(|a| analysis.as_ref()?.nodes.get(&a.network_pos()))
                .is_some_and(|s| s.online);
            if let Some(mut v) = visual(p, e, powered) {
                self.ae2_enrich_anchor_visual(&mut v);
                if v.kind=="ae2:crank" {v.crank_angle=self.ae2_cranks.get(&(self.dim,p)).map_or(0,|s|s.angle);}
                if let Some(a) = &analysis { self.ae2_enrich_visual(&mut v, &network, a); self.ae2_enrich_monitor_visual(&mut v,&network,a,&mut monitor_cache,&mut cpu_jobs); }
                grouped.entry((p.0 >> 4, p.2 >> 4)).or_default().push(v);
            }
        }
        for values in grouped.values_mut() {
            values.sort_by_key(|v| v.pos);
        }
        let positions: HashSet<_> = grouped.values().flatten().filter(|v|v.kind=="ae2:cable_bus").map(|v|(v.pos[0],v.pos[1],v.pos[2])).collect();
        self.world.dynamic_shapes.retain(|p,_|positions.contains(p));
        for v in grouped.values().flatten() { self.ae2_apply_dynamic_shape(v); }
        let mut keep = HashSet::new();
        for g in &host.hub.guests {
            if g.dimension != dim {
                continue;
            }
            for &(cx, cz) in &g.sent_chunks {
                let key = (g.conn, dim.clone(), cx, cz);
                keep.insert(key.clone());
                let entries = grouped.get(&(cx, cz)).map(Vec::as_slice).unwrap_or(&[]);
                if entries.is_empty() && !host.ae2_visual_sent.contains_key(&key) {
                    continue;
                }
                let Ok(bytes) = serde_json::to_vec(entries) else {
                    continue;
                };
                let mut h = std::collections::hash_map::DefaultHasher::new();
                bytes.hash(&mut h);
                let hash = h.finish();
                if host.ae2_visual_sent.get(&key) == Some(&hash) {
                    continue;
                }
                // The normal visual establishes the owner first. Ticket
                // coordinates are sent separately in bounded fragments and
                // made visible only after the complete set arrives.
                for (i, chunk) in entries.chunks(32).enumerate() {
                    let mut base=chunk.to_vec();
                    for v in &mut base {v.anchor_chunks.clear();}
                    let batch=Batch{dim:dim.clone(),chunk:[cx,cz],reset:i==0,entries:base,fragment:None};
                    if let Ok(data)=serde_json::to_vec(&batch) {g.send(&Clientbound::CustomPayload(CustomPayload{channel:CHANNEL.into(),data}));}
                }
                for v in entries.iter().filter(|v|!v.anchor_chunks.is_empty()) {
                    for (i,part) in v.anchor_chunks.chunks(ANCHOR_FRAGMENT_CHUNKS).enumerate() {
                        let fragment=AnchorFragment{pos:v.pos,total:v.anchor_chunks.len(),offset:i*ANCHOR_FRAGMENT_CHUNKS,chunks:part.to_vec()};
                        let batch=Batch{dim:dim.clone(),chunk:[cx,cz],reset:false,entries:vec![],fragment:Some(fragment)};
                        if let Ok(data)=serde_json::to_vec(&batch) {g.send(&Clientbound::CustomPayload(CustomPayload{channel:CHANNEL.into(),data}));}
                    }
                }
                if entries.is_empty() {
                    let batch = Batch {
                        dim: dim.clone(),
                        chunk: [cx, cz],
                        reset: true,
                        entries: vec![],
                        fragment: None,
                    };
                    if let Ok(data) = serde_json::to_vec(&batch) {
                        g.send(&Clientbound::CustomPayload(CustomPayload {
                            channel: CHANNEL.into(),
                            data,
                        }));
                    }
                }
                host.ae2_visual_sent.insert(key, hash);
            }
        }
        host.ae2_visual_sent
            .retain(|k, _| k.1 != dim || keep.contains(k));
    }
    pub(super) fn ae2_accept_visual_payload(&mut self, channel: &str, bytes: &[u8]) {
        if channel != CHANNEL || bytes.len() > 131_072 {
            return;
        }
        let Ok(batch) = serde_json::from_slice::<Batch>(bytes) else {
            return;
        };
        if batch.dim != self.dim.id() || batch.entries.len() > 32 {
            return;
        }
        if batch.entries.iter().any(|v| {
            v.pos[0] >> 4 != batch.chunk[0]
                || v.pos[2] >> 4 != batch.chunk[1]
                || v.kind.len() > 128
                || v.slots.len() > 10
                || v.parts.len() > 6
                || v.facades.len() > 6
                || v.facades.iter().any(|f| f.face.len()>16 || f.block.len()>128 || f.material.len()>128 || f.properties.len()>16
                    || f.properties.iter().any(|(k,v)| k.len()>64 || v.len()>64))
                || v.connections.len() > 6
                || v.paint_dots.len() > 43
                || v.anchor_chunks.len()>MAX_SOURCE_CHUNKS
                || v.anchor_chunks.iter().any(|c|c.iter().any(|n| !(-1_875_000..1_875_000).contains(n)))
                || (!v.anchor_chunks.is_empty() && v.kind!="ae2:spatial_anchor")
                || v.slots
                    .iter()
                    .flatten()
                    .any(|s| s.id.len() > 128 || s.state > 4)
                || v.parts
                    .iter()
                    .any(|p| p.id.len() > 128 || p.face.len() > 16)
        }) {
            return;
        }
        if let Some(fragment)=batch.fragment {
            if batch.reset || !batch.entries.is_empty() || fragment.pos[0]>>4!=batch.chunk[0] || fragment.pos[2]>>4!=batch.chunk[1] {return;}
            if let Some(v)=self.ae2_visual_cache.get_mut(&(fragment.pos[0],fragment.pos[1],fragment.pos[2])) {apply_anchor_fragment(v,fragment);}
            return;
        }
        if batch.reset {
            self.world.dynamic_shapes.retain(|p,_| p.0 >> 4 != batch.chunk[0] || p.2 >> 4 != batch.chunk[1]);
            self.ae2_visual_cache
                .retain(|p, _| p.0 >> 4 != batch.chunk[0] || p.2 >> 4 != batch.chunk[1]);
        }
        for v in batch.entries {
            self.ae2_apply_dynamic_shape(&v);
            self.ae2_visual_cache
                .insert((v.pos[0], v.pos[1], v.pos[2]), v);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn anchor_fragments_publish_only_complete_bounded_ordered_sets() {
        use horizon_client::ae2_world::{Machine,MachineKind};
        let mut v=visual((1,2,3),&BlockEntity::Ae2(Box::new(Machine::new(MachineKind::SpatialAnchor))),false).unwrap();
        let all:Vec<[i32;2]>=(0..5001).map(|i|[i,0]).collect();
        let pos=v.pos;
        for (i,chunk) in all.chunks(ANCHOR_FRAGMENT_CHUNKS).enumerate() {
            let f=AnchorFragment{pos,total:all.len(),offset:i*ANCHOR_FRAGMENT_CHUNKS,chunks:chunk.to_vec()};
            let batch=Batch{dim:"minecraft:overworld".into(),chunk:[0,0],reset:false,entries:vec![],fragment:Some(f)};
            let encoded=serde_json::to_vec(&batch).unwrap();assert!(encoded.len()<=131_072);
            let decoded:Batch=serde_json::from_slice(&encoded).unwrap();
            assert!(apply_anchor_fragment(&mut v,decoded.fragment.unwrap()));
            if i<4 {assert!(v.anchor_chunks.is_empty());}
        }
        assert_eq!(v.anchor_chunks,all);
        let old=v.anchor_chunks.clone();let pos=v.pos;
        assert!(!apply_anchor_fragment(&mut v,AnchorFragment{pos,total:MAX_SOURCE_CHUNKS+1,offset:0,chunks:vec![[1,2]]}));
        assert_eq!(v.anchor_chunks,old);
        assert!(apply_anchor_fragment(&mut v,AnchorFragment{pos,total:2,offset:0,chunks:vec![[0,0]]}));
        assert!(!apply_anchor_fragment(&mut v,AnchorFragment{pos,total:2,offset:2,chunks:vec![[1,0]]}));
        assert_eq!(v.anchor_chunks,old);
    }
    #[test]
    fn source_facade_projection_preserves_only_material_and_public_state() {
        use horizon_client::{ae2_world::{Machine,MachineKind},items::ItemStack,save};
        let mut facade=ItemStack::new("ae2:facade",1);
        facade.data_mut().set_other("ae2:facade_item",Some(Tag::String("example:oak_log_item".into())));
        facade.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("secret",Tag::Int(42))])));
        let mut machine=Machine::new(MachineKind::CableBus);
        let state=horizon_world::chunk::BlockState{name:"minecraft:oak_log".into(),properties:BTreeMap::from([("axis".into(),"z".into())])};
        machine.config.insert("facades".into(),Tag::List(vec![Tag::compound([
            ("face",Tag::String("south".into())),("stack",save::item_to_nbt(&facade)),("state",state.to_nbt()),
        ])]));
        let projection=visual((1,2,3),&BlockEntity::Ae2(Box::new(machine)),false).unwrap();
        assert_eq!(projection.facades[0].block,"minecraft:oak_log");
        assert_eq!(projection.facades[0].material,"example:oak_log_item");
        assert_eq!(projection.facades[0].properties.get("axis").map(String::as_str),Some("z"));
        let encoded=serde_json::to_string(&projection).unwrap();assert!(!encoded.contains("secret"));
        assert!(encoded.contains("\"axis\":\"z\""));
    }
    #[test]
    fn visual_projection_exposes_ids_not_item_components() {
        let mut m =
            horizon_client::ae2_world::Machine::new(horizon_client::ae2_world::MachineKind::Drive);
        let mut item = horizon_client::items::ItemStack::new("ae2:item_storage_cell_1k", 1);
        item.data_mut().set_other(
            "minecraft:custom_data",
            Some(Tag::compound([("secret", Tag::Int(42))])),
        );
        m.slots[0] = Some(item);
        let v = visual((1, 2, 3), &BlockEntity::Ae2(Box::new(m)), true).unwrap();
        let data = serde_json::to_string(&v).unwrap();
        assert!(data.contains("ae2:item_storage_cell_1k"));
        assert!(!data.contains("secret"));
    }
}
