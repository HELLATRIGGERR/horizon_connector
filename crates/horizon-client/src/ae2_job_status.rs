//! Owner-targeted AE2 crafting lifecycle messages.
//! A key is an existing canonical item SNBT key or the native
//! `fluid|<registry-id>|<component-hex>` key; no physical stack is sent.
use serde::{Deserialize, Serialize};

pub const CHANNEL: &str = "ae2:crafting_job_status";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum Status { Started, Finished, Cancelled }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CraftingJobStatus {
    pub job_id: [i64;2],
    pub key: String,
    pub requested: u64,
    pub remaining: u64,
    pub status: Status,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_payload_preserves_owner_job_identity_and_key() {
        let message=CraftingJobStatus{job_id:[i64::MIN,i64::MAX],key:"fluid|minecraft:water|00ff".into(),requested:2000,remaining:1000,status:Status::Started};
        let json=serde_json::to_vec(&message).unwrap();
        assert_eq!(serde_json::from_slice::<CraftingJobStatus>(&json).unwrap(),message);
        assert!(serde_json::from_str::<Status>("\"unknown\"").is_err());
    }
}
