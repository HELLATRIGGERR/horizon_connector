// A press on a Mekanism block's screen: the menu button it is on.
fn mek_screen_click(block: &str, data: &[i32], at: [f32; 2], right: bool, button: &mut Option<i32>) {
    crate::game::mek_ui::click(block, data, at, right, button);
}
