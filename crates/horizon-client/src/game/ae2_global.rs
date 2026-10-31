//! Stable addresses for loaded AE2 grid nodes across dimensions.
//! The encoded Y namespace is internal to the AE2 graph and persisted jobs;
//! it is never sent as a Minecraft block coordinate.
use super::{Dim, Game};
use horizon_client::{containers::BlockEntity, world::World};
use horizon_world::registry::BlockStateId;

pub(super) type LocalPos = (i32, i32, i32);
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub(super) struct GridAddress(LocalPos);
impl GridAddress {
    pub(super) fn from_local(dim: Dim, pos: LocalPos) -> Option<Self> {
        encode(dim, pos)
    }
    pub(super) fn network_pos(self) -> ae2_network_core::Pos {
        ae2_network_core::Pos {
            x: self.0.0,
            y: self.0.1,
            z: self.0.2,
        }
    }
    pub(super) fn from_network(pos: ae2_network_core::Pos) -> Option<Self> {
        let a = Self((pos.x, pos.y, pos.z));
        decode(a)?;
        Some(a)
    }
    pub(super) fn local_parts(self) -> Option<(Dim, LocalPos)> {
        decode(self)
    }
    pub(super) fn encoded(self) -> LocalPos {
        self.0
    }
}
const STRIDE: i32 = 4096;
fn ordinal(dim: Dim) -> i32 {
    match dim {
        Dim::Overworld => 0,
        Dim::Nether => 1,
        Dim::End => 2,
        Dim::Spatial => 3,
    }
}
fn encode(dim: Dim, p: LocalPos) -> Option<GridAddress> {
    if !(-64..=319).contains(&p.1) {
        return None;
    }
    Some(GridAddress((
        p.0,
        p.1.checked_add(ordinal(dim) * STRIDE)?,
        p.2,
    )))
}
fn decode(address: GridAddress) -> Option<(Dim, LocalPos)> {
    let p = address.0;
    let n = p.1.checked_add(64)?.div_euclid(STRIDE);
    let dim = match n {
        0 => Dim::Overworld,
        1 => Dim::Nether,
        2 => Dim::End,
        3 => Dim::Spatial,
        _ => return None,
    };
    let y = p.1.checked_sub(n * STRIDE)?;
    if !(-64..=319).contains(&y) {
        return None;
    }
    Some((dim, (p.0, y, p.2)))
}
pub(super) struct BlockSnapshot {
    levels: Vec<(
    Dim,
    Vec<(LocalPos, BlockEntity)>,
    horizon_client::fast_hash::FxHashSet<(i32, i32)>,
)>,
    monitors: super::ae2_storage_monitor_host::StorageMonitors,
}
impl Game<'_> {
    pub(super) fn ae2_local_address(&self, p: LocalPos) -> Option<GridAddress> {
        encode(self.dim, p)
    }
    pub(super) fn ae2_dimension_address(&self, dim: Dim, p: LocalPos) -> Option<GridAddress> {
        encode(dim, p)
    }
    pub(super) fn ae2_address_parts(&self, p: GridAddress) -> Option<(Dim, LocalPos)> {
        decode(p)
    }
    pub(super) fn ae2_loaded_worlds(&self) -> Vec<(Dim, &World)> {
        let mut dims: Vec<_> = self.levels.ae2_dimensions().collect();
        dims.sort_by_key(|d| ordinal(*d));
        let mut out = vec![(self.dim, &self.world)];
        for dim in dims {
            if dim != self.dim {
                if let Some(world) = self.levels.ae2_world(dim) {
                    out.push((dim, world));
                }
            }
        }
        out
    }
    pub(super) fn ae2_world_for(&self, dim: Dim) -> Option<&World> {
        if dim == self.dim {
            Some(&self.world)
        } else {
            self.levels.ae2_world(dim)
        }
    }
    pub(super) fn ae2_world_for_mut(&mut self, dim: Dim) -> Option<&mut World> {
        if dim == self.dim {
            Some(&mut self.world)
        } else {
            self.levels.ae2_world_mut(dim)
        }
    }
    pub(super) fn ae2_block_entity(&self, p: GridAddress) -> Option<&BlockEntity> {
        let (dim, local) = decode(p)?;
        self.ae2_world_for(dim)?.block_entities.get(&local)
    }
    pub(super) fn ae2_block_entity_mut(&mut self, p: GridAddress) -> Option<&mut BlockEntity> {
        let (dim, local) = decode(p)?;
        self.ae2_world_for_mut(dim)?.block_entities.get_mut(&local)
    }
    pub(super) fn ae2_mark_modified(&mut self, p: GridAddress) -> bool {
        let Some((dim, local)) = decode(p) else {
            return false;
        };
        let Some(world) = self.ae2_world_for_mut(dim) else {
            return false;
        };
        world.modified.insert((local.0 >> 4, local.2 >> 4));
        true
    }
    pub(super) fn ae2_snapshot_blocks(&self) -> BlockSnapshot {
        let levels = self.ae2_loaded_worlds()
            .into_iter()
            .map(|(dim, world)| {
                (
                    dim,
                    world
                        .block_entities
                        .iter()
                        .map(|(p, be)| (*p, be.clone()))
                        .collect(),
                    world.modified.clone(),
                )
            })
            .collect();
        BlockSnapshot {levels, monitors:self.ae2_storage_monitors.borrow().clone()}
    }
    pub(super) fn ae2_restore_blocks(&mut self, snapshot: BlockSnapshot) {
        for (dim, entries, dirty) in snapshot.levels {
            if let Some(world) = self.ae2_world_for_mut(dim) {
                for (pos, be) in entries {
                    if let Some(current) = world.block_entities.get_mut(&pos) {
                        *current = be;
                    }
                }
                world.modified = dirty;
            }
        }
        *self.ae2_storage_monitors.borrow_mut()=snapshot.monitors;
    }
    pub(super) fn ae2_grid_block(&self, p: GridAddress) -> Option<BlockStateId> {
        let (dim, local) = decode(p)?;
        Some(self.ae2_world_for(dim)?.block(local.0, local.1, local.2))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_addresses_roundtrip_all_dimensions_and_minimum_y() {
        for dim in [Dim::Overworld, Dim::Nether, Dim::End, Dim::Spatial] {
            for y in [-64, 0, 319] {
                let p = (123, y, -456);
                assert_eq!(decode(encode(dim, p).unwrap()), Some((dim, p)));
            }
        }
        assert!(encode(Dim::End, (0, 320, 0)).is_none());
        assert!(decode(GridAddress((0, 512, 0))).is_none());
    }
}
