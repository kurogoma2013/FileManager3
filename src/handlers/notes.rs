use crate::core::can_delete_permanently;
use crate::db::DbPool;
use crate::handlers::auth_handlers::{authenticate_admin, authenticate_headers};
use crate::models::{ApiError, AppState, CreateNoteRequest, NoteItem};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use std::sync::Arc;

pub async fn list_project_notes(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<i64>,
    headers: HeaderMap,
) -> Result<Json<Vec<NoteItem>>, ApiError> {
    let _user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    let notes: Vec<NoteItem> = crate::db::query_as(
        "SELECT pn.id, pn.content,
            COALESCE(u.username, CASE WHEN pn.created_by = '' THEN 'ゲスト' ELSE pn.created_by END) as created_by,
            strftime('%Y-%m-%d %H:%M:%S', pn.created_at, '+9 hours') as created_at
         FROM project_notes pn
         LEFT JOIN users u ON u.id::text = pn.created_by OR u.username = pn.created_by
         WHERE pn.project_id = ? AND pn.deleted_at IS NULL
         ORDER BY pn.id DESC",
    )
    .bind(project_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(notes))
}

pub async fn list_deleted_project_notes(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<i64>,
    headers: HeaderMap,
) -> Result<Json<Vec<NoteItem>>, ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if user.1 == "viewer" {
        return Err(ApiError::Forbidden);
    }
    if !can_delete_permanently(&user.1) {
        return Err(ApiError::Forbidden);
    }
    let notes: Vec<NoteItem> = crate::db::query_as(
        "SELECT pn.id, pn.content,
            COALESCE(u.username, CASE WHEN pn.created_by = '' THEN 'ゲスト' ELSE pn.created_by END) as created_by,
            strftime('%Y-%m-%d %H:%M:%S', pn.created_at, '+9 hours') as created_at
         FROM project_notes pn
         LEFT JOIN users u ON u.id::text = pn.created_by OR u.username = pn.created_by
         WHERE pn.project_id = ? AND pn.deleted_at IS NOT NULL
         ORDER BY pn.id DESC",
    )
    .bind(project_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(notes))
}

pub async fn create_project_note(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<i64>,
    headers: HeaderMap,
    Json(request): Json<CreateNoteRequest>,
) -> Result<StatusCode, ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    let content = request.content.trim();
    if content.is_empty() {
        return Err(ApiError::BadRequest("メモ内容を入力してください"));
    }

    let username: String = crate::db::query_scalar("SELECT username FROM users WHERE id = ?")
        .bind(user.0)
        .fetch_optional(&state.pool)
        .await?
        .unwrap_or_else(|| "管理者".to_string());

    crate::db::query(
        "INSERT INTO project_notes (project_id, content, created_by) VALUES (?, ?, ?)",
    )
    .bind(project_id)
    .bind(content)
    .bind(&username)
    .execute(&state.pool)
    .await?;
    touch_project_updated_at(&state.pool, project_id).await?;

    Ok(StatusCode::CREATED)
}

pub async fn list_dealer_notes(
    State(state): State<Arc<AppState>>,
    Path(dealer_id): Path<i64>,
    headers: HeaderMap,
) -> Result<Json<Vec<NoteItem>>, ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if user.1 == "viewer" {
        return Err(ApiError::Forbidden);
    }

    let notes: Vec<NoteItem> = crate::db::query_as(
        "SELECT dn.id, dn.content,
            COALESCE(u.username, CASE WHEN dn.created_by = '' THEN 'ゲスト' ELSE dn.created_by END) as created_by,
            strftime('%Y-%m-%d %H:%M:%S', dn.created_at, '+9 hours') as created_at
         FROM dealer_notes dn
         LEFT JOIN users u ON u.id::text = dn.created_by OR u.username = dn.created_by
         WHERE dn.dealer_id = ? AND dn.deleted_at IS NULL
         ORDER BY dn.id DESC",
    )
    .bind(dealer_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(notes))
}

pub async fn list_deleted_dealer_notes(
    State(state): State<Arc<AppState>>,
    Path(dealer_id): Path<i64>,
    headers: HeaderMap,
) -> Result<Json<Vec<NoteItem>>, ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if user.1 == "viewer" {
        return Err(ApiError::Forbidden);
    }
    if !can_delete_permanently(&user.1) {
        return Err(ApiError::Forbidden);
    }
    let notes: Vec<NoteItem> = crate::db::query_as(
        "SELECT dn.id, dn.content,
            COALESCE(u.username, CASE WHEN dn.created_by = '' THEN 'ゲスト' ELSE dn.created_by END) as created_by,
            strftime('%Y-%m-%d %H:%M:%S', dn.created_at, '+9 hours') as created_at
         FROM dealer_notes dn
         LEFT JOIN users u ON u.id::text = dn.created_by OR u.username = dn.created_by
         WHERE dn.dealer_id = ? AND dn.deleted_at IS NOT NULL
         ORDER BY dn.id DESC",
    )
    .bind(dealer_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(notes))
}

pub async fn create_dealer_note(
    State(state): State<Arc<AppState>>,
    Path(dealer_id): Path<i64>,
    headers: HeaderMap,
    Json(request): Json<CreateNoteRequest>,
) -> Result<StatusCode, ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if user.1 == "viewer" {
        return Err(ApiError::Forbidden);
    }

    let content = request.content.trim();
    if content.is_empty() {
        return Err(ApiError::BadRequest("メモ内容を入力してください"));
    }

    let username: String = crate::db::query_scalar("SELECT username FROM users WHERE id = ?")
        .bind(user.0)
        .fetch_optional(&state.pool)
        .await?
        .unwrap_or_else(|| "管理者".to_string());

    crate::db::query("INSERT INTO dealer_notes (dealer_id, content, created_by) VALUES (?, ?, ?)")
        .bind(dealer_id)
        .bind(content)
        .bind(&username)
        .execute(&state.pool)
        .await?;

    Ok(StatusCode::CREATED)
}

pub async fn delete_project_note(
    State(state): State<Arc<AppState>>,
    Path((project_id, note_id)): Path<(i64, i64)>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if !can_delete_permanently(&user.1) {
        return Err(ApiError::Forbidden);
    }

    let result = crate::db::query("UPDATE project_notes SET deleted_at = CURRENT_TIMESTAMP WHERE id = ? AND project_id = ? AND deleted_at IS NULL")
        .bind(note_id)
        .bind(project_id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }

    touch_project_updated_at(&state.pool, project_id).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn restore_project_note(
    State(state): State<Arc<AppState>>,
    Path((project_id, note_id)): Path<(i64, i64)>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if !can_delete_permanently(&user.1) {
        return Err(ApiError::Forbidden);
    }
    let result = crate::db::query("UPDATE project_notes SET deleted_at = NULL WHERE id = ? AND project_id = ? AND deleted_at IS NOT NULL")
        .bind(note_id)
        .bind(project_id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    touch_project_updated_at(&state.pool, project_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn permanently_delete_project_note(
    State(state): State<Arc<AppState>>,
    Path((project_id, note_id)): Path<(i64, i64)>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    authenticate_admin(&state, &headers).await?;
    let result = crate::db::query(
        "DELETE FROM project_notes WHERE id = ? AND project_id = ? AND deleted_at IS NOT NULL",
    )
    .bind(note_id)
    .bind(project_id)
    .execute(&state.pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn touch_project_updated_at(pool: &DbPool, project_id: i64) -> Result<(), ApiError> {
    crate::db::query("UPDATE projects SET updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_dealer_note(
    State(state): State<Arc<AppState>>,
    Path((dealer_id, note_id)): Path<(i64, i64)>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if !can_delete_permanently(&user.1) {
        return Err(ApiError::Forbidden);
    }

    let result = crate::db::query("UPDATE dealer_notes SET deleted_at = CURRENT_TIMESTAMP WHERE id = ? AND dealer_id = ? AND deleted_at IS NULL")
        .bind(note_id)
        .bind(dealer_id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn restore_dealer_note(
    State(state): State<Arc<AppState>>,
    Path((dealer_id, note_id)): Path<(i64, i64)>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if !can_delete_permanently(&user.1) {
        return Err(ApiError::Forbidden);
    }
    let result = crate::db::query("UPDATE dealer_notes SET deleted_at = NULL WHERE id = ? AND dealer_id = ? AND deleted_at IS NOT NULL")
        .bind(note_id)
        .bind(dealer_id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn permanently_delete_dealer_note(
    State(state): State<Arc<AppState>>,
    Path((dealer_id, note_id)): Path<(i64, i64)>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    authenticate_admin(&state, &headers).await?;
    let result = crate::db::query(
        "DELETE FROM dealer_notes WHERE id = ? AND dealer_id = ? AND deleted_at IS NOT NULL",
    )
    .bind(note_id)
    .bind(dealer_id)
    .execute(&state.pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
