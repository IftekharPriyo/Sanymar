use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};

use crate::{
    errors::AppError, music_facts::MusicFact, music_provider::Track, rj_engine::DjProfile,
    settings::AppSettings,
};

#[derive(Clone)]
pub struct SanymarRepository {
    pool: SqlitePool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedScriptRecord {
    pub id: String,
    pub job_id: String,
    pub track_provider_id: String,
    pub dialogue: String,
    pub segment_type: String,
    pub fact_ids_json: String,
    pub validation_status: String,
    pub validation_issues_json: String,
}

impl SanymarRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn save_settings(&self, settings: &AppSettings) -> Result<(), AppError> {
        settings.validate()?;
        let json = serde_json::to_string(settings)
            .map_err(|error| AppError::Database(error.to_string()))?;
        sqlx::query(
            "INSERT INTO application_settings (key, value_json, updated_at) VALUES (?, ?, ?) \
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at",
        )
        .bind("app")
        .bind(json)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn load_settings(&self) -> Result<Option<AppSettings>, AppError> {
        let row = sqlx::query("SELECT value_json FROM application_settings WHERE key = ?")
            .bind("app")
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| {
            serde_json::from_str(row.get::<&str, _>("value_json"))
                .map_err(|error| AppError::Database(error.to_string()))
        })
        .transpose()
    }

    pub async fn save_profile(&self, profile: &DjProfile) -> Result<(), AppError> {
        let json = serde_json::to_string(profile)
            .map_err(|error| AppError::Database(error.to_string()))?;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO dj_profiles (id, name, station_name, profile_json, is_active, created_at, updated_at) \
             VALUES (?, ?, ?, ?, 1, ?, ?) ON CONFLICT(id) DO UPDATE SET profile_json = excluded.profile_json, \
             name = excluded.name, station_name = excluded.station_name, updated_at = excluded.updated_at",
        )
        .bind(&profile.id)
        .bind(&profile.name)
        .bind(&profile.station_name)
        .bind(json)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn save_generated_script(
        &self,
        script: &GeneratedScriptRecord,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO generated_scripts \
             (id, job_id, track_provider_id, dialogue, segment_type, fact_ids_json, validation_status, validation_issues_json, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&script.id)
        .bind(&script.job_id)
        .bind(&script.track_provider_id)
        .bind(&script.dialogue)
        .bind(&script.segment_type)
        .bind(&script.fact_ids_json)
        .bind(&script.validation_status)
        .bind(&script.validation_issues_json)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn load_cached_facts(
        &self,
        provider: &str,
        track_provider_id: &str,
        fresh_after: DateTime<Utc>,
    ) -> Result<Option<Vec<MusicFact>>, AppError> {
        let cache = sqlx::query(
            "SELECT outcome, checked_at FROM fact_lookup_cache WHERE provider = ? AND track_provider_id = ?",
        )
        .bind(provider)
        .bind(track_provider_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(cache) = cache else {
            return Ok(None);
        };
        let checked_at = DateTime::parse_from_rfc3339(cache.get("checked_at"))
            .map_err(|error| AppError::Database(error.to_string()))?
            .with_timezone(&Utc);
        if checked_at < fresh_after {
            return Ok(None);
        }
        if cache.get::<&str, _>("outcome") == "no_match" {
            return Ok(Some(Vec::new()));
        }
        let rows = sqlx::query(
            "SELECT f.id, f.text, f.category, f.source_url, f.confidence, f.human_reviewed, \
                    f.verification_method, f.created_at, f.last_verified_at, s.name AS source_name \
             FROM tracks t \
             JOIN track_facts tf ON tf.track_id = t.id \
             JOIN music_facts f ON f.id = tf.fact_id \
             JOIN fact_sources s ON s.id = f.source_id \
             WHERE t.provider = ? AND t.provider_id = ?",
        )
        .bind(provider)
        .bind(track_provider_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(MusicFact {
                    id: row.get("id"),
                    text: row.get("text"),
                    category: parse_enum(row.get("category"))?,
                    source_name: row.get("source_name"),
                    source_url: row.get("source_url"),
                    confidence: row.get("confidence"),
                    human_reviewed: row.get::<i64, _>("human_reviewed") != 0,
                    verification_method: parse_enum(row.get("verification_method"))?,
                    created_at: parse_timestamp(row.get("created_at"))?,
                    last_verified_at: row
                        .get::<Option<&str>, _>("last_verified_at")
                        .map(parse_timestamp)
                        .transpose()?,
                    track_id: Some(track_provider_id.to_owned()),
                    album_id: None,
                    artist_id: None,
                })
            })
            .collect::<Result<Vec<_>, AppError>>()
            .map(Some)
    }

    pub async fn save_fact_lookup(
        &self,
        provider: &str,
        track: &Track,
        facts: &[MusicFact],
    ) -> Result<(), AppError> {
        let mut transaction = self.pool.begin().await?;
        let now = Utc::now().to_rfc3339();
        let duration_ms = i64::try_from(track.duration_ms)
            .map_err(|error| AppError::Database(error.to_string()))?;
        let variant = enum_value(&track.variant)?;
        sqlx::query(
            "INSERT INTO tracks (provider, provider_id, title, duration_ms, isrc, release_date, explicit, variant, artwork_url, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(provider, provider_id) DO UPDATE SET title = excluded.title, duration_ms = excluded.duration_ms, \
             isrc = excluded.isrc, release_date = excluded.release_date, explicit = excluded.explicit, \
             variant = excluded.variant, artwork_url = excluded.artwork_url, updated_at = excluded.updated_at",
        )
        .bind(provider)
        .bind(&track.provider_id)
        .bind(&track.title)
        .bind(duration_ms)
        .bind(&track.isrc)
        .bind(track.release_date.map(|date| date.to_string()))
        .bind(i64::from(track.explicit))
        .bind(variant)
        .bind(&track.artwork_url)
        .bind(&now)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        let track_id: i64 =
            sqlx::query_scalar("SELECT id FROM tracks WHERE provider = ? AND provider_id = ?")
                .bind(provider)
                .bind(&track.provider_id)
                .fetch_one(&mut *transaction)
                .await?;
        for fact in facts {
            sqlx::query(
                "INSERT INTO fact_sources (name, base_url, source_kind) VALUES (?, ?, ?) \
                 ON CONFLICT(name, base_url) DO NOTHING",
            )
            .bind(&fact.source_name)
            .bind("https://musicbrainz.org")
            .bind("authoritative_metadata")
            .execute(&mut *transaction)
            .await?;
            let source_id: i64 =
                sqlx::query_scalar("SELECT id FROM fact_sources WHERE name = ? AND base_url = ?")
                    .bind(&fact.source_name)
                    .bind("https://musicbrainz.org")
                    .fetch_one(&mut *transaction)
                    .await?;
            sqlx::query(
                "INSERT INTO music_facts (id, text, category, source_id, source_url, confidence, human_reviewed, created_at, last_verified_at, verification_method) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT(id) DO UPDATE SET text = excluded.text, source_url = excluded.source_url, confidence = excluded.confidence, \
                 last_verified_at = excluded.last_verified_at, verification_method = excluded.verification_method",
            )
            .bind(&fact.id)
            .bind(&fact.text)
            .bind(enum_value(&fact.category)?)
            .bind(source_id)
            .bind(&fact.source_url)
            .bind(fact.confidence)
            .bind(i64::from(fact.human_reviewed))
            .bind(fact.created_at.to_rfc3339())
            .bind(fact.last_verified_at.map(|value| value.to_rfc3339()))
            .bind(enum_value(&fact.verification_method)?)
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                "INSERT INTO track_facts (track_id, fact_id, relevance) VALUES (?, ?, ?) \
                 ON CONFLICT(track_id, fact_id) DO UPDATE SET relevance = excluded.relevance",
            )
            .bind(track_id)
            .bind(&fact.id)
            .bind(fact.confidence)
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query(
            "INSERT INTO fact_lookup_cache (provider, track_provider_id, outcome, checked_at) VALUES (?, ?, ?, ?) \
             ON CONFLICT(provider, track_provider_id) DO UPDATE SET outcome = excluded.outcome, checked_at = excluded.checked_at",
        )
        .bind(provider)
        .bind(&track.provider_id)
        .bind(if facts.is_empty() { "no_match" } else { "matched" })
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}

fn enum_value<T: serde::Serialize>(value: &T) -> Result<String, AppError> {
    serde_json::to_value(value)
        .map_err(|error| AppError::Database(error.to_string()))?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| AppError::Database("enum did not serialize as a string".into()))
}

fn parse_enum<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, AppError> {
    serde_json::from_value(serde_json::Value::String(value.to_owned()))
        .map_err(|error| AppError::Database(error.to_string()))
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>, AppError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| AppError::Database(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    #[tokio::test]
    async fn settings_round_trip() -> Result<(), AppError> {
        let database = Database::connect("sqlite::memory:").await?;
        let repository = database.repository();
        let settings = AppSettings::default();
        repository.save_settings(&settings).await?;
        assert_eq!(repository.load_settings().await?, Some(settings));
        Ok(())
    }
}
