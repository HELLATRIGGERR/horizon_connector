//! Isolated native block-placement attempts. A simulation must not flush item
//! spills, publish anchor callbacks, process level events, or export BE IDs.
use super::{Game, Dim};
use std::collections::HashMap;
use horizon_client::{world::World, random_sequences::RandomSequences, random_tick_sections::RandomTickSections};
use horizon_world::level::{Ticks, LevelEvent};

struct Saved {
    world: World,
    ticks: Ticks,
    events: Vec<LevelEvent>,
    sequences: RandomSequences,
    pending: super::spill::Pending,
    spill: super::spill::Spill,
    quantum: super::ae2_quantum::QuantumRuntime,
    detectors: HashMap<(Dim,(i32,i32,i32)),super::ae2_light_detector::DetectorState>,
    random_sections: RandomTickSections,
    preview: bool,
    import_error:Option<String>,
    fluid_runtime:super::ae2_fluid_formation::FluidFormationRuntime,
}
struct Guard<'a,'r> { game: &'a mut Game<'r>, saved: Option<Saved> }
impl<'a,'r> Guard<'a,'r> {
    fn new(game: &'a mut Game<'r>) -> Self {
        // Clone before replacing any state, so an allocation panic cannot
        // leave a partially installed sandbox.
        let world=game.world.placement_snapshot();
        let ticks=game.block_ticks.clone();
        let events=game.level_events.clone();
        let sequences=game.rand_sequences.clone();
        let pending=game.pending.clone();
        let spill=game.spill.clone();
        let quantum=game.ae2_quantum_runtime.get_mut().clone();
        let detectors=game.ae2_detectors.clone();
        let fluid_runtime=game.ae2_formation_fluid_runtime.clone();
        let saved=Saved {
            world:std::mem::replace(&mut game.world,world),
            ticks:std::mem::replace(&mut game.block_ticks,ticks),
            events:std::mem::replace(&mut game.level_events,events),
            sequences:std::mem::replace(&mut game.rand_sequences,sequences),
            pending:std::mem::replace(&mut game.pending,pending),
            spill:std::mem::replace(&mut game.spill,spill),
            quantum:std::mem::replace(game.ae2_quantum_runtime.get_mut(),quantum),
            detectors:std::mem::replace(&mut game.ae2_detectors,detectors),
            random_sections:std::mem::take(&mut game.random_tick_sections),
            preview:std::mem::replace(&mut game.ae2_formation_staging,true),
            import_error:std::mem::take(&mut game.ae2_formation_import_error),
            fluid_runtime:std::mem::replace(&mut game.ae2_formation_fluid_runtime,fluid_runtime),
        };
        Self {game,saved:Some(saved)}
    }
    fn commit(mut self) {
        let mut saved=self.saved.take().expect("placement transaction already finished");
        self.game.ae2_formation_staging=saved.preview;
        self.game.ae2_formation_import_error=std::mem::take(&mut saved.import_error);
        // The staged world and its actual newly created BE identities become
        // authoritative. Only the old snapshots are discarded.
        drop(saved);
    }

}
impl Drop for Guard<'_,'_> {
    fn drop(&mut self) {
        let Some(saved)=self.saved.take() else {return};
        self.game.world=saved.world;
        self.game.block_ticks=saved.ticks;
        self.game.level_events=saved.events;
        self.game.rand_sequences=saved.sequences;
        self.game.pending=saved.pending;
        self.game.spill=saved.spill;
        *self.game.ae2_quantum_runtime.get_mut()=saved.quantum;
        self.game.ae2_detectors=saved.detectors;
        self.game.random_tick_sections=saved.random_sections;
        self.game.ae2_formation_import_error=saved.import_error;
        self.game.ae2_formation_staging=saved.preview;
        self.game.ae2_formation_fluid_runtime=saved.fluid_runtime;
    }
}
impl Game<'_> {
    /// The result may contain prospective positions/states, but no simulated
    /// incarnation IDs or journal entries. Global monotonic ID gaps are benign;
    /// actual placement creates its own committed identities after validation.
    pub(super) fn ae2_with_placement_sandbox<R>(&mut self, f: impl FnOnce(&mut Self)->R) -> R {
        let mut guard=Guard::new(self);
        f(&mut *guard.game)
    }
    pub(super) fn ae2_with_placement_transaction<R>(&mut self, f:impl FnOnce(&mut Self)->Result<R,String>) -> Result<R,String> {
        let value={
            let mut guard=Guard::new(self);
            let value=f(&mut *guard.game)?;
            if let Some(error)=guard.game.ae2_formation_import_error.take() {return Err(error);}
            guard.commit();
            value
        };
        // No callbacks or item entities escaped before this point.
        self.flush_spill();
        if self.ae2_has_anchor_owners() {
            if let Err(error)=self.ae2_apply_anchor_callbacks() {
                log::debug!("AE2 placement anchor callbacks deferred: {error}");
            }
        }
        Ok(value)
    }

}
