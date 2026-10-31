//! AE2 19.2.18 AdvancementTriggers and node owning-player attribution.
//! Runtime bracket state is not saved; physical owner UUIDs are saved on nodes.
use super::{Game,ae2_global::GridAddress};
use ae2_network_core::NodeIdentity;
use horizon_client::containers::BlockEntity;
use horizon_world::nbt::Tag;
use std::{collections::{BTreeMap,BTreeSet},path::PathBuf};
const OWNER:&str="ae2_node_owner_uuid";
type Uuid=[i32;4];
#[derive(Default)]
pub(super) struct Service {world:PathBuf,brackets:BTreeMap<u64,Option<&'static str>>,pending:BTreeSet<(Uuid,&'static str)>,pending_connections:BTreeSet<(u32,&'static str)>}
fn owner(tag:Option<&Tag>)->Option<Uuid>{let Tag::IntArray(values)=tag? else{return None;};values.as_slice().try_into().ok()}
fn bracket(channels:usize)->Option<&'static str>{match channels{0..8=>None,8..128=>Some("ae2:network_apprentice"),128..2048=>Some("ae2:network_engineer"),_=>Some("ae2:network_admin")}}
fn face(tag:&Tag)->Option<u8>{let Tag::String(face)=tag.get("face")? else{return None;};Some(match face.to_string_lossy().as_str(){"east"=>0,"west"=>1,"up"=>2,"down"=>3,"south"=>4,"north"=>5,_=>return None})}
impl Game<'_>{
 pub(super) fn ae2_record_node_owner(&mut self,pos:[i32;3]) {
  let id=self.acting_uuid();let key=(pos[0],pos[1],pos[2]);
  if let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get_mut(&key){machine.config.insert(OWNER.into(),Tag::IntArray(id.to_vec()));self.world.modified.insert((pos[0]>>4,pos[2]>>4));}
 }
 pub(super) fn ae2_player_owner_tag(&self)->(&'static str,Tag){(OWNER,Tag::IntArray(self.acting_uuid().to_vec()))}
 pub(super) fn ae2_queue_connection_advancement(&mut self,conn:u32,trigger:&'static str){
  if self.ae2_advancement_service.world!=self.world_root {self.ae2_advancement_service=Service{world:self.world_root.clone(),..Default::default()};}
  self.ae2_advancement_service.pending_connections.insert((conn,trigger));
 }
 pub(super) fn ae2_queue_advancement(&mut self,uuid:Uuid,trigger:&'static str){
  if self.ae2_advancement_service.world!=self.world_root {self.ae2_advancement_service=Service{world:self.world_root.clone(),..Default::default()};}
  self.ae2_advancement_service.pending.insert((uuid,trigger));
 }
 pub(super) fn ae2_node_player_owners(&self,network:&ae2_network_core::Network)->BTreeMap<NodeIdentity,Uuid>{
  let mut owners=BTreeMap::new();
  for(dim,world)in self.ae2_loaded_worlds(){for(&pos,entity)in world.block_entities.iter(){let BlockEntity::Ae2(machine)=entity else{continue;};let Some(address)=GridAddress::from_local(dim,pos) else{continue;};let host=address.network_pos();
   if let (Some(identity),Some(uuid))=(network.node_identities.get(&host),owner(machine.config.get(&OWNER.into()))){owners.insert(*identity,uuid);}
   if let Some(Tag::List(parts))=machine.config.get(&"parts".into()){for part in parts{if let(Some(face),Some(uuid))=(face(part),owner(part.get(OWNER))){if let Some(identity)=network.part_identities.get(&(host,face)){owners.insert(*identity,uuid);owners.insert(NodeIdentity{outer:true,..*identity},uuid);}}}}
  }}owners
 }
 pub(super) fn ae2_check_network_advancements(&mut self){
  if self.remote.is_some() || !self.level_pass.last{return;}
  let network=self.ae2_network();let Ok(analysis)=network.analyze()else{return;};let Some(services)=self.ae2_event_service_ids(&network)else{return;};
  let owners=self.ae2_node_player_owners(&network);let mut components:BTreeMap<u64,(usize,BTreeSet<Uuid>)>=BTreeMap::new();
  for(pos,identity)in &network.node_identities{let(Some(service),Some(status),Some(device))=(services.get(identity),analysis.nodes.get(pos),network.nodes.get(pos))else{continue;};
   let count=analysis.components[status.component].channels_in_use;
   let entry=components.entry(*service).or_default();entry.0=entry.0.max(count);
   if device.channel_override.unwrap_or_else(||device.kind.requires_channel()){if let Some(uuid)=owners.get(identity){entry.1.insert(*uuid);}}
  }
  if self.ae2_advancement_service.world!=self.world_root{self.ae2_advancement_service=Service{world:self.world_root.clone(),..Default::default()};}
  self.ae2_advancement_service.brackets.retain(|service,_|components.contains_key(service));
  for(service,(channels,owners))in components{let current=bracket(channels);let previous=self.ae2_advancement_service.brackets.insert(service,current).flatten();if current!=previous{if let Some(trigger)=current{for uuid in owners{self.ae2_advancement_service.pending.insert((uuid,trigger));}}}}
 }
 pub(super) fn ae2_flush_advancements(&mut self,host:&mut super::lan::LanHost){
  let connections=std::mem::take(&mut self.ae2_advancement_service.pending_connections);
  for(conn,trigger)in connections {if host.hub.guests.iter().any(|guest|guest.conn==conn){self.as_guest(host,conn,|game|game.trigger_player(trigger));}}
  let pending=std::mem::take(&mut self.ae2_advancement_service.pending);
  for(uuid,trigger)in pending{if let Some(conn)=host.hub.guests.iter().find(|guest|horizon_client::lan::playerdata::uuid_ints(guest.uuid)==uuid).map(|guest|guest.conn){self.as_guest(host,conn,|game|game.trigger_player(trigger));}else if self.options.dedicated.is_none()&&self.level.player_uuid==Some(uuid){self.trigger_player(trigger);}}
 }
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn source_network_brackets_and_owner_uuid_are_exact(){for(count,expected)in[(0,None),(7,None),(8,Some("ae2:network_apprentice")),(127,Some("ae2:network_apprentice")),(128,Some("ae2:network_engineer")),(2047,Some("ae2:network_engineer")),(2048,Some("ae2:network_admin"))]{assert_eq!(bracket(count),expected);}assert_eq!(owner(Some(&Tag::IntArray(vec![1,2,3,4]))),Some([1,2,3,4]));assert_eq!(owner(Some(&Tag::IntArray(vec![1,2,3]))),None);}
}
