//! Physical entity exchange for an AE2 spatial volume. Based on AE2 19.2.18
//! SpatialStorageHelper (LGPL-3.0-or-later). The caller loads both regions and
//! swaps dimensions between take/restore; no clone of an entity is created.
use super::{
    FallingBlock, Game, boats, commands, dragon, fireworks, lightning, minecarts,
    ominous_item_spawner, tnt, trident,
};
use horizon_client::{
    entities::{ArmorStand, Cushion, ExperienceOrb, ItemEntity, ItemFrame, Painting},
    mobs::Mob,
    projectiles::Projectile,
};
use horizon_core::geometry::{Aabb, Vec3};
use horizon_world::nbt::Tag;
use std::collections::HashMap;

pub(super) struct SpatialLocalPlayerMove {
    destination: super::Dim,
    at: [f64; 3],
}
pub(super) struct SpatialEntities {
    min: (i32, i32, i32),
    size: [i32; 3],
    mobs: Vec<Mob>,
    dropped: Vec<ItemEntity>,
    orbs: Vec<ExperienceOrb>,
    falling: Vec<FallingBlock>,
    paintings: Vec<Painting>,
    frames: Vec<ItemFrame>,
    stands: Vec<ArmorStand>,
    cushions: Vec<Cushion>,
    others: Vec<commands::OtherEntity>,
    boats: Vec<boats::Boat>,
    carts: Vec<minecarts::Minecart>,
    bolts: Vec<lightning::Bolt>,
    crystals: Vec<dragon::Crystal>,
    fireballs: Vec<dragon::DragonFireball>,
    breaths: Vec<dragon::Breath>,
    tridents: Vec<trident::ThrownTrident>,
    rockets: Vec<fireworks::Rocket>,
    spawners: Vec<ominous_item_spawner::OminousItemSpawner>,
    tnts: Vec<tnt::PrimedTnt>,
    projectiles: Vec<Projectile>,
    dragon: Option<dragon::Dragon>,
    uuids: HashMap<u32, [i32; 4]>,
    raw: HashMap<u32, Tag>,
    anonymous: Vec<(String, Vec3, Option<[i32; 4]>, Tag)>,
    unknown: Vec<Tag>,
}
fn array(p: Vec3) -> [f64; 3] {
    [p.x, p.y, p.z]
}
fn contains(p: [f64; 3], min: (i32, i32, i32), size: [i32; 3]) -> bool {
    let m = [min.0, min.1, min.2];
    (0..3).all(|i| {
        size[i] > 0 && p[i] >= f64::from(m[i]) && p[i] < f64::from(m[i]) + f64::from(size[i])
    })
}
/// The source uses Level.getEntitiesOfClass, which intersects actual body
/// boxes. A feet/origin test loses players and large entities at the edges.
fn region_box(min: (i32,i32,i32), size: [i32;3]) -> Aabb {
    Aabb::new(vec3([min.0 as f64,min.1 as f64,min.2 as f64]),
        vec3([(min.0+size[0]) as f64,(min.1+size[1]) as f64,(min.2+size[2]) as f64]))
}
fn intersects(bounds: Aabb, min:(i32,i32,i32),size:[i32;3])->bool {
    size.iter().all(|v|*v>0) && bounds.intersects(region_box(min,size))
}
fn from_corners((lo,hi):([f64;3],[f64;3]))->Aabb { Aabb::new(vec3(lo),vec3(hi)) }
fn guest_box(guest:&horizon_client::lan::guest::Guest)->Aabb {
    guest.motion.as_ref().map(|m|m.dimensions.bounds(vec3(guest.position)))
        .unwrap_or_else(||super::lan_mobs::guest_bounds(guest))
}
fn drain_bounds<T>(items:&mut Vec<T>,min:(i32,i32,i32),size:[i32;3],bounds:impl Fn(&T)->Aabb)->Vec<T> {
    let mut selected=Vec::new();let mut left=Vec::with_capacity(items.len());
    for item in std::mem::take(items) {
        if intersects(bounds(&item),min,size){selected.push(item)}else{left.push(item)}
    }
    *items=left;selected
}
#[derive(Clone, Copy, serde::Deserialize)]
pub(super) struct SpatialEntityDimensions { width:f32, height:f32 }
type TypeDimensions = HashMap<String,SpatialEntityDimensions>;
type FormationGuest = (String, u32, Aabb);
thread_local! {
    static FORMATION_GUEST_SCOPES: std::cell::RefCell<Vec<Vec<FormationGuest>>> = const { std::cell::RefCell::new(Vec::new()) };
    static AE2_ENTITY_DIMENSIONS: std::cell::RefCell<Option<(std::path::PathBuf, TypeDimensions)>> = const { std::cell::RefCell::new(None) };
}
/// The hub is temporarily taken out of Game while it dispatches a packet.
/// Keep only a scoped read-only snapshot; nested dispatches restore the outer
/// snapshot on return, and no guest state is changed by a simulation.
pub(super) struct FormationGuestScope;
impl Drop for FormationGuestScope {
    fn drop(&mut self) { FORMATION_GUEST_SCOPES.with(|scopes| { scopes.borrow_mut().pop(); }); }
}
fn sized_box(p:[f64;3],width:f32,height:f32)->Aabb {
    let half=f64::from(width/2.0);
    Aabb::new(vec3([p[0]-half,p[1],p[2]-half]),vec3([p[0]+half,p[1]+f64::from(height),p[2]+half]))
}
fn type_box(types:&TypeDimensions, kind:&str,p:[f64;3],raw:Option<&Tag>)->Aabb {
    let mut d=types.get(kind).copied().unwrap_or(SpatialEntityDimensions{width:0.0,height:0.0});
    // Interaction entities set their actual dimensions from synchronized NBT.
    if kind=="minecraft:interaction" {
        let numeric=|key:&str|raw.and_then(|t|t.get(key)).and_then(|v|match v {Tag::Float(v)=>Some(f32::from_bits(*v)),Tag::Double(v)=>Some(f64::from_bits(*v) as f32),_=>None}).filter(|v|v.is_finite()&&*v>=0.0);
        d.width=numeric("width").unwrap_or(d.width);d.height=numeric("height").unwrap_or(d.height);
    }
    sized_box(p,d.width,d.height)
}
fn mapped(p: [f64; 3], from: (i32, i32, i32), to: (i32, i32, i32), size: [i32; 3]) -> [f64; 3] {
    let a = [from.0, from.1, from.2];
    let b = [to.0, to.1, to.2];
    std::array::from_fn(|i| {
        (p[i] + f64::from(b[i] - a[i]))
            .clamp(f64::from(b[i]) + 0.5, f64::from(b[i] + size[i]) - 0.5)
    })
}
fn vec3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}
fn block(p: [i32; 3], from: (i32, i32, i32), to: (i32, i32, i32)) -> [i32; 3] {
    [
        p[0] + to.0 - from.0,
        p[1] + to.1 - from.1,
        p[2] + to.2 - from.2,
    ]
}
fn map_entity_nbt(tag: &mut Tag, from: (i32, i32, i32), to: (i32, i32, i32), size: [i32; 3]) {
    let position =
        horizon_client::entity_nbt::position(tag).map(|p| mapped(array(p), from, to, size));
    if let Tag::Compound(fields) = tag {
        if let Some(p) = position {
            fields.insert(
                "Pos".into(),
                Tag::List(p.into_iter().map(|v| Tag::Double(v.to_bits())).collect()),
            );
        }
        if let Some(Tag::List(passengers)) = fields.get_mut(&"Passengers".into()) {
            for passenger in passengers {
                map_entity_nbt(passenger, from, to, size);
            }
        }
    }
}
impl Game<'_> {
    pub(super) fn ae2_formation_guest_scope(&self, host: &super::lan::LanHost) -> FormationGuestScope {
        let guests = host.hub.guests.iter().filter(|g| g.loaded && !g.dead)
            .map(|g| (g.dimension.clone(), g.conn, guest_box(g))).collect();
        FORMATION_GUEST_SCOPES.with(|scopes| scopes.borrow_mut().push(guests));
        FormationGuestScope
    }
    pub(super) fn ae2_spatial_type_dimensions(&self)->Result<TypeDimensions,String> {
        let file=self.options.resources.join("entity-types.json");
        let mut types:TypeDimensions=serde_json::from_slice(&std::fs::read(&file).map_err(|e|format!("entity dimensions: {e}"))?)
            .map_err(|e|format!("entity dimensions: {e}"))?;
        if types.is_empty() || types.values().any(|d| !d.width.is_finite() || !d.height.is_finite() || d.width<0.0 || d.height<0.0) {
            return Err("invalid native entity dimensions".into());
        }
        if let Some(tnt)=types.get("minecraft:tnt").copied() {types.insert("ae2:tiny_tnt_primed".into(),tnt);}
        Ok(types)
    }
    /// Native entity body from the active resource registry, without the
    /// larger crosshair pick radius. Callers add any source-specific padding.
    pub(super) fn ae2_entity_type_bounds(&self, kind: &str, position: Vec3) -> Option<Aabb> {
        AE2_ENTITY_DIMENSIONS.with(|cell| {
            let mut cached = cell.borrow_mut();
            let root = self.options.resources.clone();
            if cached.as_ref().is_none_or(|(path, _)| *path != root) {
                *cached = Some((root, self.ae2_spatial_type_dimensions().ok()?));
            }
            let types = &cached.as_ref()?.1;
            types.contains_key(kind).then(|| type_box(types, kind, array(position), None))
        })
    }
    /// A read-only Level.getEntitiesOfClass count. Formation Plane previews
    /// must observe the same bodies as a commit without materializing or
    /// transferring pending chunk entities.
    pub(super) fn ae2_count_entities_in_box(&self, bounds: Aabb) -> Result<usize, String> {
        AE2_ENTITY_DIMENSIONS.with(|cell| {
            let mut cached = cell.borrow_mut();
            let root = self.options.resources.clone();
            if cached.as_ref().is_none_or(|(path, _)| *path != root) {
                *cached = Some((root, self.ae2_spatial_type_dimensions()?));
            }
            let types = &cached.as_ref().unwrap().1;
            let mut count = usize::from(self.player.as_ref().is_some_and(|p| bounds.intersects(p.bounds())));
            if let Some(host) = self.lan.as_ref() {
                count += host.hub.guests.iter()
                    .filter(|g| g.dimension == self.dim.id() && g.loaded && !g.dead
                        && self.lan_acting != Some(g.conn) && bounds.intersects(guest_box(g)))
                    .count();
            } else {
                count += FORMATION_GUEST_SCOPES.with(|scopes| scopes.borrow().last().map_or(0, |guests|
                    guests.iter().filter(|(dim,conn,body)| dim == self.dim.id()
                        && self.lan_acting != Some(*conn) && bounds.intersects(*body)).count()));
            }
            macro_rules! count_bodies {
                ($list:expr, $f:expr) => {
                    count += $list.iter().filter(|e| bounds.intersects(($f)(e))).count();
                };
            }
            count_bodies!(self.mobs, |e: &Mob| e.bounds());
            count_bodies!(self.dropped, |e: &ItemEntity| e.bounds());
            count_bodies!(self.orbs, |e: &ExperienceOrb| e.bounds());
            count_bodies!(self.falling, |e: &FallingBlock| type_box(types, "minecraft:falling_block", array(e.position), None));
            count_bodies!(self.paintings, |e: &Painting| from_corners(e.bounds()));
            count_bodies!(self.item_frames, |e: &ItemFrame| from_corners(e.bounds()));
            count_bodies!(self.armor_stands, |e: &ArmorStand| e.bounds());
            count_bodies!(self.cushions, |e: &Cushion| from_corners(e.bounds()));
            count_bodies!(self.others, |e: &commands::OtherEntity| type_box(types, &e.kind, array(e.position), self.persist.raw.get(&e.id)));
            count_bodies!(self.boats, |e: &boats::Boat| e.bounds());
            count_bodies!(self.carts, |e: &minecarts::Minecart| e.bounds());
            count_bodies!(self.bolts, |e: &lightning::Bolt| type_box(types, "minecraft:lightning_bolt", e.at, None));
            count_bodies!(self.crystals, |e: &dragon::Crystal| type_box(types, "minecraft:end_crystal", array(e.position), None));
            count_bodies!(self.dragon_fireballs, |e: &dragon::DragonFireball| type_box(types, "minecraft:dragon_fireball", array(e.position), None));
            count_bodies!(self.breath_clouds, |e: &dragon::Breath| sized_box(e.at, e.radius * 2.0, 0.5));
            count_bodies!(self.tridents, |e: &trident::ThrownTrident| type_box(types, "minecraft:trident", array(e.position), None));
            count_bodies!(self.rockets, |e: &fireworks::Rocket| type_box(types, "minecraft:firework_rocket", array(e.position), None));
            count_bodies!(self.ominous_spawners, |e: &ominous_item_spawner::OminousItemSpawner| type_box(types, "minecraft:ominous_item_spawner", e.position, None));
            count_bodies!(self.tnts, |e: &tnt::PrimedTnt| type_box(types, if e.tiny {"ae2:tiny_tnt_primed"} else {"minecraft:tnt"}, array(e.position), None));
            count_bodies!(self.creatures.projectiles, |e: &Projectile| type_box(types, e.kind.id(), array(e.position), None));
            count += usize::from(self.dragon.as_ref().is_some_and(|e| bounds.intersects(type_box(types, "minecraft:ender_dragon", array(e.position), None))));
            let nbt_intersects = |tag: &Tag| horizon_client::entity_nbt::position(tag).is_some_and(|p| {
                let kind = horizon_client::entity_nbt::kind(tag).unwrap_or_default();
                bounds.intersects(type_box(types, &kind, array(p), Some(tag)))
            });
            count += self.persist.unknown.iter()
                .filter(|((x, z), _)| self.world.loaded(x * 16, z * 16))
                .flat_map(|(_, tags)| tags.iter()).filter(|tag| nbt_intersects(tag)).count();
            count += self.world.loaded_entities.iter()
                .filter(|((x, z), _)| self.world.loaded(x * 16, z * 16))
                .flat_map(|(_, tags)| tags.iter()).filter(|tag| nbt_intersects(tag)).count();
            Ok(count)
        })
    }
    pub(super) fn ae2_take_spatial_entities(
        &mut self,
        min: (i32, i32, i32),
        size: [i32; 3],
        types:&TypeDimensions,
    ) -> SpatialEntities {
        // Include entity records already read with the region's loaded chunks.
        self.spawn_loaded_entities();
        let guest_vehicles: std::collections::HashSet<u32> = self
            .lan
            .as_ref()
            .into_iter()
            .flat_map(|host| host.hub.guests.iter())
            .filter(|guest| guest.dimension == self.dim.id() && intersects(guest_box(guest), min, size))
            .filter_map(|guest| guest.vehicle)
            .collect();
        let mut mobs = drain_bounds(&mut self.mobs, min, size, |e| e.bounds());
        let dropped = drain_bounds(&mut self.dropped, min, size, |e| e.bounds());
        let orbs = drain_bounds(&mut self.orbs, min, size, |e| e.bounds());
        let falling = drain_bounds(&mut self.falling, min, size, |e| type_box(types,"minecraft:falling_block",array(e.position),None));
        let paintings = drain_bounds(&mut self.paintings, min, size, |e| from_corners(e.bounds()));
        let frames = drain_bounds(&mut self.item_frames, min, size, |e| from_corners(e.bounds()));
        let stands = drain_bounds(&mut self.armor_stands, min, size, |e| e.bounds());
        let cushions = drain_bounds(&mut self.cushions, min, size, |e| from_corners(e.bounds()));
        let others = drain_bounds(&mut self.others, min, size, |e| type_box(types,&e.kind,array(e.position),self.persist.raw.get(&e.id)));
        let mut boats = drain_bounds(&mut self.boats, min, size, |e| e.bounds());
        let mut carts = drain_bounds(&mut self.carts, min, size, |e| e.bounds());
        let bolts = drain_bounds(&mut self.bolts, min, size, |e| type_box(types,"minecraft:lightning_bolt",e.at,None));
        let crystals = drain_bounds(&mut self.crystals, min, size, |e| type_box(types,"minecraft:end_crystal",array(e.position),None));
        let fireballs = drain_bounds(&mut self.dragon_fireballs, min, size, |e| type_box(types,"minecraft:dragon_fireball",array(e.position),None));
        let breaths = drain_bounds(&mut self.breath_clouds, min, size, |e| sized_box(e.at,e.radius*2.0,0.5));
        let tridents = drain_bounds(&mut self.tridents, min, size, |e| type_box(types,"minecraft:trident",array(e.position),None));
        let rockets = drain_bounds(&mut self.rockets, min, size, |e| type_box(types,"minecraft:firework_rocket",array(e.position),None));
        let spawners = drain_bounds(&mut self.ominous_spawners, min, size, |e| type_box(types,"minecraft:ominous_item_spawner",e.position,None));
        let tnts = drain_bounds(&mut self.tnts, min, size, |e| type_box(types,if e.tiny{"ae2:tiny_tnt_primed"}else{"minecraft:tnt"},array(e.position),None));
        let projectiles = drain_bounds(&mut self.creatures.projectiles, min, size, |e| type_box(types,e.kind.id(),array(e.position),None));
        // AE2 transfers passengers with their vehicle even when a rider's
        // coordinates lie just outside the source AABB.
        loop {
            let mut connected: std::collections::HashSet<u32> = mobs
                .iter()
                .map(|m| m.id)
                .chain(boats.iter().map(|e| e.id))
                .chain(carts.iter().map(|e| e.id))
                .collect();
            connected.extend(mobs.iter().filter_map(|m| m.extra.mount));
            connected.extend(guest_vehicles.iter().copied());
            let previous_vehicles = boats.len() + carts.len();
            let mut left_boats = Vec::new();
            for boat in std::mem::take(&mut self.boats) {
                if connected.contains(&boat.id) {
                    boats.push(boat);
                } else {
                    left_boats.push(boat);
                }
            }
            self.boats = left_boats;
            let mut left_carts = Vec::new();
            for cart in std::mem::take(&mut self.carts) {
                if connected.contains(&cart.id) {
                    carts.push(cart);
                } else {
                    left_carts.push(cart);
                }
            }
            self.carts = left_carts;
            let mut remaining = Vec::new();
            let mut linked = Vec::new();
            for mob in std::mem::take(&mut self.mobs) {
                if connected.contains(&mob.id)
                    || mob.extra.mount.is_some_and(|id| connected.contains(&id))
                {
                    linked.push(mob);
                } else {
                    remaining.push(mob);
                }
            }
            self.mobs = remaining;
            if linked.is_empty() && previous_vehicles == boats.len() + carts.len() {
                break;
            }
            mobs.extend(linked);
        }
        let dragon = if self
            .dragon
            .as_ref()
            .is_some_and(|e| intersects(type_box(types,"minecraft:ender_dragon",array(e.position),None), min, size))
        {
            self.dragon.take()
        } else {
            None
        };
        let mut ids = Vec::new();
        macro_rules! add_ids {($($v:ident),*)=>{$(ids.extend($v.iter().map(|e|e.id));)*};}
        add_ids!(
            mobs,
            dropped,
            orbs,
            falling,
            paintings,
            frames,
            stands,
            cushions,
            others,
            boats,
            carts,
            bolts,
            crystals,
            fireballs,
            breaths,
            tridents,
            rockets,
            tnts,
            projectiles
        );
        if let Some(d) = &dragon {
            ids.push(d.id)
        }
        let mut uuids = HashMap::new();
        let mut raw = HashMap::new();
        for id in ids {
            if let Some(v) = self.persist.uuids.remove(&id) {
                uuids.insert(id, v);
            }
            if let Some(v) = self.persist.raw.remove(&id) {
                raw.insert(id, v);
            }
        }
        let mut selected_anon=std::collections::HashSet::new();
        let mut remember_anon=|kind:&str,p:[f64;3]| {selected_anon.insert((kind.to_owned(),p.map(f64::to_bits)));};
        for e in &orbs {remember_anon("minecraft:experience_orb",array(e.position));}
        for e in &falling {remember_anon("minecraft:falling_block",array(e.position));}
        for e in &crystals {remember_anon("minecraft:end_crystal",array(e.position));}
        for e in &projectiles {remember_anon(e.kind.id(),array(e.position));}
        if let Some(e)=&dragon {remember_anon("minecraft:ender_dragon",array(e.position));}
        let mut anonymous = Vec::new();
        self.persist.anon.retain(|key, (pos, uuid, tag)| {
            if selected_anon.contains(key) {
                anonymous.push((key.0.clone(), *pos, *uuid, tag.clone()));
                false
            } else { true }
        });
        let mut unknown = Vec::new();
        self.persist.unknown.retain(|_, tags| {
            tags.retain(|tag| {
                if horizon_client::entity_nbt::position(tag)
                     .is_some_and(|p| intersects(type_box(types,&horizon_client::entity_nbt::kind(tag).unwrap_or_default(),array(p),Some(tag)), min, size))
                {
                    unknown.push(tag.clone());
                    false
                } else {
                    true
                }
            });
            !tags.is_empty()
        });
        SpatialEntities {
            min,
            size,
            mobs,
            dropped,
            orbs,
            falling,
            paintings,
            frames,
            stands,
            cushions,
            others,
            boats,
            carts,
            bolts,
            crystals,
            fireballs,
            breaths,
            tridents,
            rockets,
            spawners,
            tnts,
            projectiles,
            dragon,
            uuids,
            raw,
            anonymous,
            unknown,
        }
    }
    /// Snapshot each player in this volume and passengers of transferred
    /// vehicles. The caller queues both sides after exchanging the blocks.
    pub(super) fn ae2_spatial_guest_transfers(
        &self,
        host: &super::lan::LanHost,
        batch: &SpatialEntities,
        destination: super::Dim,
        to: (i32, i32, i32),
    ) -> Vec<super::server_travel::LevelTeleport> {
        let vehicles: std::collections::HashSet<_> = batch
            .boats
            .iter()
            .map(|e| e.id)
            .chain(batch.carts.iter().map(|e| e.id))
            .collect();
        host.hub
            .guests
            .iter()
            .filter(|guest| {
                guest.dimension == self.dim.id()
                    && (intersects(guest_box(guest), batch.min, batch.size)
                        || guest.vehicle.is_some_and(|id| vehicles.contains(&id)))
            })
            .map(|guest| super::server_travel::LevelTeleport {
                conn: guest.conn,
                dim: destination,
                at: mapped(guest.position, batch.min, to, batch.size),
                rotation: None,
            })
            .collect()
    }
    pub(super) fn ae2_queue_spatial_guests(
        &mut self,
        moves: Vec<super::server_travel::LevelTeleport>,
    ) {
        for movement in &moves {if movement.dim==super::Dim::Spatial {self.ae2_queue_connection_advancement(movement.conn,"ae2:spatial_explorer");}}
        self.level_teleports.extend(moves);
    }
    /// Capture a standalone in-frame player; defer travel until both halves
    /// of the exchange are committed.
    pub(super) fn ae2_spatial_local_player_transfer(
        &self,
        batch: &SpatialEntities,
        destination: super::Dim,
        to: (i32, i32, i32),
    ) -> Option<SpatialLocalPlayerMove> {
        if self.options.dedicated.is_some() || self.remote.is_some() {
            return None;
        }
        let player = self.player.as_ref()?;
        let p = array(player.position);
        intersects(player.bounds(), batch.min, batch.size).then(|| SpatialLocalPlayerMove {
            destination,
            at: mapped(p, batch.min, to, batch.size),
        })
    }
    pub(super) fn ae2_apply_spatial_local_player_transfer(
        &mut self,
        movement: SpatialLocalPlayerMove,
    ) -> horizon_client::Result<()> {
        if movement.destination==super::Dim::Spatial && self.player.is_some() {self.trigger_player("ae2:spatial_explorer");}
        let Some(mut player) = self.player.take() else {
            return Ok(());
        };
        let from = self.dim;
        if let Err(error) = self.switch_level(movement.destination, player.position) {
            self.player = Some(player);
            return Err(error);
        }
        self.trigger_dimension(from, movement.destination, movement.at);
        let target = vec3(movement.at);
        player.position = target;
        player.velocity = Vec3::ZERO;
        player.fall_distance = 0.0;
        player.ground.on_ground = false;
        self.travelling = Some((player, target));
        self.portals.travel = None;
        self.end_travel = false;
        self.end_portal_travel = false;
        Ok(())
    }
    pub(super) fn ae2_restore_spatial_entities(
        &mut self,
        mut b: SpatialEntities,
        to: (i32, i32, i32),
    ) {
        let from = b.min;
        let size = b.size;
        macro_rules! xyz {
            ($v:expr,$dest:expr) => {
                for mut e in $v.drain(..) {
                    e.position = vec3(mapped(array(e.position), from, to, size));
                    $dest.push(e);
                }
            };
        }
        macro_rules! xyz_velocity {
            ($v:expr,$dest:expr) => {
                for mut e in $v.drain(..) {
                    e.position = vec3(mapped(array(e.position), from, to, size));
                    e.velocity = Vec3::ZERO;
                    $dest.push(e);
                }
            };
        }
        macro_rules! xyz_previous {
            ($v:expr,$dest:expr) => {
                for mut e in $v.drain(..) {
                    e.position = vec3(mapped(array(e.position), from, to, size));
                    e.previous = e.position;
                    e.velocity = Vec3::ZERO;
                    $dest.push(e);
                }
            };
        }
        xyz_previous!(b.mobs, self.mobs);
        xyz_previous!(b.dropped, self.dropped);
        xyz_previous!(b.orbs, self.orbs);
        xyz_previous!(b.falling, self.falling);
        xyz_velocity!(b.stands, self.armor_stands);
        xyz!(b.others, self.others);
        xyz_previous!(b.boats, self.boats);
        xyz_previous!(b.carts, self.carts);
        xyz!(b.crystals, self.crystals);
        xyz_previous!(b.fireballs, self.dragon_fireballs);
        xyz_velocity!(b.tridents, self.tridents);
        xyz_velocity!(b.rockets, self.rockets);
        xyz_previous!(b.tnts, self.tnts);
        xyz_previous!(b.projectiles, self.creatures.projectiles);
        for mut e in b.paintings {
            e.pos = block(e.pos, from, to);
            self.paintings.push(e)
        }
        for mut e in b.frames {
            e.pos = block(e.pos, from, to);
            self.item_frames.push(e)
        }
        for mut e in b.cushions {
            e.pos = block(e.pos, from, to);
            e.position = vec3(mapped(array(e.position), from, to, size));
            self.cushions.push(e)
        }
        for mut e in b.bolts {
            e.at = mapped(e.at, from, to, size);
            self.bolts.push(e)
        }
        for mut e in b.breaths {
            e.at = mapped(e.at, from, to, size);
            self.breath_clouds.push(e)
        }
        for mut e in b.spawners {
            e.position = mapped(e.position, from, to, size);
            self.ominous_spawners.push(e)
        }
        if let Some(mut d) = b.dragon {
            d.position = vec3(mapped(array(d.position), from, to, size));
            d.previous = d.position;
            self.dragon = Some(d)
        }
        for (kind, position, uuid, mut tag) in b.anonymous {
            map_entity_nbt(&mut tag, from, to, size);
            let position = vec3(mapped(array(position), from, to, size));
            let key = (
                kind,
                [
                    position.x.to_bits(),
                    position.y.to_bits(),
                    position.z.to_bits(),
                ],
            );
            self.persist.anon.insert(key, (position, uuid, tag));
        }
        for mut tag in b.unknown {
            let Some(position) = horizon_client::entity_nbt::position(&tag) else {
                continue;
            };
            let p = mapped(array(position), from, to, size);
            map_entity_nbt(&mut tag, from, to, size);
            let chunk = ((p[0].floor() as i32) >> 4, (p[2].floor() as i32) >> 4);
            self.persist.unknown.entry(chunk).or_default().push(tag);
        }
        self.persist.uuids.extend(b.uuids);
        for (id, mut tag) in b.raw {
            map_entity_nbt(&mut tag, from, to, size);
            self.persist.raw.insert(id, tag);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spatial_raw_entity_and_passenger_nbt_are_translated() {
        fn entity(x: f64, y: f64, z: f64) -> Tag {
            Tag::compound([(
                "Pos",
                Tag::List(
                    [x, y, z]
                        .into_iter()
                        .map(|v| Tag::Double(v.to_bits()))
                        .collect(),
                ),
            )])
        }
        let mut vehicle = entity(10.75, 4.0, 9.75);
        if let Tag::Compound(fields) = &mut vehicle {
            fields.insert(
                "Passengers".into(),
                Tag::List(vec![entity(11.0, 4.0, 10.0)]),
            );
        }
        map_entity_nbt(&mut vehicle, (10, 3, 9), (-5, 20, 30), [2, 2, 2]);
        assert_eq!(
            array(horizon_client::entity_nbt::position(&vehicle).unwrap()),
            [-4.25, 21.0, 30.75]
        );
        let Some(Tag::List(passengers)) = vehicle.get("Passengers") else {
            panic!("passenger disappeared")
        };
        assert_eq!(
            array(horizon_client::entity_nbt::position(&passengers[0]).unwrap()),
            [-4.0, 21.0, 31.0]
        );
    }
    #[test]
    fn spatial_query_intersects_entity_bodies_and_excludes_only_touching_edges() {
        let min=(10,64,10);let size=[2,2,2];
        let body=|x:f64|Aabb::new(vec3([x-0.3,64.0,10.5]),vec3([x+0.3,65.8,11.1]));
        assert!(!contains([9.85,64.0,10.8],min,size));
        assert!(intersects(body(9.85),min,size));
        assert!(!intersects(body(9.7),min,size));
        assert!(intersects(body(12.15),min,size));
        let mut bodies=vec![body(9.85),body(9.7),body(12.15)];
        assert_eq!(drain_bounds(&mut bodies,min,size,|v|*v).len(),2);
        assert_eq!(bodies.len(),1);
    }

    #[test]
    fn spatial_position_mapping_clamps_like_source() {
        assert!(contains([10.1, 3.0, 9.9], (10, 3, 9), [2, 2, 2]));
        assert!(!contains([12.0, 3.0, 9.9], (10, 3, 9), [2, 2, 2]));
        assert_eq!(
            mapped([10.1, 4.9, 9.2], (10, 3, 9), (-5, 20, 30), [2, 2, 2]),
            [-4.5, 21.5, 30.5]
        );
    }
}
