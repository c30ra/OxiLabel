//! The admin screen's accounts: listing them, creating annotators and admins, changing roles and
//! resetting passwords.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, put},
};
use oxilabel_core::user::{NewUser, User, UserChange};
use sqlx::SqlitePool;

use crate::{
    AppState,
    auth::{Admin, hash_password, new_account, user_from_row},
    error::ApiError,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/users", get(list_users).post(create_user))
        .route("/api/users/{id}", put(change_user))
}

async fn list_users(_: Admin, State(pool): State<SqlitePool>) -> Result<Json<Vec<User>>, ApiError> {
    let rows = sqlx::query("SELECT id, username, role FROM users ORDER BY id").fetch_all(&pool).await?;
    Ok(Json(rows.iter().map(user_from_row).collect()))
}

async fn create_user(_: Admin, State(pool): State<SqlitePool>, Json(new): Json<NewUser>) -> Result<(StatusCode, Json<User>), ApiError> {
    let (username, hash) = new_account(&new.username, new.password).await?;
    let id = sqlx::query_scalar("INSERT INTO users (username, password_hash, role) VALUES (?, ?, ?) RETURNING id")
        .bind(&username)
        .bind(hash)
        .bind(new.role.as_str())
        .fetch_one(&pool)
        .await
        .map_err(|err| match err.as_database_error() {
            Some(db) if db.is_unique_violation() => ApiError::Conflict(format!("there is already a user named \"{username}\"")),
            _ => err.into(),
        })?;
    Ok((StatusCode::CREATED, Json(User { id, username, role: new.role })))
}

/// Changes a role and/or resets a password. A reset signs the user out everywhere, except the
/// admin's own current session when they reset their own password.
async fn change_user(
    Admin(current): Admin,
    State(pool): State<SqlitePool>,
    Path(id): Path<i64>,
    Json(change): Json<UserChange>,
) -> Result<Json<User>, ApiError> {
    let exists = sqlx::query("SELECT 1 FROM users WHERE id = ?").bind(id).fetch_optional(&pool).await?.is_some();
    if !exists {
        return Err(ApiError::NotFound);
    }
    let hash = match change.password {
        Some(password) => Some(hash_password(password).await?),
        None => None,
    };
    // Writes come before reads, so a concurrent change waits for this one instead of failing.
    let mut tx = pool.begin().await?;
    if let Some(role) = change.role {
        // The count is part of the update, so the last two admins cannot both step down at once.
        let changed = sqlx::query(
            "UPDATE users SET role = ?1 WHERE id = ?2
             AND (?1 = 'admin' OR role <> 'admin' OR (SELECT COUNT(*) FROM users WHERE role = 'admin') > 1)",
        )
        .bind(role.as_str())
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed == 0 {
            return Err(ApiError::Conflict("the last admin must stay an admin".to_owned()));
        }
    }
    if let Some(hash) = hash {
        sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?").bind(hash).bind(id).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM sessions WHERE user_id = ? AND token_hash <> ?")
            .bind(id)
            .bind(current.token_hash())
            .execute(&mut *tx)
            .await?;
    }
    let row = sqlx::query("SELECT id, username, role FROM users WHERE id = ?").bind(id).fetch_optional(&mut *tx).await?.ok_or(ApiError::NotFound)?;
    tx.commit().await?;
    Ok(Json(user_from_row(&row)))
}
