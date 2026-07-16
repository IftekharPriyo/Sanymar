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
    music_provider::{MusicProvider, PlaybackInterruption, PlaybackState},
    settings::AppSettings,
};

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const PLAYBACK_POLL_TIMEOUT: Duration = Duration::from_secs(20);
const PREPARATION_TIMEOUT: Duration = Duration::from_secs(180);
const PLAYBACK_TIMEOUT_GRACE: Duration = Duration::from_secs(20);
const MAX_PLAYBACK_TIMEOUT: Duration = Duration::from_secs(120);
const TRANSITION_ARM_WINDOW_MS: u64 = 6_000;
const PAUSE_COMMAND_LEAD_MS: u64 = 750;
const TRACK_HANDOFF_POLL: Duration = Duration::from_millis(100);
const TRACK_HANDOFF_ATTEMPTS: usize = 30;
const AUDIO_STOP_TIMEOUT: Duration = Duration::from_secs(3);
const RETRY_COOLDOWN: Duration = Duration::from_secs(10);
const SUPERVISOR_RESTART_DELAY: Duration = Duration::from_secs(2);
const MAX_PREPARATION_ATTEMPTS: u8 = 2;
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
        let settings = state.settings.read().await.clone();
        resume_spotify_if_interrupted(&state, &settings).await;
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
            resume_spotify_if_interrupted(&state, &settings).await;
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
            resume_spotify_if_interrupted(&state, &settings).await;
        }
        if settle_playback(&mut playback).await {
            resume_spotify_if_interrupted(&state, &settings).await;
        }

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
            resume_spotify_if_interrupted(&state, &settings).await;
            abort_task(&mut preparation);
            abort_task(&mut playback);
            preparation_attempts = 0;
            retry_not_before = None;
            pair_complete = false;
            ready_audio = None;
            continue;
        };
        let expected_handoff = playback.is_some()
            && active_key
                .as_ref()
                .is_some_and(|active| active.next_track_id == key.current_track_id);
        if active_key.as_ref() != Some(&key) && !expected_handoff {
            state
                .coordinator
                .write()
                .await
                .cancel_if_playback_changed(&key.current_track_id, Some(&key.next_track_id));
            stop_audio_with_timeout(&state).await;
            abort_task(&mut preparation);
            abort_task(&mut playback);
            resume_spotify_if_interrupted(&state, &settings).await;
            active_key = Some(key.clone());
            preparation_attempts = 0;
            retry_not_before = None;
            pair_complete = false;
            ready_audio = None;
            tracing::info!("automatic transition track pair changed");
        }

        if expected_handoff {
            continue;
        }

        if playback.is_some() || preparation.is_some() || !snapshot.is_playing {
            continue;
        }

        if ready_audio.is_some() {
            if should_start_transition(&snapshot) {
                if let Some(prepared) = ready_audio.take() {
                    tracing::info!(
                        duration_ms = prepared.artifact.duration_ms,
                        "automatic transition playback started"
                    );
                    let app_for_playback = app.clone();
                    let timeout =
                        transition_timeout(remaining_ms(&snapshot), prepared.artifact.duration_ms);
                    let transition_key = key.clone();
                    playback = Some(WatchedTask::new(
                        tokio::spawn(async move {
                            play_transition_at_boundary(
                                &app_for_playback,
                                prepared,
                                snapshot,
                                transition_key,
                            )
                            .await
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

fn should_start_transition(playback: &PlaybackState) -> bool {
    remaining_ms(playback).is_some_and(|remaining| remaining <= TRANSITION_ARM_WINDOW_MS)
}

fn transition_timeout(remaining_ms: Option<u64>, duration_ms: Option<u64>) -> Duration {
    Duration::from_millis(
        remaining_ms
            .unwrap_or_default()
            .saturating_add(duration_ms.unwrap_or(FALLBACK_SPEECH_DURATION_MS)),
    )
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

async fn play_transition_at_boundary(
    app: &AppHandle,
    mut prepared: PreparedAudio,
    initial_playback: PlaybackState,
    key: TransitionKey,
) -> Result<SpeechResultView, AppError> {
    let remaining = remaining_ms(&initial_playback).ok_or(AppError::StaleJob)?;
    let delay = Duration::from_millis(remaining.saturating_sub(PAUSE_COMMAND_LEAD_MS));
    tokio::select! {
        () = tokio::time::sleep(delay) => {}
        () = prepared.cancellation.cancelled() => return Err(AppError::Cancelled),
    }

    let state = app.state::<AppState>();
    state
        .coordinator
        .write()
        .await
        .transition(crate::rj_engine::BroadcastState::PausingMusic);
    let settings = state.settings.read().await.clone();
    let provider = spotify_provider(&settings, state.credential_store.clone())?;
    let device_id = initial_playback
        .device
        .as_ref()
        .and_then(|device| device.id.clone());
    let interruption = PlaybackInterruption {
        device_id: device_id.clone(),
    };
    *state.spotify_interruption.write().await = Some(interruption.clone());

    if let Err(error) = provider.pause(device_id.as_deref()).await {
        if !matches!(
            &error,
            crate::music_provider::MusicProviderError::Timeout
                | crate::music_provider::MusicProviderError::Unavailable
        ) {
            clear_spotify_interruption(&state, &interruption).await;
        }
        let pause_error = AppError::Provider(error.to_string());
        resume_spotify_if_interrupted(&state, &settings).await;
        return Err(pause_error);
    }
    tracing::info!("automatic transition paused Spotify");

    let paused = provider
        .playback_state()
        .await
        .map_err(|error| AppError::Provider(error.to_string()));
    let paused = match paused {
        Ok(paused) => paused,
        Err(error) => {
            resume_spotify_if_interrupted(&state, &settings).await;
            return Err(error);
        }
    };
    let paused_track_id = paused
        .current_track
        .as_ref()
        .map(|track| track.provider_id.as_str());
    if paused_track_id == Some(key.current_track_id.as_str()) {
        if let Err(error) = provider.skip(device_id.as_deref()).await {
            let skip_error = AppError::Provider(error.to_string());
            resume_spotify_if_interrupted(&state, &settings).await;
            return Err(skip_error);
        }
        tracing::info!("automatic transition advanced Spotify while paused");
    } else if paused_track_id != Some(key.next_track_id.as_str()) {
        resume_spotify_if_interrupted(&state, &settings).await;
        return Err(AppError::StaleJob);
    }

    if let Err(error) = wait_for_track_handoff(&provider, &key.next_track_id).await {
        resume_spotify_if_interrupted(&state, &settings).await;
        return Err(error);
    }
    tracing::info!("automatic transition confirmed the next Spotify track");
    if let Err(error) = provider.seek(0, device_id.as_deref()).await {
        let seek_error = AppError::Provider(error.to_string());
        resume_spotify_if_interrupted(&state, &settings).await;
        return Err(seek_error);
    }
    tracing::info!("automatic transition reset the next Spotify track");

    let handoff_result = state
        .coordinator
        .write()
        .await
        .handoff_to_next(&prepared.job_id, &key.next_track_id);
    if let Err(error) = handoff_result {
        resume_spotify_if_interrupted(&state, &settings).await;
        return Err(error);
    }
    prepared.track_provider_id = key.next_track_id;
    tracing::info!("automatic transition is playing RJ audio");
    let speech_result = play_prepared_audio(&state, prepared).await;
    let resume_result = try_resume_spotify(&state, &settings).await;
    match (speech_result, resume_result) {
        (_, Err(resume_error)) => Err(resume_error),
        (Err(speech_error), Ok(())) => Err(speech_error),
        (Ok(speech), Ok(())) => Ok(speech),
    }
}

async fn wait_for_track_handoff(
    provider: &impl MusicProvider,
    expected_track_id: &str,
) -> Result<(), AppError> {
    for attempt in 0..TRACK_HANDOFF_ATTEMPTS {
        let playback = provider
            .playback_state()
            .await
            .map_err(|error| AppError::Provider(error.to_string()))?;
        if playback
            .current_track
            .as_ref()
            .is_some_and(|track| track.provider_id == expected_track_id)
        {
            return Ok(());
        }
        if attempt + 1 < TRACK_HANDOFF_ATTEMPTS {
            tokio::time::sleep(TRACK_HANDOFF_POLL).await;
        }
    }
    Err(AppError::StaleJob)
}

async fn resume_spotify_if_interrupted(state: &AppState, settings: &AppSettings) {
    if let Err(error) = try_resume_spotify(state, settings).await {
        tracing::warn!(error = %error, "Spotify resume is pending after commentary interruption");
    }
}

async fn try_resume_spotify(state: &AppState, settings: &AppSettings) -> Result<(), AppError> {
    let Some(interruption) = state.spotify_interruption.read().await.clone() else {
        return Ok(());
    };
    state
        .coordinator
        .write()
        .await
        .transition(crate::rj_engine::BroadcastState::ResumingMusic);
    let provider = spotify_provider(settings, state.credential_store.clone())?;
    provider
        .resume(interruption.device_id.as_deref())
        .await
        .map_err(|error| AppError::Provider(error.to_string()))?;
    clear_spotify_interruption(state, &interruption).await;
    state
        .coordinator
        .write()
        .await
        .transition(crate::rj_engine::BroadcastState::Monitoring);
    tracing::info!("Spotify playback resumed after commentary");
    Ok(())
}

async fn clear_spotify_interruption(state: &AppState, expected: &PlaybackInterruption) {
    let mut active = state.spotify_interruption.write().await;
    if active.as_ref() == Some(expected) {
        *active = None;
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

async fn settle_playback(
    task: &mut Option<WatchedTask<Result<SpeechResultView, AppError>>>,
) -> bool {
    if !task.as_ref().is_some_and(|task| task.handle.is_finished()) {
        return false;
    }
    let Some(completed) = task.take() else {
        return false;
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
    true
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
    fn arms_the_interruption_close_to_the_track_boundary() {
        assert!(!should_start_transition(&playback(190_000, 200_000)));
        assert!(should_start_transition(&playback(195_000, 200_000)));
    }

    #[test]
    fn invalid_progress_does_not_trigger_automation() {
        let invalid = playback(201_000, 200_000);
        assert!(!should_prepare(&invalid));
        assert!(!should_start_transition(&invalid));
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
        assert_eq!(
            transition_timeout(Some(5_000), Some(4_000)),
            Duration::from_secs(29)
        );
        assert_eq!(
            transition_timeout(Some(u64::MAX), Some(u64::MAX)),
            MAX_PLAYBACK_TIMEOUT
        );
    }
}
