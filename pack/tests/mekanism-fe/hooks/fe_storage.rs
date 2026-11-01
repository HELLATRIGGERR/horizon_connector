// The Forge Energy of a Mekanism cube on one side (`IEnergyStorage` readback).
fn mek_fe_storage(
    face: Option<horizon_core::geometry::Direction>,
    entity: &horizon_client::containers::ModBlockEntity,
    info: &mut Option<crate::game::native_hooks::NativeFeInfo>,
) {
    crate::game::mek_fe::storage(face, entity, info);
}
