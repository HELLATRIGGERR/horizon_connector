// Whether a Mekanism block's entity ticks.
fn mek_block_entity_ticks(block: &str, ticks: &mut bool) {
    crate::game::mek_world::ticks(block, ticks);
}
