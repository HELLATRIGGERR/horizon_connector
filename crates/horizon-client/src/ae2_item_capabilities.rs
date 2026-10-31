//! Native item-capability probes for AE2 P2P attunement.
//! AE2 19.2.18 InitP2PAttunements checks EnergyStorage.ITEM before
//! FluidHandler.ITEM, after all item tags. Hosts may register real additional
//! item capability providers; probes must be read-only.
use crate::items::ItemStack;
use std::sync::{OnceLock, RwLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemCapability { Energy, Fluid }
pub type Probe = fn(&ItemStack) -> bool;
fn probes() -> &'static RwLock<Vec<(ItemCapability, Probe)>> {
    static PROBES: OnceLock<RwLock<Vec<(ItemCapability, Probe)>>> = OnceLock::new();
    PROBES.get_or_init(|| RwLock::new(Vec::new()))
}
/// Register an actual host item-capability provider, not a tag or counter.
pub fn register_probe(capability: ItemCapability, probe: Probe) {
    probes().write().unwrap_or_else(|poison| poison.into_inner()).push((capability, probe));
}
fn powered_ae2_item(stack: &ItemStack) -> bool {
    let id = stack.item.as_ref();
    matches!(id, "ae2:entropy_manipulator" | "ae2:charged_staff" | "ae2:color_applicator" | "ae2:matter_cannon" | "ae2:wireless_terminal" | "ae2:wireless_crafting_terminal")
        || ["ae2:portable_item_cell_", "ae2:portable_fluid_cell_"].iter().any(|prefix| {
            id.strip_prefix(prefix).is_some_and(|tier| matches!(tier, "1k" | "4k" | "16k" | "64k" | "256k"))
        })
}
fn native_fluid_container(stack: &ItemStack) -> bool {
    // These are the physical bucket handlers currently implemented by this host.
    (stack.item.as_ref() == "minecraft:bucket" || crate::ae2_cell_codec::bucket_fluid(&stack.item).is_some())
        && stack.same_item_same_components(&ItemStack::new(&stack.item, 1))
}
/// Return the first actual capability in AE2's source registration order.
pub fn first_attunement_capability(stack: &ItemStack) -> Option<ItemCapability> {
    let registered = probes().read().unwrap_or_else(|poison| poison.into_inner());
    for capability in [ItemCapability::Energy, ItemCapability::Fluid] {
        let native = match capability {
            ItemCapability::Energy => powered_ae2_item(stack),
            ItemCapability::Fluid => native_fluid_container(stack),
        };
        if native || registered.iter().any(|(kind, probe)| *kind == capability && probe(stack)) {
            return Some(capability);
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registered_providers_follow_energy_before_fluid() {
        fn both(stack: &ItemStack) -> bool { stack.item.as_ref() == "example:dual_capability" }
        register_probe(ItemCapability::Fluid, both);
        register_probe(ItemCapability::Energy, both);
        assert_eq!(first_attunement_capability(&ItemStack::new("example:dual_capability",1)), Some(ItemCapability::Energy));
    }
    #[test]
    fn built_in_energy_and_physical_bucket_capabilities_are_detected() {
        assert_eq!(first_attunement_capability(&ItemStack::new("ae2:charged_staff",1)),Some(ItemCapability::Energy));
        assert_eq!(first_attunement_capability(&ItemStack::new("ae2:portable_fluid_cell_16k",1)),Some(ItemCapability::Energy));
        assert_eq!(first_attunement_capability(&ItemStack::new("minecraft:water_bucket",1)),Some(ItemCapability::Fluid));
        assert_eq!(first_attunement_capability(&ItemStack::new("minecraft:diamond",1)),None);
    }
}
