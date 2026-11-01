// A player's `/mek` command on the server.
fn mek_perform_command(
    game: &mut crate::game::Game<'_>,
    player: &crate::game::native_server::NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    crate::game::mek_world::perform(game, player, command, ran);
}
