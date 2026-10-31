# Project status

This is an experimental, unofficial native integration. The target is AE2 19.2.18 behavior, but the port is incomplete. This page records the scope of the current checkpoint rather than certifying full compatibility.

## Reference versions

| Component | Pinned reference |
| --- | --- |
| Applied Energistics 2 | 19.2.18 for Minecraft 1.21.1, commit [`db17504a86128fdf3dae31f5fb7a112a646e0b93`](https://github.com/AppliedEnergistics/Applied-Energistics-2/commit/db17504a86128fdf3dae31f5fb7a112a646e0b93) |
| Horizon | Minecraft 26.3, commit [`7f8a6680a88f36a3c4a499f821862c447b72ce3f`](https://github.com/ludovic111/horizon/commit/7f8a6680a88f36a3c4a499f821862c447b72ce3f) |

Horizon and AE2 target different Minecraft versions here. The port maps selected AE2 behavior into Horizon; it does not load the upstream Java mod.

## Current checkpoint and evidence

The latest completed host checkpoint and its binary/source hashes are recorded in [VALIDATION.md](VALIDATION.md). Tests are evidence for their named scenarios, not a percentage of AE2 completed. The three requirement ledgers separate implementation, physical/source checks, and open work:

- [Storage, crafting and network](parity-storage-crafting.md)
- [Machines, links and progression](parity-machines-links.md)
- [Menus, Guide and items](parity-ui-items.md)

The implementation now has source-format readers/writers for all 35 persistent AE2 component identifiers and a command/transport bridge. Specific native-world checks cover source-only cells and patterns, recoverable missing items, recipe identity, physical upgrades and Memory Cards, wireless ownership, spatial plot reuse, quantum chamber/hopper callbacks, large crafting previews and restart. Component registration or round-trip transport alone does not establish the consumer's full behavior.

Selected local in-frame menus now work: Provider block/part, ME/Crafting/Encoding terminals, amount/confirmation and CPU status. The normal server-thread runtime has broader menu coverage. Physical crafting, refill, drag, source pattern encoding, search operators, owner-only notifications and cursor custody have named hidden regressions.

A private copy of the existing 31-node native hall retained seven emeralds, sixteen iron, 256 sticks and power through another restart. Its source save was command-seeded for integration tests; this is not a survival playthrough or Java save import. The README's original hall images belong to the prior checkpoint unless a caption explicitly states otherwise.

The initial public Stage 83 export is retained in Git history: its coordinated gate reported 80 library and 245 client tests passing, with three library tests ignored. Later gates and source changes supersede those counts; the earlier totals are not current working-tree validation.

## Known boundaries

- **Full parity is unfinished.** Concrete remaining requirements and verification gaps are in the ledgers. The Wave 17 checkpoint verifies selected mixed/overfilled source cells, recipe-aware machine menus, component-preserving native block/fluid Formation placement and authoritative repository clicks. Remaining work includes cross-dimension and non-BlockItem Formation, CPU finish/cancel/idle inventory, wireless lifecycle, source crafting/fluid gestures and further source callback details. Each later change needs its own gate.
- **No Java mod loader or modpack support is implied.** This native integration is not an upstream `.jar` or `.hmod`; ATM10 and other Java modpacks are unsupported.
- **Java-world migration has not been established.** Horizon and the pinned AE2 reference target different Minecraft versions. Do not open an existing Java save as an experiment without a separate preserved copy.
- **Extension and platform coverage is bounded.** Arbitrary NeoForge custom ingredients, third-party key/container capabilities, unsupported addon integrations, every multiplayer timing and every operating system are not certified by these macOS tests.
- **The alternate local runtime remains partial.** Several nonterminal machine/part menus, hotkeys, client settings presentation and source UI details still need implementation or comparison. The default integrated-server path remains the tested play path.
- **Planner safeguards and callback coverage remain explicit.** Native allocation limits and unusual lifecycle sequences remain explicit. The former recursive depth and fixed node/step guards were replaced and validated in the Wave 11 checkpoint. Source stubs do not replace a live Java reference world.

All automated native clients use isolated saves, invisible/unfocused windows, virtual input and a shared rendering lease at 30 FPS. They do not occupy or capture the real desktop. Runtime binaries, logs, test worlds and generated resources remain local and ignored.

## Source and content

The modified Horizon workspace is in `ludovic111-horizon-7f8a668/`; the five `ae2-*-core` directories contain native supporting crates. Runtime content and Minecraft resources are generated locally, not checked in as game files. See the [README](../README.md) for preparation commands and [NOTICE.md](../NOTICE.md) for upstream attribution and asset terms.

