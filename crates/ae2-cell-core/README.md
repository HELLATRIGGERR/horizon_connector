# Experimental AE2 storage-cell core

Derived from Applied Energistics 2, branch `1.21.1`, commit
`db17504a86128fdf3dae31f5fb7a112a646e0b93`:
https://github.com/AppliedEnergistics/Applied-Energistics-2/tree/db17504a86128fdf3dae31f5fb7a112a646e0b93

Sources: `src/main/java/appeng/me/cells/BasicCellInventory.java`,
`src/main/java/appeng/core/definitions/AEItems.java`,
`src/main/java/appeng/items/storage/BasicStorageCell.java`, and
`src/main/java/appeng/api/stacks/{AEKeyType,AEItemKey,AEFluidKey,AEFluidKeys}.java`.
AE2 code is LGPL-3.0-or-later; this derived implementation uses the same terms,
see LICENSE. AE2 textures/models have separate CC BY-NC-SA 3.0 terms and are not
included in this crate.

Supports 1K, 4K, 16K, 64K, and 256K item/fluid cells, pooled byte accounting,
partition whitelist/blacklist with default fuzzy IGNORE_ALL, equal distribution and void upgrades, and
simulation. Fluid units follow this upstream NeoForge branch: 1000 per bucket.
`CellBank` is a simple host-ordered collection, not AE2 network scheduling.
`Cell::from_entries` validates physical saved contents without applying current
filters or upgrades; calling insert to restore can lose existing contents.

This library is only a storage-cell subsystem. The host must enforce channel,
item blacklists, nonempty nested cells, canonical Minecraft component equality,
and persistence. Percentage-based fuzzy cell filters, upgrade inventory rules, network routing and
priorities, grid, channels, energy, recipes, machines, autocrafting, world/UI,
and the rest of AE2 are not implemented here. The Java differential harness
extracts the actual upstream accounting and mutation methods and compares
against minimal stubs, not a running Minecraft or AE2 installation. Run
`python3 tools/differential.py` with Java, Rust, and adjacent `ae2-reference`.
