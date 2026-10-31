# Storage fuzzy-search oracle

`java ae2-cell-core/tests/oracle/StorageFuzzyOracle.java` runs a small
standalone check using the bounds methods from AE2 19.2.18 at commit
`db17504a86128fdf3dae31f5fb7a112a646e0b93`. It needs a JDK with single-file
source execution and writes no classes into this repository.

The stub keys make the item damage and maximum damage explicit. The cases
cover a query whose maximum differs from the configured key, a matching range,
and a configured damage beyond the query maximum. The implementation is
attributed to `FuzzySearch.makeLowerBound` and `makeUpperBound`; this is not
a running AE2 installation. Fluid primary-key behavior follows
`KeyCounter.getSubIndex` and `VariantCounter.UnorderedVariantMap.findFuzzy`
and is separately exercised by the Rust cell tests and the prepared physical
world fixture.

The mixed-channel cell oracle runs `bun ae2-cell-core/tests/oracle/run-mixed-cell-oracle.ts`
with `AE2_REFERENCE` set to the pinned reference checkout. It extracts the
actual `BasicCellInventory.getCellItems`, `loadCellItems`, `getAvailableStacks`,
`extract`, `innerInsert`, `saveChanges`, `persist` and capacity-accounting methods into a small Java
stub. It checks that stored item and fluid keys are both exposed, a simulated
foreign-channel extraction is nonmutating, committed extraction preserves the
other key and remainder, and an overfilled 1K cell remains extractable. It also
checks duplicate-key last-write behavior, the source's initial list accounting
and post-mutation map recount, including an overfilled cell whose zero-accepted
insert still triggers a recount. After persistence, positive duplicate contents
agree between the mounted and reloaded views, while zero/negative entries can
make those views differ. The native cell decoder does not yet support those cases.
The stub models only the key, counter, primitive-map and save hooks used by
those methods; it is not an AE2 game instance. Compilation occurs in a
system temporary directory.

`run-io-port-oracle.ts` extracts the pinned `IOPortBlockEntity.transferContents`
method into scoped storage/grid stubs. It verifies that a blocked fluid does
not block a later item, and that a later 1,000 mB transfer uses eight fluid
operations and preserves custody. It requires the same `AE2_REFERENCE` and JDK;
all compiled classes stay in a temporary directory.
