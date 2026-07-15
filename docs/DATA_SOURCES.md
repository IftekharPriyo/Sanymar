# Data sources

## Supported architecture

- Curated local facts: reviewable, high-confidence records with attribution.
- MusicBrainz: structured recording/release metadata using a compliant contact-bearing User-Agent, confidence gates, rate limiting, and a normalized positive/negative cache.
- Spotify: future playback and provider metadata, normalized at the adapter boundary.

Facts include source, optional URL, confidence, verification method/date, and entity association. `human_reviewed` remains distinct from `authoritative_metadata`; automated MusicBrainz facts are never mislabeled as human-reviewed. Retrieved text is untrusted: it cannot alter system instructions or request tools/credentials. No web scraping is used.

Mock playback uses authored fixture facts. Live Spotify playback uses MusicBrainz only when a valid contact is configured. ISRC is preferred; fallback search requires a score of at least 95 plus exact normalized title/artist and close duration. Ambiguous matches, missing release dates, outages, and rate limits produce no facts and never pause the broadcast workflow.
