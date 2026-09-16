use crate::core::can_manage_projects;
use crate::db::DbPool;
use crate::handlers::auth_handlers::{
    authenticate_admin, authenticate_headers, require_project_access,
};
use crate::maintenance;
use crate::models::{ApiError, AppState, CreateProjectRequest, FileItem, ProjectTrashItem};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ProjectSummary {
    pub id: i64,
    pub project_number: String,
    pub name: String,
    pub kana: String,
    pub address: String,
    pub dealer: Option<String>,
    pub assignee: Option<String>,
    pub assignee_phone: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub plus_code: Option<String>,
    pub documents_count: Option<i64>,
    pub pictures_count: Option<i64>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub search: Option<String>,
    pub q: Option<String>,
    pub project_number: Option<String>,
    pub project_name: Option<String>,
    pub project_kana: Option<String>,
    pub address: Option<String>,
    pub dealer: Option<String>,
    pub sort: Option<String>,
}

pub(crate) fn project_order_by(value: Option<&str>) -> &'static str {
    match value.unwrap_or("updated_desc") {
        "updated_asc" => "updated_at ASC, id ASC",
        "name_asc" => "name COLLATE NOCASE ASC, id DESC",
        "name_desc" => "name COLLATE NOCASE DESC, id DESC",
        "number_asc" => "project_number ASC, id DESC",
        "number_desc" => "project_number DESC, id DESC",
        _ => "updated_at DESC, id DESC",
    }
}

#[derive(Debug, Serialize)]
pub struct ProjectDetail {
    pub project: ProjectSummary,
    pub documents: Vec<FileItem>,
    pub pictures: Vec<FileItem>,
}

pub fn normalize_phone(value: Option<&str>) -> Option<String> {
    let normalized: String = value
        .unwrap_or_default()
        .chars()
        .filter_map(|character| match character {
            '０'..='９' => char::from_u32(character as u32 - '０' as u32 + '0' as u32),
            character if character.is_ascii_digit() => Some(character),
            _ => None,
        })
        .collect();
    (!normalized.is_empty()).then_some(normalized)
}

pub fn normalize_project_number(value: &str) -> String {
    value
        .chars()
        .filter_map(|character| match character {
            '０'..='９' => char::from_u32(character as u32 - '０' as u32 + '0' as u32),
            character if character.is_ascii_digit() => Some(character),
            _ => None,
        })
        .collect()
}

pub fn validate_project_location(
    latitude: Option<f64>,
    longitude: Option<f64>,
    plus_code: Option<&str>,
) -> Result<(), &'static str> {
    let has_coordinates = latitude.is_some() || longitude.is_some();
    let has_plus_code = plus_code.is_some_and(|value| !value.trim().is_empty());

    if !has_coordinates && !has_plus_code {
        return Ok(());
    }

    if has_coordinates && has_plus_code {
        return Err("緯度経度またはPlusCodeのどちらか一方を入力してください");
    }

    if has_plus_code {
        return Ok(());
    }

    match (latitude, longitude) {
        (Some(latitude), Some(longitude))
            if (-90.0..=90.0).contains(&latitude) && (-180.0..=180.0).contains(&longitude) =>
        {
            Ok(())
        }
        (Some(_), Some(_)) => Err("緯度は-90から90、経度は-180から180の範囲で入力してください"),
        _ if has_plus_code => Ok(()),
        _ => Err("緯度と経度は両方入力してください"),
    }
}

pub async fn search_projects(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<SearchParams>,
) -> Result<Json<Vec<ProjectSummary>>, ApiError> {
    let _user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;

    let search_term = params.search.or(params.q).unwrap_or_default();
    let num = params.project_number.unwrap_or_default();
    let name = params.project_name.unwrap_or_default();
    let kana = params.project_kana.unwrap_or_default();
    let address = params.address.unwrap_or_default();
    let dealer = params.dealer.unwrap_or_default();
    let order_by = project_order_by(params.sort.as_deref());

    let mut builder = sqlx::QueryBuilder::new(
        "SELECT id, project_number, name, kana, address, phone, email, dealer, assignee, (SELECT phone FROM dealer_contacts c WHERE c.dealer_name = projects.dealer AND c.name = projects.assignee AND c.deleted_at IS NULL LIMIT 1) as assignee_phone, latitude, longitude, plus_code, to_char(updated_at AT TIME ZONE 'Asia/Tokyo', 'YYYY-MM-DD HH24:MI:SS') AS updated_at,
         (SELECT COUNT(*) FROM files f WHERE f.project_id = projects.id AND LOWER(f.file_type) = 'document' AND f.deleted_at IS NULL) as documents_count,
         (SELECT COUNT(*) FROM files f WHERE f.project_id = projects.id AND LOWER(f.file_type) = 'picture' AND f.deleted_at IS NULL) as pictures_count
         FROM projects
         WHERE deleted_at IS NULL",
    );

    let tokens: Vec<&str> = search_term.split_whitespace().collect();
    for token in tokens {
        builder.push(" AND (");
        builder
            .push("project_number LIKE ")
            .push_bind(format!("%{token}%"));
        builder
            .push(" OR name LIKE ")
            .push_bind(format!("%{token}%"));
        builder
            .push(" OR kana LIKE ")
            .push_bind(format!("%{token}%"));
        builder
            .push(" OR address LIKE ")
            .push_bind(format!("%{token}%"));
        builder
            .push(" OR dealer LIKE ")
            .push_bind(format!("%{token}%"));
        builder
            .push(" OR assignee LIKE ")
            .push_bind(format!("%{token}%"));
        builder
            .push(" OR email LIKE ")
            .push_bind(format!("%{token}%"));
        builder.push(")");
    }

    if !num.is_empty() {
        builder
            .push(" AND project_number LIKE ")
            .push_bind(format!("%{num}%"));
    }
    if !name.is_empty() {
        builder
            .push(" AND name LIKE ")
            .push_bind(format!("%{name}%"));
    }
    if !kana.is_empty() {
        builder
            .push(" AND kana LIKE ")
            .push_bind(format!("%{kana}%"));
    }
    if !address.is_empty() {
        builder
            .push(" AND address LIKE ")
            .push_bind(format!("%{address}%"));
    }
    if !dealer.is_empty() {
        builder
            .push(" AND dealer LIKE ")
            .push_bind(format!("%{dealer}%"));
    }

    builder.push(" ORDER BY ").push(order_by);

    let query = builder.build_query_as::<ProjectSummary>();
    let projects = query.fetch_all(&state.pool).await.map_err(|err| {
        tracing::error!(error = %err, "search_projects database query error");
        ApiError::Database
    })?;
    Ok(Json(projects))
}

async fn ensure_dealer_exists(pool: &DbPool, dealer_name: Option<&str>) -> Result<(), ApiError> {
    let Some(raw_name) = dealer_name else {
        return Ok(());
    };
    let name = raw_name.trim();
    if name.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO dealers (name, address) VALUES ($1, '')
         ON CONFLICT(name) DO UPDATE SET deleted_at = NULL",
    )
    .bind(name)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn create_project(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateProjectRequest>,
) -> Result<(StatusCode, Json<ProjectSummary>), ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if !can_manage_projects(&user.1) {
        return Err(ApiError::Forbidden);
    }
    let project_number = normalize_project_number(&request.project_number);
    validate_project_location(
        request.latitude,
        request.longitude,
        request.plus_code.as_deref(),
    )
    .map_err(ApiError::BadRequest)?;
    if project_number.is_empty() {
        return Err(ApiError::BadRequest("案件番号は数字のみ入力してください"));
    }
    if request.name.trim().is_empty()
        || request.kana.trim().is_empty()
        || request.dealer.as_deref().unwrap_or("").trim().is_empty()
    {
        return Err(ApiError::BadRequest("案件名・案件名カナ・販売店は必須です"));
    }
    ensure_dealer_exists(&state.pool, request.dealer.as_deref()).await?;
    let project_id = sqlx::query_scalar::<_,  i64>(
        "INSERT INTO projects (project_number, name, kana, address, dealer, assignee, latitude, longitude, plus_code, phone, email, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, CURRENT_TIMESTAMP) RETURNING id",
    )
    .bind(project_number)
    .bind(request.name)
    .bind(request.kana)
    .bind(request.address.unwrap_or_default())
    .bind(request.dealer.as_ref().map(|s| s.trim()))
    .bind(request.assignee.as_ref().map(|s| s.trim()))
    .bind(request.latitude)
    .bind(request.longitude)
    .bind(request.plus_code.filter(|value| !value.trim().is_empty()))
    .bind(normalize_phone(request.phone.as_deref()))
    .bind(request.email.as_deref().map(str::trim))
    .fetch_one(&state.pool)
    .await?;
    let project = sqlx::query_as::<_,  ProjectSummary>(
        "SELECT id, project_number, name, kana, address, phone, email, dealer, assignee, (SELECT phone FROM dealer_contacts c WHERE c.dealer_name = projects.dealer AND c.name = projects.assignee AND c.deleted_at IS NULL LIMIT 1) as assignee_phone, latitude, longitude, plus_code, to_char(updated_at AT TIME ZONE 'Asia/Tokyo', 'YYYY-MM-DD HH24:MI:SS') AS updated_at,
         (SELECT COUNT(*) FROM files f WHERE f.project_id = projects.id AND LOWER(f.file_type) = 'document' AND f.deleted_at IS NULL) as documents_count,
         (SELECT COUNT(*) FROM files f WHERE f.project_id = projects.id AND LOWER(f.file_type) = 'picture' AND f.deleted_at IS NULL) as pictures_count
         FROM projects WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(project_id)
    .fetch_one(&state.pool)
    .await?;
    Ok((StatusCode::CREATED, Json(project)))
}

pub async fn update_project(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Json(request): Json<CreateProjectRequest>,
) -> Result<Json<ProjectSummary>, ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if !can_manage_projects(&user.1) {
        let has_perm: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM project_permissions WHERE user_id = $1 AND project_id = $2 AND permission = 'member'",
        )
        .bind(user.0)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
        if has_perm.is_none() {
            return Err(ApiError::Forbidden);
        }
    }
    let project_number = normalize_project_number(&request.project_number);
    validate_project_location(
        request.latitude,
        request.longitude,
        request.plus_code.as_deref(),
    )
    .map_err(ApiError::BadRequest)?;
    if project_number.is_empty() {
        return Err(ApiError::BadRequest("案件番号は数字のみ入力してください"));
    }
    if request.name.trim().is_empty()
        || request.kana.trim().is_empty()
        || request.dealer.as_deref().unwrap_or("").trim().is_empty()
    {
        return Err(ApiError::BadRequest("案件名・案件名カナ・販売店は必須です"));
    }

    let duplicate: Option<i64> =
        sqlx::query_scalar("SELECT id FROM projects WHERE project_number = $1 AND id != $2")
            .bind(&project_number)
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    if duplicate.is_some() {
        return Err(ApiError::BadRequest("その案件番号は既に使用されています"));
    }

    ensure_dealer_exists(&state.pool, request.dealer.as_deref()).await?;
    sqlx::query(
        "UPDATE projects SET project_number = $1, name = $2, kana = $3, address = $4, dealer = $5, assignee = $6, latitude = $7, longitude = $8, plus_code = $9, phone = $10, email = $11, updated_at = CURRENT_TIMESTAMP WHERE id = $12",
    )
    .bind(&project_number)
    .bind(&request.name)
    .bind(&request.kana)
    .bind(request.address.unwrap_or_default())
    .bind(request.dealer.as_ref().map(|s| s.trim()))
    .bind(request.assignee.as_ref().map(|s| s.trim()))
    .bind(request.latitude)
    .bind(request.longitude)
    .bind(request.plus_code.filter(|v| !v.trim().is_empty()))
    .bind(normalize_phone(request.phone.as_deref()))
    .bind(request.email.as_deref().map(str::trim))
    .bind(id)
    .execute(&state.pool)
    .await?;

    let project = sqlx::query_as::<_,  ProjectSummary>(
        "SELECT id, project_number, name, kana, address, phone, email, dealer, assignee, (SELECT phone FROM dealer_contacts c WHERE c.dealer_name = projects.dealer AND c.name = projects.assignee AND c.deleted_at IS NULL LIMIT 1) as assignee_phone, latitude, longitude, plus_code, to_char(updated_at AT TIME ZONE 'Asia/Tokyo', 'YYYY-MM-DD HH24:MI:SS') AS updated_at,
         (SELECT COUNT(*) FROM files f WHERE f.project_id = projects.id AND LOWER(f.file_type) = 'document' AND f.deleted_at IS NULL) as documents_count,
         (SELECT COUNT(*) FROM files f WHERE f.project_id = projects.id AND LOWER(f.file_type) = 'picture' AND f.deleted_at IS NULL) as pictures_count
         FROM projects WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(project))
}

pub async fn get_project(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Json<ProjectDetail>, ApiError> {
    project_detail(State(state), Path(id), headers).await
}

pub async fn project_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Json<ProjectDetail>, ApiError> {
    require_project_access(&headers, &state, id).await?;
    let project = sqlx::query_as::<_,  ProjectSummary>(
        "SELECT id, project_number, name, kana, address, phone, email, dealer, assignee, (SELECT phone FROM dealer_contacts c WHERE c.dealer_name = projects.dealer AND c.name = projects.assignee AND c.deleted_at IS NULL LIMIT 1) as assignee_phone, latitude, longitude, plus_code, to_char(updated_at AT TIME ZONE 'Asia/Tokyo', 'YYYY-MM-DD HH24:MI:SS') AS updated_at,
         (SELECT COUNT(*) FROM files f WHERE f.project_id = projects.id AND LOWER(f.file_type) = 'document' AND f.deleted_at IS NULL) as documents_count,
         (SELECT COUNT(*) FROM files f WHERE f.project_id = projects.id AND LOWER(f.file_type) = 'picture' AND f.deleted_at IS NULL) as pictures_count
         FROM projects WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    let files = sqlx::query_as::<_,  FileItem>(
        "SELECT id, version_number, file_name AS file_path, file_type, file_hash, source_hash, tag, file_size,
            to_char(created_at AT TIME ZONE 'Asia/Tokyo', 'YYYY-MM-DD HH24:MI:SS') AS created_at FROM files
         WHERE project_id = $1 AND deleted_at IS NULL ORDER BY file_path",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    let (documents, pictures) = files
        .into_iter()
        .partition(|file| file.file_type == "Document");
    Ok(Json(ProjectDetail {
        project,
        documents,
        pictures,
    }))
}

pub async fn delete_project(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if !can_manage_projects(&user.1) {
        return Err(ApiError::Forbidden);
    }

    if maintenance::soft_delete_project(&state.pool, id).await? == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn restore_project(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    authenticate_admin(&state, &headers).await?;
    if maintenance::restore_project(&state.pool, id).await? == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_deleted_projects(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<ProjectTrashItem>>, ApiError> {
    authenticate_admin(&state, &headers).await?;
    Ok(Json(maintenance::list_deleted_projects(&state.pool).await?))
}

pub async fn permanently_delete_project(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    authenticate_admin(&state, &headers).await?;
    if maintenance::permanently_delete_project(&state.pool, &state.storage_root, id).await? == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
