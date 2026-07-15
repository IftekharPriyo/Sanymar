use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum BroadcastState {
    Idle,
    Monitoring,
    FetchingFacts,
    SelectingSegment,
    GeneratingScript,
    ValidatingScript,
    SynthesizingSpeech,
    WaitingForTransition,
    PausingMusic,
    Speaking,
    ResumingMusic,
    Cancelled,
    Failed(String),
}

impl BroadcastState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Monitoring => "Monitoring",
            Self::FetchingFacts => "Fetching facts",
            Self::SelectingSegment => "Selecting segment",
            Self::GeneratingScript => "Generating script",
            Self::ValidatingScript => "Validating script",
            Self::SynthesizingSpeech => "Synthesizing speech",
            Self::WaitingForTransition => "Waiting for transition",
            Self::PausingMusic => "Pausing music",
            Self::Speaking => "Speaking",
            Self::ResumingMusic => "Resuming music",
            Self::Cancelled => "Cancelled",
            Self::Failed(_) => "Failed",
        }
    }
}
