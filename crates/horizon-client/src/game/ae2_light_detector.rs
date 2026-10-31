//! AE2 19.2.18 light detector polling and weak redstone signal (LGPL-3.0-or-later).
use super::Game;
use horizon_client::{ae2_world::MachineKind,containers::BlockEntity};
use horizon_core::position::BlockPos;
use horizon_world::level::LevelAccess;

#[derive(Clone,Copy)]
pub(super) struct DetectorState { pub last_light:i32, ticks:i32 }
impl Default for DetectorState {fn default()->Self{Self{last_light:0,ticks:30}}}
pub(super) fn brightness(packed:u8,darken:i32)->i32 {
    i32::from(packed&15).max((i32::from(packed>>4)-darken).max(0))
}
pub(super) fn weak_signal(last:i32,current:i32)->i32 { if last>0 {(current-6).clamp(0,15)}else{0} }
impl Game<'_> {
    pub(super) fn ae2_tick_light_detectors(&mut self) {
        let dim=self.dim;let entities=&self.world.block_entities;
        self.ae2_detectors.retain(|(d,p),_|*d!=dim||matches!(entities.get(p),Some(BlockEntity::Ae2(m)) if m.kind==MachineKind::LightDetector));
        let positions:Vec<_>=entities.iter().filter_map(|(p,e)|matches!(e,BlockEntity::Ae2(m) if m.kind==MachineKind::LightDetector).then_some(*p)).collect();
        let darken=self.sky_darken();let mut changed=Vec::new();
        for p in positions {
            let value=brightness(self.world.light(p.0,p.1,p.2),darken);
            let state=self.ae2_detectors.entry((dim,p)).or_default();state.ticks+=1;
            if state.ticks>30 {state.ticks=0;if state.last_light!=value{state.last_light=value;changed.push(p);}}
        }
        for p in changed {
            let pos=BlockPos::new(p.0,p.1,p.2);
            self.with_level(|b,l,t|{let state=l.block_state(pos);b.update_neighbors_at(l,t,pos,b.block(state));});
        }
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn source_brightness_and_weak_signal() {
        assert_eq!(brightness(0xf2,4),11);
        assert_eq!(brightness(0x29,4),9);
        assert_eq!(weak_signal(0,15),0);
        assert_eq!(weak_signal(15,15),9);
        assert_eq!(weak_signal(2,2),0);
    }
}
