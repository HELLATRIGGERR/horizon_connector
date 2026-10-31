//! Dispenser use of buckets: `BucketPickup.pickupBlock` for an empty bucket and
//! `DispensibleContainerItem.emptyContents` for filled ones (`BucketItem`,
//! `MobBucketItem`, `SolidBucketItem`), both without a player.
use super::{
    Behaviours, Dimension, LevelAccess, Sim, Ticks,
    classes::{Class, Interface},
};
use crate::{
    fluid::{FluidKind, FluidState},
    registry::BlockStateId,
};
use horizon_core::{position::BlockPos, random::RandomSource};
use std::sync::Arc;

/// What a filled bucket empties into the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BucketContent {
    /// Water or lava (mob buckets hold water).
    Fluid(FluidKind),
    /// `SolidBucketItem`: the block placed into an empty space (powder snow).
    Powder,
    /// `MobBucketItem` holding water: its empty sound posts no `FLUID_PLACE`.
    MobWater,
    /// `MobBucketItem` without a fluid (the sulfur cube bucket): only the sound.
    Nothing,
}

impl BucketContent {
    /// `BucketItem.content == Fluids.WATER` (`MobBucketItem` holding water
    /// counts too): the only content a bucket pours inside a liquid
    /// container, and the only one `canBeReplaced(Fluid)` sees as water.
    pub fn is_water(self) -> bool {
        matches!(self, Self::Fluid(FluidKind::Water) | Self::MobWater)
    }
}

impl<L: LevelAccess + ?Sized> Sim<'_, L> {
    /// `BucketPickup.pickupBlock(user, level, pos, state)`: the filled bucket
    /// that results, None when the block gives nothing. `creative` is whether
    /// `user` is a creative player (only barriers care).
    pub(crate) fn bucket_pickup(&mut self, pos: BlockPos, creative: bool) -> Option<&'static str> {
        let b = self.b;
        let state = self.state(pos);
        let item = if b.instance_of(state, Class::Liquid) {
            match b.fluid(state).kind() {
                FluidKind::Water | FluidKind::FlowingWater => "minecraft:water_bucket",
                FluidKind::Lava | FluidKind::FlowingLava => "minecraft:lava_bucket",
                FluidKind::Empty => return None,
            }
        } else if b.implements(state, Interface::SimpleWaterlogged)
            && b.table.has(state, b.props.waterlogged)
        {
            "minecraft:water_bucket"
        } else if b.is_block(state, b.named.powder_snow) {
            "minecraft:powder_snow_bucket"
        } else if b.is_block(state, b.named.bubble_column) {
            "minecraft:water_bucket"
        } else {
            return None;
        };
        self.pickup_block(pos, state, creative).then_some(item)
    }
    /// `BucketItem.emptyContents(user, level, pos, hitResult)`. `sound` is the
    /// bucket's empty sound (mob buckets have their own); `sneaking` is
    /// `user.isShiftKeyDown()` (false for a dispenser, whose user is null);
    /// `creative` is whether that user is a creative player.
    pub(crate) fn bucket_empty(
        &mut self,
        pos: BlockPos,
        content: BucketContent,
        sound: &str,
        sneaking: bool,
        creative: bool,
    ) -> bool {
        let b = self.b;
        let fluid = match content {
            BucketContent::Powder => {
                if self.is_in_valid_bounds(pos) && b.is_air(self.state(pos)) {
                    let snow = b.table.default_state(b.named.powder_snow);
                    self.set_block_and_update(pos, snow);
                    // SolidBucketItem.emptyContents: one FLUID_PLACE, then the place sound.
                    self.game_event(pos, "minecraft:fluid_place");
                    let sound: Arc<str> = sound.into();
                    self.sound(pos, &sound, 1.0, 1.0);
                    return true;
                }
                return false;
            }
            BucketContent::Nothing => {
                let sound: Arc<str> = sound.into();
                self.sound(pos, &sound, 1.0, 1.0);
                return true;
            }
            BucketContent::MobWater => FluidKind::Water,
            BucketContent::Fluid(fluid) => fluid,
        };
        let mob = content == BucketContent::MobWater;
        let state = self.state(pos);
        // canBeReplaced(fluid): replaceable or not solid (portals refuse).
        let may_replace = !b.instance_of(state, Class::EndPortal)
            && !b.instance_of(state, Class::EndGateway)
            && (b.is_replaceable(state) || !b.is_solid(state));
        let liquid_container = self.is_liquid_container(state);
        let container = liquid_container && self.can_place_liquid(state, fluid, creative);
        let place_liquid = may_replace || container;
        // `canPlaceFluidInsideBlock`: air always takes the fluid; a block that
        // merely holds it (a waterloggable block, a replaceable plant) only
        // when the player is not sneaking (a dispenser has no user and so
        // no hit result, which always allows it).
        if !(b.is_air(state) || place_liquid && !sneaking) {
            return false;
        }
        let water = matches!(fluid, FluidKind::Water | FluidKind::FlowingWater);
        if water && self.level.dimension() == Dimension::Nether {
            // WATER_EVAPORATES: the water hisses away.
            let pitch = 2.6 + (self.random().next_float() - self.random().next_float()) * 0.8;
            self.emit(super::LevelEvent::Sound {
                pos,
                sound: "minecraft:block.fire.extinguish".into(),
                volume: 0.5,
                pitch,
            });
            return true;
        }
        if liquid_container && water {
            self.place_liquid(
                pos,
                state,
                FluidState::new(FluidKind::Water, 8, false),
                creative,
            );
            self.empty_sound(pos, sound, mob);
            return true;
        }
        if may_replace && !b.instance_of(state, Class::Liquid) {
            self.destroy_block(pos, true, super::flags::UPDATE_LIMIT);
        }
        let source = FluidState::new(
            if water {
                FluidKind::Water
            } else {
                FluidKind::Lava
            },
            8,
            false,
        );
        let legacy: BlockStateId = b.legacy_block(source);
        if !self.set_block(pos, legacy, super::flags::UPDATE_ALL_IMMEDIATE)
            && !self.fluid_at(pos).is_source()
        {
            return false;
        }
        self.empty_sound(pos, sound, mob);
        true
    }
    /// AE2 FluidPlacementStrategy.canPlace for the four vanilla flowing-fluid
    /// registry entries. Source caches failures at the owning plane, not here.
    fn formation_fluid_can_place(&self, pos: BlockPos, fluid: FluidKind) -> bool {
        if fluid == FluidKind::Empty {return false;}
        let b=self.b;
        let state=self.state(pos);
        // `Fluid.defaultFluidState`: flowing fluids default to level 1 and
        // FALLING=true; source fluids have amount 8 and become legacy level 0.
        let amount=if matches!(fluid,FluidKind::FlowingWater|FluidKind::FlowingLava) {1} else {8};
        let legacy=b.legacy_block(FluidState::new(fluid,amount,true));
        if state==legacy {return false;}
        if b.is_air(state) {return true;}
        let replaceable=!b.instance_of(state,Class::EndPortal)
            && !b.instance_of(state,Class::EndGateway)
            && (b.is_replaceable(state) || !b.is_solid(state));
        replaceable || (self.is_liquid_container(state) && self.can_place_liquid(state,fluid))
    }
    /// AE2 FluidPlacementStrategy.placeInWorld MODULATE after canPlace. Effects
    /// are emitted separately, after callbacks and the plane's throttle check.
    fn formation_fluid_place(&mut self,pos:BlockPos,fluid:FluidKind)->bool {
        if !self.formation_fluid_can_place(pos,fluid) {return false;}
        let b=self.b;let state=self.state(pos);let old_fluid=self.fluid_at(pos);
        let water=matches!(fluid,FluidKind::Water|FluidKind::FlowingWater);
        if water && self.level.dimension()==Dimension::Nether {
            return true;
        }
        if self.is_liquid_container(state) && fluid==FluidKind::Water {
            // Source ignores placeLiquid's result, including an already-filled
            // waterloggable block, and still consumes one full bucket.
            self.place_liquid(pos,state,FluidState::new(FluidKind::Water,8,false));
            return true;
        }
        let replaceable=!b.instance_of(state,Class::EndPortal)
            && !b.instance_of(state,Class::EndGateway)
            && (b.is_replaceable(state) || !b.is_solid(state));
        if replaceable && !b.instance_of(state,Class::Liquid) {
            self.destroy_block(pos,true,super::flags::UPDATE_LIMIT);
        }
        let amount=if matches!(fluid,FluidKind::FlowingWater|FluidKind::FlowingLava) {1} else {8};
        let legacy=b.legacy_block(FluidState::new(fluid,amount,true));
        if !self.set_block(pos,legacy,super::flags::UPDATE_ALL_IMMEDIATE) && !old_fluid.is_source() {
            return false;
        }
        true
    }
    fn formation_fluid_effect(&mut self,pos:BlockPos,fluid:FluidKind) {
        let water=matches!(fluid,FluidKind::Water|FluidKind::FlowingWater);
        if water && self.level.dimension()==Dimension::Nether {
            let pitch=2.6+(self.random().next_float()-self.random().next_float())*0.8;
            self.emit(super::LevelEvent::Sound {pos,sound:"minecraft:block.fire.extinguish".into(),volume:0.5,pitch});
            // Source calls Level.addParticle eight times on ServerLevel;
            // the 1.21.1 server inherits Level's no-op addParticle.
        } else {
            self.empty_sound(pos,if water {"minecraft:item.bucket.empty"} else {"minecraft:item.bucket.empty_lava"},false);
        }
    }
    /// `playEmptySound`: `MobBucketItem` overrides it without the game event.
    fn empty_sound(&mut self, pos: BlockPos, sound: &str, mob: bool) {
        let sound: Arc<str> = sound.into();
        self.sound(pos, &sound, 1.0, 1.0);
        if !mob {
            self.game_event(pos, "minecraft:fluid_place");
        }
    }
}

impl Behaviours {
    /// Query AE2's source fluid placement check without mutating the world.
    pub fn formation_fluid_can_place<L: LevelAccess + ?Sized>(
        &self, level:&mut L, ticks:&mut Ticks, pos:BlockPos, fluid:FluidKind,
    )->bool {
        self.sim(level,ticks).formation_fluid_can_place(pos,fluid)
    }
    /// Execute an already-authorized AE2 fluid placement without effects.
    pub fn formation_fluid_place<L: LevelAccess + ?Sized>(
        &self, level:&mut L, ticks:&mut Ticks, pos:BlockPos, fluid:FluidKind,
    )->bool {
        self.sim(level,ticks).formation_fluid_place(pos,fluid)
    }
    /// Emit the source sound and game event after the owning plane's throttle.
    pub fn formation_fluid_effect<L: LevelAccess + ?Sized>(
        &self, level:&mut L, ticks:&mut Ticks, pos:BlockPos, fluid:FluidKind,
    ) {
        self.sim(level,ticks).formation_fluid_effect(pos,fluid)
    }
    /// `BucketPickup.pickupBlock(user, level, pos, state)`: the filled bucket
    /// item. A dispenser has no user (`false`).
    pub fn bucket_pickup<L: LevelAccess + ?Sized>(
        &self,
        level: &mut L,
        ticks: &mut Ticks,
        pos: BlockPos,
        creative: bool,
    ) -> Option<&'static str> {
        self.sim(level, ticks).bucket_pickup(pos, creative)
    }
    /// `DispensibleContainerItem.emptyContents(user, level, pos, hitResult)`.
    pub fn bucket_empty<L: LevelAccess + ?Sized>(
        &self,
        level: &mut L,
        ticks: &mut Ticks,
        pos: BlockPos,
        content: BucketContent,
        sound: &str,
        sneaking: bool,
        creative: bool,
    ) -> bool {
        self.sim(level, ticks)
            .bucket_empty(pos, content, sound, sneaking, creative)
    }
}
