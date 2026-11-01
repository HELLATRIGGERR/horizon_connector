//! Forge Energy of the game's own machines for the FE dispatcher
//! (`machines/native_energy.rs`): AE2's energy acceptors, chargers, P2P
//! tunnels and debug energy generator, through its `IEnergyStorage` adapters
//! (`ae2_p2p/energy.rs`, `ae2_power.rs`), when the game is built with AE2.
//! Without it they answer nothing. AE2's adapters are sided and `int`: a
//! call for the block itself (no side) finds none, and amounts are clamped
//! to `i32::MAX` whatever the call's width.
use super::{Dim, Game, native_hooks::NativeFeInfo};
use horizon_core::geometry::Direction;

#[cfg(feature = "ae2-lab")]
impl Game<'_> {
    /// AE2's machine at `pos` in level `dim`, by its grid address.
    fn extra_fe_address(&self, dim: Dim, pos: [i32; 3]) -> Option<super::ae2_global::GridAddress> {
        let address = self.ae2_dimension_address(dim, (pos[0], pos[1], pos[2]))?;
        matches!(
            self.ae2_block_entity(address),
            Some(horizon_client::containers::BlockEntity::Ae2(_))
        )
        .then_some(address)
    }

    /// The FE readback of the game's own machine at `pos` through `face`.
    pub(super) fn extra_fe_info(&self, dim: Dim, pos: [i32; 3], face: Option<Direction>) -> Option<NativeFeInfo> {
        let address = self.extra_fe_address(dim, pos)?;
        let s = self.ae2_fe_storage(address, face?)?;
        Some(NativeFeInfo {
            stored: i64::from(s.stored),
            capacity: i64::from(s.capacity),
            can_receive: s.can_receive,
            can_extract: s.can_extract,
        })
    }

    /// `receiveEnergy` (with `extract`, `extractEnergy`) on the game's own
    /// machine at `pos` through `face`: what moved.
    pub(super) fn extra_fe_move(
        &mut self,
        dim: Dim,
        pos: [i32; 3],
        face: Option<Direction>,
        amount: i64,
        simulate: bool,
        extract: bool,
    ) -> i64 {
        let (Some(address), Some(face)) = (self.extra_fe_address(dim, pos), face) else {
            return 0;
        };
        let amount = amount.clamp(0, i64::from(i32::MAX)) as u32;
        let moved = if extract {
            self.ae2_fe_extract(address, face, amount, simulate)
        } else {
            self.ae2_fe_receive(address, face, amount, simulate)
        };
        i64::from(moved)
    }
}

#[cfg(not(feature = "ae2-lab"))]
impl Game<'_> {
    pub(super) fn extra_fe_info(&self, dim: Dim, pos: [i32; 3], face: Option<Direction>) -> Option<NativeFeInfo> {
        let _ = (dim, pos, face);
        None
    }

    pub(super) fn extra_fe_move(
        &mut self,
        dim: Dim,
        pos: [i32; 3],
        face: Option<Direction>,
        amount: i64,
        simulate: bool,
        extract: bool,
    ) -> i64 {
        let _ = (dim, pos, face, amount, simulate, extract);
        0
    }
}
