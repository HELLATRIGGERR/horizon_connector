//! Deferred Formation Plane writes and item-entity placement. Based on AE2
//! ItemPlacementStrategy (LGPL-3.0-or-later).
use super::Dim;
use horizon_client::items::ItemStack;
use horizon_core::geometry::{Aabb, Vec3};
use horizon_world::registry::BlockStateId;

#[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord)]
pub(super) struct PlaneOwner {
    pub address: super::ae2_global::GridAddress,
    pub identity: ae2_network_core::NodeIdentity,
    pub face: u8,
}
impl PlaneOwner {
    pub fn direction(self) -> horizon_core::geometry::Direction {
        use horizon_core::geometry::Direction::*;
        match self.face {0=>East,1=>West,2=>Up,3=>Down,4=>South,_=>North}
    }
}
#[derive(Clone,Copy,Debug)]
pub(super) struct PlaneTarget {
    pub owner:PlaneOwner,
    pub host:(i32,i32,i32),
    pub target:(i32,i32,i32),
    pub entity:bool,
    pub player_owner:Option<[i32;4]>,
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(super) enum FluidPreview {Blocked,CannotPlace,CanPlace(BlockStateId)}

pub(super) enum FormationAction {
    /// Optimistic source SIMULATE reservation; must never be committed.
    Reservation {dim:Dim,pos:(i32,i32,i32)},
    Place {dim:Dim,pos:(i32,i32,i32),old:BlockStateId,stack:ItemStack,plane:PlaneOwner,player_owner:Option<[i32;4]>},
    Fluid {dim:Dim,pos:(i32,i32,i32),old:BlockStateId,fluid:horizon_world::fluid::FluidKind,plane:PlaneOwner},
    /// Container writes have already been staged under the enclosing block
    /// snapshot; notify comparators only once that transaction commits.
    ContainerChanged { address: super::ae2_global::GridAddress, incarnation: u64 },
    Entity {
        dim: Dim, target: (i32, i32, i32), old: BlockStateId,
        position: [f64; 3], direction: [i32; 3], stack: ItemStack, plane:PlaneOwner,
    },
}
#[derive(Default)]
pub(super) struct FormationPlan { pub(super) actions: Vec<FormationAction> }
impl FormationPlan {
    pub(super) fn len(&self) -> usize { self.actions.len() }
    pub(super) fn truncate(&mut self, n: usize) { self.actions.truncate(n); }
    pub(super) fn block_at(&self, dim: Dim, pos: (i32, i32, i32)) -> bool {
        self.actions.iter().any(|action| matches!(action, FormationAction::Place {dim:d,pos:p,..} | FormationAction::Fluid {dim:d,pos:p,..} | FormationAction::Reservation {dim:d,pos:p} if *d == dim && *p == pos))
    }
    pub(super) fn entities_in(&self, dim: Dim, bounds: Aabb) -> usize {
        self.actions.iter().filter(|action| matches!(action, FormationAction::Entity { dim: d, position, .. }
            if *d == dim && bounds.intersects(entity_bounds(*position)))).count()
    }
}
pub(super) fn area(pos: (i32, i32, i32)) -> Aabb {
    Aabb::new(Vec3::new(f64::from(pos.0)-8.0, f64::from(pos.1)-8.0, f64::from(pos.2)-8.0),
              Vec3::new(f64::from(pos.0)+9.0, f64::from(pos.1)+9.0, f64::from(pos.2)+9.0))
}
fn entity_bounds(pos: [f64; 3]) -> Aabb {
    Aabb::new(Vec3::new(pos[0]-0.125, pos[1], pos[2]-0.125),
              Vec3::new(pos[0]+0.125, pos[1]+0.25, pos[2]+0.125))
}
pub(super) fn entity_intersects(pos: [f64; 3], area: Aabb) -> bool { area.intersects(entity_bounds(pos)) }
pub(super) fn accepted(count: usize, limit: i32, requested: u64, max_stack: u32) -> u32 {
    if limit <= 0 || count >= limit as usize { return 0; }
    requested.min(u64::from(max_stack)) as u32
}
pub(super) fn max_stack(stack: &ItemStack, item_default: u32) -> u32 {
    horizon_client::items::stack_max_with_default(stack,item_default)
}
pub(super) fn spawn_position(host: (i32, i32, i32), direction: [i32; 3], random: [f32; 3]) -> [f64; 3] {
    let horizontal = |i: usize| if direction[i] == 0 { f64::from(random[i]) * 0.75 - 0.375 }
        else { f64::from(direction[i]) * 0.65 };
    [f64::from(host.0) + 0.5 + horizontal(0),
     f64::from(host.1) + if direction[1] == 0 { f64::from(random[1]) * 0.75 }
         else if direction[1] < 0 { -0.25 } else { 1.0 },
     f64::from(host.2) + 0.5 + horizontal(2)]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn container_notifications_follow_the_transaction_savepoint() {
        let first=super::super::ae2_global::GridAddress::from_local(Dim::Overworld,(1,64,1)).unwrap();
        let second=super::super::ae2_global::GridAddress::from_local(Dim::Overworld,(2,64,1)).unwrap();
        let mut plan=FormationPlan::default();
        plan.actions.push(FormationAction::ContainerChanged {address:first,incarnation:41});
        let savepoint=plan.len();
        plan.actions.push(FormationAction::ContainerChanged {address:second,incarnation:42});
        plan.truncate(savepoint);
        assert!(matches!(plan.actions.as_slice(),
            [FormationAction::ContainerChanged {address,incarnation:41}] if *address==first));
        plan.truncate(0);
        assert!(plan.actions.is_empty());
    }
    #[test]
    fn formation_entity_capacity_bounds_and_preview_reservations() {
        assert_eq!(accepted(127,128,100,64),64);
        assert_eq!(accepted(128,128,1,64),0);
        assert_eq!(accepted(0,0,1,64),0);
        assert_eq!(accepted(0,-1,1,64),0);
        assert_eq!(accepted(0,128,9,1),1);
        for dir in [[1,0,0],[-1,0,0],[0,1,0],[0,-1,0],[0,0,1],[0,0,-1]] {
            let p = spawn_position((0,0,0),dir,[0.0,0.5,1.0]);
            let target=(dir[0],dir[1],dir[2]);
            assert!(area(target).intersects(entity_bounds(p)));
            if dir[1] == -1 { assert_eq!(p[1], -0.25); }
        }
        let mut plan=FormationPlan::default();
        plan.actions.push(FormationAction::Entity { dim:Dim::Overworld, target:(1,0,0), old:BlockStateId(0), position:[1.15,0.0,0.5], direction:[1,0,0], plane:PlaneOwner{address:super::super::ae2_global::GridAddress::from_local(Dim::Overworld,(0,0,0)).unwrap(),identity:ae2_network_core::NodeIdentity{host:1,part:Some([1,0,0,0]),outer:false},face:0}, stack:ItemStack::new("minecraft:diamond",1) });
        assert_eq!(plan.entities_in(Dim::Overworld,area((1,0,0))),1);
        assert_eq!(plan.len(),1);
        plan.truncate(0);
        assert_eq!(plan.entities_in(Dim::Overworld,area((1,0,0))),0);
    }
}
