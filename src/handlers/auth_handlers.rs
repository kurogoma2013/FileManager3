use crate::auth;
use crate::config::{google_oauth_config, GoogleOAuthConfig};
use crate::core::can_delete_permanently;
use crate::db::DbPool;
use crate::models::{ApiError, AppState, SESSION_COOKIE_NAME, SESSION_MAX_AGE_SECONDS};
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use webauthn_rs::prelude::*;

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct GoogleCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct GoogleUserInfo {
    sub: String,
    email: String,
    email_verified: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct PasskeyChallengeResponse<T> {
    pub challenge_id: String,
    pub public_key: T,
}

#[derive(Debug, Deserialize)]
pub struct PasskeyRegisterFinishRequest {
    pub challenge_id: String,
    pub credential: RegisterPublicKeyCredential,
}

#[derive(Debug, Deserialize)]
pub struct PasskeyLoginStartRequest {
    pub username: String,
}

#[derive(Debug, Deserialize)]
pub struct PasskeyLoginFinishRequest {
    pub challenge_id: String,
    pub credential: PublicKeyCredential,
}

#[derive(Debug, Serialize)]
pub struct UserMeResponse {
    pub id: i64,
    pub username: String,
    pub role: String,
    pub session_expires_at: String,
}

#[derive(Debug, Serialize)]
pub struct AccessUrlsResponse {
    pub local_url: Option<String>,
    pub lan_url: Option<String>,
    pub mobile_url: Option<String>,
    pub urls: Vec<String>,
}

pub fn session_token(headers: &HeaderMap) -> Option<String> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    cookie.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name == SESSION_COOKIE_NAME && !value.is_empty()).then(|| value.to_string())
    })
}

pub fn hash_session_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn session_cookie(token: &str, secure: bool) -> String {
    let secure_attr = if secure { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={SESSION_MAX_AGE_SECONDS}{secure_attr}"
    )
}

pub fn clear_session_cookie(secure: bool) -> String {
    let secure_attr = if secure { "; Secure" } else { "" };
    format!("{SESSION_COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure_attr}")
}

pub async fn cleanup_expired_auth_state(pool: &DbPool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM sessions WHERE expires_at <= CURRENT_TIMESTAMP")
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM passkey_challenges WHERE expires_at <= CURRENT_TIMESTAMP")
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM auth_login_attempts WHERE window_started_at < CURRENT_TIMESTAMP - INTERVAL '15 minutes' AND (blocked_until IS NULL OR blocked_until <= CURRENT_TIMESTAMP)")
        .execute(pool)
        .await?;
    sqlx::query(
        "DELETE FROM oauth_states WHERE created_at <= CURRENT_TIMESTAMP - INTERVAL '10 minutes'",
    )
    .execute(pool)
    .await?;
    Ok(())
}

const LOGIN_ATTEMPT_LIMIT: i64 = 5;

async fn check_login_rate_limit(pool: &DbPool, username: &str) -> Result<(), ApiError> {
    let blocked: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM auth_login_attempts WHERE username = $1 AND blocked_until > CURRENT_TIMESTAMP",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?;
    if blocked.is_some() {
        return Err(ApiError::Unauthorized);
    }
    Ok(())
}

async fn record_login_failure(pool: &DbPool, username: &str) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO auth_login_attempts (username, attempts, window_started_at, blocked_until)
         VALUES ($1, 1, CURRENT_TIMESTAMP, NULL)
         ON CONFLICT(username) DO UPDATE SET
           attempts = CASE
             WHEN auth_login_attempts.window_started_at <= CURRENT_TIMESTAMP - INTERVAL '15 minutes' THEN 1
             ELSE auth_login_attempts.attempts + 1
           END,
           window_started_at = CASE
             WHEN auth_login_attempts.window_started_at <= CURRENT_TIMESTAMP - INTERVAL '15 minutes' THEN CURRENT_TIMESTAMP
             ELSE auth_login_attempts.window_started_at
           END,
           blocked_until = CASE
             WHEN auth_login_attempts.window_started_at > CURRENT_TIMESTAMP - INTERVAL '15 minutes'
                  AND auth_login_attempts.attempts + 1 >= $2 THEN CURRENT_TIMESTAMP + INTERVAL '15 minutes'
             ELSE NULL
           END",
    )
    .bind(username)
    .bind(LOGIN_ATTEMPT_LIMIT)
    .execute(pool)
    .await?;
    Ok(())
}

async fn clear_login_failures(pool: &DbPool, username: &str) -> Result<(), ApiError> {
    sqlx::query("DELETE FROM auth_login_attempts WHERE username = $1")
        .bind(username)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn authenticate_headers(
    pool: &DbPool,
    headers: &HeaderMap,
) -> anyhow::Result<Option<(i64, String)>> {
    let Some(token) = session_token(headers) else {
        return Ok(None);
    };
    let token_hash = hash_session_token(&token);
    sqlx::query_as::<_, (i64, String)>(
        "SELECT u.id, u.role
         FROM sessions s
         JOIN users u ON u.id = s.user_id
         WHERE s.token_hash = $1 AND s.expires_at > CURRENT_TIMESTAMP AND u.active",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

pub async fn authenticate_request(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(i64, String), ApiError> {
    authenticate_headers(&state.pool, headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)
}

pub async fn authenticate_admin(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(i64, String), ApiError> {
    let user = authenticate_request(state, headers).await?;
    if !can_delete_permanently(&user.1) {
        return Err(ApiError::Forbidden);
    }
    Ok(user)
}

pub async fn require_project_access(
    headers: &HeaderMap,
    state: &AppState,
    project_id: i64,
) -> Result<(), ApiError> {
    authenticate_request(state, headers).await?;
    let project_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM projects WHERE id = $1 AND deleted_at IS NULL")
            .bind(project_id)
            .fetch_optional(&state.pool)
            .await?;
    if project_exists.is_none() {
        return Err(ApiError::NotFound);
    }
    Ok(())
}

pub async fn create_session_response(state: &AppState, user_id: i64) -> Result<Response, ApiError> {
    cleanup_expired_auth_state(&state.pool).await?;
    let token = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, expires_at)
         VALUES ($1, $2, CURRENT_TIMESTAMP + INTERVAL '1 hour')",
    )
    .bind(hash_session_token(&token))
    .bind(user_id)
    .execute(&state.pool)
    .await?;
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(
            header::SET_COOKIE,
            session_cookie(&token, state.secure_cookie),
        )
        .body(Body::empty())
        .map_err(|_| ApiError::Database)
}

pub async fn keepalive(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authenticate_request(&state, &headers).await?;
    let Some(token) = session_token(&headers) else {
        return Err(ApiError::Unauthorized);
    };
    let result = sqlx::query(
        "UPDATE sessions SET expires_at = CURRENT_TIMESTAMP + INTERVAL '1 hour'
         WHERE token_hash = $1 AND expires_at > CURRENT_TIMESTAMP",
    )
    .bind(hash_session_token(&token))
    .execute(&state.pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::Unauthorized);
    }
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(
            header::SET_COOKIE,
            session_cookie(&token, state.secure_cookie),
        )
        .body(Body::empty())
        .map_err(|_| ApiError::Database)
}

pub async fn index_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Html<String>, ApiError> {
    if authenticate_request(&state, &headers).await.is_ok() {
        Ok(Html(crate::templates::render_page(
            crate::templates::INDEX_HTML,
        )))
    } else {
        Ok(Html(crate::templates::login_page_html()))
    }
}

pub async fn projects_page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Html<String>, ApiError> {
    if authenticate_request(&state, &headers).await.is_ok() {
        Ok(Html(crate::templates::render_page(
            crate::templates::PROJECT_RESULTS_HTML,
        )))
    } else {
        Ok(Html(crate::templates::login_page_html()))
    }
}

pub async fn access_urls(State(state): State<Arc<AppState>>) -> Json<AccessUrlsResponse> {
    let local_url = state
        .access_urls
        .iter()
        .find(|url| {
            url.contains("://127.0.0.1") || url.contains("://localhost") || url.contains("://[::1]")
        })
        .cloned();
    let lan_url = state
        .access_urls
        .iter()
        .find(|url| local_url.as_deref() != Some(url.as_str()))
        .cloned();
    Json(AccessUrlsResponse {
        local_url,
        lan_url: lan_url.clone(),
        mobile_url: lan_url,
        urls: state.access_urls.clone(),
    })
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(request): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    let username = request.username.trim();
    check_login_rate_limit(&state.pool, username).await?;
    let user = auth::authenticate_credentials(&state.pool, username, request.password.as_str())
        .await
        .map_err(|err| {
            tracing::error!(username = %username, error = %err, "login auth error");
            ApiError::Unauthorized
        })?;
    let user = match user {
        Some(user) => user,
        None => {
            tracing::warn!(username = %username, "login failed: invalid credentials");
            record_login_failure(&state.pool, username).await?;
            return Err(ApiError::Unauthorized);
        }
    };
    clear_login_failures(&state.pool, username).await?;
    create_session_response(&state, user.0).await
}

pub async fn google_login_start(State(state): State<Arc<AppState>>) -> Result<Response, ApiError> {
    let Some(config) = google_oauth_config() else {
        return redirect_response("/login?error=google_unavailable");
    };
    cleanup_expired_auth_state(&state.pool).await?;
    let oauth_state = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO oauth_states (state) VALUES ($1)")
        .bind(&oauth_state)
        .execute(&state.pool)
        .await?;
    let location = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope=openid%20email%20profile&state={}&access_type=online&prompt=select_account",
        url_encode_query_component(&config.client_id),
        url_encode_query_component(&config.redirect_uri),
        url_encode_query_component(&oauth_state),
    );
    redirect_response(&location)
}

pub async fn google_login_callback(
    State(state): State<Arc<AppState>>,
    Query(query): Query<GoogleCallbackQuery>,
) -> Result<Response, ApiError> {
    if query.error.is_some() {
        return redirect_response("/login?error=google");
    }
    let Some(oauth_state) = query.state.filter(|value| !value.trim().is_empty()) else {
        return redirect_response("/login?error=google");
    };
    let Some(code) = query.code.filter(|value| !value.trim().is_empty()) else {
        return redirect_response("/login?error=google");
    };
    let Some(config) = google_oauth_config() else {
        return redirect_response("/login?error=google_unavailable");
    };

    let mut transaction = state.pool.begin().await?;
    let valid_state: Option<String> = sqlx::query_scalar(
        "SELECT state FROM oauth_states
         WHERE state = $1 AND created_at > CURRENT_TIMESTAMP - INTERVAL '10 minutes'",
    )
    .bind(&oauth_state)
    .fetch_optional(&mut *transaction)
    .await?;
    if valid_state.is_none() {
        return redirect_response("/login?error=google");
    }
    sqlx::query("DELETE FROM oauth_states WHERE state = $1")
        .bind(&oauth_state)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;

    let profile = match fetch_google_user_info(&config, &code).await {
        Ok(profile) if !profile.sub.trim().is_empty() && profile.email_verified == Some(true) => {
            profile
        }
        Ok(_) => {
            tracing::warn!("Googleアカウントのメールアドレスが未検証です");
            return redirect_response("/login?error=google");
        }
        Err(error) => {
            tracing::error!(error = %error, "Googleアカウント情報を取得できませんでした");
            return redirect_response("/login?error=google");
        }
    };
    let email = profile.email.trim().to_lowercase();
    if email.is_empty() {
        return redirect_response("/login?error=google");
    }

    let user_id = match find_or_create_google_user(&state.pool, &profile.sub, &email).await? {
        Some(user_id) => user_id,
        None => return redirect_response("/login?error=google_inactive"),
    };
    let session_response = create_session_response(&state, user_id).await?;
    let cookie = session_response
        .headers()
        .get(header::SET_COOKIE)
        .cloned()
        .ok_or(ApiError::Database)?;
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(header::LOCATION, "/")
        .header(header::SET_COOKIE, cookie)
        .body(Body::empty())
        .map_err(|_| ApiError::Database)
}

async fn fetch_google_user_info(
    config: &GoogleOAuthConfig,
    code: &str,
) -> anyhow::Result<GoogleUserInfo> {
    let client = reqwest::Client::new();
    let token: GoogleTokenResponse = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", code),
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("redirect_uri", config.redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(client
        .get("https://openidconnect.googleapis.com/v1/userinfo")
        .bearer_auth(token.access_token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

async fn find_or_create_google_user(
    pool: &DbPool,
    subject: &str,
    email: &str,
) -> Result<Option<i64>, ApiError> {
    let mut transaction = pool.begin().await?;
    let identity_user: Option<(i64, bool)> = sqlx::query_as(
        "SELECT u.id, u.active
         FROM oauth_identities i JOIN users u ON u.id = i.user_id
         WHERE i.provider = 'google' AND i.subject = $1",
    )
    .bind(subject)
    .fetch_optional(&mut *transaction)
    .await?;
    if let Some((user_id, active)) = identity_user {
        transaction.commit().await?;
        return Ok(active.then_some(user_id));
    }

    let existing_user: Option<(i64, bool)> =
        sqlx::query_as("SELECT id, active FROM users WHERE username = $1")
            .bind(email)
            .fetch_optional(&mut *transaction)
            .await?;
    let user_id = if let Some((user_id, active)) = existing_user {
        if !active {
            transaction.commit().await?;
            return Ok(None);
        }
        user_id
    } else {
        let password_hash = auth::hash_password(&uuid::Uuid::new_v4().to_string())
            .map_err(|error| {
                tracing::error!(error = %error, "Googleユーザーの初期パスワードを生成できませんでした");
                ApiError::Database
            })?;
        let user_id = sqlx::query_scalar::<_,  i64>(
            "INSERT INTO users (username, password_hash, role, updated_at) VALUES ($1, $2, 'member', CURRENT_TIMESTAMP) RETURNING id",
        )
        .bind(email)
        .bind(password_hash)
        .fetch_one(&mut *transaction)
        .await?;
        user_id
    };
    sqlx::query(
        "INSERT INTO oauth_identities (provider, subject, user_id, email) VALUES ('google', $1, $2, $3)",
    )
    .bind(subject)
    .bind(user_id)
    .bind(email)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Some(user_id))
}

fn redirect_response(location: &str) -> Result<Response, ApiError> {
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(header::LOCATION, location)
        .body(Body::empty())
        .map_err(|_| ApiError::Database)
}

fn url_encode_query_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

pub async fn logout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(token) = session_token(&headers) {
        sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
            .bind(hash_session_token(&token))
            .execute(&state.pool)
            .await?;
    }
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(header::LOCATION, "/")
        .header(
            header::SET_COOKIE,
            clear_session_cookie(state.secure_cookie),
        )
        .body(Body::empty())
        .map_err(|_| ApiError::Database)
}

pub async fn logout_on_close(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let Some(token) = session_token(&headers) else {
        return Ok(StatusCode::NO_CONTENT);
    };
    // Reload also fires pagehide. Keep a short grace period so the new page can
    // call /api/me and renew the session before a real window close expires it.
    sqlx::query(
        "UPDATE sessions SET expires_at = CURRENT_TIMESTAMP + INTERVAL '5 seconds' WHERE token_hash = $1",
    )
    .bind(hash_session_token(&token))
    .execute(&state.pool)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<UserMeResponse>, ApiError> {
    let auth_user = authenticate_request(&state, &headers).await?;

    let user: Option<(i64, String, String)> =
        sqlx::query_as("SELECT id, username, role FROM users WHERE id = $1 AND active")
            .bind(auth_user.0)
            .fetch_optional(&state.pool)
            .await?;

    let Some((id, username, role)) = user else {
        return Err(ApiError::Unauthorized);
    };

    let session_expires_at: String = sqlx::query_scalar(
        "SELECT to_char(expires_at AT TIME ZONE 'Asia/Tokyo', 'YYYY-MM-DD\"T\"HH24:MI:SS') || '+09:00' FROM sessions WHERE token_hash = $1",
    )
    .bind(session_token(&headers).map(|token| hash_session_token(&token)))
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(UserMeResponse {
        id,
        username,
        role,
        session_expires_at,
    }))
}

pub async fn ensure_webauthn_id(pool: &DbPool, user_id: i64) -> Result<uuid::Uuid, ApiError> {
    let existing: Option<String> =
        sqlx::query_scalar("SELECT webauthn_id FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    if let Some(value) = existing {
        if let Ok(uuid) = uuid::Uuid::parse_str(&value) {
            return Ok(uuid);
        }
    }
    let webauthn_id = uuid::Uuid::new_v4();
    sqlx::query("UPDATE users SET webauthn_id = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2")
        .bind(webauthn_id.to_string())
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(webauthn_id)
}

pub async fn load_passkeys(pool: &DbPool, user_id: i64) -> Result<Vec<Passkey>, ApiError> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT passkey_json FROM passkeys WHERE user_id = $1 ORDER BY id ASC")
            .bind(user_id)
            .fetch_all(pool)
            .await?;
    rows.into_iter()
        .map(|(json,)| serde_json::from_str(&json).map_err(|_| ApiError::Database))
        .collect()
}

pub async fn user_passkey_register_start(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<i64>,
    headers: HeaderMap,
) -> Result<Json<PasskeyChallengeResponse<CreationChallengeResponse>>, ApiError> {
    cleanup_expired_auth_state(&state.pool).await?;
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if user.1 != "admin" {
        return Err(ApiError::Forbidden);
    }
    let username: Option<String> = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.pool)
        .await?;
    let username = username.ok_or(ApiError::NotFound)?;
    start_passkey_registration_for_user(&state, user_id, &username).await
}

pub async fn start_passkey_registration_for_user(
    state: &Arc<AppState>,
    user_id: i64,
    username: &str,
) -> Result<Json<PasskeyChallengeResponse<CreationChallengeResponse>>, ApiError> {
    let webauthn_id = ensure_webauthn_id(&state.pool, user_id).await?;
    let existing = load_passkeys(&state.pool, user_id).await?;
    let exclude_credentials = existing
        .iter()
        .map(|passkey| passkey.cred_id().clone())
        .collect::<Vec<_>>();
    let (public_key, registration) = state
        .webauthn
        .start_passkey_registration(
            webauthn_id,
            username,
            username,
            (!exclude_credentials.is_empty()).then_some(exclude_credentials),
        )
        .map_err(|_| ApiError::BadRequest("パスキー登録を開始できませんでした"))?;
    let challenge_id = uuid::Uuid::new_v4().to_string();
    let state_json = serde_json::to_string(&registration).map_err(|_| ApiError::Database)?;
    sqlx::query(
        "INSERT INTO passkey_challenges (challenge_id, user_id, challenge_type, state_json, expires_at)
         VALUES ($1, $2, 'registration', $3, CURRENT_TIMESTAMP + INTERVAL '5 minutes')"
    )
    .bind(&challenge_id)
    .bind(user_id)
    .bind(&state_json)
    .execute(&state.pool)
    .await?;
    Ok(Json(PasskeyChallengeResponse {
        challenge_id,
        public_key,
    }))
}

pub async fn passkey_register_finish(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<PasskeyRegisterFinishRequest>,
) -> Result<Response, ApiError> {
    authenticate_admin(&state, &headers).await?;
    let row: Option<(i64, String)> = sqlx::query_as(
        "SELECT user_id, state_json FROM passkey_challenges
         WHERE challenge_id = $1 AND challenge_type = 'registration' AND expires_at > CURRENT_TIMESTAMP"
    )
    .bind(&request.challenge_id)
    .fetch_optional(&state.pool)
    .await?;

    let Some((user_id, state_json)) = row else {
        return Err(ApiError::BadRequest("パスキー登録の有効期限が切れました"));
    };

    let registration_state: PasskeyRegistration =
        serde_json::from_str(&state_json).map_err(|_| ApiError::Database)?;

    let passkey = state
        .webauthn
        .finish_passkey_registration(&request.credential, &registration_state)
        .map_err(|_| ApiError::Unauthorized)?;

    sqlx::query("DELETE FROM passkey_challenges WHERE challenge_id = $1")
        .bind(&request.challenge_id)
        .execute(&state.pool)
        .await?;

    let credential_id = serde_json::to_string(passkey.cred_id()).map_err(|_| ApiError::Database)?;
    let passkey_json = serde_json::to_string(&passkey).map_err(|_| ApiError::Database)?;
    sqlx::query(
        "INSERT INTO passkeys (user_id, credential_id, passkey_json) VALUES ($1, $2, $3)
         ON CONFLICT(credential_id) DO UPDATE SET user_id = excluded.user_id, passkey_json = excluded.passkey_json",
    )
    .bind(user_id)
    .bind(credential_id)
    .bind(passkey_json)
    .execute(&state.pool)
    .await?;
    sqlx::query("UPDATE users SET updated_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(user_id)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn passkey_login_start(
    State(state): State<Arc<AppState>>,
    Json(request): Json<PasskeyLoginStartRequest>,
) -> Result<Json<PasskeyChallengeResponse<RequestChallengeResponse>>, ApiError> {
    cleanup_expired_auth_state(&state.pool).await?;
    let username = request.username.trim();
    let user: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM users WHERE username = $1 AND active")
            .bind(username)
            .fetch_optional(&state.pool)
            .await?;
    let Some((user_id,)) = user else {
        return Err(ApiError::Unauthorized);
    };
    let passkeys = load_passkeys(&state.pool, user_id).await?;
    if passkeys.is_empty() {
        return Err(ApiError::Unauthorized);
    }
    let (public_key, authentication) = state
        .webauthn
        .start_passkey_authentication(&passkeys)
        .map_err(|_| ApiError::BadRequest("パスキー認証を開始できませんでした"))?;
    let challenge_id = uuid::Uuid::new_v4().to_string();
    let state_json = serde_json::to_string(&authentication).map_err(|_| ApiError::Database)?;
    sqlx::query(
        "INSERT INTO passkey_challenges (challenge_id, user_id, challenge_type, state_json, expires_at)
         VALUES ($1, $2, 'authentication', $3, CURRENT_TIMESTAMP + INTERVAL '5 minutes')"
    )
    .bind(&challenge_id)
    .bind(user_id)
    .bind(&state_json)
    .execute(&state.pool)
    .await?;
    Ok(Json(PasskeyChallengeResponse {
        challenge_id,
        public_key,
    }))
}

pub async fn passkey_login_finish(
    State(state): State<Arc<AppState>>,
    Json(request): Json<PasskeyLoginFinishRequest>,
) -> Result<Response, ApiError> {
    let row: Option<(i64, String)> = sqlx::query_as(
        "SELECT user_id, state_json FROM passkey_challenges
         WHERE challenge_id = $1 AND challenge_type = 'authentication' AND expires_at > CURRENT_TIMESTAMP"
    )
    .bind(&request.challenge_id)
    .fetch_optional(&state.pool)
    .await?;

    let Some((user_id, state_json)) = row else {
        return Err(ApiError::BadRequest("パスキー認証の有効期限が切れました"));
    };

    let authentication_state: PasskeyAuthentication =
        serde_json::from_str(&state_json).map_err(|_| ApiError::Database)?;

    let result = state
        .webauthn
        .finish_passkey_authentication(&request.credential, &authentication_state)
        .map_err(|_| ApiError::Unauthorized)?;

    sqlx::query("DELETE FROM passkey_challenges WHERE challenge_id = $1")
        .bind(&request.challenge_id)
        .execute(&state.pool)
        .await?;

    let mut passkeys = load_passkeys(&state.pool, user_id).await?;
    for passkey in &mut passkeys {
        if passkey.update_credential(&result).is_some() {
            let credential_id =
                serde_json::to_string(passkey.cred_id()).map_err(|_| ApiError::Database)?;
            let passkey_json = serde_json::to_string(passkey).map_err(|_| ApiError::Database)?;
            sqlx::query("UPDATE passkeys SET passkey_json = $1 WHERE credential_id = $2")
                .bind(passkey_json)
                .bind(credential_id)
                .execute(&state.pool)
                .await?;
            break;
        }
    }
    create_session_response(&state, user_id).await
}

pub async fn delete_user_passkeys(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<i64>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    authenticate_admin(&state, &headers).await?;
    sqlx::query("DELETE FROM passkeys WHERE user_id = $1")
        .bind(user_id)
        .execute(&state.pool)
        .await?;
    sqlx::query("UPDATE users SET updated_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(user_id)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
