# AE2 network core

Rust network logic derived from Applied Energistics 2 commit `db17504a86128fdf3dae31f5fb7a112a646e0b93`, copyright AlgorithmX2 and AE2 contributors, under LGPL-3.0-or-later. See `LICENSE`. This crate supplies the native Horizon integration; it does not load Java mods or establish complete AE2 parity.

`Network` is a transient graph projection. The host supplies physical devices, orientation/color masks, stable node and part identities, explicit quantum/ME P2P edges, energy-only quartz-fiber connections, and formed CPU groups. Physical items, saved machines and the event journal own persistent state. Host adapters encode dimensions and virtual endpoints separately from physical block positions.

`service_events::OrderedServices` tracks actual ready/connect/disconnect/destroy callbacks, grid membership, connection order and merge/split behavior. `CommittedChannels` retains the previously committed channel state through a pathing update and validates its identity mapping against the current graph. `record_connection_order` accepts the live incident order only after checking every logical edge; rebuilding the same coordinates alone cannot recover that history.

Controller allocation follows the source's separate node/connection breadth-first queues, priorities, compressed-channel policy, multiblock demand and channel-use propagation. Its bounded oracle compiles the pinned Java `PathingCalculation` with test stubs and source node/connection methods. It compares 128 synthetic graphs, not a running Java game. Multi-controller JVM identity-hash iteration is not reproduced. Graphs without validated live ordering retain deterministic coordinate ordering.

Power models include physical capacity/storage, vibration-chamber fuel/ramping and the host's source-ordered energy-service projection. The host remains responsible for fuel custody, per-node buffer membership, scheduling, actual inventory/fluid capabilities, menus, chunk lifecycle and packet delivery. A graph test cannot establish those integration behaviors.

Run `cargo test --locked` here. With the pinned AE2 checkout, Bun and JDK 21+, run `bun scripts/test-pathing-oracle.ts` from the repository root. See the root parity ledgers for named native-world evidence and remaining requirements.
