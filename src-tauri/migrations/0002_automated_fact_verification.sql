ALTER TABLE music_facts ADD COLUMN verification_method TEXT NOT NULL DEFAULT 'unverified';

UPDATE music_facts
SET verification_method = 'human_reviewed'
WHERE human_reviewed = 1;

CREATE TABLE fact_lookup_cache (
    provider TEXT NOT NULL,
    track_provider_id TEXT NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('matched', 'no_match')),
    checked_at TEXT NOT NULL,
    PRIMARY KEY (provider, track_provider_id)
);

CREATE INDEX idx_fact_lookup_cache_checked ON fact_lookup_cache(checked_at);
