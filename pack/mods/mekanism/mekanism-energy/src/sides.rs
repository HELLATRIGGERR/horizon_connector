//! Faces and Mekanism's side configuration: the six `Direction`s of a block
//! (in Minecraft's order, which Java's `EnumSet<Direction>` iterates),
//! `RelativeSide` (front, left, right, back, top, bottom) seen from a
//! block's facing, `DataType` (what a side does, with Mekanism's ordinals,
//! which its saves keep), and the energy cube's visual side state
//! (`CubeSideState`).

/// `net.minecraft.core.Direction`, in its ordinal order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Direction {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Direction {
    pub const ALL: [Self; 6] = [Self::Down, Self::Up, Self::North, Self::South, Self::West, Self::East];

    pub fn name(self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Up => "up",
            Self::North => "north",
            Self::South => "south",
            Self::West => "west",
            Self::East => "east",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.name() == name)
    }

    pub fn opposite(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }

    /// The step to the neighbour on this side.
    pub fn offset(self) -> [i32; 3] {
        match self {
            Self::Down => [0, -1, 0],
            Self::Up => [0, 1, 0],
            Self::North => [0, 0, -1],
            Self::South => [0, 0, 1],
            Self::West => [-1, 0, 0],
            Self::East => [1, 0, 0],
        }
    }

    /// `getClockWise` about the vertical axis; up and down stay (Java
    /// throws for them, and no caller here asks).
    pub fn clockwise(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::East => Self::South,
            Self::South => Self::West,
            Self::West => Self::North,
            other => other,
        }
    }

    /// `getCounterClockWise` about the vertical axis.
    pub fn counter_clockwise(self) -> Self {
        match self {
            Self::North => Self::West,
            Self::West => Self::South,
            Self::South => Self::East,
            Self::East => Self::North,
            other => other,
        }
    }
}

/// `mekanism.api.RelativeSide`, in its ordinal order (the order of a saved
/// `config<type>` int array).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RelativeSide {
    Front,
    Left,
    Right,
    Back,
    Top,
    Bottom,
}

impl RelativeSide {
    pub const ALL: [Self; 6] = [Self::Front, Self::Left, Self::Right, Self::Back, Self::Top, Self::Bottom];

    pub fn name(self) -> &'static str {
        match self {
            Self::Front => "front",
            Self::Left => "left",
            Self::Right => "right",
            Self::Back => "back",
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// `getDirection(facing)`: the absolute face of this side.
    pub fn direction(self, facing: Direction) -> Direction {
        let vertical = matches!(facing, Direction::Down | Direction::Up);
        match self {
            Self::Front => facing,
            Self::Back => facing.opposite(),
            Self::Left if vertical => Direction::East,
            Self::Left => facing.clockwise(),
            Self::Right if vertical => Direction::West,
            Self::Right => facing.counter_clockwise(),
            Self::Top => match facing {
                Direction::Down => Direction::North,
                Direction::Up => Direction::South,
                _ => Direction::Up,
            },
            Self::Bottom => match facing {
                Direction::Down => Direction::South,
                Direction::Up => Direction::North,
                _ => Direction::Down,
            },
        }
    }

    /// `fromDirections(facing, side)`: which side of a block facing
    /// `facing` the absolute face `side` is.
    pub fn from_directions(facing: Direction, side: Direction) -> Self {
        if side == facing {
            return Self::Front;
        }
        if side == facing.opposite() {
            return Self::Back;
        }
        if matches!(facing, Direction::Down | Direction::Up) {
            return match side {
                Direction::North if facing == Direction::Down => Self::Top,
                Direction::North => Self::Bottom,
                Direction::South if facing == Direction::Down => Self::Bottom,
                Direction::South => Self::Top,
                Direction::West => Self::Right,
                _ => Self::Left,
            };
        }
        match side {
            Direction::Down => Self::Bottom,
            Direction::Up => Self::Top,
            s if s == facing.counter_clockwise() => Self::Right,
            s if s == facing.clockwise() => Self::Left,
            _ => Self::Front,
        }
    }
}

/// `DataType`, by Mekanism's ordinals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DataType {
    None,
    Input,
    Input1,
    Input2,
    Output,
    Output1,
    Output2,
    InputOutput,
    Energy,
    Extra,
}

impl DataType {
    pub const ALL: [Self; 10] = [
        Self::None,
        Self::Input,
        Self::Input1,
        Self::Input2,
        Self::Output,
        Self::Output1,
        Self::Output2,
        Self::InputOutput,
        Self::Energy,
        Self::Extra,
    ];

    /// The ordinal a save keeps.
    pub fn id(self) -> i32 {
        self as i32
    }

    /// `BY_ID`, which wraps out-of-range ids.
    pub fn by_id(id: i32) -> Self {
        Self::ALL[id.rem_euclid(Self::ALL.len() as i32) as usize]
    }

    /// `getSerializedName`.
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Input => "input",
            Self::Input1 => "input_1",
            Self::Input2 => "input_2",
            Self::Output => "output",
            Self::Output1 => "output_1",
            Self::Output2 => "output_2",
            Self::InputOutput => "input_output",
            Self::Energy => "energy",
            Self::Extra => "extra",
        }
    }

    /// `getNext(supported)`: the next type in ordinal order (wrapping) that
    /// `supported` accepts.
    pub fn next(self, supported: impl Fn(Self) -> bool) -> Self {
        let n = Self::ALL.len();
        (1..=n)
            .map(|step| Self::ALL[(self as usize + step) % n])
            .find(|&t| supported(t))
            .unwrap_or(self)
    }

    /// `getPrevious(supported)`.
    pub fn previous(self, supported: impl Fn(Self) -> bool) -> Self {
        let n = Self::ALL.len();
        (1..=n)
            .map(|step| Self::ALL[(self as usize + n - step) % n])
            .find(|&t| supported(t))
            .unwrap_or(self)
    }
}

/// What a side set up by `TileComponentConfig.setupIOConfig(type, container,
/// outputSide)` does: `INPUT` takes in, `OUTPUT` gives out,
/// `INPUT_OUTPUT` both, and every other type (`NONE`) has no slot info, so
/// nothing is exposed on that face. `(can_input, can_output)`.
pub fn io(data: DataType) -> Option<(bool, bool)> {
    match data {
        DataType::Input => Some((true, false)),
        DataType::Output => Some((false, true)),
        DataType::InputOutput => Some((true, true)),
        _ => None,
    }
}

/// The types such a side cycles through (`ConfigInfo.supports`).
pub fn io_supports(data: DataType) -> bool {
    data == DataType::None || io(data).is_some()
}

/// `TileEntityEnergyCube.CubeSideState`: what a face of the cube's model
/// shows. A side that can output is lit (`ACTIVE_LIT`), one that only takes
/// in shows its port unlit (`ACTIVE_UNLIT`), and one with nothing shows
/// neither (`INACTIVE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SideState {
    None,
    Input,
    Output,
}

impl SideState {
    pub fn of(data: DataType) -> Self {
        match io(data) {
            Some((_, true)) => Self::Output,
            Some((true, false)) => Self::Input,
            _ => Self::None,
        }
    }

    /// The value of the cube block's face property (`north=output`).
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Input => "input",
            Self::Output => "output",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_sides_follow_the_facing_and_come_back() {
        use Direction::*;
        // Facing north, the block's left is east (its own left, not the viewer's).
        let north: Vec<_> = RelativeSide::ALL.iter().map(|s| s.direction(North)).collect();
        assert_eq!(north, [North, East, West, South, Up, Down]);
        let up: Vec<_> = RelativeSide::ALL.iter().map(|s| s.direction(Up)).collect();
        assert_eq!(up, [Up, East, West, Down, South, North]);
        let down: Vec<_> = RelativeSide::ALL.iter().map(|s| s.direction(Down)).collect();
        assert_eq!(down, [Down, East, West, Up, North, South]);
        for facing in Direction::ALL {
            let mut seen: Vec<_> = RelativeSide::ALL.iter().map(|s| s.direction(facing)).collect();
            seen.sort();
            assert_eq!(seen, Direction::ALL, "every face once facing {facing:?}");
            for side in RelativeSide::ALL {
                assert_eq!(RelativeSide::from_directions(facing, side.direction(facing)), side);
            }
        }
    }

    #[test]
    fn data_types_keep_mekanism_ids_and_cycle_through_the_supported_ones() {
        assert_eq!(DataType::Output.id(), 4);
        assert_eq!(DataType::InputOutput.id(), 7);
        assert_eq!(DataType::by_id(4), DataType::Output);
        assert_eq!(DataType::by_id(11), DataType::Input);
        let cycle: Vec<_> = std::iter::successors(Some(DataType::None), |t| Some(t.next(io_supports)))
            .take(5)
            .collect();
        assert_eq!(
            cycle,
            [DataType::None, DataType::Input, DataType::Output, DataType::InputOutput, DataType::None]
        );
        assert_eq!(DataType::None.previous(io_supports), DataType::InputOutput);
        assert_eq!(SideState::of(DataType::InputOutput), SideState::Output);
        assert_eq!(SideState::of(DataType::Input), SideState::Input);
        assert_eq!(SideState::of(DataType::Extra), SideState::None);
    }
}
