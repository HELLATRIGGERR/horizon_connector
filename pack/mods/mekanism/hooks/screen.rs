// The layout of a Mekanism block's screen.
fn mek_screen(block: &str, screen: &mut Option<crate::game::native_hooks::NativeScreen>) {
    crate::game::mek_ui::screen(block, screen);
}
