use serde::{Deserialize, Serialize};

use crate::music_provider::Track;

use super::{DjProfile, SegmentType};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ValidationIssue {
    Empty,
    TooLong,
    RepeatedPhrase,
    ExcessiveSongTitle,
    DisallowedPhrase,
    UnexpectedFormatting,
    FactualSegmentWithoutFacts,
    ModelExplanation,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ValidationReport {
    pub valid: bool,
    pub word_count: usize,
    pub issues: Vec<ValidationIssue>,
}

pub struct ScriptValidator;

impl ScriptValidator {
    pub fn validate(
        dialogue: &str,
        maximum_words: usize,
        segment_type: SegmentType,
        supplied_fact_count: usize,
        track: Option<&Track>,
        profile: &DjProfile,
        recent_openings: &[String],
    ) -> ValidationReport {
        let trimmed = dialogue.trim();
        let word_count = trimmed.split_whitespace().count();
        let lower = trimmed.to_ascii_lowercase();
        let mut issues = Vec::new();
        if trimmed.is_empty() {
            issues.push(ValidationIssue::Empty);
        }
        if word_count > maximum_words {
            issues.push(ValidationIssue::TooLong);
        }
        if recent_openings
            .iter()
            .any(|opening| lower.starts_with(&opening.to_ascii_lowercase()))
        {
            issues.push(ValidationIssue::RepeatedPhrase);
        }
        if profile
            .disallowed_phrases
            .iter()
            .any(|phrase| lower.contains(&phrase.to_ascii_lowercase()))
        {
            issues.push(ValidationIssue::DisallowedPhrase);
        }
        let speaker_label = format!("{}:", profile.name.trim().to_ascii_lowercase());
        if trimmed.contains("```")
            || trimmed.contains("**")
            || trimmed.lines().count() > 3
            || lower.starts_with(&speaker_label)
            || contains_page_artifacts(trimmed)
        {
            issues.push(ValidationIssue::UnexpectedFormatting);
        }
        if lower.starts_with("here is")
            || lower.starts_with("as an ai")
            || lower.contains("script:")
        {
            issues.push(ValidationIssue::ModelExplanation);
        }
        if matches!(
            segment_type,
            SegmentType::FunFact
                | SegmentType::RecordingStory
                | SegmentType::ArtistStory
                | SegmentType::CulturalContext
                | SegmentType::MusicHistoryConnection
        ) && supplied_fact_count == 0
        {
            issues.push(ValidationIssue::FactualSegmentWithoutFacts);
        }
        if let Some(track) = track {
            let title = track.title.to_ascii_lowercase();
            if !title.is_empty() && lower.matches(&title).count() > 2 {
                issues.push(ValidationIssue::ExcessiveSongTitle);
            }
        }
        ValidationReport {
            valid: issues.is_empty(),
            word_count,
            issues,
        }
    }
}

fn contains_page_artifacts(dialogue: &str) -> bool {
    dialogue.chars().any(|character| {
        !(character.is_alphanumeric()
            || character.is_whitespace()
            || matches!(
                character,
                '.' | ','
                    | '!'
                    | '?'
                    | ';'
                    | ':'
                    | '\''
                    | '’'
                    | '-'
                    | '—'
                    | '–'
                    | '&'
                    | '%'
                    | '/'
                    | '+'
            ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_script() {
        let report = ScriptValidator::validate(
            "  ",
            40,
            SegmentType::SimpleTransition,
            0,
            None,
            &DjProfile::default(),
            &[],
        );
        assert!(report.issues.contains(&ValidationIssue::Empty));
    }

    #[test]
    fn rejects_excessively_long_script() {
        let dialogue = vec!["word"; 43].join(" ");
        let report = ScriptValidator::validate(
            &dialogue,
            42,
            SegmentType::SimpleTransition,
            0,
            None,
            &DjProfile::default(),
            &[],
        );
        assert!(report.issues.contains(&ValidationIssue::TooLong));
    }

    #[test]
    fn rejects_page_formatting_and_speaker_labels() {
        for dialogue in [
            "Mira Vale: Keep it moving.",
            "Now playing ‘Lights’. 🎧",
            "[energetic] Keep it moving.",
        ] {
            let report = ScriptValidator::validate(
                dialogue,
                42,
                SegmentType::SimpleTransition,
                0,
                None,
                &DjProfile::default(),
                &[],
            );
            assert!(report
                .issues
                .contains(&ValidationIssue::UnexpectedFormatting));
        }
    }
}
