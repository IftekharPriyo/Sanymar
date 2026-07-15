mod content_director;
mod coordinator;
mod personality;
mod spoken_text;
mod state;
mod validator;

pub use content_director::{ContentDirector, SegmentFocus, SegmentPlan, SegmentType};
pub use coordinator::BroadcastCoordinator;
pub use personality::DjProfile;
pub use spoken_text::{normalize_for_speech, normalize_generated_dialogue};
pub use state::BroadcastState;
pub use validator::{ScriptValidator, ValidationIssue, ValidationReport};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BroadcastMemory {
    pub recent_segment_types: Vec<SegmentType>,
    pub recent_fact_ids: Vec<String>,
    pub recent_openings: Vec<String>,
    pub consecutive_with_commentary: u8,
    pub consecutive_without_commentary: u8,
}
