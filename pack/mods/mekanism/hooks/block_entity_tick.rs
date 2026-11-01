// A server tick of a Mekanism block entity.
fn mek_block_entity_tick(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    entity: &mut horizon_client::containers::ModBlockEntity,
) {
    crate::game::mek_world::tick(game, pos, entity);
}
