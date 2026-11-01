// Forge Energy drawn from a Mekanism cube (`IEnergyStorage.extractEnergy`).
fn mek_fe_extract(
    face: Option<horizon_core::geometry::Direction>,
    entity: &mut horizon_client::containers::ModBlockEntity,
    max: i64,
    simulate: bool,
    given: &mut Option<i64>,
) {
    crate::game::mek_fe::transfer(face, entity, max, simulate, true, given);
}
