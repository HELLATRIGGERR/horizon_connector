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
}
