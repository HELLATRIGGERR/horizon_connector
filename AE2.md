# This fork: Applied Energistics 2 on Horizon

This branch is Horizon with a native Rust port of Applied Energistics 2. It is not an official Horizon or AE2 release, and neither project supports it. Full AE2 parity is unfinished.

The port comes from [AdamAugustinsky/ae2-horizon](https://github.com/AdamAugustinsky/ae2-horizon), commit `345f7dc`. That repository keeps Horizon as a copied folder, so it can't merge upstream. Here the same changes sit on Horizon's real history.

## Where things are

| Path | What it holds |
|---|---|
| `crates/horizon-client/src/ae2_*.rs`, `crates/horizon-client/src/game/ae2_*.rs` | The port's gameplay, menus and rendering |
| `crates/ae2-*-core/` | AE2 logic that doesn't depend on the engine, each crate with its own tests |
| `ae2/scripts/` | Preparing resources and content, building, playing, testing (Bun) |
| `ae2/tests/world/` | Tests that run in a hidden game |
| `ae2/docs/` | Status, validation and the parity ledgers |
| `ae2/addons/`, `crates/horizon-client/src/ae2x/`, `crates/horizon-client/src/game/ae2x/` | AE2 addons from All the Mods 10 (MEGA Cells so far), one Cargo feature each, and the definitions registry they add to |

The ledgers and `ae2/SOURCE_MANIFEST.json` record the import: where they name `ludovic111-horizon-7f8a668/`, read the repo root, and `ae2-*-core/` is now `crates/ae2-*-core/`.

## Licenses

Horizon's own code stays under MIT. The code derived from AE2, including the `ae2_*` files and `crates/ae2-*-core`, is under LGPL-3.0-or-later. AE2's textures and models are under CC BY-NC-SA 3.0, so they can't be used commercially; they are generated locally and not stored here. See `ae2/LICENSE.md` and `ae2/NOTICE.md`.

Horizon doesn't accept code derived from LGPL projects, so this port can't go upstream. A hook proposed to Horizon has to be written without using this code.
