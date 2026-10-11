mod error;
mod labels;
mod projects;

use std::{str::FromStr, time::Duration};

use axum::Router;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

pub async fn app(database_url: &str) -> Result<Router, sqlx::Error> {
    let mut options = SqlitePoolOptions::new();
    if database_url.contains(":memory:") {
        // Each connection to `sqlite::memory:` is a separate database, so tests must share one.
        options = options.max_connections(1);
    }
    let connect = SqliteConnectOptions::from_str(database_url)?
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(true);
    let pool = options.connect_with(connect).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(Router::new()
        .merge(projects::routes())
        .merge(labels::routes())
        .with_state(pool))
}
