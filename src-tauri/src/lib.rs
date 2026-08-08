pub mod audio;
pub mod broadcast;
pub mod commands;
pub mod database;
pub mod errors;
pub mod llm;
pub mod music_facts;
pub mod music_provider;
pub mod playback;
pub mod rj_engine;
pub mod security;
pub mod settings;
pub mod spotify;
pub mod tts;

use std::sync::Arc;

use commands::AppState;
use database::Database;
use rj_engine::DjProfile;
use security::windows::WindowsCredentialStore;
use settings::AppSettings;
use spotify::auth::SpotifyAuthService;
use tauri::Manager;
use tracing_subscriber::EnvFilter;

pub fn run() {
    init_logging();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let (database, settings) = tauri::async_runtime::block_on(async {
                let database = Database::open(app.handle()).await?;
                let repository = database.repository();
                let mut settings = repository
                    .load_settings()
                    .await?
                    .unwrap_or_else(AppSettings::default);
                settings.normalize_legacy_values();
                repository.save_settings(&settings).await?;
                repository.save_profile(&DjProfile::default()).await?;
                Ok::<_, errors::AppError>((database, settings))
            })?;
            let credential_store: Arc<dyn security::CredentialStore> =
                Arc::new(WindowsCredentialStore);
            let spotify_auth = SpotifyAuthService::new(credential_store.clone())
                .map_err(|error| errors::AppError::Authentication(error.to_string()))?;
            app.manage(AppState::new(
                Arc::new(database),
                settings,
                spotify_auth,
                credential_store,
            ));
            tauri::async_runtime::spawn(broadcast::run_transition_automation(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_dashboard,
            commands::generate_test_segment,
            commands::speak_test_segment,
            commands::get_settings,
            commands::save_settings,
            commands::get_ollama_status,
            commands::get_groq_status,
            commands::save_groq_api_key,
            commands::delete_groq_api_key,
            commands::get_tts_status,
            commands::get_spotify_connection,
            commands::connect_spotify,
            commands::disconnect_spotify,
        ]);

    if let Err(error) = builder.run(tauri::generate_context!()) {
        tracing::error!(error = %error, "tauri runtime failed");
    }
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .try_init();
}
