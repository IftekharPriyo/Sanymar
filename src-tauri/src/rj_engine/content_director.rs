use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::{music_facts::MusicFact, settings::TalkFrequency};

use super::BroadcastMemory;

const SHORT_JOKE_PERCENT: u32 = 8;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SegmentType {
    Silence,
    OneLineReaction,
    NextSongTease,
    FunFact,
    RecordingStory,
    ArtistStory,
    SongInterpretation,
    CulturalContext,
    MusicHistoryConnection,
    ShortJoke,
    StationIdentification,
    StationLore,
    ListenerObservation,
    SimpleTransition,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SegmentFocus {
    PreviousTrack,
    NextTrack,
    Station,
}

#[derive(Clone, Debug)]
pub struct SegmentPlan {
    pub segment_type: SegmentType,
    pub focus: SegmentFocus,
    pub target_words: u16,
    pub fact_ids: Vec<String>,
    pub use_station_lore: bool,
}

pub struct ContentDirector<R> {
    rng: R,
}

impl<R: RngCore> ContentDirector<R> {
    pub fn new(rng: R) -> Self {
        Self { rng }
    }

    pub fn select(
        &mut self,
        mode: TalkFrequency,
        facts: &[MusicFact],
        memory: &BroadcastMemory,
    ) -> SegmentPlan {
        let talk_threshold = match mode {
            TalkFrequency::Minimal => 25,
            TalkFrequency::Normal => 55,
            TalkFrequency::Talkative => 78,
        };
        let roll = self.rng.next_u32() % 100;
        let should_speak = memory.consecutive_without_commentary >= 4
            || (memory.consecutive_with_commentary < 2 && roll < talk_threshold);
        if !should_speak {
            return silence();
        }

        let humour_roll = self.rng.next_u32();
        let short_joke_is_eligible = short_joke_is_eligible(humour_roll, memory);

        let has_unused_fact = facts
            .iter()
            .any(|fact| !memory.recent_fact_ids.contains(&fact.id));
        let candidates = if has_unused_fact {
            vec![
                SegmentType::FunFact,
                SegmentType::OneLineReaction,
                SegmentType::NextSongTease,
                SegmentType::SimpleTransition,
                SegmentType::StationIdentification,
            ]
        } else {
            vec![
                SegmentType::OneLineReaction,
                SegmentType::NextSongTease,
                SegmentType::SimpleTransition,
                SegmentType::StationIdentification,
            ]
        };
        let filtered: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|candidate| !memory.recent_segment_types.contains(candidate))
            .collect();
        let pool = if filtered.is_empty() {
            &candidates
        } else {
            &filtered
        };
        let segment_type = if short_joke_is_eligible {
            SegmentType::ShortJoke
        } else {
            let index = (self.rng.next_u32() as usize) % pool.len();
            pool[index]
        };
        let fact_ids = if segment_type == SegmentType::FunFact {
            facts
                .iter()
                .find(|fact| !memory.recent_fact_ids.contains(&fact.id))
                .map(|fact| vec![fact.id.clone()])
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        SegmentPlan {
            segment_type,
            focus: match segment_type {
                SegmentType::NextSongTease => SegmentFocus::NextTrack,
                SegmentType::StationIdentification | SegmentType::StationLore => {
                    SegmentFocus::Station
                }
                _ => SegmentFocus::PreviousTrack,
            },
            target_words: match mode {
                TalkFrequency::Minimal => 16,
                TalkFrequency::Normal => 26,
                TalkFrequency::Talkative => 36,
            },
            fact_ids,
            use_station_lore: segment_type == SegmentType::StationLore,
        }
    }
}

fn silence() -> SegmentPlan {
    SegmentPlan {
        segment_type: SegmentType::Silence,
        focus: SegmentFocus::Station,
        target_words: 0,
        fact_ids: Vec::new(),
        use_station_lore: false,
    }
}

fn short_joke_is_eligible(roll: u32, memory: &BroadcastMemory) -> bool {
    roll % 100 < SHORT_JOKE_PERCENT
        && !memory
            .recent_segment_types
            .contains(&SegmentType::ShortJoke)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    use super::*;
    use crate::music_facts::{FactCategory, MusicFact};

    fn fact(id: &str) -> MusicFact {
        MusicFact {
            id: id.into(),
            text: "A reviewed development fact".into(),
            category: FactCategory::RecordingStory,
            source_name: "fixture".into(),
            source_url: None,
            confidence: 1.0,
            human_reviewed: true,
            verification_method: crate::music_facts::VerificationMethod::HumanReviewed,
            created_at: Utc::now(),
            last_verified_at: Some(Utc::now()),
            track_id: None,
            album_id: None,
            artist_id: None,
        }
    }

    #[test]
    fn fixed_seed_is_deterministic() {
        let mut first = ContentDirector::new(ChaCha8Rng::seed_from_u64(42));
        let mut second = ContentDirector::new(ChaCha8Rng::seed_from_u64(42));
        let memory = BroadcastMemory::default();
        let facts = vec![fact("one")];
        assert_eq!(
            first
                .select(TalkFrequency::Normal, &facts, &memory)
                .segment_type,
            second
                .select(TalkFrequency::Normal, &facts, &memory)
                .segment_type
        );
    }

    #[test]
    fn minimal_mode_can_select_silence() {
        let seed = (0_u64..1000)
            .find(|seed| {
                let mut director = ContentDirector::new(ChaCha8Rng::seed_from_u64(*seed));
                director
                    .select(TalkFrequency::Minimal, &[], &BroadcastMemory::default())
                    .segment_type
                    == SegmentType::Silence
            })
            .expect("test seed range must include a silence result");
        let mut director = ContentDirector::new(ChaCha8Rng::seed_from_u64(seed));
        assert_eq!(
            director
                .select(TalkFrequency::Minimal, &[], &BroadcastMemory::default())
                .segment_type,
            SegmentType::Silence
        );
    }

    #[test]
    fn avoids_recent_segment_and_used_fact() {
        let mut director = ContentDirector::new(ChaCha8Rng::seed_from_u64(9));
        let memory = BroadcastMemory {
            recent_segment_types: vec![SegmentType::OneLineReaction],
            recent_fact_ids: vec!["old".into()],
            consecutive_without_commentary: 5,
            ..BroadcastMemory::default()
        };
        let plan = director.select(
            TalkFrequency::Normal,
            &[fact("old"), fact("fresh")],
            &memory,
        );
        assert_ne!(plan.segment_type, SegmentType::OneLineReaction);
        assert!(!plan.fact_ids.contains(&"old".to_owned()));
    }

    #[test]
    fn no_facts_never_selects_factual_segment() {
        let mut director = ContentDirector::new(ChaCha8Rng::seed_from_u64(2));
        let memory = BroadcastMemory {
            consecutive_without_commentary: 5,
            ..BroadcastMemory::default()
        };
        assert_ne!(
            director
                .select(TalkFrequency::Talkative, &[], &memory)
                .segment_type,
            SegmentType::FunFact
        );
    }

    #[test]
    fn short_jokes_are_infrequent_and_respect_recent_memory() {
        let memory = BroadcastMemory::default();
        let eligible_rolls = (0..100)
            .filter(|roll| short_joke_is_eligible(*roll, &memory))
            .count();
        assert_eq!(eligible_rolls, SHORT_JOKE_PERCENT as usize);

        let recent_joke = BroadcastMemory {
            recent_segment_types: vec![SegmentType::ShortJoke],
            ..BroadcastMemory::default()
        };
        for roll in 0..100 {
            assert!(!short_joke_is_eligible(roll, &recent_joke));
        }
    }
}
