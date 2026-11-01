// A cube's ejector, into its Forge Energy neighbours.
fn mek_fe_block_entity_tick(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    entity: &mut horizon_client::containers::ModBlockEntity,
) {
    crate::game::mek_fe::tick(game, pos, entity);
}
