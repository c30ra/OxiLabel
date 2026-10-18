mod annotations;
mod archives;
mod auth;
mod cvat;
pub mod enhance;
mod error;
mod exports;
mod host;
mod images;
mod inference;
mod jobs;
mod labels;
mod projects;
mod thumbnails;
mod tiles;
mod user_settings;
mod users;
mod worker_process;

pub use worker_process::stop_workers;

pub use tiles::KEPT_FILTERS;

use std::{
    fs,
    future::Future,
    io::{self, Write},
    path::{Path, PathBuf},
    pin::Pin,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use axum::{Router, extract::FromRef, middleware};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

/// Opens the desktop's folder dialog on the server's machine: the chosen folder, or none if it
/// was cancelled.
pub type FolderDialog = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Result<Option<PathBuf>, String>> + Send>> + Send + Sync>;

pub struct Config {
    pub database_url: String,
    /// The folder the server's own folder browser shows; requests from the server's machine may
    /// also import any folder by its absolute path (ADR 0003).
    pub images_root: PathBuf,
    /// Images uploaded from other machines are kept here.
    pub upload_dir: PathBuf,
    /// How to show the desktop's folder dialog, if the server runs on a desktop.
    pub folder_dialog: Option<FolderDialog>,
    /// Generated thumbnails and tiles live here; safe to delete.
    pub cache_dir: PathBuf,
    /// Exports are written into folders here.
    pub export_dir: PathBuf,
}

#[derive(Clone)]
struct AppState {
    pool: SqlitePool,
    images_root: PathBuf,
    upload_dir: PathBuf,
    folder_dialog: Option<FolderDialog>,
    cache_dir: PathBuf,
    export_dir: PathBuf,
    pyramid_locks: tiles::GenerationLocks,
    worker: inference::Worker,
    runs: inference::Runs,
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
    // Ready before the first login with an unknown name, which must take no longer than others.
    tokio::task::spawn_blocking(|| std::sync::LazyLock::force(&auth::UNKNOWN_USER_HASH));
    let state = AppState {
        pool,
        images_root: config.images_root,
        upload_dir: config.upload_dir,
        folder_dialog: config.folder_dialog,
        cache_dir: config.cache_dir,
        export_dir: config.export_dir,
        pyramid_locks: tiles::GenerationLocks::default(),
        worker: inference::Worker::default(),
        runs: inference::Runs::default(),
    };
    let signed_in = Router::new()
        .merge(projects::routes())
        .merge(host::routes())
        .merge(labels::routes())
        .merge(images::routes())
        .merge(annotations::routes())
        .merge(exports::routes())
        .merge(thumbnails::routes())
        .merge(tiles::routes())
        .merge(users::routes())
        .merge(jobs::routes())
        .merge(cvat::routes())
        .merge(archives::routes())
        .merge(user_settings::routes())
        .merge(inference::routes())
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::require_session));
    // The worker as an admin set it up (started now, if the server runs it).
    state.worker.apply(&inference::stored_worker_settings(&state.pool).await.map_err(|err| match err {
        error::ApiError::Database(err) => err,
        other => sqlx::Error::Protocol(other.message()),
    })?);
    Ok(Router::new().merge(signed_in).merge(auth::routes()).with_state(state))
}

/// Writes `path` through a temporary file in the same folder, so a concurrent reader never sees
/// a partial file.
pub(crate) fn write_atomically_to(path: &Path, write: impl FnOnce(&mut fs::File) -> io::Result<()>) -> io::Result<()> {
    let dir = path.parent().expect("written files have a parent folder");
    fs::create_dir_all(dir)?;
    let mut temporary = tempfile::NamedTempFile::new_in(dir)?;
    write(temporary.as_file_mut())?;
    temporary.persist(path).map(|_| ()).map_err(|err| err.error)
}

pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomically_to(path, |file| file.write_all(bytes))
}
