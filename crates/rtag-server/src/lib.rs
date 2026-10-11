mod error;
mod images;
mod labels;
mod projects;
mod thumbnails;
mod tiles;

use std::{path::PathBuf, str::FromStr, time::Duration};

use axum::{Router, extract::FromRef};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

pub struct Config {
    pub database_url: String,
    /// Images can only be imported from inside this folder.
    pub images_root: PathBuf,
    /// Generated thumbnails and tiles live here; safe to delete.
    pub cache_dir: PathBuf,
}

#[derive(Clone)]
struct AppState {
    pool: SqlitePool,
    images_root: PathBuf,
    cache_dir: PathBuf,
    pyramid_locks: tiles::GenerationLocks,
}

impl FromRef<AppState> for SqlitePool {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}

pub async fn app(config: Config) -> Result<Router, sqlx::Error> {
    let mut options = SqlitePoolOptions::new();
    if config.database_url.contains(":memory:") {
        // Each connection to `sqlite::memory:` is a separate database, so tests must share one.
        options = options.max_connections(1);
    }
    let connect = SqliteConnectOptions::from_str(&config.database_url)?
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(true)
        .with_regexp();
    let pool = options.connect_with(connect).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    tokio::spawn(images::backfill_file_info(pool.clone(), config.images_root.clone()));
    let state = AppState {
        pool,
        images_root: config.images_root,
        cache_dir: config.cache_dir,
        pyramid_locks: tiles::GenerationLocks::default(),
    };
    Ok(Router::new()
        .merge(projects::routes())
        .merge(labels::routes())
        .merge(images::routes())
        .merge(thumbnails::routes())
        .merge(tiles::routes())
        .with_state(state))
}
