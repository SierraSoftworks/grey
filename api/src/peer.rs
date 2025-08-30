use serde::{Deserialize, Serialize};

/// Information about cluster peers as returned by the /api/v1/cluster/peers endpoint
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct Peer {
    pub id: String,

    #[serde(with = "chrono::serde::ts_seconds")]
    pub last_seen: chrono::DateTime<chrono::Utc>,
}