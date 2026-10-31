//! Screens of the game's own that ride the mod-screen path (`native_hooks`:
//! `NativeScreen`, menu buttons, data slots): AE2's machine screens
//! (`ae2_screens`) when the game is built with them. `native_hooks.rs` calls
//! these without `cfg`, so that its hook points stay hookable; without AE2
//! they do nothing.
use super::{Game, native_hooks::NativeScreen};
use horizon_client::hud::HudBatch;

/// What an open extra screen keeps.
#[cfg(feature = "ae2-lab")]
pub(super) type Extra = Option<super::ae2_screens::Ae2Screen>;
#[cfg(not(feature = "ae2-lab"))]
pub(super) type Extra = ();

#[cfg(feature = "ae2-lab")]
impl Game<'_> {
    /// A menu opens on block `block`: an AE2 machine's screen when no mod
    /// laid one out.
    pub(super) fn extra_native_screen(&self, block: &str, screen: &mut Option<NativeScreen>) -> Extra {
        if screen.is_some() {
            return None;
        }
        let (laid_out, style) = self.ae2_native_screen(block)?;
        *screen = Some(laid_out);
        Some(style)
    }

    /// While an AE2 machine's priority editor holds its window, the
    /// machine's screen steps aside for the editor's own menu.
    pub(super) fn extra_screen_stepped_aside(&self) -> bool {
        self.remote.as_ref().is_some_and(|r| r.ae2_priority_screen)
    }

    /// Draws the screen when it is an extra one: true then.
    pub(super) fn extra_screen_draw(&self, batch: &mut HudBatch, ox: f32, oy: f32, extra: &Extra, data: &[i32]) -> bool {
        let Some(screen) = extra.as_ref() else {
            return false;
        };
        self.ae2_draw_screen(batch, ox, oy, screen, data);
        true
    }

    /// A press handled on this client alone (the help button): true then.
    pub(super) fn extra_screen_local_press(&mut self, at: [f32; 2], right: bool) -> bool {
        !right && self.ae2_screen_help(at)
    }

    /// The menu button a press at `at` makes, if any.
    pub(super) fn extra_screen_button(&self, data: &[i32], at: [f32; 2], right: bool) -> Option<i32> {
        let screen = self.mod_screen_extra()?.as_ref()?;
        self.ae2_screen_button(screen, data, at, right)
    }

    pub(super) fn extra_screen_tooltip(&self, data: &[i32], at: [f32; 2], lines: &mut Vec<String>) {
        if let Some(screen) = self.mod_screen_extra().and_then(|e| e.as_ref()) {
            self.ae2_screen_tooltip(screen, data, at, lines);
        }
    }
}

#[cfg(not(feature = "ae2-lab"))]
impl Game<'_> {
    pub(super) fn extra_native_screen(&self, _block: &str, _screen: &mut Option<NativeScreen>) -> Extra {}

    pub(super) fn extra_screen_stepped_aside(&self) -> bool {
        false
    }

    pub(super) fn extra_screen_draw(&self, _batch: &mut HudBatch, _ox: f32, _oy: f32, _extra: &Extra, _data: &[i32]) -> bool {
        false
    }

    pub(super) fn extra_screen_local_press(&mut self, _at: [f32; 2], _right: bool) -> bool {
        false
    }

    pub(super) fn extra_screen_button(&self, _data: &[i32], _at: [f32; 2], _right: bool) -> Option<i32> {
        None
    }

    pub(super) fn extra_screen_tooltip(&self, _data: &[i32], _at: [f32; 2], _lines: &mut Vec<String>) {}
}
