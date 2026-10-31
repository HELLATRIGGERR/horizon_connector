//! AE2 ServerBlockEntityRepo first-tick chunk queues. The source uses
//! fastutil Long2ObjectOpenHashMap; iteration order affects node connection
//! creation and therefore grid-service ownership.
use crate::NodeIdentity;
use std::collections::BTreeMap;

fn chunk_key(x: i32, z: i32) -> u64 {
    u64::from(x as u32) | (u64::from(z as u32) << 32)
}
fn coordinates(key: u64) -> (i32, i32) {
    (key as u32 as i32, (key >> 32) as u32 as i32)
}
fn mix(mut value: u64) -> u64 {
    value = value.wrapping_mul(0x9E3779B97F4A7C15);
    value ^= value >> 32;
    value ^ (value >> 16)
}
fn max_fill(n: usize) -> usize {
    ((n * 3).div_ceil(4)).min(n - 1)
}
#[derive(Clone, Debug)]
struct ChunkQueue {
    slots: Vec<Option<u64>>,
    values: BTreeMap<u64, Vec<NodeIdentity>>,
}
impl Default for ChunkQueue {
    fn default() -> Self {
        Self {
            slots: vec![None; 32],
            values: BTreeMap::new(),
        }
    }
}
impl ChunkQueue {
    fn put(&mut self, key: u64, node: NodeIdentity) {
        if let Some(items) = self.values.get_mut(&key) {
            items.push(node);
            return;
        }
        if key != 0 {
            let mut at = (mix(key) as usize) & (self.slots.len() - 1);
            while self.slots[at].is_some() {
                at = (at + 1) & (self.slots.len() - 1)
            }
            self.slots[at] = Some(key);
        }
        self.values.insert(key, vec![node]);
        if self.values.len() > max_fill(self.slots.len()) {
            self.rehash(self.slots.len() * 2);
        }
    }
    fn rehash(&mut self, size: usize) {
        let old = std::mem::replace(&mut self.slots, vec![None; size]);
        for key in old.into_iter().rev().flatten() {
            let mut at = (mix(key) as usize) & (size - 1);
            while self.slots[at].is_some() {
                at = (at + 1) & (size - 1)
            }
            self.slots[at] = Some(key);
        }
    }
    fn ordered_keys(&self) -> Vec<u64> {
        let mut out = Vec::with_capacity(self.values.len());
        if self.values.contains_key(&0) {
            out.push(0)
        }
        out.extend(self.slots.iter().rev().flatten().copied());
        out
    }
    fn remove(&mut self, key: u64) -> Option<Vec<NodeIdentity>> {
        let value = self.values.remove(&key)?;
        if key != 0 {
            let mask = self.slots.len() - 1;
            let mut hole = (mix(key) as usize) & mask;
            while self.slots[hole] != Some(key) {
                hole = (hole + 1) & mask;
            }
            loop {
                let mut scan = (hole + 1) & mask;
                loop {
                    let Some(candidate) = self.slots[scan] else {
                        self.slots[hole] = None;
                        break;
                    };
                    let home = (mix(candidate) as usize) & mask;
                    if if hole <= scan {
                        hole >= home || home > scan
                    } else {
                        hole >= home && home > scan
                    } {
                        self.slots[hole] = Some(candidate);
                        hole = scan;
                        break;
                    }
                    scan = (scan + 1) & mask;
                }
                if self.slots[hole].is_none() {
                    break;
                }
            }
        }
        if self.slots.len() > 32 && self.values.len() < max_fill(self.slots.len()) / 4 {
            self.rehash(self.slots.len() / 2)
        }
        Some(value)
    }
}
/// One queue per dimension. Events in each chunk retain scheduleInit order;
/// The caller snapshots chunk keys, then takes and processes one chunk at a
/// time. Callbacks may enqueue work in a later chunk of the same snapshot.
#[derive(Clone, Debug, Default)]
pub struct ReadyQueue {
    levels: BTreeMap<i32, ChunkQueue>,
}
impl ReadyQueue {
    pub fn schedule(&mut self, dimension: i32, x: i32, z: i32, node: NodeIdentity) {
        self.levels
            .entry(dimension)
            .or_default()
            .put(chunk_key(x, z), node);
    }
    pub fn unload_chunk(&mut self, dimension: i32, x: i32, z: i32) {
        if let Some(level) = self.levels.get_mut(&dimension) {
            level.remove(chunk_key(x, z));
        }
    }
    pub fn unload_level(&mut self, dimension: i32) {
        self.levels.remove(&dimension);
    }
    pub fn snapshot_chunks(&self, dimension: i32) -> Vec<(i32, i32)> {
        self.levels.get(&dimension).map_or_else(Vec::new, |level| {
            level.ordered_keys().into_iter().map(coordinates).collect()
        })
    }
    /// The caller checks source chunk-ticking eligibility before invoking this.
    /// Removing one chunk before its callbacks lets newly scheduled work for
    /// that same chunk wait for the next tick, exactly as upstream does.
    pub fn take_chunk(
        &mut self,
        dimension: i32,
        x: i32,
        z: i32,
        mut live: impl FnMut(NodeIdentity) -> bool,
    ) -> Vec<NodeIdentity> {
        self.levels
            .get_mut(&dimension)
            .and_then(|level| level.remove(chunk_key(x, z)))
            .unwrap_or_default()
            .into_iter()
            .filter(|node| live(*node))
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn node(n: u64) -> NodeIdentity {
        NodeIdentity {
            host: n,
            part: None,
            outer: false,
        }
    }
    #[test]
    fn chunk_iteration_matches_real_fastutil_after_rehash_and_deletes() {
        // Recorded from Java's actual Long2ObjectOpenHashMap 8.5.12 using
        // the same sequence, including zero, collisions and automatic shrink.
        let expected: Vec<Vec<i64>> =
            serde_json::from_str(include_str!("../tests/fixtures/fastutil-8.5.12-order.json"))
                .unwrap();
        let key = |i: i32| chunk_key(i * 17 - 300, i * 31 - 200);
        let mut queue = ChunkQueue::default();
        queue.put(0, node(0));
        for i in 0..70 {
            queue.put(key(i), node(i as u64));
        }
        assert_eq!(
            queue.ordered_keys(),
            expected[0].iter().map(|n| *n as u64).collect::<Vec<_>>()
        );
        for i in 0..70 {
            if i % 3 != 0 {
                queue.remove(key(i));
            }
        }
        assert_eq!(
            queue.ordered_keys(),
            expected[1].iter().map(|n| *n as u64).collect::<Vec<_>>()
        );
        for i in 70..115 {
            queue.put(key(i), node(i as u64));
        }
        queue.remove(0);
        assert_eq!(
            queue.ordered_keys(),
            expected[2].iter().map(|n| *n as u64).collect::<Vec<_>>()
        );
    }
    #[test]
    fn readiness_skips_unticking_chunks_and_removed_incarnations() {
        let mut q = ReadyQueue::default();
        q.schedule(0, 0, 0, node(1));
        q.schedule(0, 0, 0, node(2));
        q.schedule(0, 1, 0, node(3));
        q.schedule(1, 0, 0, node(4));
        let keys = q.snapshot_chunks(0);
        let first = keys
            .into_iter()
            .filter(|(x, _)| *x == 0)
            .flat_map(|(x, z)| q.take_chunk(0, x, z, |n| n != node(1)))
            .collect::<Vec<_>>();
        assert_eq!(first, vec![node(2)]);
        assert_eq!(q.take_chunk(0, 1, 0, |_| true), vec![node(3)]);
        assert_eq!(q.take_chunk(1, 0, 0, |_| true), vec![node(4)]);
    }
    #[test]
    fn callbacks_can_add_to_a_later_snapshotted_chunk_but_not_current_chunk() {
        let mut q = ReadyQueue::default();
        q.schedule(0, 0, 0, node(1));
        q.schedule(0, 1, 0, node(2));
        let keys = q.snapshot_chunks(0);
        assert_eq!(keys.len(), 2);
        let first = keys[0];
        let later = keys[1];
        let first_nodes = q.take_chunk(0, first.0, first.1, |_| true);
        assert_eq!(first_nodes.len(), 1);
        q.schedule(0, first.0, first.1, node(3));
        q.schedule(0, later.0, later.1, node(4));
        let later_nodes = q.take_chunk(0, later.0, later.1, |_| true);
        assert_eq!(later_nodes.len(), 2);
        assert!(later_nodes.contains(&node(4)));
        assert_eq!(q.take_chunk(0, first.0, first.1, |_| true), vec![node(3)]);
    }
}
