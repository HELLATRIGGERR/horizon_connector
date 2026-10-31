//! AE2 19.2.18 MeteoritePlacer/Fallout port (LGPL-3.0-or-later).
//! This module retains source iteration and random draw order; the host owns
//! the generation phase, biome selection, block update propagation and RNG seed.
use horizon_core::random::RandomSource;

/// Object-safe subset used by both native feature RNGs and structure pieces.
pub trait MeteorRandom {
    fn next_float(&mut self) -> f32;
    fn next_double(&mut self) -> f64;
    fn next_int_bound(&mut self, bound: i32) -> Result<i32, horizon_core::random::InvalidBound>;
}
impl<R: RandomSource> MeteorRandom for R {
    fn next_float(&mut self) -> f32 {
        RandomSource::next_float(self)
    }
    fn next_double(&mut self) -> f64 {
        RandomSource::next_double(self)
    }
    fn next_int_bound(&mut self, bound: i32) -> Result<i32, horizon_core::random::InvalidBound> {
        RandomSource::next_int_bound(self, bound)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crater {
    None,
    Normal,
    Lava,
    Obsidian,
    Water,
    Snow,
    Ice,
}
impl Crater {
    pub fn filler(self) -> &'static str {
        match self {
            Self::None | Self::Normal => "minecraft:air",
            Self::Lava => "minecraft:lava",
            Self::Obsidian => "minecraft:obsidian",
            Self::Water => "minecraft:water",
            Self::Ice => "minecraft:ice",
            Self::Snow => "minecraft:snow_block",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallout {
    Default,
    Sand,
    Terracotta,
    IceSnow,
}
impl Fallout {
    pub fn crater_adjustment(self) -> f64 {
        if matches!(self, Self::Sand | Self::IceSnow) {
            2.0
        } else {
            0.0
        }
    }
    /// `a` is the one source nextFloat, without converting f32 thresholds to f64.
    pub fn fall(self, copied: &'static str, a: f32) -> Option<&'static str> {
        match self {
            Self::Default => Some(if a > 0.9 {
                "minecraft:stone"
            } else if a > 0.8 {
                "minecraft:cobblestone"
            } else if a > 0.7 {
                "minecraft:dirt"
            } else {
                "minecraft:gravel"
            }),
            _ if a > 0.9 => Some(copied),
            _ => self.other(a),
        }
    }
    pub fn inset(self, copied: &'static str, a: f32) -> Option<&'static str> {
        match self {
            Self::Default => Some(if a > 0.9 {
                "minecraft:cobblestone"
            } else if a > 0.8 {
                "minecraft:stone"
            } else if a > 0.7 {
                "minecraft:grass_block"
            } else if a > 0.6 {
                "ae2:sky_stone_block"
            } else if a > 0.5 {
                "minecraft:gravel"
            } else {
                "minecraft:air"
            }),
            _ if a > 0.9 => Some(copied),
            _ if a > 0.8 => Some("minecraft:air"),
            _ => self.other(a - 0.1),
        }
    }
    fn other(self, a: f32) -> Option<&'static str> {
        match self {
            Self::Sand if a > 0.66 => Some("minecraft:glass"),
            Self::IceSnow if a > 0.7 => Some("minecraft:snow"),
            Self::IceSnow if a > 0.5 => Some("minecraft:ice"),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Meteorite {
    pub pos: [i32; 3],
    pub radius: f64,
    pub crater: Crater,
    pub fallout: Fallout,
    pub pure_crater: bool,
    pub crater_lake: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub spawn_presses: bool,
    pub flawless_only: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            spawn_presses: true,
            flawless_only: false,
        }
    }
}

/// Source `MeteoriteStructure.determineCraterType`. Uses the same RNG stream
/// that already selected center X/Z and radius. Ocean consumes no extra draw.
pub fn choose_crater(
    random: &mut impl MeteorRandom,
    temp: f32,
    rain: bool,
    snow: bool,
    ocean: bool,
) -> Crater {
    if ocean {
        return Crater::None;
    }
    if random.next_float() <= 0.5 {
        return Crater::Normal;
    }
    if temp >= 1.0 {
        let lava = random.next_float() > 0.5;
        if !rain {
            return if lava { Crater::Lava } else { Crater::Normal };
        }
        if !snow {
            let obsidian = random.next_float() > 0.75;
            return if !lava {
                Crater::Normal
            } else if obsidian {
                Crater::Obsidian
            } else {
                Crater::Lava
            };
        }
    }
    if temp < 1.0 && temp >= 0.2 {
        let lake = random.next_float() > 0.25;
        let lava = random.next_float() > 0.8;
        if !rain {
            return if lava { Crater::Lava } else { Crater::Normal };
        }
        if !snow {
            let obsidian = random.next_float() > 0.75;
            return if lava {
                if obsidian {
                    Crater::Obsidian
                } else {
                    Crater::Lava
                }
            } else if lake {
                Crater::Water
            } else {
                Crater::Normal
            };
        }
        let snowy = random.next_float() > 0.75;
        return if snowy {
            Crater::Snow
        } else if lake {
            Crater::Water
        } else {
            Crater::Normal
        };
    }
    if temp < 0.2 {
        let lake = random.next_float() > 0.25;
        let lava = random.next_float() > 0.95;
        let frozen = random.next_float() > 0.25;
        if !rain {
            return if lava { Crater::Lava } else { Crater::Normal };
        }
        if !snow {
            return if lava {
                Crater::Lava
            } else if !lake {
                Crater::Normal
            } else if frozen {
                Crater::Ice
            } else {
                Crater::Water
            };
        }
        return if lava {
            Crater::Lava
        } else if lake {
            Crater::Snow
        } else {
            Crater::Normal
        };
    }
    Crater::Normal
}

/// A clipped generation view. State queries use exported native block facts,
/// not names guessed to be solid or replaceable. `put` protects bedrock.
pub trait World {
    type State: Copy + Eq;
    fn bounds(&self) -> ([i32; 3], [i32; 3]);
    fn sea_level(&self) -> i32;
    fn state(&self, name: &str) -> Option<Self::State>;
    fn get(&self, pos: [i32; 3]) -> Self::State;
    fn put(&mut self, pos: [i32; 3], state: Self::State);
    fn air(&self, state: Self::State) -> bool;
    fn solid(&self, state: Self::State) -> bool;
    fn replaceable(&self, state: Self::State) -> bool;
    fn fluid(&self, state: Self::State) -> bool;
    fn replaceable_tag(&self, state: Self::State) -> bool;
    fn same_block(&self, a: Self::State, b: Self::State) -> bool;
    fn copied_fallout(&self) -> &'static str;
    fn schedule_surface_water(&mut self, pos: [i32; 3]);
    /// Only lava slope enclosure uses the level RNG, independently of the
    /// feature-placement RNG. Preserve that source distinction.
    fn level_random_float(&mut self) -> f32;
    /// Live Java placement discards intersecting item entities after the
    /// crater and before the core. Worldgen adapters have no live entities.
    fn after_crater(&mut self, _meteorite: Meteorite) {}
}
fn named(w: &mut impl World, p: [i32; 3], name: &str) {
    if let Some(state) = w.state(name) {
        w.put(p, state);
    }
}
fn fall(w: &mut impl World, p: [i32; 3], kind: Fallout, r: &mut impl MeteorRandom) {
    if let Some(name) = kind.fall(w.copied_fallout(), r.next_float()) {
        named(w, p, name);
    }
}
fn inset(w: &mut impl World, p: [i32; 3], kind: Fallout, r: &mut impl MeteorRandom) {
    if let Some(name) = kind.inset(w.copied_fallout(), r.next_float()) {
        named(w, p, name);
    }
}
pub fn place(w: &mut impl World, m: Meteorite, options: Options, r: &mut impl MeteorRandom) {
    let (min, max) = w.bounds();
    let [x, y, z] = m.pos;
    let Some(air) = w.state("minecraft:air") else {
        return;
    };
    let Some(sky) = w.state("ae2:sky_stone_block") else {
        return;
    };
    let Some(filler) = w.state(m.crater.filler()) else {
        return;
    };
    let radius2 = m.radius * m.radius;
    let crater = (m.radius * 2.0 + 5.0).powi(2);
    let cutoff = |i: i32, k: i32| {
        f64::from(y) - m.radius
            + 1.0
            + m.fallout.crater_adjustment()
            + f64::from((i - x) * (i - x) + (k - z) * (k - z)) * 0.02
    };
    // Source loops Y, X, Z. Writes have no random draw here.
    if m.crater != Crater::None {
        for j in (y - 5).max(min[1])..=max[1] {
            for i in min[0]..=max[0] {
                for k in min[2]..=max[2] {
                    if f64::from(j) > cutoff(i, k) {
                        let p = [i, j, k];
                        let current = w.get(p);
                        w.put(
                            p,
                            if m.crater != Crater::Normal && j < y && w.solid(current) {
                                filler
                            } else {
                                air
                            },
                        );
                    }
                }
            }
        }
        w.after_crater(m);
    }
    let quartz = if options.flawless_only {
        vec!["ae2:flawless_budding_quartz"]
    } else {
        vec![
            "ae2:quartz_block",
            "ae2:damaged_budding_quartz",
            "ae2:chipped_budding_quartz",
            "ae2:flawed_budding_quartz",
            "ae2:flawless_budding_quartz",
        ]
    };
    let buds = [
        "ae2:small_quartz_bud",
        "ae2:medium_quartz_bud",
        "ae2:large_quartz_bud",
    ];
    // Source loops X, Y, Z, selecting the bud immediately above each floor
    // block with the same random stream (not a new per-coordinate seed).
    for i in (x - 8).clamp(min[0], max[0])..=(x + 8).clamp(min[0], max[0]) {
        for j in y - 8..y + 8 {
            for k in (z - 8).clamp(min[2], max[2])..=(z + 8).clamp(min[2], max[2]) {
                let (dx, dy, dz) = (i - x, j - y, k - z);
                if f64::from(dx * dx) * 0.7
                    + f64::from(dy * dy) * if j > y { 1.4 } else { 0.8 }
                    + f64::from(dz * dz) * 0.7
                    >= radius2
                {
                    continue;
                }
                let p = [i, j, k];
                if dx.abs() <= 1 && dy.abs() <= 1 && dz.abs() <= 1 {
                    if dy == -1 {
                        let index = r
                            .next_int_bound(quartz.len() as i32)
                            .expect("positive length") as usize;
                        named(w, p, quartz[index]);
                        // Upstream deliberately suppresses buds when flawless-only
                        // uses its sole index zero, despite the name of that block.
                        if index != 0 && (dx != 0 || dz != 0) && r.next_float() <= 0.7 {
                            let bud = r.next_int_bound(3).expect("positive bound") as usize;
                            named(w, [i, j + 1, k], buds[bud]);
                        }
                    }
                } else {
                    w.put(p, sky);
                }
            }
        }
    }
    if options.spawn_presses
        && (0..3).all(|axis| m.pos[axis] >= min[axis] && m.pos[axis] <= max[axis])
    {
        named(w, m.pos, "ae2:mysterious_cube");
    }
    if m.crater != Crater::None {
        let mut fallen = 0.0;
        for i in (x - 30).clamp(min[0], max[0])..=(x + 30).clamp(min[0], max[0]) {
            for k in (z - 30).clamp(min[2], max[2])..=(z + 30).clamp(min[2], max[2]) {
                for j in y - 9..y + 30 {
                    let p = [i, j, k];
                    let state = w.get(p);
                    if m.pure_crater && w.same_block(state, filler) {
                        continue;
                    }
                    let up = w.get([i, j + 1, k]);
                    if w.replaceable(state) {
                        if !w.air(up) {
                            w.put(p, up);
                        } else if fallen < 100.0 * crater {
                            let (dx, dy, dz) = (i - x, j - y, k - z);
                            let dist = f64::from(dx * dx + dy * dy + dz * dz);
                            let below = w.get([i, j - 1, k]);
                            if !w.replaceable(below) {
                                let extra = r.next_double() * 0.6;
                                let height = crater * (extra + 0.2) - (dist - crater * 1.7).abs();
                                if !w.air(below) && height > 0.0 && r.next_double() > 0.6 {
                                    fallen += 1.0;
                                    fall(w, p, m.fallout, r);
                                }
                            }
                        }
                    } else if w.air(up) && r.next_double() > 0.4 {
                        let (dx, dy, dz) = (i - x, j - y, k - z);
                        let dist = f64::from(dx * dx + dy * dy + dz * dz);
                        if !(dx.abs() <= 1 && dy.abs() <= 1 && dz.abs() <= 1) && dist < crater * 1.6
                        {
                            inset(w, p, m.fallout, r);
                        }
                    }
                }
            }
        }
    }
    if m.crater_lake {
        let top = w.sea_level() - 1;
        for i in min[0]..=max[0] {
            for k in min[2]..=max[2] {
                for j in (y - 5).max(min[1])..=top.min(max[1]) {
                    let p = [i, j, k];
                    let h = cutoff(i, k);
                    if f64::from(j) > h {
                        if w.get(p) == air {
                            named(w, p, "minecraft:water");
                            if j == top {
                                w.schedule_surface_water(p);
                            }
                        }
                    } else if f64::from(top + (top - j) * 2 + 2) > h {
                        for down in 0..20 {
                            let p = [i, j - down, k];
                            let state = w.get(p);
                            if state == air
                                || (!w.fluid(state)
                                    && (w.replaceable(state) || w.replaceable_tag(state)))
                            {
                                if m.crater == Crater::Lava && w.level_random_float() < 0.075 {
                                    named(w, p, "minecraft:magma_block");
                                } else {
                                    fall(w, p, m.fallout, r);
                                }
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::random::LegacyRandom;
    use std::collections::BTreeMap;
    struct Fixture {
        blocks: BTreeMap<[i32; 3], &'static str>,
        bounds: ([i32; 3], [i32; 3]),
        copied: &'static str,
        level_random: LegacyRandom,
    }
    impl World for Fixture {
        type State = &'static str;

        fn bounds(&self) -> ([i32; 3], [i32; 3]) {
            self.bounds
        }
        fn sea_level(&self) -> i32 {
            10
        }
        fn state(&self, n: &str) -> Option<Self::State> {
            Some(match n {
                "minecraft:air" => "minecraft:air",
                "minecraft:stone" => "minecraft:stone",
                "minecraft:cobblestone" => "minecraft:cobblestone",
                "minecraft:dirt" => "minecraft:dirt",
                "minecraft:gravel" => "minecraft:gravel",
                "minecraft:grass_block" => "minecraft:grass_block",
                "ae2:sky_stone_block" => "ae2:sky_stone_block",
                "ae2:mysterious_cube" => "ae2:mysterious_cube",
                "ae2:quartz_block" => "ae2:quartz_block",
                "ae2:damaged_budding_quartz" => "ae2:damaged_budding_quartz",
                "ae2:chipped_budding_quartz" => "ae2:chipped_budding_quartz",
                "ae2:flawed_budding_quartz" => "ae2:flawed_budding_quartz",
                "ae2:flawless_budding_quartz" => "ae2:flawless_budding_quartz",
                "ae2:small_quartz_bud" => "ae2:small_quartz_bud",
                "ae2:medium_quartz_bud" => "ae2:medium_quartz_bud",
                "ae2:large_quartz_bud" => "ae2:large_quartz_bud",
                "minecraft:water" => "minecraft:water",
                "minecraft:lava" => "minecraft:lava",
                "minecraft:ice" => "minecraft:ice",
                "minecraft:obsidian" => "minecraft:obsidian",
                "minecraft:snow_block" => "minecraft:snow_block",
                "minecraft:snow" => "minecraft:snow",
                "minecraft:magma_block" => "minecraft:magma_block",
                "minecraft:glass" => "minecraft:glass",
                "minecraft:sand" => "minecraft:sand",
                "minecraft:terracotta" => "minecraft:terracotta",
                _ => return None,
            })
        }
        fn get(&self, p: [i32; 3]) -> Self::State {
            self.blocks.get(&p).copied().unwrap_or("minecraft:air")
        }
        fn put(&mut self, p: [i32; 3], s: Self::State) {
            if (0..3).all(|a| p[a] >= self.bounds.0[a] && p[a] <= self.bounds.1[a])
                && self.get(p) != "minecraft:bedrock"
            {
                self.blocks.insert(p, s);
            }
        }
        fn air(&self, s: Self::State) -> bool {
            s == "minecraft:air"
        }
        fn solid(&self, s: Self::State) -> bool {
            !self.air(s)
                && !self.fluid(s)
                && !matches!(s, "minecraft:short_grass" | "minecraft:snow")
        }
        fn replaceable(&self, s: Self::State) -> bool {
            self.air(s) || self.fluid(s) || matches!(s, "minecraft:short_grass" | "minecraft:snow")
        }
        fn fluid(&self, s: Self::State) -> bool {
            matches!(s, "minecraft:water" | "minecraft:lava")
        }
        fn replaceable_tag(&self, _: Self::State) -> bool {
            false
        }
        fn same_block(&self, a: Self::State, b: Self::State) -> bool {
            a == b
        }
        fn copied_fallout(&self) -> &'static str {
            self.copied
        }
        fn schedule_surface_water(&mut self, _: [i32; 3]) {}
        fn level_random_float(&mut self) -> f32 {
            RandomSource::next_float(&mut self.level_random)
        }
    }
    fn fixture() -> Fixture {
        Fixture {
            blocks: BTreeMap::new(),
            bounds: ([-16, -16, -16], [16, 40, 16]),
            copied: "minecraft:sand",
            level_random: LegacyRandom::new(999),
        }
    }
    fn meteor() -> Meteorite {
        Meteorite {
            pos: [0, 0, 0],
            radius: 5.0,
            crater: Crater::Normal,
            fallout: Fallout::Default,
            pure_crater: false,
            crater_lake: false,
        }
    }
    #[test]
    fn copy_fallout_uses_source_shifted_thresholds() {
        assert_eq!(
            Fallout::Sand.inset("minecraft:sand", 0.78),
            Some("minecraft:glass")
        );
        assert_eq!(Fallout::Sand.inset("minecraft:sand", 0.75), None);
        assert_eq!(
            Fallout::IceSnow.inset("minecraft:snow_block", 0.78),
            Some("minecraft:ice")
        );
        assert_eq!(
            Fallout::IceSnow.fall("minecraft:snow_block", 0.78),
            Some("minecraft:snow")
        );
        assert_eq!(Fallout::Terracotta.fall("minecraft:terracotta", 0.75), None);
    }
    #[test]
    fn ocean_consumes_no_crater_random_and_warm_snow_is_normal() {
        let mut a = LegacyRandom::new(2);
        let mut b = a.clone();
        assert_eq!(choose_crater(&mut a, 0.5, true, false, true), Crater::None);
        assert_eq!(a.next_int(), b.next_int());
        for seed in 0..100 {
            assert_eq!(
                choose_crater(&mut LegacyRandom::new(seed), 1.2, true, true, false),
                Crater::Normal
            );
        }
    }
    #[test]
    fn flawless_only_source_suppresses_buds_and_preserves_center_bedrock() {
        let mut w = fixture();
        w.blocks.insert([0, 0, 0], "minecraft:bedrock");
        let mut m = meteor();
        m.crater = Crater::None;
        place(
            &mut w,
            m,
            Options {
                spawn_presses: true,
                flawless_only: true,
            },
            &mut LegacyRandom::new(123),
        );
        assert_eq!(w.get([0, 0, 0]), "minecraft:bedrock");
        assert_eq!(
            w.blocks
                .values()
                .filter(|s| s.ends_with("budding_quartz"))
                .count(),
            9
        );
        assert!(!w.blocks.values().any(|s| s.ends_with("quartz_bud")));
    }
    #[test]
    fn replaceable_column_copies_above_and_pure_crater_preserves_filler() {
        let mut w = fixture();
        w.blocks.insert([12, 2, 0], "minecraft:water");
        w.blocks.insert([12, 3, 0], "minecraft:stone");
        let mut m = meteor();
        m.pos[1] = 20;
        m.crater = Crater::Water;
        m.pure_crater = true;
        place(&mut w, m, Options::default(), &mut LegacyRandom::new(9));
        assert_eq!(w.get([12, 2, 0]), "minecraft:water");
        let mut w = fixture();
        w.blocks.insert([12, 2, 0], "minecraft:short_grass");
        w.blocks.insert([12, 3, 0], "minecraft:stone");
        m.pos[1] = 10;
        m.crater = Crater::Normal;
        m.pure_crater = false;
        place(&mut w, m, Options::default(), &mut LegacyRandom::new(9));
        assert_eq!(w.get([12, 2, 0]), "minecraft:stone");
    }
    #[test]
    fn placement_matches_forty_unmodified_java_source_fixtures() {
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("ae2_meteorite_oracle.json")).unwrap();
        assert_eq!(cases.len(), 40);
        for case in cases {
            let mode = case["mode"].as_u64().unwrap();
            let seed = case["seed"].as_i64().unwrap();
            let mut w = fixture();
            for x in -16..=16 {
                for z in -16..=16 {
                    for y in -16..=5 {
                        w.blocks.insert(
                            [x, y, z],
                            if y == -16 {
                                "minecraft:bedrock"
                            } else {
                                "minecraft:stone"
                            },
                        );
                    }
                }
            }
            w.blocks.insert([12, 6, 0], "minecraft:short_grass");
            w.blocks.insert([12, 7, 0], "minecraft:stone");
            let mut m = meteor();
            m.crater = match mode {
                1 => Crater::Water,
                2 => Crater::Lava,
                3 => Crater::Snow,
                4 | 5 | 6 | 7 => Crater::None,
                _ => Crater::Normal,
            };
            m.fallout = match mode {
                1 => Fallout::Sand,
                2 => Fallout::Terracotta,
                3 => Fallout::IceSnow,
                _ => Fallout::Default,
            };
            m.pure_crater = mode == 3;
            m.crater_lake = mode == 1 || mode == 2;
            w.copied = match mode {
                1 => "minecraft:sand",
                2 => "minecraft:terracotta",
                3 => "minecraft:snow_block",
                _ => "minecraft:cobblestone",
            };
            let mut random = LegacyRandom::new(seed);
            place(
                &mut w,
                m,
                Options {
                    spawn_presses: mode < 6,
                    flawless_only: mode == 5 || mode == 7,
                },
                &mut random,
            );
            let mut hash = 0xcbf29ce484222325_u64;
            let mut count = 0;
            for ([x, y, z], id) in &w.blocks {
                if *id == "minecraft:air" {
                    continue;
                }
                count += 1;
                for byte in format!("{x},{y},{z}={id}\n").bytes() {
                    hash ^= u64::from(byte);
                    hash = hash.wrapping_mul(0x100000001b3);
                }
            }

            assert_eq!(
                count,
                case["blocks"].as_u64().unwrap(),
                "block count mode={mode} seed={seed}"
            );
            assert_eq!(
                format!("{hash:x}"),
                case["hash"].as_str().unwrap(),
                "all block positions mode={mode} seed={seed}"
            );
            assert_eq!(
                i64::from(random.next_int()),
                case["next"].as_i64().unwrap(),
                "remaining random stream mode={mode} seed={seed}"
            );
        }
    }
}
