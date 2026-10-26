//! Signing in: local accounts with argon2-hashed passwords, sessions held in a cookie, and the
//! extractors handlers use to require a user or an admin. Every API route except the ones here
//! sits behind `require_session`.

use std::{
    sync::LazyLock,
    time::{SystemTime, UNIX_EPOCH},
};

use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{
        SaltString,
        rand_core::{OsRng, RngCore},
    },
};
use axum::{
    Json, Router,
    extract::{FromRequestParts, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header, request::Parts},
    middleware::Next,
    response::{AppendHeaders, IntoResponse, Response},
    routing::{get, post},
};
use oxilabel_core::user::{Credentials, MIN_PASSWORD_LENGTH, Role, Session, User};
use sqlx::{Row, SqlitePool, sqlite::SqliteRow};

use crate::{AppState, error::ApiError};

const COOKIE: &str = "oxilabel_session";
/// How long a login lasts.
const SESSION_SECONDS: i64 = 30 * 24 * 60 * 60;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/session", get(session))
        .route("/api/setup", post(setup))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
}

/// The signed-in user of a request that passed `require_session`.
#[derive(Clone)]
pub struct CurrentUser {
    pub user: User,
    /// Identifies the session the request came with.
    token_hash: String,
}

impl CurrentUser {
    pub fn token_hash(&self) -> &str {
        &self.token_hash
    }
}

impl<S: Send + Sync> FromRequestParts<S> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, ApiError> {
        parts.extensions.get::<CurrentUser>().cloned().ok_or_else(not_signed_in)
    }
}

/// A signed-in admin; any other user is refused.
pub struct Admin(pub CurrentUser);

impl<S: Send + Sync> FromRequestParts<S> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        let current = CurrentUser::from_request_parts(parts, state).await?;
        match current.user.role {
            Role::Admin => Ok(Admin(current)),
            Role::Annotator => Err(ApiError::Forbidden("only admins may do this".to_owned())),
        }
    }
}

/// Rejects requests without a valid session; the others carry their `CurrentUser`.
pub async fn require_session(State(pool): State<SqlitePool>, mut request: Request, next: Next) -> Result<Response, ApiError> {
    let current = signed_in(&pool, request.headers()).await?.ok_or_else(not_signed_in)?;
    request.extensions_mut().insert(current);
    Ok(next.run(request).await)
}

fn not_signed_in() -> ApiError {
    ApiError::Unauthorized("sign in first".to_owned())
}

async fn signed_in(pool: &SqlitePool, headers: &HeaderMap) -> Result<Option<CurrentUser>, ApiError> {
    let Some(token) = session_token(headers) else { return Ok(None) };
    let token_hash = hash_token(token);
    let row = sqlx::query(
        "SELECT users.id, users.username, users.role FROM sessions JOIN users ON users.id = sessions.user_id
         WHERE sessions.token_hash = ? AND sessions.expires_at > ?",
    )
    .bind(&token_hash)
    .bind(now())
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| CurrentUser { user: user_from_row(&row), token_hash }))
}

fn session_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| pair.trim().strip_prefix(COOKIE)?.strip_prefix('='))
}

fn hash_token(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs() as i64)
}

/// A row of `users` with at least `id`, `username` and `role`.
pub fn user_from_row(row: &SqliteRow) -> User {
    User {
        id: row.get("id"),
        username: row.get("username"),
        role: Role::parse(row.get("role")).expect("the schema allows only known roles"),
    }
}

/// A new account's trimmed username, unless it is blank, and its password's hash, unless the
/// password is too short.
pub async fn new_account(username: &str, password: String) -> Result<(String, String), ApiError> {
    let username = username.trim();
    if username.is_empty() {
        return Err(ApiError::BadRequest("the username must not be blank".to_owned()));
    }
    Ok((username.to_owned(), hash_password(password).await?))
}

fn check_password(password: &str) -> Result<(), ApiError> {
    if password.chars().count() < MIN_PASSWORD_LENGTH {
        return Err(ApiError::BadRequest(format!("the password needs at least {MIN_PASSWORD_LENGTH} characters")));
    }
    Ok(())
}

/// Hashes a password with argon2 off the async threads (it takes a while on purpose), unless it
/// is too short.
pub async fn hash_password(password: String) -> Result<String, ApiError> {
    check_password(&password)?;
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default().hash_password(password.as_bytes(), &salt).map(|hash| hash.to_string())
    })
    .await
    .map_err(|err| ApiError::Internal(err.to_string()))?
    .map_err(|err| ApiError::Internal(err.to_string()))
}

/// Compared against when the username is unknown, so that a wrong name takes as long to refuse as
/// a wrong password and does not reveal which names exist.
pub static UNKNOWN_USER_HASH: LazyLock<String> = LazyLock::new(|| {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default().hash_password(b"no such user", &salt).expect("hashing a constant").to_string()
});

async fn verify_password(password: String, hash: Option<String>) -> Result<bool, ApiError> {
    tokio::task::spawn_blocking(move || {
        let known = hash.is_some();
        let hash = hash.unwrap_or_else(|| UNKNOWN_USER_HASH.clone());
        let matches = PasswordHash::new(&hash).is_ok_and(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok());
        known && matches
    })
    .await
    .map_err(|err| ApiError::Internal(err.to_string()))
}

/// Starts a session for `user` and returns the cookie that carries it.
async fn start_session(pool: &SqlitePool, user: i64) -> Result<HeaderValue, ApiError> {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    let now = now();
    sqlx::query("DELETE FROM sessions WHERE expires_at <= ?").bind(now).execute(pool).await?;
    sqlx::query("INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?, ?, ?)")
        .bind(hash_token(&token))
        .bind(user)
        .bind(now + SESSION_SECONDS)
        .execute(pool)
        .await?;
    Ok(cookie(&token, SESSION_SECONDS))
}

/// The session cookie: not readable by scripts, and never sent with requests from other sites.
fn cookie(token: &str, max_age: i64) -> HeaderValue {
    HeaderValue::from_str(&format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}")).expect("a valid cookie")
}

async fn session(State(pool): State<SqlitePool>, headers: HeaderMap) -> Result<Json<Session>, ApiError> {
    let user = signed_in(&pool, &headers).await?.map(|current| current.user);
    let setup = sqlx::query("SELECT 1 FROM users LIMIT 1").fetch_optional(&pool).await?.is_none();
    Ok(Json(Session { setup, user }))
}

/// Creates the first account, an admin, and signs it in; refused once any account exists.
async fn setup(State(pool): State<SqlitePool>, Json(credentials): Json<Credentials>) -> Result<Response, ApiError> {
    let (username, hash) = new_account(&credentials.username, credentials.password).await?;
    // One statement, so two people setting up at once cannot both become the first admin.
    let id = sqlx::query("INSERT INTO users (username, password_hash, role) SELECT ?, ?, 'admin' WHERE NOT EXISTS (SELECT 1 FROM users) RETURNING id")
        .bind(&username)
        .bind(hash)
        .fetch_optional(&pool)
        .await?
        .ok_or_else(|| ApiError::Conflict("this server already has accounts; sign in instead".to_owned()))?
        .get("id");
    let cookie = start_session(&pool, id).await?;
    let user = User { id, username, role: Role::Admin };
    Ok((StatusCode::CREATED, AppendHeaders([(header::SET_COOKIE, cookie)]), Json(user)).into_response())
}

async fn login(State(pool): State<SqlitePool>, Json(credentials): Json<Credentials>) -> Result<Response, ApiError> {
    let row = sqlx::query("SELECT id, username, role, password_hash FROM users WHERE username = ?")
        .bind(credentials.username.trim())
        .fetch_optional(&pool)
        .await?;
    let hash = row.as_ref().map(|row| row.get::<String, _>("password_hash"));
    if !verify_password(credentials.password, hash).await? {
        return Err(ApiError::Unauthorized("wrong username or password".to_owned()));
    }
    let user = user_from_row(&row.expect("a verified password belongs to a user"));
    let cookie = start_session(&pool, user.id).await?;
    Ok((AppendHeaders([(header::SET_COOKIE, cookie)]), Json(user)).into_response())
}

/// Ends this browser's session, if it has one.
async fn logout(State(pool): State<SqlitePool>, headers: HeaderMap) -> Result<Response, ApiError> {
    if let Some(token) = session_token(&headers) {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?").bind(hash_token(token)).execute(&pool).await?;
    }
    Ok((StatusCode::NO_CONTENT, AppendHeaders([(header::SET_COOKIE, cookie("", 0))])).into_response())
}
