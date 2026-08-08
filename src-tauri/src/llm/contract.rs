use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::rj_engine::{normalize_generated_dialogue, ScriptValidator, ValidationIssue};

use super::{ScriptGeneratorError, ScriptOutputError, ScriptRequest};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct StructuredOutput {
    pub dialogue: String,
    pub fact_ids: Vec<String>,
}

pub(super) fn output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "dialogue": { "type": "string" },
            "factIds": { "type": "array", "items": { "type": "string" } }
        },
        "required": ["dialogue", "factIds"],
        "additionalProperties": false
    })
}

pub(super) fn system_instruction(
    base: &str,
    schema: &Value,
    corrective_instruction: Option<&str>,
) -> Result<String, ScriptGeneratorError> {
    let schema =
        serde_json::to_string(schema).map_err(|_| ScriptGeneratorError::MalformedOutput)?;
    let corrective_instruction = corrective_instruction
        .map(|instruction| format!("\n{instruction}"))
        .unwrap_or_default();
    Ok(format!(
        "{base}\nRequired JSON schema: {schema}{corrective_instruction}"
    ))
}

pub(super) fn corrective_instruction(error: &ScriptOutputError, maximum_words: u16) -> String {
    let common = "This is a corrective retry. Return only the required JSON object, with no explanation or Markdown.";
    match error {
        ScriptOutputError::JsonContract => {
            format!("{common} The previous response did not match the required JSON schema.")
        }
        ScriptOutputError::FactReferences => format!(
            "{common} The previous response used invalid fact IDs. Use only IDs present in verifiedFacts, and include only IDs actually used in the dialogue."
        ),
        ScriptOutputError::Dialogue { issues }
            if issues.contains(&ValidationIssue::TooLong) =>
        {
            let retry_target = maximum_words.saturating_mul(3).saturating_div(4).max(1);
            format!(
                "{common} The previous dialogue exceeded the hard limit of {maximum_words} words. Write no more than {retry_target} words this time."
            )
        }
        ScriptOutputError::Dialogue { issues } => format!(
            "{common} The previous dialogue failed these local rules: {issues:?}. Avoid recent/disallowed phrases and follow the supplied dialogue constraints exactly."
        ),
    }
}

pub(super) fn parse_and_validate_output(
    request: &ScriptRequest,
    verified_fact_ids: &HashSet<String>,
    content: &str,
) -> Result<StructuredOutput, ScriptOutputError> {
    let mut output: StructuredOutput =
        serde_json::from_str(content).map_err(|_| ScriptOutputError::JsonContract)?;
    output.dialogue = normalize_generated_dialogue(&output.dialogue, &request.profile.name);
    validate_output(request, verified_fact_ids, &output)?;
    Ok(output)
}

fn validate_output(
    request: &ScriptRequest,
    verified_fact_ids: &HashSet<String>,
    output: &StructuredOutput,
) -> Result<(), ScriptOutputError> {
    let unique_fact_ids: HashSet<&str> = output.fact_ids.iter().map(String::as_str).collect();
    if unique_fact_ids.len() != output.fact_ids.len()
        || output
            .fact_ids
            .iter()
            .any(|fact_id| !verified_fact_ids.contains(fact_id))
    {
        return Err(ScriptOutputError::FactReferences);
    }
    let report = ScriptValidator::validate(
        &output.dialogue,
        request.maximum_words.into(),
        request.plan.segment_type,
        output.fact_ids.len(),
        request.previous_track.as_ref(),
        &request.profile,
        &request.memory.recent_openings,
    );
    if report.valid {
        Ok(())
    } else {
        Err(ScriptOutputError::Dialogue {
            issues: report.issues,
        })
    }
}
