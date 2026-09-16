use crate::auth;
use crate::core::{can_delete_user_target, can_edit_user};
use crate::handlers::auth_handlers::{authenticate_admin, authenticate_headers};
use crate::models::{ApiError, AppState, CreateUserRequest, UpdateUserRequest};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct UserSummary {
    pub id: i64,
    pub username: String,
    pub role: String,
    pub updated_at: Option<String>,
    pub passkey_count: i64,
}

#[derive(Debug, Deserialize)]
pub struct GrantPermissionRequest {
    pub user_id: i64,
    pub permission: String,
}

pub fn valid_user_role(role: &str) -> bool {
    matches!(role, "admin" | "member" | "viewer")
}

pub async fn list_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<UserSummary>>, ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if !matches!(user.1.as_str(), "admin" | "viewer") {
        return Err(ApiError::Forbidden);
    }
    let query = if user.1 == "admin" {
        sqlx::query_as::<_,  UserSummary>(
            "SELECT users.id, users.username, users.role,
            to_char(users.updated_at AT TIME ZONE 'Asia/Tokyo', 'YYYY-MM-DD HH24:MI:SS') as updated_at,
            COUNT(passkeys.id) as passkey_count
         FROM users
         LEFT JOIN passkeys ON passkeys.user_id = users.id
         WHERE users.active
         GROUP BY users.id, users.username, users.role, users.updated_at
         ORDER BY users.id ASC",
        )
    } else {
        sqlx::query_as::<_,  UserSummary>(
            "SELECT users.id, users.username, users.role,
            to_char(users.updated_at AT TIME ZONE 'Asia/Tokyo', 'YYYY-MM-DD HH24:MI:SS') as updated_at,
            COUNT(passkeys.id) as passkey_count
         FROM users
         LEFT JOIN passkeys ON passkeys.user_id = users.id
         WHERE users.active AND users.id = $1
         GROUP BY users.id, users.username, users.role, users.updated_at
         ORDER BY users.id ASC",
        )
        .bind(user.0)
    };
    let users: Vec<UserSummary> = query.fetch_all(&state.pool).await?;
    Ok(Json(users))
}

pub async fn list_deleted_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<UserSummary>>, ApiError> {
    authenticate_admin(&state, &headers).await?;
    let users: Vec<UserSummary> = sqlx::query_as(
        "SELECT users.id, users.username, users.role,
            to_char(users.updated_at AT TIME ZONE 'Asia/Tokyo', 'YYYY-MM-DD HH24:MI:SS') as updated_at,
            COUNT(passkeys.id) as passkey_count
         FROM users
         LEFT JOIN passkeys ON passkeys.user_id = users.id
         WHERE NOT users.active
         GROUP BY users.id, users.username, users.role, users.updated_at
         ORDER BY users.id ASC",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(users))
}

pub async fn create_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateUserRequest>,
) -> Result<StatusCode, ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if user.1 != "admin" {
        return Err(ApiError::Forbidden);
    }
    if request.username.trim().is_empty()
        || request.password.len() < 8
        || !valid_user_role(request.role.as_str())
    {
        return Err(ApiError::BadRequest(
            "ユーザー名・8文字以上のパスワード・有効なロールが必要です",
        ));
    }
    sqlx::query(
        "INSERT INTO users (username, password_hash, role, webauthn_id, created_at, updated_at) VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
        .bind(request.username.trim())
        .bind(auth::hash_password(&request.password).map_err(|_| ApiError::Storage)?)
        .bind(request.role)
        .bind(uuid::Uuid::new_v4().to_string())
        .execute(&state.pool)
        .await
        .map_err(|_| ApiError::BadRequest("ユーザー名が既に存在します"))?;
    Ok(StatusCode::CREATED)
}

pub async fn update_user(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Json(request): Json<UpdateUserRequest>,
) -> Result<StatusCode, ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if !can_edit_user(user.0, &user.1, id) {
        return Err(ApiError::Forbidden);
    }

    if user.1 == "viewer" && request.role.is_some() {
        return Err(ApiError::Forbidden);
    }

    if let Some(ref role) = request.role {
        if !valid_user_role(role.as_str()) {
            return Err(ApiError::BadRequest("有効なロールを指定してください"));
        }
        sqlx::query("UPDATE users SET role = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2")
            .bind(role)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }

    if let Some(ref password) = request.password {
        let pwd = password.trim();
        if !pwd.is_empty() {
            if pwd.len() < 8 {
                return Err(ApiError::BadRequest("パスワードは8文字以上必要です"));
            }
            let hash = auth::hash_password(pwd).map_err(|_| ApiError::Storage)?;
            sqlx::query(
                "UPDATE users SET password_hash = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2",
            )
            .bind(hash)
            .bind(id)
            .execute(&state.pool)
            .await?;
        }
    }

    Ok(StatusCode::OK)
}

pub async fn delete_user(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if user.0 == id {
        return Err(ApiError::BadRequest(
            "ログイン中の管理ユーザーは削除できません",
        ));
    }

    let target_role: Option<String> =
        sqlx::query_scalar("SELECT role FROM users WHERE id = $1 AND active")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    if let Some(target_role) = target_role {
        if !can_delete_user_target(&user.1, &target_role) {
            return Err(ApiError::Forbidden);
        }
    }
    if user.1 != "admin" {
        return Err(ApiError::Forbidden);
    }

    let mut transaction = state.pool.begin().await?;
    let result = sqlx::query(
        "UPDATE users SET active = FALSE, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND active",
    )
    .bind(id)
    .execute(&mut *transaction)
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    sqlx::query("UPDATE sessions SET expires_at = CURRENT_TIMESTAMP WHERE user_id = $1")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn permanently_delete_user(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    authenticate_admin(&state, &headers).await?;
    let result = sqlx::query("DELETE FROM users WHERE id = $1 AND NOT active")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn grant_project_permission(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<i64>,
    headers: HeaderMap,
    Json(request): Json<GrantPermissionRequest>,
) -> Result<StatusCode, ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if user.1 != "admin" {
        return Err(ApiError::Forbidden);
    }
    if !matches!(request.permission.as_str(), "member" | "viewer") {
        return Err(ApiError::BadRequest(
            "permissionはmember・viewerのいずれかです",
        ));
    }
    sqlx::query(
        "INSERT INTO project_permissions (user_id, project_id, permission) VALUES ($1, $2, $3)
         ON CONFLICT(user_id, project_id) DO UPDATE SET permission = excluded.permission",
    )
    .bind(request.user_id)
    .bind(project_id)
    .bind(request.permission)
    .execute(&state.pool)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
