// Derived from AE2 BasicCellInventory, commit db17504a86128fdf3dae31f5fb7a112a646e0b93.
// Copyright (c) 2021 TeamAppliedEnergistics. LGPL-3.0-or-later; see LICENSE.
use std::collections::{BTreeMap, BTreeSet};

/// Host-provided identity. Component bytes must be canonical under Minecraft equality.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ItemKey {
    pub item_id: String,
    pub components: Vec<u8>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Simulate,
    Modulate,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    K1,
    K4,
    K16,
    K64,
    K256,
}
impl Tier {
    fn kilobytes(self) -> u64 {
        match self {
            Self::K1 => 1,
            Self::K4 => 4,
            Self::K16 => 16,
            Self::K64 => 64,
            Self::K256 => 256,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Item,
    Fluid,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterMode {
    Whitelist,
    Blacklist,
}
/// Exact-match partition. An empty list permits all keys, in either mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    pub mode: FilterMode,
    pub keys: BTreeSet<ItemKey>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FuzzyMode {
    #[default]
    IgnoreAll,
    Percent99,
    Percent75,
    Percent50,
    Percent25,
}
/// AE2 powered tools also implement IBasicCellItem with custom layouts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolCell {
    MatterCannon,
    ColorApplicator,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellConfig {
    pub tier: Tier,
    pub tool: Option<ToolCell>,
    pub channel: Channel,
    pub portable: bool,
    pub filter: Option<Filter>,
    /// Fuzzy card in AE2's default IGNORE_ALL mode: match primary item ID.
    pub fuzzy: bool,
    pub fuzzy_mode: FuzzyMode,
    pub equal_distribution: bool,
    pub void_excess: bool,
}
impl CellConfig {
    pub fn item(tier: Tier) -> Self {
        Self {
            tier,
            tool: None,
            channel: Channel::Item,
            portable: false,
            filter: None,
            fuzzy: false,
            fuzzy_mode: FuzzyMode::IgnoreAll,
            equal_distribution: false,
            void_excess: false,
        }
    }
    pub fn fluid(tier: Tier) -> Self {
        Self {
            channel: Channel::Fluid,
            ..Self::item(tier)
        }
    }
    pub fn portable_item(tier: Tier) -> Self {
        Self {
            portable: true,
            ..Self::item(tier)
        }
    }
    pub fn portable_fluid(tier: Tier) -> Self {
        Self {
            portable: true,
            ..Self::fluid(tier)
        }
    }
    pub fn matter_cannon() -> Self {
        Self {
            tool: Some(ToolCell::MatterCannon),
            ..Self::item(Tier::K1)
        }
    }
    pub fn color_applicator() -> Self {
        Self {
            tool: Some(ToolCell::ColorApplicator),
            ..Self::item(Tier::K4)
        }
    }
    pub fn total_bytes(&self) -> u64 {
        if let Some(tool) = self.tool {
            return match tool {
                ToolCell::MatterCannon => 512,
                ToolCell::ColorApplicator => 2048,
            };
        }
        self.tier.kilobytes() * if self.portable { 512 } else { 1024 }
    }
    pub fn bytes_per_type(&self) -> u64 {
        if let Some(tool) = self.tool {
            return match tool {
                ToolCell::MatterCannon => 8,
                ToolCell::ColorApplicator => 32,
            };
        }
        self.tier.kilobytes() * 8
    }
    pub fn amount_per_byte(&self) -> u64 {
        match self.channel {
            Channel::Item => 8,
            Channel::Fluid => 8000,
        }
    }
    pub fn max_types(&self) -> usize {
        if let Some(tool) = self.tool {
            return match tool {
                ToolCell::MatterCannon => 1,
                ToolCell::ColorApplicator => 27,
            };
        }
        match self.channel {
            Channel::Item if self.portable => {
                63 - match self.tier {
                    Tier::K1 => 9,
                    Tier::K4 => 18,
                    Tier::K16 => 27,
                    Tier::K64 => 36,
                    Tier::K256 => 45,
                }
            }
            Channel::Item => 63,
            Channel::Fluid => 18,
        }
    }
    fn allows_with(
        &self,
        key: &ItemKey,
        fuzzy_match: &impl Fn(&ItemKey, &ItemKey, FuzzyMode) -> bool,
    ) -> bool {
        match &self.filter {
            Some(f) if !f.keys.is_empty() => {
                let matched = if self.fuzzy {
                    f.keys
                        .iter()
                        .any(|other| fuzzy_match(other, key, self.fuzzy_mode))
                } else {
                    f.keys.contains(key)
                };
                match f.mode {
                    FilterMode::Whitelist => matched,
                    FilterMode::Blacklist => !matched,
                }
            }
            _ => true,
        }
    }
    fn preformatted(&self) -> bool {
        self.filter.as_ref().is_some_and(|f| !f.keys.is_empty())
    }
    fn per_type_limit(&self) -> u64 {
        if !self.equal_distribution {
            return u64::MAX;
        }
        let max = match &self.filter {
            Some(f) if !self.fuzzy && f.mode == FilterMode::Whitelist && !f.keys.is_empty() => {
                f.keys.len().min(self.max_types())
            }
            _ => self.max_types(),
        } as u64;
        ((self.total_bytes() - self.bytes_per_type() * max) * self.amount_per_byte()).div_ceil(max)
    }
}
impl Default for CellConfig {
    fn default() -> Self {
        Self::item(Tier::K1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreError {
    ZeroAmount,
    DuplicateKey,
    Overflow,
    TooManyTypes,
    OverCapacity,
}

#[derive(Clone, Debug, Default)]
pub struct Cell {
    config: CellConfig,
    items: BTreeMap<ItemKey, u64>,
    /// Stored keys outside the cell's insertion channel remain distinct even
    /// if their host key bytes happen to equal a key in the primary channel.
    other_items: BTreeMap<ItemKey, u64>,
    total_items: u64,
}
impl Cell {
    // Backward-compatible 1K item constants. For other cells use config accessors.
    pub const TOTAL_BYTES: u64 = 1024;
    pub const BYTES_PER_TYPE: u64 = 8;
    pub const ITEMS_PER_BYTE: u64 = 8;
    pub const MAX_TYPES: usize = 63;
    pub fn new_1k() -> Self {
        Self::default()
    }
    pub fn new(config: CellConfig) -> Self {
        Self {
            config,
            items: BTreeMap::new(),
            other_items: BTreeMap::new(),
            total_items: 0,
        }
    }
    /// Restore stored contents without applying insertion filters or upgrades.
    /// Rejects zero counts, duplicate keys, overflow, type and byte excess.
    pub fn from_entries(
        config: CellConfig,
        entries: impl IntoIterator<Item = (ItemKey, u64)>,
    ) -> Result<Self, RestoreError> {
        Self::from_entries_with_other(config, entries, std::iter::empty())
    }
    /// Restore both source key channels in one physical cell. The other
    /// channel can be extracted but cannot be inserted by this cell type.
    pub fn from_entries_with_other(
        config: CellConfig,
        entries: impl IntoIterator<Item = (ItemKey, u64)>,
        other_entries: impl IntoIterator<Item = (ItemKey, u64)>,
    ) -> Result<Self, RestoreError> {
        Self::restore_entries(config, entries, other_entries, true)
    }
    /// Source BasicCellInventory loads existing entries even if they exceed
    /// the cell's present type/byte limits. Extraction must remain possible.
    pub fn from_source_entries_with_other(
        config: CellConfig,
        entries: impl IntoIterator<Item = (ItemKey, u64)>,
        other_entries: impl IntoIterator<Item = (ItemKey, u64)>,
    ) -> Result<Self, RestoreError> {
        Self::restore_entries(config, entries, other_entries, false)
    }
    fn restore_entries(
        config: CellConfig,
        entries: impl IntoIterator<Item = (ItemKey, u64)>,
        other_entries: impl IntoIterator<Item = (ItemKey, u64)>,
        strict: bool,
    ) -> Result<Self, RestoreError> {
        let mut cell = Self::new(config);
        let tagged = entries.into_iter().map(|(key,amount)|(false,key,amount))
            .chain(other_entries.into_iter().map(|(key,amount)|(true,key,amount)));
        for (other, key, amount) in tagged {
            if amount == 0 {
                return Err(RestoreError::ZeroAmount);
            }
            let items = if other { &mut cell.other_items } else { &mut cell.items };
            if items.contains_key(&key) {
                return Err(RestoreError::DuplicateKey);
            }
            cell.total_items = cell
                .total_items
                .checked_add(amount)
                .ok_or(RestoreError::Overflow)?;
            items.insert(key, amount);
            if strict && cell.stored_types() > cell.config.max_types() {
                return Err(RestoreError::TooManyTypes);
            }
        }
        // Avoid overflow in ceil division for arbitrary corrupted input.
        let a = cell.config.amount_per_byte();
        let payload = cell.total_items / a + u64::from(cell.total_items % a != 0);
        let overhead = cell.stored_types() as u64 * cell.config.bytes_per_type();
        if strict && payload > cell.config.total_bytes().saturating_sub(overhead) {
            return Err(RestoreError::OverCapacity);
        }
        Ok(cell)
    }
    pub fn config(&self) -> &CellConfig {
        &self.config
    }
    pub fn entries(&self) -> impl Iterator<Item = (&ItemKey, &u64)> {
        self.items.iter()
    }
    pub fn count(&self, key: &ItemKey) -> u64 {
        self.items.get(key).copied().unwrap_or(0)
    }
    pub fn other_entries(&self) -> impl Iterator<Item = (&ItemKey, &u64)> {
        self.other_items.iter()
    }
    pub fn entries_for(&self, channel: Channel) -> impl Iterator<Item = (&ItemKey, &u64)> {
        if self.config.channel==channel {self.items.iter()} else {self.other_items.iter()}
    }
    pub fn other_count(&self, key: &ItemKey) -> u64 {
        self.other_items.get(key).copied().unwrap_or(0)
    }
    pub fn stored_types(&self) -> usize {
        self.items.len() + self.other_items.len()
    }
    pub fn total_items(&self) -> u64 {
        self.total_items
    }
    pub fn used_bytes(&self) -> u64 {
        self.stored_types() as u64 * self.config.bytes_per_type()
            + self.total_items.div_ceil(self.config.amount_per_byte())
    }
    fn unused(&self) -> u64 {
        let a = self.config.amount_per_byte();
        (a - self.total_items % a) % a
    }
    pub fn remaining_items(&self) -> u64 {
        if self.used_bytes()>self.config.total_bytes() {return 0;}
        (self.config.total_bytes() - self.used_bytes()) * self.config.amount_per_byte()
            + self.unused()
    }
    pub fn can_hold_new_type(&self) -> bool {
        let free = self.config.total_bytes().saturating_sub(self.used_bytes());
        let per = self.config.bytes_per_type();
        self.stored_types() < self.config.max_types()
            && (free > per || (free == per && self.unused() > 0))
    }
    /// Return accepted amount. With void enabled this may exceed actual stored amount.
    pub fn insert(&mut self, key: &ItemKey, amount: u64, action: Action) -> u64 {
        let fluid = self.config.channel == Channel::Fluid;
        self.insert_with_fuzzy_match(key, amount, action, |a, b, mode| {
            (fluid || mode == FuzzyMode::IgnoreAll) && a.item_id == b.item_id
        })
    }
    /// Host resolves registry-backed durability for non-default fuzzy partitions.
    /// The callback receives filter key, candidate key and source AE2 mode.
    pub fn insert_with_fuzzy_match(
        &mut self,
        key: &ItemKey,
        amount: u64,
        action: Action,
        fuzzy_match: impl Fn(&ItemKey, &ItemKey, FuzzyMode) -> bool,
    ) -> u64 {
        if amount == 0 || !self.config.allows_with(key, &fuzzy_match) {
            return 0;
        }
        let current = self.count(key);
        let mut remaining = self.remaining_items();
        let stored = if current == 0 && !self.can_hold_new_type() {
            0
        } else {
            if current == 0 {
                remaining = remaining
                    .saturating_sub(self.config.bytes_per_type() * self.config.amount_per_byte());
            }
            amount
                .min(remaining)
                .min(self.config.per_type_limit().saturating_sub(current))
        };
        if stored > 0 && action == Action::Modulate {
            self.items.insert(key.clone(), current + stored);
            self.total_items += stored;
        }
        if self.config.void_excess {
            if !self.config.preformatted() && !self.can_hold_new_type() {
                return if self.count(key) > 0 { amount } else { stored };
            }
            return amount;
        }
        stored
    }
    pub fn extract(&mut self, key: &ItemKey, amount: u64, action: Action) -> u64 {
        let current = self.count(key);
        let extracted = amount.min(current);
        if extracted > 0 && action == Action::Modulate {
            if extracted == current {
                self.items.remove(key);
            } else {
                self.items.insert(key.clone(), current - extracted);
            }
            self.total_items -= extracted;
        }
        extracted
    }
    /// `BasicCellInventory.extract` permits already-stored keys regardless
    /// of channel; only insertion checks the physical cell's channel.
    pub fn extract_other(&mut self, key: &ItemKey, amount: u64, action: Action) -> u64 {
        let current = self.other_count(key);
        let extracted = amount.min(current);
        if extracted > 0 && action == Action::Modulate {
            if extracted == current { self.other_items.remove(key); }
            else { self.other_items.insert(key.clone(), current - extracted); }
            self.total_items -= extracted;
        }
        extracted
    }
}

/// Ordered collection of cells. Ordering is supplied by the host; this does not
/// implement AE2 network priority, routing, energy or cell mounting semantics.
#[derive(Clone, Debug, Default)]
pub struct CellBank {
    cells: Vec<Cell>,
}
impl CellBank {
    pub fn new(cells: Vec<Cell>) -> Self {
        Self { cells }
    }
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }
    pub fn cells_mut(&mut self) -> &mut [Cell] {
        &mut self.cells
    }
    pub fn insert(&mut self, key: &ItemKey, amount: u64, action: Action) -> u64 {
        let mut accepted = 0;
        for c in &mut self.cells {
            accepted += c.insert(key, amount - accepted, action);
            if accepted == amount {
                break;
            }
        }
        accepted
    }
    pub fn extract(&mut self, key: &ItemKey, amount: u64, action: Action) -> u64 {
        let mut extracted = 0;
        for c in &mut self.cells {
            extracted += c.extract(key, amount - extracted, action);
            if extracted == amount {
                break;
            }
        }
        extracted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_foreign_channel_is_extractable_and_shares_physical_capacity() {
        let key=ItemKey{item_id:"same:id".into(),components:vec![1,2]};
        let config=CellConfig::fluid(Tier::K1);
        let expected=2*config.bytes_per_type()+1012_u64.div_ceil(config.amount_per_byte());
        let mut cell=Cell::from_entries_with_other(config,[(key.clone(),1000)],[(key.clone(),12)]).unwrap();
        assert_eq!(cell.count(&key),1000);assert_eq!(cell.other_count(&key),12);
        assert_eq!(cell.stored_types(),2);assert_eq!(cell.total_items(),1012);assert_eq!(cell.used_bytes(),expected);
        assert_eq!(cell.extract_other(&key,5,Action::Simulate),5);assert_eq!(cell.other_count(&key),12);
        assert_eq!(cell.extract_other(&key,5,Action::Modulate),5);assert_eq!(cell.other_count(&key),7);
        assert_eq!(cell.count(&key),1000);assert_eq!(cell.total_items(),1007);
        assert_eq!(cell.extract_other(&key,7,Action::Modulate),7);assert_eq!(cell.stored_types(),1);
    }
    #[test]
    fn source_foreign_keys_count_toward_the_shared_type_limit() {
        let primary=(0..9).map(|index|(key(index),1)).collect::<Vec<_>>();
        let other=(9..18).map(|index|(key(index),1)).collect::<Vec<_>>();
        let mut cell=Cell::from_entries_with_other(CellConfig::fluid(Tier::K1),primary,other).unwrap();
        assert_eq!(cell.stored_types(),18);assert!(!cell.can_hold_new_type());
        assert_eq!(cell.insert(&key(99),1,Action::Modulate),0);
        assert_eq!(cell.extract_other(&key(9),1,Action::Modulate),1);
        assert!(cell.can_hold_new_type());assert_eq!(cell.insert(&key(99),1,Action::Modulate),1);
    }
    #[test]
    fn overfilled_source_cell_keeps_contents_extractable_without_free_space() {
        let config=CellConfig::fluid(Tier::K1);
        let entries=(0..20).map(|index|(key(index),1)).collect::<Vec<_>>();
        assert_eq!(Cell::from_entries(config.clone(),entries.clone()).unwrap_err(),RestoreError::TooManyTypes);
        let mut cell=Cell::from_source_entries_with_other(config,entries,std::iter::empty()).unwrap();
        assert_eq!(cell.stored_types(),20);assert!(!cell.can_hold_new_type());
        assert_eq!(cell.extract(&key(0),1,Action::Modulate),1);assert_eq!(cell.stored_types(),19);
        let config=CellConfig::item(Tier::K1);
        let oversized=Cell::from_source_entries_with_other(config,[(key(0),9000)],std::iter::empty()).unwrap();
        assert_eq!(oversized.remaining_items(),0);assert!(!oversized.can_hold_new_type());
        assert_eq!(oversized.count(&key(0)),9000);
    }
    fn key(id: usize) -> ItemKey {
        ItemKey {
            item_id: format!("test:{id}"),
            components: vec![],
        }
    }
    #[test]
    fn source_tool_cell_layouts_and_type_limits() {
        let mut cannon = Cell::new(CellConfig::matter_cannon());
        assert_eq!(cannon.insert(&key(0), u64::MAX, Action::Simulate), 4032);
        assert_eq!(cannon.insert(&key(0), 1, Action::Modulate), 1);
        assert_eq!(cannon.insert(&key(1), 1, Action::Modulate), 0);
        let mut applicator = Cell::new(CellConfig::color_applicator());
        assert_eq!(
            applicator.insert(&key(0), u64::MAX, Action::Simulate),
            16128
        );
        for i in 0..27 {
            assert_eq!(applicator.insert(&key(i), 1, Action::Modulate), 1);
        }
        assert_eq!(applicator.insert(&key(27), 1, Action::Modulate), 0);
        assert_eq!(applicator.used_bytes(), 27 * 32 + 4);
    }
    #[test]
    fn single_type_capacity_and_simulation() {
        let mut cell = Cell::new_1k();
        let k = key(0);
        assert_eq!(cell.insert(&k, u64::MAX, Action::Simulate), 8128);
        assert_eq!(cell.used_bytes(), 0);
        assert_eq!(cell.insert(&k, u64::MAX, Action::Modulate), 8128);
        assert_eq!(cell.used_bytes(), 1024);
        assert_eq!(cell.insert(&k, 1, Action::Modulate), 0);
        assert_eq!(cell.extract(&k, 9, Action::Simulate), 9);
        assert_eq!(cell.count(&k), 8128);
        assert_eq!(cell.extract(&k, 9, Action::Modulate), 9);
        assert_eq!(cell.insert(&k, 20, Action::Modulate), 9);
    }
    #[test]
    fn types_share_partial_item_byte_and_are_limited() {
        let mut cell = Cell::new_1k();
        for n in 0..63 {
            assert_eq!(cell.insert(&key(n), 1, Action::Modulate), 1);
        }
        assert_eq!(cell.used_bytes(), 63 * 8 + 8);
        assert_eq!(cell.insert(&key(63), 1, Action::Modulate), 0);
        assert_eq!(cell.insert(&key(0), 1, Action::Modulate), 1);
        assert_eq!(cell.extract(&key(1), 1, Action::Modulate), 1);
        assert_eq!(cell.insert(&key(63), 1, Action::Modulate), 1);
    }
    #[test]
    fn components_are_distinct_and_removing_a_type_reclaims_overhead() {
        let mut cell = Cell::new_1k();
        let a = key(1);
        let mut b = a.clone();
        b.components.push(42);
        assert_eq!(cell.insert(&a, 1, Action::Modulate), 1);
        assert_eq!(cell.insert(&b, 1, Action::Modulate), 1);
        assert_eq!(cell.used_bytes(), 17);
        assert_eq!(cell.extract(&a, 99, Action::Modulate), 1);
        assert_eq!(cell.used_bytes(), 9);
        assert_eq!(cell.count(&b), 1);
    }
    #[test]
    fn new_type_can_use_partial_byte_when_exactly_eight_free_bytes_remain() {
        let mut cell = Cell::new_1k();
        assert_eq!(cell.insert(&key(0), 8063, Action::Modulate), 8063);
        assert_eq!(cell.used_bytes(), 1016);
        assert_eq!(cell.insert(&key(1), 10, Action::Modulate), 1);
        assert_eq!(cell.used_bytes(), 1024);
    }
}

#[cfg(test)]
mod extended_tests {
    use super::*;
    fn key(n: usize) -> ItemKey {
        ItemKey {
            item_id: format!("test:{n}"),
            components: vec![],
        }
    }
    #[test]
    fn tier_and_channel_capacities_and_limits() {
        for tier in [Tier::K1, Tier::K4, Tier::K16, Tier::K64, Tier::K256] {
            for config in [CellConfig::item(tier), CellConfig::fluid(tier)] {
                let expected =
                    (config.total_bytes() - config.bytes_per_type()) * config.amount_per_byte();
                let mut c = Cell::new(config.clone());
                assert_eq!(c.insert(&key(0), u64::MAX, Action::Modulate), expected);
                assert_eq!(c.used_bytes(), config.total_bytes());
                assert_eq!(c.extract(&key(0), u64::MAX, Action::Modulate), expected);
                for n in 0..config.max_types() {
                    assert_eq!(c.insert(&key(n), 1, Action::Modulate), 1);
                }
                assert_eq!(c.insert(&key(config.max_types()), 1, Action::Modulate), 0);
            }
        }
    }
    #[test]
    fn distribution_filter_and_void_match_upstream_examples() {
        let mut config = CellConfig::item(Tier::K1);
        config.equal_distribution = true;
        let mut c = Cell::new(config);
        let limit = ((1024u64 - 63 * 8) * 8).div_ceil(63);
        for n in 0..62 {
            assert_eq!(c.insert(&key(n), u64::MAX, Action::Modulate), limit);
        }
        assert_eq!(
            c.insert(&key(62), u64::MAX, Action::Modulate),
            (1024 - 63 * 8) * 8 - limit * 62
        );
        let mut config = CellConfig::item(Tier::K1);
        config.filter = Some(Filter {
            mode: FilterMode::Whitelist,
            keys: [key(0), key(1)].into_iter().collect(),
        });
        config.void_excess = true;
        let mut c = Cell::new(config);
        assert_eq!(c.insert(&key(0), 10000, Action::Modulate), 10000);
        assert_eq!(c.count(&key(0)), 8128);
        assert_eq!(c.insert(&key(1), 1, Action::Modulate), 1);
        assert_eq!(c.insert(&key(2), 1, Action::Modulate), 0);
        let mut config = CellConfig::item(Tier::K1);
        config.void_excess = true;
        let mut c = Cell::new(config);
        assert_eq!(c.insert(&key(0), 10000, Action::Modulate), 10000);
        assert_eq!(c.insert(&key(1), 1, Action::Modulate), 0);
    }
    #[test]
    fn portable_cells_follow_upstream_tiers_and_item_type_limits() {
        for (tier, bytes, item_types) in [
            (Tier::K1, 512, 54),
            (Tier::K4, 2048, 45),
            (Tier::K16, 8192, 36),
            (Tier::K64, 32768, 27),
            (Tier::K256, 131072, 18),
        ] {
            let items = CellConfig::portable_item(tier);
            let fluids = CellConfig::portable_fluid(tier);
            assert_eq!(items.total_bytes(), bytes);
            assert_eq!(items.max_types(), item_types);
            assert_eq!(fluids.total_bytes(), bytes);
            assert_eq!(fluids.max_types(), 18);
        }
        let mut c = Cell::new(CellConfig::portable_item(Tier::K1));
        assert_eq!(c.insert(&key(0), u64::MAX, Action::Simulate), 4032);
    }
    #[test]
    fn fuzzy_ignore_all_matches_primary_item_and_uses_full_distribution_count() {
        let mut config = CellConfig::item(Tier::K1);
        let original = key(1);
        let mut variant = original.clone();
        variant.components = vec![1];
        config.filter = Some(Filter {
            mode: FilterMode::Whitelist,
            keys: [original].into_iter().collect(),
        });
        config.fuzzy = true;
        config.equal_distribution = true;
        let mut cell = Cell::new(config);
        let per_type = ((1024u64 - 63 * 8) * 8).div_ceil(63);
        assert_eq!(cell.insert(&variant, u64::MAX, Action::Modulate), per_type);
        assert_eq!(cell.insert(&key(2), 1, Action::Modulate), 0);
    }
    #[test]
    fn fluid_fuzzy_modes_match_primary_key_and_keep_blacklist_semantics() {
        let water=ItemKey {item_id:"minecraft:water".into(),components:vec![1]};
        let variant=ItemKey {item_id:"minecraft:water".into(),components:vec![2]};
        let lava=ItemKey {item_id:"minecraft:lava".into(),components:vec![]};
        for mode in [FuzzyMode::IgnoreAll,FuzzyMode::Percent99,FuzzyMode::Percent75,FuzzyMode::Percent50,FuzzyMode::Percent25] {
            for (filter_mode,water_expected,lava_expected) in [
                (FilterMode::Whitelist,1000,0),(FilterMode::Blacklist,0,1000),
            ] {
                let mut config=CellConfig::fluid(Tier::K1);
                config.fuzzy=true;
                config.fuzzy_mode=mode;
                config.filter=Some(Filter {mode:filter_mode,keys:[water.clone()].into_iter().collect()});
                let mut cell=Cell::new(config);
                assert_eq!(cell.insert(&variant,1000,Action::Simulate),water_expected);
                assert_eq!(cell.insert(&lava,1000,Action::Simulate),lava_expected);
                assert_eq!(cell.total_items(),0);
                assert_eq!(cell.insert(&variant,1000,Action::Modulate),water_expected);
                assert_eq!(cell.insert(&lava,1000,Action::Modulate),lava_expected);
                assert_eq!(cell.total_items(),1000);
            }
        }
    }
    #[test]
    fn fuzzy_partition_delegates_registry_durability_and_simulation_is_read_only() {
        let mut config = CellConfig::item(Tier::K1);
        let mut reference = key(1);
        reference.components = vec![20];
        let mut candidate = reference.clone();
        candidate.components = vec![30];
        let mut outlier = reference.clone();
        outlier.components = vec![60];
        config.filter = Some(Filter {
            mode: FilterMode::Whitelist,
            keys: [reference].into_iter().collect(),
        });
        config.fuzzy = true;
        config.fuzzy_mode = FuzzyMode::Percent50;
        let mut cell = Cell::new(config);
        assert_eq!(cell.insert(&candidate, 10, Action::Simulate), 0);
        let resolver = |a: &ItemKey, b: &ItemKey, mode| {
            a.item_id == b.item_id
                && mode == FuzzyMode::Percent50
                && (a.components[0] > 50) == (b.components[0] > 50)
        };
        assert_eq!(
            cell.insert_with_fuzzy_match(&candidate, 10, Action::Simulate, resolver),
            10
        );
        assert_eq!(cell.total_items(), 0);
        assert_eq!(
            cell.insert_with_fuzzy_match(&outlier, 10, Action::Modulate, resolver),
            0
        );
        assert_eq!(
            cell.insert_with_fuzzy_match(&candidate, 10, Action::Modulate, resolver),
            10
        );
        assert_eq!(cell.total_items(), 10);
    }
    #[test]
    fn restoration_checks_physical_capacity_without_applying_new_policy() {
        let mut config = CellConfig::item(Tier::K1);
        config.filter = Some(Filter {
            mode: FilterMode::Whitelist,
            keys: [key(1)].into_iter().collect(),
        });
        config.equal_distribution = true;
        config.void_excess = true;
        let c = Cell::from_entries(config.clone(), [(key(0), 8128)]).unwrap();
        assert_eq!(c.count(&key(0)), 8128);
        assert!(matches!(
            Cell::from_entries(config.clone(), [(key(0), 8129)]),
            Err(RestoreError::OverCapacity)
        ));
        assert!(matches!(
            Cell::from_entries(config.clone(), [(key(0), 1), (key(0), 1)]),
            Err(RestoreError::DuplicateKey)
        ));
        assert!(matches!(
            Cell::from_entries(config, [(key(0), 0)]),
            Err(RestoreError::ZeroAmount)
        ));
    }
    #[test]
    fn bank_routes_remainders_and_simulation_is_read_only() {
        let mut bank = CellBank::new(vec![Cell::new_1k(), Cell::new_1k()]);
        assert_eq!(bank.insert(&key(1), 9000, Action::Simulate), 9000);
        assert_eq!(bank.cells()[0].total_items(), 0);
        assert_eq!(bank.insert(&key(1), 9000, Action::Modulate), 9000);
        assert_eq!(bank.cells()[0].count(&key(1)), 8128);
        assert_eq!(bank.cells()[1].count(&key(1)), 872);
        assert_eq!(bank.extract(&key(1), 9000, Action::Modulate), 9000);
    }
}
