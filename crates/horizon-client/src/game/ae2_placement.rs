//! AE2 19.2.18 facing/spin placement strategies (LGPL-3.0-or-later).
use super::Game;
use horizon_world::chunk::BlockState;

pub(super) fn placement(
    name: &str,
    yaw: f32,
    pitch: f32,
    face: [i32; 3],
) -> Option<(&'static str, Option<u8>)> {
    let horizontal = ["north", "east", "south", "west"]
        [((yaw / 90.0 + 0.5).floor() as i32).rem_euclid(4) as usize];
    if matches!(name,"ae2:wireless_access_point" | "ae2:growth_accelerator" | "ae2:spatial_anchor" | "ae2:crystal_resonance_generator") {
        let facing = match face {
            [0, -1, 0] => "down",
            [0, 1, 0] => "up",
            [0, 0, -1] => "north",
            [0, 0, 1] => "south",
            [-1, 0, 0] => "west",
            [1, 0, 0] => "east",
            _ => return None,
        };
        return Some((facing, None));
    }
    if matches!(name, "ae2:sky_stone_chest" | "ae2:smooth_sky_stone_chest") {
        // `horizontal` is already opposite the player's looking direction,
        // as required by HorizontalFacingStrategy.getStateForPlacement.
        return Some((horizontal,None));
    }
    if !matches!(name, "ae2:drive" | "ae2:charger" | "ae2:inscriber" | "ae2:vibration_chamber" | "ae2:io_port" | "ae2:spatial_io_port" | "ae2:cell_workbench" | "ae2:chest" | "ae2:crafting_monitor") {
        return None;
    }
    let opposite = match horizontal {
        "north" => "south",
        "south" => "north",
        "west" => "east",
        _ => "west",
    };
    let (facing, up) = if pitch > 65.0 {
        ("up", opposite)
    } else if pitch < -65.0 {
        ("down", opposite)
    } else {
        (horizontal, "up")
    };
    let dirs = match facing {
        "down" => ["north", "west", "south", "east"],
        "up" => ["north", "east", "south", "west"],
        "north" => ["up", "west", "down", "east"],
        "south" => ["up", "east", "down", "west"],
        "west" => ["up", "south", "down", "north"],
        _ => ["up", "north", "down", "south"],
    };
    Some((
        facing,
        Some(dirs.iter().position(|v| *v == up).unwrap_or(0) as u8),
    ))
}
/// ItemPlacementStrategy.PlaneDirectionalPlaceContext overrides horizontal
/// direction to NORTH for vertical planes. Platform.configurePlayer sets
/// pitch to 90 for both vertical directions; the clicked face opposes look.
pub(super) fn plane_placement(name: &str, look: [i32; 3]) -> Option<(&'static str, Option<u8>)> {
    let (yaw, pitch) = match look {
        [0,0,1] => (0.0,0.0), [-1,0,0] => (90.0,0.0),
        [0,0,-1] => (180.0,0.0), [1,0,0] => (270.0,0.0),
        [0,1,0] | [0,-1,0] => (180.0,90.0), _ => return None,
    };
    placement(name, yaw, pitch, [-look[0],-look[1],-look[2]])
}
/// Source BlockOrientation.rotateClockwiseAround: facing and up both rotate
/// a quarter turn around the clicked face, then are encoded as facing/spin.
fn wrench_rotation(facing:&str, spin:u8, axis:[i32;3]) -> Option<(&'static str,u8)> {
    fn vec(s:&str)->Option<[i32;3]>{Some(match s {"down"=>[0,-1,0],"up"=>[0,1,0],"north"=>[0,0,-1],"south"=>[0,0,1],"west"=>[-1,0,0],"east"=>[1,0,0],_=>return None})}
    fn name(v:[i32;3])->Option<&'static str>{Some(match v {[0,-1,0]=>"down",[0,1,0]=>"up",[0,0,-1]=>"north",[0,0,1]=>"south",[-1,0,0]=>"west",[1,0,0]=>"east",_=>return None})}
    fn ups(f:&str)->Option<[&'static str;4]>{Some(match f {"down"=>["north","west","south","east"],"up"=>["north","east","south","west"],"north"=>["up","west","down","east"],"south"=>["up","east","down","west"],"west"=>["up","south","down","north"],"east"=>["up","north","down","south"],_=>return None})}
    if axis.iter().map(|n|n.abs()).sum::<i32>()!=1 || spin>3{return None}
    let rotate=|v:[i32;3]| {let dot=axis[0]*v[0]+axis[1]*v[1]+axis[2]*v[2];let cross=[axis[1]*v[2]-axis[2]*v[1],axis[2]*v[0]-axis[0]*v[2],axis[0]*v[1]-axis[1]*v[0]];[axis[0]*dot-cross[0],axis[1]*dot-cross[1],axis[2]*dot-cross[2]]};
    let front=name(rotate(vec(facing)?))?;let up=name(rotate(vec(ups(facing)?[spin as usize])?))?;
    Some((front,ups(front)?.iter().position(|d|*d==up)? as u8))
}
/// PatternProviderBlock.setSide uses the clicked face as its rotation axis.
fn provider_side(current:&str, axis:[i32;3])->Option<&'static str> {
    fn vec(s:&str)->Option<[i32;3]>{Some(match s {"down"=>[0,-1,0],"up"=>[0,1,0],"north"=>[0,0,-1],"south"=>[0,0,1],"west"=>[-1,0,0],"east"=>[1,0,0],_=>return None})}
    fn name(v:[i32;3])->Option<&'static str>{Some(match v {[0,-1,0]=>"down",[0,1,0]=>"up",[0,0,-1]=>"north",[0,0,1]=>"south",[-1,0,0]=>"west",[1,0,0]=>"east",_=>return None})}
    if axis.iter().map(|v|v.abs()).sum::<i32>()!=1{return None}
    let opposite=[-axis[0],-axis[1],-axis[2]];
    if current=="all" {return name(opposite)}
    let d=vec(current)?;
    if d==opposite{return name(axis)}
    if d==axis{return Some("all")}
    name([d[1]*axis[2]-d[2]*axis[1],d[2]*axis[0]-d[0]*axis[2],d[0]*axis[1]-d[1]*axis[0]])
}
/// AE2 wireless access point connects to its BACK side only. Bits follow
/// network-core: east, west, up, down, south, north.
pub(super) fn wap_connection_faces(state: &BlockState) -> u8 {
    match state.properties.get("facing").map(String::as_str) {
        Some("east") => 1 << 1,
        Some("west") => 1 << 0,
        Some("up") => 1 << 3,
        Some("down") => 1 << 2,
        Some("south") => 1 << 5,
        Some("north") => 1 << 4,
        _ => 0,
    }
}
impl Game<'_> {
    pub(super) fn ae2_wrench_can_disassemble(&self,stack:&horizon_client::items::ItemStack)->bool {
        self.res.items().item_tags.get("c:tools/wrench").is_some_and(|tag|tag.contains(stack.item.as_ref()))
    }
    pub(super) fn ae2_wrench_can_rotate(&self,stack:&horizon_client::items::ItemStack)->bool {
        stack.item.as_ref()!="ae2:network_tool" && self.ae2_wrench_can_disassemble(stack)
    }
    /// Dismantle a physical AE2 block entity with single ownership of real
    /// items. CableBus is handled by its per-face multipart path.
    pub(super) fn ae2_try_wrench_disassemble(&mut self,pos:[i32;3])->bool {
        use horizon_client::{ae2_world::MachineKind,containers::BlockEntity};
        use horizon_world::nbt::Tag;
        if !self.keys.contains(&super::KeyCode::ShiftLeft) || !self.held().is_some_and(|s|self.ae2_wrench_can_disassemble(s)){return false;}
        let key=(pos[0],pos[1],pos[2]);let old=self.world.block(pos[0],pos[1],pos[2]);
        let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get(&key) else{return false};
        if matches!(machine.kind,MachineKind::CableBus|MachineKind::Paint) || self.res.items().get(machine.kind.id()).is_none() || self.res.facts().get(old).name.as_ref()!=machine.kind.id(){return false;}
        let max=match machine.kind {MachineKind::EnergyCell=>Some(200_000.0),MachineKind::DenseEnergyCell=>Some(1_600_000.0),_=>None};
        if max.is_some_and(|max|!machine.energy.is_finite() || machine.energy<0.0 || machine.energy>max) {return true;}
        // A formed CPU must settle its physical job ownership while the old
        // block entity still exists, before any inventory snapshot/removal.
        if self.ae2_prepare_cpu_removal(key).is_err(){return true;}
        let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get(&key) else{return true};
        let machine=(**machine).clone();let name=self.world.block_names.get(&key).cloned();
        let held=self.held().cloned();let mut drops=self.block_loot_at(pos,old,held.as_ref());
        if let Some(block)=drops.iter_mut().find(|item|item.item.as_ref()==machine.kind.id()) {
            block.set_custom_name_component(super::ae2_memory_host::machine_name_component(&machine,name.as_deref()));
            if matches!(machine.kind,MachineKind::EnergyCell|MachineKind::DenseEnergyCell) && machine.energy>0.0 {
                block.data_mut().set_other("ae2:stored_energy",Some(Tag::Double(machine.energy.to_bits())));
            }
        } else if matches!(machine.kind,MachineKind::EnergyCell|MachineKind::DenseEnergyCell) && machine.energy>0.0 {
            // Do not destroy stored energy when the block's loot is unavailable.
            return true;
        }
        drops.extend(super::spill::ae2_physical_drops_with_max(&machine,&|stack| {
            let default=self.res.items().get(&stack.item).map_or(64,|info|info.max_stack).max(1);
            super::ae2_formation::max_stack(stack,default)
        }));
        let entity=self.world.block_entities.remove(&key);let block_name=self.world.block_names.remove(&key);
        // A loaded chunk can still retain an older serialized BE. Remove that
        // copy too so generic block-removal fallback cannot spill it a second time.
        let saved={
            self.world.chunks.get_mut(&(pos[0]>>4,pos[2]>>4)).and_then(|chunk| {
                let saved=std::sync::Arc::make_mut(chunk).saved.as_mut()?;
                let list=&mut std::sync::Arc::make_mut(saved).block_entities;
                let at=list.iter().position(|tag|matches!(tag.get("x"),Some(Tag::Int(x)) if *x==pos[0]) && matches!(tag.get("y"),Some(Tag::Int(y)) if *y==pos[1]) && matches!(tag.get("z"),Some(Tag::Int(z)) if *z==pos[2]))?;
                Some((at,list.remove(at)))
            })
        };
        let result=self.level_set(pos,self.world.air);
        if self.world.block(pos[0],pos[1],pos[2])==old {
            if let Some(entity)=entity {self.world.block_entities.insert(key,entity);}
            if let Some(name)=block_name {self.world.block_names.insert(key,name);}
            if let Some((index,tag))=saved {if let Some(chunk)=self.world.chunks.get_mut(&(pos[0]>>4,pos[2]>>4)) {if let Some(saved)=std::sync::Arc::make_mut(chunk).saved.as_mut(){let list=&mut std::sync::Arc::make_mut(saved).block_entities;list.insert(index.min(list.len()),tag);}}}
            return result.is_ok();
        }
        for item in drops {if let Some(rest)=self.give(item){self.pop_resource(pos,rest);}}
        true
    }
    /// Rotate supported orientation-bearing AE2 blocks with a main-hand wrench.
    pub(super) fn ae2_try_wrench_rotate(&mut self,pos:[i32;3],face:[i32;3])->bool {
        if self.keys.contains(&super::KeyCode::ShiftLeft) || !self.held().is_some_and(|s|self.ae2_wrench_can_rotate(s)){return false;}
        let old=self.world.block(pos[0],pos[1],pos[2]);
        let Some(state)=self.pack.assets.registry.state(old) else{return false};
        if state.name=="ae2:pattern_provider" {
            let Some(next)=state.properties.get("push_direction").and_then(|v|provider_side(v,face)) else{return false};
            let mut props=state.properties.clone();props.insert("push_direction".into(),next.into());
            let Ok(id)=self.pack.assets.registry.resolve(&BlockState{name:state.name.clone(),properties:props}) else{return false};
            return self.level_set(pos,id).is_ok();
        }
        if !matches!(state.name.as_str(),"ae2:drive"|"ae2:charger"|"ae2:inscriber"|"ae2:wireless_access_point"|"ae2:crystal_resonance_generator"|"ae2:sky_stone_chest"|"ae2:smooth_sky_stone_chest"|"ae2:growth_accelerator"|"ae2:vibration_chamber"|"ae2:io_port"|"ae2:spatial_io_port"|"ae2:cell_workbench"|"ae2:chest"|"ae2:spatial_anchor"|"ae2:crafting_monitor"){return false;}
        let Some(facing)=state.properties.get("facing") else{return false};
        let spin=state.properties.get("spin").and_then(|s|s.parse().ok()).unwrap_or(0);
        let Some((front,new_spin))=wrench_rotation(facing,spin,face) else{return false};
        let mut props=state.properties.clone();props.insert("facing".into(),front.into());
        if props.contains_key("spin"){props.insert("spin".into(),new_spin.to_string());}
        let Ok(id)=self.pack.assets.registry.resolve(&BlockState{name:state.name.clone(),properties:props}) else{return false};
        if id==old{return false;}
        let survives=self.with_level(|b,level,ticks|b.can_survive(level,ticks,horizon_core::position::BlockPos::new(pos[0],pos[1],pos[2]),id));
        if !survives{return false;}
        self.level_set(pos,id).is_ok()
    }
    pub(super) fn ae2_orient_placed_block(
        &mut self,
        pos: [i32; 3],
        face: [i32; 3],
        yaw: f32,
        pitch: f32,
        waterlogged: bool,
    ) {
        self.ae2_orient_placed_block_using(pos,waterlogged,|name|placement(name,yaw,pitch,face));
    }
    pub(super) fn ae2_orient_placed_block_from_plane(&mut self, pos:[i32;3], look:[i32;3], waterlogged:bool) {
        self.ae2_orient_placed_block_using(pos,waterlogged,|name|plane_placement(name,look));
    }
    fn ae2_orient_placed_block_using(
        &mut self, pos:[i32;3], waterlogged:bool,
        orientation:impl FnOnce(&str)->Option<(&'static str,Option<u8>)>,
    ) {
        let old = self.world.block(pos[0], pos[1], pos[2]);
        let Some(state) = self.pack.assets.registry.state(old) else {
            return;
        };
        let Some((facing, spin)) = orientation(&state.name) else {
            return;
        };
        let mut props = state.properties.clone();
        props.insert("facing".into(), facing.into());
        if let Some(spin) = spin {
            props.insert("spin".into(), spin.to_string());
        }
        if props.contains_key("waterlogged") {
            props.insert("waterlogged".into(), waterlogged.to_string());
        }
        if let Ok(id) = self.pack.assets.registry.resolve(&BlockState {
            name: state.name.clone(),
            properties: props,
        }) {
            let _ = self.level_set(pos, id);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_wrench_rotates_facing_and_spin_on_all_axes() {
        assert_eq!(wrench_rotation("north",0,[0,1,0]),Some(("east",0)));
        assert_eq!(wrench_rotation("north",0,[0,0,-1]),Some(("north",1)));
        assert_eq!(wrench_rotation("north",1,[0,0,1]),Some(("north",0)));
        for face in [[0,1,0],[0,-1,0],[1,0,0],[-1,0,0],[0,0,1],[0,0,-1]] {
            let mut state=("north",0);for _ in 0..4 {state=wrench_rotation(state.0,state.1,face).unwrap();}assert_eq!(state,("north",0));
        }
    }
    #[test]
    fn source_provider_side_cycle() {
        assert_eq!(provider_side("all",[0,0,-1]),Some("south"));
        assert_eq!(provider_side("south",[0,0,-1]),Some("north"));
        assert_eq!(provider_side("north",[0,0,-1]),Some("all"));
        assert_eq!(provider_side("east",[0,1,0]),Some("south"));
    }
    #[test]
    fn source_placement_uses_player_facing_pitch_and_clicked_face() {
        for (yaw, facing) in [(0.0,"north"),(90.0,"east"),(180.0,"south"),(270.0,"west")] {
            for name in ["ae2:sky_stone_chest", "ae2:smooth_sky_stone_chest"] {
                assert_eq!(placement(name, yaw, 80.0, [0,1,0]), Some((facing,None)));
            }
            assert_eq!(placement("ae2:crafting_monitor",yaw,0.0,[0,1,0]),Some((facing,Some(0))));
        }
        assert_eq!(placement("ae2:crafting_monitor",90.0,75.0,[0,1,0]),Some(("up",Some(3))));
        assert_eq!(
            placement("ae2:drive", 0.0, 0.0, [0, 1, 0]),
            Some(("north", Some(0)))
        );
        assert_eq!(
            placement("ae2:drive", 90.0, 75.0, [0, 1, 0]),
            Some(("up", Some(3)))
        );
        assert_eq!(
            placement("ae2:drive", 0.0, -75.0, [0, 1, 0]),
            Some(("down", Some(2)))
        );
        assert_eq!(
            placement("ae2:wireless_access_point", 0.0, 0.0, [1, 0, 0]),
            Some(("east", None))
        );
    }
    #[test]
    fn source_plane_context_uses_the_six_look_directions() {
        for (look, facing, clicked) in [
            ([0,0,1],"north","north"), ([0,0,-1],"south","south"),
            ([1,0,0],"west","west"), ([-1,0,0],"east","east"),
            ([0,1,0],"up","down"), ([0,-1,0],"up","up"),
        ] {
            for item in ["ae2:drive","ae2:crafting_monitor"] {
                assert_eq!(plane_placement(item,look),Some((facing,Some(0))));
            }
            let sky = if look[1] != 0 { "south" } else { facing };
            assert_eq!(plane_placement("ae2:sky_stone_chest",look),Some((sky,None)));
            assert_eq!(plane_placement("ae2:wireless_access_point",look),Some((clicked,None)));
        }
        assert_eq!(plane_placement("ae2:drive",[0,0,0]),None);
    }
}
