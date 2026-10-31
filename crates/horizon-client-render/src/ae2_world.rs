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
                        if let Some(stack) = horizon_items::item_nbt::item_from_nbt(entry) {
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
                        if let Some(stack) = horizon_items::item_nbt::item_from_nbt(entry) {
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
                            if let Some(stack)=horizon_items::item_nbt::item_from_nbt(entry) {out.push(stack);}
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
