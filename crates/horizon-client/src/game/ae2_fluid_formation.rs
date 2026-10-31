//! AE2 19.2.18 FluidPlacementStrategy and per-physical-plane runtime.
//! Fluid custody is deferred in the same FormationPlan as item placement.
use super::{Game, ae2_formation::{FluidPreview, FormationAction, FormationPlan, PlaneOwner}};
use ae2_cell_core::ItemKey;
use ae2_network_core::NodeIdentity;
use horizon_client::{blocks::BlockFacts, containers::BlockEntity, world::World};
use horizon_core::position::BlockPos;
use horizon_world::{fluid::FluidKind, nbt::{self, Limits, Tag}};
use std::{collections::HashMap, time::{SystemTime, UNIX_EPOCH}};

const BUCKET_MB: u64 = 1000;

fn empty_fluid_patch(bytes: &[u8]) -> bool {
    if bytes.is_empty() { return true; }
    matches!(nbt::read_network(bytes, Limits::default()), Ok((Tag::Compound(map), used)) if map.is_empty() && used == bytes.len())
}

#[derive(Clone, Default)]
struct PlaneState {
    // Distinct Fluid registry entries, including flowing/source of one family.
    blocked: u8,
    item_blocked: bool,
    last_effect_ms: u64,
}

#[derive(Clone, Default)]
pub(super) struct FluidFormationRuntime {
    planes: HashMap<NodeIdentity, PlaneState>,
}

impl FluidFormationRuntime {
    fn blocked(&self, identity: NodeIdentity, fluid: FluidKind) -> bool {
        self.planes.get(&identity).is_some_and(|entry| entry.blocked & fluid.mask() != 0)
    }
    fn record_blocked(&mut self, identity: NodeIdentity, fluid: FluidKind) {
        self.planes.entry(identity).or_default().blocked |= fluid.mask();
    }
    fn clear_blocked(&mut self, identity: NodeIdentity) {
        if let Some(entry) = self.planes.get_mut(&identity) { entry.blocked = 0; }
    }
    fn item_blocked(&self, identity: NodeIdentity) -> bool {
        self.planes.get(&identity).is_some_and(|entry| entry.item_blocked)
    }
    fn set_item_blocked(&mut self, identity: NodeIdentity, blocked: bool) {
        self.planes.entry(identity).or_default().item_blocked = blocked;
    }
    fn effect_due(&self, identity: NodeIdentity, now: u64) -> bool {
        let previous = self.planes.get(&identity).map_or(0, |entry| entry.last_effect_ms);
        now >= previous.saturating_add(250)
    }
    fn effect_played(&mut self, identity: NodeIdentity, now: u64) {
        self.planes.entry(identity).or_default().last_effect_ms = now;
    }
}

/// Source FormationPlanePart clears only the part facing the changed neighbor,
/// for both neighborChanged and updateShape; a same-state shape update counts.
pub(super) fn clear_facing(world: &World, facts: &BlockFacts, runtime: &mut FluidFormationRuntime, pos: BlockPos, neighbor: BlockPos) {
    let face = match (neighbor.x - pos.x, neighbor.y - pos.y, neighbor.z - pos.z) {
        (1, 0, 0) => "east", (-1, 0, 0) => "west", (0, 1, 0) => "up", (0, -1, 0) => "down",
        (0, 0, 1) => "south", (0, 0, -1) => "north", _ => return,
    };
    let key = (pos.x, pos.y, pos.z);
    let Some(host) = world.block_entities.incarnation(&key) else { return };
    let Some(BlockEntity::Ae2(machine)) = world.block_entities.get(&key) else { return };
    let Some(Tag::List(parts)) = machine.config.get(&"parts".into()) else { return };
    for part in parts {
        if part.get("id") != Some(&Tag::String("ae2:formation_plane".into()))
            || part.get("face") != Some(&Tag::String(face.into())) { continue; }
        let Some(part_id) = super::ae2_channels::part_identity(part) else { continue };
        let identity=NodeIdentity { host, part: Some(part_id), outer: false };
        runtime.clear_blocked(identity);
        let target=world.block(neighbor.x,neighbor.y,neighbor.z);
        runtime.set_item_blocked(identity,!facts.get(target).replaceable);
    }
}

impl Game<'_> {
    pub(super) fn ae2_fluid_cached_blocked(&self, plane: PlaneOwner, fluid: FluidKind) -> bool {
        self.ae2_formation_fluid_runtime.blocked(plane.identity, fluid)
    }
    pub(super) fn ae2_fluid_record_blocked(&mut self, plane: PlaneOwner, fluid: FluidKind) {
        self.ae2_formation_fluid_runtime.record_blocked(plane.identity, fluid);
    }
    pub(super) fn ae2_item_cached_blocked(&self, plane: PlaneOwner) -> bool {
        self.ae2_formation_fluid_runtime.item_blocked(plane.identity)
    }
    pub(super) fn ae2_item_update_blocked(&mut self, plane: PlaneOwner, pos: (i32,i32,i32)) {
        let state=self.world.block(pos.0,pos.1,pos.2);
        self.ae2_formation_fluid_runtime.set_item_blocked(plane.identity,!self.res.facts().get(state).replaceable);
    }
    pub(super) fn ae2_can_place_formation_fluid(&mut self, pos: (i32, i32, i32), fluid: FluidKind) -> bool {
        let pos = BlockPos::new(pos.0, pos.1, pos.2);
        self.with_level(|b, level, ticks| b.formation_fluid_can_place(level, ticks, pos, fluid))
    }
    pub(super) fn ae2_place_formation_fluid(&mut self, pos: (i32, i32, i32), fluid: FluidKind, plane: PlaneOwner) -> Result<(), String> {
        let at = BlockPos::new(pos.0, pos.1, pos.2);
        if !self.with_level(|b, level, ticks| b.formation_fluid_place(level, ticks, at, fluid)) {
            return Err("formation fluid placement failed".into());
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(u128::from(u64::MAX)) as u64;
        if self.ae2_formation_fluid_runtime.effect_due(plane.identity, now) {
            self.ae2_formation_fluid_runtime.effect_played(plane.identity, now);
            self.with_level(|b, level, ticks| b.formation_fluid_effect(level, ticks, at, fluid));
        }
        Ok(())
    }
    pub(super) fn ae2_form_fluids(&mut self, anchor: super::ae2_global::GridAddress, key: &ItemKey, limit: u64, priority: i32, plan: &mut FormationPlan) -> Result<u64, String> {
        if limit < BUCKET_MB || !empty_fluid_patch(&key.components) { return Ok(0); }
        // Legacy cells may retain an encoded empty compound. Its source patch
        // is empty, so match it against a canonical unpatched fluid filter.
        let normalized = (!key.components.is_empty()).then(|| ItemKey { item_id:key.item_id.clone(), components:Vec::new() });
        let key=normalized.as_ref().unwrap_or(key);
        let Some(fluid) = FluidKind::from_name(&key.item_id).filter(|fluid| *fluid != FluidKind::Empty) else { return Ok(0) };
        let mut placed = 0;
        for target in self.ae2_formation_targets(anchor, key, true, priority) {
            if limit - placed < BUCKET_MB { break; }
            if target.entity { continue; }
            match self.ae2_preview_fluid_formation(plan, &target, fluid)? {
                FluidPreview::CanPlace(old) => {
                    plan.actions.push(FormationAction::Fluid { dim: self.dim, pos: target.target, old, fluid, plane: target.owner });
                    placed += BUCKET_MB;
                }
                FluidPreview::CannotPlace => self.ae2_fluid_record_blocked(target.owner, fluid),
                FluidPreview::Blocked => {},
            }
        }
        Ok(placed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocked_fluids_and_effect_throttle_belong_to_one_physical_plane() {
        let first = NodeIdentity {host: 7, part: Some([1,2,3,4]), outer: false};
        let replaced = NodeIdentity {host: 7, part: Some([1,2,3,5]), outer: false};
        let mut runtime = FluidFormationRuntime::default();
        runtime.record_blocked(first, FluidKind::Water);
        runtime.set_item_blocked(first,true);
        assert!(runtime.blocked(first, FluidKind::Water));
        assert!(!runtime.blocked(first, FluidKind::FlowingWater));
        assert!(!runtime.blocked(replaced, FluidKind::Water));
        assert!(runtime.item_blocked(first));
        assert!(!runtime.item_blocked(replaced));
        assert!(runtime.effect_due(first, 1000));
        runtime.effect_played(first, 1000);
        assert!(!runtime.effect_due(first, 1249));
        assert!(runtime.effect_due(first, 1250));
        assert!(runtime.effect_due(replaced, 1001));
        runtime.clear_blocked(first);
        runtime.set_item_blocked(first,false);
        assert!(!runtime.blocked(first, FluidKind::Water));
        assert!(!runtime.item_blocked(first));
        assert!(!runtime.effect_due(first, 1249), "neighbor updates clear blockage, not effect timing");
    }
    #[test]
    fn an_encoded_empty_fluid_patch_is_unpatched_but_nonempty_and_invalid_are_not() {
        let empty=nbt::write_network(&Tag::Compound(Default::default()),Limits::default()).unwrap();
        assert!(empty_fluid_patch(&[]));
        assert!(empty_fluid_patch(&empty));
        let nonempty=nbt::write_network(&Tag::compound([("test:value",Tag::Int(1))]),Limits::default()).unwrap();
        assert!(!empty_fluid_patch(&nonempty));
        assert!(!empty_fluid_patch(&[1,2,3]));
        let mut trailing=empty;trailing.push(0);
        assert!(!empty_fluid_patch(&trailing));
    }
}
