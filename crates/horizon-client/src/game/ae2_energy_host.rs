//! Real face-owned Energy Emitter callbacks and redstone publication.
use super::{Game,ae2_parts::PartAddress};
use ae2_network_core::NodeIdentity;
use horizon_client::containers::BlockEntity;
use horizon_world::nbt::Tag;
impl Game<'_> {
    pub(super) fn ae2_energy_service_end_tick(&self){
        if !self.ae2_sync_events(){return}
        self.ae2_live_events.borrow_mut().energy.end_tick();
    }
    /// Read-only diagnostic: do not refresh the source cache while inspecting
    /// its age or comparing it to physical storage.
    pub(super) fn ae2_energy_storage_diagnostic(&self,address:super::ae2_global::GridAddress)->Option<serde_json::Value>{
        if !self.ae2_sync_events(){return None}
        let (dim,pos)=self.ae2_address_parts(address)?;
        let world=self.ae2_world_for(dim)?;
        let node=NodeIdentity{host:world.block_entities.incarnation(&pos)?,part:None,outer:false};
        let live=self.ae2_live_events.borrow();
        if live.energy.failure.is_some(){return None}
        let service=live.energy.lifecycle.services.get(&live.energy.lifecycle.service_of(node)?)?;
        Some(serde_json::json!({"cached":service.cached.max(0.0),"age":service.age,"buffer":service.local(),"buffer_capacity":service.local_capacity(),"members":service.nodes(),"passive_suppressed":live.energy.passive_suppressed.get(&node).copied()}))
    }
    /// Record a direct physical cell change after earlier node events have
    /// been drained. This is not a grid injection and does not raise the cache.
    pub(super) fn ae2_energy_store_changed(&self, address:super::ae2_global::GridAddress, legacy_before:Option<f64>) {
        let Some((dim,pos))=self.ae2_address_parts(address) else{return};
        let Some(world)=self.ae2_world_for(dim) else{return};
        let Some(host)=world.block_entities.incarnation(&pos) else{return};
        let Some(BlockEntity::Ae2(machine))=world.block_entities.get(&pos) else{return};
        let id=NodeIdentity{host,part:None,outer:false};
        let mut live=self.ae2_live_events.borrow_mut();
        if live.energy.failure.is_some()||!live.energy.physical.contains_key(&id){return}
        let result=match legacy_before {
            Some(before)=>live.energy.lifecycle.reconcile_legacy(id,before,machine.energy),
            None=>live.energy.lifecycle.update_external(id,machine.energy),
        };
        if let Err(error)=result {log::warn!("AE2 energy storage observation failed: {error}");live.energy.failure=Some(error.into());}
    }
    /// Project source saveNodeData into a cloned save batch. Existing unrelated
    /// node tags survive; a zero share removes an old saved share.
    pub(super) fn ae2_project_saved_energy(&self,chunk:&mut horizon_client::chunk::Chunk) {
        if !self.ae2_sync_events(){return}
        let live=self.ae2_live_events.borrow();
        if live.energy.failure.is_some(){return}
        fn project(data:&mut horizon_world::nbt::Compound,key:&str,share:f64){
            if data.get(&key.into()).is_some_and(|tag|!matches!(tag,Tag::Compound(_))){return}
            if share>0.0 {
                let value=data.entry(key.into()).or_insert_with(||Tag::Compound(horizon_world::nbt::Compound::new()));
                if let Tag::Compound(node)=value {node.insert("e".into(),Tag::Double(share.to_bits()));}
            } else if let Some(Tag::Compound(node))=data.get_mut(&key.into()){node.remove(&"e".into());}
        }
        for (pos,entity) in &mut chunk.block_entities {
            let Some(host)=self.world.block_entities.incarnation(pos) else{continue};
            let BlockEntity::Ae2(machine)=entity else{continue};
            let id=NodeIdentity{host,part:None,outer:false};
            let tag=if machine.kind==horizon_client::ae2_world::MachineKind::DebugPhantomNode{"proxy"}else{"gn"};
            if let Ok(share)=live.energy.lifecycle.saved_share(id){project(&mut machine.config,tag,share);}
            if let Some(Tag::List(parts))=machine.config.get_mut(&"parts".into()) {
                for part in parts {
                    let Some(part_id)=super::ae2_channels::part_identity(part) else{continue};
                    let Tag::Compound(data)=part else{continue};
                    for outer in [false,true] {
                        let id=NodeIdentity{host,part:Some(part_id),outer};
                        if let Ok(share)=live.energy.lifecycle.saved_share(id){project(data,if outer{"outer"}else{"gn"},share);}
                    }
                }
            }
        }
    }
    fn ae2_energy_face_context(&self,address:PartAddress)->Option<(NodeIdentity,super::ae2_emitter::EnergyEmitterState,bool)> {
        let (dim,pos)=self.ae2_address_parts(address.host)?;
        let world=self.ae2_world_for(dim)?;let host=world.block_entities.incarnation(&pos)?;
        let BlockEntity::Ae2(machine)=world.block_entities.get(&pos)? else{return None};
        let Tag::List(parts)=machine.config.get(&"parts".into())? else{return None};
        let face=["east","west","up","down","south","north"].get(address.face as usize)?;
        let part=parts.iter().find(|p|p.get("face")==Some(&Tag::String((*face).into()))&&p.get("id")==Some(&Tag::String("ae2:energy_level_emitter".into())))?;
        let id=super::ae2_channels::part_identity(part)?;let Tag::Compound(data)=part else{return None};
        let state=super::ae2_emitter::energy_emitter_read(data).ok()?;
        let network=self.ae2_network();let analysis=network.analyze().ok()?;
        let online=network.part_status(&analysis,address.host.network_pos(),address.face).is_some_and(|p|p.online);
        Some((NodeIdentity{host,part:Some(id),outer:false},state,online))
    }
    fn ae2_energy_publish(&mut self,address:PartAddress,node:NodeIdentity,reported:i64,output:bool)->bool {
        let Some((dim,pos))=self.ae2_address_parts(address.host) else{return false};
        let Some(world)=self.ae2_world_for_mut(dim) else{return false};
        if world.block_entities.incarnation(&pos)!=Some(node.host){return false}
        let Some(BlockEntity::Ae2(machine))=world.block_entities.get_mut(&pos) else{return false};
        let Some(Tag::List(parts))=machine.config.get_mut(&"parts".into()) else{return false};
        let Some(face)=["east","west","up","down","south","north"].get(address.face as usize) else{return false};
        let Some(Tag::Compound(part))=parts.iter_mut().find(|p|p.get("face")==Some(&Tag::String((*face).into()))&&super::ae2_channels::part_identity(p)==node.part) else{return false};
        if part.get(&"id".into())!=Some(&Tag::String("ae2:energy_level_emitter".into())){return false}
        let changed=part.get(&"emitter_active".into())!=Some(&Tag::Byte(i8::from(output)));
        part.insert("emitter_count".into(),Tag::Long(reported));part.insert("emitter_active".into(),Tag::Byte(i8::from(output)));
        if changed && dim==self.dim {
            let at=horizon_core::position::BlockPos::new(pos.0,pos.1,pos.2);
            let delta=match address.face{0=>(1,0,0),1=>(-1,0,0),2=>(0,1,0),3=>(0,-1,0),4=>(0,0,1),_=>(0,0,-1)};
            let neighbor=horizon_core::position::BlockPos::new(pos.0+delta.0,pos.1+delta.1,pos.2+delta.2);
            let block=self.world.block(pos.0,pos.1,pos.2);
            self.with_level(|b,level,ticks|{b.update_neighbors_at(level,ticks,at,b.block(block));b.update_neighbors_at(level,ticks,neighbor,b.block(block));});
        }
        true
    }
    pub(super) fn ae2_energy_emitter_setting_callback(&mut self,address:PartAddress,previous_threshold:i64)->bool {
        if !self.ae2_sync_events(){return false}
        let Some((node,state,online))=self.ae2_energy_face_context(address) else{return false};
        let value={let mut live=self.ae2_live_events.borrow_mut();live.energy.setting(node,state,previous_threshold,online)};
        match value {Ok((reported,output))=>self.ae2_energy_publish(address,node,reported,output),Err(error)=>{log::warn!("AE2 energy emitter setting: {error}");false}}
    }
    pub(super) fn ae2_energy_emitter_threshold_callback(&mut self,address:PartAddress) {
        if !self.ae2_sync_events(){return}
        let Some((node,state,online))=self.ae2_energy_face_context(address) else{return};
        let value={let mut live=self.ae2_live_events.borrow_mut();live.energy.threshold(node,state,online)};
        if let Ok((reported,output))=value{self.ae2_energy_publish(address,node,reported,output);}
    }
}
