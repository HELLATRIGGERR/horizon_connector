//! Mekanism 10.7.19's energy, as All the Mods 10 runs it. Mekanism is MIT,
//! Copyright (c) 2017-2025 Aidan C. Brady.
//!
//! Energy is in Joules (`long`); Forge Energy is converted at the edges,
//! `feConversionRate` Joules to the FE (`fe`). Energy cubes and universal
//! cables come in tiers sized by `tiers.toml` (`tier`, `config`), and
//! machines and cable networks hand energy to their neighbours with the
//! even split of `EmitUtils.sendToAcceptors` (`split`). Energy is held in
//! containers with rates by automation type (`container`); blocks expose it
//! by side, as their side configuration says (`sides`); an energy cube keeps
//! its Joules and config and ejects out of its output faces (`cube`). Its
//! screen reads data slots and sends buttons (`menu`), and writes energy as
//! Mekanism's screens do (`display`).
//! Nothing here knows the game: the native mod's helpers bring the world.
pub mod config;
pub mod container;
pub mod cube;
pub mod display;
pub mod fe;
pub mod menu;
pub mod network;
pub mod sides;
pub mod split;
pub mod tier;

/// Java's `(long) d` for the energy maths: NaN is 0, the rest saturates.
pub(crate) fn clamp_to_long(d: f64) -> i64 {
    if d.is_nan() {
        0
    } else if d >= i64::MAX as f64 {
        i64::MAX
    } else if d <= i64::MIN as f64 {
        i64::MIN
    } else {
        d as i64
    }
}
