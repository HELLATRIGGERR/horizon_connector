//! Physical Memory Card transactions for supported machines and Interface faces.
//! Each part belongs to one cable face; unrelated host state is never replaced.
use super::Game;
use horizon_client::{containers::BlockEntity,ae2_network_tool::Inventory,ae2_world::{Machine,MachineKind},save};
use horizon_world::nbt::Tag;
// Preserve a structured AE2 machine name beside the existing plain display.
const MEMORY_NAME_COMPONENT: &str = "horizon:ae2_memory_name_component";

pub(super) fn memory_card_with_plain_name(card:&horizon_client::items::ItemStack)->horizon_client::items::ItemStack {
    let mut copy=card.clone();
    if let Some(raw)=card.data().and_then(|data|data.other("ae2:exported_custom_name")) {
        let plain=horizon_client::entity_nbt::name_text(raw).plain();
        copy.data_mut().set_other("ae2:exported_custom_name",Some(Tag::String(plain.as_str().into())));
    }
    copy
}

pub(super) fn memory_card_name_component(card:&horizon_client::items::ItemStack,legacy:Option<&str>)->Option<Tag> {
    if let Some(data)=card.data() {
        if data.other("ae2:exported_settings_source").is_some() {return data.other("ae2:exported_custom_name").cloned();}
    }
    legacy.map(|name|Tag::String(name.into()))
}

pub(super) fn machine_name_component(machine:&Machine,plain:Option<&str>)->Option<Tag> {
    let plain=plain?;
    match machine.config.get(&MEMORY_NAME_COMPONENT.into()) {
        Some(raw) if horizon_client::entity_nbt::name_text(raw).plain()==plain=>Some(raw.clone()),
        _=>Some(Tag::String(plain.into())),
    }
}

pub(super) fn set_machine_name_component(machine:&mut Machine,component:Option<&Tag>) {
    match component {
        Some(raw @ (Tag::Compound(_) | Tag::List(_)))=>{machine.config.insert(MEMORY_NAME_COMPONENT.into(),raw.clone());}
        _=>{machine.config.remove(&MEMORY_NAME_COMPONENT.into());}
    }
}

pub(super) fn stack_name_component(stack:&horizon_client::items::ItemStack)->Option<Tag> {
    stack.custom_name_component().cloned()
}

pub(super) fn publish_memory_name(card:&mut horizon_client::items::ItemStack,component:Option<Tag>) {
    card.data_mut().set_other("ae2:exported_custom_name",component);
}

enum Saved { Inscriber(super::ae2_memory::SavedInscriber), Io(super::ae2_memory::SavedIo), Vibration(super::ae2_memory::SavedVibration), Interface(super::ae2_memory::SavedInterface), Assembler(super::ae2_memory::SavedAssembler) }
enum Stored { Inscriber(super::ae2_memory::SavedInscriber), Io(super::ae2_memory::SavedIo), Vibration(super::ae2_memory::SavedVibration), Interface(super::ae2_memory::SavedInterface), Assembler(super::ae2_memory::SavedAssembler), Bus(super::ae2_memory::SavedBus) }
#[derive(Clone,Copy)] enum Target {Block, InterfacePart {index:usize,face:u8}}
struct Snapshot {target:Target,host:Machine,machine:Machine,incarnation:u64,name:Option<String>,name_component:Option<Tag>}
fn uses_legacy_provider_priority_shortcut(card:&horizon_client::items::ItemStack)->bool {
    !card.data().is_some_and(|data|["ae2:exported_settings_source","ae2:exported_settings","ae2:exported_upgrades","ae2:exported_priority","ae2:exported_config_inv","ae2:exported_custom_name","ae2:exported_patterns","ae2:exported_push_direction","ae2:exported_level_emitter_value"].iter().any(|id|data.other(id).is_some()))
}
fn simple_direct_source(card:&horizon_client::items::ItemStack,kind:MachineKind)->bool {
    matches!(kind,MachineKind::Inscriber|MachineKind::IoPort|MachineKind::VibrationChamber|MachineKind::MolecularAssembler)
        && !uses_legacy_provider_priority_shortcut(card)
}
fn apply_simple_generic_settings(machine:&mut Machine,card:&horizon_client::items::ItemStack)->Result<(),String>{
    let Some(settings)=card.data().and_then(|data|data.other("ae2:exported_settings")) else{return Ok(())};
    let Tag::Compound(settings)=settings else{return Err("invalid exported settings".into())};
    let get=|key:&str|->Option<String>{match settings.get(&key.into()){Some(Tag::String(value))=>value.to_string().ok(),_=>None}};
    if machine.kind==MachineKind::Inscriber {
        for (key,allowed) in [("auto_export",&["YES","NO"][..]),("inscriber_separate_sides",&["YES","NO"][..]),("inscriber_input_capacity",&["ONE","FOUR","SIXTY_FOUR"][..])] {
            if let Some(value)=get(key).filter(|value|allowed.contains(&value.as_str())){machine.config.insert(key.into(),Tag::String(value.as_str().into()));}
        }
    }
    if machine.kind==MachineKind::IoPort {
        for (source,target,allowed) in [("operation_mode","io_operation",&["FILL","EMPTY"][..]),("fullness_mode","io_fullness",&["EMPTY","HALF","FULL"][..])] {
            if let Some(value)=get(source).filter(|value|allowed.contains(&value.as_str())){machine.config.insert(target.into(),Tag::String(value.to_ascii_lowercase().as_str().into()));}
        }
        if let Some(value)=get("redstone_controlled").and_then(|v|match v.as_str(){"IGNORE"=>Some("ignore"),"HIGH_SIGNAL"=>Some("high"),"LOW_SIGNAL"=>Some("low"),_=>None}){machine.config.insert("io_redstone".into(),Tag::String(value.into()));}
    }
    Ok(())
}
fn read_saved(card:&horizon_client::items::ItemStack,interface_target:bool)->Result<Option<Stored>,String> {
    if let Some(v)=super::ae2_memory::read_inscriber(card)? {return Ok(Some(Stored::Inscriber(v)))}
    if let Some(v)=super::ae2_memory::read_io(card)? {return Ok(Some(Stored::Io(v)))}
    if let Some(v)=super::ae2_memory::read_vibration(card)? {return Ok(Some(Stored::Vibration(v)))}
    if let Some(v)=super::ae2_memory::read_interface(card)? {return Ok(Some(Stored::Interface(v)))}
    if let Some(v)=super::ae2_memory::read_assembler(card)? {return Ok(Some(Stored::Assembler(v)))}
    if let Some(v)=super::ae2_memory::emitter_as_interface(card)? {return Ok(Some(Stored::Interface(v)))}
    if let Some(v)=super::ae2_memory::storage_as_interface(card)? {return Ok(Some(Stored::Interface(v)))}
    if let Some(id)=super::ae2_memory::bus_source_from_card(card) {
        if let Some(v)=super::ae2_memory::read_bus(card,id)? {return Ok(Some(Stored::Bus(v)))}
    }
    if interface_target {if let Some(v)=super::ae2_memory::read_source_interface(card,true)? {return Ok(Some(Stored::Interface(v)))}}
    Ok(None)
}
#[derive(Clone,Copy)] enum ToolLocation {Inventory(usize),Armor(usize),Offhand}
impl Game<'_> {
    /// MemoryCardItem.use clears exported settings on alternate use in air.
    pub(super) fn ae2_clear_memory_in_air(&mut self) -> bool {
        if !self.keys.contains(&super::KeyCode::ShiftLeft) { return false; }
        let Some(mut card) = self.held().filter(|s| s.item.as_ref() == "ae2:memory_card" && s.count == 1).cloned() else { return false; };
        if super::ae2_memory::clear(&mut card).is_err() { return true; }
        horizon_client::ae2_stack_identity::synchronize_tracked(&card);
        *self.held_mut() = Some(card);
        self.system_message(horizon_client::text::Text::literal("Settings cleared"));
        true
    }
    fn memory_tool(&self)->Result<Option<(ToolLocation,Inventory)>,String>{
        for (i,item) in self.inventory.iter().enumerate(){if let Some(s)=item.as_ref().filter(|s|s.item.as_ref()=="ae2:network_tool") {return Ok(Some((ToolLocation::Inventory(i),Inventory::read(s)?)))}}
        for (i,item) in self.armor.iter().enumerate(){if let Some(s)=item.as_ref().filter(|s|s.item.as_ref()=="ae2:network_tool") {return Ok(Some((ToolLocation::Armor(i),Inventory::read(s)?)))}}
        if let Some(s)=self.offhand.as_ref().filter(|s|s.item.as_ref()=="ae2:network_tool") {return Ok(Some((ToolLocation::Offhand,Inventory::read(s)?)))}
        Ok(None)
    }
    fn machine_memory_snapshot(&self,pos:[i32;3],face:Option<&str>)->Result<Option<Snapshot>,String>{
        let key=(pos[0],pos[1],pos[2]);
        let Some(BlockEntity::Ae2(host))=self.world.block_entities.get(&key) else{return Ok(None)};
        let (target,machine,name,name_component)=if matches!(host.kind,MachineKind::Inscriber|MachineKind::IoPort|MachineKind::VibrationChamber|MachineKind::Interface|MachineKind::MolecularAssembler|MachineKind::Condenser|MachineKind::Drive|MachineKind::Chest) {
            if self.res.facts().get(self.world.block(pos[0],pos[1],pos[2])).name.as_ref()!=host.kind.id(){return Ok(None)}
            let name=self.world.block_names.get(&key).cloned();let component=machine_name_component(host,name.as_deref());
            (Target::Block,(**host).clone(),name,component)
        }else if host.kind==MachineKind::CableBus {
            let Some(face)=face else{return Ok(None)};
            let Some(face_index)=["east","west","up","down","south","north"].iter().position(|f|*f==face) else{return Err("invalid Interface face".into())};
            let Some(Tag::List(parts))=host.config.get(&"parts".into()) else{return Ok(None)};
            let mut found=parts.iter().enumerate().filter(|(_,part)|part.get("face")==Some(&Tag::String(face.into())));
            let Some((index,part))=found.next() else{return Ok(None)};
            if found.next().is_some(){return Err("duplicate Interface face".into())}
            if part.get("id")!=Some(&Tag::String("ae2:cable_interface".into())){return Ok(None)}
            super::ae2_channels::part_identity(part).ok_or("invalid Interface part identity")?;
            let stack=part.get("stack").and_then(save::item_from_nbt).filter(|s|s.item.as_ref()=="ae2:cable_interface"&&s.count==1).ok_or("invalid Interface part stack")?;
            let address=super::ae2_parts::PartAddress{host:self.ae2_local_address(key).ok_or("invalid host address")?,face:face_index as u8};
            let machine=self.ae2_part_machine(address)?.filter(|m|m.kind==MachineKind::Interface).ok_or("missing Interface part")?;
            let name=stack.custom_name();let component=stack_name_component(&stack);
            (Target::InterfacePart{index,face:face_index as u8},machine,name,component)
        }else{return Ok(None)};
        Ok(Some(Snapshot{target,host:(**host).clone(),machine,incarnation:self.world.block_entities.incarnation(&key).ok_or("missing machine identity")?,name,name_component}))
    }
    /// Commit one staged machine and optional name against the whole host
    /// snapshot and incarnation, keeping all other faces and host data intact.
    fn machine_memory_commit(&mut self,pos:[i32;3],before:&Snapshot,machine:Machine,name:Option<Option<Tag>>)->Result<(),String>{
        self.machine_memory_commit_with_dirty(pos,before,machine,name,true)
    }
    fn machine_memory_commit_with_dirty(&mut self,pos:[i32;3],before:&Snapshot,machine:Machine,name:Option<Option<Tag>>,mark_dirty:bool)->Result<(),String>{
        let key=(pos[0],pos[1],pos[2]);
        if self.world.block_entities.incarnation(&key)!=Some(before.incarnation){return Err("machine identity changed".into())}
        let mut next=match before.target {
            Target::Block=>machine,
            Target::InterfacePart{index,face}=>{
                let mut host=before.host.clone();
                let Some(Tag::List(parts))=host.config.get_mut(&"parts".into()) else{return Err("missing Interface part list".into())};
                let part=parts.get_mut(index).ok_or("missing Interface part")?;
                let expected=["east","west","up","down","south","north"][usize::from(face)];
                if part.get("face")!=Some(&Tag::String(expected.into())){return Err("Interface face changed".into())}
                let Tag::Compound(part)=part else{return Err("invalid Interface part".into())};
                part.insert("machine".into(),Tag::compound([
                    ("slots",save::encode_items(&machine.slots)),("upgrades",save::encode_items(&machine.upgrades)),("config",Tag::Compound(machine.config))]));
                if let Some(name)=name.as_ref(){
                    let mut stack=part.get(&"stack".into()).and_then(save::item_from_nbt).ok_or("invalid Interface stack")?;
                    stack.set_custom_name_component(name.clone());part.insert("stack".into(),save::item_to_nbt(&stack));
                }
                host
            }
        };
        if matches!(before.target,Target::Block){if let Some(name)=name.as_ref(){set_machine_name_component(&mut next,name.as_ref());}}
        let Some(BlockEntity::Ae2(current))=self.world.block_entities.get_mut(&key) else{return Err("machine disappeared".into())};
        if **current!=before.host{return Err("machine changed during import".into())}
        std::mem::swap(&mut **current,&mut next);
        if matches!(before.target,Target::Block){if let Some(name)=name{match name{Some(name)=>{self.world.block_names.insert(key,horizon_client::entity_nbt::name_text(&name).plain());},None=>{self.world.block_names.remove(&key);}}}}
        if mark_dirty {self.world.modified.insert((pos[0]>>4,pos[2]>>4));}Ok(())
    }
    pub(super) fn ae2_try_machine_memory_face(&mut self,pos:[i32;3],face:&str)->bool {
        if self.ae2_try_bus_memory_face(pos,face){return true}
        if self.ae2_try_energy_emitter_memory_face(pos,face){return true}
        if self.ae2_try_emitter_memory_face(pos,face){return true}
        self.ae2_try_machine_memory_at(pos,Some(face))
    }
    fn ae2_try_machine_memory_at(&mut self,pos:[i32;3],face:Option<&str>)->bool {
        let Some(mut card)=self.held().filter(|s|s.item.as_ref()=="ae2:memory_card"&&s.count==1).cloned() else{return false};
        let key=(pos[0],pos[1],pos[2]);
        let snapshot=match self.machine_memory_snapshot(pos,face){Ok(Some(v))=>v,Ok(None)=>return false,Err(_)=>return true};
        let before=&snapshot.machine;let kind=before.kind;
        if self.keys.contains(&super::KeyCode::ShiftLeft) {
            if super::ae2_memory::clear(&mut card).is_err(){return true}
            let name=snapshot.name.as_deref();
            let result=match kind {
                MachineKind::Inscriber=>super::ae2_memory::save_inscriber(&mut card,before,name),
                MachineKind::IoPort=>super::ae2_memory::save_io(&mut card,before,name),
                MachineKind::VibrationChamber=>super::ae2_memory::save_vibration(&mut card,before,name),
                MachineKind::Interface if matches!(snapshot.target,Target::InterfacePart{..})=>super::ae2_memory::save_interface_part(&mut card,before,name),
                MachineKind::Interface=>super::ae2_memory::save_interface(&mut card,before,name),
                MachineKind::MolecularAssembler=>super::ae2_memory::save_assembler(&mut card,before,name),
                MachineKind::Condenser=>super::ae2_memory::save_condenser(&mut card,before,name),
                MachineKind::Drive|MachineKind::Chest=>super::ae2_memory::save_storage(&mut card,before,name),_=>unreachable!()};
            if result.is_ok() && super::ae2_memory::publish_simple_source(&mut card).is_ok() {
                publish_memory_name(&mut card,snapshot.name_component.clone());
                horizon_client::ae2_stack_identity::synchronize_tracked(&card);*self.held_mut()=Some(card);
                self.system_message(horizon_client::text::Text::literal("Settings saved"));
            }
            return true
        }
        use horizon_client::ae2_world::MachineKind as K;
        let original_card=card.clone();let card=memory_card_with_plain_name(&card);
        let direct_simple=simple_direct_source(&card,kind);
        let direct_interface=kind==K::Interface&&!uses_legacy_provider_priority_shortcut(&card);
        let card=if direct_simple||direct_interface{card}else{match super::ae2_memory::simple_source_card(&card){Ok(Some(converted))=>converted,Ok(None)=>card,Err(_)=>{self.system_message(horizon_client::text::Text::literal("Invalid machine"));return true}}};
        if matches!(kind,K::Drive|K::Chest) {
            let saved=match super::ae2_memory::read_storage(&card,kind){Ok(Some(v))=>v,_=>{self.system_message(horizon_client::text::Text::literal("Invalid machine"));return true}};
            if !saved.matching&&!super::ae2_memory::storage_has_components(&saved){self.system_message(horizon_client::text::Text::literal("Invalid machine"));return true}
            let mut machine=snapshot.machine.clone();if super::ae2_memory::apply_storage(&mut machine,&saved).is_err(){return true}
            // Drive priority setters dirty even for the same value. Chest
            // priority alone does not; its terminal-setting listeners do.
            let mark_dirty=super::ae2_memory::storage_import_marks_dirty(kind,&saved);
            let name=if saved.matching{Some(memory_card_name_component(&original_card,saved.name.as_deref()))}else{None};
            if self.machine_memory_commit_with_dirty(pos,&snapshot,machine,name,mark_dirty).is_err(){return true}
            // The source setter is invoked even for a repeated priority value;
            // current native routing reads the committed value on each operation.
            if let Some(priority)=saved.priority {if self.ae2_set_storage_priority_local(key,priority).is_err(){return true}}
            self.system_message(horizon_client::text::Text::literal(if saved.matching{"Settings loaded"}else{"Compatible settings restored"}));return true
        }
        if kind==K::Condenser {
            let saved=match super::ae2_memory::read_condenser(&card){Ok(Some(v))=>v,_=>{self.system_message(horizon_client::text::Text::literal("Invalid machine"));return true}};
            if !saved.matching && saved.output.is_none(){self.system_message(horizon_client::text::Text::literal("Invalid machine"));return true}
            let mut machine=snapshot.machine.clone();
            if let Some(mode)=saved.output.as_deref(){
                let max=match super::ae2_memory::condenser_output_item(mode){
                    Ok(Some(item))=>match self.res.items().get(item){Some(v)=>v.max_stack.max(1),None=>return true},
                    Ok(None)=>64,Err(_)=>return true,
                };
                if super::ae2_storage_devices::condenser_apply_memory_mode(&mut machine,mode,max).is_err(){return true}
            }
            let name=if saved.matching{Some(memory_card_name_component(&original_card,saved.name.as_deref()))}else{None};
            if self.machine_memory_commit(pos,&snapshot,machine,name).is_err(){return true}
            self.system_message(horizon_client::text::Text::literal(if saved.matching{"Settings loaded"}else{"Compatible settings restored"}));return true
        }
        if kind==K::Interface && uses_legacy_provider_priority_shortcut(&card) {
            match super::ae2_memory_provider::priority_from_card(&card) {
                Ok(Some(priority))=>{
                    let mut machine=snapshot.machine.clone();
                    if super::ae2_memory::apply_interface_priority(&mut machine,priority).is_err(){return true}
                    if self.machine_memory_commit(pos,&snapshot,machine,None).is_err(){return true}
                    self.system_message(horizon_client::text::Text::literal("Compatible priority restored"));
                    return true
                },
                Ok(None)=>{},
                Err(_)=>{self.system_message(horizon_client::text::Text::literal("Invalid machine"));return true},
            }
        }
        let source=if direct_simple {
            // These machines do not have a generic config inventory. Ignore
            // that unrelated component rather than decoding another host's rows.
            let mut generic=card.clone();generic.data_mut().set_other("ae2:exported_config_inv",None);
            match super::ae2_memory::read_bus(&generic,"ae2:export_bus"){Ok(Some(v))=>Stored::Bus(v),_=>{self.system_message(horizon_client::text::Text::literal("Invalid machine"));return true}}
        }else if direct_interface {
            match super::ae2_memory::read_source_interface(&card,true){Ok(Some(v))=>Stored::Interface(v),_=>{self.system_message(horizon_client::text::Text::literal("Invalid machine"));return true}}
        }else{match read_saved(&card,kind==K::Interface) {Ok(Some(v))=>v,_=>{self.system_message(horizon_client::text::Text::literal("Invalid machine"));return true}}};
        let preserve_direct_upgrades=direct_simple&&matches!(&source,Stored::Bus(saved) if saved.desired.is_none());
        if let Stored::Interface(saved)=&source {
            // On the generic path, an absent component performs no action.
            // Unlike machines only share upgrades; Interface shares the four
            // generic components and its part source never authorizes a name.
            let compatible=if kind==K::Interface {
                (saved.source_is_interface && saved.part_source==matches!(snapshot.target,Target::InterfacePart{..})) || super::ae2_memory::interface_has_components(saved)
            }else{saved.desired.is_some()};
            if !compatible {self.system_message(horizon_client::text::Text::literal("Invalid machine"));return true}
        }
        let (bus_io_redstone,bus_preserve_upgrades)=match &source {
            Stored::Bus(saved) if kind==K::IoPort=>(super::ae2_memory::bus_io_redstone(saved),saved.desired.is_none()),
            _=>(None,false)
        };
        if let Stored::Bus(saved)=&source {
            if !direct_simple && kind!=K::Interface && saved.desired.is_none() && bus_io_redstone.is_none() {
                self.system_message(horizon_client::text::Text::literal("Invalid machine"));return true
            }
        }
        // Both Interface kinds import the same generic components. Only an
        // equal source description authorizes copying the name.
        let interface_contents=kind==K::Interface && matches!(&source,Stored::Interface(_)|Stored::Bus(_));
        let (saved,mut same)=match (kind,source) {
            (K::Inscriber,Stored::Inscriber(v))=>(Some(Saved::Inscriber(v)),true),
            (K::Inscriber,Stored::Io(v))=>(super::ae2_memory::io_to_inscriber(before,&v).ok().map(Saved::Inscriber),false),
            (K::Inscriber,Stored::Vibration(v))=>(super::ae2_memory::vibration_to_inscriber(before,&v).ok().map(Saved::Inscriber),false),
            (K::Inscriber,Stored::Interface(_))=>(super::ae2_memory::interface_to_inscriber(before).ok().map(Saved::Inscriber),false),
            (K::Inscriber,Stored::Assembler(v))=>(super::ae2_memory::assembler_to_inscriber(before,&v).ok().map(Saved::Inscriber),false),
            (K::Inscriber,Stored::Bus(v))=>(super::ae2_memory::bus_to_inscriber(before,&v).ok().map(Saved::Inscriber),false),
            (K::IoPort,Stored::Io(v))=>(Some(Saved::Io(v)),true),
            (K::IoPort,Stored::Inscriber(v))=>(super::ae2_memory::inscriber_to_io(before,&v).ok().map(Saved::Io),false),
            (K::IoPort,Stored::Vibration(v))=>(super::ae2_memory::vibration_to_io(before,&v).ok().map(Saved::Io),false),
            (K::IoPort,Stored::Interface(_))=>(super::ae2_memory::interface_to_io(before).ok().map(Saved::Io),false),
            (K::IoPort,Stored::Assembler(v))=>(super::ae2_memory::assembler_to_io(before,&v).ok().map(Saved::Io),false),
            (K::IoPort,Stored::Bus(v))=>(super::ae2_memory::bus_to_io(before,&v).ok().map(Saved::Io),false),
            (K::VibrationChamber,Stored::Vibration(v))=>(Some(Saved::Vibration(v)),true),
            (K::VibrationChamber,Stored::Inscriber(v))=>(Some(Saved::Vibration(super::ae2_memory::inscriber_to_vibration(&v))),false),
            (K::VibrationChamber,Stored::Io(v))=>(Some(Saved::Vibration(super::ae2_memory::io_to_vibration(&v))),false),
            (K::VibrationChamber,Stored::Interface(_))=>(Some(Saved::Vibration(super::ae2_memory::interface_to_vibration())),false),
            (K::VibrationChamber,Stored::Assembler(v))=>(Some(Saved::Vibration(super::ae2_memory::assembler_to_vibration(&v))),false),
            (K::VibrationChamber,Stored::Bus(v))=>(Some(Saved::Vibration(super::ae2_memory::bus_as_vibration(&v))),false),
            (K::Interface,Stored::Interface(v))=>{let same=v.source_is_interface&&v.part_source==matches!(snapshot.target,Target::InterfacePart{..});(Some(Saved::Interface(v)),same)},
            (K::Interface,Stored::Bus(_))=>(super::ae2_memory::bus_as_interface(&card).ok().flatten().map(Saved::Interface),false),
            (K::Interface,_)=> (super::ae2_memory::partial_interface(before).ok().map(Saved::Interface),false),
            (K::MolecularAssembler,Stored::Assembler(v))=>(Some(Saved::Assembler(v)),true),
            (K::MolecularAssembler,Stored::Inscriber(v))=>(Some(Saved::Assembler(super::ae2_memory::inscriber_to_assembler(&v))),false),
            (K::MolecularAssembler,Stored::Io(v))=>(Some(Saved::Assembler(super::ae2_memory::io_to_assembler(&v))),false),
            (K::MolecularAssembler,Stored::Vibration(v))=>(Some(Saved::Assembler(super::ae2_memory::vibration_to_assembler(&v))),false),
            (K::MolecularAssembler,Stored::Interface(_))=>(Some(Saved::Assembler(super::ae2_memory::interface_to_assembler())),false),
            (K::MolecularAssembler,Stored::Bus(v))=>(Some(Saved::Assembler(super::ae2_memory::bus_to_assembler(&v))),false),
            _=>unreachable!(),
        };
        if direct_simple {
            let translation=format!("block.ae2.{}",kind.id().strip_prefix("ae2:").unwrap_or(kind.id()));
            same=super::ae2_memory::source_description_matches(card.data().and_then(|d|d.other("ae2:exported_settings_source")),&translation);
            if !same&&preserve_direct_upgrades&&(!matches!(kind,K::Inscriber|K::IoPort)||card.data().and_then(|d|d.other("ae2:exported_settings")).is_none()){
                self.system_message(horizon_client::text::Text::literal("Invalid machine"));return true
            }
        }
        let Some(saved)=saved else{self.system_message(horizon_client::text::Text::literal("Invalid machine"));return true};
        let found=match self.memory_tool(){Ok(v)=>v,Err(_)=>return true};let location=found.as_ref().map(|(loc,_)|*loc);let mut pocket=found.map(|(_,inv)|inv);
        let mut machine=snapshot.machine.clone();let mut inventory=self.inventory.clone();let changes=match &saved {
            Saved::Inscriber(_) if preserve_direct_upgrades=>Ok(super::ae2_memory::PhysicalChanges{drops:Vec::new(),changed_tracked:Vec::new(),replaced_tracked:Vec::new(),missing:0}),
            Saved::Inscriber(v) if same=>super::ae2_memory::apply_inscriber(&mut machine,&mut inventory,&mut pocket,v,self.creative),
            Saved::Inscriber(v)=>super::ae2_memory::apply_inscriber_partial(&mut machine,&mut inventory,&mut pocket,v,self.creative),
            Saved::Io(_) if preserve_direct_upgrades=>Ok(super::ae2_memory::PhysicalChanges{drops:Vec::new(),changed_tracked:Vec::new(),replaced_tracked:Vec::new(),missing:0}),
            Saved::Io(v) if same=>super::ae2_memory::apply_io(&mut machine,&mut inventory,&mut pocket,v,self.creative),
            Saved::Io(_) if bus_preserve_upgrades=>Ok(super::ae2_memory::PhysicalChanges{drops:Vec::new(),changed_tracked:Vec::new(),replaced_tracked:Vec::new(),missing:0}),
            Saved::Io(v)=>super::ae2_memory::apply_io_partial(&mut machine,&mut inventory,&mut pocket,v,self.creative),
            Saved::Vibration(_) if preserve_direct_upgrades=>Ok(super::ae2_memory::PhysicalChanges{drops:Vec::new(),changed_tracked:Vec::new(),replaced_tracked:Vec::new(),missing:0}),
            Saved::Vibration(v)=>super::ae2_memory::apply_vibration(&mut machine,&mut inventory,&mut pocket,v,self.creative),
            Saved::Interface(v)=>super::ae2_memory::apply_interface_with_limits(&mut machine,&mut inventory,&mut pocket,v,self.creative,interface_contents,|stack|self.res.items().stack_max(stack)),
            Saved::Assembler(_) if preserve_direct_upgrades=>Ok(super::ae2_memory::PhysicalChanges{drops:Vec::new(),changed_tracked:Vec::new(),replaced_tracked:Vec::new(),missing:0}),
            Saved::Assembler(v)=>super::ae2_memory::apply_assembler(&mut machine,&mut inventory,&mut pocket,v,self.creative)
        };let mut changes=match changes {Ok(v)=>v,Err(_)=>return true};
        if let Some(redstone)=bus_io_redstone{machine.config.insert("io_redstone".into(),Tag::String(redstone.into()));}
        if direct_simple&&apply_simple_generic_settings(&mut machine,&card).is_err(){return true}
        // Validate and store the actual Network Tool before committing any of
        // the staged state; malformed pockets must not cause partial movement.
        let mut armor=self.armor.clone();let mut offhand=self.offhand.clone();
        if let (Some(location),Some(pocket))=(location,pocket.as_ref()) {
            let target=match location {ToolLocation::Inventory(i)=>inventory.get_mut(i).and_then(Option::as_mut),ToolLocation::Armor(i)=>armor.get_mut(i).and_then(Option::as_mut),ToolLocation::Offhand=>offhand.as_mut()};
            let Some(tool)=target else{return true};if pocket.store(tool).is_err(){return true}changes.changed_tracked.push(tool.clone());
        }
        let name=if same {let plain=match &saved {Saved::Inscriber(v)=>v.name.as_deref(),Saved::Io(v)=>v.name.as_deref(),Saved::Vibration(v)=>v.name.as_deref(),Saved::Interface(v)=>v.name.as_deref(),Saved::Assembler(v)=>v.name.as_deref()};Some(memory_card_name_component(&original_card,plain))} else {None};
        if self.machine_memory_commit(pos,&snapshot,machine,name).is_err(){return true}
        self.inventory=inventory;self.armor=armor;self.offhand=offhand;
        if (same||direct_simple) && kind==MachineKind::IoPort {self.ae2_invalidate_variable_processor(key);}
        for stack in changes.replaced_tracked {horizon_client::ae2_stack_identity::mark_detached_by_replacement(&stack,None);}
        for stack in changes.changed_tracked {horizon_client::ae2_stack_identity::synchronize_tracked(&stack);}
        for stack in changes.drops {self.player_drop(stack);}
        if changes.missing>0 {self.system_message(horizon_client::text::Text::literal(format!("Missing {} upgrade cards",changes.missing)));}
        self.system_message(horizon_client::text::Text::literal(if same {"Settings loaded"} else {"Compatible settings restored"}));true
    }
}

#[cfg(test)]
mod rich_memory_name_tests {
    use super::*;
    #[test]
    fn source_interface_card_does_not_take_legacy_priority_only_shortcut() {
        let mut card=horizon_client::items::ItemStack::new("ae2:memory_card",1);assert!(uses_legacy_provider_priority_shortcut(&card));
        card.data_mut().set_other("ae2:exported_priority",Some(Tag::Int(23)));assert!(!uses_legacy_provider_priority_shortcut(&card));
    }
    #[test]
    fn foreign_source_settings_apply_only_valid_target_fields_without_touching_upgrades() {
        let mut card=horizon_client::items::ItemStack::new("ae2:memory_card",1);
        card.data_mut().set_other("ae2:exported_settings_source",Some(Tag::compound([("translate",Tag::String("block.ae2.interface".into()))])));
        card.data_mut().set_other("ae2:exported_settings",Some(Tag::compound([("auto_export",Tag::String("YES".into())),("inscriber_separate_sides",Tag::String("yes".into())),("operation_mode",Tag::String("FILL".into())),("redstone_controlled",Tag::String("HIGH_SIGNAL".into()))])));
        let mut inscriber=Machine::new(MachineKind::Inscriber);inscriber.upgrades[0]=Some(horizon_client::items::ItemStack::new("ae2:speed_card",1));
        assert!(simple_direct_source(&card,inscriber.kind));apply_simple_generic_settings(&mut inscriber,&card).unwrap();
        assert_eq!(inscriber.config.get(&"auto_export".into()),Some(&Tag::String("YES".into())));assert!(!inscriber.config.contains_key(&"inscriber_separate_sides".into()));
        assert_eq!(inscriber.upgrades[0].as_ref().unwrap().item.as_ref(),"ae2:speed_card");
        let mut io=Machine::new(MachineKind::IoPort);apply_simple_generic_settings(&mut io,&card).unwrap();
        assert_eq!(io.config.get(&"io_operation".into()),Some(&Tag::String("fill".into())));assert_eq!(io.config.get(&"io_redstone".into()),Some(&Tag::String("high".into())));
    }
    #[test]
    fn original_machine_and_part_components_survive_plain_fallback_and_stale_names() {
        let raw=Tag::compound([("translate",Tag::String("item.minecraft.diamond".into())),("color",Tag::String("red".into())),("extra",Tag::List(vec![Tag::compound([("text",Tag::String("!".into()))])]))]);
        let mut card=horizon_client::items::ItemStack::new("ae2:memory_card",1);
        card.data_mut().set_other("ae2:exported_settings_source",Some(Tag::compound([("translate",Tag::String("block.ae2.interface".into()))])));
        card.data_mut().set_other("ae2:exported_custom_name",Some(raw.clone()));
        let proxy=memory_card_with_plain_name(&card);assert!(matches!(proxy.data().unwrap().other("ae2:exported_custom_name"),Some(Tag::String(_))));
        let plain=horizon_client::entity_nbt::name_text(&raw).plain();
        assert_eq!(memory_card_name_component(&card,Some(&plain)),Some(raw.clone()));
        let mut machine=Machine::new(MachineKind::Interface);set_machine_name_component(&mut machine,Some(&raw));
        assert_eq!(machine_name_component(&machine,Some(&plain)),Some(raw.clone()));
        assert_eq!(machine_name_component(&machine,Some("renamed")),Some(Tag::String("renamed".into())));
        let mut stack=horizon_client::items::ItemStack::new("ae2:level_emitter",1);stack.set_custom_name_component(Some(raw.clone()));
        assert_eq!(stack_name_component(&stack),Some(raw));
        set_machine_name_component(&mut machine,None);assert!(machine_name_component(&machine,None).is_none());
    }
}

include!("ae2_memory_emitter_host.rs");
include!("ae2_memory_energy_host.rs");
include!("ae2_memory_bus_host.rs");
