//! The energy settings of Mekanism's config files, as ATM10 ships them in
//! `config/Mekanism/`: `general.toml` (`[energy_conversion]`) and
//! `tiers.toml` (`[energy_cubes]`, `[transmitters.energy]`), and the unit
//! screens show from `common.toml` (`energyType`). A missing file,
//! key or a value out of the mod's range keeps the default, as NeoForge's
//! config spec corrects it.
use crate::tier::{CableTier, CubeTier};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct EnergyConfig {
    /// `blacklistForge`: no Forge Energy in or out.
    pub blacklist_forge: bool,
    /// `feConversionRate`: Joules to the FE (FE * rate = J), 1e-4..=1e4.
    pub fe_conversion_rate: f64,
    /// `<tier>Capacity` and `<tier>Output` of each cube tier, in `CubeTier::ALL` order.
    pub cubes: [(u64, u64); 5],
    /// `<tier>Capacity` of each cable tier, in `CableTier::ALL` order.
    pub cables: [u64; 4],
    /// `energyType`: screens show Forge Energy (`FORGE_ENERGY`, the
    /// default) or Joules (`JOULES`).
    pub show_fe: bool,
}

impl Default for EnergyConfig {
    fn default() -> Self {
        Self {
            blacklist_forge: false,
            fe_conversion_rate: 2.5,
            cubes: CubeTier::ALL.map(|t| (t.default_capacity(), t.default_output())),
            cables: CableTier::ALL.map(CableTier::default_capacity),
            show_fe: true,
        }
    }
}

impl EnergyConfig {
    /// Reads `<config>/Mekanism/general.toml` and `tiers.toml`.
    pub fn load(config: &Path) -> Self {
        let read = |name: &str| {
            std::fs::read_to_string(config.join("Mekanism").join(name))
                .ok()
                .and_then(|text| text.parse::<toml::Table>().ok())
        };
        let mut config = Self::from_tables(read("general.toml").as_ref(), read("tiers.toml").as_ref());
        config.show_fe = shows_fe(read("common.toml").as_ref());
        config
    }

    pub fn from_tables(general: Option<&toml::Table>, tiers: Option<&toml::Table>) -> Self {
        let mut config = Self::default();
        let section = |table: Option<&toml::Table>, path: &[&str]| -> Option<toml::Table> {
            let mut at = table?;
            for key in path {
                at = at.get(*key)?.as_table()?;
            }
            Some(at.clone())
        };
        if let Some(conversion) = section(general, &["energy_conversion"]) {
            if let Some(b) = conversion.get("blacklistForge").and_then(toml::Value::as_bool) {
                config.blacklist_forge = b;
            }
            if let Some(rate) = conversion.get("feConversionRate").and_then(float)
                && (1e-4..=1e4).contains(&rate)
            {
                config.fe_conversion_rate = rate;
            }
        }
        if let Some(cubes) = section(tiers, &["energy_cubes"]) {
            for (i, tier) in CubeTier::ALL.into_iter().enumerate() {
                if let Some(v) = positive(&cubes, &format!("{}Capacity", tier.name())) {
                    config.cubes[i].0 = v;
                }
                if let Some(v) = positive(&cubes, &format!("{}Output", tier.name())) {
                    config.cubes[i].1 = v;
                }
            }
        }
        if let Some(cables) = section(tiers, &["transmitters", "energy"]) {
            for (i, tier) in CableTier::ALL.into_iter().enumerate() {
                if let Some(v) = positive(&cables, &format!("{}Capacity", tier.name())) {
                    config.cables[i] = v;
                }
            }
        }
        config
    }

    pub fn cube(&self, tier: CubeTier) -> (u64, u64) {
        self.cubes[CubeTier::ALL.iter().position(|t| *t == tier).unwrap_or(0)]
    }

    pub fn cable(&self, tier: CableTier) -> u64 {
        self.cables[CableTier::ALL.iter().position(|t| *t == tier).unwrap_or(0)]
    }
}

/// `common.toml`'s `energyType` (top level): Forge Energy unless it says `JOULES`.
pub fn shows_fe(common: Option<&toml::Table>) -> bool {
    common.and_then(|c| c.get("energyType")).and_then(toml::Value::as_str) != Some("JOULES")
}

fn float(v: &toml::Value) -> Option<f64> {
    v.as_float().or_else(|| v.as_integer().map(|i| i as f64))
}

/// A `long` setting of range 1..=Long.MAX_VALUE.
fn positive(table: &toml::Table, key: &str) -> Option<u64> {
    let v = table.get(key)?.as_integer()?;
    (v >= 1).then_some(v as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atm10_files_override_the_defaults_and_bad_values_are_ignored() {
        let general: toml::Table = "[energy_conversion]\nblacklistForge = false\nfeConversionRate = 4.0\n"
            .parse()
            .unwrap();
        let tiers: toml::Table = "[energy_cubes]\nbasicCapacity = 5000000\nbasicOutput = 0\n\
             creativeCapacity = 9223372036854775807\n[transmitters]\n[transmitters.energy]\n\
             ultimateCapacity = 9000000\n"
            .parse()
            .unwrap();
        let c = EnergyConfig::from_tables(Some(&general), Some(&tiers));
        assert_eq!(c.fe_conversion_rate, 4.0);
        assert_eq!(c.cube(CubeTier::Basic), (5_000_000, 4_000));
        assert_eq!(c.cube(CubeTier::Creative).0, i64::MAX as u64);
        assert_eq!(c.cable(CableTier::Ultimate), 9_000_000);
        assert_eq!(c.cable(CableTier::Basic), 8_000);
        let wild: toml::Table = "[energy_conversion]\nfeConversionRate = 0.0\n".parse().unwrap();
        assert_eq!(EnergyConfig::from_tables(Some(&wild), None).fe_conversion_rate, 2.5);
        assert_eq!(EnergyConfig::from_tables(None, None), EnergyConfig::default());
        let joules: toml::Table = "energyType = \"JOULES\"\n".parse().unwrap();
        let fe: toml::Table = "energyType = \"FORGE_ENERGY\"\n".parse().unwrap();
        assert!(!shows_fe(Some(&joules)));
        assert!(shows_fe(Some(&fe)) && shows_fe(None));
    }
}
