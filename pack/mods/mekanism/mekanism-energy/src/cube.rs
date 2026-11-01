//! The energy cube (`TileEntityEnergyCube`): what its block entity keeps,
//! which faces expose its energy, and its ejector (`TileComponentEjector`
//! with `CableUtils.emit`), which pushes up to the tier output a tick out of
//! its output faces, split among the neighbours that take energy there.
use crate::container::{Automation, EnergyContainer};
use crate::menu::{Button, Kind};
use crate::sides::{self, DataType, Direction, RelativeSide, SideState};
use crate::split;
use crate::tier::CubeTier;

/// A cube as its block entity saves it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CubeState {
    pub tier: CubeTier,
    /// Joules held.
    pub stored: u64,
    /// The way its front looks. Java keeps it in the block state; here it is
    /// the entity's, so the six face properties fit the state cap.
    pub facing: Direction,
    /// The energy side config, by `RelativeSide` ordinal.
    pub energy: [DataType; 6],
    /// Whether the ejector pushes energy out (`eject0`).
    pub eject: bool,
    /// The item side config, kept for the item slots (`config3`).
    pub items: [DataType; 6],
    pub items_eject: bool,
}

impl CubeState {
    /// A new cube with the item's default config
    /// (`ItemBlockEnergyCube.SIDE_CONFIG`): energy out of the front, ejecting,
    /// in through the five other sides; items the same, not ejecting.
    /// `stored` is what [`stored_from`] makes of nothing saved.
    pub fn new(tier: CubeTier, capacity: u64) -> Self {
        let front_out = |side: RelativeSide| {
            if side == RelativeSide::Front { DataType::Output } else { DataType::Input }
        };
        Self {
            tier,
            stored: stored_from(tier, None, capacity),
            facing: Direction::North,
            energy: RelativeSide::ALL.map(front_out),
            eject: true,
            items: RelativeSide::ALL.map(front_out),
            items_eject: false,
        }
    }

    /// The energy data type on the absolute face `face`.
    pub fn energy_at(&self, face: Direction) -> DataType {
        self.energy[RelativeSide::from_directions(self.facing, face).index()]
    }

    /// What the model shows on `face`.
    pub fn side_state(&self, face: Direction) -> SideState {
        SideState::of(self.energy_at(face))
    }

    /// The faces the ejector pushes through: every side whose type can
    /// output, in `Direction` order (Java's `EnumSet`).
    pub fn output_faces(&self) -> Vec<Direction> {
        Direction::ALL
            .into_iter()
            .filter(|&d| sides::io(self.energy_at(d)).is_some_and(|(_, out)| out))
            .collect()
    }

    /// Whether `face` exposes the cube's energy at all: none for a `NONE`
    /// side, so nothing connects there.
    pub fn exposes(&self, face: Direction) -> bool {
        sides::io(self.energy_at(face)).is_some()
    }

    /// Its container, as the tier sizes it (`capacity`, `output`).
    pub fn container(&self, capacity: u64, output: u64) -> EnergyContainer {
        EnergyContainer::cube(self.stored, capacity, output, self.tier == CubeTier::Creative)
    }

    /// Another block offers `amount` Joules through `face`
    /// (`IStrictEnergyHandler.insertEnergy` on that side): what it did not
    /// take, or none when the face exposes nothing. A side that only outputs
    /// takes nothing.
    pub fn receive(&mut self, face: Direction, amount: u64, simulate: bool, capacity: u64, output: u64) -> Option<u64> {
        let (input, _) = sides::io(self.energy_at(face))?;
        if !input {
            return Some(amount);
        }
        let mut container = self.container(capacity, output);
        let rest = container.insert(amount, simulate, Automation::External);
        self.stored = container.stored;
        Some(rest)
    }
}

impl CubeState {
    /// A side configuration button (`PacketSideData`,
    /// `PacketEjectConfiguration`, `PacketBatchConfiguration`): true when the
    /// energy config changed, so the model's faces follow. Items can't
    /// eject (`setCanEject(false)`).
    pub fn press(&mut self, button: Button) -> bool {
        let (sides, eject) = match button {
            Button::Page(_) => return false,
            Button::Side { kind, .. } | Button::Eject(kind) | Button::Clear { kind, .. } => match kind {
                Kind::Energy => (&mut self.energy, Some(&mut self.eject)),
                Kind::Items => (&mut self.items, None),
            },
        };
        let before = *sides;
        match button {
            Button::Side { side, forward, .. } => {
                if let Some(t) = sides.get_mut(side) {
                    *t = if forward { t.next(sides::io_supports) } else { t.previous(sides::io_supports) };
                }
            }
            Button::Eject(_) => {
                if let Some(e) = eject {
                    *e = !*e;
                }
            }
            Button::Clear { forward, .. } => {
                let shared = sides.iter().all(|t| *t == sides[0]).then_some(sides[0]);
                let target = match shared {
                    Some(t) if forward => t.next(sides::io_supports),
                    Some(t) => t.previous(sides::io_supports),
                    None => DataType::None,
                };
                *sides = [target; 6];
            }
            Button::Page(_) => {}
        }
        let energy = matches!(button, Button::Side { kind: Kind::Energy, .. } | Button::Clear { kind: Kind::Energy, .. });
        energy && self.energy != before
    }
}

/// What a cube holds when its save says `saved` (none: nothing saved,
/// Mekanism's empty container). A creative cube with nothing saved counts as
/// full, so a placed one is a source until items carry energy; in Java only
/// the creative tab's filled cube is.
pub fn stored_from(tier: CubeTier, saved: Option<u64>, capacity: u64) -> u64 {
    match saved {
        Some(v) => v.min(capacity),
        None if tier == CubeTier::Creative => capacity,
        None => 0,
    }
}

/// `CableUtils.emit(targets, container, maxOutput)`: offers what the
/// container gives inside (at most `max_output`) to `count` acceptors with
/// `EmitUtils.sendToAcceptors`, takes out what they took and returns it.
/// `insert(i, amount, simulate)` returns what acceptor `i` did not take.
pub fn emit(
    container: &mut EnergyContainer,
    max_output: u64,
    count: usize,
    insert: impl FnMut(usize, u64, bool) -> u64,
) -> u64 {
    if container.stored == 0 || max_output == 0 || count == 0 {
        return 0;
    }
    let to_send = container.extract(max_output, true, Automation::Internal);
    if to_send == 0 {
        return 0;
    }
    let sent = split::send_to_acceptors(count, to_send, insert);
    container.extract(sent, false, Automation::Internal);
    sent
}

#[cfg(test)]
mod tests {
    use super::*;
    use Direction::*;

    #[test]
    fn a_new_cube_outputs_through_its_front() {
        let mut cube = CubeState::new(CubeTier::Basic, 4_000_000);
        assert_eq!(cube.stored, 0);
        assert_eq!(cube.output_faces(), [North]);
        let shown: Vec<_> = Direction::ALL.iter().map(|&d| cube.side_state(d).name()).collect();
        assert_eq!(shown, ["input", "input", "output", "input", "input", "input"]);
        cube.facing = East;
        assert_eq!(cube.output_faces(), [East]);
        assert_eq!(cube.side_state(North), SideState::Input);
        assert_eq!(CubeState::new(CubeTier::Creative, i64::MAX as u64).stored, i64::MAX as u64);
        assert_eq!(stored_from(CubeTier::Creative, Some(5), 100), 5);
        assert_eq!(stored_from(CubeTier::Basic, Some(500), 100), 100);
    }

    #[test]
    fn faces_take_by_their_type() {
        let mut cube = CubeState::new(CubeTier::Basic, 4_000_000);
        cube.energy[RelativeSide::Back.index()] = DataType::None;
        // Facing north: the back is south, the front north.
        assert_eq!(cube.receive(South, 100, false, 4_000_000, 4_000), None, "a NONE side exposes nothing");
        assert_eq!(cube.receive(North, 100, false, 4_000_000, 4_000), Some(100), "an output side takes nothing");
        assert_eq!(cube.receive(Up, 3_999_000, true, 4_000_000, 4_000), Some(0));
        assert_eq!(cube.stored, 0);
        assert_eq!(cube.receive(Up, 5_000_000, false, 4_000_000, 4_000), Some(1_000_000));
        assert_eq!(cube.stored, 4_000_000);
        assert!(!cube.exposes(South));
        assert!(cube.exposes(North));
    }

    #[test]
    fn the_ejector_splits_the_output_across_its_faces() {
        let mut cube = CubeState::new(CubeTier::Basic, 4_000_000);
        cube.stored = 1_000_000;
        cube.energy[RelativeSide::Left.index()] = DataType::InputOutput;
        cube.energy[RelativeSide::Top.index()] = DataType::Output;
        // Up, north and east, in Direction order.
        assert_eq!(cube.output_faces(), [Up, North, East]);
        let mut room = [1_000u64, 100_000, 100_000];
        let mut container = cube.container(4_000_000, 4_000);
        let sent = emit(&mut container, 4_000, room.len(), |i, offered, simulate| {
            let taken = offered.min(room[i]);
            if !simulate {
                room[i] -= taken;
            }
            offered - taken
        });
        assert_eq!(sent, 4_000);
        assert_eq!(room, [0, 98_500, 98_500], "the small one fills, the rest is shared");
        assert_eq!(container.stored, 996_000);
        // Nothing to take it: nothing leaves.
        assert_eq!(emit(&mut container, 4_000, 0, |_, a, _| a), 0);
        assert_eq!(emit(&mut container, 4_000, 2, |_, a, _| a), 0);
        assert_eq!(container.stored, 996_000);
    }

    #[test]
    fn side_buttons_cycle_toggle_and_clear() {
        let mut cube = CubeState::new(CubeTier::Basic, 4_000_000);
        let front = RelativeSide::Front.index();
        // Right click on the front: OUTPUT back to INPUT, and the model follows.
        assert!(cube.press(Button::Side { kind: Kind::Energy, side: front, forward: false }));
        assert_eq!(cube.energy[front], DataType::Input);
        assert_eq!(cube.output_faces(), []);
        assert!(cube.press(Button::Side { kind: Kind::Energy, side: front, forward: true }));
        assert!(cube.press(Button::Side { kind: Kind::Energy, side: front, forward: true }));
        assert_eq!(cube.energy[front], DataType::InputOutput);
        assert!(!cube.press(Button::Eject(Kind::Energy)));
        assert!(!cube.eject);
        assert!(!cube.press(Button::Eject(Kind::Items)));
        assert!(!cube.items_eject, "items can't eject");
        // Mixed sides clear to NONE; shared ones step together.
        assert!(cube.press(Button::Clear { kind: Kind::Energy, forward: true }));
        assert_eq!(cube.energy, [DataType::None; 6]);
        assert!(cube.press(Button::Clear { kind: Kind::Energy, forward: true }));
        assert_eq!(cube.energy, [DataType::Input; 6]);
        assert!(!cube.press(Button::Side { kind: Kind::Items, side: front, forward: true }));
        assert_eq!(cube.items[front], DataType::InputOutput);
        assert!(!cube.press(Button::Page(1)));
    }

    #[test]
    fn a_creative_cube_ejects_without_running_dry() {
        let max = i64::MAX as u64;
        let cube = CubeState::new(CubeTier::Creative, max);
        let mut container = cube.container(max, max);
        let mut basic = EnergyContainer::cube(0, 4_000_000, 4_000, false);
        let sent = emit(&mut container, max, 1, |_, offered, simulate| {
            basic.insert(offered, simulate, Automation::External)
        });
        assert_eq!(sent, 4_000_000, "a basic cube fills from it in one tick");
        assert_eq!(basic.stored, 4_000_000);
        assert_eq!(container.stored, max);
    }
}
