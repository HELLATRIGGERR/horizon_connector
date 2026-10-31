# Contributing

Open issues and pull requests in this repository for the native AE2 integration. Include the source commit, Rust version, operating system, reproduction steps, and whether the normal integrated server or the alternate in-frame path was used. Remove private paths and account details from logs.

Keep gameplay in Rust. Follow the pinned AE2 source for source-derived behavior, preserve its attribution and LGPL obligations, and describe the source class/method or documented behavior behind a change. This port has its own contribution policy; the retained Horizon contribution document describes its upstream project.

Use focused regression tests for behavioral changes. `bun run scripts/test.ts` runs the standalone cores, and `bun run scripts/test.ts --client` also exercises the AE2 client/host tests. World-level changes need a reproducible world check; passing catalog or helper tests does not establish complete AE2 parity.

Run unattended clients with `--background`, isolated profiles, no system pointer/clipboard/controller interaction, and one rendering fixture at a time. Do not open a visible client unless the user requested it. Keep rendering capped at 30 FPS for automated fixtures.

Never commit Minecraft JARs, prepared resources, decompiled game code, downloaded AE2 assets, generated content, personal worlds, profiles, secrets, or local evidence dumps. Regenerate local content using the documented preparation scripts.

Use clear commits, retain third-party notices, and include an accurate `Co-Authored-By` trailer when an agent contributed. Report what was checked and any remaining limitations; do not present this project as official AE2 or as ATM10 compatible.
