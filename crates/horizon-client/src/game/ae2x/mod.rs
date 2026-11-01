//! The game behaviour of AE2 addons, one module per addon behind its
//! `ae2-<id>` feature. Their rows in the definitions registry are in the
//! library's `ae2x`.
#[cfg(feature = "ae2-megacells")]
pub(super) mod megacells;
