// `/mek` in every player's command tree.
fn mek_register_commands(commands: &mut Vec<crate::game::native_server::NativeCommand>) {
    crate::game::mek_world::commands(commands);
}
