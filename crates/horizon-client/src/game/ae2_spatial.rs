//! Spatial cell region exchange, derived from AE2 19.2.18 SpatialStorageCellItem,
//! SpatialIOPortBlockEntity and CachedPlane (LGPL-3.0-or-later).
use super::{Dim, Game};
use ae2_links_core::{Pos, PylonLine, SpatialRegion};
use ae2_network_core::{DeviceKind, Network, Pos as NetworkPos};
use horizon_client::{
    ae2_world::MachineKind, containers::BlockEntity, items::ItemStack, streaming::ChunkGenerator,
};
use horizon_core::{
    position::{BlockPos, ChunkPos},
    ticks::{ScheduledTick, TickArea},
};
use horizon_world::{nbt::Tag, registry::BlockStateId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashSet},
    path::PathBuf,
};
type WorldPos = (i32, i32, i32);
/// Ordered Source SpatialStorageHelper.transverseEdges visitor positions.
fn transverse_edges(min: WorldPos, max: WorldPos) -> Vec<WorldPos> {
    let mut out = Vec::new();
    for y in min.1..max.1 { for z in min.2..max.2 {
        out.push((min.0, y, z)); out.push((max.0, y, z));
    }}
    for x in min.0..max.0 { for z in min.2..max.2 {
        out.push((x, min.1, z)); out.push((x, max.1, z));
    }}
    for x in min.0..max.0 { for y in min.1..max.1 {
        out.push((x, y, min.2)); out.push((x, y, max.2));
    }}
    out
}
fn np(p: WorldPos) -> NetworkPos {
    NetworkPos {
        x: p.0,
        y: p.1,
        z: p.2,
    }
}
fn world(p: Pos) -> WorldPos {
    (p.0, p.1, p.2)
}
fn area(min: WorldPos, size: [i32; 3]) -> TickArea {
    TickArea::new(
        BlockPos::new(min.0, min.1, min.2),
        BlockPos::new(
            min.0 + size[0] - 1,
            min.1 + size[1] - 1,
            min.2 + size[2] - 1,
        ),
    )
}
fn keys(min: WorldPos, size: [i32; 3]) -> impl Iterator<Item = (i32, i32)> {
    let (x, z) = (min.0 >> 4, min.2 >> 4);
    let (maxx, maxz) = ((min.0 + size[0] - 1) >> 4, (min.2 + size[2] - 1) >> 4);
    (x..=maxx).flat_map(move |x| (z..=maxz).map(move |z| (x, z)))
}
#[derive(Clone, Serialize, Deserialize)]
struct Plot {
    id: i32,
    size: [i32; 3],
    last_dimension: String,
    last_min: WorldPos,
    last_max: WorldPos,
}
#[derive(Default, Serialize, Deserialize)]
struct Plots {
    next: i32,
    plots: Vec<Plot>,
}
struct Block {
    offset: [i32; 3],
    state: BlockStateId,
    entity: Option<BlockEntity>,
    raw_entity: Option<Tag>,
    name: Option<String>,
    vanilla_metadata: Option<Tag>,
    rolled_loot: Option<horizon_client::save::RolledLoot>,
    skip: bool,
}
struct Plane {
    blocks: Vec<Block>,
    ticks: Vec<ScheduledTick>,
    fluid_ticks: Vec<ScheduledTick>,
}
fn capacity(s: &ItemStack) -> Option<i32> {
    match s.item.as_ref() {
        "ae2:spatial_storage_cell_2" => Some(2),
        "ae2:spatial_storage_cell_16" => Some(16),
        "ae2:spatial_storage_cell_128" => Some(128),
        _ => None,
    }
}
/// SpatialStorageCellItem.getAllocatedPlotId reads the component's ID; the
/// plot manager is authoritative for whether it exists and its actual size.
fn spatial_plot_id(cell: &ItemStack) -> Option<i32> {
    let data = cell.data()?;
    let id = match data.other("ae2:spatial_plot_info") {
        Some(component) => component.get("id"),
        // Native worlds created before this component existed remain usable.
        None => data.other("minecraft:custom_data").and_then(|tag| tag.get("ae2_spatial_plot")),
    }?;
    // Codec.INT uses Number.intValue(), including narrower or wider numeric
    // tags accepted by NbtOps and wrapping a long's low 32 bits.
    match id {
        Tag::Byte(id) => Some(i32::from(*id)),
        Tag::Short(id) => Some(i32::from(*id)),
        Tag::Int(id) => Some(*id),
        Tag::Long(id) => Some(*id as i32),
        Tag::Float(id) => Some(f32::from_bits(*id) as i32),
        Tag::Double(id) => Some(f64::from_bits(*id) as i32),
        _ => None,
    }
}
fn set_spatial_plot(cell: &mut ItemStack, id: i32, size: [i32; 3]) {
    cell.data_mut().set_other("ae2:spatial_plot_info", Some(Tag::compound([
        ("id", Tag::Int(id)), ("size", Tag::IntArray(size.to_vec())),
    ])));
}
impl Game<'_> {
    pub(super) fn ae2_spatial_graph(&self, network: &mut Network) {
        let pylons=network.nodes.keys().filter_map(|p| {
            let address=super::ae2_global::GridAddress::from_network(*p)?;
            matches!(self.ae2_block_entity(address),Some(BlockEntity::Ae2(m))if m.kind==MachineKind::SpatialPylon).then_some(Pos(p.x,p.y,p.z))
        }).collect::<BTreeSet<_>>();
        for p in &pylons {
            if let Some(d) = network.nodes.get_mut(&np(world(*p))) {
                d.channel_override = Some(true);
                d.multiblock = None;
                d.faces = 0;
            }
        }
        for line in PylonLine::scan(&pylons) {
            for i in 0..line.count {
                let mut p = [line.min.0, line.min.1, line.min.2];
                p[line.axis] += i as i32;
                if let Some(d) = network.nodes.get_mut(&np((p[0], p[1], p[2]))) {
                    d.faces = 63;
                    d.multiblock = Some(np(world(line.min)));
                }
            }
            if let Some(d) = network.nodes.get_mut(&np(world(line.min))) {
                d.channel_override = Some(true);
                d.multiblock = Some(np(world(line.min)));
            }
        }
    }
    fn ae2_spatial_region(&self, pos: WorldPos) -> Result<SpatialRegion, String> {
        let address = self.ae2_local_address(pos).ok_or("invalid spatial port position")?;
        self.ae2_spatial_region_from(address, None)
    }
    /// Resolve the Spatial service reached by the actual selected grid node.
    /// Replicator cards can address a host in a different loaded dimension.
    pub(super) fn ae2_spatial_region_from(
        &self,
        address: super::ae2_global::GridAddress,
        face: Option<u8>,
    ) -> Result<SpatialRegion, String> {
        let (dimension, _) = self.ae2_address_parts(address).ok_or("invalid grid position")?;
        let network = self.ae2_network();
        let analysis = network.analyze().map_err(|e| format!("network: {e:?}"))?;
        let host = address.network_pos();
        let node = if let Some(face) = face {
            if let Some(endpoint) = network.external_ports.get(&(host, face)) {
                *endpoint
            } else if matches!(self.ae2_block_entity(address), Some(BlockEntity::Ae2(m))
                if m.kind == MachineKind::CableBus
                    && m.config.get(&"ae2_centerless".into()) == Some(&Tag::Byte(1))
                    && !m.config.contains_key(&"cable".into())) {
                return Err("No grid node?".into());
            } else { host }
        } else { host };
        let status = analysis
            .nodes
            .get(&node)
            .filter(|s| face.is_some() || s.online)
            .ok_or("spatial I/O port is offline")?;
        let pylons=analysis.components[status.component].positions.iter().filter_map(|p| {
            let address=super::ae2_global::GridAddress::from_network(*p)?;
            let (pylon_dim,local)=self.ae2_address_parts(address)?;
            if pylon_dim!=dimension {return None;}
            matches!(self.ae2_block_entity(address),Some(BlockEntity::Ae2(m))if m.kind==MachineKind::SpatialPylon).then_some(Pos(local.0,local.1,local.2))
        }).collect::<BTreeSet<_>>();
        let lines = PylonLine::scan(&pylons)
            .into_iter()
            .filter(|l| {
                self.ae2_dimension_address(dimension, world(l.min))
                    .and_then(|p| analysis.nodes.get(&p.network_pos()))
                    .is_some_and(|s| s.online)
            })
            .collect::<Vec<_>>();
        let config = super::ae2_common_config::spatial();
        SpatialRegion::from_lines(&lines, config.power_multiplier, config.power_exponent).map_err(str::to_owned)
    }
    fn ae2_plot_file(&self) -> PathBuf {
        self.world_root.join("data/ae2/native_spatial_plots.json")
    }
    fn ae2_read_plots(&self) -> Result<Plots, String> {
        let p = self.ae2_plot_file();
        if !p.exists() {
            return Ok(Plots::default());
        }
        serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?)
            .map_err(|e| format!("spatial plot data: {e}"))
    }
    fn ae2_write_plots(&self, plots: &Plots) -> Result<(), String> {
        let p = self.ae2_plot_file();
        std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
        let temp = p.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec(plots).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        std::fs::rename(temp, p).map_err(|e| e.to_string())
    }
    fn ae2_spatial_load_destination(
        &mut self,
        min: WorldPos,
        size: [i32; 3],
    ) -> Result<(), String> {
        if self.dim != Dim::Spatial {
            return Err("destination is not the spatial dimension".into());
        }
        // Load saved chunks before either region; only a genuinely absent record
        // gets the source full Matrix Frame column, exactly as background generation.
        let matrix = self.pack.assets.registry.default_state("ae2:matrix_frame").ok_or("matrix frame is unavailable")?;
        let terrain = horizon_client::flatgen::FlatTerrain::filled_range(self.res.biomes().id("ae2:spatial_storage"), matrix, 0..256).ok_or("invalid spatial generator height")?;
        for k in keys(
            (min.0 - 1, min.1 - 1, min.2 - 1),
            [size[0] + 2, size[1] + 2, size[2] + 2],
        ) {
            if self.world.chunks.contains_key(&k) {
                continue;
            }
            let mut chunk = self
                .save
                .load_checked(k.0, k.1, self.pack.assets.registry, self.res.biomes())
                .map_err(|e| format!("cannot read Spatial chunk {} {}: {e}", k.0, k.1))?
                .unwrap_or_else(|| terrain.finish(terrain.proto(&mut (), k.0, k.1)));
            let persisted_entities = self.save.load_entities_checked(k.0,k.1)
                .map_err(|e| format!("cannot read Spatial entities {} {}: {e}",k.0,k.1))?;
            chunk.entities.extend(persisted_entities);
            self.streaming.insert_authoritative_flat(&mut self.world,chunk)?;
        }
        self.spatial_register_ticks();
        self.spawn_loaded_entities();
        Ok(())
    }
    fn spatial_register_ticks(&mut self) {
        if self.tick_chunks.synchronize(&self.world.chunks) {
            let added = self.tick_chunks.take_added();
            for &key in &added {
                self.register_chunk_ticks(key);
            }
            self.tick_chunks.recycle_added(added);
        }
        for &key in self.world.chunks.keys() {
            let p = ChunkPos::new(key.0, key.1);
            self.block_ticks
                .blocks
                .unpack_chunk(p, self.world.game_time);
            self.block_ticks
                .fluids
                .unpack_chunk(p, self.world.game_time);
        }
    }
    fn ae2_spatial_snapshot(&mut self, min: WorldPos, size: [i32; 3]) -> Result<Plane, String> {
        if size.iter().any(|&n| n <= 0)
            || min.1 < self.dim.min_y()
            || min
                .1
                .checked_add(size[1])
                .is_none_or(|y| y > self.dim.max_y() + 1)
        {
            return Err("spatial region exceeds world height".into());
        }
        if keys(min, size).any(|k| !self.world.chunks.contains_key(&k)) {
            return Err("spatial region crosses unloaded chunks".into());
        }
        self.spatial_register_ticks();
        let mut blocks = Vec::with_capacity(size.into_iter().map(|n| n as usize).product());
        let blacklist = self.res.items().block_tags.get("ae2:blacklisted/spatial");
        for y in 0..size[1] {
            for z in 0..size[2] {
                for x in 0..size[0] {
                    let p = (min.0 + x, min.1 + y, min.2 + z);
                    let state = self.world.block(p.0, p.1, p.2);
                    let id = &self.res.facts().get(state).name;
                    let entity=self.world.block_entities.get(&p).cloned();
                    let raw_id=match id.as_ref() {
                        "minecraft:decorated_pot"=>Some("minecraft:decorated_pot"),
                        "minecraft:chiseled_bookshelf"=>Some("minecraft:chiseled_bookshelf"),
                        "minecraft:campfire"|"minecraft:soul_campfire"=>Some("minecraft:campfire"),
                        "minecraft:enchanting_table"=>Some("minecraft:enchanting_table"),
                        other if other.ends_with("_shelf")=>Some("minecraft:shelf"),
                        _=>None,
                    };
                    let raw_entity=if entity.is_none() {
                        raw_id.and_then(|expected|super::spill::saved_entity(&self.world,[p.0,p.1,p.2])
                            .filter(|raw|raw.get("id")==Some(&Tag::String(expected.into()))))
                    } else {None};
                    let vanilla_metadata=self.world.block_entities.vanilla_metadata(&p).cloned();
                    let table=vanilla_metadata.as_ref().and_then(|tag|horizon_client::save::text(tag.get("LootTable")));
                    let rolled_loot=table.as_deref().and_then(|table|self.world.chunks.get(&(p.0>>4,p.2>>4))
                        .and_then(|chunk|chunk.saved.as_deref())
                        .and_then(|saved|saved.rolled_loot.iter().find(|roll|roll.pos==p && roll.table==table)).cloned());
                    blocks.push(Block {
                        offset: [x, y, z],
                        state,
                        entity,
                        raw_entity,
                        name: self.world.block_names.get(&p).cloned(),
                        vanilla_metadata,
                        rolled_loot,
                        skip: blacklist.is_some_and(|set| set.contains(id.as_ref())),
                    });
                }
            }
        }
        let bounds = area(min, size);
        let ticks = self.block_ticks.blocks.area_ticks(bounds);
        let fluid_ticks = self.block_ticks.fluids.area_ticks(bounds);
        Ok(Plane {
            blocks,
            ticks,
            fluid_ticks,
        })
    }
    fn ae2_spatial_write(
        &mut self,
        min: WorldPos,
        size: [i32; 3],
        from: WorldPos,
        plane: &Plane,
        skip: &HashSet<[i32; 3]>,
        strip_frame: bool,
    ) -> Result<(), String> {
        let frame = self.pack.assets.registry.default_state("ae2:matrix_frame");
        let mut raw_positions=HashSet::new();
        let mut rolled_positions=HashSet::new();
        for chunk in keys(min,size).filter_map(|key|self.world.chunks.get(&key)) {
            if let Some(saved)=chunk.saved.as_deref() {
                for tag in &saved.block_entities {
                    if let (Some(Tag::Int(x)),Some(Tag::Int(y)),Some(Tag::Int(z)))=(tag.get("x"),tag.get("y"),tag.get("z")) {
                        raw_positions.insert((*x,*y,*z));
                    }
                }
                rolled_positions.extend(saved.rolled_loot.iter().map(|roll|roll.pos));
            }
        }
        for block in &plane.blocks {
            if skip.contains(&block.offset) {
                continue;
            }
            let p = (
                min.0 + block.offset[0],
                min.1 + block.offset[1],
                min.2 + block.offset[2],
            );
            self.world.block_entities.remove(&p);
            self.world.block_names.remove(&p);
            let state = if strip_frame && Some(block.state) == frame {
                self.res.facts().air
            } else {
                block.state
            };
            self.world
                .set(p.0, p.1, p.2, state)
                .map_err(|e| e.to_string())?;
            if let Some(entity) = &block.entity {
                self.world.block_entities.insert(p, entity.clone());
                if let Some(raw)=block.vanilla_metadata.as_ref() {
                    self.world.block_entities.remember_vanilla_metadata(p,raw);
                }
            }
            if let Some(name) = &block.name {
                self.world.block_names.insert(p, name.clone());
            }
            if let Some(chunk)=self.world.chunks.get_mut(&(p.0>>4,p.2>>4)) {
                let old=rolled_positions.remove(&p);
                let old_raw=raw_positions.remove(&p);
                if old || block.rolled_loot.is_some() || old_raw || block.raw_entity.is_some() {
                    let chunk=std::sync::Arc::make_mut(chunk);
                    let saved=std::sync::Arc::make_mut(chunk.saved.get_or_insert_with(||std::sync::Arc::new(horizon_client::save::ChunkExtras::default())));
                    saved.rolled_loot.retain(|roll|roll.pos!=p);
                    if let Some(roll)=block.rolled_loot.as_ref() {
                        let mut roll=roll.clone();roll.pos=p;saved.rolled_loot.push(roll);
                    }
                    saved.block_entities.retain(|tag|tag.get("x")!=Some(&Tag::Int(p.0)) || tag.get("y")!=Some(&Tag::Int(p.1)) || tag.get("z")!=Some(&Tag::Int(p.2)));
                    if let Some(Tag::Compound(raw))=block.raw_entity.as_ref() {
                        let mut raw=raw.clone();raw.insert("x".into(),Tag::Int(p.0));raw.insert("y".into(),Tag::Int(p.1));raw.insert("z".into(),Tag::Int(p.2));
                        saved.block_entities.push(Tag::Compound(raw));
                    }
                }
            }
            self.world.modified.insert((p.0 >> 4, p.2 >> 4));
        }
        let bounds = area(min, size);
        let keep = |tick: &ScheduledTick| {
            skip.contains(&[tick.pos.x - min.0, tick.pos.y - min.1, tick.pos.z - min.2])
        };
        let old_blocks = self
            .block_ticks
            .blocks
            .area_ticks(bounds)
            .into_iter()
            .filter(keep)
            .collect::<Vec<_>>();
        let old_fluids = self
            .block_ticks
            .fluids
            .area_ticks(bounds)
            .into_iter()
            .filter(keep)
            .collect::<Vec<_>>();
        self.block_ticks.blocks.clear_area(bounds);
        self.block_ticks.fluids.clear_area(bounds);
        for tick in old_blocks {
            self.block_ticks.blocks.schedule(tick);
        }
        for tick in old_fluids {
            self.block_ticks.fluids.schedule(tick);
        }
        for (ticks, fluid) in [(&plane.ticks, false), (&plane.fluid_ticks, true)] {
            for tick in ticks {
                let offset = [
                    tick.pos.x - from.0,
                    tick.pos.y - from.1,
                    tick.pos.z - from.2,
                ];
                if skip.contains(&offset) {
                    continue;
                }
                let mut tick = *tick;
                tick.pos = BlockPos::new(min.0 + offset[0], min.1 + offset[1], min.2 + offset[2]);
                if fluid {
                    self.block_ticks.fluids.schedule(tick);
                } else {
                    self.block_ticks.blocks.schedule(tick);
                }
            }
        }
        Ok(())
    }
    fn ae2_spatial_frame(&mut self, min: WorldPos, size: [i32; 3]) -> Result<(), String> {
        let frame = self.pack.assets.registry.default_state("ae2:matrix_frame")
            .ok_or("matrix frame is unavailable")?;
        let max = (min.0 + size[0], min.1 + size[1], min.2 + size[2]);
        let min = (min.0 - 1, min.1 - 1, min.2 - 1);
        // SpatialStorageHelper.transverseEdges: preserve the half-open ranges,
        // duplicate visits and axis order, including its omitted maximum edges.
        for p in transverse_edges(min, max) {
            // Source WrapInMatrixFrame uses Level.setBlockAndUpdate even over
            // non-air blocks. Normal removal drops real container contents and
            // sends neighbor/shape updates before the cached-plane exchange.
            self.level_set([p.0, p.1, p.2], frame).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    /// SpatialStorageHelper's post-exchange callbacks, after entity movement.
    /// These notify existing states; they must not place/copy those states again.
    fn ae2_spatial_post_updates(&mut self, source_dim: Dim, source: WorldPos,
        destination: WorldPos, size: [i32;3], skip: &HashSet<[i32;3]>) -> Result<(),String> {
        let back=self.dim;
        let result=(|| -> Result<(),String> {
            let mut skipped:Vec<_>=skip.iter().copied().collect();
            skipped.sort_by_key(|p|(p[0],p[2],p[1])); // CachedPlane.swap: x, z, y.
            // CachedPlane.markForUpdate appends the position and Direction.values
            // neighbors, retaining duplicates. Source applies the opposite plane's
            // update coordinates to each level; preserve that literal ordering.
            for (dim,origin) in [(source_dim,destination),(Dim::Spatial,source)] {
                self.enter_level(None,dim).map_err(|e|e.to_string())?;
                for offset in &skipped {for delta in [[0,0,0],[0,-1,0],[0,1,0],[0,0,-1],[0,0,1],[-1,0,0],[1,0,0]] {
                    let pos=BlockPos::new(origin.0+offset[0]+delta[0],origin.1+offset[1]+delta[1],origin.2+offset[2]+delta[2]);
                    let air=self.res.facts().air;
                    self.with_level(|b,level,ticks|b.update_neighbors_at(level,ticks,pos,b.block(air)));
                    self.process_level_events().map_err(|e|e.to_string())?;
                }}
            }
            // TriggerUpdates visits both outer boundaries, then both interiors.
            for (dim,origin,outer) in [(source_dim,source,true),(Dim::Spatial,destination,true),
                (source_dim,source,false),(Dim::Spatial,destination,false)] {
                self.enter_level(None,dim).map_err(|e|e.to_string())?;
                let min=if outer {(origin.0-1,origin.1-1,origin.2-1)}else{origin};
                let adjust=if outer {0}else{-1};
                let max=(origin.0+size[0]+adjust,origin.1+size[1]+adjust,origin.2+size[2]+adjust);
                for p in transverse_edges(min,max) {
                    let pos=BlockPos::new(p.0,p.1,p.2);
                    let state=self.world.block(p.0,p.1,p.2);
                    self.with_level(|b,level,ticks|b.handle_neighbor_changed(level,ticks,pos,b.block(state)));
                    self.process_level_events().map_err(|e|e.to_string())?;
                }
            }
            Ok(())
        })();
        let restored=self.enter_level(None,back).map_err(|e|e.to_string());
        result.and(restored)
    }
    pub(super) fn ae2_spatial_transition(&mut self, pos: WorldPos) -> Result<Value, String> {
        let dimension=self.dim;
        let previous=self.streaming.extra.clone();
        let result=self.ae2_spatial_transition_demanded(pos);
        // The operation visits another level, and may move a local player.
        // Release only its source-level demands without changing that result
        // or overwriting the destination's independent streaming tickets.
        if !self.ae2_restore_extra_columns(dimension,previous) {
            return Err("Spatial source level disappeared before releasing chunk demands".into());
        }
        result
    }
    fn ae2_spatial_transition_demanded(&mut self, pos: WorldPos) -> Result<Value, String> {
        if self.options.execution_role.is_client() {
            return Err("spatial transitions run on the authoritative server".into());
        }
        let mut cell = match self.world.block_entities.get(&pos) {
            Some(BlockEntity::Ae2(m))
                if m.kind == MachineKind::SpatialIoPort
                    && m.slots.get(1).is_some_and(Option::is_none) =>
            {
                m.slots
                    .first()
                    .and_then(Option::as_ref)
                    .cloned()
                    .ok_or("insert a spatial cell")?
            }
            _ => return Err("spatial port missing or output occupied".into()),
        };
        let max = capacity(&cell).ok_or("unsupported spatial cell")?;
        if cell.count != 1 {
            return Err("spatial slot holds exactly one cell".into());
        }
        let region = self.ae2_spatial_region(pos)?;
        if !region.fits(max) {
            return Err("region exceeds spatial cell capacity".into());
        }
        if self.ae2_take_power(pos, region.power as f64, true) < region.power as f64 * 0.999 {
            return Err("insufficient spatial transition energy".into());
        }
        let source_dim = self.dim;
        let source = (region.min.0 + 1, region.min.1 + 1, region.min.2 + 1);
        let mut plots = self.ae2_read_plots()?;
        let saved_id = spatial_plot_id(&cell);
        let mut plot = if let Some(p) = saved_id
            .and_then(|id| plots.plots.iter().find(|p| p.id == id))
            .cloned()
        {
            if p.size != region.size {
                return Err("formatted cell dimensions do not match this region".into());
            }
            p
        } else {
            let id = plots.next;
            plots.next = plots
                .next
                .checked_add(1)
                .ok_or("spatial plot IDs exhausted")?;
            Plot {
                id,
                size: region.size,
                last_dimension: source_dim.id().into(),
                last_min: world(region.min),
                last_max: world(region.max),
            }
        };
        let dst = world(ae2_links_core::plot_origin(plot.id).map_err(str::to_owned)?);
        // Load and inspect both complete regions before touching inventory,
        // power or blocks. Loaded destination chunks retain their entities.
        let entity_dimensions=self.ae2_spatial_type_dimensions()?;
        // CachedPlane constructs its columns with ServerLevel.getChunk: a
        // pylon volume can span columns outside the player's view distance.
        // Use the ordinary checked streamer, holding each reached source
        // column through both snapshots and the cross-level exchange.
        for (cx,cz) in keys(source,region.size) {self.ae2_demand_column(cx,cz)?;}
        self.ae2_spatial_snapshot(source, region.size)?;
        self.enter_level(None, Dim::Spatial)
            .map_err(|e| e.to_string())?;
        let destination = (|| {
            self.ae2_spatial_load_destination(dst, region.size)?;
            self.ae2_spatial_snapshot(dst, region.size)
        })();
        self.enter_level(None, source_dim)
            .map_err(|e| e.to_string())?;
        let dst_plane = destination?;
        if dst.1 - 1 < Dim::Spatial.min_y()
            || dst
                .1
                .checked_add(region.size[1])
                .is_none_or(|y| y > Dim::Spatial.max_y())
        {
            return Err("spatial frame exceeds world height".into());
        }
        if self
            .pack
            .assets
            .registry
            .default_state("ae2:matrix_frame")
            .is_none()
        {
            return Err("matrix frame is unavailable".into());
        }
        plot.last_dimension = source_dim.id().into();
        plot.last_min = world(region.min);
        plot.last_max = world(region.max);
        plots.plots.retain(|p| p.id != plot.id);
        plots.plots.push(plot.clone());
        self.ae2_write_plots(&plots)?;
        let network = self.ae2_network();
        let creative_source = network
            .analyze()
            .ok()
            .and_then(|analysis| {
                let status = analysis
                    .nodes
                    .get(&self.ae2_local_address(pos)?.network_pos())?;
                Some(
                    analysis.components[status.component]
                        .positions
                        .iter()
                        .any(|p| network.nodes[p].kind == DeviceKind::CreativeEnergyCell),
                )
            })
            .unwrap_or(false);
        let extracted = self.ae2_take_power(pos, region.power as f64, false);
        let charged_ae = extracted * horizon_client::ae2_common_config::gameplay().usage_multiplier;
        if extracted < region.power as f64 * 0.999 {
            if extracted > 0.0 && !creative_source {
                let _ = self.ae2_add_power(pos, charged_ae);
            }
            return Err("spatial energy changed before transition".into());
        }
        // Snapshot after charging: a battery inside the moved volume must not
        // recover its pre-transfer charge when its block entity is restored.
        let src = match self.ae2_spatial_snapshot(source, region.size) {
            Ok(p) => p,
            Err(e) => {
                if !creative_source {
                    let _ = self.ae2_add_power(pos, charged_ae);
                }
                return Err(e);
            }
        };
        let skip = src
            .blocks
            .iter()
            .chain(&dst_plane.blocks)
            .filter(|b| b.skip)
            .map(|b| b.offset)
            .collect::<HashSet<_>>();
        let src_entities = self.ae2_take_spatial_entities(source, region.size, &entity_dimensions);
        let src_guests = self
            .lan
            .as_ref()
            .map(|h| self.ae2_spatial_guest_transfers(h, &src_entities, Dim::Spatial, dst))
            .unwrap_or_default();
        let local = self.ae2_spatial_local_player_transfer(&src_entities, Dim::Spatial, dst);
        if let Err(e) = self.enter_level(None, Dim::Spatial) {
            self.ae2_restore_spatial_entities(src_entities, source);
            if !creative_source {
                let _ = self.ae2_add_power(pos, charged_ae);
            }
            return Err(e.to_string());
        }
        let dst_entities = self.ae2_take_spatial_entities(dst, region.size, &entity_dimensions);
        let dst_guests = self
            .lan
            .as_ref()
            .map(|h| self.ae2_spatial_guest_transfers(h, &dst_entities, source_dim, source))
            .unwrap_or_default();
        match self.ae2_spatial_frame(dst, region.size) {
            Ok(()) => (),
            Err(error) => {
                self.ae2_restore_spatial_entities(dst_entities, dst);
                if self.enter_level(None, source_dim).is_ok() {
                    self.ae2_restore_spatial_entities(src_entities, source);
                    if !creative_source {
                        let _ = self.ae2_add_power(pos, charged_ae);
                    }
                }
                return Err(error);
            }
        };
        let apply = (|| -> Result<(), String> {
            self.ae2_spatial_write(dst, region.size, source, &src, &skip, true)?;
            self.enter_level(None, source_dim)
                .map_err(|e| e.to_string())?;
            self.ae2_spatial_write(source, region.size, dst, &dst_plane, &skip, true)?;
            Ok(())
        })();
        if let Err(error) = apply {
            let none = HashSet::new();
            // Restore both snapshots, pending entity ownership and energy on
            // ordinary errors. This is not a crash-atomic transaction.
            if self.dim != Dim::Spatial {
                let _ = self.enter_level(None, Dim::Spatial);
            }
            if self.dim == Dim::Spatial {
                let _ = self.ae2_spatial_write(dst, region.size, dst, &dst_plane, &none, false);
                // The frame's normal removal/neighbor effects are already
                // committed before the swap, as in Source. Restoring its old
                // containers here would duplicate items that were dropped.
                self.ae2_restore_spatial_entities(dst_entities, dst);
            }
            if self.enter_level(None, source_dim).is_ok() {
                let _ = self.ae2_spatial_write(source, region.size, source, &src, &none, false);
                self.ae2_restore_spatial_entities(src_entities, source);
                if !creative_source {
                    let _ = self.ae2_add_power(pos, charged_ae);
                }
            }
            return Err(error);
        }
        // Both writes succeeded. The levels were pre-opened above, so these
        // level swaps only exchange already-resident state.
        self.enter_level(None, Dim::Spatial)
            .map_err(|e| e.to_string())?;
        self.ae2_restore_spatial_entities(src_entities, dst);
        self.enter_level(None, source_dim)
            .map_err(|e| e.to_string())?;
        self.ae2_restore_spatial_entities(dst_entities, source);
        let mut anchor_moves=Vec::new();
        for (plane,from_dim,from,to_dim,to) in [(&src,source_dim,source,Dim::Spatial,dst),(&dst_plane,Dim::Spatial,dst,source_dim,source)] {
            for block in &plane.blocks {
                if skip.contains(&block.offset)||!matches!(&block.entity,Some(BlockEntity::Ae2(m))if m.kind==MachineKind::SpatialAnchor){continue;}
                let old=(from.0+block.offset[0],from.1+block.offset[1],from.2+block.offset[2]);
                let new=(to.0+block.offset[0],to.1+block.offset[1],to.2+block.offset[2]);
                if let (Some(old),Some(new))=(self.ae2_dimension_address(from_dim,old),self.ae2_dimension_address(to_dim,new)){anchor_moves.push((old,new));}
            }
        }
        self.ae2_spatial_anchors_moved(&anchor_moves);
        set_spatial_plot(&mut cell, plot.id, plot.size);
        // The triggering port itself can be in the captured volume.
        let offset = [pos.0 - source.0, pos.1 - source.1, pos.2 - source.2];
        let moved_port =
            (0..3).all(|i| offset[i] >= 0 && offset[i] < region.size[i]) && !skip.contains(&offset);
        let port = if moved_port {
            self.enter_level(None, Dim::Spatial)
                .map_err(|e| e.to_string())?;
            (dst.0 + offset[0], dst.1 + offset[1], dst.2 + offset[2])
        } else {
            pos
        };
        if let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&port) {
            m.slots[0] = None;
            m.slots[1] = Some(cell);
            self.world.modified.insert((port.0 >> 4, port.2 >> 4));
        }
        self.enter_level(None, source_dim)
            .map_err(|e| e.to_string())?;
        self.ae2_queue_spatial_guests(src_guests);
        if let Some(mut host) = self.lan.take() {
            self.run_level_teleports(&mut host);
            self.lan = Some(host);
        }
        self.enter_level(None, Dim::Spatial)
            .map_err(|e| e.to_string())?;
        self.ae2_queue_spatial_guests(dst_guests);
        if let Some(mut host) = self.lan.take() {
            self.run_level_teleports(&mut host);
            self.lan = Some(host);
        }
        self.enter_level(None, source_dim)
            .map_err(|e| e.to_string())?;
        if let Some(local) = local {
            self.ae2_apply_spatial_local_player_transfer(local)
                .map_err(|e| e.to_string())?;
        }
        self.ae2_spatial_post_updates(source_dim,source,dst,region.size,&skip)?;
        Ok(
            json!({"plot":plot.id,"dimension":Dim::Spatial.id(),"origin":dst,"size":region.size,"power":region.power,"efficiency":region.efficiency}),
        )
    }
    pub(super) fn ae2_tick_spatial(&mut self) {
        let positions = self
            .world
            .block_entities
            .iter()
            .filter_map(|(&p, b)| {
                matches!(b,BlockEntity::Ae2(m)if m.kind==MachineKind::SpatialIoPort).then_some(p)
            })
            .collect::<Vec<_>>();
        for p in positions {
            let high = self
                .with_level(|b, l, t| b.has_neighbor_signal(l, t, BlockPos::new(p.0, p.1, p.2)));
            let prior = matches!(self.world.block_entities.get(&p),Some(BlockEntity::Ae2(m))if m.config.get(&"ae2_spatial_redstone".into())==Some(&Tag::Byte(1)));
            if let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&p) {
                m.config
                    .insert("ae2_spatial_redstone".into(), Tag::Byte(i8::from(high)));
                if high != prior {
                    self.world.modified.insert((p.0 >> 4, p.2 >> 4));
                }
            }
            if high && !prior {
                if let Err(e) = self.ae2_spatial_transition(p) {
                    log::debug!("AE2 spatial transition at {p:?}: {e}");
                }
            }
        }
    }
    pub(super) fn ae2_spatial_command(&mut self, words: &[&str]) -> Result<Value, String> {
        let p = (
            words
                .get(1)
                .ok_or("spatial action x y z")?
                .parse()
                .map_err(|_| "invalid x")?,
            words
                .get(2)
                .ok_or("missing y")?
                .parse()
                .map_err(|_| "invalid y")?,
            words
                .get(3)
                .ok_or("missing z")?
                .parse()
                .map_err(|_| "invalid z")?,
        );
        match words.first().copied() {
            Some("status") => {
                let r = self.ae2_spatial_region(p)?;
                Ok(
                    json!({"min":world(r.min),"max":world(r.max),"size":r.size,"power":r.power,"efficiency":r.efficiency}),
                )
            }
            Some("transition") => self.ae2_spatial_transition(p),
            _ => Err("spatial status|transition x y z".into()),
        }
    }
}

#[cfg(test)] mod frame_tests {
    use super::*;
    #[test] fn source_plot_component_preserves_legacy_data_and_survives_nbt() {
        let mut cell = ItemStack::new("ae2:spatial_storage_cell_2", 1);
        let legacy = Tag::compound([("ae2_spatial_plot", Tag::Int(7)), ("marker", Tag::Int(42))]);
        cell.data_mut().set_other("minecraft:custom_data", Some(legacy.clone()));
        assert_eq!(spatial_plot_id(&cell), Some(7));
        set_spatial_plot(&mut cell, 3, [2, 2, 2]);
        assert_eq!(spatial_plot_id(&cell), Some(3), "the source component takes precedence over the old wrapper");
        assert_eq!(cell.data().unwrap().other("minecraft:custom_data"), Some(&legacy));
        let saved = horizon_client::save::item_to_nbt(&cell);
        let bytes = horizon_world::nbt::write_network(&saved, Default::default()).unwrap();
        let (decoded, _) = horizon_world::nbt::read_network(&bytes, Default::default()).unwrap();
        let restored = horizon_client::save::item_from_nbt(&decoded).unwrap();
        assert_eq!(restored, cell);
        assert_eq!(spatial_plot_id(&restored), Some(3));
        assert_eq!(restored.data().unwrap().other("ae2:spatial_plot_info"), Some(&Tag::compound([
            ("id", Tag::Int(3)), ("size", Tag::IntArray(vec![2, 2, 2])),
        ])));
        cell.data_mut().set_other("ae2:spatial_plot_info", Some(Tag::compound([("id", Tag::Long(4_294_967_299))])));
        assert_eq!(spatial_plot_id(&cell), Some(3), "Number.intValue wraps a long's low bits");
        cell.data_mut().set_other("ae2:spatial_plot_info", Some(Tag::compound([("id", Tag::String("invalid".into()))])));
        assert_eq!(spatial_plot_id(&cell), None, "an invalid actual component does not fall back to stale data");
    }
    #[test] fn ordered_frame_visits_match_unmodified_java_helper() {
        let oracle: Value = serde_json::from_str(include_str!("ae2_spatial_frame_oracle.json")).unwrap();
        let expected: Vec<WorldPos> = oracle["ordered_visits"].as_array().unwrap().iter()
            .map(|p| (p[0].as_i64().unwrap() as i32, p[1].as_i64().unwrap() as i32, p[2].as_i64().unwrap() as i32)).collect();
        let actual = transverse_edges((63,99,63),(66,102,66));
        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 54);
        assert_eq!(actual.iter().collect::<BTreeSet<_>>().len(), 46);
        assert!(!actual.contains(&(66,102,66)));
    }
}
