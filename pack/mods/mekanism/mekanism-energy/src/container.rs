//! `BasicEnergyContainer` and `EnergyCubeEnergyContainer`: a store of
//! Joules with insert and extract rates by `AutomationType`. A cube's
//! container takes and gives at any rate from outside (`EXTERNAL`) and at its
//! tier's output from its own slots and its ejector (`INTERNAL`); a creative
//! cube's inserts and extracts answer as if they acted and never change what
//! it holds (`action.combine(!isCreative)`).

/// `mekanism.api.AutomationType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Automation {
    /// Another block: a cable, a neighbour's ejector, an FE handler.
    External,
    /// The block itself: its ejector, its charge and discharge slots.
    Internal,
    /// A player through a screen.
    Manual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnergyContainer {
    /// Joules held, at most `capacity`.
    pub stored: u64,
    /// `getMaxEnergy`, at most `Long.MAX_VALUE`.
    pub capacity: u64,
    /// The `INTERNAL` rate both ways (an energy cube's tier output); none:
    /// unlimited.
    pub internal_rate: Option<u64>,
    /// A creative cube: nothing it does changes `stored`.
    pub creative: bool,
}

const LONG_MAX: u64 = i64::MAX as u64;

impl EnergyContainer {
    /// `BasicEnergyContainer.create(maxEnergy, listener)`: both ways, every
    /// automation type, no rate.
    pub fn basic(capacity: u64) -> Self {
        Self {
            stored: 0,
            capacity: capacity.min(LONG_MAX),
            internal_rate: None,
            creative: false,
        }
    }

    /// `EnergyCubeEnergyContainer.create(tier, listener)` holding `stored`.
    pub fn cube(stored: u64, capacity: u64, output: u64, creative: bool) -> Self {
        let capacity = capacity.min(LONG_MAX);
        Self {
            stored: stored.min(capacity),
            capacity,
            internal_rate: Some(output.min(LONG_MAX)),
            creative,
        }
    }

    fn rate(&self, automation: Automation) -> u64 {
        match (automation, self.internal_rate) {
            (Automation::Internal, Some(rate)) => rate,
            _ => LONG_MAX,
        }
    }

    /// `getNeeded`.
    pub fn needed(&self) -> u64 {
        self.capacity.saturating_sub(self.stored)
    }

    /// `insert`: what of `amount` was not taken.
    pub fn insert(&mut self, amount: u64, simulate: bool, automation: Automation) -> u64 {
        if amount == 0 {
            return 0;
        }
        let needed = self.rate(automation).min(self.needed());
        if needed == 0 {
            return amount;
        }
        let to_add = amount.min(needed);
        if !simulate && !self.creative {
            self.stored += to_add;
        }
        amount - to_add
    }

    /// `extract`: what was given, at most `amount`.
    pub fn extract(&mut self, amount: u64, simulate: bool, automation: Automation) -> u64 {
        if self.stored == 0 || amount == 0 {
            return 0;
        }
        let given = self.rate(automation).min(self.stored).min(amount);
        if given > 0 && !simulate && !self.creative {
            self.stored -= given;
        }
        given
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cube_takes_anything_from_outside_and_moves_its_output_inside() {
        let mut c = EnergyContainer::cube(0, 4_000_000, 4_000, false);
        assert_eq!(c.insert(10_000, true, Automation::External), 0);
        assert_eq!(c.stored, 0, "simulate changes nothing");
        assert_eq!(c.insert(10_000, false, Automation::External), 0);
        assert_eq!(c.stored, 10_000);
        // Its own slots fill it at the tier output.
        assert_eq!(c.insert(10_000, false, Automation::Internal), 6_000);
        assert_eq!(c.stored, 14_000);
        // The ejector takes at most the output; outside at any rate.
        assert_eq!(c.extract(10_000, false, Automation::Internal), 4_000);
        assert_eq!(c.extract(20_000, false, Automation::External), 10_000);
        assert_eq!(c.stored, 0);
        assert_eq!(c.extract(1, false, Automation::External), 0);
        // Full: the remainder comes back whole.
        let mut full = EnergyContainer::cube(4_000_000, 4_000_000, 4_000, false);
        assert_eq!(full.insert(5, false, Automation::External), 5);
        // A saved value above capacity is cut, as setEnergy does.
        assert_eq!(EnergyContainer::cube(9_000_000, 4_000_000, 4_000, false).stored, 4_000_000);
    }

    #[test]
    fn a_creative_cube_answers_and_never_changes() {
        let max = i64::MAX as u64;
        let mut c = EnergyContainer::cube(max, max, max, true);
        assert_eq!(c.extract(1_000, false, Automation::Internal), 1_000);
        assert_eq!(c.extract(max, false, Automation::External), max);
        assert_eq!(c.stored, max);
        let mut empty = EnergyContainer::cube(0, max, max, true);
        assert_eq!(empty.insert(500, false, Automation::External), 0, "it takes as if it stored");
        assert_eq!(empty.stored, 0);
        assert_eq!(empty.extract(500, false, Automation::External), 0, "an empty one gives nothing");
        let basic = EnergyContainer::basic(u64::MAX);
        assert_eq!(basic.capacity, max);
    }
}
