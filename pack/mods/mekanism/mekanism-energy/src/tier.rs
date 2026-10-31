//! The tiers of energy cubes (`EnergyCubeTier`) and universal cables
//! (`CableTier`), with Mekanism 10.7's default sizes; `tiers.toml` may change
//! them (`config`).

/// `EnergyCubeTier`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CubeTier {
    Basic,
    Advanced,
    Elite,
    Ultimate,
    Creative,
}

impl CubeTier {
    pub const ALL: [Self; 5] = [Self::Basic, Self::Advanced, Self::Elite, Self::Ultimate, Self::Creative];

    /// The tier's name in ids and config keys (`basic_energy_cube`, `basicCapacity`).
    pub fn name(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::Advanced => "advanced",
            Self::Elite => "elite",
            Self::Ultimate => "ultimate",
            Self::Creative => "creative",
        }
    }

    /// The tier of block `mekanism:<tier>_energy_cube`.
    pub fn of_block(id: &str) -> Option<Self> {
        let name = id.strip_prefix("mekanism:")?.strip_suffix("_energy_cube")?;
        Self::ALL.into_iter().find(|t| t.name() == name)
    }

    /// Joules the cube holds by default.
    pub fn default_capacity(self) -> u64 {
        match self {
            Self::Basic => 4_000_000,
            Self::Advanced => 16_000_000,
            Self::Elite => 64_000_000,
            Self::Ultimate => 256_000_000,
            Self::Creative => i64::MAX as u64,
        }
    }

    /// Joules a tick the cube puts out (and takes in from its own slots) by default.
    pub fn default_output(self) -> u64 {
        match self {
            Self::Basic => 4_000,
            Self::Advanced => 16_000,
            Self::Elite => 64_000,
            Self::Ultimate => 256_000,
            Self::Creative => i64::MAX as u64,
        }
    }
}

/// `CableTier`: universal cables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CableTier {
    Basic,
    Advanced,
    Elite,
    Ultimate,
}

impl CableTier {
    pub const ALL: [Self; 4] = [Self::Basic, Self::Advanced, Self::Elite, Self::Ultimate];

    pub fn name(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::Advanced => "advanced",
            Self::Elite => "elite",
            Self::Ultimate => "ultimate",
        }
    }

    /// The tier of block `mekanism:<tier>_universal_cable`.
    pub fn of_block(id: &str) -> Option<Self> {
        let name = id.strip_prefix("mekanism:")?.strip_suffix("_universal_cable")?;
        Self::ALL.into_iter().find(|t| t.name() == name)
    }

    /// Joules each cable adds to its network's buffer by default.
    pub fn default_capacity(self) -> u64 {
        match self {
            Self::Basic => 8_000,
            Self::Advanced => 128_000,
            Self::Elite => 1_024_000,
            Self::Ultimate => 8_192_000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_come_from_block_ids() {
        assert_eq!(CubeTier::of_block("mekanism:elite_energy_cube"), Some(CubeTier::Elite));
        assert_eq!(CubeTier::of_block("mekanism:elite_universal_cable"), None);
        assert_eq!(CableTier::of_block("mekanism:ultimate_universal_cable"), Some(CableTier::Ultimate));
        assert_eq!(CableTier::of_block("mekanismgenerators:basic_universal_cable"), None);
    }
}
