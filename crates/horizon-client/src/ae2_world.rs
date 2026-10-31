//! Experimental persistent AE2 machine state. Simulation belongs to the game.
use crate::items::ItemStack;
use horizon_world::nbt::Compound;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineKind {
    Drive,
    Chest,
    VibrationChamber,
    Interface,
    PatternProvider,
    Inscriber,
    Charger,
    MolecularAssembler,
    Controller,
    EnergyAcceptor,
    EnergyCell,
    DenseEnergyCell,
    CableBus,
    WirelessAccessPoint,
    Terminal,
    CraftingTerminal,
    PatternEncodingTerminal,
    CraftingUnit,
    CraftingAccelerator,
    CraftingStorage1k,
    CraftingStorage4k,
    CraftingStorage16k,
    CraftingStorage64k,
    CraftingStorage256k,
    CraftingMonitor,
    IoPort,
    CellWorkbench,
    Condenser,
    SpatialIoPort,
    SpatialPylon,
    SpatialAnchor,
    QuantumRing,
    QuantumLink,
    GrowthAccelerator,
    CrystalResonanceGenerator,
    CreativeEnergyCell,
    DebugEnergyGenerator,
    DebugItemGenerator,
    DebugCubeGenerator,
    DebugPhantomNode,
    SkyStoneTank,
    SkyStoneChest,
    SmoothSkyStoneChest,
    LightDetector,
    Paint,
    Crank,
    MysteriousCube,
}
impl MachineKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Drive => "ae2:drive",
            Self::Chest => "ae2:chest",
            Self::VibrationChamber => "ae2:vibration_chamber",
            Self::Interface => "ae2:interface",
            Self::PatternProvider => "ae2:pattern_provider",
            Self::Inscriber => "ae2:inscriber",
            Self::Charger => "ae2:charger",
            Self::MolecularAssembler => "ae2:molecular_assembler",
            Self::Controller => "ae2:controller",
            Self::EnergyAcceptor => "ae2:energy_acceptor",
            Self::EnergyCell => "ae2:energy_cell",
            Self::DenseEnergyCell => "ae2:dense_energy_cell",
            Self::CableBus => "ae2:cable_bus",
            Self::WirelessAccessPoint => "ae2:wireless_access_point",
            Self::Terminal => "ae2:terminal",
            Self::CraftingTerminal => "ae2:crafting_terminal",
            Self::PatternEncodingTerminal => "ae2:pattern_encoding_terminal",
            Self::CraftingUnit => "ae2:crafting_unit",
            Self::CraftingAccelerator => "ae2:crafting_accelerator",
            Self::CraftingStorage1k => "ae2:1k_crafting_storage",
            Self::CraftingStorage4k => "ae2:4k_crafting_storage",
            Self::CraftingStorage16k => "ae2:16k_crafting_storage",
            Self::CraftingStorage64k => "ae2:64k_crafting_storage",
            Self::CraftingStorage256k => "ae2:256k_crafting_storage",
            Self::CraftingMonitor => "ae2:crafting_monitor",
            Self::IoPort => "ae2:io_port",
            Self::CellWorkbench => "ae2:cell_workbench",
            Self::Condenser => "ae2:condenser",
            Self::SpatialIoPort => "ae2:spatial_io_port",
            Self::SpatialPylon => "ae2:spatial_pylon",
            Self::SpatialAnchor => "ae2:spatial_anchor",
            Self::QuantumRing => "ae2:quantum_ring",
            Self::QuantumLink => "ae2:quantum_link",
            Self::GrowthAccelerator => "ae2:growth_accelerator",
            Self::CrystalResonanceGenerator => "ae2:crystal_resonance_generator",
            Self::CreativeEnergyCell => "ae2:creative_energy_cell",
            Self::DebugEnergyGenerator => "ae2:debug_energy_gen",
            Self::DebugItemGenerator => "ae2:debug_item_gen",
            Self::DebugCubeGenerator => "ae2:debug_cube_gen",
            Self::DebugPhantomNode => "ae2:debug_phantom_node",
            Self::SkyStoneTank => "ae2:sky_stone_tank",
            Self::SkyStoneChest => "ae2:sky_stone_chest",
            Self::SmoothSkyStoneChest => "ae2:smooth_sky_stone_chest",
            Self::LightDetector => "ae2:light_detector",
            Self::Paint => "ae2:paint",
            Self::Crank => "ae2:crank",
            Self::MysteriousCube => "ae2:mysterious_cube",
        }
    }
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "ae2:drive" => Self::Drive,
            "ae2:chest" => Self::Chest,
            "ae2:vibration_chamber" => Self::VibrationChamber,
            "ae2:interface" => Self::Interface,
            "ae2:pattern_provider" => Self::PatternProvider,
            "ae2:inscriber" => Self::Inscriber,
            "ae2:charger" => Self::Charger,
            "ae2:molecular_assembler" => Self::MolecularAssembler,
            "ae2:controller" => Self::Controller,
            "ae2:energy_acceptor" => Self::EnergyAcceptor,
            "ae2:energy_cell" => Self::EnergyCell,
            "ae2:dense_energy_cell" => Self::DenseEnergyCell,
            "ae2:cable_bus" => Self::CableBus,
            "ae2:wireless_access_point" => Self::WirelessAccessPoint,
            "ae2:terminal" => Self::Terminal,
            "ae2:crafting_terminal" => Self::CraftingTerminal,
            "ae2:pattern_encoding_terminal" => Self::PatternEncodingTerminal,
            "ae2:crafting_unit" => Self::CraftingUnit,
            "ae2:crafting_accelerator" => Self::CraftingAccelerator,
            "ae2:1k_crafting_storage" => Self::CraftingStorage1k,
            "ae2:4k_crafting_storage" => Self::CraftingStorage4k,
            "ae2:16k_crafting_storage" => Self::CraftingStorage16k,
            "ae2:64k_crafting_storage" => Self::CraftingStorage64k,
            "ae2:256k_crafting_storage" => Self::CraftingStorage256k,
            "ae2:crafting_monitor" => Self::CraftingMonitor,
            "ae2:io_port" => Self::IoPort,
            "ae2:cell_workbench" => Self::CellWorkbench,
            "ae2:condenser" => Self::Condenser,
            "ae2:spatial_io_port" => Self::SpatialIoPort,
            "ae2:spatial_pylon" => Self::SpatialPylon,
            "ae2:spatial_anchor" => Self::SpatialAnchor,
            "ae2:quantum_ring" => Self::QuantumRing,
            "ae2:quantum_link" => Self::QuantumLink,
            "ae2:growth_accelerator" => Self::GrowthAccelerator,
            "ae2:crystal_resonance_generator" => Self::CrystalResonanceGenerator,
            "ae2:creative_energy_cell" => Self::CreativeEnergyCell,
            "ae2:debug_energy_gen" => Self::DebugEnergyGenerator,
            "ae2:debug_item_gen" => Self::DebugItemGenerator,
            "ae2:debug_cube_gen" => Self::DebugCubeGenerator,
            "ae2:debug_phantom_node" => Self::DebugPhantomNode,
            "ae2:sky_stone_tank" => Self::SkyStoneTank,
            "ae2:sky_stone_chest" => Self::SkyStoneChest,
            "ae2:smooth_sky_stone_chest" => Self::SmoothSkyStoneChest,
            "ae2:light_detector" => Self::LightDetector,
            "ae2:paint" => Self::Paint,
            "ae2:crank" => Self::Crank,
            "ae2:mysterious_cube" => Self::MysteriousCube,
            _ => return None,
        })
    }
    pub fn physical_slots(self) -> usize {
        match self {
            Self::Drive => 10,
            Self::DebugItemGenerator => 16,
            Self::Chest | Self::VibrationChamber | Self::Charger | Self::WirelessAccessPoint => 1,
            Self::Interface | Self::PatternProvider | Self::CraftingTerminal => 9,
            Self::MolecularAssembler => 11,
            Self::Inscriber => 4,
            Self::IoPort => 12,
            Self::CellWorkbench => 1,
            Self::Condenser | Self::SpatialIoPort => 2,
            Self::PatternEncodingTerminal => 2,
            Self::QuantumRing | Self::QuantumLink => 1,
            Self::SkyStoneChest | Self::SmoothSkyStoneChest => 36,
            _ => 0,
        }
    }
    /// Physical card slots from AE2 19.2.18; cell-workbench upgrades live on its cell.
    pub fn upgrade_slots(self) -> usize {
        match self {
            Self::Inscriber => 4,
            Self::VibrationChamber | Self::IoPort => 3,
            Self::MolecularAssembler => 5,
            Self::Interface => 1,
            _ => 0,
        }
    }
}
/// `slots` are owned physical inventory only. Network contents must never be copied here.
#[derive(Clone, Debug, PartialEq)]
pub struct Machine {
    pub kind: MachineKind,
    pub slots: Vec<Option<ItemStack>>,
    /// Real installed cards; never synthetic settings or virtual display slots.
    pub upgrades: Vec<Option<ItemStack>>,
    pub energy: f64,
    /// MEChestBlockEntity captures its buffer maximum in its constructor. This
    /// runtime-only value is rebuilt from configuration when the entity loads.
    pub chest_power_capacity: Option<f64>,
    pub burn_remaining: f64,
    pub burn_rate: f64,
    pub progress: f64,
    pub config: Compound,
}
impl Machine {
    pub fn new(kind: MachineKind) -> Self {
        Self {
            kind,
            slots: vec![None; kind.physical_slots()],
            upgrades: vec![None; kind.upgrade_slots()],
            energy: 0.0,
            chest_power_capacity: (kind == MachineKind::Chest)
                .then(|| (500.0 * crate::ae2_common_config::gameplay().usage_multiplier)
                    .min(ae2_machines_core::stored_energy::MAX_MAXIMUM)),
            burn_remaining: 0.0,
            burn_rate: 1.0,
            progress: 0.0,
            config: Compound::new(),
        }
    }
    /// Private AEBasePoweredBlockEntity buffer maximum, fixed at construction.
    pub fn private_power_capacity(&self) -> Option<f64> {
        match self.kind {
            MachineKind::Inscriber | MachineKind::Charger | MachineKind::GrowthAccelerator => Some(1600.0),
            MachineKind::Chest => self.chest_power_capacity,
            _ => None,
        }
    }
    pub fn set_private_energy(&mut self, amount: f64) {
        if let Some(maximum) = self.private_power_capacity() {
            let mut stored = ae2_machines_core::stored_energy::StoredEnergy::new(self.energy, maximum);
            stored.set_stored(amount);
            self.energy = stored.stored();
        }
    }
    /// The powered machine and energy-cell source classes use setStored when
    /// their saved block entity is loaded. Other machine state is untouched.
    pub fn source_energy_capacity(&self) -> Option<f64> {
        self.private_power_capacity().or(match self.kind {
            MachineKind::Controller => Some(8000.0),
            MachineKind::EnergyAcceptor => Some(0.0),
            MachineKind::EnergyCell => Some(200_000.0),
            MachineKind::DenseEnergyCell => Some(1_600_000.0),
            _ => None,
        })
    }
    pub fn set_saved_energy(&mut self, amount: f64) {
        if let Some(maximum) = self.source_energy_capacity() {
            let mut stored = ae2_machines_core::stored_energy::StoredEnergy::new(0.0, maximum);
            stored.set_stored(amount);
            self.energy = stored.stored();
        } else {
            self.energy = amount;
        }
    }
    pub fn private_energy_insert(&mut self, amount: f64, commit: bool) -> f64 {
        let Some(maximum) = self.private_power_capacity() else { return 0.0; };
        let mut stored = ae2_machines_core::stored_energy::StoredEnergy::new(self.energy, maximum);
        let accepted = stored.insert(amount, commit);
        if commit { self.energy = stored.stored(); }
        accepted
    }
    pub fn private_energy_extract(&mut self, amount: f64, commit: bool) -> f64 {
        let Some(maximum) = self.private_power_capacity() else { return 0.0; };
        let mut stored = ae2_machines_core::stored_energy::StoredEnergy::new(self.energy, maximum);
        let extracted = stored.extract(amount, commit);
        if commit { self.energy = stored.stored(); }
        extracted
    }
    /// Real owned inventory, including crafting CPU escrow. Job metadata and
    /// terminal listings are virtual and must never be emitted as drops.
    pub fn physical_contents(&self) -> Vec<ItemStack> {
        let mut out=self.physical_slots_and_upgrades();
        if matches!(
            self.kind,
            MachineKind::CraftingUnit
                | MachineKind::CraftingAccelerator
                | MachineKind::CraftingStorage1k
                | MachineKind::CraftingStorage4k
                | MachineKind::CraftingStorage16k
                | MachineKind::CraftingStorage64k
                | MachineKind::CraftingStorage256k
                | MachineKind::CraftingMonitor
        ) {
            if let Some(horizon_world::nbt::Tag::List(entries)) =
                self.config.get(&"ae2_cpu_escrow".into())
            {
                for entry in entries {
                    // Decoder's compatibility repairs are unsafe for escrow:
                    // require a positive saved count before decoding.
                    if matches!(entry.get("count"),Some(horizon_world::nbt::Tag::Int(n)) if *n>0) {
                        if let Some(stack) = crate::save::item_from_nbt(entry) {
                            out.push(stack);
                        }
                    }
                }
            }
        }
        if self.kind == MachineKind::PatternProvider {
            if let Some(horizon_world::nbt::Tag::List(entries)) =
                self.config.get(&"ae2_provider_return_items".into())
            {
                for entry in entries.iter().take(9) {
                    if matches!(entry.get("count"),Some(horizon_world::nbt::Tag::Int(n)) if *n>0) {
                        if let Some(stack) = crate::save::item_from_nbt(entry) {
                            out.push(stack);
                        }
                    }
                }
            }
            if self.config.contains_key(&crate::ae2_provider_queue::QUEUE.into()) {
                if let Ok(entries)=crate::ae2_provider_queue::read(&self.config) {
                    if let Ok(items)=crate::ae2_provider_queue::physical_items(&entries) {out.extend(items);}
                }
            } else if let Some(horizon_world::nbt::Tag::List(entries)) = self.config.get(&"ae2_provider_send_items".into()) {
                if entries.len() <= 8192 {
                    for entry in entries {
                        if matches!(entry.get("count"),Some(horizon_world::nbt::Tag::Int(n)) if *n>0) {
                            if let Some(stack)=crate::save::item_from_nbt(entry) {out.push(stack);}
                        }
                    }
                }
            }
        }
        out
    }
    /// Physical block slots and installed upgrades; deferred AE2 inventories
    /// are handled by the source-aware drop path using their own limits.
    pub fn physical_slots_and_upgrades(&self) -> Vec<ItemStack> {
        let physical_slots = if self.kind == MachineKind::DebugItemGenerator { &[][..] } else { &self.slots[..] };
        physical_slots
            .iter()
            .chain(self.upgrades.iter())
            .flatten()
            .cloned()
            .collect()
    }
    pub fn installed_speed_cards(&self) -> usize {
        self.upgrades
            .iter()
            .flatten()
            .filter(|s| s.item.as_ref() == "ae2:speed_card" && s.count == 1)
            .count()
    }
    pub fn for_block(id: &str) -> Option<Self> {
        Some(Self::new(MachineKind::from_id(id)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{containers::BlockEntity, save};
    use horizon_world::nbt::Tag;
    use std::sync::Arc;
    #[test]
    fn chest_constructor_capacity_is_runtime_state_not_saved_nbt() {
        let expected = Machine::new(MachineKind::Chest).chest_power_capacity.unwrap();
        let mut chest = Machine::new(MachineKind::Chest);
        chest.chest_power_capacity = Some(expected + 17.0);
        chest.energy = 42.0;
        let entity = BlockEntity::Ae2(Box::new(chest));
        let nbt = save::encode_block_entity((0, 0, 0), &entity, "ae2:chest");
        let (_, BlockEntity::Ae2(restored)) = save::decode_block_entity(&nbt).unwrap() else {
            panic!("expected chest")
        };
        assert_eq!(restored.energy, 42.0);
        assert_eq!(restored.chest_power_capacity, Some(expected));
    }
    #[test]
    fn saved_powered_block_and_cell_use_source_stored_energy_bounds() {
        use ae2_machines_core::stored_energy::MIN_AMOUNT;
        let mut charger = Machine::new(MachineKind::Charger);
        charger.set_saved_energy(MIN_AMOUNT / 2.0);
        assert_eq!(charger.energy, 0.0);
        assert_eq!(charger.private_energy_insert(MIN_AMOUNT / 2.0, true), 0.0);
        assert_eq!(charger.private_energy_insert(MIN_AMOUNT, false), MIN_AMOUNT);
        assert_eq!(charger.energy, 0.0);
        assert_eq!(charger.private_energy_insert(MIN_AMOUNT, true), MIN_AMOUNT);
        assert_eq!(charger.energy, MIN_AMOUNT);
        charger.set_saved_energy(1601.0);
        assert_eq!(charger.energy, 1600.0);
        let mut cell = Machine::new(MachineKind::EnergyCell);
        cell.set_saved_energy(200_001.0);
        assert_eq!(cell.energy, 200_000.0);
        cell.set_saved_energy(MIN_AMOUNT / 2.0);
        assert_eq!(cell.energy, 0.0);
        cell.set_saved_energy(-3.0);
        assert_eq!(cell.energy, 0.0);
        let mut other = Machine::new(MachineKind::VibrationChamber);
        other.set_saved_energy(MIN_AMOUNT / 2.0);
        assert_eq!(other.energy, MIN_AMOUNT / 2.0);
    }
    #[test]
    fn debug_item_generator_persists_filter_but_never_persists_or_drops_generated_stacks() {
        let mut machine = Machine::new(MachineKind::DebugItemGenerator);
        machine.config.insert("filter".into(), Tag::String("minecraft:diamond".into()));
        machine.slots[0] = Some(ItemStack::new("minecraft:diamond", 64));
        machine.progress = 12.0;
        assert!(machine.physical_contents().is_empty());
        let nbt = save::encode_block_entity((0, 64, 0), &BlockEntity::Ae2(Box::new(machine)), "ae2:debug_item_gen");
        let (_, BlockEntity::Ae2(restored)) = save::decode_block_entity(&nbt).expect("debug machine should decode") else { panic!("debug machine missing") };
        assert_eq!(restored.config.get(&"filter".into()), Some(&Tag::String("minecraft:diamond".into())));
        assert!(restored.slots.iter().all(Option::is_none));
        assert_eq!(restored.progress, 0.0);
    }
    #[test]
    fn world_nbt_retains_machine_and_real_physical_stack_components() {
        let mut machine = Machine::new(MachineKind::Drive);
        let mut stack = ItemStack {
            item: Arc::from("ae2:item_storage_cell_1k"),
            count: 1,
            damage: 0,
            extra: None,
        };
        stack.data_mut().set_other(
            "minecraft:custom_data",
            Some(Tag::compound([("payload", Tag::Int(123))])),
        );
        machine.slots[0] = Some(stack.clone());
        machine.energy = 42.5;
        machine.burn_remaining = 7.0;
        machine.burn_rate = 1.5;
        machine.progress = 0.25;
        machine
            .config
            .insert("filter".into(), Tag::String("minecraft:stone".into()));
        let original = BlockEntity::Ae2(Box::new(machine.clone()));
        let nbt = save::encode_block_entity((3, 64, 5), &original, machine.kind.id());
        assert_eq!(crate::block_entity_nbt::canonical(&nbt), Some(nbt.clone()));
        let (pos, decoded) = save::decode_block_entity(&nbt).unwrap();
        assert_eq!(pos, (3, 64, 5));
        let BlockEntity::Ae2(back) = decoded else {
            panic!("wrong block entity")
        };
        assert_eq!(*back, machine);
        assert_eq!(original.contents(), vec![stack]);
    }
    #[test]
    fn actual_upgrade_cards_and_io_slots_survive_save_and_drop() {
        let mut machine = Machine::new(MachineKind::IoPort);
        assert_eq!(machine.slots.len(), 12);
        assert_eq!(machine.upgrades.len(), 3);
        machine.slots[0] = Some(ItemStack::new("ae2:item_storage_cell_1k", 1));
        let mut card = ItemStack::new("ae2:speed_card", 1);
        card.data_mut().custom_name = Some("real card".into());
        machine.upgrades[0] = Some(card.clone());
        assert_eq!(machine.installed_speed_cards(), 1);
        let nbt = save::encode_block_entity(
            (3, 64, 5),
            &BlockEntity::Ae2(Box::new(machine.clone())),
            machine.kind.id(),
        );
        let (_, BlockEntity::Ae2(back)) = save::decode_block_entity(&nbt).unwrap() else {
            panic!("AE2 IO port")
        };
        assert_eq!(*back, machine);
        assert_eq!(BlockEntity::Ae2(back).contents().len(), 2);
        let mut old = nbt.clone();
        if let Tag::Compound(ref mut map) = old {
            map.remove(&"ae2_lab_upgrades".into());
        }
        let (_, BlockEntity::Ae2(old)) = save::decode_block_entity(&old).unwrap() else {
            panic!("legacy")
        };
        assert_eq!(old.upgrades, vec![None, None, None]);
        assert_eq!(old.slots[0], machine.slots[0]);
    }

    #[test]
    fn cpu_physical_escrow_is_saved_and_dropped_without_virtual_jobs() {
        let real = ItemStack::new("minecraft:iron_ingot", 7);
        let mut cpu = Machine::new(MachineKind::CraftingStorage1k);
        cpu.config.insert(
            "ae2_cpu_escrow".into(),
            Tag::List(vec![
                save::item_to_nbt(&real),
                Tag::compound([
                    ("id", Tag::String("minecraft:diamond".into())),
                    ("count", Tag::Int(0)),
                ]),
            ]),
        );
        cpu.config.insert(
            "ae2_virtual_pending".into(),
            Tag::List(vec![save::item_to_nbt(&ItemStack::new(
                "minecraft:diamond",
                32,
            ))]),
        );
        let nbt = save::encode_block_entity(
            (0, 64, 0),
            &BlockEntity::Ae2(Box::new(cpu.clone())),
            cpu.kind.id(),
        );
        let (_, BlockEntity::Ae2(restored)) = save::decode_block_entity(&nbt).unwrap() else {
            panic!("CPU")
        };
        assert_eq!(restored.physical_contents(), vec![real]);
        assert!(
            Machine::new(MachineKind::Terminal)
                .physical_contents()
                .is_empty()
        );
    }

    #[test]
    fn provider_return_buffer_survives_save_and_is_physical_once() {
        let mut p = Machine::new(MachineKind::PatternProvider);
        let item = ItemStack::new("minecraft:iron_ingot", 3);
        p.config.insert(
            "ae2_provider_return_items".into(),
            Tag::List(vec![save::item_to_nbt(&item)]),
        );
        let encoded = save::encode_block_entity(
            (1, 2, 3),
            &BlockEntity::Ae2(Box::new(p)),
            "ae2:pattern_provider",
        );
        let (_, BlockEntity::Ae2(restored)) = save::decode_block_entity(&encoded).unwrap() else {
            panic!("provider")
        };
        assert_eq!(restored.physical_contents(), vec![item]);
        let mut other = Machine::new(MachineKind::Terminal);
        other.config = restored.config.clone();
        assert!(other.physical_contents().is_empty());
    }
    #[test]
    fn provider_send_items_are_real_saved_contents_only_for_provider() {
        let mut provider=Machine::new(MachineKind::PatternProvider);
        let item=ItemStack::new("minecraft:diamond",3);
        provider.config.insert("ae2_provider_send_items".into(),Tag::List(vec![save::item_to_nbt(&item),Tag::compound([("id",Tag::String("minecraft:stone".into()))])]));
        provider.config.insert("ae2_provider_send_fluids".into(),Tag::List(vec![Tag::compound([("fluid",Tag::String("minecraft:water".into()))])]));
        let encoded=save::encode_block_entity((1,2,3),&BlockEntity::Ae2(Box::new(provider)),"ae2:pattern_provider");
        let (_,BlockEntity::Ae2(restored))=save::decode_block_entity(&encoded).unwrap() else{panic!("provider")};
        assert_eq!(restored.physical_contents(),vec![item]);
        let mut other=Machine::new(MachineKind::Terminal);other.config=restored.config.clone();assert!(other.physical_contents().is_empty());
    }
    #[test]
    fn provider_ordered_queue_preserves_item_custody_across_save() {
        let mut provider=Machine::new(MachineKind::PatternProvider);
        let mut item=ItemStack::new("minecraft:stone",1);
        item.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("variant",Tag::String("queued".into()))])));
        let water=ae2_cell_core::ItemKey{item_id:"minecraft:water".into(),components:vec![1,2]};
        crate::ae2_provider_queue::write(&mut provider.config,&[
            crate::ae2_provider_queue::Entry::Fluid{key:water,amount:1000},
            crate::ae2_provider_queue::Entry::Item{prototype:item.clone(),amount:999_999},
        ]).unwrap();
        let encoded=save::encode_block_entity((1,2,3),&BlockEntity::Ae2(Box::new(provider)),"ae2:pattern_provider");
        let (_,BlockEntity::Ae2(restored))=save::decode_block_entity(&encoded).unwrap() else{panic!("provider")};
        let items=restored.physical_contents();assert_eq!(items.len(),1);assert_eq!(items[0].count,999_999);
        assert!(items[0].same_item_same_components(&item));
    }
    #[test]
    fn terminal_does_not_duplicate_network_contents_and_invalid_state_is_rejected() {
        let entity = BlockEntity::for_block("ae2:terminal").unwrap();
        assert!(entity.slots().is_empty());
        assert!(entity.contents().is_empty());
        let mut tag = save::encode_block_entity((0, 0, 0), &entity, "ae2:terminal");
        if let Tag::Compound(ref mut c) = tag {
            c.insert("ae2_lab_energy".into(), Tag::Double(f64::NAN.to_bits()));
        }
        assert!(save::decode_block_entity(&tag).is_none());
    }
}
