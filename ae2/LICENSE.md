# License overview

This repository combines code from separately licensed projects. The original
Horizon code remains under the [MIT license](LICENSES/Horizon-MIT.txt). The
AE2-derived integration in the client and the AE2 core crates is distributed
under the [GNU Lesser General Public License, version 3 or later](LICENSES/AE2-LGPL-3.0.txt),
with the accompanying [GPL version 3 text](LICENSES/GPL-3.0.txt). Client package
metadata records both licenses as `MIT AND LGPL-3.0-or-later`. Upstream AE2's API
has a separate [MIT license](LICENSES/AE2-API-MIT.txt).

New general-purpose project tooling, including the resource and content
bootstrap, is MIT licensed. The AE2-derived catalog and content projection and
Guide compiler (`scripts/ae2-content-catalog.json`, `scripts/sync-ae2-content.ts`,
and `scripts/guidebook/`) are provided under LGPL-3.0-or-later.

The local content generator retrieves additional upstream materials; their
licenses are described in [NOTICE.md](NOTICE.md). Generated content, upstream
checkouts, downloaded archives, and the player's Minecraft resources are not
part of this source distribution. Their applicable terms continue to apply
when they are obtained or used.
