//! `ServerExplosion`: exploded positions (vanilla ray march over a Java
//! HashSet, so iteration and the following shuffle match vanilla's random
//! draws), entity damage with exposure and knockback, block interaction with
//! merged drops and TNT chain priming, and optional fire.
use super::Game;
use horizon_client::{Result, blocks::Fluid, collisions::SceneCollisions, items::ItemStack};
use horizon_core::{
    geometry::{Vec3, VoxelShape},
    position::BlockPos,
    random::{LegacyRandom, RandomSource},
};
use horizon_world::{
    collision_context::{CollisionContext, CollisionEntity, CollisionItem},
    entity_obstruction::EntityIdentity,
    java_hash_map::{JavaHashMap, JavaKey},
    registry::BlockStateId,
    world_ray::{RayBlockMode, RayFluidMode, RayWorld, clip_world},
};
use std::{cmp::Ordering, io};

/// `Level.clip` with `ClipContext.Block.COLLIDER`, no fluids and the
/// collision context of the entity whose exposure is measured.
struct ColliderRays<'a, 'r> {
    scene: SceneCollisions<'a, 'r>,
    context: CollisionContext<'static>,
}
impl RayWorld for ColliderRays<'_, '_> {
    type Error = io::Error;
    type Block = BlockStateId;
    type Fluid = ();
    fn block_state(&mut self, p: BlockPos) -> io::Result<BlockStateId> {
        Ok(self.scene.world.block(p.x, p.y, p.z))
    }
    fn fluid_state(&mut self, _: BlockPos) -> io::Result<()> {
        Ok(())
    }
    fn block_shape(
        &mut self,
        state: BlockStateId,
        p: BlockPos,
        _: RayBlockMode,
    ) -> io::Result<VoxelShape> {
        let binding = self.scene.binding;
        binding
            .collision_shape_with_context(&mut self.scene, state, p, &self.context)
            .map_err(|e| io::Error::other(format!("{e:?}")))
    }
    fn interaction_shape(&mut self, _: BlockStateId, _: BlockPos) -> io::Result<VoxelShape> {
        Ok(VoxelShape::empty())
    }
    fn fluid_is_source(&mut self, _: ()) -> io::Result<bool> {
        Ok(false)
    }
    fn fluid_is_empty(&mut self, _: ()) -> io::Result<bool> {
        Ok(true)
    }
    fn fluid_is_water(&mut self, _: ()) -> io::Result<bool> {
        Ok(false)
    }
    fn fluid_shape(&mut self, _: (), _: BlockPos) -> io::Result<VoxelShape> {
        Ok(VoxelShape::empty())
    }
}

/// `BlockPos` with Java `Vec3i.hashCode` / `compareTo`.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Key([i32; 3]);
impl JavaKey for Key {
    fn java_hash(&self) -> i32 {
        let [x, y, z] = self.0;
        y.wrapping_add(z.wrapping_mul(31))
            .wrapping_mul(31)
            .wrapping_add(x)
    }
    fn comparable_compare(&self, other: &Self) -> Option<Ordering> {
        let ([x, y, z], [ox, oy, oz]) = (self.0, other.0);
        Some(y.cmp(&oy).then(z.cmp(&oz)).then(x.cmp(&ox)))
    }
    fn tie_break(&self, _other: &Self) -> Ordering {
        Ordering::Less
    }
}

/// `Explosion.BlockInteraction` for the explosions Horizon produces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BlockInteraction {
    Keep,
    Destroy,
    DestroyWithDecay,
}

thread_local! {
    /// Client-side randomness (explosion sound pitch), separate from the level random.
    static CLIENT_RANDOM: std::cell::RefCell<LegacyRandom> = std::cell::RefCell::new(LegacyRandom::new(0x5eed));
}
pub(super) fn client_float() -> f32 {
    CLIENT_RANDOM.with(|r| r.borrow_mut().next_float())
}

impl Game<'_> {
    /// `ServerExplosion.getSeenPercent` for an entity box.
    pub(super) fn seen_percent(&self, center: [f64; 3], min: [f64; 3], max: [f64; 3]) -> f32 {
        let xs = 1.0 / ((max[0] - min[0]) * 2.0 + 1.0);
        let ys = 1.0 / ((max[1] - min[1]) * 2.0 + 1.0);
        let zs = 1.0 / ((max[2] - min[2]) * 2.0 + 1.0);
        let x_offset = (1.0 - (1.0 / xs).floor() * xs) / 2.0;
        let z_offset = (1.0 - (1.0 / zs).floor() * zs) / 2.0;
        if xs < 0.0 || ys < 0.0 || zs < 0.0 {
            return 0.0;
        }
        // ClipContext(..., entity): the entity standing at the box's bottom.
        let mut rays = ColliderRays {
            scene: SceneCollisions {
                world: &self.world,
                binding: self.collision,
                subject: None,
            },
            context: CollisionContext::for_entity(
                CollisionEntity {
                    identity: EntityIdentity(0),
                    y: min[1],
                    descending: false,
                    main_hand: Some(CollisionItem::EMPTY),
                },
                false,
            ),
        };
        let (mut hits, mut count) = (0, 0);
        let mut xx = 0.0;
        while xx <= 1.0 {
            let mut yy = 0.0;
            while yy <= 1.0 {
                let mut zz = 0.0;
                while zz <= 1.0 {
                    let x = min[0] + (max[0] - min[0]) * xx;
                    let y = min[1] + (max[1] - min[1]) * yy;
                    let z = min[2] + (max[2] - min[2]) * zz;
                    let from = Vec3::new(x + x_offset, y, z + z_offset);
                    let to = Vec3::new(center[0], center[1], center[2]);
                    let missed = clip_world(
                        &mut rays,
                        from,
                        to,
                        RayBlockMode::Collider,
                        RayFluidMode::None,
                        4096,
                    )
                    .is_ok_and(|r| r.miss);
                    if missed {
                        hits += 1;
                    }
                    count += 1;
                    zz += zs;
                }
                yy += ys;
            }
            xx += xs;
        }
        hits as f32 / count as f32
    }
    /// `ServerExplosion.calculateExplodedPositions`; `resists` overrides one
    /// block's resistance (`ExplosionDamageCalculator.getBlockExplosionResistance`).
    fn exploded_positions(
        &mut self,
        center: [f64; 3],
        radius: f32,
        resists: Option<([i32; 3], f32)>,
    ) -> Vec<[i32; 3]> {
        let (min_y, max_y) = self.with_level(|_, level, _| {
            use horizon_world::level::LevelAccess;
            (level.min_y(), level.max_y())
        });
        let mut set: JavaHashMap<Key, ()> = JavaHashMap::default();
        for xx in 0..16 {
            for yy in 0..16 {
                for zz in 0..16 {
                    if !(xx == 0 || xx == 15 || yy == 0 || yy == 15 || zz == 0 || zz == 15) {
                        continue;
                    }
                    let mut xd = f64::from(xx as f32 / 15.0 * 2.0 - 1.0);
                    let mut yd = f64::from(yy as f32 / 15.0 * 2.0 - 1.0);
                    let mut zd = f64::from(zz as f32 / 15.0 * 2.0 - 1.0);
                    let d = (xd * xd + yd * yd + zd * zd).sqrt();
                    xd /= d;
                    yd /= d;
                    zd /= d;
                    let mut power = radius * (0.7 + self.world.random.next_float() * 0.6);
                    let (mut xp, mut yp, mut zp) = (center[0], center[1], center[2]);
                    while power > 0.0 {
                        let pos = [xp.floor() as i32, yp.floor() as i32, zp.floor() as i32];
                        if pos[1] < min_y
                            || pos[1] >= max_y
                            || !(-30_000_000..30_000_000).contains(&pos[0])
                            || !(-30_000_000..30_000_000).contains(&pos[2])
                        {
                            break;
                        }
                        let state = self.world.block(pos[0], pos[1], pos[2]);
                        let f = self.res.facts().get(state);
                        let fluid = if f.fluid != Fluid::None {
                            100.0f32
                        } else {
                            0.0
                        };
                        match resists {
                            Some((at, resistance)) if at == pos => {
                                power -= (resistance + 0.3) * 0.3;
                            }
                            _ if !self.res.facts().is_air(state) || f.fluid != Fluid::None => {
                                power -= (f.explosion_resistance.max(fluid) + 0.3) * 0.3;
                            }
                            _ => {}
                        }
                        if power > 0.0 {
                            set.put(Key(pos), ());
                        }
                        xp += xd * f64::from(0.3f32);
                        yp += yd * f64::from(0.3f32);
                        zp += zd * f64::from(0.3f32);
                        power -= 0.225_000_01;
                    }
                }
            }
        }
        set.entries().map(|(k, _)| k.0).collect()
    }
    /// `ServerExplosion.hurtEntities` (player, mobs, items, primed TNT, minecarts);
    /// the player takes `damage_type`.
    /// `ServerExplosion.hurtEntities` for the player: damage scaled by
    /// distance and exposure, through a raised shield (`hurtServer` blocks
    /// explosions like any attack from where the blast is), then the push
    /// (less with explosion knockback resistance; none for spectators or a
    /// creative player flying).
    pub(super) fn explosion_hurt_player(
        &mut self,
        center: [f64; 3],
        radius: f32,
        damage_type: &str,
        exploding: Option<u32>,
    ) {
        let diameter = f64::from(radius * 2.0);
        // Blast protection: `EXPLOSION_KNOCKBACK_RESISTANCE` (0.15 per level and
        // piece, so two Blast Protection IV pieces cancel it).
        let resistance = self
            .player
            .as_mut()
            .and_then(|p| p.attribute("minecraft:explosion_knockback_resistance").ok())
            .unwrap_or(0.0);
        let Some(p) = self.player.as_ref() else {
            return;
        };
        let feet = [p.position.x, p.position.y, p.position.z];
        let dist = ((feet[0] - center[0]).powi(2)
            + (feet[1] - center[1]).powi(2)
            + (feet[2] - center[2]).powi(2))
        .sqrt()
            / diameter;
        if dist > 1.0 || p.health.dead_or_dying() {
            return;
        }
        let w = f64::from(p.dimensions.width) / 2.0;
        let h = f64::from(p.dimensions.height);
        let eye = [
            feet[0],
            feet[1] + f64::from(p.dimensions.eye_height),
            feet[2],
        ];
        let exposure = self.seen_percent(
            center,
            [feet[0] - w, feet[1], feet[2] - w],
            [feet[0] + w, feet[1] + h, feet[2] + w],
        );
        let pow = (1.0 - dist) * f64::from(exposure);
        let amount = ((pow * pow + pow) / 2.0 * 7.0 * diameter + 1.0) as f32;
        let d = [eye[0] - center[0], eye[1] - center[1], eye[2] - center[2]];
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let dir = if l < 1.0e-4 {
            [0.0; 3]
        } else {
            [d[0] / l, d[1] / l, d[2] / l]
        };
        let power = pow * (1.0 - resistance);
        // `source.doTeamsAllowDamage(entity)`.
        let hurtable = exploding.is_none_or(|id| {
            horizon_client::scoreboard::teams_allow_damage(self.mob_team(id), self.player_team())
        });
        let blocking = self.player_blocking(Some(center), damage_type, false);
        let raised = blocking.is_some();
        let (creative, spectator) = (self.creative, self.spectator());
        let flying = self.player.as_ref().is_some_and(|p| p.flying);
        let mut outcome = horizon_client::health::BlockOutcome::default();
        if let Some(p) = self.player.as_mut()
            && let Some(id) = self.motion.health.damage.id(damage_type)
        {
            // Creative players are invulnerable to explosions.
            if !creative
                && !spectator
                && hurtable
                && let Ok(o) = p.hurt_caused_by(
                    &self.motion.health,
                    self.world.health_rules,
                    id,
                    amount,
                    blocking,
                    // The exploding mob is the causing entity.
                    exploding.is_some(),
                )
            {
                outcome = o;
            }
            if !spectator && !(creative && flying) {
                p.velocity = Vec3::new(
                    p.velocity.x + dir[0] * power,
                    p.velocity.y + dir[1] * power,
                    p.velocity.z + dir[2] * power,
                );
            }
        }
        if outcome.hurt && !creative {
            self.hurt_armor(amount);
        }
        if raised {
            self.after_block(outcome, amount, None, false);
        }
    }

    fn explosion_hurt_entities(&mut self, center: [f64; 3], radius: f32, damage_type: &str) {
        if radius < 1.0e-5 {
            return;
        }
        self.explosion_hits_special(center, radius);
        let diameter = f64::from(radius * 2.0);
        let distance = |p: [f64; 3]| {
            ((p[0] - center[0]).powi(2) + (p[1] - center[1]).powi(2) + (p[2] - center[2]).powi(2))
                .sqrt()
        };
        let damage = |dist: f64, exposure: f32| {
            let pow = (1.0 - dist) * f64::from(exposure);
            ((pow * pow + pow) / 2.0 * 7.0 * diameter + 1.0) as f32
        };
        let direction = |origin: [f64; 3]| {
            let d = [
                origin[0] - center[0],
                origin[1] - center[1],
                origin[2] - center[2],
            ];
            let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if l < 1.0e-4 {
                [0.0; 3]
            } else {
                [d[0] / l, d[1] / l, d[2] / l]
            }
        };
        // `source.doTeamsAllowDamage(entity)`: the exploding mob spares its
        // team mates when its team has no friendly fire (they are still pushed).
        let exploding = self.combat.explosion_source;
        let teams_allow = |g: &Self, victim: Option<&horizon_client::scoreboard::Team>| {
            exploding.is_none_or(|id| {
                horizon_client::scoreboard::teams_allow_damage(g.mob_team(id), victim)
            })
        };
        // The player, then every other player in the level (LAN guests),
        // each swapped in as the player.
        self.explosion_hurt_player(center, radius, damage_type, exploding);
        if let Some(mut host) = self.lan.take() {
            let guests: Vec<u32> = host
                .hub
                .guests
                .iter()
                .filter(|g| host.hub.here(g) && g.loaded && !g.dead)
                .map(|g| g.conn)
                .collect();
            for conn in guests {
                self.guest_hit_by(&mut host, conn, |g| {
                    g.explosion_hurt_player(center, radius, damage_type, exploding);
                });
            }
            self.lan = Some(host);
        }
        // Mobs.
        let mut mobs = std::mem::take(&mut self.mobs);
        for m in mobs.iter_mut().filter(|m| m.alive()) {
            let hurtable = teams_allow(self, self.mob_team(m.id));
            let feet = [m.position.x, m.position.y, m.position.z];
            let dist = distance(feet) / diameter;
            if dist > 1.0 {
                continue;
            }
            let (w, h) = (m.kind.width / 2.0, m.kind.height);
            let eye = [feet[0], feet[1] + m.kind.eye_height, feet[2]];
            let exposure = self.seen_percent(
                center,
                [feet[0] - w, feet[1], feet[2] - w],
                [feet[0] + w, feet[1] + h, feet[2] + w],
            );
            let source = horizon_client::mobs::Damage {
                kind: horizon_client::mobs::DamageKind::Explosion,
                attacker: None,
                from: Some(center),
            };
            if hurtable && !m.damage(damage(dist, exposure), source) {
                continue;
            }
            // `EXPLOSION_KNOCKBACK_RESISTANCE`, not `KNOCKBACK_RESISTANCE`: its
            // base is 0 for every mob type, so iron golems and ravagers are pushed.
            let dir = direction(eye);
            let power = (1.0 - dist) * f64::from(exposure);
            m.velocity = Vec3::new(
                m.velocity.x + dir[0] * power,
                m.velocity.y + dir[1] * power,
                m.velocity.z + dir[2] * power,
            );
        }
        self.mobs = mobs;
        // Dropped items: 5 health, destroyed by the explosion damage (nether stars resist).
        #[cfg(feature = "ae2-lab")]
        let ae2_protected = self.ae2_explosion_singularities(center, radius);
        let mut items = std::mem::take(&mut self.dropped);
        items.retain_mut(|item| {
            #[cfg(feature = "ae2-lab")]
            if ae2_protected.contains(&item.id) { return true; }
            let feet = [item.position.x, item.position.y, item.position.z];
            let dist = distance(feet) / diameter;
            if dist > 1.0 {
                return true;
            }
            let half = 0.125;
            let exposure = self.seen_percent(
                center,
                [feet[0] - half, feet[1], feet[2] - half],
                [feet[0] + half, feet[1] + 0.25, feet[2] + half],
            );
            if &*item.stack.item != "minecraft:nether_star" && damage(dist, exposure) >= 5.0 {
                return false;
            }
            let dir = direction([feet[0], feet[1] + 0.2125, feet[2]]);
            let power = (1.0 - dist) * f64::from(exposure);
            item.velocity = Vec3::new(
                item.velocity.x + dir[0] * power,
                item.velocity.y + dir[1] * power,
                item.velocity.z + dir[2] * power,
            );
            true
        });
        self.dropped = items;
        // Primed TNT: knockback only, from its position (not its eyes).
        let mut tnts = std::mem::take(&mut self.tnts);
        for t in &mut tnts {
            let feet = [t.position.x, t.position.y, t.position.z];
            let dist = distance(feet) / diameter;
            if dist > 1.0 {
                continue;
            }
            let half = 0.49;
            let exposure = self.seen_percent(
                center,
                [feet[0] - half, feet[1], feet[2] - half],
                [feet[0] + half, feet[1] + 0.98, feet[2] + half],
            );
            let dir = direction(feet);
            let power = (1.0 - dist) * f64::from(exposure);
            t.velocity = Vec3::new(
                t.velocity.x + dir[0] * power,
                t.velocity.y + dir[1] * power,
                t.velocity.z + dir[2] * power,
            );
        }
        self.tnts = tnts;
        // Minecarts: VehicleEntity.hurtServer (explosions ignite TNT carts),
        // then the push from their eyes (0.85 of their height).
        let ids: Vec<u32> = self.carts.iter().map(|c| c.id).collect();
        for id in ids {
            let Some(i) = self.carts.iter().position(|c| c.id == id) else {
                continue;
            };
            let bounds = self.carts[i].bounds();
            let feet = self.carts[i].position;
            let dist = distance([feet.x, feet.y, feet.z]) / diameter;
            if dist > 1.0 {
                continue;
            }
            let (lo, hi) = (bounds.min_position(), bounds.max_position());
            let exposure = self.seen_percent(center, [lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]);
            self.hurt_cart(i, damage(dist, exposure), false, true);
            let Some(c) = self.carts.iter_mut().find(|c| c.id == id) else {
                continue;
            };
            let eye_height = f64::from((hi.y - lo.y) as f32 * 0.85f32);
            let dir = direction([feet.x, feet.y + eye_height, feet.z]);
            let power = (1.0 - dist) * f64::from(exposure);
            c.velocity = Vec3::new(
                c.velocity.x + dir[0] * power,
                c.velocity.y + dir[1] * power,
                c.velocity.z + dir[2] * power,
            );
        }
    }
    /// `Level.explode` on the server: positions, entities, blocks, fire; then
    /// the client's explosion sound.
    pub(super) fn server_explode(
        &mut self,
        center: [f64; 3],
        radius: f32,
        interaction: BlockInteraction,
        fire: bool,
    ) -> Result<()> {
        self.server_explode_as(
            center,
            radius,
            interaction,
            fire,
            "minecraft:explosion",
            None,
        )
    }
    /// `server_explode` with the damage type the player takes and a block
    /// whose explosion resistance the damage calculator overrides.
    pub(super) fn server_explode_as(
        &mut self,
        center: [f64; 3],
        radius: f32,
        interaction: BlockInteraction,
        fire: bool,
        damage_type: &str,
        resists: Option<([i32; 3], f32)>,
    ) -> Result<()> {
        if let Some(p) = &self.player {
            let d = [
                p.position.x - center[0],
                p.position.y - center[1],
                p.position.z - center[2],
            ];
            let distance = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            self.pad_rumble(horizon_client::gamepad::Rumble::Explosion { distance, radius });
        }
        // Explosion.explode: the blast is heard.
        let ctx = horizon_client::sculk::Context::default();
        self.post_game_event("minecraft:explode", center, ctx);
        let mut targets = self.exploded_positions(center, radius, resists);
        self.explosion_hurt_entities(center, radius, damage_type);
        if interaction != BlockInteraction::Keep {
            // Util.shuffle(list, level.random)
            for i in (2..=targets.len()).rev() {
                let swap = self.world.random.next_int_bound(i as i32).unwrap_or(0) as usize;
                targets.swap(i - 1, swap);
            }
            let mut stacks: Vec<([i32; 3], ItemStack)> = Vec::new();
            for &pos in &targets {
                self.explosion_hit_block(pos, radius, interaction, &mut stacks,None)?;
            }
            for (pos, stack) in stacks {
                self.pop_resource(pos, stack);
            }
        }
        if fire {
            for &pos in &targets {
                if self.world.random.next_int_bound(3).unwrap_or(1) == 0
                    && self
                        .res
                        .facts()
                        .is_air(self.world.block(pos[0], pos[1], pos[2]))
                    && self
                        .res
                        .facts()
                        .get(self.world.block(pos[0], pos[1] - 1, pos[2]))
                        .solid_render
                {
                    // BaseFireBlock.getState: soul fire on its base blocks,
                    // fire shaped by its neighbours elsewhere.
                    let at = BlockPos::new(pos[0], pos[1], pos[2]);
                    let fire =
                        self.with_level(|b, level, ticks| b.fire_state_for(level, ticks, at));
                    self.level_set(pos, fire)?;
                }
            }
        }
        let pitch = (1.0 + (client_float() - client_float()) * 0.2) * 0.7;
        self.audio
            .play("minecraft:entity.generic.explode", Some(center), 4.0, pitch);
        let block_count = if interaction == BlockInteraction::Keep {
            0
        } else {
            targets.len()
        };
        let (world, facts) = (&self.world, self.res.facts());
        self.particles.explosion(center, radius, block_count, &|b| {
            facts.is_air(world.block(b[0], b[1], b[2]))
        });
        self.lan_explode(center, radius, block_count);
        Ok(())
    }
    /// AE2 TinyTNTPrimedEntity.explode: a compact, resistance-weighted blast.
    #[cfg(feature = "ae2-lab")]
    pub(super) fn ae2_explode_tiny_tnt(&mut self, center: [f64; 3],owner:Option<super::tnt::TinyTntOwner>) -> Result<()> {
        let overlaps = |p: [f64;3], width:f64, height:f64| {
            let h=width/2.0;
            p[0]+h > center[0]-1.5 && p[0]-h < center[0]+1.5 &&
            p[1]+height > center[1]-1.5 && p[1] < center[1]+1.5 &&
            p[2]+h > center[2]-1.5 && p[2]-h < center[2]+1.5
        };
        for mob in &mut self.mobs {
            if mob.alive() && overlaps([mob.position.x,mob.position.y,mob.position.z],mob.kind.width,mob.kind.height) {
                let source=horizon_client::mobs::Damage{kind:horizon_client::mobs::DamageKind::Explosion,attacker:owner.map(|_|horizon_client::mobs::Who::Player),from:Some(center)};
                if mob.damage(6.0,source) && let Some(super::tnt::TinyTntOwner::Player(conn))=owner {self.last_hurt_by_player.insert(mob.id,conn);}
            }
        }
        let affected:Vec<u32>=self.dropped.iter().filter_map(|item|{
            let p=item.position;overlaps([p.x,p.y,p.z],0.25,0.25).then_some(item.id)
        }).collect();
        let protected=self.ae2_explosion_transform_items(&affected);
        self.dropped.retain_mut(|item| {
            let p=item.position;
            if !protected.contains(&item.id) && overlaps([p.x,p.y,p.z],0.25,0.25) && &*item.stack.item != "minecraft:nether_star" {
                item.health-=6;
            }
            item.health>0
        });
        let cart_ids:Vec<u32>=self.carts.iter().filter_map(|cart| {
            let b=cart.bounds();let lo=b.min_position();let hi=b.max_position();
            (hi.x>center[0]-1.5 && lo.x<center[0]+1.5 && hi.y>center[1]-1.5 && lo.y<center[1]+1.5 && hi.z>center[2]-1.5 && lo.z<center[2]+1.5).then_some(cart.id)
        }).collect();
        for id in cart_ids {if let Some(i)=self.carts.iter().position(|cart|cart.id==id){self.hurt_cart(i,6.0,false,true);}}
        let intersects=|lo:[f64;3],hi:[f64;3]|(0..3).all(|axis|hi[axis]>center[axis]-1.5 && lo[axis]<center[axis]+1.5);
        let boats:Vec<usize>=self.boats.iter().enumerate().filter_map(|(i,b)|{
            let bounds=b.bounds();let lo=bounds.min_position();let hi=bounds.max_position();
            intersects([lo.x,lo.y,lo.z],[hi.x,hi.y,hi.z]).then_some(i)
        }).collect();
        for i in boats.into_iter().rev(){self.hurt_boat(i,6.0,false);}
        let stands:Vec<usize>=self.armor_stands.iter().enumerate().filter_map(|(i,a)|{
            let bounds=a.bounds();let lo=bounds.min_position();let hi=bounds.max_position();
            intersects([lo.x,lo.y,lo.z],[hi.x,hi.y,hi.z]).then_some(i)
        }).collect();
        for i in stands.into_iter().rev(){self.ae2_explode_armor_stand_at(i);}
        let frames:Vec<usize>=self.item_frames.iter().enumerate().filter_map(|(i,f)|{
            let (lo,hi)=f.bounds();intersects(lo,hi).then_some(i)
        }).collect();
        for i in frames.into_iter().rev(){self.ae2_explode_frame_at(i);}
        let paintings:Vec<usize>=self.paintings.iter().enumerate().filter_map(|(i,p)|{
            let (lo,hi)=p.bounds();intersects(lo,hi).then_some(i)
        }).collect();
        for i in paintings.into_iter().rev(){self.break_painting(i,false);}
        self.ae2_tiny_tnt_hurt_player(center,owner);
        if let Some(mut host)=self.lan.take() {
            let guests:Vec<u32>=host.hub.guests.iter().filter(|g|host.hub.here(g)&&g.loaded&&!g.dead).map(|g|g.conn).collect();
            for conn in guests { self.guest_hit_by(&mut host,conn,|g|g.ae2_tiny_tnt_hurt_player(center,owner)); }
            self.lan=Some(host);
        }
        if horizon_client::ae2_common_config::damage().tiny_tnt {
        let y=center[1]-0.25;
        let mut drops=Vec::new();
        for x in (center[0]-2.0) as i32..=(center[0]+2.0).floor() as i32 {
          for yy in (y-2.0) as i32..=(y+2.0).floor() as i32 {
            for z in (center[2]-2.0) as i32..=(center[2]+2.0).floor() as i32 {
              let pos=[x,yy,z]; let state=self.world.block(x,yy,z); let f=self.res.facts().get(state);
              if self.res.facts().is_air(state) {continue;}
              let d=(x as f64+0.5-center[0]).powi(2)+(yy as f64+0.5-y).powi(2)+(z as f64+0.5-center[2]).powi(2);
              let fluid=if f.fluid != Fluid::None {100.0} else {0.0};
              let strength=2.3_f32-d as f32-(f.explosion_resistance.max(fluid)+0.3)*0.11;
              if strength>0.01 {self.explosion_hit_block(pos,0.2,BlockInteraction::Destroy,&mut drops,owner)?;}
            }
          }
        }
        for (pos,stack) in drops {self.pop_resource(pos,stack);}
        }
        let pitch=(1.0+(self.world.random.next_float()-self.world.random.next_float())*0.2)*32.9;
        self.audio.play("minecraft:entity.generic.explode",Some(center),4.0,pitch);
        self.lan_explode(center,0.2,0);
        Ok(())
    }
    #[cfg(feature = "ae2-lab")]
    fn ae2_tiny_tnt_hurt_player(&mut self,center:[f64;3],owner:Option<super::tnt::TinyTntOwner>) {
        let Some(p)=self.player.as_ref() else {return};
        let w=f64::from(p.dimensions.width)/2.0; let h=f64::from(p.dimensions.height);
        let pos=p.position;
        if pos.x+w<=center[0]-1.5 || pos.x-w>=center[0]+1.5 || pos.y+h<=center[1]-1.5 || pos.y>=center[1]+1.5 || pos.z+w<=center[2]-1.5 || pos.z-w>=center[2]+1.5 || self.creative || self.spectator() {return;}
        let damage_type=if owner.is_some() {"minecraft:player_explosion"} else {"minecraft:explosion"};
        let blocking=self.player_blocking(Some(center),damage_type,false);
        let raised=blocking.is_some();
        if let Some(id)=self.motion.health.damage.id(damage_type) {
            if let Some(player)=self.player.as_mut() {
                if let Ok(outcome)=player.hurt_caused_by(&self.motion.health,self.world.health_rules,id,6.0,blocking,owner.is_some()) {
                    if outcome.hurt {self.hurt_armor(6.0);}
                    if raised {self.after_block(outcome,6.0,None,false);}
                }
            }
        }
    }
    /// `BlockBehaviour.onExplosionHit` (TNT: `wasExploded` primes a short fuse).
    fn explosion_hit_block(
        &mut self,
        pos: [i32; 3],
        radius: f32,
        interaction: BlockInteraction,
        stacks: &mut Vec<([i32; 3], ItemStack)>,
        owner: Option<super::tnt::TinyTntOwner>,
    ) -> Result<()> {
        let state = self.world.block(pos[0], pos[1], pos[2]);
        if self.res.facts().is_air(state) {
            return Ok(());
        }
        let name = self.res.facts().get(state).name.clone();
        let is_tiny = cfg!(feature = "ae2-lab") && &*name == "ae2:tiny_tnt";
        let is_tnt = &*name == "minecraft:tnt" || is_tiny;
        // Block.onExplosionHit: the drops copy from the block entity, which
        // then spills its contents as the block turns to air.
        let key = (pos[0], pos[1], pos[2]);
        if let Some(entity) = self.world.block_entities.get(&key).cloned() {
            let custom = self.world.block_names.get(&key).cloned();
            self.pending.record(pos, &entity, &name, custom);
        } else if let Some(tag) = super::spill::saved_entity(&self.world, pos) {
            self.pending.record_saved(pos, &tag);
        }
        if !is_tnt {
            let dynamic = self.pending.dynamic(pos);
            // LootContextParams.EXPLOSION_RADIUS under DESTROY_WITH_DECAY: the
            // tables' survives_explosion and explosion_decay draw from their
            // random sequence.
            let decay = (interaction == BlockInteraction::DestroyWithDecay).then_some(radius);
            let drops = self.res.items().block_drops_exploded(
                self.pack.assets.registry,
                state,
                None,
                &dynamic,
                &mut self.world.random,
                Some((&mut self.rand_sequences, self.level.seed)),
                decay,
            );
            for mut stack in drops {
                if &*stack.item == "minecraft:air" || stack.count == 0 {
                    continue;
                }
                self.pending.attach(pos, &mut stack);
                add_or_append_stack(stacks, stack, pos);
            }
        }
        self.pending.clear(pos);
        self.level_set(pos, self.world.air)?;
        // TntBlock.wasExploded: new PrimedTnt (its push drawn first), then
        // getRandomShortFuse(80); no game event.
        if is_tnt && (is_tiny || self.ui.rules.bool("tnt_explodes")) {
            let r = &mut self.world.random;
            let rot = r.next_double() * f64::from(std::f32::consts::TAU);
            let fuse = r.next_int_bound(20).unwrap_or(0) + 10;
            self.add_primed_tnt(BlockPos::new(pos[0], pos[1], pos[2]), rot, fuse);
            if is_tiny { if let Some(t) = self.tnts.last_mut() { t.tiny = true;t.tiny_owner=owner; } }
        }
        Ok(())
    }
}

/// `ServerExplosion.addOrAppendStack` with `ItemEntity.merge(.., 16)`.
fn add_or_append_stack(
    stacks: &mut Vec<([i32; 3], ItemStack)>,
    mut stack: ItemStack,
    pos: [i32; 3],
) {
    for (_, existing) in stacks.iter_mut() {
        if existing.item == stack.item && existing.damage == stack.damage && existing.count < 16 {
            let moved = (16 - existing.count).min(stack.count);
            existing.count += moved;
            stack.count -= moved;
            if stack.count == 0 {
                return;
            }
        }
    }
    stacks.push((pos, stack));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_pos_hash_matches_vec3i() {
        // Vec3i.hashCode: (y + z * 31) * 31 + x
        assert_eq!(Key([1, 2, 3]).java_hash(), (2 + 3 * 31) * 31 + 1);
        assert_eq!(Key([-5, 64, -7]).java_hash(), (64 - 7 * 31) * 31 - 5);
        assert_eq!(
            Key([0, 1, 0]).comparable_compare(&Key([5, 0, 9])),
            Some(Ordering::Greater)
        );
        assert_eq!(
            Key([1, 0, 2]).comparable_compare(&Key([0, 0, 3])),
            Some(Ordering::Less)
        );
    }

    #[test]
    fn drops_merge_up_to_sixteen() {
        let stack = |item: &str| ItemStack {
            item: item.into(),
            count: 1,
            damage: 0,
            extra: None,
        };
        let mut stacks = Vec::new();
        for _ in 0..20 {
            add_or_append_stack(&mut stacks, stack("minecraft:dirt"), [0, 0, 0]);
        }
        add_or_append_stack(&mut stacks, stack("minecraft:stone"), [1, 0, 0]);
        let counts: Vec<u32> = stacks.iter().map(|(_, s)| s.count).collect();
        assert_eq!(counts, vec![16, 4, 1]);
    }
}
