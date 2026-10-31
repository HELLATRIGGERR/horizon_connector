// Atomic real Level Emitter face Memory Card transactions. Included by
// ae2_memory_host to share its player and Network Tool ownership helpers.
struct EmitterMemorySnapshot {
    index:usize,
    face:String,
    address:super::ae2_parts::PartAddress,
    host:Machine,
    incarnation:u64,
    state:super::ae2_emitter::EmitterState,
    name:Option<String>,
    name_component:Option<Tag>,
}
impl Game<'_> {
    fn emitter_memory_snapshot(&self,pos:[i32;3],face:&str)->Result<Option<EmitterMemorySnapshot>,String>{
        let Some(face_index)=["east","west","up","down","south","north"].iter().position(|v|*v==face) else{return Ok(None)};
        let key=(pos[0],pos[1],pos[2]);let Some(BlockEntity::Ae2(host))=self.world.block_entities.get(&key)else{return Ok(None)};
        if host.kind!=MachineKind::CableBus{return Ok(None)}
        let Some(Tag::List(parts))=host.config.get(&"parts".into())else{return Ok(None)};
        let mut found=parts.iter().enumerate().filter(|(_,p)|p.get("face")==Some(&Tag::String(face.into())));
        let Some((index,part))=found.next()else{return Ok(None)};
        if found.next().is_some(){return Err("duplicate emitter face".into())}
        if part.get("id")!=Some(&Tag::String("ae2:level_emitter".into())){return Ok(None)}
        super::ae2_channels::part_identity(part).ok_or("invalid emitter identity")?;
        let stack=part.get("stack").and_then(save::item_from_nbt).filter(|s|s.item.as_ref()=="ae2:level_emitter"&&s.count==1).ok_or("invalid emitter stack")?;
        let Tag::Compound(data)=part else{return Err("invalid emitter part".into())};
        let address=super::ae2_parts::PartAddress{host:self.ae2_local_address(key).ok_or("invalid emitter address")?,face:face_index as u8};
        Ok(Some(EmitterMemorySnapshot{index,face:face.to_owned(),address,host:(**host).clone(),incarnation:self.world.block_entities.incarnation(&key).ok_or("missing emitter host identity")?,state:super::ae2_emitter::emitter_read(data)?,name:stack.custom_name().map(str::to_owned),name_component:stack_name_component(&stack)}))
    }
    fn emitter_memory_commit(&mut self,pos:[i32;3],snapshot:&EmitterMemorySnapshot,state:&super::ae2_emitter::EmitterState,name:Option<Option<Tag>>)->Result<(),String>{
        let key=(pos[0],pos[1],pos[2]);if self.world.block_entities.incarnation(&key)!=Some(snapshot.incarnation){return Err("emitter host identity changed".into())}
        let mut next=snapshot.host.clone();let Some(Tag::List(parts))=next.config.get_mut(&"parts".into())else{return Err("missing emitter parts".into())};
        let Tag::Compound(part)=parts.get_mut(snapshot.index).ok_or("missing emitter part")? else{return Err("invalid emitter part".into())};
        if part.get(&"face".into())!=Some(&Tag::String(snapshot.face.as_str().into()))||part.get(&"id".into())!=Some(&Tag::String("ae2:level_emitter".into())){return Err("emitter face changed".into())}
        super::ae2_emitter::emitter_store(part,state)?;
        if let Some(name)=name{let mut stack=part.get(&"stack".into()).and_then(save::item_from_nbt).ok_or("invalid emitter stack")?;stack.set_custom_name_component(name);part.insert("stack".into(),save::item_to_nbt(&stack));}
        let Some(BlockEntity::Ae2(current))=self.world.block_entities.get_mut(&key)else{return Err("emitter host disappeared".into())};
        if **current!=snapshot.host{return Err("emitter changed during import".into())}
        std::mem::swap(&mut **current,&mut next);self.world.modified.insert((pos[0]>>4,pos[2]>>4));
        Ok(())
    }
    fn ae2_try_emitter_memory_face(&mut self,pos:[i32;3],face:&str)->bool{
        let Some(mut card)=self.held().filter(|s|s.item.as_ref()=="ae2:memory_card"&&s.count==1).cloned()else{return false};
        let snapshot=match self.emitter_memory_snapshot(pos,face){Ok(None)=>return false,Ok(Some(s))=>s,Err(_)=>return true};
        if self.keys.contains(&super::KeyCode::ShiftLeft){
            if super::ae2_memory::clear(&mut card).is_err(){return true}
            if super::ae2_memory::save_emitter(&mut card,&snapshot.state,snapshot.name.as_deref()).is_ok(){publish_memory_name(&mut card,snapshot.name_component.clone());horizon_client::ae2_stack_identity::synchronize_tracked(&card);*self.held_mut()=Some(card);self.system_message(horizon_client::text::Text::literal("Settings saved"));}
            return true
        }
        let original_card=card.clone();let card=memory_card_with_plain_name(&card);
        let saved=match super::ae2_memory::read_emitter(&card){Ok(Some(v))=>v,_=>{self.system_message(horizon_client::text::Text::literal("Invalid machine"));return true}};
        if !saved.matching&&!super::ae2_memory::emitter_has_components(&saved){self.system_message(horizon_client::text::Text::literal("Invalid machine"));return true}
        let found=match self.memory_tool(){Ok(v)=>v,Err(_)=>return true};let location=found.as_ref().map(|(loc,_)|*loc);let mut pocket=found.map(|(_,inv)|inv);
        let mut state=snapshot.state.clone();let mut inventory=self.inventory.clone();
        let mut changes=match super::ae2_memory::apply_emitter(&mut state,&mut inventory,&mut pocket,&saved,self.creative){Ok(v)=>v,Err(_)=>return true};
        let mut armor=self.armor.clone();let mut offhand=self.offhand.clone();
        if let (Some(location),Some(pocket))=(location,pocket.as_ref()){
            let target=match location{ToolLocation::Inventory(i)=>inventory.get_mut(i).and_then(Option::as_mut),ToolLocation::Armor(i)=>armor.get_mut(i).and_then(Option::as_mut),ToolLocation::Offhand=>offhand.as_mut()};
            let Some(tool)=target else{return true};if pocket.store(tool).is_err(){return true}changes.changed_tracked.push(tool.clone());
        }
        let name=if saved.matching{Some(memory_card_name_component(&original_card,saved.name.as_deref()))}else{None};
        // Settings callbacks fire even for a repeated value. A ghost callback
        // fires on actual key change; physical upgrade callbacks fire on the
        // source removal/install operations (creative may clear an empty slot).
        let refresh=saved.low.is_some()||saved.fuzzy.is_some()||saved.craft_via_redstone.is_some()
            ||saved.filter.is_some()&&state.filter!=snapshot.state.filter
            ||saved.desired.is_some()&&(self.creative||snapshot.state.upgrades[0].is_some()||state.upgrades!=snapshot.state.upgrades);
        if self.emitter_memory_commit(pos,&snapshot,&state,name).is_err(){return true}
        self.inventory=inventory;self.armor=armor;self.offhand=offhand;
        if refresh{self.ae2_emitter_provider_refresh(snapshot.address);}
        for stack in changes.replaced_tracked{horizon_client::ae2_stack_identity::mark_detached_by_replacement(&stack,None);}
        for stack in changes.changed_tracked{horizon_client::ae2_stack_identity::synchronize_tracked(&stack);}
        for stack in changes.drops{self.player_drop(stack);}
        if changes.missing>0{self.system_message(horizon_client::text::Text::literal(format!("Missing {} upgrade cards",changes.missing)));}
        self.system_message(horizon_client::text::Text::literal(if saved.matching{"Settings loaded"}else{"Compatible settings restored"}));true
    }
}
