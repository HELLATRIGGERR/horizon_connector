// Staged to be included by ae2_memory_host.rs after the Stage 68 build.
// The threshold watcher callback API is finalized with the energy service integration.
struct EnergyEmitterMemorySnapshot {
    index: usize,
    face: String,
    address: super::ae2_parts::PartAddress,
    host: Machine,
    incarnation: u64,
    state: super::ae2_emitter::EnergyEmitterState,
    name: Option<String>,
    name_component:Option<Tag>,
}
impl Game<'_> {
    fn energy_emitter_memory_snapshot(&self,pos:[i32;3],face:&str)->Result<Option<EnergyEmitterMemorySnapshot>,String> {
        let Some(face_index)=["east","west","up","down","south","north"].iter().position(|v|*v==face) else{return Ok(None)};
        let key=(pos[0],pos[1],pos[2]);let Some(BlockEntity::Ae2(host))=self.world.block_entities.get(&key) else{return Ok(None)};
        if host.kind!=MachineKind::CableBus{return Ok(None)}
        let Some(Tag::List(parts))=host.config.get(&"parts".into()) else{return Ok(None)};
        let mut found=parts.iter().enumerate().filter(|(_,p)|p.get("face")==Some(&Tag::String(face.into())));
        let Some((index,part))=found.next() else{return Ok(None)};
        if found.next().is_some(){return Err("duplicate energy emitter face".into())}
        if part.get("id")!=Some(&Tag::String("ae2:energy_level_emitter".into())){return Ok(None)}
        super::ae2_channels::part_identity(part).ok_or("invalid energy emitter identity")?;
        let stack=part.get("stack").and_then(save::item_from_nbt).filter(|s|s.item.as_ref()=="ae2:energy_level_emitter"&&s.count==1).ok_or("invalid energy emitter stack")?;
        let Tag::Compound(data)=part else{return Err("invalid energy emitter part".into())};
        let address=super::ae2_parts::PartAddress{host:self.ae2_local_address(key).ok_or("invalid energy emitter address")?,face:face_index as u8};
        Ok(Some(EnergyEmitterMemorySnapshot{index,face:face.to_owned(),address,host:(**host).clone(),incarnation:self.world.block_entities.incarnation(&key).ok_or("missing emitter host identity")?,state:super::ae2_emitter::energy_emitter_read(data)?,name:stack.custom_name(),name_component:stack_name_component(&stack)}))
    }
    fn energy_emitter_memory_commit(&mut self,pos:[i32;3],snapshot:&EnergyEmitterMemorySnapshot,state:&super::ae2_emitter::EnergyEmitterState,name:Option<Option<Tag>>)->Result<(),String> {
        let key=(pos[0],pos[1],pos[2]);if self.world.block_entities.incarnation(&key)!=Some(snapshot.incarnation){return Err("energy emitter host changed".into())}
        let mut next=snapshot.host.clone();let Some(Tag::List(parts))=next.config.get_mut(&"parts".into()) else{return Err("missing energy emitter parts".into())};
        let Tag::Compound(part)=parts.get_mut(snapshot.index).ok_or("missing energy emitter")? else{return Err("invalid energy emitter part".into())};
        if part.get(&"face".into())!=Some(&Tag::String(snapshot.face.as_str().into())) || part.get(&"id".into())!=Some(&Tag::String("ae2:energy_level_emitter".into())) {return Err("energy emitter face changed".into())}
        super::ae2_emitter::energy_emitter_store(part,state)?;
        if let Some(name)=name {let mut stack=part.get(&"stack".into()).and_then(save::item_from_nbt).ok_or("invalid energy emitter stack")?;stack.set_custom_name_component(name);part.insert("stack".into(),save::item_to_nbt(&stack));}
        let Some(BlockEntity::Ae2(current))=self.world.block_entities.get_mut(&key) else{return Err("energy emitter host disappeared".into())};
        if **current!=snapshot.host{return Err("energy emitter changed during import".into())}
        std::mem::swap(&mut **current,&mut next);
        // Matching name/threshold and empty zero-slot upgrades alone do not
        // dirty the source chunk. The settings callback does so when it succeeds.
        Ok(())
    }
    fn ae2_try_energy_emitter_memory_face(&mut self,pos:[i32;3],face:&str)->bool {
        let Some(mut card)=self.held().filter(|s|s.item.as_ref()=="ae2:memory_card"&&s.count==1).cloned() else{return false};
        let snapshot=match self.energy_emitter_memory_snapshot(pos,face){Ok(None)=>return false,Ok(Some(v))=>v,Err(_)=>return true};
        if self.keys.contains(&super::KeyCode::ShiftLeft) {
            if super::ae2_memory::clear(&mut card).is_err(){return true}
            if super::ae2_memory::save_energy_emitter(&mut card,&snapshot.state,snapshot.name.as_deref()).is_ok() {publish_memory_name(&mut card,snapshot.name_component.clone());
                horizon_client::ae2_stack_identity::synchronize_tracked(&card);*self.held_mut()=Some(card);
                self.system_message(horizon_client::text::Text::literal("Settings saved"));
            }
            return true
        }
        let original_card=card.clone();let card=memory_card_with_plain_name(&card);
        let saved=match super::ae2_memory::read_energy_emitter(&card){Ok(Some(v))=>v,_=>{self.system_message(horizon_client::text::Text::literal("Invalid machine"));return true}};
        if !saved.matching && !super::ae2_memory::energy_emitter_has_components(&saved){self.system_message(horizon_client::text::Text::literal("Invalid machine"));return true}
        let mut state=snapshot.state.clone();super::ae2_memory::apply_energy_emitter(&mut state,&saved);
        let name=if saved.matching{Some(memory_card_name_component(&original_card,saved.name.as_deref()))}else{None};
        if self.energy_emitter_memory_commit(pos,&snapshot,&state,name).is_err(){return true}
        // Java imports generic settings before its source-specific threshold.
        // A negative existing watch can make the settings callback fail after
        // mutating the setting; the later threshold setter still runs.
        if saved.low.is_some() && self.ae2_energy_emitter_setting_callback(snapshot.address,snapshot.state.threshold) {
            self.world.modified.insert((pos[0]>>4,pos[2]>>4));
        }
        if saved.matching && saved.threshold.is_some(){self.ae2_energy_emitter_threshold_callback(snapshot.address);}
        self.system_message(horizon_client::text::Text::literal(if saved.matching{"Settings loaded"}else{"Compatible settings restored"}));true
    }
}
