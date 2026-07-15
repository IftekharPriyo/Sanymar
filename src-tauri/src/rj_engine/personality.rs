use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DjProfile {
    pub id: String,
    pub name: String,
    pub station_name: String,
    pub personality_traits: Vec<String>,
    pub energy_level: u8,
    pub humour_style: String,
    pub formality: u8,
    pub preferred_language: String,
    pub bangla_english_mix: u8,
    pub average_words: u16,
    pub minimum_words: u16,
    pub maximum_words: u16,
    pub talk_frequency: f32,
    pub restricted_subjects: Vec<String>,
    pub disallowed_phrases: Vec<String>,
    pub station_lore: Vec<String>,
    pub running_jokes: Vec<String>,
    pub addresses_listener: bool,
    pub reacts_to_time_of_day: bool,
    pub mild_sarcasm: bool,
}

impl Default for DjProfile {
    fn default() -> Self {
        Self {
            id: "mira-vale".into(),
            name: "Mira Vale".into(),
            station_name: "Night Current".into(),
            personality_traits: vec!["curious".into(), "warm".into(), "observant".into()],
            energy_level: 4,
            humour_style: "lightly dry, never cruel".into(),
            formality: 2,
            preferred_language: "English".into(),
            bangla_english_mix: 1,
            average_words: 26,
            minimum_words: 8,
            maximum_words: 42,
            talk_frequency: 0.55,
            restricted_subjects: vec!["private listener assumptions".into()],
            disallowed_phrases: vec![
                "Did you know".into(),
                "without further ado".into(),
                "coming up next".into(),
            ],
            station_lore: vec![
                "The Night Current studio sits above a tea shop that never closes.".into(),
            ],
            running_jokes: Vec::new(),
            addresses_listener: true,
            reacts_to_time_of_day: true,
            mild_sarcasm: true,
        }
    }
}
