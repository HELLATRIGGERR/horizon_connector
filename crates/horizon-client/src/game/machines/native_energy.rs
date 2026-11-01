//! Forge Energy of a block for native server mods (docs/native-mods.md,
//! API 63): NeoForge's `IEnergyStorage` capability, asked for a block and one
//! of its sides. A mod block entity answers through the hook points
//! `native_fe_storage`, `native_fe_receive` and `native_fe_extract`; the game's
//! own machines answer through `energy_extras.rs` (AE2's energy acceptors,
//! P2P tunnels and debug generator in this fork), and AE2 reaches mod blocks
//! back through the same dispatcher. The helpers below are what a cable, a
//! generator or a machine calls. They name a level by its `dimension` and
//! reach any open one, not only the level in place, and act only where the
//! world is simulated.
//!
//! Amounts are FE as `i64`. A call is narrow, Java's `int`, unless it says
//! `wide` (Flux Networks' long FE): a narrow amount is clamped to `i32::MAX`
//! before any hook sees it, and a narrow readback saturates there. There are
//! no transactions: `simulate` asks what would happen and changes nothing,
//! as with 1.21.1's `IEnergyStorage`, which the ported mods speak.
//!
//! Re-entry: the block entity ticking now, or answering a receive or extract
//! now, is out of the world and answers nothing. A forwarder may call the
//! helpers from inside its own hooks; the same block, side and operation
//! asked again inside its own call answers nothing, and so does a chain 16
//! calls deep.
use super::Game;
use crate::game::{Dim, native_hooks::NativeFeInfo};
use horizon_client::{
    block_entities::Pos,
    containers::{BlockEntity, ModBlockEntity},
    world::World,
};
use horizon_core::geometry::Direction;
use std::cell::RefCell;

/// What a call does at a block; the re-entry guard tells them apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Info,
    Receive,
    Extract,
}

/// The deepest chain of FE calls (forwarders handing on to forwarders).
const DEPTH: usize = 16;

type Call = (Dim, [i32; 3], Option<Direction>, Op);

thread_local! {
    /// The FE calls under way on this thread, outermost first. The game
    /// simulating a world runs its server on one thread.
    static CALLS: RefCell<Vec<Call>> = const { RefCell::new(Vec::new()) };
}

/// A call under way; dropping it ends the call.
struct Entered;

impl Entered {
    /// Enters `call`: none when it is already under way (a loop) or the
    /// chain is `DEPTH` calls deep.
    fn enter(call: Call) -> Option<Self> {
        CALLS.with(|calls| {
            let mut calls = calls.borrow_mut();
            if calls.len() >= DEPTH || calls.contains(&call) {
                return None;
            }
            calls.push(call);
            Some(Self)
        })
    }
}

impl Drop for Entered {
    fn drop(&mut self) {
        CALLS.with(|calls| {
            calls.borrow_mut().pop();
        });
    }
}

/// The level named `dimension` (`minecraft:overworld`); none for one the
/// game doesn't have.
fn dim_of(dimension: &str) -> Option<Dim> {
    let dim = Dim::from_id(dimension);
    (dim.id() == dimension).then_some(dim)
}

/// The amount a hook is asked to move: a narrow one clamped to `i32::MAX`;
/// none when there is nothing to move.
fn request(amount: i64, wide: bool) -> Option<i64> {
    let amount = if wide {
        amount
    } else {
        amount.min(i64::from(i32::MAX))
    };
    (amount > 0).then_some(amount)
}

/// What the hooks answered, as the helper hands it on: within `0..=amount`,
/// no answer being 0.
fn settle(answer: Option<i64>, amount: i64) -> i64 {
    answer.unwrap_or(0).clamp(0, amount)
}

/// A readback as the caller reads it: `getEnergyStored` and
/// `getMaxEnergyStored` are `int`s unless the call is wide.
fn narrow(mut info: NativeFeInfo, wide: bool) -> NativeFeInfo {
    if !wide {
        info.stored = info.stored.min(i64::from(i32::MAX));
        info.capacity = info.capacity.min(i64::from(i32::MAX));
    }
    info
}

/// The mod block entity at `key`, taken out of `world` for a hook: an empty
/// id marks its place meanwhile, as during its tick. None for any other
/// block entity, and for one already out (ticking, or in a call of its own).
fn take(world: &mut World, key: Pos) -> Option<ModBlockEntity> {
    let Some(BlockEntity::Mod(m)) = world.block_entities.get_mut(&key) else {
        return None;
    };
    if m.id.is_empty() {
        return None;
    }
    Some(std::mem::take(&mut **m))
}

/// Puts `taken` back where `take` left its mark; with `changed`, its
/// `setChanged`: the next save writes its chunk. Whether it went back: a
/// block entity the call removed or replaced stays gone.
fn put_back(world: &mut World, key: Pos, taken: ModBlockEntity, changed: bool) -> bool {
    match world.block_entities.get_mut(&key) {
        Some(BlockEntity::Mod(m)) if m.id.is_empty() => {
            **m = taken;
            if changed {
                world.modified.insert((key.0 >> 4, key.2 >> 4));
            }
            true
        }
        _ => false,
    }
}

impl Game<'_> {
    /// The level `dim`: the one in place or a parked one.
    fn fe_world(&self, dim: Dim) -> Option<&World> {
        if dim == self.dim {
            Some(&self.world)
        } else {
            self.levels.parked_world(dim)
        }
    }

    fn fe_world_mut(&mut self, dim: Dim) -> Option<&mut World> {
        if dim == self.dim {
            Some(&mut self.world)
        } else {
            self.levels.parked_world_mut(dim)
        }
    }

    /// The FE readback of the block at `pos` in level `dim` through `face`:
    /// a mod block entity's (`native_fe_storage`), or a machine's of the
    /// game's own. A loop's readback is an empty one.
    pub(in crate::game) fn fe_info_at(
        &self,
        dim: Dim,
        pos: [i32; 3],
        face: Option<Direction>,
        wide: bool,
    ) -> Option<NativeFeInfo> {
        let Some(_call) = Entered::enter((dim, pos, face, Op::Info)) else {
            return Some(NativeFeInfo::default());
        };
        let key = (pos[0], pos[1], pos[2]);
        let info = match self.fe_world(dim)?.block_entities.get(&key)? {
            BlockEntity::Mod(m) => {
                if m.id.is_empty() {
                    return None;
                }
                let mut info = None;
                self.native_fe_storage(dim.id(), pos, face, m, wide, &mut info);
                info
            }
            _ => self.extra_fe_info(dim, pos, face),
        };
        info.map(|info| narrow(info, wide))
    }

    /// `receiveEnergy` (or with `extract`, `extractEnergy`) of `amount` FE
    /// on the block at `pos` in level `dim` through `face`: what moved.
    pub(in crate::game) fn fe_move_at(
        &mut self,
        dim: Dim,
        pos: [i32; 3],
        face: Option<Direction>,
        amount: i64,
        simulate: bool,
        wide: bool,
        extract: bool,
    ) -> i64 {
        let Some(amount) = request(amount, wide) else {
            return 0;
        };
        let op = if extract { Op::Extract } else { Op::Receive };
        let Some(_call) = Entered::enter((dim, pos, face, op)) else {
            return 0;
        };
        let key = (pos[0], pos[1], pos[2]);
        let found = self
            .fe_world(dim)
            .and_then(|world| world.block_entities.get(&key))
            .map(|entity| matches!(entity, BlockEntity::Mod(_)));
        match found {
            Some(true) => {}
            Some(false) => {
                let moved = self.extra_fe_move(dim, pos, face, amount, simulate, extract);
                return moved.clamp(0, amount);
            }
            None => return 0,
        }
        let Some(mut taken) = self.fe_world_mut(dim).and_then(|world| take(world, key)) else {
            return 0;
        };
        let items = taken.items.clone();
        let mut answer = None;
        if extract {
            self.native_fe_extract(dim.id(), pos, face, &mut taken, amount, simulate, wide, &mut answer);
        } else {
            self.native_fe_receive(dim.id(), pos, face, &mut taken, amount, simulate, wide, &mut answer);
        }
        let moved = settle(answer, amount);
        let items_moved = taken.items != items;
        let changed = (moved > 0 && !simulate) || items_moved;
        let back = self
            .fe_world_mut(dim)
            .is_some_and(|world| put_back(world, key, taken, changed));
        if back && items_moved && dim == self.dim {
            self.container_changed(key);
        }
        moved
    }
}

/// Forge Energy helpers for native mods. API 63.
#[allow(dead_code)]
impl Game<'_> {
    /// The Forge Energy of the block at `pos` in `dimension` through `face`
    /// (the side of that block you touch: a cable west of it asks with
    /// `West`; none is the block itself), as `IEnergyStorage` reads it: a mod
    /// block entity's, or one of the game's own machines'. None where there
    /// is no energy on that side, in a level that isn't open, on a client's
    /// copy of the world, and for the block entity ticking now. A loop
    /// (asked again inside its own readback) reads as empty. `wide`: read
    /// 64-bit amounts; otherwise they saturate at `i32::MAX`. API 63.
    pub(in crate::game) fn native_fe_info(
        &self,
        dimension: &str,
        pos: [i32; 3],
        face: Option<Direction>,
        wide: bool,
    ) -> Option<NativeFeInfo> {
        if !self.native_simulates() {
            return None;
        }
        self.fe_info_at(dim_of(dimension)?, pos, face, wide)
    }

    /// `IEnergyStorage.receiveEnergy` on the block at `pos` in `dimension`
    /// through `face`, as `native_fe_info` finds it: offers `amount` FE (at
    /// most `i32::MAX` unless `wide`) and answers what it took, 0 to
    /// `amount`; with `simulate`, what it would take. 0 where
    /// `native_fe_info` is none, and for a call already under way at that
    /// block and side (a loop). API 63.
    pub(in crate::game) fn native_fe_offer(
        &mut self,
        dimension: &str,
        pos: [i32; 3],
        face: Option<Direction>,
        amount: i64,
        simulate: bool,
        wide: bool,
    ) -> i64 {
        match dim_of(dimension) {
            Some(dim) if self.native_simulates() => {
                self.fe_move_at(dim, pos, face, amount, simulate, wide, false)
            }
            _ => 0,
        }
    }

    /// `IEnergyStorage.extractEnergy` on the block at `pos` in `dimension`
    /// through `face`, as `native_fe_offer`: draws up to `amount` FE and
    /// answers what it gave. API 63.
    pub(in crate::game) fn native_fe_draw(
        &mut self,
        dimension: &str,
        pos: [i32; 3],
        face: Option<Direction>,
        amount: i64,
        simulate: bool,
        wide: bool,
    ) -> i64 {
        match dim_of(dimension) {
            Some(dim) if self.native_simulates() => {
                self.fe_move_at(dim, pos, face, amount, simulate, wide, true)
            }
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::chunk::Chunk;
    use horizon_world::{nbt::Tag, registry::BlockStateId};
    use std::sync::Arc;

    #[test]
    fn narrow_calls_are_ints_and_nothing_moves_nothing() {
        assert_eq!(request(5, false), Some(5));
        assert_eq!(request(i64::MAX, false), Some(i64::from(i32::MAX)));
        assert_eq!(request(i64::MAX, true), Some(i64::MAX));
        assert_eq!(request(0, true), None);
        assert_eq!(request(-7, false), None);
        // A hook that doesn't answer moved nothing; one can't move more than
        // it was asked, nor less than nothing.
        assert_eq!(settle(None, 40), 0);
        assert_eq!(settle(Some(25), 40), 25);
        assert_eq!(settle(Some(41), 40), 40);
        assert_eq!(settle(Some(-3), 40), 0);
        let big = NativeFeInfo {
            stored: 5_000_000_000,
            capacity: i64::MAX,
            can_receive: true,
            can_extract: false,
        };
        let int = narrow(big, false);
        assert_eq!((int.stored, int.capacity), (2_147_483_647, 2_147_483_647));
        assert!(int.can_receive && !int.can_extract);
        assert_eq!(narrow(big, true), big);
        // AE2's P2P readbacks wrap like Java ints: a negative sum stays.
        let wrapped = NativeFeInfo {
            stored: -2,
            capacity: -2,
            ..Default::default()
        };
        assert_eq!(narrow(wrapped, false), wrapped);
    }

    #[test]
    fn levels_are_named_exactly() {
        assert_eq!(dim_of("minecraft:overworld"), Some(Dim::Overworld));
        assert_eq!(dim_of("minecraft:the_nether"), Some(Dim::Nether));
        assert_eq!(dim_of("minecraft:the_end"), Some(Dim::End));
        // `Dim::from_id` falls back to the overworld; the helpers don't.
        assert_eq!(dim_of("mymod:mining_dimension"), None);
        assert_eq!(dim_of("overworld"), None);
    }

    #[test]
    fn the_guard_refuses_a_loop_and_a_deep_chain() {
        let at = |x: i32, op| (Dim::Overworld, [x, 64, 0], Some(Direction::East), op);
        {
            let _outer = Entered::enter(at(0, Op::Receive)).expect("first call");
            assert!(Entered::enter(at(0, Op::Receive)).is_none(), "the same call again is a loop");
            // Another side, another operation or another block is no loop.
            assert!(Entered::enter((Dim::Overworld, [0, 64, 0], Some(Direction::West), Op::Receive)).is_some());
            assert!(Entered::enter(at(0, Op::Extract)).is_some());
            assert!(Entered::enter((Dim::Nether, [0, 64, 0], Some(Direction::East), Op::Receive)).is_some());
            let _inner = Entered::enter(at(1, Op::Receive)).expect("a neighbour");
            assert!(Entered::enter(at(1, Op::Receive)).is_none());
        }
        // Both ended: the call may be made again.
        assert!(Entered::enter(at(0, Op::Receive)).is_some());
        let chain: Vec<_> = (0..DEPTH as i32)
            .map(|x| Entered::enter(at(x, Op::Info)).expect("within the depth"))
            .collect();
        assert!(Entered::enter(at(99, Op::Info)).is_none(), "call 17 is refused");
        drop(chain);
        assert!(Entered::enter(at(99, Op::Info)).is_some());
        CALLS.with(|calls| assert!(calls.borrow().is_empty()));
    }

    fn world_with(entity: ModBlockEntity) -> (World, Pos) {
        let mut world = World::new(BlockStateId(0), 0);
        world.chunks.insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        let key = (3, 64, 5);
        world
            .block_entities
            .insert(key, BlockEntity::Mod(Box::new(entity)));
        (world, key)
    }

    fn cube(stored: i64) -> ModBlockEntity {
        let mut entity = ModBlockEntity {
            id: "mymod:battery".into(),
            ..Default::default()
        };
        entity.nbt.insert("energy".into(), Tag::Long(stored));
        entity
    }

    #[test]
    fn a_block_entity_is_out_of_the_world_during_its_call() {
        let (mut world, key) = world_with(cube(100));
        let mut taken = take(&mut world, key).expect("a mod block entity");
        assert_eq!(taken.id, "mymod:battery");
        // Its place reads as the ticking placeholder: asked again, it isn't there.
        assert!(take(&mut world, key).is_none());
        assert!(take(&mut world, (0, 64, 0)).is_none(), "no block entity");
        // A simulated call changes nothing: back, and its chunk not marked.
        assert!(put_back(&mut world, key, taken.clone(), false));
        assert!(world.modified.is_empty());
        taken = take(&mut world, key).unwrap();
        taken.nbt.insert("energy".into(), Tag::Long(140));
        assert!(put_back(&mut world, key, taken, true));
        assert!(world.modified.contains(&(0, 0)));
        let Some(BlockEntity::Mod(m)) = world.block_entities.get(&key) else {
            panic!("put back");
        };
        assert_eq!(m.nbt.get(&"energy".into()), Some(&Tag::Long(140)));
    }

    #[test]
    fn a_block_entity_removed_during_its_call_stays_gone() {
        let (mut world, key) = world_with(cube(100));
        let taken = take(&mut world, key).unwrap();
        world.block_entities.remove(&key);
        assert!(!put_back(&mut world, key, taken.clone(), true));
        assert!(world.block_entities.get(&key).is_none());
        // Replaced by another mod's entity meanwhile: that one stays.
        let (mut world, key) = world_with(cube(100));
        let _ = take(&mut world, key).unwrap();
        world
            .block_entities
            .insert(key, BlockEntity::Mod(Box::new(cube(7))));
        assert!(!put_back(&mut world, key, taken, true));
        let Some(BlockEntity::Mod(m)) = world.block_entities.get(&key) else {
            panic!("the new entity");
        };
        assert_eq!(m.nbt.get(&"energy".into()), Some(&Tag::Long(7)));
    }
}
