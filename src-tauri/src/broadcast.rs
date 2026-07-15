use std::time::Duration;

use tauri::{AppHandle, Manager};
use tokio::{task::JoinHandle, time::MissedTickBehavior};

use crate::{
    audio::AudioPlayer,
    commands::{
        generate_segment_for_playback, play_prepared_audio, spotify_provider,
        synthesize_recent_script, AppState, PreparedAudio, SpeechResultView,
    },
    errors::AppError,
    music_provider::{MusicProvider, PlaybackState},
};

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const PLAYBACK_FINISH_BUFFER_MS: u64 = 3_000;
const FALLBACK_SPEECH_DURATION_MS: u64 = 5_000;

#[derive(Clone, Debug, PartialEq, Eq)]
struct TransitionKey {
    current_track_id: String,
    next_track_id: String,
}

pub async fn run_transition_automation(app: AppHandle) {
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut active_key: Option<TransitionKey> = None;
    let mut attempted_key: Option<TransitionKey> = None;
    let mut preparation: Option<JoinHandle<Result<Option<PreparedAudio>, AppError>>> = None;
    let mut ready_audio: Option<PreparedAudio> = None;
    let mut playback: Option<JoinHandle<Result<SpeechResultView, AppError>>> = None;

    loop {
        interval.tick().await;
        let state = app.state::<AppState>();
        let settings = state.settings.read().await.clone();

        if !settings.automatic_transition_speech || settings.mock_mode {
            if active_key.take().is_some() {
                state.coordinator.write().await.cancel();
                let _ = state.audio.stop().await;
            }
            abort_task(&mut preparation);
            abort_task(&mut playback);
            attempted_key = None;
            ready_audio = None;
            continue;
        }

        settle_preparation(&mut preparation, &mut ready_audio).await;
        settle_playback(&mut playback).await;

        let snapshot = match spotify_provider(&settings, state.credential_store.clone()) {
            Ok(provider) => match provider.playback_state().await {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    tracing::warn!(error = %error, "automatic transition playback poll failed");
                    continue;
                }
            },
            Err(error) => {
                tracing::warn!(error = %error, "automatic transition provider is not configured");
                continue;
            }
        };

        let Some(key) = transition_key(&snapshot) else {
            if active_key.take().is_some() {
                state.coordinator.write().await.cancel();
                let _ = state.audio.stop().await;
            }
            abort_task(&mut preparation);
            abort_task(&mut playback);
            attempted_key = None;
            ready_audio = None;
            continue;
        };
        if active_key.as_ref() != Some(&key) {
            state
                .coordinator
                .write()
                .await
                .cancel_if_playback_changed(&key.current_track_id, Some(&key.next_track_id));
            let _ = state.audio.stop().await;
            abort_task(&mut preparation);
            abort_task(&mut playback);
            active_key = Some(key.clone());
            attempted_key = None;
            ready_audio = None;
            tracing::info!("automatic transition track pair changed");
        }

        if playback.is_some() || preparation.is_some() || !snapshot.is_playing {
            continue;
        }

        if let Some(prepared) = ready_audio.as_ref() {
            if should_play(&snapshot, prepared.artifact.duration_ms) {
                if let Some(prepared) = ready_audio.take() {
                    tracing::info!(
                        duration_ms = prepared.artifact.duration_ms,
                        "automatic transition playback started"
                    );
                    let app_for_playback = app.clone();
                    playback = Some(tokio::spawn(async move {
                        let state = app_for_playback.state::<AppState>();
                        play_prepared_audio(&state, prepared).await
                    }));
                }
            }
            continue;
        }

        if attempted_key.as_ref() == Some(&key) || !should_prepare(&snapshot) {
            continue;
        }

        attempted_key = Some(key);
        tracing::info!("automatic transition preparation started");
        let app_for_preparation = app.clone();
        preparation = Some(tokio::spawn(async move {
            let state = app_for_preparation.state::<AppState>();
            let settings = state.settings.read().await.clone();
            let generated =
                generate_segment_for_playback(&state, settings.clone(), snapshot, true).await?;
            if generated.dialogue.is_none() {
                return Ok(None);
            }
            synthesize_recent_script(&app_for_preparation, &state, settings)
                .await
                .map(Some)
        }));
    }
}

fn transition_key(playback: &PlaybackState) -> Option<TransitionKey> {
    Some(TransitionKey {
        current_track_id: playback.current_track.as_ref()?.provider_id.clone(),
        next_track_id: playback.next_track.as_ref()?.provider_id.clone(),
    })
}

fn remaining_ms(playback: &PlaybackState) -> Option<u64> {
    let duration = playback.current_track.as_ref()?.duration_ms;
    (playback.progress_ms <= duration).then(|| duration - playback.progress_ms)
}

fn should_prepare(playback: &PlaybackState) -> bool {
    playback.is_playing
        && playback.next_track.is_some()
        && remaining_ms(playback).is_some_and(|remaining| remaining > 0)
}

fn should_play(playback: &PlaybackState, duration_ms: Option<u64>) -> bool {
    let speech_duration = duration_ms.unwrap_or(FALLBACK_SPEECH_DURATION_MS);
    remaining_ms(playback)
        .is_some_and(|remaining| remaining <= speech_duration + PLAYBACK_FINISH_BUFFER_MS)
}

async fn settle_preparation(
    task: &mut Option<JoinHandle<Result<Option<PreparedAudio>, AppError>>>,
    ready_audio: &mut Option<PreparedAudio>,
) {
    if !task.as_ref().is_some_and(|task| task.is_finished()) {
        return;
    }
    let Some(completed) = task.take() else {
        return;
    };
    match completed.await {
        Ok(Ok(prepared)) => {
            if let Some(prepared) = prepared {
                tracing::info!(
                    duration_ms = prepared.artifact.duration_ms,
                    "automatic transition audio is ready"
                );
                *ready_audio = Some(prepared);
            } else {
                tracing::info!("automatic transition selected intentional silence");
            }
        }
        Ok(Err(AppError::Cancelled | AppError::StaleJob)) => {}
        Ok(Err(error)) => tracing::warn!(error = %error, "automatic transition preparation failed"),
        Err(error) if error.is_cancelled() => {}
        Err(error) => tracing::warn!(error = %error, "automatic transition task failed"),
    }
}

async fn settle_playback(task: &mut Option<JoinHandle<Result<SpeechResultView, AppError>>>) {
    if !task.as_ref().is_some_and(|task| task.is_finished()) {
        return;
    }
    let Some(completed) = task.take() else {
        return;
    };
    match completed.await {
        Ok(Ok(_)) => tracing::info!("automatic transition playback completed"),
        Ok(Err(AppError::Cancelled | AppError::StaleJob)) => {
            tracing::info!("automatic transition playback cancelled after a track change");
        }
        Ok(Err(error)) => tracing::warn!(error = %error, "automatic transition playback failed"),
        Err(error) if error.is_cancelled() => {}
        Err(error) => tracing::warn!(error = %error, "automatic transition playback task failed"),
    }
}

fn abort_task<T>(task: &mut Option<JoinHandle<T>>) {
    if let Some(task) = task.take() {
        task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music_provider::{Track, TrackVariant};

    fn playback(progress_ms: u64, duration_ms: u64) -> PlaybackState {
        let track = |id: &str, duration_ms| Track {
            provider_id: id.into(),
            title: id.into(),
            artists: Vec::new(),
            album: None,
            duration_ms,
            isrc: None,
            release_date: None,
            explicit: false,
            variant: TrackVariant::Studio,
            artwork_url: None,
        };
        PlaybackState {
            current_track: Some(track("current", duration_ms)),
            next_track: Some(track("next", 200_000)),
            progress_ms,
            is_playing: true,
            device: None,
        }
    }

    #[test]
    fn prepares_as_soon_as_a_valid_track_pair_is_available() {
        assert!(should_prepare(&playback(1, 200_000)));
        assert!(should_prepare(&playback(125_000, 200_000)));
    }

    #[test]
    fn schedules_audio_to_finish_at_the_track_boundary() {
        assert!(!should_play(&playback(190_000, 200_000), Some(4_000)));
        assert!(should_play(&playback(195_000, 200_000), Some(4_000)));
    }

    #[test]
    fn invalid_progress_does_not_trigger_automation() {
        let invalid = playback(201_000, 200_000);
        assert!(!should_prepare(&invalid));
        assert!(!should_play(&invalid, Some(4_000)));
    }
}
