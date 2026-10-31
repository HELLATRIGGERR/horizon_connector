# Experimental AE2 machine core

Source-derived Rust experiment based on Applied Energistics 2 branch 1.21.1,
commit `db17504a86128fdf3dae31f5fb7a112a646e0b93`:
https://github.com/AppliedEnergistics/Applied-Energistics-2/tree/db17504a86128fdf3dae31f5fb7a112a646e0b93

Sources: `parts/automation/IOBusPart.java`, `ImportBusPart.java`,
`ExportBusPart.java`, `StackTransferContextImpl.java`, `StorageImportStrategy.java`,
`StorageExportStrategy.java`, `UpgradeablePart.java`, `AbstractLevelEmitterPart.java`,
`parts/storagebus/StorageBusPart.java`, `api/storage/StorageHelper.java`, and
`core/settings/TickRates.java` under `src/main/java/appeng`. Source and this
implementation are LGPL-3.0-or-later; see LICENSE. No AE2 textures/models included.

`BusState` serializes through serde; the host must persist it and invoke ticks in
world time. Source timing bounds are 5–40 ticks for import and 5–60 for export;
this library returns wake/sleep modulation only and does not schedule ticks.
The host supplies item/fluid identity, authoritative adjacent/network inventories,
Minecraft fuzzy equivalence, active grid/channel and loaded-chunk state. Export
requires powered extraction; the host charges energy atomically with transfer.
The upstream implementation attempts rollback on post-simulation mismatches and
can log discarded overflow; this API instead requires an atomic host transfer
that never loses or duplicates items. A successful earlier transfer within a tick
is not rolled back if a later host transfer errors.

Implemented: item/fluid import and export budgets, exact and host-provided fuzzy
filters, inversion, redstone signal and persisted pulses, speed and capacity
cards, default/round-robin export scheduling, partial storage bus access/filter
policy, emitter threshold output, formation-plane filter/placement host contract,
annihilation result scheduling, and energy affordability calculation. `Key`
converts to/from cell `ItemKey`; both require host canonicalization. A `StorageBus`
policy does not mount storage or implement network priorities itself.

Not implemented: random scheduling, card-count validation, actual Minecraft
fuzzy rules, autocrafting requests, world block/entity APIs and actual formation placement or annihilation pickup, ME interface, true energy grid/multiplier, routing,
network scheduling or game integration. The cell core and these policies do not
constitute an AE2 port. Tests use a transactional in-memory host, not Minecraft.
