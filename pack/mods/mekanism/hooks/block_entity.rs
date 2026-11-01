// A Mekanism block gets its block entity (`EntityBlock.newBlockEntity`).
fn mek_block_entity(block: &str, entity: &mut Option<(String, horizon_world::nbt::Compound)>) {
    crate::game::mek_world::new_entity(block, entity);
}
