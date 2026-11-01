//! Forge Energy at a Mekanism block's edge: `ForgeEnergyIntegration` (a
//! Mekanism handler seen as `IEnergyStorage`) with `IEnergyConversion`'s
//! maths. FE amounts are Java `int`s, Joules `long`s.
use crate::clamp_to_long;

/// `IEnergyConversion` at `feConversionRate` Joules to the FE.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Conversion(pub f64);

impl Conversion {
    /// `convertFrom`: FE to Joules.
    pub fn from_fe(self, fe: i64) -> i64 {
        clamp_to_long(fe as f64 * self.0)
    }
    /// `convertTo`: Joules to FE, as a `long`.
    pub fn to_fe(self, joules: i64) -> i64 {
        if joules == 0 {
            0
        } else {
            clamp_to_long(joules as f64 / self.0)
        }
    }
    /// `convertToAsInt`.
    pub fn to_fe_int(self, joules: i64) -> i32 {
        self.to_fe(joules).clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }
    /// `isOneToOne` (`Mth.equal(1, rate)`).
    pub fn one_to_one(self) -> bool {
        (1.0 - self.0).abs() < 1.0e-5
    }
    /// `ForgeEnergyIntegration.convertToAndBack`: the Joules of the whole
    /// FE in `joules`, so no fraction of an FE is moved.
    fn to_and_back(self, joules: i64) -> i64 {
        let fe = self.to_fe_int(joules) as i64;
        let result = self.from_fe(fe);
        if self.0 >= 1.0 && (result as f64 % self.0) > 0.0 {
            return self.from_fe(fe - 1);
        }
        result
    }
}

/// A Mekanism energy handler: `insertEnergy` returns what it did not take,
/// `extractEnergy` what it gave; nothing changes with `simulate`.
pub trait StrictEnergy {
    fn insert(&mut self, joules: i64, simulate: bool) -> i64;
    fn extract(&mut self, joules: i64, simulate: bool) -> i64;
}

/// `ForgeEnergyIntegration.receiveEnergy`.
pub fn receive_fe(handler: &mut impl StrictEnergy, rate: Conversion, max_receive: i32, simulate: bool) -> i32 {
    if max_receive <= 0 {
        return 0;
    }
    let mut to_insert = rate.from_fe(max_receive as i64);
    if to_insert == 0 {
        return 0;
    }
    if !rate.one_to_one() {
        let remainder = handler.insert(to_insert, true);
        if remainder == to_insert {
            return 0;
        }
        to_insert = rate.to_and_back(to_insert - remainder);
        if to_insert == 0 {
            return 0;
        }
    }
    let remainder = handler.insert(to_insert, simulate);
    if remainder == to_insert {
        return 0;
    }
    rate.to_fe_int(to_insert - remainder)
}

/// `ForgeEnergyIntegration.extractEnergy`.
pub fn extract_fe(handler: &mut impl StrictEnergy, rate: Conversion, max_extract: i32, simulate: bool) -> i32 {
    if max_extract <= 0 {
        return 0;
    }
    let mut to_extract = rate.from_fe(max_extract as i64);
    if to_extract == 0 {
        return 0;
    }
    if !rate.one_to_one() {
        to_extract = rate.to_and_back(handler.extract(to_extract, true));
        if to_extract == 0 {
            return 0;
        }
    }
    rate.to_fe_int(handler.extract(to_extract, simulate))
}

impl Conversion {
    /// `ForgeStrictEnergyHandler.convertFromAndBack`: the whole FE that
    /// `fe` FE are worth in whole Joules.
    fn from_and_back(self, fe: i64) -> i32 {
        let joules = self.from_fe(fe);
        let result = self.to_fe_int(joules);
        let per_joule = 1.0 / self.0;
        if per_joule >= 1.0 && (result as f64 % per_joule) > 0.0 {
            return self.to_fe_int(joules - 1);
        }
        result
    }
}

/// A Forge Energy neighbour as Mekanism sees it, `ForgeStrictEnergyHandler`
/// over its `IEnergyStorage`: `receive(fe, simulate)` is its
/// `receiveEnergy`, `can_receive` its `canReceive`. Offers `joules` and
/// returns what it did not take (`insertEnergy`), in whole FE; with
/// `simulate`, what it would not take.
pub fn insert_into_fe(
    joules: i64,
    rate: Conversion,
    can_receive: bool,
    simulate: bool,
    mut receive: impl FnMut(i32, bool) -> i32,
) -> i64 {
    if !can_receive || joules <= 0 {
        return joules;
    }
    let mut to_insert = rate.to_fe_int(joules);
    if to_insert <= 0 {
        return joules;
    }
    if !rate.one_to_one() {
        let simulated = receive(to_insert, true);
        if simulated <= 0 {
            return joules;
        }
        to_insert = rate.from_and_back(simulated as i64);
        if to_insert <= 0 {
            return joules;
        }
    }
    let inserted = receive(to_insert, simulate);
    if inserted > 0 {
        return joules - rate.from_fe(inserted as i64);
    }
    joules
}

/// `ForgeStrictEnergyHandler.extractEnergy`: draws up to `joules` from a
/// Forge Energy neighbour through its `extractEnergy` (`extract(fe,
/// simulate)`), and returns the Joules it gave (with `simulate`, would
/// give).
pub fn extract_from_fe(
    joules: i64,
    rate: Conversion,
    can_extract: bool,
    simulate: bool,
    mut extract: impl FnMut(i32, bool) -> i32,
) -> i64 {
    if !can_extract || joules <= 0 {
        return 0;
    }
    let mut to_extract = rate.to_fe_int(joules);
    if to_extract <= 0 {
        return 0;
    }
    if !rate.one_to_one() {
        let simulated = extract(to_extract, true);
        to_extract = rate.from_and_back(simulated as i64);
        if to_extract <= 0 {
            return 0;
        }
    }
    rate.from_fe(extract(to_extract, simulate) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Buffer {
        stored: i64,
        capacity: i64,
    }
    impl StrictEnergy for Buffer {
        fn insert(&mut self, joules: i64, simulate: bool) -> i64 {
            let taken = joules.min(self.capacity - self.stored);
            if !simulate {
                self.stored += taken;
            }
            joules - taken
        }
        fn extract(&mut self, joules: i64, simulate: bool) -> i64 {
            let given = joules.min(self.stored);
            if !simulate {
                self.stored -= given;
            }
            given
        }
    }

    #[test]
    fn whole_fe_cross_the_edge_at_two_and_a_half_joules() {
        let rate = Conversion(2.5);
        let mut b = Buffer { stored: 0, capacity: 11 };
        // 10 FE = 25 J offered; 11 J of room is 4.4 FE, so 4 FE (10 J) go in.
        assert_eq!(receive_fe(&mut b, rate, 10, true), 4);
        assert_eq!(b.stored, 0);
        assert_eq!(receive_fe(&mut b, rate, 10, false), 4);
        assert_eq!(b.stored, 10);
        // 3 FE out is 7.5 J, cut to 7 J: 2.8 FE, so 2 whole FE (5 J) leave.
        assert_eq!(extract_fe(&mut b, rate, 3, false), 2);
        assert_eq!(b.stored, 5);
        assert_eq!(extract_fe(&mut b, rate, 3, false), 2);
        assert_eq!(b.stored, 0);
        assert_eq!(extract_fe(&mut b, rate, 3, false), 0);
        assert_eq!(rate.to_fe_int(i64::MAX), i32::MAX);
    }

    /// An `IEnergyStorage` that takes at most `room` FE and holds `stored`.
    struct Storage {
        stored: i32,
        room: i32,
        calls: Vec<(i32, bool)>,
    }
    impl Storage {
        fn receive(&mut self, fe: i32, simulate: bool) -> i32 {
            self.calls.push((fe, simulate));
            let took = fe.min(self.room);
            if !simulate {
                self.room -= took;
                self.stored += took;
            }
            took
        }
        fn extract(&mut self, fe: i32, simulate: bool) -> i32 {
            let given = fe.min(self.stored);
            if !simulate {
                self.stored -= given;
            }
            given
        }
    }

    #[test]
    fn a_cube_ejects_whole_fe_into_a_forge_energy_neighbour() {
        let rate = Conversion(2.5);
        // A basic cube's 4000 J a tick are 1600 FE, all taken.
        let mut acceptor = Storage { stored: 0, room: i32::MAX, calls: vec![] };
        assert_eq!(insert_into_fe(4000, rate, true, false, |fe, s| acceptor.receive(fe, s)), 0);
        assert_eq!(acceptor.stored, 1600);
        assert_eq!(acceptor.calls, [(1600, true), (1600, false)]);
        // Room for 7 FE: 7 FE are 17 whole J, which are 6 whole FE (15 J).
        let mut small = Storage { stored: 0, room: 7, calls: vec![] };
        assert_eq!(insert_into_fe(25, rate, true, false, |fe, s| small.receive(fe, s)), 10);
        assert_eq!(small.stored, 6);
        // A simulation moves nothing; a storage that can't receive is not asked.
        let mut sim = Storage { stored: 0, room: 100, calls: vec![] };
        assert_eq!(insert_into_fe(250, rate, true, true, |fe, s| sim.receive(fe, s)), 0);
        assert_eq!((sim.stored, sim.calls.iter().all(|c| c.1)), (0, true));
        assert_eq!(insert_into_fe(250, rate, false, false, |_, _| unreachable!()), 250);
        // Less than an FE is not offered.
        assert_eq!(insert_into_fe(2, rate, true, false, |_, _| unreachable!()), 2);
        // Drawing: 10 J are 4 FE.
        let mut source = Storage { stored: 100, room: 0, calls: vec![] };
        assert_eq!(extract_from_fe(10, rate, true, false, |fe, s| source.extract(fe, s)), 10);
        assert_eq!(source.stored, 96);
        assert_eq!(extract_from_fe(10, rate, false, false, |_, _| unreachable!()), 0);
    }
}
