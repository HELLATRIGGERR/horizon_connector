# Native validation checkpoint

The latest completed host gate recorded here is Wave 17g on macOS / Apple M3 Pro. The source target is AE2 19.2.18 at `db17504a86128fdf3dae31f5fb7a112a646e0b93`; the engine target is Horizon / Minecraft 26.3 at `7f8a6680a88f36a3c4a499f821862c447b72ce3f`.

This is bounded evidence, not certification of full AE2, Java-world or ATM10 compatibility. Later working-tree changes need their own gate. The parity ledgers distinguish implementation, named tests and remaining work.

## Reproducible inputs

- Client binary SHA-256: `cdffc01040a2e926e6cbd795f618c1bbccbe49b89e0f5978988118f7f5d57774`.
- Frozen source archive SHA-256: `108e2eb669a1c12e7bab59ba28b69c7307249c4157b6640abfa79f90a5ca9f9b`.
- Rust 1.95.0, locked dependencies, `fast` profile, `horizon-client`, default features disabled, `ae2-lab` enabled.
- Runtime content generated locally from the pinned source and owned Minecraft resources. Prepared assets, saves and binaries are excluded from the repository.

## Automated checks

The Wave 17 cohort passed 1,341 library tests (32 ignored) and 459 client-host tests (three ignored). The library gate ran on 17d; the subsequent 17e–17g fixes affect only the client-host modules and their tests. The final client-host gate ran on 17g. Their AE2-named subsets contain 113 and 332 passes. Sixteen live cell-core tests passed separately. The three resource-backed Formation/shape/context checks passed with the actual prepared resources; resource absence was not counted as a pass. Earlier source registry and crafting-core checks retain their separate checkpoint scope below.

Actual pinned Java tree classes compiled against test-only host stubs agree with the native byte-accounting cases, including cake (68 bytes), reusable containers (34), crafted substitutes, source-greedy shortages, and four 10,000-push cases (60,016 or 70,016 bytes). These stubs do not represent a running Java game. Separate source oracles cover pathing on 128 synthetic graphs, stored-energy numeric boundaries, and seven actual QuantumCluster/Locatables readiness, pairing and unload/reload states.

## Physical world evidence

The retained fixtures exercise physical items and server/client transactions. Passing cases across the immutable development builds include:

- Source-only item/fluid cells and named patterns, Missing Content recovery, real consumption, output and restart; a named recipe with a stale encoded result selects and produces the current recipe's diamond.
- Mixed patterns, crafted substitutes, cake/container remainders, CPU capacity rejection, asynchronous export-bus calculation and card-removal invalidation, owner-only job events with a second peer, cancellation and restart.
- A 10,000-push confirmation preview at 60,016 bytes with all uncommitted physical stock preserved. It is a planner/custody test, not a 40,000-item execution benchmark.
- Source Memory Cards for machine/part settings, compatible generic imports, physical upgrade movement, Provider blank consumption, styled names, P2P frequency/LEDs and restart.
- Interface fluid target/stock limited to the source's registered 4,000 mB, real item stack maxima, four filled buckets accepted and the fifth retained. The earlier Wave 7 unlimited-fluid test was retracted after finding `useRegisteredCapacities`; it is not parity evidence.
- Explosion-produced source singularities, real Quantum chamber menus, hopper insertion/extraction callbacks and two-frame restart; source SpatialPlotInfo formatting, plot reuse, legacy native cells and retained emeralds.
- Fluid View Cell filtering with distinct component-bearing water entries; direct wireless link/energy/crafting components, key-type selections and restart.
- Hidden in-frame Provider, ME and Crafting terminals, physical matrix/refill/drag, encoding, search, clear-on-close, confirmation and job status. Other local menus remain separately bounded.
- A private copy of the existing 31-node native hall retains seven emeralds, sixteen iron and 256 sticks, plus power, through another restart. This checks a prior native save, not a Java save migration.

The earlier wrapped Workbench test used an AE2-screen creative clone that the source client prohibits. That pass was retracted. The corrected source-valid rapid pickup/ghost/custody/restart fixture passed on Wave 11; backend component-preserving conversion is separately unit-tested.

Some failed runs exposed implementation defects; others exposed stale client observations, an incorrect unordered-NBT assertion, or physically blocked rays. Corrected fixtures were rerun on the same immutable binary where applicable. Unresolved failures remain in the parity ledgers and are not counted as passing.

## Screen isolation

Every automated native client uses `--background`, an isolated game/profile directory, virtual input and a 30 FPS cap. A cross-process lease serializes rendering, including LAN peers. The harness asserts invisible, unfocused, windowed operation and no system-pointer capture before issuing test actions. Framebuffer captures do not read the real desktop.

Run `bun run scripts/test.ts --client` for the host gate and `bun run scripts/test-world.ts SUITE ...` for selected worlds. The world runner supports `--build METADATA.json` to pin binary SHA-256, `--content DIRECTORY`, and `--in-frame` for supported alternate-runtime cases. Local metadata, logs, transcripts and disposable saves stay under ignored `.local/`.

Wave 11 also passed actual three-frame first-registration pairing, unload without promoting the third frame, and original-pair reconnection. The old 128-depth, 4,096-node and CPU-step guards were replaced by an explicit continuation machine, interned nodes and fallible allocations; 256 source-Java layers, 4,097 nodes and 4,097 steps have differential cases. A real 4,097-step job and component-bearing escrow round-trip through NBT, using a ByteArray above the old modified-UTF-8 String boundary. Processing amount and Interface stock editors have physical Set/Back, per-item limits, restart and stale-owner checks.

On macOS, harness deadlines, control waits and shutdown timers use `mach_absolute_time`, matching awake-time progression; sleep pauses a budget and never causes an action to be resent. No display assertion is used. Lid closure can still suspend the machine.

## Wave 12: menu ownership, fake slots and physical container/fluid custody

The immutable native binary SHA-256 is `dd0779ac13ba4b8c431d9b09c4ff9c2ad7ec2e79f637f4223ca14c96129527b4`; its source archive is `baf5bf23bfd4e6ded848c0adf7beab594a86334ab43e733f24a389c44169ae94` and patch against `9a065b746dc99e392749428ea007db1afe6a9aa4` is `4adc4f196aaf758020fe8d5ce48f675ec32a300072916e9eb351fe98efe37fd1`. The frozen host gate passed 106 library tests and 316 client tests; the separate wrapped-item gate passed seven tests. Three resource-gated checks also passed when explicitly run against the actual pinned Java recipe export and source advancement data: 491 AE2 recipe IDs/special flags, synchronized recipe IDs, and 407 advancements including 381 recipe-unlock criteria.

Eleven hidden server-thread worlds passed: `machine-menu-identity`, `encoding-fake-drag`, `fake-slot-drag-machines`, `encoding-fast-clicks`, `encoding-fake-gestures`, `wrapped-workbench`, `annihilation-container-custody`, `annihilation-container-overflow`, `source-wrapped-crafting-input`, `provider-condenser-fluid`, and `storage-bus-condenser-fluid`. The Encoding drag and rapid same-slot processing cases also passed in-frame. These cover exact physical cursor/cell/card custody, stale host/part closure, item/fluid ghost actions, source wrapped-input crafting, chest/furnace pickup, partial cell acceptance with overflow left in the world, Condenser ME-capability routing, blocked fluid escrow and restart.

The first machine-drag fixture expected an Interface-specific title although the current renderer uses `AE2 Machine`. The first rapid-quantity fixture incorrectly expected more than one item in crafting mode, which source `fixCraftingRecipes` clamps; its queued-amount assertion now uses processing mode. The first Storage Bus fixture held the server while waiting for inventory replication. One in-frame run issued setup writes before its target chunk loaded, so its power source was never placed. These setup/assertion failures were corrected and rerun on the same binary. Setup now waits for the relevant loaded chunks; the harness immediately rejects an unloaded-position response to a block mutation. No failed setup or unexecuted gesture is counted as a passing behavior.

Two additional actual-source method oracles passed: eight wrapped input conversion cases and thirteen Provider/Condenser cases. They use minimal host stubs, so they do not replace a live Java world comparison. Full AE2 parity remains unfinished, including the next aggregate processing-queue integration and broader block-entity/menu coverage.

## Wave 13b: aggregate reservations and ordered Provider queues

The source archive above was built against `9a065b746dc99e392749428ea007db1afe6a9aa4`; its patch SHA-256 is `0179b5fa5c8efe6fe5563575d06be40cad3788e72176d1efb9b144ec8d253b0f`. The host gate includes queue migration, mixed item/fluid order, partial retries, stack-size-aware drop boundaries and CPU restart/cancellation cases. Legacy queue readers retain opaque fluid bytes. The cable-block additional-drop boundary and non-unit crafting inputs remain separate source gaps.

Four new hidden server-thread fixtures passed: Encoding fake hotkeys, Interface fake hotkeys, a 999,999-item physical processing reservation/queue, and a separate creative-cell grid reached through an unconfigured Interface. The finite job checks exact component identity, a lower-priority multi-stack chest drained before a cell, CPU escrow restart and cancellation, a blocked Provider queue, queue restart and retry of only the accepted 64 items. The creative case verifies the physical remote-grid pass-through and pending quantity after restart. The source large amount is not represented as millions of physical dropped entities.

The existing fluid-template, CPU-capacity and reusable-container worlds also passed on this binary; Encoding fake hotkeys passed in-frame. A new physical save fixture creates a valid component-bearing ingredient whose active CPU job exceeds 65,535 bytes. It checks the real NBT ByteArray and the existence of index 65,535 before and after a full world restart, then cancels and verifies the exact ingredient after another restart. Its first length assertion incorrectly counted the deliberately truncated `/data get` chat; the corrected authoritative index check passed on the same binary. This extends the earlier 4,097-step NBT unit test with real region/save/reload custody; it is not a 4,097-step physical execution test.

## Wave 14: container lifecycle and local machine menus

Binary `6a1ba6f01c2e506c13d8547fe065302f98c04cf8d4d8ba371701da0724f72ee2` was built from source archive `4f8067f7f733ac6a45ccc09f6072cde60c52c9f7d5a43877250398563a65fd3f` against `58bcf32433de2325b57388803c1eab9942480a42`. The gate passed 110 library and 320 client tests. Three explicitly selected tests passed for vanilla metadata incarnation ownership, live-name save projection, and source cable-block additional-drop ordering.

Eleven hidden worlds passed: local machine families and wireless access point; broader Annihilation container/energy pickup; shulker metadata edits, same-state clone and replacement; bees; Spatial container metadata; charged/named cell wrench recovery; running CPU fluid escrow recovery; and server Interface editor, machine ghost/physical dragging and wireless regressions. Metadata belongs to the current native BE incarnation and follows load/save, ordinary data edits and supported Spatial moves. These checks do not establish Java-save migration. The source-only stonecutting and smithing fixture also passed on Wave 13b, including physical assembler execution and the base sword's styled name.

## Wave 15: source input order, bounded CPU drops and client custody

The current binary and source hashes appear above; patch SHA-256 is `ccfd7b6d8afa3fb3ace7cafa60040d9ce21ea19a7736db7c1e929f92e3a0898b` against `58bcf32433de2325b57388803c1eab9942480a42`. Two additional library wire checks passed, including all 35 AE2 component IDs. The live standalone crafting core passed 83 tests, and actual pinned Java tree, wrapper/Input-constructor, assembler and item-drop method oracles passed with test-only host stubs.

The six immutable-14 before-cases reproduced real defects in per-push mixed input shape, non-unit source-pattern planning, oversized CPU-break entities, missing client item components, ignored item block-state properties, and stale Pattern Access ownership. All passed their Wave 15 after-cases. A separate mixed item/fluid processing world passed on both builds: fluid reaches the Condenser first, fills its output, and leaves the later exact item queued across restart.

The CPU-break world now emits 1,001 max-16 stone stacks from a 999,999-item escrow and separately retains all 65 max-8 dirt items after restart. The omitted excess follows the actual source `AEItemKey.addDrops` cutoff. Non-unit patterns calculate source byte counts and retain physical escrow; native preflight deliberately avoids the actual Java assembler's observed partial mutation followed by an exception. This safety difference remains explicit.

Eleven named scenarios passed on Wave 15, with the bus fixture rerun after correcting its virtual gesture: mixed pushes, non-unit pattern custody, large CPU break, real client component round trip, item block-state placement, Pattern Access ownership, emitter fake actions, local bus, local emitter, mixed processing, and the local machine-family regression. Emitter fake actions also passed in-frame. Source fake drags collect slots on movement, so the corrected bus gesture crosses A to B and back to A; physical card drags retain the ordinary A-to-B action. The passing transport case covers its named components; complete original item patches and all physical component stack boundaries remain next-wave work.

## Wave 16c: modeled component retention and physical stack boundaries

The full `horizon-client` library suite passed **1,336 tests** (32 ignored), and the full client-host suite passed **451 tests** (three ignored), using the locked fast profile. The ignored tests retain their source resource/GPU/manual prerequisites; the AE2 registry prerequisites were separately supplied in Wave 12. Binary and source archive hashes appear above; patch SHA-256 is `ab6dd52d1388e035c061395204ee7b521986c54ff0a6ff6a38575232acc1bc2d` against `1f233c62079fc622db8b4b69499043a30a6720c8`.

The v2 local compatibility envelope preserves the full modeled item-component patch, explicit removals, nested modeled stacks and transient map operations, while retaining its v1 reader and escaping colliding custom data. Typed max-stack packets use the ordinary protocol component too. This is Horizon-to-Horizon transport, within the existing network-NBT quota. It preserves data retained by the host model; it does not implement every component's gameplay or a vanilla-client codec. Source-valid extra potion/effect/color, enchantment visibility and book page/filter/resolved data now retain raw payloads while unchanged; baseline tracking prevents a later modeled edit from resurrecting stale data. Supported default/alternate codec shapes normalize to one native item identity.

Shared physical stack limits now cover menus/cursors, real machine and Interface slots, pickups, world-item merging, commands, hoppers/carts, terminal/tool/monitor transfers and returns. Cell, CPU and Provider logical quantities keep their separate limits. Inscriber ONE/FOUR/SIXTY_FOUR capacity and Assembler input limits have source-shaped implementations. The recipe-aware Inscriber filter is separate pending work; an earlier local-machine test that accepted iron in an invalid recipe slot is retracted as Inscriber parity evidence.

Eleven named hidden worlds passed on the current runtime: physical menu/terminal limits, Inscriber capacity, item pickup, `/give` and explicit component removal, hopper transfer, hopper-minecart pickup, complete modeled component wire/creative return, rich item cell storage, local Pattern Access, source-only cell/pattern crafting and wireless source components. The menu/terminal and Inscriber capacity fixtures also passed in-frame. These checks include authoritative server component assertions and real save/reload custody. The before-runs reproduced oversized inventory, hopper/cart and command stacks, discarded removals/potion fields, and the absent local Pattern Access opener.

The first broad gate exposed sparse test registries lacking real CUSTOM_DATA ID 0 and a numerical source-pattern test with fewer than the now-required nine slots. Those fixtures were corrected. Two crash-stack assertions assumed line tables although fast inherits release with none: they now assert the actual originating function in every profile and additionally require the source location in development builds. All full suites then passed. These corrections are test-only: the 16, 16b and 16c native binaries have identical SHA-256, so the existing physical runs test the exact runtime bytes. Both earlier source archives remain retained by their own hashes locally.

General Formation placement, mixed-channel/overfilled source cells, recipe-aware machine filters and complete local wireless lifecycle remain ongoing source-port work. New Java-method comparison helpers may document those requirements before their native implementations pass a gate.


## Wave 17: mixed source cells, placement transactions and repository clicks

The final native binary/source hashes appear above. Patch SHA-256 is `276b0e9e214863d8c313772b61c56671fc316a0f05677c272adfd8cf0aaab55f` against `747e4823632bb7a4d7a264612b29b603ed901288`. Production Rust/TOML/lock files, including newly added files, were checked against the archived build inputs before publication.

This cohort has **24 distinct passing fixture/mode combinations** across three immutable builds. It is not a claim that every fixture ran on one binary:

| Build | Binary SHA-256 | Physical scope |
| --- | --- | --- |
| 17e | `e90f5d25b7463ad06feef36a86509ca237598c6944f6c4662cef176d0a1f2b10` | Fifteen selected worlds passed, plus four in-frame menu worlds. The broader mixed-cell server case exposed the prediction/portable defects corrected below. |
| 17f | `7d370e0e752f37369ae2adc6feb1fbc709544f43108e8b012073f1a5c454e49c` | Repository prediction, local machine menus, physical menu ownership and fake/physical drag custody passed. Mixed-cell server advanced to the portable queued-click defect. |
| 17g | `cdffc01040a2e926e6cbd795f618c1bbccbe49b89e0f5978988118f7f5d57774` | Mixed-cell server menus, held-server authoritative portable rapid clicks and repository prediction all passed, with exact custody and restart. |

The named storage cases recover unavailable items from a fluid cell, preserve positive foreign-channel contents, extract from an overfilled source cell without accepting more input, and let an I/O Port transfer an item past blocked water. ME Chest exposes both channels; a portable menu hides foreign keys while retaining them physically. The Inscriber rejects unrelated input and honors partial recipes/Name Presses. The Assembler uses its named current recipe, source substitution rules and extract-only output; a forced plan retains its definition and executes with real substitute input. Corrected in-frame hotkeys select the source-ranked cell without requiring it in the selected slot.

Formation now stages real native BlockItem placement, imports item block state and block-entity components before `setPlacedBy`, and commits or discards the physical changes together. Hidden worlds place a charged/styled Energy Cell, a nested shulker with lock/name, an oriented Monitor, torch, seed and both door halves; the corresponding custody and restart assertions passed. A held-server TNT circuit verifies that preview does not publish or duplicate its neighbor-triggered priming event. Native fluid placement covers water, flowing lava, waterlogging (including an already-wet slab), component rejection, source filters, power loss, exact 1,377→1,000+377 mB I/O custody, empty legacy patches, Nether evaporation and restart. Normal player chest/Monitor orientation, energy-cell wrench recovery and hive item-state/bee placement passed too.

Server virtual clicks now retain bounded visible-key history, so an unrelated state update does not discard an ordered action while changed key mappings still reject it. The server identifies repository slots per window; those slots do not receive vanilla physical slot/cursor prediction or physical drag/double-click handling. Separate held-server before/after worlds check authoritative cell contents, not just predicted client icons. Physical cells, upgrades, encoding ghosts and machine ownership remain on their respective existing routes.

The first gate exposed two test assumptions: source cell loading preserves an already-stored nonempty nested cell even though insertion rejects it, and the Assembler test catalog required its loot-table directory. Those were corrected without a native behavior change. Hotkey options needed their actual data-version header. A flowing-lava fixture also needed freeze restored after restart and neighboring fixture water cleared before checking its transient state; the unchanged 17d binary then passed. The menu failures were actual production defects and received the separate 17e–17g fixes.

Scoped Java-method oracles cover five currently implemented mixed/overfilled-cell cases plus pending duplicate/nonpositive requirements, four I/O transfer cases, and seven pattern-validation/assembly cases. New CPU-completion and Armor Stand source oracles document further requirements against explicit host stubs; they do not establish native gameplay. The world runner lists unverified or failing follow-ons as explicitly selectable pending suites, excluded from its default checkpoint run.

**Remaining:** cross-dimension Formation execution, successful non-BlockItem `useOn` adapters and golem/Wither callbacks; source CPU finish/cancel/idle-inventory behavior; wireless/portable lifecycle and source crafting/fluid gestures; duplicate/nonpositive cell edge cases and other ledger items. The hidden before-cases reproduce remote Quantum-plane rejection, blocked-fluid CPU completion withholding the independent item output, and the old shift-click crafting shortcut. Full AE2 and ATM10 compatibility remain unfinished.
