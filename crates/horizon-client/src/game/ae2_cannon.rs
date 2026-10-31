//! MatterCannonItem ray, ammo and penetration, AE2 19.2.18, LGPL-3.0+.
use super::{Game, GuestUse, ae2_lab, view_vector};
use ae2_cell_core::Action;
use horizon_client::{
    ae2_powered_items as power,
    collisions::SceneCollisions,
    first_person::UseResult,
    items::ItemStack,
    mobs::{Damage, DamageKind, Species, Who},
    projectiles::ProjectileKind,
};
use horizon_core::{
    geometry::{Aabb, Vec3, traverse_blocks},
    position::BlockPos,
};
enum Impact {
    Entity {
        target: CannonEntity,
        point: Vec3,
    },
    Guest {
        conn: u32,
        point: Vec3,
    },
    Block {
        pos: [i32; 3],
        face: [i32; 3],
        point: Vec3,
    },
    Mob {
        index: usize,
        point: Vec3,
    },
}
#[derive(Clone, Copy)]
enum CannonEntity {
    ArmorStand(usize),
    Orb(usize),
    Boat(usize),
    Cart(usize),
    Frame(usize),
    Painting(usize),
    Special(u32),
    Projectile(usize),
    StoredShulkerBullet(usize),
    UnhurtProjectile,
    Tnt,
    FallingBlock,
}
fn distance(a: Vec3, b: Vec3) -> f64 {
    (a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)
}
fn damage(penetration: f32) -> f32 {
    (penetration / 20.0).ceil()
}
fn remaining(penetration: f32, hardness: f32) -> Option<f32> {
    let cost = hardness * 9.0;
    (cost >= 0.0 && penetration > cost).then_some((penetration - cost) * 0.60)
}
/// In the pinned game, ShulkerBullet.hurt discards the bullet even for zero
/// damage. Other native projectile classes handled here refuse the hit.
fn cannon_destroys_projectile(kind: &ProjectileKind) -> bool {
    matches!(kind, ProjectileKind::ShulkerBullet { .. })
}
impl Game<'_> {
    fn ae2_cannon_hit(
        &self,
        from: Vec3,
        to: Vec3,
        host: Option<&super::lan::LanHost>,
    ) -> Option<Impact> {
        let mut scene = SceneCollisions {
            world: &self.world,
            binding: self.collision,
            subject: None,
        };
        let block = traverse_blocks(from, to, 1024, |p: BlockPos| {
            let state = self.world.block(p.x, p.y, p.z);
            if self.res.facts().is_air(state) {
                return None;
            }
            self.collision
                .collision_shape(&mut scene, state, [p.x, p.y, p.z])
                .ok()?
                .clip(from, to, p)
        })
        .ok()
        .flatten();
        let mut closest = block
            .as_ref()
            .map_or(f64::INFINITY, |b| distance(from, b.location));
        let mut result = block.map(|b| Impact::Block {
            pos: [b.block.x, b.block.y, b.block.z],
            face: b.direction.normal(),
            point: b.location,
        });
        for (index, mob) in self
            .mobs
            .iter()
            .enumerate()
            .filter(|(_, m)| m.alive() && Some(m.id) != self.riding)
        {
            if let Some(point) = mob
                .bounds()
                .inflate(Vec3::new(0.3, 0.3, 0.3))
                .clip(from, to)
            {
                let d = distance(from, point);
                if d < closest {
                    closest = d;
                    result = Some(Impact::Mob { index, point });
                }
            }
        }
        let array_box = |(lo, hi): ([f64; 3], [f64; 3])| {
            Aabb::new(
                Vec3::new(lo[0], lo[1], lo[2]),
                Vec3::new(hi[0], hi[1], hi[2]),
            )
        };
        let entities = self
            .armor_stands
            .iter()
            .enumerate()
            .filter(|(_, a)| a.health > 0.0)
            .map(|(i, a)| (CannonEntity::ArmorStand(i), a.bounds()))
            .chain(
                self.boats
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| Some(b.id) != self.riding)
                    .map(|(i, b)| (CannonEntity::Boat(i), b.bounds())),
            )
            .chain(
                self.carts
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| Some(c.id) != self.riding)
                    .map(|(i, c)| (CannonEntity::Cart(i), c.bounds())),
            )
            .chain(
                self.item_frames
                    .iter()
                    .enumerate()
                    .map(|(i, f)| (CannonEntity::Frame(i), array_box(f.bounds()))),
            )
            .chain(
                self.paintings
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (CannonEntity::Painting(i), array_box(p.bounds()))),
            )
            .chain(
                self.special_targets()
                    .into_iter()
                    .map(|(id, b)| (CannonEntity::Special(id), b)),
            )
            .chain(self.orbs.iter().enumerate().map(|(i,o)|(CannonEntity::Orb(i),o.bounds())))
            .chain(self.tnts.iter().map(|t| (CannonEntity::Tnt, t.bounds())))
            .chain(self.falling.iter().map(|f| {
                (
                    CannonEntity::FallingBlock,
                    horizon_client::falling_block::bounds(f.position),
                )
            }))
            .chain(self.creatures.projectiles.iter().enumerate()
                .filter(|(_,p)| p.arrow.as_ref().is_none_or(|a| !a.removed))
                .filter_map(|(i,p)| self.ae2_entity_type_bounds(p.kind.id(),p.position)
                    .map(|bounds| (CannonEntity::Projectile(i),bounds))))
            .chain(self.tridents.iter().filter_map(|p|
                self.ae2_entity_type_bounds("minecraft:trident",p.position)
                    .map(|bounds| (CannonEntity::UnhurtProjectile,bounds))))
            .chain(self.rockets.iter().filter_map(|p|
                self.ae2_entity_type_bounds("minecraft:firework_rocket",p.position)
                    .map(|bounds| (CannonEntity::UnhurtProjectile,bounds))))
            .chain(self.potions.thrown_entities().into_iter().filter_map(|(kind,position,_)|
                self.ae2_entity_type_bounds(kind,position)
                    .map(|bounds| (CannonEntity::UnhurtProjectile,bounds))))
            .chain(self.dragon_fireballs.iter().filter_map(|p|
                self.ae2_entity_type_bounds("minecraft:dragon_fireball",p.position)
                    .map(|bounds| (CannonEntity::UnhurtProjectile,bounds))))
            // The native load/summon path currently represents this same
            // source entity as OtherEntity rather than a homing projectile.
            .chain(self.others.iter().enumerate()
                .filter(|(_,e)| e.kind == "minecraft:shulker_bullet")
                .filter_map(|(i,e)| self.ae2_entity_type_bounds(&e.kind,e.position)
                    .map(|bounds| (CannonEntity::StoredShulkerBullet(i),bounds))));
        for (target, bounds) in entities {
            if let Some(point) = bounds.inflate(Vec3::new(0.3, 0.3, 0.3)).clip(from, to) {
                let d = distance(from, point);
                if d < closest {
                    closest = d;
                    result = Some(Impact::Entity { target, point });
                }
            }
        }
        if let Some(host) = host.or(self.lan.as_ref()) {
            for guest in host.hub.guests.iter().filter(|g| {
                Some(g.conn) != self.lan_acting
                    && g.loaded
                    && !g.dead
                    && !g.spectator()
                    && g.dimension == self.dim.id()
            }) {
                if let Some(point) = super::lan_mobs::guest_bounds(guest)
                    .inflate(Vec3::new(0.3, 0.3, 0.3))
                    .clip(from, to)
                {
                    let d = distance(from, point);
                    if d < closest {
                        closest = d;
                        result = Some(Impact::Guest {
                            conn: guest.conn,
                            point,
                        });
                    }
                }
            }
        }
        result
    }
    /// Non-living entities retain their native hurt behavior. Armor stands,
    /// TNT and falling blocks refuse this custom damage and still stop the ray.
    fn ae2_cannon_entity(&mut self, target: CannonEntity, amount: f32, paint: bool, creative: bool) -> bool {
        match target {
            CannonEntity::Orb(i) => {
                if self.orbs[i].hurt(amount) { self.orbs.remove(i); true } else { false }
            }
            CannonEntity::ArmorStand(i) => {
                if paint {
                    self.hurt_armor_stand_at(i);
                }
                false
            }
            CannonEntity::Tnt | CannonEntity::FallingBlock => false,
            CannonEntity::UnhurtProjectile => false,
            CannonEntity::StoredShulkerBullet(i) => {
                let bullet = self.others.remove(i);
                self.persist.raw.remove(&bullet.id);
                self.persist.uuids.remove(&bullet.id);
                self.ae2_cannon_destroyed_bullet(bullet.position);
                true
            }
            CannonEntity::Projectile(i) => {
                if !cannon_destroys_projectile(&self.creatures.projectiles[i].kind) { return false; }
                let bullet = self.creatures.projectiles.remove(i);
                self.ae2_cannon_destroyed_bullet(bullet.position);
                true
            }
            CannonEntity::Boat(i) => self.hurt_boat(i, amount, creative),
            CannonEntity::Cart(i) => {
                let id = self.carts[i].id;
                self.hurt_cart(i, amount, creative, false);
                self.carts.iter().all(|c| c.id != id)
            }
            CannonEntity::Frame(i) => {
                let id = self.item_frames[i].id;
                self.hurt_frame_at(i) && self.item_frames.iter().all(|f| f.id != id)
            }
            CannonEntity::Painting(i) => {
                self.break_painting(i, true);
                true
            }
            CannonEntity::Special(id) => {
                let before = self.special_targets().iter().any(|(i, _)| *i == id);
                let hurt = self.hit_special(id, amount, true, horizon_client::mobs::DamageKind::PlayerAttack, None);
                before
                    && matches!(hurt, super::dragon::Hurt::Landed { .. })
                    && !self.special_targets().iter().any(|(i, _)| *i == id)
            }
        }
    }
    fn ae2_cannon_destroyed_bullet(&mut self, position: Vec3) {
        let at = [position.x,position.y,position.z];
        self.audio.play("minecraft:entity.shulker_bullet.hurt",Some(at),1.0,1.0);
        let options = horizon_client::particles::ParticleOptions::simple(horizon_client::particles::ParticleType::Crit);
        self.particles.send_particles(&options,at,[0.2;3],0.0,15);
        self.projectile_event("minecraft:entity_damage",None,at);
    }
    fn ae2_cannon_guest(
        &mut self,
        host: &mut super::lan::LanHost,
        conn: u32,
        amount: f32,
        origin: [f64; 3],
        dir: [f64; 3],
    ) -> (bool, bool) {
        let shooter = self
            .lan_acting
            .and_then(|c| host.hub.guest(c))
            .map_or(host.host_entity as u32, |g| g.entity_id as u32);
        let attacker = self.player_team().cloned();
        let target = host
            .hub
            .guest(conn)
            .and_then(|g| self.score.board.players_team(&g.name))
            .cloned();
        if !self.ui.rules.bool("pvp")
            || !horizon_client::scoreboard::teams_allow_damage(target.as_ref(), attacker.as_ref())
        {
            return (false, false);
        }
        let result = self.guest_hit_by(host, conn, |game| {
            let hurt = game.player_hits_player_kind(amount, origin, "ae2:matter_cannon", shooter);
            if hurt {
                game.knock_back_player(0.0, -dir[0], -dir[2]);
            }
            (
                hurt,
                game.player
                    .as_ref()
                    .is_none_or(|p| p.health.health() <= 0.0),
                if hurt {
                    game.thorns_damage()
                } else {
                    Vec::new()
                },
            )
        });
        if let Some((hurt, dead, thorns)) = result {
            for amount in thorns {
                self.hurt_player_by("minecraft:thorns", amount);
            }
            (hurt, dead)
        } else {
            (false, false)
        }
    }
    pub(super) fn ae2_use_cannon(
        &mut self,
        stack: &ItemStack,
        host: Option<&mut super::lan::LanHost>,
    ) -> Result<UseResult, String> {
        if matches!(self.guest_use, Some(GuestUse::OnBlock(_))) { return Ok(UseResult::Pass); }
        let Some(origin) = self.eye_position() else { return Ok(UseResult::Fail); };
        let dir = view_vector(self.yaw, self.pitch);
        let mut updated = stack.clone();
        let result = self.ae2_cannon_at(&mut updated, origin, dir, 255.0, self.may_build(), false, host);
        *self.held_mut() = Some(updated);
        result
    }
    /// Source dispenser firing uses a 32-block ray and mutates its own slot.
    pub(super) fn ae2_cannon_at(&mut self, stack: &mut ItemStack, origin: [f64;3], dir: [f64;3], range: f64, may_build: bool, dispenser: bool, mut host: Option<&mut super::lan::LanHost>) -> Result<UseResult, String> {
        let mut cell = ae2_lab::read_cell(stack)?;
        let Some((key, count)) = cell.entries().next().map(|(key, n)| (key.clone(), *n)) else {
            if !dispenser { self.system_message(horizon_client::text::Text::literal("Ammo depleted")); }
            return Ok(UseResult::Success { swing: true });
        };
        let (available, _) = power::energy(stack)?;
        if available < 1600.0 {
            return Ok(UseResult::Fail);
        }
        let (_, speed) = power::tool_upgrades(stack)?;
        let shots = u64::from(1 + speed)
            .min(count)
            .min((available / 1600.0) as u64);
        if shots == 0 {
            return Ok(UseResult::Fail);
        }
        let ammo = ae2_lab::prototype(&key)?;
        let mut updated = stack.clone();
        if cell.extract(&key, 1, Action::Modulate) != 1 {
            return Ok(UseResult::Success { swing: true });
        }
        power::transfer(&mut updated, 1600.0 * shots as f64, false, false)?;
        ae2_lab::write_cell(&mut updated, &cell)?;
        *stack = updated;
        let from = Vec3::new(origin[0], origin[1], origin[2]);
        let to = Vec3::new(origin[0]+dir[0]*range, origin[1]+dir[1]*range, origin[2]+dir[2]*range);
        let mut penetration =
            power::ammo_weight(self.res.recipes(), self.res.items(), &ammo) * shots as f32;
        let paint = power::paint_ball(&ammo);
        loop {
            let hit = self.ae2_cannon_hit(from, to, host.as_deref());
            let point = match &hit {
                Some(
                    Impact::Block { point, .. }
                    | Impact::Mob { point, .. }
                    | Impact::Guest { point, .. }
                    | Impact::Entity { point, .. },
                ) => Some(*point),
                None => None,
            };
            self.ae2_matter_fx(
                origin,
                dir,
                point.map_or(32, |p| ((distance(from, p) + 1.0) as i32) as u8),
            );
            if penetration <= 0.0 {
                if let Some((color, lit)) = paint {
                    match hit {
                        Some(Impact::Entity { target, .. }) => {
                            self.ae2_cannon_entity(target, 0.0, true, self.creative && !dispenser);
                        }
                        Some(Impact::Mob { index, .. }) => {
                            if self.mobs[index].species() == Species::Sheep {
                                if let Some(dye) = horizon_client::block_entity_render::DYES
                                    .iter()
                                    .position(|(name, _)| *name == power::COLORS[color as usize])
                                {
                                    self.mobs[index].extra.color = dye as u8;
                                }
                            }
                            self.mobs[index].damage(
                                0.0,
                                Damage {
                                    kind: DamageKind::PlayerAttack,
                                    attacker: Some(Who::Player),
                                    from: Some(origin),
                                },
                            );
                        }
                        Some(Impact::Block { pos, face, point }) if self.may_build() => {
                            let paint_pos = [pos[0] + face[0], pos[1] + face[1], pos[2] + face[2]];
                            self.ae2_add_paint(
                                paint_pos,
                                face.map(|n| -n),
                                [
                                    point.x - f64::from(paint_pos[0]),
                                    point.y - f64::from(paint_pos[1]),
                                    point.z - f64::from(paint_pos[2]),
                                ],
                                color,
                                lit,
                            )?;
                        }
                        _ => {}
                    }
                }
                break;
            }
            match hit {
                Some(Impact::Entity { target, .. }) => {
                    if !self.ae2_cannon_entity(target, damage(penetration), false, self.creative && !dispenser) {
                        break;
                    }
                }
                Some(Impact::Guest { conn, .. }) => {
                    let amount = damage(penetration);
                    penetration -= amount;
                    let result = if let Some(host) = host.as_deref_mut() {
                        self.ae2_cannon_guest(host, conn, amount, origin, dir)
                    } else if let Some(mut own) = self.lan.take() {
                        let r = self.ae2_cannon_guest(&mut own, conn, amount, origin, dir);
                        self.lan = Some(own);
                        r
                    } else {
                        (false, false)
                    };
                    if !(result.0 && result.1 && penetration > 0.0) {
                        break;
                    }
                }
                Some(Impact::Mob { index, .. }) => {
                    let amount = damage(penetration);
                    penetration -= amount;
                    let old = self.mobs[index].health;
                    let hurt = self.mobs[index].damage(
                        amount,
                        Damage {
                            kind: DamageKind::MatterCannon,
                            attacker: Some(Who::Player),
                            from: Some(origin),
                        },
                    );
                    if hurt && self.mobs[index].health != old {
                        self.mob_damage_event(self.mobs[index].id);
                        self.mobs[index].knockback(0.0, -dir[0], -dir[2]);
                    }
                    if !(hurt && !self.mobs[index].alive() && penetration > 0.0) {
                        break;
                    }
                }
                Some(Impact::Block { pos, .. }) => {
                    if !may_build || !horizon_client::ae2_common_config::damage().matter_cannon {
                        break;
                    }
                    let state = self.world.block(pos[0], pos[1], pos[2]);
                    let Some(left) = remaining(penetration, self.res.facts().get(state).hardness)
                    else {
                        break;
                    };
                    let removed = self.with_level(|b, level, ticks| {
                        b.destroy_block(level, ticks, BlockPos::new(pos[0], pos[1], pos[2]), true)
                    });
                    self.process_level_events().map_err(|e| e.to_string())?;
                    if !removed {
                        break;
                    }
                    penetration = left;
                }
                None => break,
            }
        }
        Ok(UseResult::Success { swing: true })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cannon_projectile_hurt_destroys_only_shulker_bullets() {
        let bullet = ProjectileKind::ShulkerBullet { target:Who::Player, dir:None, steps:0, delta:[0.0;3], seed:0 };
        assert!(cannon_destroys_projectile(&bullet));
        assert!(!cannon_destroys_projectile(&ProjectileKind::Snowball));
        assert!(!cannon_destroys_projectile(&ProjectileKind::LargeFireball { power:1 }));
        assert!(!cannon_destroys_projectile(&ProjectileKind::WindCharge));
        assert!(!cannon_destroys_projectile(&ProjectileKind::WitherSkull { dangerous:false }));
    }
    #[test]
    fn source_penetration_threshold_and_decay() {
        assert_eq!(damage(32.0), 2.0);
        assert_eq!(damage(196.96655), 10.0);
        assert!(remaining(32.0, -1.0).is_none());
        assert!(remaining(13.5, 1.5).is_none());
        let left = remaining(32.0, 0.3).unwrap();
        assert!((left - 17.58).abs() < 0.0001);
        assert!(remaining(left, 1.5).is_some());
    }
}
