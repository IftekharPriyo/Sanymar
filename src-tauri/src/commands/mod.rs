use std::sync::Arc;

use chrono::NaiveDate;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    audio::{router::LocalAudioRouter, AudioPlayer, AudioPlayerError},
    database::{Database, GeneratedScriptRecord},
    errors::{AppError, CommandError},
    llm::{
        mock::MockScriptGenerator,
        ollama::{OllamaConfiguration, OllamaHealth, OllamaScriptGenerator},
        ScriptGenerator, ScriptGeneratorError, ScriptRequest,
    },
    music_facts::{
        mock::MockMusicFactProvider,
        musicbrainz::{MusicBrainzFactProvider, MusicBrainzRateLimiter},
        MusicFact, MusicFactError, MusicFactProvider,
    },
    music_provider::{Album, Artist, MusicProvider, PlaybackState, Track, TrackVariant},
    playback::CommentaryJob,
    rj_engine::{
        normalize_for_speech, BroadcastCoordinator, BroadcastMemory, BroadcastState,
        ContentDirector, DjProfile, ScriptValidator, SegmentType,
    },
    security::CredentialStore,
    settings::{AppSettings, TtsProviderSetting},
    spotify::{
        api::SpotifyProvider,
        auth::{SpotifyAuthService, SpotifyConnectionStatus},
        SpotifyConfiguration,
    },
    tts::{
        mock::MockTextToSpeechProvider,
        parler::{ParlerConfiguration, ParlerMiniTtsProvider},
        sherpa::{SherpaKokoroConfiguration, SherpaKokoroTtsProvider, SherpaTtsHealth},
        AudioArtifact, DeliveryStyle, TextToSpeechProvider, TtsError, VoiceSettings,
    },
};

#[derive(Clone, Debug)]
pub(crate) struct PreparedScript {
    pub(crate) dialogue: String,
    pub(crate) job_id: String,
    pub(crate) track_provider_id: String,
    pub(crate) segment_type: SegmentType,
}

pub struct AppState {
    pub(crate) database: Arc<Database>,
    pub(crate) settings: RwLock<AppSettings>,
    pub(crate) coordinator: RwLock<BroadcastCoordinator>,
    pub(crate) memory: RwLock<BroadcastMemory>,
    pub(crate) recent_script: RwLock<Option<PreparedScript>>,
    pub(crate) audio: LocalAudioRouter,
    pub(crate) spotify_auth: SpotifyAuthService,
    pub(crate) credential_store: Arc<dyn CredentialStore>,
    pub(crate) musicbrainz_rate_limiter: Arc<MusicBrainzRateLimiter>,
}

impl AppState {
    pub fn new(
        database: Arc<Database>,
        settings: AppSettings,
        spotify_auth: SpotifyAuthService,
        credential_store: Arc<dyn CredentialStore>,
    ) -> Self {
        Self {
            database,
            settings: RwLock::new(settings),
            coordinator: RwLock::new(BroadcastCoordinator::default()),
            memory: RwLock::new(BroadcastMemory::default()),
            recent_script: RwLock::new(None),
            audio: LocalAudioRouter::default(),
            spotify_auth,
            credential_store,
            musicbrainz_rate_limiter: Arc::new(MusicBrainzRateLimiter::default()),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardView {
    pub mock_mode: bool,
    pub llm_mock_mode: bool,
    pub tts_mock_mode: bool,
    pub connection_status: String,
    pub current_provider: String,
    pub playback: PlaybackState,
    pub broadcast_state: String,
    pub dj_profile: DjProfile,
    pub talk_frequency: String,
    pub llm_status: String,
    pub tts_status: String,
    pub recent_script: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedSegmentView {
    pub job_id: String,
    pub dialogue: Option<String>,
    pub segment_type: SegmentType,
    pub broadcast_state: String,
    pub is_mock: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechResultView {
    pub artifact_id: String,
    pub duration_ms: Option<u64>,
    pub is_mock: bool,
    pub message: String,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedAudio {
    pub(crate) artifact: AudioArtifact,
    pub(crate) cancellation: tokio_util::sync::CancellationToken,
    pub(crate) job_id: String,
    pub(crate) track_provider_id: String,
    pub(crate) provider: TtsProviderSetting,
}

#[tauri::command]
pub async fn get_dashboard(state: State<'_, AppState>) -> Result<DashboardView, CommandError> {
    let settings = state.settings.read().await.clone();
    let broadcast_state = state.coordinator.read().await.state().label().to_owned();
    let (playback, connection_status, current_provider) = if settings.mock_mode {
        (
            mock_playback(),
            "Connected to mock provider".into(),
            "Spotify (mock)".into(),
        )
    } else {
        let provider = spotify_provider(&settings, state.credential_store.clone())?;
        let playback = provider
            .playback_state()
            .await
            .map_err(|error| AppError::Provider(error.to_string()))?;
        (
            playback,
            "Connected to Spotify".into(),
            "Spotify Web API".into(),
        )
    };
    if let Some(current_track) = &playback.current_track {
        state.coordinator.write().await.cancel_if_playback_changed(
            &current_track.provider_id,
            playback
                .next_track
                .as_ref()
                .map(|track| track.provider_id.as_str()),
        );
    }
    let llm_mock_mode = !settings.use_ollama;
    Ok(DashboardView {
        mock_mode: settings.mock_mode,
        llm_mock_mode,
        tts_mock_mode: matches!(settings.tts_provider, TtsProviderSetting::Mock),
        connection_status,
        current_provider,
        playback,
        broadcast_state,
        dj_profile: DjProfile::default(),
        talk_frequency: format!("{:?}", settings.talk_frequency),
        llm_status: if settings.use_ollama {
            format!(
                "Ollama: {} (health not checked)",
                settings
                    .ollama_model
                    .as_deref()
                    .unwrap_or("model not selected")
            )
        } else {
            "Mock script generator ready".into()
        },
        tts_status: match settings.tts_provider {
            TtsProviderSetting::Mock => "Mock TTS ready (no audio generated)".into(),
            TtsProviderSetting::SherpaKokoro => {
                "Sherpa-ONNX Kokoro configured (health not checked)".into()
            }
            TtsProviderSetting::ParlerMini => {
                "Parler Mini local service configured (health not checked)".into()
            }
        },
        recent_script: state
            .recent_script
            .read()
            .await
            .as_ref()
            .map(|script| script.dialogue.clone()),
    })
}

#[tauri::command]
pub async fn generate_test_segment(
    state: State<'_, AppState>,
) -> Result<GeneratedSegmentView, CommandError> {
    let settings = state.settings.read().await.clone();
    let playback = if settings.mock_mode {
        mock_playback()
    } else {
        spotify_provider(&settings, state.credential_store.clone())?
            .playback_state()
            .await
            .map_err(|error| AppError::Provider(error.to_string()))?
    };
    generate_segment_for_playback(&state, settings, playback, true)
        .await
        .map_err(Into::into)
}

pub(crate) async fn generate_segment_for_playback(
    state: &AppState,
    settings: AppSettings,
    playback: PlaybackState,
    force_commentary: bool,
) -> Result<GeneratedSegmentView, AppError> {
    let current = playback
        .current_track
        .clone()
        .ok_or(AppError::NoActivePlayback)?;
    let job = CommentaryJob {
        job_id: Uuid::new_v4().to_string(),
        current_track_id: current.provider_id.clone(),
        next_track_id: playback
            .next_track
            .as_ref()
            .map(|track| track.provider_id.clone()),
    };
    let cancellation = {
        let mut coordinator = state.coordinator.write().await;
        let cancellation = coordinator.start(job.clone());
        coordinator.transition(BroadcastState::FetchingFacts);
        cancellation
    };

    let facts = if settings.mock_mode {
        MockMusicFactProvider
            .facts_for(&current, cancellation.clone())
            .await
            .map_err(|error| AppError::Provider(error.to_string()))?
    } else if let Some(contact) = settings.musicbrainz_contact.as_deref() {
        let provider = MusicBrainzFactProvider::new(
            contact,
            state.database.repository(),
            settings.cache_retention_days,
            state.musicbrainz_rate_limiter.clone(),
        )
        .map_err(|error| AppError::Configuration(error.to_string()))?;
        provider
            .facts_for(&current, cancellation.clone())
            .await
            .or_else(unattended_fact_fallback)?
    } else {
        Vec::new()
    };
    state
        .coordinator
        .write()
        .await
        .transition(BroadcastState::SelectingSegment);
    let mut director = ContentDirector::new(ChaCha8Rng::from_entropy());
    let stored_memory = state.memory.read().await.clone();
    let memory = if force_commentary {
        BroadcastMemory {
            consecutive_without_commentary: 4,
            ..stored_memory
        }
    } else {
        stored_memory
    };
    let plan = director.select(settings.talk_frequency.clone(), &facts, &memory);
    if plan.segment_type == SegmentType::Silence {
        let mut stored_memory = state.memory.write().await;
        record_silence(&mut stored_memory);
        state
            .coordinator
            .write()
            .await
            .transition(BroadcastState::Monitoring);
        return Ok(GeneratedSegmentView {
            job_id: job.job_id,
            dialogue: None,
            segment_type: SegmentType::Silence,
            broadcast_state: "Monitoring".into(),
            is_mock: !settings.use_ollama,
        });
    }

    state
        .coordinator
        .write()
        .await
        .transition(BroadcastState::GeneratingScript);
    let profile = DjProfile::default();
    let selected_facts = facts
        .into_iter()
        .filter(|fact| plan.fact_ids.contains(&fact.id))
        .collect();
    let request = ScriptRequest {
        plan: plan.clone(),
        profile: profile.clone(),
        previous_track: Some(current.clone()),
        next_track: playback.next_track,
        facts: selected_facts,
        memory: memory.clone(),
        maximum_words: settings.maximum_segment_words,
        cancellation,
    };
    let generation_result = if settings.use_ollama {
        let model = settings
            .ollama_model
            .as_deref()
            .ok_or_else(|| AppError::Configuration("select an Ollama model in Settings".into()))?;
        let configuration = OllamaConfiguration::new(&settings.ollama_base_url, model)
            .map_err(|error| AppError::Configuration(error.to_string()))?;
        OllamaScriptGenerator::new(configuration)
            .map_err(|error| AppError::Configuration(error.to_string()))?
            .generate(request)
            .await
    } else {
        MockScriptGenerator.generate(request).await
    };
    let candidates = match generation_result {
        Ok(candidates) => candidates,
        Err(error) if should_fall_back_to_silence(&error) => {
            tracing::warn!(reason = %error, "Ollama output failed validation; continuing with silence");
            state
                .coordinator
                .write()
                .await
                .transition(BroadcastState::Monitoring);
            return Ok(GeneratedSegmentView {
                job_id: job.job_id,
                dialogue: None,
                segment_type: SegmentType::Silence,
                broadcast_state: "Monitoring".into(),
                is_mock: false,
            });
        }
        Err(error) => return Err(AppError::ScriptGeneration(error.to_string())),
    };
    let candidate = candidates
        .into_iter()
        .next()
        .ok_or_else(|| AppError::ScriptGeneration("generator returned no candidate".into()))?;

    state
        .coordinator
        .write()
        .await
        .transition(BroadcastState::ValidatingScript);
    let report = ScriptValidator::validate(
        &candidate.dialogue,
        settings.maximum_segment_words.into(),
        plan.segment_type,
        candidate.fact_ids.len(),
        Some(&current),
        &profile,
        &memory.recent_openings,
    );
    if !report.valid {
        return Err(AppError::Validation(format!("{:?}", report.issues)));
    }
    state
        .coordinator
        .read()
        .await
        .ensure_current(&job.job_id, &current.provider_id)?;
    state
        .coordinator
        .write()
        .await
        .transition(BroadcastState::WaitingForTransition);
    *state.recent_script.write().await = Some(PreparedScript {
        dialogue: candidate.dialogue.clone(),
        job_id: job.job_id.clone(),
        track_provider_id: current.provider_id.clone(),
        segment_type: plan.segment_type,
    });

    let record = GeneratedScriptRecord {
        id: Uuid::new_v4().to_string(),
        job_id: job.job_id.clone(),
        track_provider_id: current.provider_id,
        dialogue: candidate.dialogue.clone(),
        segment_type: format!("{:?}", plan.segment_type),
        fact_ids_json: serde_json::to_string(&candidate.fact_ids)
            .map_err(|error| AppError::Internal(error.to_string()))?,
        validation_status: "valid".into(),
        validation_issues_json: "[]".into(),
    };
    state
        .database
        .repository()
        .save_generated_script(&record)
        .await?;

    {
        let mut stored_memory = state.memory.write().await;
        record_commentary(
            &mut stored_memory,
            plan.segment_type,
            &candidate.fact_ids,
            &candidate.dialogue,
        );
    }

    Ok(GeneratedSegmentView {
        job_id: job.job_id,
        dialogue: Some(candidate.dialogue),
        segment_type: plan.segment_type,
        broadcast_state: "Waiting for transition".into(),
        is_mock: !settings.use_ollama,
    })
}

fn record_silence(memory: &mut BroadcastMemory) {
    memory.consecutive_without_commentary = memory.consecutive_without_commentary.saturating_add(1);
    memory.consecutive_with_commentary = 0;
}

fn record_commentary(
    memory: &mut BroadcastMemory,
    segment_type: SegmentType,
    fact_ids: &[String],
    dialogue: &str,
) {
    memory.consecutive_with_commentary = memory.consecutive_with_commentary.saturating_add(1);
    memory.consecutive_without_commentary = 0;
    memory.recent_segment_types.insert(0, segment_type);
    memory.recent_segment_types.truncate(4);
    for fact_id in fact_ids.iter().rev() {
        memory.recent_fact_ids.insert(0, fact_id.clone());
    }
    memory.recent_fact_ids.truncate(12);
    if let Some(opening) = dialogue.split_whitespace().next() {
        memory.recent_openings.insert(0, opening.to_owned());
        memory.recent_openings.truncate(6);
    }
}

fn unattended_fact_fallback(error: MusicFactError) -> Result<Vec<MusicFact>, AppError> {
    if matches!(error, MusicFactError::Cancelled) {
        Err(AppError::Cancelled)
    } else {
        tracing::warn!(error = %error, "MusicBrainz lookup failed; continuing without facts");
        Ok(Vec::new())
    }
}

fn should_fall_back_to_silence(error: &ScriptGeneratorError) -> bool {
    matches!(error, ScriptGeneratorError::InvalidOutput(_))
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaStatusView {
    pub configured: bool,
    pub health: Option<OllamaHealth>,
    pub message: String,
}

#[tauri::command]
pub async fn get_ollama_status(
    state: State<'_, AppState>,
) -> Result<OllamaStatusView, CommandError> {
    let settings = state.settings.read().await.clone();
    let Some(model) = settings.ollama_model.as_deref() else {
        return Ok(OllamaStatusView {
            configured: false,
            health: None,
            message: "Select a model to check Ollama".into(),
        });
    };
    let configuration = OllamaConfiguration::new(&settings.ollama_base_url, model)
        .map_err(|error| AppError::Configuration(error.to_string()))?;
    let generator = OllamaScriptGenerator::new(configuration)
        .map_err(|error| AppError::Configuration(error.to_string()))?;
    let health = generator
        .health_check()
        .await
        .map_err(|error| AppError::Provider(error.to_string()))?;
    let message = if health.model_installed {
        "Ollama is reachable and the selected model is installed"
    } else {
        "Ollama is reachable, but the selected model is not installed"
    };
    Ok(OllamaStatusView {
        configured: true,
        health: Some(health),
        message: message.into(),
    })
}

#[tauri::command]
pub async fn speak_test_segment(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SpeechResultView, CommandError> {
    let settings = state.settings.read().await.clone();
    let prepared = synthesize_recent_script(&app, &state, settings).await?;
    play_prepared_audio(&state, prepared)
        .await
        .map_err(Into::into)
}

pub(crate) async fn synthesize_recent_script(
    app: &AppHandle,
    state: &AppState,
    settings: AppSettings,
) -> Result<PreparedAudio, AppError> {
    let prepared = state
        .recent_script
        .read()
        .await
        .clone()
        .ok_or_else(|| AppError::Tts("generate a segment first".into()))?;
    let cancellation = state
        .coordinator
        .read()
        .await
        .cancellation_for(&prepared.job_id, &prepared.track_provider_id)?;
    let voice_settings = VoiceSettings {
        voice_id: match settings.tts_provider {
            TtsProviderSetting::ParlerMini => settings.parler_speaker.clone(),
            TtsProviderSetting::Mock | TtsProviderSetting::SherpaKokoro => {
                settings.tts_voice_id.to_string()
            }
        },
        rate: f32::from(settings.tts_speed_percent) / 100.0,
        volume: 1.0,
        delivery_style: delivery_style_for_segment(prepared.segment_type),
    };
    let spoken_dialogue = normalize_for_speech(&prepared.dialogue);
    if spoken_dialogue.is_empty() {
        return Err(AppError::Tts("script contains no speakable text".into()));
    }
    state
        .coordinator
        .write()
        .await
        .transition(BroadcastState::SynthesizingSpeech);
    let artifact = match settings.tts_provider {
        TtsProviderSetting::Mock => {
            MockTextToSpeechProvider
                .synthesize(&spoken_dialogue, &voice_settings, cancellation.clone())
                .await
        }
        TtsProviderSetting::SherpaKokoro => {
            let model_directory = settings.tts_model_directory.as_deref().ok_or_else(|| {
                AppError::Configuration("select a Kokoro model directory in Settings".into())
            })?;
            let configuration =
                SherpaKokoroConfiguration::new(model_directory, &tts_output_directory(app)?)
                    .map_err(|error| AppError::Configuration(error.to_string()))?;
            let provider =
                tokio::task::spawn_blocking(move || SherpaKokoroTtsProvider::new(configuration))
                    .await
                    .map_err(|error| AppError::Tts(error.to_string()))?
                    .map_err(|error| AppError::Tts(error.to_string()))?;
            provider
                .synthesize(&spoken_dialogue, &voice_settings, cancellation.clone())
                .await
        }
        TtsProviderSetting::ParlerMini => {
            let configuration = ParlerConfiguration::new(
                &settings.parler_base_url,
                &tts_output_directory(app)?,
                &settings.parler_speaker,
            )
            .map_err(|error| AppError::Configuration(error.to_string()))?;
            let provider = ParlerMiniTtsProvider::new(configuration)
                .map_err(|error| AppError::Configuration(error.to_string()))?;
            provider
                .synthesize(&spoken_dialogue, &voice_settings, cancellation.clone())
                .await
        }
    }
    .map_err(map_tts_error)?;
    state
        .coordinator
        .read()
        .await
        .ensure_current(&prepared.job_id, &prepared.track_provider_id)?;
    state
        .coordinator
        .write()
        .await
        .transition(BroadcastState::WaitingForTransition);
    Ok(PreparedAudio {
        artifact,
        cancellation,
        job_id: prepared.job_id,
        track_provider_id: prepared.track_provider_id,
        provider: settings.tts_provider,
    })
}

pub(crate) async fn play_prepared_audio(
    state: &AppState,
    prepared: PreparedAudio,
) -> Result<SpeechResultView, AppError> {
    state
        .coordinator
        .read()
        .await
        .ensure_current(&prepared.job_id, &prepared.track_provider_id)?;
    state
        .coordinator
        .write()
        .await
        .transition(BroadcastState::Speaking);
    state
        .audio
        .play(&prepared.artifact, prepared.cancellation)
        .await
        .map_err(map_audio_error)?;
    state.audio.stop().await.map_err(map_audio_error)?;
    state
        .coordinator
        .write()
        .await
        .transition(BroadcastState::Monitoring);
    Ok(SpeechResultView {
        artifact_id: prepared.artifact.artifact_id,
        duration_ms: prepared.artifact.duration_ms,
        is_mock: prepared.artifact.is_mock,
        message: if prepared.artifact.is_mock {
            "Mock speech completed; no sound was produced.".into()
        } else {
            match prepared.provider {
                TtsProviderSetting::ParlerMini => {
                    "Parler Mini speech played on the default audio device.".into()
                }
                TtsProviderSetting::Mock | TtsProviderSetting::SherpaKokoro => {
                    "Kokoro speech played on the default audio device.".into()
                }
            }
        },
    })
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsStatusView {
    pub configured: bool,
    pub health: Option<TtsHealthView>,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsHealthView {
    pub ready: bool,
    pub provider: String,
    pub sample_rate: Option<u32>,
    pub available_voices: Option<u16>,
    pub model: Option<String>,
    pub speaker: Option<String>,
}

#[tauri::command]
pub async fn get_tts_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TtsStatusView, CommandError> {
    let settings = state.settings.read().await.clone();
    match settings.tts_provider {
        TtsProviderSetting::Mock => Ok(TtsStatusView {
            configured: false,
            health: None,
            message: "Mock TTS is active".into(),
        }),
        TtsProviderSetting::SherpaKokoro => {
            let model_directory = settings.tts_model_directory.as_deref().ok_or_else(|| {
                AppError::Configuration("select a Kokoro model directory in Settings".into())
            })?;
            let configuration =
                SherpaKokoroConfiguration::new(model_directory, &tts_output_directory(&app)?)
                    .map_err(|error| AppError::Configuration(error.to_string()))?;
            let health: SherpaTtsHealth = tokio::task::spawn_blocking(move || {
                let provider = SherpaKokoroTtsProvider::new(configuration)?;
                provider.health()
            })
            .await
            .map_err(|error| AppError::Tts(error.to_string()))?
            .map_err(|error| AppError::Tts(error.to_string()))?;
            if settings.tts_voice_id >= health.available_voices {
                return Err(AppError::Configuration(format!(
                    "voice ID must be below {} for this model",
                    health.available_voices
                ))
                .into());
            }
            Ok(TtsStatusView {
                configured: true,
                health: Some(TtsHealthView {
                    ready: health.ready,
                    provider: "sherpa_kokoro".into(),
                    sample_rate: Some(health.sample_rate),
                    available_voices: Some(health.available_voices),
                    model: Some("kokoro-en-v0_19".into()),
                    speaker: Some(settings.tts_voice_id.to_string()),
                }),
                message: "Sherpa-ONNX Kokoro model is ready".into(),
            })
        }
        TtsProviderSetting::ParlerMini => {
            let configuration = ParlerConfiguration::new(
                &settings.parler_base_url,
                &tts_output_directory(&app)?,
                &settings.parler_speaker,
            )
            .map_err(|error| AppError::Configuration(error.to_string()))?;
            let health = ParlerMiniTtsProvider::new(configuration)
                .map_err(|error| AppError::Configuration(error.to_string()))?
                .health_check()
                .await
                .map_err(map_tts_error)?;
            let available_voices = u16::try_from(health.speakers.len()).ok();
            Ok(TtsStatusView {
                configured: true,
                health: Some(TtsHealthView {
                    ready: health.ready,
                    provider: health.provider,
                    sample_rate: Some(health.sample_rate),
                    available_voices,
                    model: Some(health.model),
                    speaker: Some(settings.parler_speaker),
                }),
                message: "Parler Mini is loaded and ready on the local service".into(),
            })
        }
    }
}

fn tts_output_directory(app: &AppHandle) -> Result<std::path::PathBuf, AppError> {
    app.path()
        .app_cache_dir()
        .map(|path| path.join("tts"))
        .map_err(|error| AppError::Configuration(error.to_string()))
}

fn map_tts_error(error: TtsError) -> AppError {
    match error {
        TtsError::Cancelled => AppError::Cancelled,
        other => AppError::Tts(other.to_string()),
    }
}

fn map_audio_error(error: AudioPlayerError) -> AppError {
    match error {
        AudioPlayerError::Cancelled => AppError::Cancelled,
        other => AppError::Audio(other.to_string()),
    }
}

fn delivery_style_for_segment(segment_type: SegmentType) -> DeliveryStyle {
    match segment_type {
        SegmentType::NextSongTease
        | SegmentType::OneLineReaction
        | SegmentType::SimpleTransition => DeliveryStyle::Energetic,
        SegmentType::ShortJoke => DeliveryStyle::Playful,
        SegmentType::ListenerObservation => DeliveryStyle::Warm,
        SegmentType::RecordingStory
        | SegmentType::ArtistStory
        | SegmentType::SongInterpretation
        | SegmentType::CulturalContext
        | SegmentType::MusicHistoryConnection
        | SegmentType::StationLore => DeliveryStyle::Reflective,
        SegmentType::StationIdentification => DeliveryStyle::Authoritative,
        SegmentType::Silence | SegmentType::FunFact => DeliveryStyle::Neutral,
    }
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, CommandError> {
    Ok(state.settings.read().await.clone())
}

#[tauri::command]
pub async fn save_settings(
    mut settings: AppSettings,
    state: State<'_, AppState>,
) -> Result<AppSettings, CommandError> {
    settings.normalize_user_input();
    settings.validate()?;
    if !settings.mock_mode {
        let status = state
            .spotify_auth
            .status(settings.spotify_client_id.is_some())
            .await;
        if !status.connected {
            return Err(AppError::Configuration(
                "connect Spotify before enabling live playback".into(),
            )
            .into());
        }
    }
    state.database.repository().save_settings(&settings).await?;
    *state.settings.write().await = settings.clone();
    Ok(settings)
}

pub(crate) fn spotify_provider(
    settings: &AppSettings,
    credential_store: Arc<dyn CredentialStore>,
) -> Result<SpotifyProvider, AppError> {
    let client_id = settings.spotify_client_id.clone().ok_or_else(|| {
        AppError::Configuration("Spotify Client ID is required for live playback".into())
    })?;
    SpotifyProvider::new(
        SpotifyConfiguration {
            client_id,
            redirect_uri: settings.spotify_redirect_uri.clone(),
        },
        credential_store,
    )
    .map_err(|error| AppError::Provider(error.to_string()))
}

#[tauri::command]
pub async fn get_spotify_connection(
    state: State<'_, AppState>,
) -> Result<SpotifyConnectionStatus, CommandError> {
    let configured = state
        .settings
        .read()
        .await
        .spotify_client_id
        .as_ref()
        .is_some_and(|client_id| !client_id.trim().is_empty());
    Ok(state.spotify_auth.status(configured).await)
}

#[tauri::command]
pub async fn connect_spotify(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SpotifyConnectionStatus, CommandError> {
    let settings = state.settings.read().await.clone();
    let configuration = SpotifyConfiguration {
        client_id: settings.spotify_client_id.ok_or_else(|| {
            AppError::Configuration("Spotify Client ID must be saved before connecting".into())
        })?,
        redirect_uri: settings.spotify_redirect_uri,
    };
    state
        .spotify_auth
        .connect(&app, &configuration)
        .await
        .map_err(|error| AppError::Authentication(error.to_string()).into())
}

#[tauri::command]
pub async fn disconnect_spotify(
    state: State<'_, AppState>,
) -> Result<SpotifyConnectionStatus, CommandError> {
    let configured = state
        .settings
        .read()
        .await
        .spotify_client_id
        .as_ref()
        .is_some_and(|client_id| !client_id.trim().is_empty());
    state
        .spotify_auth
        .disconnect(configured)
        .await
        .map_err(|error| AppError::Authentication(error.to_string()).into())
}

fn mock_playback() -> PlaybackState {
    PlaybackState {
        current_track: Some(Track {
            provider_id: "mock-current-001".into(),
            title: "Glass Satellites".into(),
            artists: vec![Artist {
                provider_id: Some("mock-artist-01".into()),
                name: "Harbour Static".into(),
            }],
            album: Some(Album {
                provider_id: Some("mock-album-01".into()),
                title: "Signals After Rain".into(),
                release_date: NaiveDate::from_ymd_opt(2024, 10, 18),
                artwork_url: None,
            }),
            duration_ms: 238_000,
            isrc: None,
            release_date: NaiveDate::from_ymd_opt(2024, 10, 18),
            explicit: false,
            variant: TrackVariant::Studio,
            artwork_url: None,
        }),
        next_track: Some(Track {
            provider_id: "mock-next-002".into(),
            title: "Lanterns on the Flyover".into(),
            artists: vec![
                Artist {
                    provider_id: Some("mock-artist-02".into()),
                    name: "June Meridian".into(),
                },
                Artist {
                    provider_id: Some("mock-artist-03".into()),
                    name: "Tariq North".into(),
                },
            ],
            album: Some(Album {
                provider_id: Some("mock-album-02".into()),
                title: "City Weather".into(),
                release_date: None,
                artwork_url: None,
            }),
            duration_ms: 204_000,
            isrc: None,
            release_date: None,
            explicit: false,
            variant: TrackVariant::Unknown,
            artwork_url: None,
        }),
        progress_ms: 184_000,
        is_playing: true,
        device: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        delivery_style_for_segment, should_fall_back_to_silence, unattended_fact_fallback,
    };
    use crate::{
        errors::AppError,
        llm::{ScriptGeneratorError, ScriptOutputError},
        music_facts::MusicFactError,
        rj_engine::{SegmentType, ValidationIssue},
        tts::DeliveryStyle,
    };

    #[test]
    fn segment_types_select_bounded_delivery_intent() {
        assert_eq!(
            delivery_style_for_segment(SegmentType::ShortJoke),
            DeliveryStyle::Playful
        );
        assert_eq!(
            delivery_style_for_segment(SegmentType::NextSongTease),
            DeliveryStyle::Energetic
        );
        assert_eq!(
            delivery_style_for_segment(SegmentType::OneLineReaction),
            DeliveryStyle::Energetic
        );
        assert_eq!(
            delivery_style_for_segment(SegmentType::SimpleTransition),
            DeliveryStyle::Energetic
        );
        assert_eq!(
            delivery_style_for_segment(SegmentType::StationLore),
            DeliveryStyle::Reflective
        );
        assert_eq!(
            delivery_style_for_segment(SegmentType::StationIdentification),
            DeliveryStyle::Authoritative
        );
    }

    #[test]
    fn unattended_fact_failures_fall_back_but_cancellation_stops() {
        assert!(unattended_fact_fallback(MusicFactError::RateLimited)
            .unwrap_or_else(|error| panic!("rate limit should fall back: {error}"))
            .is_empty());
        assert!(matches!(
            unattended_fact_fallback(MusicFactError::Cancelled),
            Err(AppError::Cancelled)
        ));
    }

    #[test]
    fn invalid_model_output_falls_back_to_silence_but_provider_failures_do_not() {
        assert!(should_fall_back_to_silence(
            &ScriptGeneratorError::InvalidOutput(ScriptOutputError::Dialogue {
                issues: vec![ValidationIssue::TooLong],
            })
        ));
        assert!(!should_fall_back_to_silence(
            &ScriptGeneratorError::Unavailable
        ));
    }
}
