//! MEGA Cells 4.11.0's rows in the definitions registry (feature
//! `ae2-megacells`). Derived from MEGA Cells 4.11.0 (`MEGAItems`,
//! `MEGACells.initUpgrades`, `MEGACellsClient.initStorageCellModels`),
//! https://github.com/62832/MEGACells tag 4.11.0, LGPL-3.0; see
//! `ae2/addons/megacells/`.
//!
//! Slice 1: the M-tier item and fluid storage cells and their components.
//! The portable M cells, the bulk cell and the cells for other addons' key
//! types come with later slices.
use super::defs::{CellDef, Defs, add_storage_cell_cards};
use ae2_cell_core::{Channel, Tier};

pub const NAMESPACE: &str = "megacells";

/// `MEGAItems.TIER_1M` to `TIER_256M`: `tier(index, component)` with
/// 4^(index - 1) KiB, so 1M is index 6.
pub const TIERS: [Tier; 5] = [Tier::M1, Tier::M4, Tier::M16, Tier::M64, Tier::M256];

/// `StorageTier.namePrefix()`: "1m" to "256m".
pub fn size(tier: Tier) -> String {
    format!("{}m", tier.kilobytes() >> 10)
}

pub fn register(defs: &mut Defs) {
    for tier in TIERS {
        let size = size(tier);
        // MEGAItems.component: StorageComponentItem(mb * 1024 KiB).
        defs.add_component(&format!("{NAMESPACE}:cell_component_{size}"), tier);
        // MEGAItems.itemCell / fluidCell: BasicStorageCell(idleDrain 0.5 * index,
        // tier.bytes() / 1024 KiB, tier.bytes() / 128 per type, 63 or 18 types).
        for (kind, channel, types) in [("item", Channel::Item, 63), ("fluid", Channel::Fluid, 18)] {
            let cell = CellDef {
                id: format!("{NAMESPACE}:{kind}_storage_cell_{size}"),
                channel,
                tier,
                types,
                idle_drain: 0.5 * f64::from(tier.index()),
                portable: false,
                // MEGACellsClient.initStorageCellModels.
                drive_model: format!("{NAMESPACE}:block/drive/cells/{size}_{kind}_cell"),
            };
            // MEGACells.initUpgrades: inverter, equal distribution and void on
            // every item and fluid cell, fuzzy on item cells.
            add_storage_cell_cards(defs, &cell);
            defs.add_cell(cell);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ae2_cell_core::CellConfig;

    #[test]
    fn m_tier_cells_have_megas_layouts_cards_and_models() {
        let mut defs = Defs::ae2();
        register(&mut defs);
        assert_eq!(defs.cells().count(), 30);
        for (tier, size, drain) in [
            (Tier::M1, "1m", 3.0),
            (Tier::M4, "4m", 3.5),
            (Tier::M16, "16m", 4.0),
            (Tier::M64, "64m", 4.5),
            (Tier::M256, "256m", 5.0),
        ] {
            let item = defs.cell(&format!("megacells:item_storage_cell_{size}")).unwrap();
            assert_eq!(item.config(), CellConfig::item(tier));
            assert_eq!(item.idle_drain, drain);
            assert_eq!(item.drive_model, format!("megacells:block/drive/cells/{size}_item_cell"));
            let fluid = defs.cell(&format!("megacells:fluid_storage_cell_{size}")).unwrap();
            assert_eq!(fluid.config(), CellConfig::fluid(tier));
            assert_eq!(defs.upgrade_slots(&item.id), Some(4));
            assert_eq!(defs.upgrade_slots(&fluid.id), Some(3));
            assert_eq!(defs.upgrade_limit("ae2:fuzzy_card", &item.id), 1);
            assert_eq!(defs.upgrade_limit("ae2:fuzzy_card", &fluid.id), 0);
            assert_eq!(defs.upgrade_limit("ae2:void_card", &fluid.id), 1);
            assert_eq!(defs.upgrade_limit("ae2:energy_card", &item.id), 0);
            assert_eq!(defs.component(&format!("megacells:cell_component_{size}")), Some(tier));
        }
        assert_eq!(defs.cell("megacells:item_storage_cell_256m").unwrap().config().total_bytes(), 1 << 28);
    }
}
