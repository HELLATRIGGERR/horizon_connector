// Forge Energy offered to a Mekanism cube (`IEnergyStorage.receiveEnergy`).
fn mek_fe_receive(
    face: Option<horizon_core::geometry::Direction>,
    entity: &mut horizon_client::containers::ModBlockEntity,
    max: i64,
    simulate: bool,
    accepted: &mut Option<i64>,
) {
    crate::game::mek_fe::transfer(face, entity, max, simulate, false, accepted);
}
