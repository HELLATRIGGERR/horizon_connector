// The tooltip over a Mekanism block's screen.
fn mek_screen_tooltip(block: &str, data: &[i32], at: [f32; 2], lines: &mut Vec<String>) {
    crate::game::mek_ui::tooltip(block, data, at, lines);
}
