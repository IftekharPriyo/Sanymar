mod repository;

use std::{str::FromStr, time::Duration};

use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    SqlitePool,
};
use tauri::{AppHandle, Manager};

use crate::errors::AppError;

pub use repository::{GeneratedScriptRecord, SanymarRepository};

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn open(app: &AppHandle) -> Result<Self, AppError> {
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|error| AppError::Database(error.to_string()))?;
        std::fs::create_dir_all(&data_dir)?;
        let database_path = data_dir.join("sanymar.sqlite3");
        let url = format!(
            "sqlite://{}",
            database_path.to_string_lossy().replace('\\', "/")
        );
        Self::connect(&url).await
    }

    pub async fn connect(url: &str) -> Result<Self, AppError> {
        let options = SqliteConnectOptions::from_str(url)
            .map_err(|error| AppError::Database(error.to_string()))?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let maximum_connections = if url.contains(":memory:") { 1 } else { 5 };
        let pool = SqlitePoolOptions::new()
            .max_connections(maximum_connections)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|error| AppError::Database(error.to_string()))?;
        Ok(Self { pool })
    }

    pub fn repository(&self) -> SanymarRepository {
        SanymarRepository::new(self.pool.clone())
    }
}
