// Derived from AE2 19.2.18 SpatialAnchorBlockEntity, LGPL-3.0-or-later.
//! Owner-ticket transitions. The host supplies actual grid statistics and a
//! controller that accepts/releases a ticket for this particular anchor.
use std::collections::BTreeSet;
pub type Chunk = (i32, i32);
pub trait Tickets {
    fn force(&mut self, chunk: Chunk) -> bool;
    fn release(&mut self, chunk: Chunk) -> bool;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modulation {
    Same,
    Sleep,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Anchor {
    chunks: BTreeSet<Chunk>,
    powerless_ticks: u32,
    initialized: bool,
    removed: bool,
    idle: f64,
}
impl Default for Anchor {
    fn default() -> Self {
        Self {
            chunks: BTreeSet::new(),
            powerless_ticks: 0,
            initialized: false,
            removed: false,
            idle: 1.0,
        }
    }
}
impl Anchor {
    pub fn chunks(&self) -> &BTreeSet<Chunk> {
        &self.chunks
    }
    pub fn idle(&self) -> f64 {
        self.idle
    }
    pub fn powerless_ticks(&self) -> u32 {
        self.powerless_ticks
    }
    fn update_power(&mut self) {
        if !self.removed {
            let n = self.chunks.len() as i32;
            self.idle = f64::from(80i32.wrapping_add(n.wrapping_mul(n.wrapping_add(1)) / 2));
        }
    }
    /// Restored ticket ownership, not a request to force a new chunk.
    pub fn register_chunk(&mut self, chunk: Chunk) {
        self.chunks.insert(chunk);
        self.update_power();
    }
    pub fn force(&mut self, chunk: Chunk, tickets: &mut impl Tickets) -> bool {
        if self.removed {
            return false;
        }
        let forced = tickets.force(chunk);
        if forced && self.chunks.insert(chunk) {
            self.update_power();
        }
        forced
    }
    pub fn release(&mut self, chunk: Chunk, remove: bool, tickets: &mut impl Tickets) -> bool {
        let released = tickets.release(chunk);
        if released && remove && self.chunks.remove(&chunk) {
            self.update_power();
        }
        released
    }
    pub fn release_all(&mut self, tickets: &mut impl Tickets) {
        for chunk in self.chunks.clone() {
            self.release(chunk, false, tickets);
        }
        self.chunks.clear();
    }
    pub fn remove(&mut self, tickets: &mut impl Tickets) {
        self.removed = true;
        self.release_all(tickets);
    }
    pub fn clean_up(&mut self, required: Option<&BTreeSet<Chunk>>, tickets: &mut impl Tickets) {
        let Some(required) = required else { return };
        for chunk in self.chunks.clone() {
            if !required.contains(&chunk) {
                self.release(chunk, false, tickets);
                self.chunks.remove(&chunk);
            }
        }
        for &chunk in required {
            if !self.chunks.contains(&chunk) {
                self.force(chunk, tickets);
            }
        }
    }
    /// Mirrors tickingRequest's ordering, including its strictly-greater-than
    /// grace check and accumulated (not reset by an online tick) counter.
    pub fn tick(
        &mut self,
        booted: bool,
        online: bool,
        elapsed: u32,
        required: Option<&BTreeSet<Chunk>>,
        tickets: &mut impl Tickets,
    ) -> Modulation {
        if self.removed {
            return Modulation::Sleep;
        }
        if !self.initialized {
            if !booted {
                return Modulation::Same;
            }
            self.initialized = true;
        }
        self.clean_up(required, tickets);
        if self.powerless_ticks > 200 {
            if !online {
                self.release_all(tickets);
            }
            self.powerless_ticks = 0;
            return Modulation::Sleep;
        }
        if !online {
            self.powerless_ticks = self.powerless_ticks.saturating_add(elapsed);
            return Modulation::Same;
        }
        Modulation::Sleep
    }
    /// Source move strategy creates a new anchor, then forces a temporary 9x9
    /// area before waiting for the destination grid to finish pathing.
    pub fn done_moving(&mut self, center: Chunk, tickets: &mut impl Tickets) {
        self.initialized = false;
        for x in center.0 - 4..=center.0 + 4 {
            for z in center.1 - 4..=center.1 + 4 {
                self.force((x, z), tickets);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Sink {
        held: BTreeSet<Chunk>,
        denied: BTreeSet<Chunk>,
    }
    impl Tickets for Sink {
        fn force(&mut self, c: Chunk) -> bool {
            !self.denied.contains(&c) && self.held.insert(c)
        }
        fn release(&mut self, c: Chunk) -> bool {
            self.held.remove(&c)
        }
    }
    #[test]
    fn waits_for_boot_and_counts_only_successful_tickets() {
        let mut a = Anchor::default();
        let mut s = Sink::default();
        let required = BTreeSet::from([(0, 0), (1, 0)]);
        s.denied.insert((1, 0));
        assert_eq!(
            a.tick(false, true, 20, Some(&required), &mut s),
            Modulation::Same
        );
        assert_eq!(a.idle(), 1.0);
        assert!(s.held.is_empty());
        assert_eq!(
            a.tick(true, true, 20, Some(&required), &mut s),
            Modulation::Sleep
        );
        assert_eq!(a.chunks(), &BTreeSet::from([(0, 0)]));
        assert_eq!(a.idle(), 81.0);
        s.denied.clear();
        a.clean_up(Some(&required), &mut s);
        assert_eq!(a.idle(), 83.0);
    }
    #[test]
    fn grace_is_checked_before_increment_and_online_sleep_keeps_accumulation() {
        let mut a = Anchor::default();
        let mut s = Sink::default();
        let required = BTreeSet::from([(0, 0)]);
        a.tick(true, false, 20, Some(&required), &mut s);
        a.tick(true, false, 20, Some(&required), &mut s);
        assert_eq!(
            a.tick(true, true, 20, Some(&required), &mut s),
            Modulation::Sleep
        );
        assert_eq!(a.powerless_ticks(), 40);
        for _ in 0..9 {
            assert_eq!(
                a.tick(true, false, 20, Some(&required), &mut s),
                Modulation::Same
            );
        }
        assert_eq!(a.powerless_ticks(), 220);
        assert!(!s.held.is_empty());
        assert_eq!(
            a.tick(true, false, 20, Some(&required), &mut s),
            Modulation::Sleep
        );
        assert!(s.held.is_empty());
        assert_eq!(a.powerless_ticks(), 0);
    }
    #[test]
    fn cleanup_and_release_all_preserve_source_last_idle_until_a_power_update() {
        let mut a = Anchor::default();
        let mut s = Sink::default();
        a.force((0, 0), &mut s);
        a.force((1, 0), &mut s);
        assert_eq!(a.idle(), 83.0);
        a.clean_up(Some(&BTreeSet::from([(0, 0)])), &mut s);
        assert_eq!(a.chunks().len(), 1);
        assert_eq!(a.idle(), 83.0);
        a.release_all(&mut s);
        assert!(a.chunks().is_empty());
        assert_eq!(a.idle(), 83.0);
        a.force((0, 0), &mut s);
        assert_eq!(a.idle(), 81.0);
    }
    #[test]
    fn restored_and_temporary_ownership_wait_for_real_destination_boot() {
        let mut a = Anchor::default();
        a.register_chunk((10, 10));
        assert_eq!(a.idle(), 81.0);
        let mut a = Anchor::default();
        let mut s = Sink::default();
        a.done_moving((-10, 20), &mut s);
        assert_eq!(a.chunks().len(), 81);
        assert_eq!(a.idle(), 3401.0);
        let needed = BTreeSet::from([(-10, 20)]);
        a.tick(false, true, 20, Some(&needed), &mut s);
        assert_eq!(s.held.len(), 81);
        a.tick(true, true, 20, Some(&needed), &mut s);
        assert_eq!(&s.held, &needed);
        assert_eq!(a.idle(), 3401.0);
        a.remove(&mut s);
        assert!(s.held.is_empty());
        assert!(!a.force((0, 0), &mut s));
    }
}
