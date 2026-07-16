use std::time::Duration;

use tauri::{AppHandle, Manager};
use tokio::{
    task::JoinHandle,
    time::{Instant, MissedTickBehavior},
};

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
const PLAYBACK_POLL_TIMEOUT: Duration = Duration::from_secs(20);
const PREPARATION_TIMEOUT: Duration = Duration::from_secs(180);
const PLAYBACK_TIMEOUT_GRACE: Duration = Duration::from_secs(20);
const MAX_PLAYBACK_TIMEOUT: Duration = Duration::from_secs(120);
const AUDIO_STOP_TIMEOUT: Duration = Duration::from_secs(3);
const RETRY_COOLDOWN: Duration = Duration::from_secs(10);
const SUPERVISOR_RESTART_DELAY: Duration = Duration::from_secs(2);
const MAX_PREPARATION_ATTEMPTS: u8 = 2;
const PLAYBACK_FINISH_BUFFER_MS: u64 = 3_000;
const FALLBACK_SPEECH_DURATION_MS: u64 = 5_000;

#[derive(Clone, Debug, PartialEq, Eq)]
struct TransitionKey {
    current_track_id: String,
    next_track_id: String,
}

pub async fn run_transition_automation(app: AppHandle) {
    loop {
        let worker = tokio::spawn(run_transition_worker(app.clone()));
        match worker.await {
            Ok(()) => tracing::warn!("automatic transition worker stopped unexpectedly"),
            Err(error) if error.is_cancelled() => return,
            Err(error) => {
                tracing::error!(error = %error, "automatic transition worker crashed")
            }
        }

        let state = app.state::<AppState>();
        state.coordinator.write().await.cancel();
        stop_audio_with_timeout(&state).await;
        tracing::info!(
            restart_delay_ms = SUPERVISOR_RESTART_DELAY.as_millis(),
            "automatic transition worker will restart"
        );
        tokio::time::sleep(SUPERVISOR_RESTART_DELAY).await;
    }
}

async fn run_transition_worker(app: AppHandle) {
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut active_key: Option<TransitionKey> = None;
    let mut preparation_attempts = 0_u8;
    let mut retry_not_before: Option<Instant> = None;
    let mut pair_complete = false;
    let mut preparation: Option<WatchedTask<Result<Option<PreparedAudio>, AppError>>> = None;
    let mut ready_audio: Option<PreparedAudio> = None;
    let mut playback: Option<WatchedTask<Result<SpeechResultView, AppError>>> = None;

    loop {
        interval.tick().await;
        let state = app.state::<AppState>();
        let settings = state.settings.read().await.clone();

        if !settings.automatic_transition_speech || settings.mock_mode {
            if active_key.take().is_some() {
                state.coordinator.write().await.cancel();
                stop_audio_with_timeout(&state).await;
            }
            abort_task(&mut preparation);
            abort_task(&mut playback);
            preparation_attempts = 0;
            retry_not_before = None;
            pair_complete = false;
            ready_audio = None;
            continue;
        }

        let now = Instant::now();
        if preparation
            .as_ref()
            .is_some_and(|task| task.is_expired(now))
        {
            tracing::warn!(
                timeout_ms = PREPARATION_TIMEOUT.as_millis(),
                "automatic transition preparation watchdog expired"
            );
            state.coordinator.write().await.cancel();
            abort_task(&mut preparation);
            ready_audio = None;
            schedule_retry(
                preparation_attempts,
                &mut retry_not_before,
                &mut pair_complete,
                now,
            );
        }
        match settle_preparation(&mut preparation, &mut ready_audio).await {
            PreparationSettlement::Pending => {}
            PreparationSettlement::Completed => pair_complete = true,
            PreparationSettlement::Failed => schedule_retry(
                preparation_attempts,
                &mut retry_not_before,
                &mut pair_complete,
                now,
            ),
        }

        if playback.as_ref().is_some_and(|task| task.is_expired(now)) {
            tracing::warn!("automatic transition playback watchdog expired");
            state.coordinator.write().await.cancel();
            stop_audio_with_timeout(&state).await;
            abort_task(&mut playback);
            pair_complete = true;
        }
        settle_playback(&mut playback).await;

        let snapshot = match spotify_provider(&settings, state.credential_store.clone()) {
            Ok(provider) => {
                match tokio::time::timeout(PLAYBACK_POLL_TIMEOUT, provider.playback_state()).await {
                    Ok(Ok(snapshot)) => snapshot,
                    Ok(Err(error)) => {
                        tracing::warn!(error = %error, "automatic transition playback poll failed");
                        continue;
                    }
                    Err(_) => {
                        tracing::warn!(
                            timeout_ms = PLAYBACK_POLL_TIMEOUT.as_millis(),
                            "automatic transition playback poll watchdog expired"
                        );
                        continue;
                    }
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, "automatic transition provider is not configured");
                continue;
            }
        };

        let Some(key) = transition_key(&snapshot) else {
            if active_key.take().is_some() {
                state.coordinator.write().await.cancel();
                stop_audio_with_timeout(&state).await;
            }
            abort_task(&mut preparation);
            abort_task(&mut playback);
            preparation_attempts = 0;
            retry_not_before = None;
            pair_complete = false;
            ready_audio = None;
            continue;
        };
        if active_key.as_ref() != Some(&key) {
            state
                .coordinator
                .write()
                .await
                .cancel_if_playback_changed(&key.current_track_id, Some(&key.next_track_id));
            stop_audio_with_timeout(&state).await;
            abort_task(&mut preparation);
            abort_task(&mut playback);
            active_key = Some(key.clone());
            preparation_attempts = 0;
            retry_not_before = None;
            pair_complete = false;
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
                    let timeout = playback_timeout(prepared.artifact.duration_ms);
                    playback = Some(WatchedTask::new(
                        tokio::spawn(async move {
                            let state = app_for_playback.state::<AppState>();
                            play_prepared_audio(&state, prepared).await
                        }),
                        timeout,
                    ));
                }
            }
            continue;
        }

        if pair_complete
            || preparation_attempts >= MAX_PREPARATION_ATTEMPTS
            || retry_not_before.is_some_and(|deadline| now < deadline)
            || !should_prepare(&snapshot)
        {
            continue;
        }

        preparation_attempts += 1;
        retry_not_before = None;
        tracing::info!(
            attempt = preparation_attempts,
            "automatic transition preparation started"
        );
        let app_for_preparation = app.clone();
        preparation = Some(WatchedTask::new(
            tokio::spawn(async move {
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
            }),
            PREPARATION_TIMEOUT,
        ));
    }
}

struct WatchedTask<T> {
    handle: JoinHandle<T>,
    deadline: Instant,
}

impl<T> WatchedTask<T> {
    fn new(handle: JoinHandle<T>, timeout: Duration) -> Self {
        Self {
            handle,
            deadline: Instant::now() + timeout,
        }
    }

    fn is_expired(&self, now: Instant) -> bool {
        !self.handle.is_finished() && now >= self.deadline
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

fn playback_timeout(duration_ms: Option<u64>) -> Duration {
    Duration::from_millis(duration_ms.unwrap_or(FALLBACK_SPEECH_DURATION_MS))
        .saturating_add(PLAYBACK_TIMEOUT_GRACE)
        .min(MAX_PLAYBACK_TIMEOUT)
}

fn schedule_retry(
    attempts: u8,
    retry_not_before: &mut Option<Instant>,
    pair_complete: &mut bool,
    now: Instant,
) {
    if attempts < MAX_PREPARATION_ATTEMPTS {
        *retry_not_before = Some(now + RETRY_COOLDOWN);
        tracing::info!(
            retry_delay_ms = RETRY_COOLDOWN.as_millis(),
            "automatic transition preparation will retry"
        );
    } else {
        *pair_complete = true;
        tracing::warn!("automatic transition preparation attempts exhausted for this track pair");
    }
}

async fn stop_audio_with_timeout(state: &AppState) {
    match tokio::time::timeout(AUDIO_STOP_TIMEOUT, state.audio.stop()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(error = %error, "automatic transition audio stop failed"),
        Err(_) => tracing::warn!(
            timeout_ms = AUDIO_STOP_TIMEOUT.as_millis(),
            "automatic transition audio stop watchdog expired"
        ),
    }
}

async fn settle_preparation(
    task: &mut Option<WatchedTask<Result<Option<PreparedAudio>, AppError>>>,
    ready_audio: &mut Option<PreparedAudio>,
) -> PreparationSettlement {
    if !task.as_ref().is_some_and(|task| task.handle.is_finished()) {
        return PreparationSettlement::Pending;
    }
    let Some(completed) = task.take() else {
        return PreparationSettlement::Pending;
    };
    match completed.handle.await {
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
            PreparationSettlement::Completed
        }
        Ok(Err(AppError::Cancelled | AppError::StaleJob)) => PreparationSettlement::Completed,
        Ok(Err(error)) => {
            tracing::warn!(error = %error, "automatic transition preparation failed");
            PreparationSettlement::Failed
        }
        Err(error) if error.is_cancelled() => PreparationSettlement::Completed,
        Err(error) => {
            tracing::warn!(error = %error, "automatic transition task failed");
            PreparationSettlement::Failed
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreparationSettlement {
    Pending,
    Completed,
    Failed,
}

async fn settle_playback(task: &mut Option<WatchedTask<Result<SpeechResultView, AppError>>>) {
    if !task.as_ref().is_some_and(|task| task.handle.is_finished()) {
        return;
    }
    let Some(completed) = task.take() else {
        return;
    };
    match completed.handle.await {
        Ok(Ok(_)) => tracing::info!("automatic transition playback completed"),
        Ok(Err(AppError::Cancelled | AppError::StaleJob)) => {
            tracing::info!("automatic transition playback cancelled after a track change");
        }
        Ok(Err(error)) => tracing::warn!(error = %error, "automatic transition playback failed"),
        Err(error) if error.is_cancelled() => {}
        Err(error) => tracing::warn!(error = %error, "automatic transition playback task failed"),
    }
}

fn abort_task<T>(task: &mut Option<WatchedTask<T>>) {
    if let Some(task) = task.take() {
        task.handle.abort();
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

    #[test]
    fn failed_preparation_gets_one_bounded_retry() {
        let now = Instant::now();
        let mut retry_not_before = None;
        let mut pair_complete = false;

        schedule_retry(1, &mut retry_not_before, &mut pair_complete, now);
        assert_eq!(retry_not_before, Some(now + RETRY_COOLDOWN));
        assert!(!pair_complete);

        schedule_retry(2, &mut retry_not_before, &mut pair_complete, now);
        assert!(pair_complete);
    }

    #[test]
    fn playback_watchdog_is_bounded() {
        assert_eq!(playback_timeout(Some(4_000)), Duration::from_secs(24));
        assert_eq!(playback_timeout(Some(u64::MAX)), MAX_PLAYBACK_TIMEOUT);
    }
}
