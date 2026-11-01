// A button of a Mekanism block's screen, on the server.
fn mek_menu_button(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    entity: &mut horizon_client::containers::ModBlockEntity,
    id: i32,
) {
    crate::game::mek_world::button(game, pos, entity, id);
}
