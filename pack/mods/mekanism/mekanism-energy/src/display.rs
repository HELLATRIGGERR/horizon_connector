//! Energy as Mekanism's screens write it (`EnergyDisplay`,
//! `UnitDisplayUtils.getDisplayShort`): Joules, or Forge Energy at
//! `feConversionRate` when `energyType` says so, with an SI prefix and two
//! decimals cut (not rounded), the number as Java's `Double.toString` writes
//! it: `1.59 MFE`, `400.0 FE`, `0.0 J`.
use crate::config::EnergyConfig;

/// `MeasurementUnit`: (prefix, size).
const UNITS: [(&str, f64); 14] = [
    ("f", 1e-15),
    ("p", 1e-12),
    ("n", 1e-9),
    ("µ", 1e-6),
    ("m", 1e-3),
    ("", 1.0),
    ("k", 1e3),
    ("M", 1e6),
    ("G", 1e9),
    ("T", 1e12),
    ("P", 1e15),
    ("E", 1e18),
    ("Z", 1e21),
    ("Y", 1e24),
];

/// Java's `Double.toString` for the values a display shows (well inside
/// 1e-3..1e7 after the prefix): the shortest digits, at least one decimal.
fn java_double(d: f64) -> String {
    if d.fract() == 0.0 { format!("{d:.1}") } else { format!("{d}") }
}

/// `roundDecimals`: cut to `places` decimals, as `(long) (d * 10^places)`.
fn cut(d: f64, places: i32) -> f64 {
    let m = 10f64.powi(places);
    (d * m).trunc() / m
}

/// `getDisplayShort(value, unit)` with the unit's short symbol (`FE`, `J`).
pub fn short(value: f64, symbol: &str) -> String {
    if value == 0.0 {
        return format!("0.0 {symbol}");
    }
    let (negative, value) = (value < 0.0, value.abs());
    let i = (0..UNITS.len())
        .find(|&i| {
            (i == 0 && value < UNITS[0].1)
                || i + 1 >= UNITS.len()
                || (value >= UNITS[i].1 && value < UNITS[i + 1].1)
        })
        .unwrap_or(UNITS.len() - 1);
    let (prefix, size) = UNITS[i];
    let rounded = cut(value / size, 2);
    let sign = if negative { "-" } else { "" };
    format!("{sign}{} {prefix}{symbol}", java_double(rounded))
}

/// `EnergyDisplay.of(joules)`: the amount in the unit screens show.
pub fn energy(joules: u64, config: &EnergyConfig) -> String {
    if config.show_fe {
        short(joules as f64 / config.fe_conversion_rate, "FE")
    } else {
        short(joules as f64, "J")
    }
}

/// `EnergyDisplay.of(stored, max)`: `stored/max`, or `Infinite` for a
/// creative store (`Long.MAX_VALUE`).
pub fn fraction(stored: u64, capacity: u64, config: &EnergyConfig) -> String {
    if stored >= i64::MAX as u64 {
        return "Infinite".into();
    }
    format!("{}/{}", energy(stored, config), energy(capacity, config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_reads_as_in_mekanisms_screens() {
        let fe = EnergyConfig::default();
        let joules = EnergyConfig { show_fe: false, ..EnergyConfig::default() };
        assert_eq!(energy(4_000_000, &fe), "1.6 MFE");
        assert_eq!(energy(3_996_000, &fe), "1.59 MFE", "cut, not rounded");
        assert_eq!(energy(4_000, &fe), "1.6 kFE");
        assert_eq!(energy(1_000, &fe), "400.0 FE");
        assert_eq!(energy(1, &fe), "400.0 mFE");
        assert_eq!(energy(0, &fe), "0.0 FE");
        assert_eq!(energy(4_000_000, &joules), "4.0 MJ");
        assert_eq!(energy(256_000_000, &joules), "256.0 MJ");
        assert_eq!(fraction(3_996_000, 4_000_000, &fe), "1.59 MFE/1.6 MFE");
        assert_eq!(fraction(i64::MAX as u64, i64::MAX as u64, &fe), "Infinite");
        assert_eq!(short(-2_500.0, "J"), "-2.5 kJ");
    }
}
