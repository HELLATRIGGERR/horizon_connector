//! Numeric behavior of AE2 19.2.18 `me.energy.StoredEnergyAmount`.
//! Copyright (c) TeamAppliedEnergistics; LGPL-3.0-or-later, see LICENSE.

pub const MIN_AMOUNT: f64 = 0.000_001;
pub const MAX_MAXIMUM: f64 = (i64::MAX / 10_000) as f64;

/// A value and its maximum. The constructor, like the source, leaves both as
/// supplied; AE2 applies `setMaximum` and `setStored` at lifecycle boundaries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StoredEnergy {
    stored: f64,
    maximum: f64,
}
impl StoredEnergy {
    pub fn new(stored: f64, maximum: f64) -> Self {
        Self { stored, maximum }
    }
    pub fn stored(&self) -> f64 {
        self.stored
    }
    pub fn maximum(&self) -> f64 {
        self.maximum
    }
    pub fn set_stored(&mut self, amount: f64) {
        let amount = if amount < MIN_AMOUNT { 0.0 } else { amount };
        self.stored = amount.max(0.0).min(self.maximum);
    }
    pub fn set_maximum(&mut self, maximum: f64) {
        self.maximum = maximum.min(MAX_MAXIMUM);
        // The source clamps to the requested maximum, not its capped field.
        self.stored = self.stored.max(0.0).min(maximum);
    }
    pub fn insert(&mut self, amount: f64, commit: bool) -> f64 {
        if amount < MIN_AMOUNT {
            return 0.0;
        }
        let inserted = amount.min(self.maximum - self.stored);
        if commit {
            self.set_stored(self.stored + inserted);
        }
        inserted
    }
    pub fn extract(&mut self, amount: f64, commit: bool) -> f64 {
        if amount < MIN_AMOUNT {
            return 0.0;
        }
        let extracted = amount.min(self.stored);
        if commit {
            self.set_stored(self.stored - extracted);
        }
        extracted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_threshold_remainder_rounding_and_maximum() {
        let mut power = StoredEnergy::new(0.0, 1.0);
        assert_eq!(power.insert(MIN_AMOUNT / 2.0, true), 0.0);
        assert_eq!(power.insert(MIN_AMOUNT, false), MIN_AMOUNT);
        assert_eq!(power.stored(), 0.0);
        assert_eq!(power.insert(MIN_AMOUNT, true), MIN_AMOUNT);
        assert_eq!(power.extract(MIN_AMOUNT / 2.0, true), 0.0);
        power.set_stored(MIN_AMOUNT * 1.5);
        assert_eq!(power.extract(MIN_AMOUNT, true), MIN_AMOUNT);
        assert_eq!(power.stored(), 0.0, "sub-minimum remainder is rounded away");
        power.set_maximum(f64::MAX);
        assert_eq!(power.maximum(), MAX_MAXIMUM);
    }
}
