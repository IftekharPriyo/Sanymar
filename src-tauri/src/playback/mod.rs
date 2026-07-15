use serde::{Deserialize, Serialize};

/// Identity attached to every prepared commentary artifact.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommentaryJob {
    pub job_id: String,
    pub current_track_id: String,
    pub next_track_id: Option<String>,
}
