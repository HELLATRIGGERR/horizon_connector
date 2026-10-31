//! AE2 19.2.18 EraserItem.onItemUseFirst, LGPL-3.0-or-later.
//! Source: appeng/debug/EraserItem.java. The operator is deliberately
//! destructive: matched blocks are replaced with air without block loot.
use std::collections::{HashSet,VecDeque};
use super::Game;
const BOX:i64=48;
const LIMIT:usize=48*48*48;

/// Source queue semantics include the one-block shell outside the expansion
/// box. Enqueued duplicates are not skipped at dequeue, matching Java.
struct Walk {pending:VecDeque<[i32;3]>,closed:HashSet<[i32;3]>,origin:[i32;3],count:usize}
impl Walk {
    fn new(origin:[i32;3])->Self{Self{pending:VecDeque::from([origin]),closed:HashSet::new(),origin,count:0}}
    fn advance<E>(&mut self,mut erase:impl FnMut([i32;3])->Result<bool,E>)->Result<usize,E>{
        while self.count<LIMIT {
            let Some(pos)=self.pending.pop_front() else{break};
            // A missing read is still the first queued read after loading.
            // Neither the closed set nor count changes until it succeeds.
            let matched=match erase(pos){Ok(value)=>value,Err(error)=>{self.pending.push_front(pos);return Err(error)}};
            self.closed.insert(pos);if !matched{continue}self.count+=1;
            if (0..3).all(|i|(i64::from(pos[i])-i64::from(self.origin[i])).abs()<=BOX){
                for x in -1i32..=1{for y in -1i32..=1{for z in -1i32..=1{
                    if x==0&&y==0&&z==0{continue}
                    let next=[pos[0].wrapping_add(x),pos[1].wrapping_add(y),pos[2].wrapping_add(z)];
                    if !self.closed.contains(&next){self.pending.push_back(next)}
                }}}
            }
        }
        Ok(self.count)
    }
}
#[cfg(test)]
fn erase_walk(mut erase:impl FnMut([i32;3])->bool,origin:[i32;3])->usize{
    Walk::new(origin).advance::<std::convert::Infallible>(|p|Ok(erase(p))).unwrap()
}

impl Game<'_> {
    pub(super) fn ae2_try_eraser(&mut self,pos:[i32;3])->bool {
        use horizon_core::position::BlockPos;
        use horizon_world::level::{LevelAccess,flags};
        if !self.held().is_some_and(|s|s.item.as_ref()=="ae2:debug_eraser"&&s.count>0){return false}
        let Some(air)=self.pack.assets.registry.default_state("minecraft:air") else{return false};
        let registry=self.pack.assets.registry;
        let alternate=self.keys.contains(&super::KeyCode::ShiftLeft);
        let result=self.ae2_with_demand_columns(|game|{
            game.ae2_demand_column(pos[0]>>4,pos[2]>>4)?;
            let initial=game.with_level(|b,level,_|b.block(level.block_state(BlockPos::new(pos[0],pos[1],pos[2]))));
            let mut walk=Walk::new(pos);let mut visited_columns=HashSet::new();
            loop {
                let result=game.with_level(|b,level,ticks|walk.advance(|p|{
                    let at=BlockPos::new(p[0],p[1],p[2]);
                    if p[1]<level.min_y()||p[1]>=level.max_y(){return Ok(false)}
                    if !level.is_loaded(at){return Err((p[0]>>4,p[2]>>4))}
                    visited_columns.insert((p[0]>>4,p[2]>>4));
                    let old=level.block_state(at);let block=b.block(old);
                    let name=registry.state(old).map(|s|s.name.as_str()).unwrap_or("");
                    let common=matches!(name,"minecraft:stone"|"minecraft:dirt"|"minecraft:grass_block"|"minecraft:cobblestone"|"minecraft:andesite"|"minecraft:granite"|"minecraft:diorite"|"minecraft:gravel"|"minecraft:sandstone"|"minecraft:netherrack"|"minecraft:water"|"minecraft:lava")
                        ||["minecraft:leaves","minecraft:sand","minecraft:logs"].iter().any(|tag|b.tag(tag).is_some_and(|set|set.contains(block)));
                    if block!=initial&&!(alternate&&common){return Ok(false)}
                    // Source setBlock(AIR,2) onRemove preserves real contents;
                    // the following destroyBlock(AIR) supplies no block loot.
                    b.set_block(level,ticks,at,air,flags::UPDATE_CLIENTS);Ok(true)
                }));
                match result{Ok(count)=>return Ok(count),Err((cx,cz))=>{
                    // Retain already-resident FIFO reads too, before pumping
                    // streaming for the first missing one. No reached column
                    // may unload halfway through this synchronous item use.
                    for (x,z) in visited_columns.drain(){game.ae2_demand_column(x,z)?;}
                    game.ae2_demand_column(cx,cz)?;
                }}
            }
        });
        let _=self.process_level_events();match result{Ok(erased)=>log::info!("AE2 Eraser deleted {erased} blocks"),Err(error)=>{
            log::warn!("AE2 Eraser stopped after a chunk read failed: {error}");
            self.system_message(horizon_client::text::Text::literal(format!("Eraser stopped: {error}")));
        }}true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_fifo_read_resumes_before_any_later_position(){
        let mut walk=Walk::new([0,0,0]);let mut first=Vec::new();
        assert_eq!(walk.advance(|p|{first.push(p);if p==[-1,-1,0]{Err("missing")}else{Ok(p==[0,0,0])}}),Err("missing"));
        assert_eq!(first,vec![[0,0,0],[-1,-1,-1],[-1,-1,0]]);assert_eq!(walk.count,1);
        let mut after=Vec::new();assert_eq!(walk.advance::<std::convert::Infallible>(|p|{after.push(p);Ok(false)}).unwrap(),1);
        assert_eq!(after[0],[-1,-1,0]);assert_eq!(after[1],[-1,-1,1]);
    }
    #[test]
    fn source_walk_uses_diagonals_one_block_shell_and_the_global_cap() {
        let mut blocks=HashSet::from([[0,0,0],[1,1,1],[2,1,1]]);
        assert_eq!(erase_walk(|p|blocks.remove(&p),[0,0,0]),3);assert!(blocks.is_empty());
        let mut line:HashSet<_>=(0..=50).map(|x|[x,0,0]).collect();
        assert_eq!(erase_walk(|p|line.remove(&p),[0,0,0]),50);assert_eq!(line,HashSet::from([[50,0,0]]));
        assert_eq!(erase_walk(|_|true,[0,0,0]),LIMIT);
    }
}
