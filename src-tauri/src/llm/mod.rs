use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::{
    music_facts::MusicFact,
    music_provider::Track,
    rj_engine::{BroadcastMemory, DjProfile, SegmentPlan, ValidationIssue},
};

#[derive(Clone, Debug)]
pub struct ScriptRequest {
    pub plan: SegmentPlan,
    pub profile: DjProfile,
    pub previous_track: Option<Track>,
    pub next_track: Option<Track>,
    pub facts: Vec<MusicFact>,
    pub memory: BroadcastMemory,
    pub maximum_words: u16,
    pub cancellation: CancellationToken,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScriptCandidate {
    pub dialogue: String,
    pub fact_ids: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ScriptGeneratorError {
    #[error("language model configuration is invalid")]
    InvalidConfiguration,
    #[error("language model provider is unavailable")]
    Unavailable,
    #[error("language model authentication failed")]
    Authentication,
    #[error("language model rate limit was reached")]
    RateLimited,
    #[error("language model provider rejected the request: {0}")]
    ProviderRejected(String),
    #[error("selected language model is unavailable")]
    ModelUnavailable,
    #[error("generation timed out")]
    Timeout,
    #[error("generation was cancelled")]
    Cancelled,
    #[error("generator returned malformed output")]
    MalformedOutput,
    #[error(transparent)]
    InvalidOutput(#[from] ScriptOutputError),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ScriptOutputError {
    #[error("generator did not return the required JSON object")]
    JsonContract,
    #[error("generator returned invalid verified-fact references")]
    FactReferences,
    #[error("generated dialogue failed validation: {issues:?}")]
    Dialogue { issues: Vec<ValidationIssue> },
}

#[async_trait]
pub trait ScriptGenerator: Send + Sync {
    async fn generate(
        &self,
        request: ScriptRequest,
    ) -> Result<Vec<ScriptCandidate>, ScriptGeneratorError>;
}

mod contract;
pub mod groq;
pub mod mock;
pub mod ollama;
mod prompt;
