// A world opens or closes: Mekanism's settings are read or dropped.
fn mek_world_lifecycle(phase: crate::game::native_lifecycle::WorldPhase) {
    crate::game::mek_world::lifecycle(phase);
}
