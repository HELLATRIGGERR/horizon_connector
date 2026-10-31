//! Owner-targeted crafting job lifecycle events, emitted after host commits.
use super::{Game,lan::LanHost};
use horizon_client::ae2_job_status::{CraftingJobStatus,CHANNEL};
use horizon_protocol::packets::{configuration::CustomPayload,play::Clientbound};

pub(super) fn owner_words(uuid:u128)->[i64;2] {[(uuid >> 64) as u64 as i64,uuid as u64 as i64]}
pub(super) fn owner_uuid(words:[i64;2])->u128 {u128::from(words[0] as u64)<<64 | u128::from(words[1] as u64)}

impl Game<'_> {
    pub(super) fn ae2_local_player_uuid(&self)->u128 {
        self.level.player_uuid.map_or_else(
            ||horizon_protocol::crypto::offline_uuid(&self.options.player_name()),
            |words|words.into_iter().fold(0u128,|uuid,word|(uuid<<32)|u128::from(word as u32)),
        )
    }
    pub(super) fn ae2_emit_job_status(&mut self,owner:u128,event:CraftingJobStatus) {
        if !self.serving() && self.remote.is_none() && self.ae2_local_player_uuid()==owner {
            if let Ok(data)=serde_json::to_vec(&event) {self.ae2_accept_job_status(&data);}
        } else if self.serving() || self.lan.is_some() || self.ae2_lan_active {
            self.ae2_job_status_pending.push((owner,event));
        }
    }
    pub(super) fn ae2_flush_job_status(&mut self,host:&LanHost) {
        for (owner,event) in self.ae2_job_status_pending.drain(..) {
            let Some(guest)=host.hub.guests.iter().find(|guest|guest.uuid==owner && guest.placed) else {continue;};
            let Ok(data)=serde_json::to_vec(&event) else {continue;};
            guest.send(&Clientbound::CustomPayload(CustomPayload{channel:CHANNEL.into(),data}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn player_identity_preserves_all_uuid_bits() {
        for uuid in [0,1,u128::MAX,0x0123_4567_89ab_cdef_fedc_ba98_7654_3210] {
            assert_eq!(owner_uuid(owner_words(uuid)),uuid);
        }
    }
}
