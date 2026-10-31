//! AE2 19.2.18 hand crank and its sided 160 AE turn (LGPL-3.0-or-later).
use super::Game;
use horizon_client::{ae2_world::MachineKind, containers::BlockEntity};
use horizon_core::{geometry::Direction, position::BlockPos};
#[derive(Clone,Copy,Default)]
pub(super) struct CrankState {rotation:i32, charge:i32, hits:i32, pub angle:i32}

pub(super) fn direction_name(d: Direction) -> &'static str {
    match d {Direction::Down=>"down",Direction::Up=>"up",Direction::North=>"north",Direction::South=>"south",Direction::West=>"west",Direction::East=>"east"}
}
fn vector(d: &str) -> Option<(i32,i32,i32)> {
    Some(match d {"down"=>(0,-1,0),"up"=>(0,1,0),"north"=>(0,0,-1),"south"=>(0,0,1),"west"=>(-1,0,0),"east"=>(1,0,0),_=>return None})
}
pub(super) fn crankable(kind: MachineKind, front: &str, side: Direction) -> bool {
    let name=direction_name(side);
    match kind {
        MachineKind::Charger | MachineKind::Inscriber => name != front,
        MachineKind::GrowthAccelerator => vector(front).zip(vector(name)).is_some_and(|(a,b)| a==b || a==(-b.0,-b.1,-b.2)),
        _ => false,
    }
}
impl Game<'_> {
    fn crank_target(&self, pos:(i32,i32,i32)) -> Option<(i32,i32,i32)> {
        let state=self.pack.assets.registry.state(self.world.block(pos.0,pos.1,pos.2))?;
        let front=state.properties.get("facing")?;let d=vector(front)?;
        let target=(pos.0-d.0,pos.1-d.1,pos.2-d.2);
        let BlockEntity::Ae2(machine)=self.world.block_entities.get(&target)? else{return None};
        let target_state=self.pack.assets.registry.state(self.world.block(target.0,target.1,target.2))?;
        let target_front=target_state.properties.get("facing").map(String::as_str).unwrap_or("north");
        let side=match front.as_str(){"down"=>Direction::Down,"up"=>Direction::Up,"north"=>Direction::North,"south"=>Direction::South,"west"=>Direction::West,"east"=>Direction::East,_=>return None};
        crankable(machine.kind,target_front,side).then_some(target)
    }
    /// `useWithoutItem` always consumes the click for a physical crank.
    pub(super) fn ae2_try_crank_use(&mut self, pos:(i32,i32,i32)) -> bool {
        let Some(BlockEntity::Ae2(m))=self.world.block_entities.get(&pos) else{return false};
        if m.kind!=MachineKind::Crank{return false}
        let Some(target)=self.crank_target(pos) else{return true};
        let can_turn=matches!(self.world.block_entities.get(&target),Some(BlockEntity::Ae2(m)) if m.energy.is_finite() && m.energy<1600.0);
        let state=self.ae2_cranks.entry((self.dim,pos)).or_default();
        let mut break_crank=false;
        if state.rotation<3 {
            if can_turn {state.rotation+=18;state.hits=0;}
            else {state.hits+=1;break_crank=state.hits>10;}
        }
        if break_crank {self.ae2_cranks.remove(&(self.dim,pos));self.with_level(|b,l,t|b.destroy_block(l,t,BlockPos::new(pos.0,pos.1,pos.2),false));}
        true
    }
    /// Ticks the real block entity; one completed turn injects up to 160 AE.
    pub(super) fn ae2_tick_cranks(&mut self) {
        let dim=self.dim;let entities=&self.world.block_entities;
        self.ae2_cranks.retain(|(d,p),_| *d!=dim || matches!(entities.get(p),Some(BlockEntity::Ae2(m)) if m.kind==MachineKind::Crank));
        let cranks:Vec<_>=self.ae2_cranks.iter().filter_map(|((dim,p),s)|(*dim==self.dim && s.rotation>0).then_some(*p)).collect();
        for pos in cranks {
            if !matches!(self.world.block_entities.get(&pos),Some(BlockEntity::Ae2(m)) if m.kind==MachineKind::Crank) {self.ae2_cranks.remove(&(self.dim,pos));continue;}
            let target=self.crank_target(pos);
            let Some(state)=self.ae2_cranks.get_mut(&(self.dim,pos)) else{continue};
            state.rotation-=1;state.charge+=1;state.angle=(state.angle-20).rem_euclid(360);
            let completed=state.charge>=18;if completed {state.charge-=18;}
            if completed {if let Some(target)=target {if let Some(BlockEntity::Ae2(m))=self.world.block_entities.get_mut(&target) {if m.energy.is_finite(){m.private_energy_insert(160.0,true);self.world.modified.insert((target.0>>4,target.2>>4));}}}}
        }
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn source_capability_sides() {
        assert!(!crankable(MachineKind::Charger,"north",Direction::North));
        assert!(crankable(MachineKind::Charger,"north",Direction::Up));
        assert!(crankable(MachineKind::GrowthAccelerator,"east",Direction::West));
        assert!(!crankable(MachineKind::GrowthAccelerator,"east",Direction::Up));
        assert!(!crankable(MachineKind::Drive,"north",Direction::Up));
    }
}
