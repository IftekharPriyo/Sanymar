PRAGMA foreign_keys = ON;

CREATE TABLE application_settings (
    key TEXT PRIMARY KEY NOT NULL,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE dj_profiles (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    station_name TEXT NOT NULL,
    profile_json TEXT NOT NULL,
    is_active INTEGER NOT NULL DEFAULT 0 CHECK (is_active IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE artists (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider TEXT NOT NULL,
    provider_id TEXT,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(provider, provider_id)
);

CREATE TABLE albums (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider TEXT NOT NULL,
    provider_id TEXT,
    title TEXT NOT NULL,
    release_date TEXT,
    artwork_url TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(provider, provider_id)
);

CREATE TABLE tracks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    title TEXT NOT NULL,
    album_id INTEGER REFERENCES albums(id) ON DELETE SET NULL,
    duration_ms INTEGER NOT NULL,
    isrc TEXT,
    release_date TEXT,
    explicit INTEGER NOT NULL DEFAULT 0 CHECK (explicit IN (0, 1)),
    variant TEXT NOT NULL,
    artwork_url TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(provider, provider_id)
);

CREATE TABLE track_artists (
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    artist_id INTEGER NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    artist_order INTEGER NOT NULL,
    PRIMARY KEY (track_id, artist_id)
);

CREATE TABLE fact_sources (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    base_url TEXT,
    source_kind TEXT NOT NULL,
    UNIQUE(name, base_url)
);

CREATE TABLE music_facts (
    id TEXT PRIMARY KEY NOT NULL,
    text TEXT NOT NULL,
    category TEXT NOT NULL,
    source_id INTEGER NOT NULL REFERENCES fact_sources(id),
    source_url TEXT,
    confidence REAL NOT NULL CHECK (confidence >= 0 AND confidence <= 1),
    human_reviewed INTEGER NOT NULL DEFAULT 0 CHECK (human_reviewed IN (0, 1)),
    artist_id INTEGER REFERENCES artists(id) ON DELETE SET NULL,
    album_id INTEGER REFERENCES albums(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    last_verified_at TEXT
);

CREATE TABLE track_facts (
    track_id INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    fact_id TEXT NOT NULL REFERENCES music_facts(id) ON DELETE CASCADE,
    relevance REAL NOT NULL DEFAULT 1 CHECK (relevance >= 0 AND relevance <= 1),
    PRIMARY KEY (track_id, fact_id)
);

CREATE TABLE broadcast_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT NOT NULL UNIQUE,
    track_id INTEGER REFERENCES tracks(id) ON DELETE SET NULL,
    artist_names TEXT NOT NULL,
    segment_type TEXT NOT NULL,
    opening_phrase TEXT,
    spoke INTEGER NOT NULL CHECK (spoke IN (0, 1)),
    outcome TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE used_facts (
    broadcast_id INTEGER NOT NULL REFERENCES broadcast_history(id) ON DELETE CASCADE,
    fact_id TEXT NOT NULL REFERENCES music_facts(id) ON DELETE CASCADE,
    used_at TEXT NOT NULL,
    PRIMARY KEY (broadcast_id, fact_id)
);

CREATE TABLE generated_scripts (
    id TEXT PRIMARY KEY NOT NULL,
    job_id TEXT NOT NULL,
    track_provider_id TEXT NOT NULL,
    dialogue TEXT NOT NULL,
    segment_type TEXT NOT NULL,
    fact_ids_json TEXT NOT NULL DEFAULT '[]',
    validation_status TEXT NOT NULL,
    validation_issues_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL
);

CREATE TABLE provider_auth_metadata (
    provider TEXT PRIMARY KEY NOT NULL,
    status TEXT NOT NULL,
    granted_scopes_json TEXT NOT NULL DEFAULT '[]',
    expires_at TEXT,
    credential_vault_key TEXT,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_tracks_isrc ON tracks(isrc) WHERE isrc IS NOT NULL;
CREATE INDEX idx_music_facts_verified ON music_facts(last_verified_at);
CREATE INDEX idx_broadcast_history_created ON broadcast_history(created_at DESC);
CREATE INDEX idx_used_facts_used_at ON used_facts(used_at DESC);
CREATE INDEX idx_generated_scripts_created ON generated_scripts(created_at DESC);

-- Raw access and refresh tokens are intentionally absent from this schema.
