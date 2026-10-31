// One atomic transaction for a particular cable-bus face and its physical cards.
struct BusMemorySnapshot {
    index:usize,
    face:String,
    id:String,
    host:Machine,
    part:horizon_world::nbt::Compound,
    incarnation:u64,
    name:Option<String>,
    name_component:Option<Tag>,
}
impl Game<'_> {
    fn bus_memory_snapshot(&self,pos:[i32;3],face:&str)->Result<Option<BusMemorySnapshot>,String>{
        if !["east","west","up","down","south","north"].contains(&face){return Ok(None)}
        let key=(pos[0],pos[1],pos[2]);let Some(BlockEntity::Ae2(host))=self.world.block_entities.get(&key)else{return Ok(None)};
        if host.kind!=MachineKind::CableBus{return Ok(None)}
        let Some(Tag::List(parts))=host.config.get(&"parts".into())else{return Ok(None)};
        let mut found=parts.iter().enumerate().filter(|(_,p)|p.get("face")==Some(&Tag::String(face.into())));
        let Some((index,part))=found.next()else{return Ok(None)};
        if found.next().is_some(){return Err("duplicate bus face".into())}
        let Some(Tag::String(id))=part.get("id")else{return Ok(None)};let id=id.to_string().map_err(|_|"invalid bus id")?;
        if !["ae2:import_bus","ae2:export_bus","ae2:storage_bus","ae2:formation_plane"].contains(&id.as_str()){return Ok(None)}
        super::ae2_channels::part_identity(part).ok_or("invalid bus identity")?;
        let stack=part.get("stack").and_then(save::item_from_nbt).filter(|s|s.item.as_ref()==id&&s.count==1).ok_or("invalid bus stack")?;
        let Tag::Compound(part)=part else{return Err("invalid bus part".into())};
        super::ae2_buses::bus_state(part)?;
        Ok(Some(BusMemorySnapshot{index,face:face.to_owned(),id,host:(**host).clone(),part:part.clone(),incarnation:self.world.block_entities.incarnation(&key).ok_or("missing bus identity")?,name:stack.custom_name().map(str::to_owned),name_component:stack_name_component(&stack)}))
    }
    fn bus_memory_commit(&mut self,pos:[i32;3],snapshot:&BusMemorySnapshot,mut part:horizon_world::nbt::Compound,name:Option<Option<Tag>>)->Result<(),String>{
        let key=(pos[0],pos[1],pos[2]);if self.world.block_entities.incarnation(&key)!=Some(snapshot.incarnation){return Err("bus host identity changed".into())}
        if part.get(&"face".into())!=Some(&Tag::String(snapshot.face.as_str().into()))||part.get(&"id".into())!=Some(&Tag::String(snapshot.id.as_str().into())){return Err("bus face changed".into())}
        if let Some(name)=name {let mut stack=part.get(&"stack".into()).and_then(save::item_from_nbt).ok_or("invalid bus stack")?;stack.set_custom_name_component(name);part.insert("stack".into(),save::item_to_nbt(&stack));}
        let mut next=snapshot.host.clone();let Some(Tag::List(parts))=next.config.get_mut(&"parts".into())else{return Err("missing bus parts".into())};
        let slot=parts.get_mut(snapshot.index).ok_or("missing bus face")?;
        if *slot!=Tag::Compound(snapshot.part.clone()){return Err("bus face changed".into())}
        *slot=Tag::Compound(part);
        let Some(BlockEntity::Ae2(current))=self.world.block_entities.get_mut(&key)else{return Err("bus host disappeared".into())};
        if **current!=snapshot.host{return Err("bus host changed".into())}
        std::mem::swap(&mut **current,&mut next);self.world.modified.insert((pos[0]>>4,pos[2]>>4));Ok(())
    }
    fn ae2_try_bus_memory_face(&mut self,pos:[i32;3],face:&str)->bool{
        let Some(mut card)=self.held().filter(|s|s.item.as_ref()=="ae2:memory_card"&&s.count==1).cloned()else{return false};
        let snapshot=match self.bus_memory_snapshot(pos,face){Ok(None)=>return false,Ok(Some(s))=>s,Err(_)=>return true};
        if self.keys.contains(&super::KeyCode::ShiftLeft){
            if super::ae2_memory::clear(&mut card).is_err(){return true}
            if super::ae2_memory::save_bus(&mut card,&snapshot.part,snapshot.name.as_deref()).is_ok(){publish_memory_name(&mut card,snapshot.name_component.clone());horizon_client::ae2_stack_identity::synchronize_tracked(&card);*self.held_mut()=Some(card);self.system_message(horizon_client::text::Text::literal("Settings saved"));}
            return true
        }
        let original_card=card.clone();let card=memory_card_with_plain_name(&card);
        let saved=match super::ae2_memory::read_bus(&card,&snapshot.id){Ok(Some(v))=>v,_=>{self.system_message(horizon_client::text::Text::literal("Invalid machine"));return true}};
        if !saved.matching&&!super::ae2_memory::bus_has_components(&saved){self.system_message(horizon_client::text::Text::literal("Invalid machine"));return true}
        let found=match self.memory_tool(){Ok(v)=>v,Err(_)=>return true};let location=found.as_ref().map(|(loc,_)|*loc);let mut pocket=found.map(|(_,inv)|inv);
        let mut part=snapshot.part.clone();let mut inventory=self.inventory.clone();
        let mut changes=match super::ae2_memory::apply_bus(&mut part,&mut inventory,&mut pocket,&saved,self.creative){Ok(v)=>v,Err(_)=>return true};
        let mut armor=self.armor.clone();let mut offhand=self.offhand.clone();
        if let (Some(location),Some(pocket))=(location,pocket.as_ref()){
            let target=match location{ToolLocation::Inventory(i)=>inventory.get_mut(i).and_then(Option::as_mut),ToolLocation::Armor(i)=>armor.get_mut(i).and_then(Option::as_mut),ToolLocation::Offhand=>offhand.as_mut()};
            let Some(tool)=target else{return true};if pocket.store(tool).is_err(){return true}changes.changed_tracked.push(tool.clone());
        }
        let name=if saved.matching{Some(memory_card_name_component(&original_card,saved.name.as_deref()))}else{None};
        if self.bus_memory_commit(pos,&snapshot,part,name).is_err(){return true}
        self.inventory=inventory;self.armor=armor;self.offhand=offhand;
        for stack in changes.replaced_tracked{horizon_client::ae2_stack_identity::mark_detached_by_replacement(&stack,None);}
        for stack in changes.changed_tracked{horizon_client::ae2_stack_identity::synchronize_tracked(&stack);}
        for stack in changes.drops{self.player_drop(stack);}
        if changes.missing>0{self.system_message(horizon_client::text::Text::literal(format!("Missing {} upgrade cards",changes.missing)));}
        self.system_message(horizon_client::text::Text::literal(if saved.matching{"Settings loaded"}else{"Compatible settings restored"}));true
    }
}
