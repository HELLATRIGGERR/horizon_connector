# First-tick queue oracle

`Ae2FastutilOrder.java` invokes the actual `Long2ObjectOpenHashMap` API, using
fastutil 8.5.12 (local jar SHA-256
`b5543ee08d062d551cf0a5c9bc0fb70588b0382079029ba48941fa9c9be8a5d4`).
It records key-set iteration after insertion, removal/shrink, and further
insertion, including the special zero key. `fastutil-8.5.12-order.json` is its
three output lines converted to JSON. The Rust unit compares all three phases.

The fixture tests fastutil ordering only. AE2's readiness eligibility,
per-chunk callback order, and host mutation adaptation require their own tests.
