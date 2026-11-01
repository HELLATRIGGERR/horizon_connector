//! The definitions registry: what AE2's items are to the port's storage code,
//! and what each addon built in (`ae2-<id>` features) adds to it.
//!
//! Shaped after AE2's public API (`appeng.api`, MIT): `IBasicCellItem` and
//! `StorageCells` for cells, `IStorageComponent` for storage components,
//! `Upgrades.add` and `IUpgradeableItem` for upgrade cards on items,
//! `StorageCellModels` for the model a drive draws. AE2's own rows come first,
//! then each addon's; the tables are built once, on first use, and do not
//! change after. An addon whose content is not loaded leaves rows that no item
//! reaches.
use ae2_cell_core::{CellConfig, Channel, Tier};
use std::{collections::HashMap, sync::LazyLock};

/// A storage cell (`IBasicCellItem`).
#[derive(Clone, Debug, PartialEq)]
pub struct CellDef {
    pub id: String,
    /// `getKeyType`: items or fluids.
    pub channel: Channel,
    /// The tier: `getBytes` is its kilobytes × 1,024 (× 512 for a portable
    /// cell) and `getBytesPerType` its kilobytes × 8.
    pub tier: Tier,
    /// `getTotalTypes`.
    pub types: usize,
    /// `getIdleDrain`: AE/t while the cell sits in a drive or chest.
    pub idle_drain: f64,
    /// A `PortableCellItem`: half the bytes, a battery, a menu of its own.
    pub portable: bool,
    /// `StorageCellModels.registerModel`: the model drawn in a drive's slot.
    pub drive_model: String,
}
impl CellDef {
    /// The cell's storage layout, before its cards and partition.
    pub fn config(&self) -> CellConfig {
        if self.portable {
            CellConfig::portable(self.tier, self.channel, self.types)
        } else {
            CellConfig::basic(self.tier, self.channel, self.types)
        }
    }
}

/// The tables. Filled by [`Defs::ae2`] and the addons' `register`.
#[derive(Debug, Default)]
pub struct Defs {
    cells: HashMap<String, CellDef>,
    components: HashMap<String, Tier>,
    upgrade_slots: HashMap<String, usize>,
    upgrades: HashMap<(String, String), usize>,
}
impl Defs {
    /// A storage cell, with the upgrade inventory `BasicStorageCell` and
    /// `PortableCellItem` give it: four slots on an item cell, three on a
    /// fluid cell. Its cards are added with [`Defs::add_upgrade`].
    pub fn add_cell(&mut self, cell: CellDef) {
        let slots = if cell.channel == Channel::Item { 4 } else { 3 };
        self.upgrade_slots.insert(cell.id.clone(), slots);
        self.cells.insert(cell.id.clone(), cell);
    }
    /// A storage component (`IStorageComponent`) of `tier`: what a Condenser
    /// takes as its storage, `getBytes` × 8 power.
    pub fn add_component(&mut self, id: &str, tier: Tier) {
        self.components.insert(id.to_owned(), tier);
    }
    /// The size of an item's upgrade inventory (`IUpgradeableItem.getUpgrades`).
    pub fn set_upgrade_slots(&mut self, host: &str, slots: usize) {
        self.upgrade_slots.insert(host.to_owned(), slots);
    }
    /// `Upgrades.add(card, host, max)`: `host` takes up to `max` of `card`.
    pub fn add_upgrade(&mut self, card: &str, host: &str, max: usize) {
        self.upgrades.insert((card.to_owned(), host.to_owned()), max);
    }

    pub fn cell(&self, id: &str) -> Option<&CellDef> {
        self.cells.get(id)
    }
    pub fn cells(&self) -> impl Iterator<Item = &CellDef> {
        self.cells.values()
    }
    pub fn component(&self, id: &str) -> Option<Tier> {
        self.components.get(id).copied()
    }
    pub fn upgrade_slots(&self, host: &str) -> Option<usize> {
        self.upgrade_slots.get(host).copied()
    }
    /// `Upgrades.getMaxInstallable`: how many of `card` fit in `host`.
    pub fn upgrade_limit(&self, card: &str, host: &str) -> usize {
        self.upgrades
            .get(&(card.to_owned(), host.to_owned()))
            .copied()
            .unwrap_or(0)
    }

    /// AE2 19.2.18's rows: `AEItems`, `InitStorageCells` and the item hosts of
    /// `InitUpgrades`. Machines and parts keep their own tables for now.
    pub fn ae2() -> Self {
        let mut defs = Self::default();
        let tiers = [Tier::K1, Tier::K4, Tier::K16, Tier::K64, Tier::K256];
        for tier in tiers {
            let size = format!("{}k", tier.kilobytes());
            // StorageTier: 0.5 AE/t per index; a portable cell drains 0.5 whatever its tier.
            let drain = 0.5 * f64::from(tier.index());
            for (kind, channel, types) in [("item", Channel::Item, 63), ("fluid", Channel::Fluid, 18)] {
                let model = format!("ae2:block/drive/cells/{size}_{kind}_cell");
                defs.add_cell(CellDef {
                    id: format!("ae2:{kind}_storage_cell_{size}"),
                    channel,
                    tier,
                    types,
                    idle_drain: drain,
                    portable: false,
                    drive_model: model.clone(),
                });
                let portable_types = if channel == Channel::Item {
                    63 - 9 * tier.index() as usize
                } else {
                    18
                };
                defs.add_cell(CellDef {
                    id: format!("ae2:portable_{kind}_cell_{size}"),
                    channel,
                    tier,
                    types: portable_types,
                    idle_drain: 0.5,
                    portable: true,
                    drive_model: model,
                });
            }
            defs.add_component(&format!("ae2:cell_component_{size}"), tier);
        }
        // InitUpgrades, the item hosts.
        for cell in defs.cells.values().cloned().collect::<Vec<_>>() {
            add_storage_cell_cards(&mut defs, &cell);
            if cell.portable {
                defs.add_upgrade("ae2:energy_card", &cell.id, 2);
            }
        }
        for terminal in ["ae2:wireless_terminal", "ae2:wireless_crafting_terminal"] {
            defs.set_upgrade_slots(terminal, 2);
            defs.add_upgrade("ae2:energy_card", terminal, 2);
        }
        defs.set_upgrade_slots("ae2:view_cell", 2);
        defs.add_upgrade("ae2:fuzzy_card", "ae2:view_cell", 1);
        defs.add_upgrade("ae2:inverter_card", "ae2:view_cell", 1);
        defs.set_upgrade_slots("ae2:color_applicator", 2);
        defs.add_upgrade("ae2:energy_card", "ae2:color_applicator", 2);
        defs.add_upgrade("ae2:equal_distribution_card", "ae2:color_applicator", 1);
        defs.add_upgrade("ae2:void_card", "ae2:color_applicator", 1);
        defs.set_upgrade_slots("ae2:matter_cannon", 4);
        defs.add_upgrade("ae2:energy_card", "ae2:matter_cannon", 2);
        defs.add_upgrade("ae2:fuzzy_card", "ae2:matter_cannon", 1);
        defs.add_upgrade("ae2:inverter_card", "ae2:matter_cannon", 1);
        defs.add_upgrade("ae2:void_card", "ae2:matter_cannon", 1);
        defs.add_upgrade("ae2:speed_card", "ae2:matter_cannon", 4);
        // CreativeCellItem has a config inventory but no upgrades.
        defs.set_upgrade_slots("ae2:creative_storage_cell", 0);
        defs
    }
}

/// The cards every item and fluid storage cell takes (`InitUpgrades`): one
/// each of inverter, equal distribution and void, and fuzzy on item cells.
pub fn add_storage_cell_cards(defs: &mut Defs, cell: &CellDef) {
    for card in ["ae2:inverter_card", "ae2:equal_distribution_card", "ae2:void_card"] {
        defs.add_upgrade(card, &cell.id, 1);
    }
    if cell.channel == Channel::Item {
        defs.add_upgrade("ae2:fuzzy_card", &cell.id, 1);
    }
}

/// The registry: AE2's rows, then those of every addon built in.
pub fn get() -> &'static Defs {
    static DEFS: LazyLock<Defs> = LazyLock::new(|| {
        #[allow(unused_mut)]
        let mut defs = Defs::ae2();
        #[cfg(feature = "ae2-megacells")]
        super::megacells::register(&mut defs);
        defs
    });
    &DEFS
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rows answer as the `match`es they replaced did.
    #[test]
    fn ae2_rows_keep_the_item_hosts_answers() {
        let defs = Defs::ae2();
        for size in ["1k", "4k", "16k", "64k", "256k"] {
            for (id, slots, fuzzy, energy) in [
                (format!("ae2:item_storage_cell_{size}"), 4, 1, 0),
                (format!("ae2:fluid_storage_cell_{size}"), 3, 0, 0),
                (format!("ae2:portable_item_cell_{size}"), 4, 1, 2),
                (format!("ae2:portable_fluid_cell_{size}"), 3, 0, 2),
            ] {
                assert_eq!(defs.upgrade_slots(&id), Some(slots), "{id}");
                assert_eq!(defs.upgrade_limit("ae2:fuzzy_card", &id), fuzzy, "{id}");
                assert_eq!(defs.upgrade_limit("ae2:energy_card", &id), energy, "{id}");
                for card in ["ae2:inverter_card", "ae2:equal_distribution_card", "ae2:void_card"] {
                    assert_eq!(defs.upgrade_limit(card, &id), 1, "{card} on {id}");
                }
                assert_eq!(defs.upgrade_limit("ae2:speed_card", &id), 0);
            }
            assert!(defs.component(&format!("ae2:cell_component_{size}")).is_some());
        }
        assert_eq!(defs.upgrade_slots("ae2:matter_cannon"), Some(4));
        assert_eq!(defs.upgrade_limit("ae2:speed_card", "ae2:matter_cannon"), 4);
        assert_eq!(defs.upgrade_limit("ae2:equal_distribution_card", "ae2:matter_cannon"), 0);
        assert_eq!(defs.upgrade_slots("ae2:color_applicator"), Some(2));
        assert_eq!(defs.upgrade_limit("ae2:fuzzy_card", "ae2:color_applicator"), 0);
        assert_eq!(defs.upgrade_slots("ae2:view_cell"), Some(2));
        assert_eq!(defs.upgrade_limit("ae2:inverter_card", "ae2:view_cell"), 1);
        assert_eq!(defs.upgrade_limit("ae2:energy_card", "ae2:wireless_crafting_terminal"), 2);
        assert_eq!(defs.upgrade_slots("ae2:creative_storage_cell"), Some(0));
        assert_eq!(defs.upgrade_slots("ae2:item_storage_cell_9k"), None);
        assert!(defs.cell("ae2:item_storage_cell_9k").is_none());
    }

    #[test]
    fn ae2_cells_have_their_source_layouts() {
        let defs = Defs::ae2();
        assert_eq!(defs.cells().count(), 20);
        let cell = defs.cell("ae2:item_storage_cell_256k").unwrap();
        assert_eq!(cell.config(), CellConfig::item(Tier::K256));
        assert_eq!(cell.idle_drain, 2.5);
        assert_eq!(cell.drive_model, "ae2:block/drive/cells/256k_item_cell");
        let cell = defs.cell("ae2:fluid_storage_cell_4k").unwrap();
        assert_eq!(cell.config(), CellConfig::fluid(Tier::K4));
        let cell = defs.cell("ae2:portable_item_cell_64k").unwrap();
        assert_eq!(cell.config(), CellConfig::portable_item(Tier::K64));
        assert_eq!((cell.idle_drain, cell.drive_model.as_str()), (0.5, "ae2:block/drive/cells/64k_item_cell"));
        assert_eq!(defs.cell("ae2:portable_fluid_cell_1k").unwrap().config(), CellConfig::portable_fluid(Tier::K1));
        assert_eq!(defs.component("ae2:cell_component_16k"), Some(Tier::K16));
    }
}
