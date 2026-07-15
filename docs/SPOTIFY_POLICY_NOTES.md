# Spotify policy notes

Status: PKCE authorization plus refresh and read-only playback/queue integration are implemented. Official Spotify authorization, redirect, player endpoint, and February 2026 change documentation was re-checked on 2026-07-14. Re-check again immediately before release because policies and endpoint availability can change.

Design assumptions to verify include user authorization, applicable account/playback-control limitations, attribution/artwork rules, rate limits, caching/retention constraints, and restrictions on synchronization, recording, downloading, rebroadcasting, or altering Spotify content. Sanymar must not save Spotify audio, download music, or rebroadcast it. Commentary is played locally between coordinated playback states, not mixed into or recorded with Spotify audio.

Use Authorization Code with PKCE and no embedded client secret. The registered redirect is `http://127.0.0.1:43821/callback`; `localhost` is not accepted. Store tokens in Windows Credential Manager and log neither tokens nor authorization codes. Requested scopes are `user-read-currently-playing`, `user-read-playback-state`, and `user-modify-playback-state`.

Spotify's player documentation prohibits commercial streaming integrations, altering Spotify content, synchronization with visual media, and non-interactive broadcasting. Sanymar must remain a personal, user-directed, noncommercial local companion that pauses Spotify before separately playing commentary; it must not mix, record, download, rebroadcast, or train on Spotify audio/content. This interpretation requires human policy review before distribution.
