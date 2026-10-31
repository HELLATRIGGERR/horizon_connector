//! AE2's block entity (`ae2_world`, in horizon-client-render) saved and loaded
//! through the client's save format.
    use crate::ae2_world::*;
    use crate::items::ItemStack;
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
        card.data_mut().custom_name = Some(crate::item_data::text_name("real card"));
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
