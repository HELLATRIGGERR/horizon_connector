//! AE2 19.2.18 ReplicatorCardItem, derived from Applied Energistics 2
//! Copyright (c) 2013-2014 AlgorithmX2; LGPL-3.0-or-later.
use super::{Dim, Game, KeyCode, ae2_global::GridAddress};
use ae2_links_core::SpatialRegion;
use horizon_client::{containers::BlockEntity, items::ItemStack, text::Text};
use horizon_core::position::BlockPos;
use horizon_world::nbt::{Compound, Tag};

const ITEM: &str = "ae2:debug_replicator_card";

fn custom(stack: &ItemStack) -> Result<Compound, String> {
    match stack.data().and_then(|d| d.other("minecraft:custom_data")) {
        None => Ok(Compound::new()),
        Some(Tag::Compound(tag)) => Ok(tag.clone()),
        _ => Err("Invalid replicator card data".into()),
    }
}
fn number(tag: &Compound, key: &str) -> i32 {
    match tag.get(&key.into()) {
        Some(Tag::Byte(v)) => i32::from(*v),
        Some(Tag::Short(v)) => i32::from(*v),
        Some(Tag::Int(v)) => *v,
        Some(Tag::Long(v)) => *v as i32,
        Some(Tag::Float(v)) => f32::from_bits(*v).floor() as i32,
        Some(Tag::Double(v)) => f64::from_bits(*v).floor() as i32,
        _ => 0,
    }
}
fn store(stack: &mut ItemStack, tag: Compound) {
    stack.data_mut().set_other("minecraft:custom_data", Some(Tag::Compound(tag)));
}
fn cycle(stack: &mut ItemStack) -> Result<i32, String> {
    let mut tag = custom(stack)?;
    let value = if tag.contains_key(&"r".into()) { number(&tag, "r").wrapping_add(1) % 4 } else { 0 };
    tag.insert("r".into(), Tag::Int(value));
    store(stack, tag);
    Ok(value + 1)
}
fn face_ordinal(face: [i32;3]) -> Option<i32> {
    Some(match face { [0,-1,0] => 0, [0,1,0] => 1, [0,0,-1] => 2,
        [0,0,1] => 3, [-1,0,0] => 4, [1,0,0] => 5, _ => return None })
}
fn network_face(face: i32) -> Option<u8> {
    [3,2,5,4,1,0].get(usize::try_from(face).ok()?).copied()
}
fn dimension(value: &str) -> Option<Dim> {
    Some(match value { "minecraft:overworld" => Dim::Overworld,
        "minecraft:the_nether" => Dim::Nether, "minecraft:the_end" => Dim::End,
        "ae2:spatial_storage" => Dim::Spatial, _ => return None })
}
fn wrap_degrees(value: f32) -> f32 {
    let mut result = value % 360.0;
    if result >= 180.0 { result -= 360.0; }
    if result < -180.0 { result += 360.0; }
    result
}
fn sign(value: f32) -> i32 { if value > 0.0 { 1 } else if value < 0.0 { -1 } else { 0 } }

/// Preserve Java's rx/ry/rz, then i/j/k iteration, including overlapping
/// copies. This calls the reader for each pair, rather than freezing a plane.
fn visit_pairs(
    min: [i32;3], max: [i32;3], source: [i32;3], target: [i32;3],
    count: i32, yaw: f32, height: (i32,i32), mut visit: impl FnMut([i32;3],[i32;3])->Result<(),String>,
) -> Result<(),String> {
    if !(1..=4).contains(&count) || !yaw.is_finite() { return Err("Invalid replicator settings".into()); }
    let mut size = [0i32;3];
    for axis in 0..3 { size[axis] = max[axis].checked_sub(min[axis]).ok_or("Invalid spatial region")?;
        if size[axis] < 2 { return Err("Invalid spatial region".into()); }
    }
    let direction = [-sign(wrap_degrees(yaw)), 1, sign(wrap_degrees(yaw + 90.0))];
    for axis in 0..3 {
        let first = i64::from(min[axis]) - i64::from(source[axis]) + i64::from(target[axis]) + 1;
        let last = first + i64::from(size[axis] - 2);
        let delta = i64::from(count-1) * i64::from(size[axis]) * i64::from(direction[axis]);
        let lo = first + delta.min(0);
        let hi = last + delta.max(0);
        let (bound_lo,bound_hi) = if axis == 1 { (i64::from(height.0), i64::from(height.1)) }
            else { (-29_999_999, 29_999_999) };
        if lo < bound_lo || hi > bound_hi { return Err("Replicator destination exceeds world bounds".into()); }
    }
    for rx in 0..count { for ry in 0..count { for rz in 0..count {
        let repeats = [rx,ry,rz];
        let mut base = [0i32;3];
        for axis in 0..3 { base[axis] = (i64::from(min[axis]) - i64::from(source[axis]) + i64::from(target[axis])
            + i64::from(repeats[axis]) * i64::from(size[axis]) * i64::from(direction[axis])) as i32; }
        for i in 1..size[0] { for j in 1..size[1] { for k in 1..size[2] {
            visit([min[0]+i,min[1]+j,min[2]+k],[base[0]+i,base[1]+j,base[2]+k])?;
        }}}
    }}}
    Ok(())
}

impl Game<'_> {
    fn ae2_replicator_held(&self) -> bool {
        self.remote.is_none() && self.held().is_some_and(|s| s.item.as_ref()==ITEM && s.count>0)
    }
    pub(super) fn ae2_try_replicator_cycle(&mut self) -> bool {
        if !self.ae2_replicator_held() { return false; }
        let result = cycle(self.held_mut().as_mut().expect("checked held item"));
        self.system_message(Text::literal(match result {
            Ok(n) => format!("{n}³ Replications"), Err(e) => e,
        }));
        true
    }
    pub(super) fn ae2_try_replicator(&mut self, clicked: [i32;3], face: [i32;3]) -> bool {
        if !self.ae2_replicator_held() { return false; }
        let result = if self.keys.contains(&KeyCode::ShiftLeft) {
            self.ae2_replicator_bind(clicked, face).map(|_| Some("Set replicator source".to_owned()))
        } else {
            self.ae2_replicator_copy(clicked).map(|_| None)
        };
        match result { Ok(Some(message)) | Err(message) => self.system_message(Text::literal(message)), Ok(None) => {} }
        true
    }
    fn ae2_replicator_bind(&mut self, pos: [i32;3], face: [i32;3]) -> Result<(),String> {
        let key=(pos[0],pos[1],pos[2]);
        if !matches!(self.world.block_entities.get(&key),Some(BlockEntity::Ae2(m)) if super::ae2_grid::kind(m).is_some()) {
            return Err("This does not host a grid node".into());
        }
        let side=face_ordinal(face).ok_or("Invalid selected side")?;
        let world=self.dim.id();
        let stack=self.held_mut().as_mut().expect("checked held item");
        let mut tag=custom(stack)?;
        for (key,value) in [("x",pos[0]),("y",pos[1]),("z",pos[2]),("side",side),("r",0)] {
            tag.insert(key.into(),Tag::Int(value));
        }
        tag.insert("w".into(),Tag::String(world.into()));
        store(stack,tag);
        Ok(())
    }
    fn ae2_replicator_copy(&mut self, target: [i32;3]) -> Result<(),String> {
        let tag=custom(self.held().expect("checked held item"))?;
        if tag.is_empty() { return Err("No Source Defined".into()); }
        let source=[number(&tag,"x"),number(&tag,"y"),number(&tag,"z")];
        let face=network_face(number(&tag,"side")).ok_or("Invalid source side")?;
        let dim=match tag.get(&"w".into()) { Some(Tag::String(value)) => value.to_string().ok().and_then(|s|dimension(&s)), _ => None }
            .ok_or("Invalid source dimension")?;
        let count=number(&tag,"r").checked_add(1).ok_or("Invalid replication count")?;
        if !(1..=4).contains(&count) { return Err("Invalid replication count".into()); }
        let target_dim=self.dim;
        let target_extra=self.streaming.extra.clone();
        self.enter_level(None,dim).map_err(|e|e.to_string())?;
        let source_extra=self.streaming.extra.clone();
        let result=(|| {
            self.ae2_demand_column(source[0]>>4,source[2]>>4)?;
            let address=GridAddress::from_local(dim,(source[0],source[1],source[2])).ok_or("Invalid source position")?;
            if !matches!(self.ae2_block_entity(address),Some(BlockEntity::Ae2(m)) if super::ae2_grid::kind(m).is_some()) {
                return Err("Src is no longer a grid block?".into());
            }
            let region:SpatialRegion=self.ae2_spatial_region_from(address,Some(face))
                .map_err(|e| if e=="No grid node?" || e=="spatial I/O port is offline" { "No grid node?".to_owned() } else { "requires valid spatial pylon setup.".to_owned() })?;
            let min=[region.min.0,region.min.1,region.min.2];
            let max=[region.max.0,region.max.1,region.max.2];
            for cx in (min[0]+1)>>4..=(max[0]-1)>>4 { for cz in (min[2]+1)>>4..=(max[2]-1)>>4 {
                self.ae2_demand_column(cx,cz)?;
            }}
            self.enter_level(None,target_dim).map_err(|e|e.to_string())?;
            let yaw=self.yaw;
            let height=(target_dim.min_y(),target_dim.max_y());
            visit_pairs(min,max,source,target,count,yaw,height,|from,to| {
                self.ae2_demand_column(to[0]>>4,to[2]>>4)?;
                let key=(from[0],from[1],from[2]);
                let source_world=self.ae2_world_for(dim).ok_or("Source dimension is unavailable")?;
                let state=source_world.block(from[0],from[1],from[2]);
                let entity=source_world.block_entities.get(&key).cloned();
                let name=source_world.block_names.get(&key).cloned();
                let dest=(to[0],to[1],to[2]);
                let previous=self.world.block(to[0],to[1],to[2]);
                let old_name=self.res.facts().get(previous).name.clone();
                let new_name=self.res.facts().get(state).name.clone();
                let old_incarnation=self.world.block_entities.incarnation(&dest);
                // Replacing a same-state CPU entity still retires its active
                // physical owner; preserve its reserved items before removal.
                if old_name==new_name && entity.is_some() && old_incarnation.is_some() {
                    self.ae2_prepare_cpu_removal(dest)?;
                }
                self.level_set(to,state).map_err(|e|e.to_string())?;
                // AE2 machines may use a generic host block with no vanilla
                // removal callback. Retire that exact old host, once only.
                if old_name!=new_name && old_incarnation.is_some()
                    && self.world.block_entities.incarnation(&dest)==old_incarnation {
                    self.with_level(|_,level,_| { level.remove_entity(BlockPos::new(to[0],to[1],to[2]),previous,true); });
                }
                if let Some(entity)=entity {
                    self.world.block_entities.insert(dest,entity);
                    match name { Some(name)=>{self.world.block_names.insert(dest,name);}, None=>{self.world.block_names.remove(&dest);} }
                    self.world.modified.insert((to[0]>>4,to[2]>>4));
                }
                // Source explicitly sends blockUpdated even when the state
                // is unchanged; the native broadcaster includes entity data.
                if let Some(log)=self.world.block_log.as_mut() { log.push(dest); }
                Ok(())
            })
        })();
        // Temporary chunk tickets belong to the level that acquired them.
        let cleanup=(|| {
            self.enter_level(None,dim).map_err(|e|e.to_string())?;
            self.streaming.extra=source_extra;
            self.enter_level(None,target_dim).map_err(|e|e.to_string())?;
            self.streaming.extra=target_extra;
            Ok::<(),String>(())
        })();
        result.and(cleanup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replicator_cycle_binding_and_ordered_yaw_offsets() {
        let mut item=ItemStack::new(ITEM,1);
        item.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("owner",Tag::String("kept".into()))])));
        for n in [1,2,3,4,1] { assert_eq!(cycle(&mut item).unwrap(),n); }
        assert_eq!(custom(&item).unwrap().get(&"owner".into()),Some(&Tag::String("kept".into())));
        assert_eq!(face_ordinal([0,0,-1]),Some(2)); assert_eq!(network_face(2),Some(5));
        let mut pairs=Vec::new();
        visit_pairs([10,20,30],[13,23,33],[10,20,30],[100,60,100],2,0.0,(-64,319),|a,b|{pairs.push((a,b));Ok(())}).unwrap();
        assert_eq!(pairs.len(),64);
        assert_eq!(pairs[0],([11,21,31],[101,61,101]));
        assert_eq!(pairs[1],([11,21,32],[101,61,102]));
        assert_eq!(pairs[8],([11,21,31],[101,61,104]));
        assert_eq!(pairs[32],pairs[0]); // yaw zero has no x displacement
        assert_eq!((-sign(wrap_degrees(90.0)),sign(wrap_degrees(180.0))),(-1,-1));
        assert_eq!((-sign(wrap_degrees(-90.0)),sign(wrap_degrees(0.0))),(1,0));
    }
}
