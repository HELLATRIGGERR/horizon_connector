// The item slots of a Mekanism block's entity, and the menu that shows them.
fn mek_block_entity_container(block: &str, container: &mut Option<horizon_client::containers::ModContainer>) {
    crate::game::mek_world::container(block, container);
}
