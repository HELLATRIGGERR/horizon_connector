//! Scoped synchronous chunk reads through Horizon's ordinary receive path.
//! Temporary demands are independent of manual and Spatial Anchor tickets.
use super::Game;
use std::time::{Duration,Instant};

impl Game<'_> {
    pub(super) fn ae2_with_demand_columns<R>(&mut self,action:impl FnOnce(&mut Self)->Result<R,String>)->Result<R,String>{
        if self.remote.is_some(){return Err("authoritative chunk demand requires the server".into())}
        let previous=self.streaming.extra.clone();
        let result=action(self);
        // Keep every reached column until this synchronous operation finishes;
        // Java cannot unload one of its earlier FIFO reads on a later read.
        self.streaming.extra=previous;
        result
    }
    pub(super) fn ae2_demand_column(&mut self,cx:i32,cz:i32)->Result<bool,String>{
        if self.world.chunks.contains_key(&(cx,cz)){
            if !self.streaming.extra.contains(&((cx,cz),0)){self.streaming.extra.push(((cx,cz),0));}
            return Ok(false)
        }
        if self.remote.is_some(){return Err("authoritative chunk demand requires the server".into())}
        if self.streaming.paused{return Err("chunk streaming is paused".into())}
        // Corruption must never become an absent chunk that gets regenerated.
        // Pending save records are included by these checked readers. Normal
        // worker installation still owns names, BEs, saved entities and light.
        self.save.load_checked(cx,cz,self.pack.assets.registry,self.res.biomes())
            .map_err(|e|format!("cannot read demanded chunk {cx} {cz}: {e}"))?;
        self.save.load_entities_checked(cx,cz)
            .map_err(|e|format!("cannot read demanded entities {cx} {cz}: {e}"))?;
        if !self.streaming.extra.contains(&((cx,cz),0)){self.streaming.extra.push(((cx,cz),0));}
        let deadline=Instant::now()+Duration::from_secs(15);
        loop {
            self.update_world_receive(false);
            if self.world.chunks.contains_key(&(cx,cz)){
                log::info!("AE2 demand loaded column {cx} {cz}");return Ok(true)
            }
            if Instant::now()>=deadline{return Err(format!("timed out loading demanded chunk {cx} {cz}"))}
            self.streaming.wait(Duration::from_millis(2));
        }
    }
    /// Server-side item-use fixture seam. The ordinary interaction handler
    /// reaches the same Eraser implementation, but render-distance buffering
    /// normally makes every position within its 49-block walk resident.
    pub(super) fn ae2_eraser_use_command(&mut self,words:&[&str])->Result<serde_json::Value,String>{
        if words.len()!=3{return Err("usage: /ae2 eraser-use <x> <y> <z>".into())}
        let mut pos=[0;3];for (value,word) in pos.iter_mut().zip(words){*value=word.parse::<i32>().map_err(|_|"invalid Eraser position")?;}
        if pos[0].unsigned_abs()>30_000_000||pos[2].unsigned_abs()>30_000_000||!(-64..320).contains(&pos[1]){return Err("Eraser position outside world bounds".into())}
        let loaded=self.world.chunks.contains_key(&(pos[0]>>4,pos[2]>>4));
        let time=self.world.game_time;let center=self.streaming.center;let extra=self.streaming.extra.clone();
        if !self.ae2_try_eraser(pos){return Err("hold the actual Eraser item".into())}
        let state=self.world.block(pos[0],pos[1],pos[2]);
        Ok(serde_json::json!({"position":pos,"initially_loaded":loaded,"block_after":self.res.facts().get(state).name.as_ref(),
            "game_time_advanced":self.world.game_time-time,"center_unchanged":self.streaming.center==center,
            "temporary_tickets_restored":self.streaming.extra==extra}))
    }
    /// Opt-in developer control for exercising the exact authoritative seam;
    /// normal gameplay reaches it from the Eraser's FIFO, not a box prefetch.
    pub(super) fn ae2_demand_column_command(&mut self,words:&[&str])->Result<serde_json::Value,String>{
        if words.len()!=2{return Err("usage: /ae2 demand-column <chunk x> <chunk z>".into())}
        let cx=words[0].parse::<i32>().map_err(|_|"invalid chunk x")?;
        let cz=words[1].parse::<i32>().map_err(|_|"invalid chunk z")?;
        if !(-1_875_000..=1_875_000).contains(&cx)||!(-1_875_000..=1_875_000).contains(&cz){return Err("chunk outside world bounds".into())}
        let game_time=self.world.game_time;let center=self.streaming.center;
        let extra=self.streaming.extra.clone();
        let (demanded,block_entities,names)=self.ae2_with_demand_columns(|game|{
            let demanded=game.ae2_demand_column(cx,cz)?;
            let entities=game.world.block_entities.keys().filter(|p|p.0>>4==cx&&p.2>>4==cz).count();
            let names=game.world.block_names.keys().filter(|p|p.0>>4==cx&&p.2>>4==cz).count();
            Ok((demanded,entities,names))
        })?;
        Ok(serde_json::json!({"column":[cx,cz],"demanded":demanded,"block_entities":block_entities,"names":names,
            "game_time_advanced":self.world.game_time-game_time,"center_unchanged":self.streaming.center==center,
            "temporary_tickets_restored":self.streaming.extra==extra}))
    }
}
