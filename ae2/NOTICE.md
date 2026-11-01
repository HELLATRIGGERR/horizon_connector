# Source and content notices

This independent project builds on [Horizon](https://github.com/ludovic111/horizon)
and [Applied Energistics 2](https://github.com/AppliedEnergistics/Applied-Energistics-2).
Horizon copyright and third-party notices are preserved in
[LICENSES/Horizon-MIT.txt](LICENSES/Horizon-MIT.txt) and
[LICENSES/Horizon-THIRD-PARTY-NOTICES.md](LICENSES/Horizon-THIRD-PARTY-NOTICES.md);
the original license files for vendored libraries, fonts, and icons remain in
the engine tree. AE2's upstream notices are retained in
[LICENSES/AE2-NOTICE.txt](LICENSES/AE2-NOTICE.txt).

The generator uses AE2 commit `db17504a86128fdf3dae31f5fb7a112a646e0b93`.
AE2 textures and models are credited to © 2020 Ridanisaurus Rid and
© 2013–2020 AlgorithmX2 and contributors. Its upstream README identifies them as
[CC BY-NC-SA 3.0](LICENSES/CC-BY-NC-SA-3.0.txt), and text and translations as
[CC0 1.0](LICENSES/CC0-1.0.txt). It also credits the Guidebook click sound to
[EminYILDIRIM](https://freesound.org/people/EminYILDIRIM/sounds/536108/)
under [CC BY 4.0](LICENSES/CC-BY-4.0.txt). These upstream assets are generated
locally from the checkout, not included in the source export; the upstream
notices and asset terms still apply to the generated pack. Screenshots in this
repository depict rendered AE2 and Minecraft content; the depicted assets
retain their respective copyright and license terms.

Conventional tags are generated locally from the official
[NeoForge 21.1.259 universal JAR](https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.259/),
whose project uses [LGPL 2.1](LICENSES/NeoForge-LGPL-2.1.txt). The Guide compiler
uses the separately installed `marked` and `yaml` packages; their package
licenses accompany the local dependency installation. Minecraft resources are
prepared from the player's own installation and are not distributed here.

AE2 addons from All the Mods 10 are listed in [addons/](addons/). Code derived
from [MEGA Cells](https://github.com/62832/MEGACells) 4.11.0 (tag `4.11.0`,
commit `f46ab8f9a2c14f0de3e09241241b49fb2d06a81a`) is under its
[LGPL 3.0](LICENSES/AE2-LGPL-3.0.txt); the files are listed in
[addons/megacells/SOURCE_MANIFEST.json](addons/megacells/SOURCE_MANIFEST.json).
MEGA Cells' assets, by its README under
[CC BY-NC-SA 3.0](LICENSES/CC-BY-NC-SA-3.0.txt), are generated locally from the
player's own jar by `scripts/prepare-addon.ts` and are not distributed here.
