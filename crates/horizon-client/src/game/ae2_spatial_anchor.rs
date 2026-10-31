//! AE2 Spatial Anchor owner tickets. Native ticket persistence is separate
//! from vanilla /forceload; the transition rules live in ae2-links-core.
use super::{Dim, Game, ae2_global::GridAddress};
use ae2_links_core::anchor::{Anchor, Chunk, Modulation, Tickets};
use horizon_client::{Result, ae2_world::MachineKind, containers::BlockEntity};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
struct Saved {
    version: u8,
    anchors: Vec<SavedAnchor>,
}
#[derive(Serialize, Deserialize)]
struct SavedAnchor {
    address: GridAddress,
    chunks: BTreeSet<Chunk>,
}
struct Owner {
    state: Anchor,
    incarnation: Option<u64>,
    next_poll: u64,
    last_poll: u64,
    awake: bool,
    last_online: Option<bool>,
    last_required: Option<BTreeSet<Chunk>>,
}
impl Owner {
    fn new(now: u64) -> Self {
        Self {
            state: Anchor::default(),
            incarnation: None,
            next_poll: now.saturating_add(20),
            last_poll: now,
            awake: true,
            last_online: None,
            last_required: None,
        }
    }
}
#[derive(Default)]
pub(super) struct Service {
    loaded_for: Option<PathBuf>,
    owners: BTreeMap<GridAddress, Owner>,
    dirty: bool,
}
pub(super) struct Snapshot {
    pub idle: f64,
    pub owned_chunks: BTreeSet<Chunk>,
    pub overlay: bool,
    pub anchor_max_by_dimension: Vec<(Dim, usize)>,
    pub grid_chunks_by_dimension: Vec<(Dim, BTreeSet<Chunk>)>,
}
fn valid_chunk(c: Chunk) -> bool {
    (-1_875_000..1_875_000).contains(&c.0) && (-1_875_000..1_875_000).contains(&c.1)
}
struct Controller {
    held: BTreeSet<Chunk>,
}
impl Tickets for Controller {
    fn force(&mut self, c: Chunk) -> bool {
        valid_chunk(c) && self.held.insert(c)
    }
    fn release(&mut self, c: Chunk) -> bool {
        self.held.remove(&c)
    }
}
impl Service {
    fn load(&mut self, root: &Path) -> Result<bool> {
        if self.loaded_for.as_deref() == Some(root) {
            return Ok(false);
        }
        let file = root.join("horizon-ae2-anchor-tickets.json");
        let saved = match std::fs::read(&file) {
            Ok(bytes) => {
                if bytes.len() > 64 * 1024 * 1024 {
                    return Err("AE2 anchor tickets exceed NBT-equivalent read bound".into());
                }
                serde_json::from_slice::<Saved>(&bytes)?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Saved {
                version: 1,
                anchors: Vec::new(),
            },
            Err(e) => return Err(e.into()),
        };
        if saved.version != 1 {
            return Err("unsupported AE2 anchor ticket version".into());
        }
        let mut owners = BTreeMap::new();
        for entry in saved.anchors {
            if GridAddress::from_network(entry.address.network_pos()).is_none()
                || entry.chunks.iter().any(|c| !valid_chunk(*c))
            {
                return Err("invalid saved AE2 anchor owner or chunk".into());
            }
            let mut owner = Owner::new(0);
            for chunk in entry.chunks {
                owner.state.register_chunk(chunk);
            }
            if owners.insert(entry.address, owner).is_some() {
                return Err("duplicate AE2 anchor owner".into());
            }
        }
        *self = Self {
            loaded_for: Some(root.to_path_buf()),
            owners,
            dirty: false,
        };
        Ok(true)
    }
    fn save(&mut self, root: &Path) -> Result<()> {
        if !self.dirty || self.loaded_for.as_deref() != Some(root) {
            return Ok(());
        }
        let saved = Saved {
            version: 1,
            anchors: self
                .owners
                .iter()
                .filter(|(_, o)| !o.state.chunks().is_empty())
                .map(|(address, o)| SavedAnchor {
                    address: *address,
                    chunks: o.state.chunks().clone(),
                })
                .collect(),
        };
        let file = root.join("horizon-ae2-anchor-tickets.json");
        let temp = file.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(&saved)?)?;
        std::fs::rename(temp, file)?;
        self.dirty = false;
        Ok(())
    }
}
impl Game<'_> {
    pub(super) fn ae2_has_anchor_owners(&self)->bool {!self.ae2_anchors.owners.is_empty()}
    /// Visual synchronization reads only the physical setting and held owner
    /// tickets. It must not reconstruct the grid for every Anchor every frame.
    pub(super) fn ae2_anchor_overlay_snapshot(
        &self,
        address: GridAddress,
    ) -> Option<(bool, BTreeSet<Chunk>)> {
        let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(address) else {
            return None;
        };
        if machine.kind != MachineKind::SpatialAnchor {
            return None;
        }
        let visible = matches!(machine.config.get(&"overlay_mode".into()),Some(horizon_world::nbt::Tag::String(value))if value.to_string_lossy()=="YES");
        let chunks = self
            .ae2_anchors
            .owners
            .get(&address)
            .map_or_else(BTreeSet::new, |o| o.state.chunks().clone());
        Some((visible, chunks))
    }
    fn ae2_anchor_node_owners(
        &self,
        network: &ae2_network_core::Network,
    ) -> BTreeMap<ae2_network_core::Pos, GridAddress> {
        let mut hosts = BTreeMap::new();
        for (dim, world) in self.ae2_loaded_worlds() {
            for (&p, be) in world.block_entities.iter() {
                if matches!(be, BlockEntity::Ae2(_))
                    && let (Some(id), Some(address)) = (
                        world.block_entities.incarnation(&p),
                        GridAddress::from_local(dim, p),
                    )
                {
                    hosts.insert(id, address);
                }
            }
        }
        network
            .nodes
            .keys()
            .filter_map(|pos| {
                let address = network
                    .node_identities
                    .get(pos)
                    .and_then(|id| hosts.get(&id.host))
                    .copied()
                    .or_else(|| {
                        GridAddress::from_network(*pos).filter(|a| {
                            matches!(self.ae2_block_entity(*a), Some(BlockEntity::Ae2(_)))
                        })
                    })?;
                Some((*pos, address))
            })
            .collect()
    }
    /// SpatialAnchorMenu statistics: per-dimension maxima for anchor tickets,
    /// and distinct physical grid chunks for total network size.
    pub(super) fn ae2_anchor_snapshot(&self, address: GridAddress) -> Option<Snapshot> {
        let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(address) else {
            return None;
        };
        if machine.kind != MachineKind::SpatialAnchor {
            return None;
        }
        let network = self.ae2_network();
        let analysis = network.analyze().ok()?;
        let status = analysis.nodes.get(&address.network_pos())?;
        let physical = self.ae2_anchor_node_owners(&network);
        let mut maxima = std::collections::HashMap::<Dim, usize>::new();
        let mut chunks = std::collections::HashMap::<Dim, BTreeSet<Chunk>>::new();
        for node in &analysis.components[status.component].positions {
            let Some(a) = physical.get(node).copied() else {
                continue;
            };
            let Some((dim, _)) = self.ae2_address_parts(a) else {
                continue;
            };
            let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity(a) else {
                continue;
            };
            if m.kind == MachineKind::SpatialAnchor {
                let count = self
                    .ae2_anchors
                    .owners
                    .get(&a)
                    .map_or(0, |o| o.state.chunks().len());
                maxima
                    .entry(dim)
                    .and_modify(|v| *v = (*v).max(count))
                    .or_insert(count);
            }
        }
        let anchor_id=*network.node_identities.get(&address.network_pos())?;
        for dim in [Dim::Overworld,Dim::Nether,Dim::End,Dim::Spatial] {
            let Some(occupied)=self.ae2_ordered_anchor_chunks(anchor_id,dim).ok()? else {continue};
            if !occupied.is_empty(){chunks.insert(dim,occupied);}
        }
        let mut anchor_max_by_dimension = maxima.into_iter().collect::<Vec<_>>();
        anchor_max_by_dimension.sort_by_key(|v| v.0.id());
        let mut grid_chunks_by_dimension = chunks.into_iter().collect::<Vec<_>>();
        grid_chunks_by_dimension.sort_by_key(|v| v.0.id());
        Some(Snapshot {
            idle: self.ae2_anchor_idle(address),
            owned_chunks: self
                .ae2_anchors
                .owners
                .get(&address)
                .map_or_else(BTreeSet::new, |o| o.state.chunks().clone()),
            overlay: matches!(machine.config.get(&"overlay_mode".into()),Some(horizon_world::nbt::Tag::String(value))if value.to_string_lossy()=="YES"),
            anchor_max_by_dimension,
            grid_chunks_by_dimension,
        })
    }
    pub(super) fn ae2_anchor_command(
        &mut self,
        words: &[&str],
    ) -> std::result::Result<serde_json::Value, String> {
        self.ae2_apply_anchor_callbacks()?;
        if words.len() != 4 || words[0] != "status" {
            return Err("anchor status x y z".into());
        }
        let p = (
            words[1].parse::<i32>().map_err(|_| "invalid x")?,
            words[2].parse::<i32>().map_err(|_| "invalid y")?,
            words[3].parse::<i32>().map_err(|_| "invalid z")?,
        );
        if !matches!(self.world.block_entities.get(&p),Some(BlockEntity::Ae2(m))if m.kind==MachineKind::SpatialAnchor)
        {
            return Err("no physical Spatial Anchor here".into());
        }
        Ok(self.ae2_anchor_status(self.ae2_local_address(p).ok_or("invalid anchor position")?))
    }
    pub(super) fn ae2_anchor_idle(&self, address: GridAddress) -> f64 {
        self.ae2_anchors
            .owners
            .get(&address)
            .map_or(1.0, |o| o.state.idle())
    }
    pub(super) fn ae2_anchor_status(&self, address: GridAddress) -> serde_json::Value {
        match self.ae2_anchors.owners.get(&address) {
            Some(o) => {
                serde_json::json!({"chunks":o.state.chunks(),"count":o.state.chunks().len(),"idle":o.state.idle(),"powerless_ticks":o.state.powerless_ticks(),"awake":o.awake,"server_tick":self.ae2_server_tick(),"running_tick":self.ae2_server_tick(),"last_poll":o.last_poll,"next_poll":o.next_poll})
            }
            None => {
                serde_json::json!({"chunks":[],"count":0,"idle":1.0,"powerless_ticks":0,"awake":false,"server_tick":self.ae2_server_tick(),"running_tick":self.ae2_server_tick()})
            }
        }
    }
    pub(super) fn ae2_restore_anchor_tickets(&mut self) -> Result<()> {
        if self.ae2_anchors.load(&self.world_root)? {
            let dimensions = self
                .ae2_anchors
                .owners
                .keys()
                .filter_map(|a| self.ae2_address_parts(*a).map(|v| v.0))
                .collect::<HashSet<_>>();
            for dim in dimensions {
                self.ae2_open_anchor_level(dim)?;
            }
            self.ae2_sync_anchor_tickets();
        }
        Ok(())
    }
    /// Apply the observed StatisticsService owner callbacks in their actual
    /// membership order. The replay staged them before removing their physical
    /// identities, and the mutable boundary owns ticket publication.
    pub(super) fn ae2_apply_anchor_callbacks(&mut self) -> std::result::Result<(),String> {
        let callbacks=self.ae2_take_anchor_callbacks()?;
        if callbacks.is_empty(){return Ok(())}
        let now=self.ae2_server_tick();
        let mut changed=false;
        for callback in callbacks {
            let (id,address,chunk,kind)=match callback {
                super::ae2_anchor_observer::AnchorCallback::Removed{owner,address}=>{
                    if self.ae2_anchors.owners.get(&address).is_some_and(|entry|entry.incarnation==Some(owner.host)) {
                        if let Some(mut owner)=self.ae2_anchors.owners.remove(&address) {
                            let mut controller=Controller{held:owner.state.chunks().clone()};
                            owner.state.remove(&mut controller);
                            changed=true;
                        }
                    }
                    continue;
                }
                super::ae2_anchor_observer::AnchorCallback::Chunk{owner,address,chunk,kind}=>(owner,address,chunk,kind),
            };
            let Some((dim,pos))=self.ae2_address_parts(address) else{continue};
            let Some(world)=self.ae2_world_for(dim) else{continue};
            if world.block_entities.incarnation(&pos)!=Some(id.host)
                || !matches!(world.block_entities.get(&pos),Some(BlockEntity::Ae2(m)) if m.kind==MachineKind::SpatialAnchor) {continue}
            let owner=self.ae2_anchors.owners.entry(address).or_insert_with(||Owner::new(now));
            if owner.incarnation.is_some_and(|host|host!=id.host){continue}
            owner.incarnation=Some(id.host);
            let before=owner.state.chunks().clone();
            let mut controller=Controller{held:before.clone()};
            match kind {
                ae2_links_core::anchor_grid::Kind::Added=>{owner.state.force(chunk,&mut controller);}
                ae2_links_core::anchor_grid::Kind::Removed=>{
                    owner.state.release(chunk,true,&mut controller);
                    owner.awake=true;
                    owner.next_poll=owner.next_poll.min(owner.last_poll.saturating_add(1));
                }
            }
            changed|=&before!=owner.state.chunks();
        }
        if changed {self.ae2_anchors.dirty=true;self.ae2_sync_anchor_tickets();}
        Ok(())
    }
    fn ae2_sync_anchor_tickets(&mut self) {
        for dim in [Dim::Overworld, Dim::Nether, Dim::End, Dim::Spatial] {
            let chunks = self
                .ae2_anchors
                .owners
                .iter()
                .filter(|(a, _)| self.ae2_address_parts(**a).is_some_and(|v| v.0 == dim))
                .flat_map(|(_, o)| o.state.chunks().iter().copied())
                .collect();
            self.ae2_set_owned_chunks(dim, chunks);
        }
    }
    /// Removal and replacement must release tickets even while world time is frozen.
    pub(super) fn ae2_validate_anchor_owners(&mut self) -> Result<()> {
        let now = self.ae2_server_tick();
        let mut stale = Vec::new();
        let mut discovered = Vec::new();
        for (dim, world) in self.ae2_loaded_worlds() {
            for (&pos, be) in world.block_entities.iter() {
                if matches!(be,BlockEntity::Ae2(m)if m.kind==MachineKind::SpatialAnchor)
                    && let Some(address) = GridAddress::from_local(dim, pos)
                {
                    discovered.push((address, world.block_entities.incarnation(&pos), now));
                }
            }
        }
        for (&address, owner) in &self.ae2_anchors.owners {
            let Some((dim, pos)) = self.ae2_address_parts(address) else {
                stale.push(address);
                continue;
            };
            let Some(world) = self.ae2_world_for(dim) else {
                continue;
            };
            if !world.chunks.contains_key(&(pos.0 >> 4, pos.2 >> 4)) {
                if owner.state.chunks().is_empty() {
                    stale.push(address);
                }
                continue;
            }
            let valid = matches!(world.block_entities.get(&pos),Some(BlockEntity::Ae2(m))if m.kind==MachineKind::SpatialAnchor);
            let replaced = owner.incarnation.is_some()
                && owner.incarnation != world.block_entities.incarnation(&pos);
            if !valid || replaced {
                stale.push(address);
            }
        }
        for address in stale {
            if let Some(mut owner) = self.ae2_anchors.owners.remove(&address) {
                let mut controller = Controller {
                    held: owner.state.chunks().clone(),
                };
                owner.state.remove(&mut controller);
                self.ae2_anchors.dirty = true;
            }
        }
        for (address, incarnation, now) in discovered {
            let owner = self
                .ae2_anchors
                .owners
                .entry(address)
                .or_insert_with(|| Owner::new(now));
            if owner.incarnation.is_none() {
                owner.incarnation = incarnation;
                owner.last_poll = now;
                owner.next_poll = now.saturating_add(20);
            }
        }
        self.ae2_sync_anchor_tickets();
        self.ae2_anchors.save(&self.world_root)
    }
    pub(super) fn ae2_tick_anchors(&mut self) {
        if let Err(error)=self.ae2_apply_anchor_callbacks(){log::warn!("AE2 anchor events unavailable: {error}");return;}
        let now = self.ae2_server_tick();
        let owners = self
            .world
            .block_entities
            .iter()
            .filter_map(|(&pos, be)| {
                matches!(be,BlockEntity::Ae2(m)if m.kind==MachineKind::SpatialAnchor).then_some(pos)
            })
            .collect::<Vec<_>>();
        if owners.is_empty() {
            return;
        }
        let network = self.ae2_network();
        let Ok(analysis) = network.analyze() else {
            return;
        };
        for pos in owners {
            let Some(address) = self.ae2_local_address(pos) else {
                continue;
            };
            let node = address.network_pos();
            let status = analysis.nodes.get(&node);
            let online = status.is_some_and(|s| s.online);
            let booted = network.node_identities.get(&node).is_some_and(|id| {
                network
                    .committed_channels
                    .as_ref()
                    .is_none_or(|c| c.contains_identity(*id))
            });
            let required = network.node_identities.get(&node).and_then(|id|
                self.ae2_ordered_anchor_chunks(*id,self.dim).ok().flatten());
            let owner = self
                .ae2_anchors
                .owners
                .entry(address)
                .or_insert_with(|| Owner::new(now));
            owner.incarnation = self.world.block_entities.incarnation(&pos);
            if owner.last_online != Some(online) || owner.last_required != required {
                owner.awake = true;
                owner.next_poll = owner.next_poll.min(owner.last_poll.saturating_add(1));
                owner.last_online = Some(online);
                owner.last_required = required.clone();
            }
            if owner.awake && now >= owner.next_poll {
                let before = owner.state.chunks().clone();
                let mut controller = Controller {
                    held: before.clone(),
                };
                let modulation = owner.state.tick(
                    booted,
                    online,
                    now.saturating_sub(owner.last_poll).min(i32::MAX as u64) as u32,
                    required.as_ref(),
                    &mut controller,
                );
                owner.last_poll = now;
                owner.next_poll = now.saturating_add(20);
                owner.awake = modulation == Modulation::Same;
                if &before != owner.state.chunks() {
                    self.ae2_anchors.dirty = true;
                }
            }
        }
        self.ae2_sync_anchor_tickets();
    }
    pub(super) fn ae2_save_anchor_tickets(&mut self) -> Result<()> {
        self.ae2_anchors.save(&self.world_root)
    }
    /// Only called after both spatial block writes have committed. Ordinary
    /// rollback never releases the original owners or creates destination tickets.
    pub(super) fn ae2_spatial_anchors_moved(&mut self, moves: &[(GridAddress, GridAddress)]) {
        for (from, _) in moves {
            if let Some(mut owner) = self.ae2_anchors.owners.remove(from) {
                let mut controller = Controller {
                    held: owner.state.chunks().clone(),
                };
                owner.state.remove(&mut controller);
            }
        }
        for (_, to) in moves {
            let Some((_dim, pos)) = self.ae2_address_parts(*to) else {
                continue;
            };
            let now = self.ae2_server_tick();
            let mut owner = Owner::new(now);
            let mut controller = Controller {
                held: BTreeSet::new(),
            };
            owner
                .state
                .done_moving((pos.0 >> 4, pos.2 >> 4), &mut controller);
            self.ae2_anchors.owners.insert(*to, owner);
        }
        if !moves.is_empty() {
            self.ae2_anchors.dirty = true;
            self.ae2_sync_anchor_tickets();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn anchor_owner_tickets_persist_independently_and_recover_source_idle() {
        let root =
            std::env::temp_dir().join(format!("ae2-anchor-owner-save-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut service = Service::default();
        service.load(&root).unwrap();
        let address = GridAddress::from_local(Dim::Spatial, (192, 64, 192)).unwrap();
        let mut owner = Owner::new(20);
        let mut controller = Controller {
            held: BTreeSet::new(),
        };
        owner.state.force((12, 12), &mut controller);
        owner.state.force((13, 12), &mut controller);
        service.owners.insert(address, owner);
        service.dirty = true;
        service.save(&root).unwrap();
        let mut restored = Service::default();
        restored.load(&root).unwrap();
        let owner = &restored.owners[&address];
        assert_eq!(owner.state.chunks(), &BTreeSet::from([(12, 12), (13, 12)]));
        assert_eq!(owner.state.idle(), 83.0);
        assert_eq!(owner.state.powerless_ticks(), 0);
        assert!(owner.incarnation.is_none());
        assert!(!root.join("data/minecraft/chunk_tickets.dat").exists());
        let path = root.join("horizon-ae2-anchor-tickets.json");
        std::fs::write(&path, b"invalid saved ticket data").unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(Service::default().load(&root).is_err());
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}
