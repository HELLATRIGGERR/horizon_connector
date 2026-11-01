# MEGA Cells 4.11.0

[MEGA Cells](https://github.com/62832/MEGACells) by 90, as All the Mods 10 ships it. Code LGPL-3.0
(tag `4.11.0`, `LICENSE`; `LICENSE_megacells` in the jar), so the port's derived files carry the same
terms; assets CC BY-NC-SA 3.0, generated locally from the player's jar and never committed.
Provenance in [SOURCE_MANIFEST.json](SOURCE_MANIFEST.json), the declared content in
[content.json](content.json).

Feature `ae2-megacells`; content `content/megacells` from
`ATM10_DIR=... bun run ae2/scripts/prepare-addon.ts megacells`.

## Slice 1: M-tier item and fluid cells

| MEGA | Port |
|---|---|
| `item_storage_cell_{1,4,16,64,256}m`, `fluid_storage_cell_*m`: `BasicStorageCell(0.5 × index AE/t, tier KiB, tier / 128 bytes per type, 63 or 18 types)` | registry rows (`ae2x/megacells.rs`) on `Tier::M1`..`Tier::M256`; contents and cards in AE2's own components (`ae2:storage_cell_inv`, `ae2:upgrades`, ...), as MEGA keeps them |
| `initUpgrades`: inverter, equal distribution, void on every cell, fuzzy on item cells; 4 card slots on item cells, 3 on fluid cells | the same rows |
| `initStorageCellModels`: `megacells:block/drive/cells/<size>_<kind>_cell` | the drive draws that model |
| `cell_component_*m`: `StorageComponentItem(mb × 1024 KiB)` | storage-component rows: a Condenser takes them |
| Sky Steel and Sky Bronze from `ae2:transform` in lava; `ItemEntityMixin` keeps the ingredients alive for up to 200 ticks | AE2's transform logic, and `game/ae2x/megacells/lava.rs` for the immunity |
| housings, components, cells (shaped and with housing), disassembly, inscriber press and processor chain, Sky Steel and Sky Bronze blocks | MEGA's own 47 recipes, copied as they are |
| Sky Steel and Sky Bronze ingots and blocks are fire resistant | `minecraft:damage_resistant` on the declared items |

Differences, known:

- `LavaTransformLogic.allIngredientsPresent` answers for the first lava recipe the recipe manager lists,
  in no fixed order; the port answers for any lava recipe.
- No tooltip of cell bytes and types yet (none for AE2's cells either), and no status colour on the cell item's LED.
- MEGA's guide pages, recipe advancements and its files under AE2's namespace come with later slices.

Tests: `ae2-cell-core` (M-tier byte maths, 256M at 2,130,706,432 items of one type), `horizon-client`
(registry rows, the cell codec on MEGA cells, disassembly, condenser, lava immunity) and the world test
`megacells-cells`: Sky Steel from lava, a 1M cell crafted by MEGA's recipe, a 256M item cell full at
2,130,706,432 items and a 1M cell at 63 types in an ME Drive, a 256M fluid cell full at 2,130,706,432
buckets and a 1M fluid cell at 18 types in an ME Chest, both listed in terminals, all through a restart:

```sh
AE2_TEST_STARTUP_MS=600000 bun run ae2/scripts/test-world.ts megacells-cells
```

## Later slices (`.build/ae2-addons-plan.md` §5.1)

2. MEGA Crafting Storage 1M–256M, Crafting Unit, Co-Processing Unit, Crafting Monitor.
3. Superdense Energy Cell, Greater Energy Card.
4. Portable M cells (`CellConfig::portable` is ready; MEGA's type count is `18 + (index - 5) × 9`), Portable Cell Workbench.
5. MEGA Pattern Provider and Interface, block and part.
6. ME Cell Dock.
7. Bulk Item Cell, Compression Card, Decompression Module.
8. Cells for other addons' key types (chemical, radioactive, source, soul), with those addons.

`neoforge:mod_loaded` recipe conditions: not needed by slice 1 (its recipes have none); the 107
conditional recipes all belong to cells of other addons and are left out by the content script.
