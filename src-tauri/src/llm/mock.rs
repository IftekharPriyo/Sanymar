use async_trait::async_trait;

use super::{ScriptCandidate, ScriptGenerator, ScriptGeneratorError, ScriptRequest};

#[derive(Default)]
pub struct MockScriptGenerator;

#[async_trait]
impl ScriptGenerator for MockScriptGenerator {
    async fn generate(
        &self,
        request: ScriptRequest,
    ) -> Result<Vec<ScriptCandidate>, ScriptGeneratorError> {
        if request.cancellation.is_cancelled() {
            return Err(ScriptGeneratorError::Cancelled);
        }
        let next = request
            .next_track
            .as_ref()
            .map(|track| track.title.as_str())
            .unwrap_or("the next one");
        let dialogue = if let Some(fact) = request.facts.first() {
            format!(
                "That one knew exactly when to leave some air in the room. {} Next, {}.",
                fact.text, next
            )
        } else {
            format!("No lecture from the booth tonight—just make a little room for {next}.")
        };
        Ok(vec![ScriptCandidate {
            dialogue,
            fact_ids: request.facts.iter().map(|fact| fact.id.clone()).collect(),
        }])
    }
}
