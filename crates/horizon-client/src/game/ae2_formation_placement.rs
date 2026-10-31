//! Real native BlockItem placement behind an atomic Formation transaction.
use super::{Game,ae2_formation::{FormationAction,FormationPlan,PlaneOwner,PlaneTarget,FluidPreview}};
use horizon_client::{containers::BlockEntity,items::ItemStack,ae2_world::MachineKind};
use horizon_core::{geometry::{Direction,Vec3},position::BlockPos};
use horizon_world::{level::{PlaceContext,PlaceOutcome},registry::BlockStateId,nbt::Tag,fluid::FluidKind};

impl Game<'_> {
    pub(super) fn ae2_formation_owner_valid(&self,owner:PlaneOwner)->bool {
        let Some((dim,pos))=self.ae2_address_parts(owner.address) else{return false};
        let Some(world)=self.ae2_world_for(dim) else{return false};
        if world.block_entities.incarnation(&pos)!=Some(owner.identity.host) {return false;}
        let Some(BlockEntity::Ae2(host))=world.block_entities.get(&pos) else{return false};
        if host.kind!=MachineKind::CableBus {return false;}
        let Some(Tag::List(parts))=host.config.get(&"parts".into()) else{return false};
        parts.iter().any(|part| {
            let face=match part.get("face") {Some(Tag::String(face))=>super::ae2_grid::face_index(&face.to_string_lossy()),_=>None};
            face==Some(owner.face) && super::ae2_channels::part_identity(part)==owner.identity.part
                && matches!(part.get("id"),Some(Tag::String(id)) if id.to_string_lossy()=="ae2:formation_plane")
        })
    }
    fn ae2_try_formation_block(&mut self,pos:(i32,i32,i32),stack:&ItemStack,plane:PlaneOwner,player_owner:Option<[i32;4]>)->Result<bool,String> {
        if !self.ae2_formation_owner_valid(plane) {return Err("formation plane was replaced".into());}
        if !self.world.loaded(pos.0,pos.2) {return Err("formation target unloaded".into());}
        if self.ae2_item_cached_blocked(plane) {return Ok(false);}
        let old=self.world.block(pos.0,pos.1,pos.2);
        if !self.res.facts().get(old).replaceable {self.ae2_item_update_blocked(plane,pos);return Ok(false);}
        let look=plane.direction();
        let yaw=match look {Direction::South=>0.0,Direction::West=>90.0,Direction::North=>180.0,Direction::East=>270.0,_=>-90.0};
        let ctx=PlaceContext {clicked_pos:BlockPos::new(pos.0,pos.1,pos.2),face:look.opposite(),
            hit:Vec3::new(pos.0 as f64+0.5,pos.1 as f64,pos.2 as f64+0.5),inside:false,
            yaw,pitch:if matches!(look,Direction::Up|Direction::Down) {90.0} else {0.0},sneaking:false,
            item:stack.item.to_string(),can_use_game_master_blocks:false};
        let pending=self.with_level(|b,l,t|b.begin_block_placement_from_plane(l,t,&ctx,look));
        let Some(pending)=pending else {self.ae2_item_update_blocked(plane,pos);return Ok(false)};
        let target=[pending.pos.x,pending.pos.y,pending.pos.z];
        if target!=[pos.0,pos.1,pos.2] {return Err("directional placement escaped its target".into());}
        let state=self.world.block(pos.0,pos.1,pos.2);
        let state=self.ae2_apply_item_block_state(target,state,stack);
        if let Some(entity)=BlockEntity::for_block(&self.res.facts().get(state).name) {
            self.world.block_entities.insert(pos,entity);
            self.apply_placed_item(target,stack,false);
            if let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get_mut(&pos) {
                // Platform.getFakePlayer uses this profile when the plane has
                // no owning player; setPlacedBy observes that fake player.
                let owner=player_owner.unwrap_or([0x60c173a5u32 as i32,0xe1e64b87u32 as i32,0x85b1272cu32 as i32,0xe424521du32 as i32]);
                machine.config.insert("ae2_node_owner_uuid".into(),Tag::IntArray(owner.to_vec()));
                self.world.modified.insert((pos.0>>4,pos.2>>4));
                if self.world.block_entities.refresh_pending_placed_ae2(pos,None)==Some(false) {
                    self.ae2_fault_placement_import(format!("placed AE2 entity {pos:?} escaped before owner assignment"));
                }
            }
            if let Some(error)=self.ae2_formation_import_error.take() {return Err(error);}
        }
        let outcome=self.with_level(|b,l,t|b.finish_block_placement(l,t,pending));
        self.ae2_item_update_blocked(plane,pos);
        Ok(matches!(outcome,PlaceOutcome::Placed {..}))
    }
    /// Replay only rollback-safe state. Item entities and external callbacks
    /// are published after the transaction is retained.
    fn ae2_replay_formation(&mut self,actions:&[FormationAction],committing:bool)->Result<(),String> {
        for (index,action) in actions.iter().enumerate() {
            match action {
                FormationAction::Reservation {..} if committing=>return Err("simulation reservation cannot commit".into()),
                FormationAction::Reservation {..}=>{},
                FormationAction::ContainerChanged {address,incarnation}=>{
                    let (dim,pos)=self.ae2_address_parts(*address).ok_or("invalid changed container address")?;
                    if self.ae2_world_for(dim).and_then(|world|world.block_entities.incarnation(&pos))!=Some(*incarnation) {
                        return Err("changed container owner replaced".into());
                    }
                },
                FormationAction::Place {dim,pos,old,stack,plane,player_owner}=>{
                    if *dim!=self.dim || self.world.block(pos.0,pos.1,pos.2)!=*old {return Err("formation target changed".into());}
                    if !self.ae2_try_formation_block(*pos,stack,*plane,*player_owner)? {return Err("formation item use no longer succeeds".into());}
                },
                FormationAction::Fluid {dim,pos,old,fluid,plane}=>{
                    if *dim!=self.dim || !self.ae2_formation_owner_valid(*plane) || !self.world.loaded(pos.0,pos.2) || self.world.block(pos.0,pos.1,pos.2)!=*old {return Err("fluid formation target changed".into());}
                    if self.ae2_fluid_cached_blocked(*plane,*fluid) {return Err("fluid formation became blocked".into());}
                    self.ae2_place_formation_fluid(*pos,*fluid,*plane)?;
                },
                FormationAction::Entity {dim,target,old,plane,..}=>{
                    if *dim!=self.dim || !self.ae2_formation_owner_valid(*plane) || !self.world.loaded(target.0,target.2)
                        || self.world.block(target.0,target.1,target.2)!=*old || !self.res.facts().get(*old).replaceable {return Err("entity formation target changed".into());}
                    let area=super::ae2_formation::area(*target);
                    let actual=self.ae2_count_entities_in_box(area)?;
                    let prior=actions[..index].iter().filter(|a|matches!(a,FormationAction::Entity {dim,position,..} if *dim==self.dim && super::ae2_formation::entity_intersects(*position,area))).count();
                    if super::ae2_formation::accepted(actual.saturating_add(prior),horizon_client::ae2_common_config::formation_entity_limit(),1,1)==0 {return Err("formation entity limit reached".into());}
                    self.ae2_item_update_blocked(*plane,*target);
                },
            }
        }
        Ok(())
    }
    pub(super) fn ae2_preview_formation_entity(&mut self,plan:&FormationPlan,target:&PlaneTarget)->Result<Option<(BlockStateId,usize)>,String> {
        self.ae2_with_placement_sandbox(|game| {
            game.ae2_replay_formation(&plan.actions,false)?;
            if !game.ae2_formation_owner_valid(target.owner) || !game.world.loaded(target.target.0,target.target.2) {return Err("entity formation owner or target unavailable".into());}
            let old=game.world.block(target.target.0,target.target.1,target.target.2);
            if game.ae2_item_cached_blocked(target.owner) || !game.res.facts().get(old).replaceable {return Ok(None);}
            let actual=game.ae2_count_entities_in_box(super::ae2_formation::area(target.target))?;
            Ok(Some((old,actual)))
        })
    }
    pub(super) fn ae2_preview_formation_block(&mut self,plan:&FormationPlan,target:&PlaneTarget,stack:&ItemStack)->Result<Option<BlockStateId>,String> {
        self.ae2_with_placement_sandbox(|game| {
            game.ae2_replay_formation(&plan.actions,false)?;
            let old=game.world.block(target.target.0,target.target.1,target.target.2);
            Ok(game.ae2_try_formation_block(target.target,stack,target.owner,target.player_owner)?.then_some(old))
        })
    }
    pub(super) fn ae2_preview_fluid_formation(&mut self,plan:&FormationPlan,target:&PlaneTarget,fluid:FluidKind)->Result<FluidPreview,String> {
        self.ae2_with_placement_sandbox(|game| {
            game.ae2_replay_formation(&plan.actions,false)?;
            if !game.ae2_formation_owner_valid(target.owner) {return Err("fluid plane was replaced".into());}
            if !game.world.loaded(target.target.0,target.target.2) {return Err("fluid formation target unloaded".into());}
            if game.ae2_fluid_cached_blocked(target.owner,fluid) {return Ok(FluidPreview::Blocked);}
            let old=game.world.block(target.target.0,target.target.1,target.target.2);
            Ok(if game.ae2_can_place_formation_fluid(target.target,fluid) {FluidPreview::CanPlace(old)} else {FluidPreview::CannotPlace})
        })
    }
    pub(super) fn ae2_execute_formation_plan(&mut self,plan:FormationPlan)->Result<(),String> {
        if plan.actions.is_empty() {return Ok(());}
        self.ae2_with_placement_transaction(|game|game.ae2_replay_formation(&plan.actions,true))?;
        // No failure below may restore inventory after physical placement.
        for action in plan.actions {
            match action {
                FormationAction::Place {pos,..}=>{
                    let state=self.world.block(pos.0,pos.1,pos.2);
                    self.ae2_publish_placed_energy(pos);
                    if matches!(self.res.facts().get(state).name.as_ref(),"ae2:quantum_ring"|"ae2:quantum_link") {self.ae2_quantum_block_added(pos);}
                },
                FormationAction::Entity {position,direction,stack,..}=>self.spawn_item(stack,position,direction.map(|v|v as f64*0.1),0),
                FormationAction::ContainerChanged {address,incarnation}=>{
                    if self.ae2_address_parts(address).and_then(|(dim,pos)|self.ae2_world_for(dim).and_then(|world|world.block_entities.incarnation(&pos)))==Some(incarnation) {self.ae2_native_notify(&[address]);}
                },
                FormationAction::Fluid {..}|FormationAction::Reservation {..}=>{},
            }
        }
        if let Err(error)=self.process_level_events() {log::warn!("AE2 committed placement event processing deferred: {error}");}
        Ok(())
    }
}
