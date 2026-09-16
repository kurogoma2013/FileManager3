use crate::core::can_manage_projects;
use crate::handlers::auth_handlers::{
    authenticate_admin, authenticate_headers, authenticate_request,
};
use crate::handlers::projects::normalize_phone;
use crate::maintenance;
use crate::models::{ApiError, AppState, CreateDealerRequest, Dealer, DealerTrashItem};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use std::sync::Arc;

pub async fn list_dealers(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<Dealer>>, ApiError> {
    let dealers: Vec<Dealer> = sqlx::query_as(
        "SELECT d.id, d.name, d.kana, d.address, d.phone, d.fax, d.email, COUNT(p.id) AS project_count
         FROM dealers d
         LEFT JOIN projects p ON p.dealer = d.name AND p.deleted_at IS NULL
         WHERE d.deleted_at IS NULL
         GROUP BY d.id, d.name, d.kana, d.address, d.phone, d.fax, d.email
         ORDER BY d.name ASC",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(dealers))
}

pub async fn create_dealer(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<CreateDealerRequest>,
) -> Result<Json<Dealer>, ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if !can_manage_projects(&user.1) {
        return Err(ApiError::Forbidden);
    }

    let name = request.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("販売店名を入力してください"));
    }
    let kana = request.kana.unwrap_or_default().trim().to_string();
    let address = request.address.unwrap_or_default();
    let phone = normalize_phone(request.phone.as_deref()).unwrap_or_default();
    let fax = normalize_phone(request.fax.as_deref()).unwrap_or_default();
    let email = request.email.as_deref().unwrap_or("").trim();

    let result = sqlx::query_scalar::<_,  i64>(
        "INSERT INTO dealers (name, kana, address, phone, fax, email) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(name)
    .bind(&kana)
    .bind(&address)
    .bind(&phone)
    .bind(&fax)
    .bind(email)
    .fetch_one(&state.pool)
    .await;

    match result {
        Ok(res) => Ok(Json(Dealer {
            id: res,
            name: name.to_string(),
            kana: Some(kana.to_string()),
            address: Some(address.to_string()),
            phone: Some(phone.to_string()),
            fax: Some(fax.to_string()),
            email: Some(email.to_string()),
            project_count: 0,
        })),
        Err(sqlx::Error::Database(err)) if err.is_unique_violation() => {
            Err(ApiError::BadRequest("この販売店名は既に登録されています"))
        }
        Err(err) => {
            tracing::error!(error = %err, "database operation failed");
            Err(ApiError::Database)
        }
    }
}

pub async fn update_dealer(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Json(request): Json<CreateDealerRequest>,
) -> Result<Json<Dealer>, ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if !can_manage_projects(&user.1) {
        return Err(ApiError::Forbidden);
    }

    let name = request.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("販売店名を入力してください"));
    }
    let kana = request.kana.unwrap_or_default().trim().to_string();
    let address = request.address.unwrap_or_default();
    let phone = normalize_phone(request.phone.as_deref()).unwrap_or_default();
    let fax = normalize_phone(request.fax.as_deref()).unwrap_or_default();
    let email = request.email.as_deref().unwrap_or("").trim();

    let mut transaction = state.pool.begin().await?;
    let old_name: Option<String> =
        sqlx::query_scalar("SELECT name FROM dealers WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(&mut *transaction)
            .await?;
    let Some(old_name) = old_name else {
        return Err(ApiError::NotFound);
    };

    let result = sqlx::query(
        "UPDATE dealers SET name = $1, kana = $2, address = $3, phone = $4, fax = $5, email = $6 WHERE id = $7 AND deleted_at IS NULL",
    )
    .bind(name)
    .bind(&kana)
    .bind(&address)
    .bind(&phone)
    .bind(&fax)
    .bind(email)
    .bind(id)
    .execute(&mut *transaction)
    .await;

    match result {
        Ok(res) if res.rows_affected() == 0 => Err(ApiError::NotFound),
        Ok(_) => {
            sqlx::query(
                "UPDATE projects SET dealer = $1, updated_at = CURRENT_TIMESTAMP WHERE dealer = $2",
            )
            .bind(name)
            .bind(&old_name)
            .execute(&mut *transaction)
            .await?;
            sqlx::query("UPDATE dealer_contacts SET dealer_name = $1 WHERE dealer_name = $2")
                .bind(name)
                .bind(&old_name)
                .execute(&mut *transaction)
                .await?;
            let project_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM projects WHERE dealer = $1 AND deleted_at IS NULL",
            )
            .bind(name)
            .fetch_one(&mut *transaction)
            .await?;
            transaction.commit().await?;
            Ok(Json(Dealer {
                id,
                name: name.to_string(),
                kana: Some(kana.to_string()),
                address: Some(address.to_string()),
                phone: Some(phone.to_string()),
                fax: Some(fax.to_string()),
                email: Some(email.to_string()),
                project_count,
            }))
        }
        Err(sqlx::Error::Database(err)) if err.is_unique_violation() => {
            Err(ApiError::BadRequest("この販売店名は既に登録されています"))
        }
        Err(err) => {
            tracing::error!(error = %err, "database operation failed");
            Err(ApiError::Database)
        }
    }
}

pub async fn delete_dealer(
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

    if maintenance::soft_delete_dealer(&state.pool, id).await? == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn restore_dealer(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let user = authenticate_request(&state, &headers).await?;
    if !can_manage_projects(&user.1) {
        return Err(ApiError::Forbidden);
    }
    if maintenance::restore_dealer(&state.pool, id).await? == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_deleted_dealers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<DealerTrashItem>>, ApiError> {
    let user = authenticate_request(&state, &headers).await?;
    if !can_manage_projects(&user.1) {
        return Err(ApiError::Forbidden);
    }
    Ok(Json(maintenance::list_deleted_dealers(&state.pool).await?))
}

pub async fn permanently_delete_dealer(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    authenticate_admin(&state, &headers).await?;
    if maintenance::permanently_delete_dealer(&state.pool, id).await? == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

use crate::models::{CreateDealerContactRequest, DealerContact};
pub async fn list_dealer_contacts(
    Path(dealer_name): Path<String>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<DealerContact>>, ApiError> {
    let user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    if user.1 == "viewer" {
        return Err(ApiError::Forbidden);
    }
    let contacts: Vec<DealerContact> = sqlx::query_as(
        "SELECT id, dealer_name, name, phone, email FROM dealer_contacts WHERE dealer_name = $1 AND deleted_at IS NULL ORDER BY created_at ASC"
    )
    .bind(dealer_name)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(contacts))
}

pub async fn create_dealer_contact(
    Path(dealer_name): Path<String>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateDealerContactRequest>,
) -> Result<StatusCode, ApiError> {
    let user = authenticate_request(&state, &headers).await?;
    if !can_manage_projects(&user.1) {
        return Err(ApiError::Forbidden);
    }
    let dealer_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM dealers WHERE name = $1 AND deleted_at IS NULL")
            .bind(&dealer_name)
            .fetch_optional(&state.pool)
            .await?;
    if dealer_exists.is_none() {
        return Err(ApiError::NotFound);
    }
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("担当者名を入力してください"));
    }
    let email = payload.email.as_deref().unwrap_or("").trim();
    sqlx::query(
        "INSERT INTO dealer_contacts (dealer_name, name, phone, email) VALUES ($1, $2, $3, $4)",
    )
    .bind(dealer_name)
    .bind(name)
    .bind(normalize_phone(Some(payload.phone.trim())).unwrap_or_default())
    .bind(email)
    .execute(&state.pool)
    .await?;
    Ok(StatusCode::CREATED)
}

pub async fn delete_dealer_contact(
    Path(id): Path<i64>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    authenticate_admin(&state, &headers).await?;
    let rows = sqlx::query("UPDATE dealer_contacts SET deleted_at = CURRENT_TIMESTAMP WHERE id = $1 AND deleted_at IS NULL")
        .bind(id)
        .execute(&state.pool)
        .await?
        .rows_affected();
    if rows == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn permanently_delete_dealer_contact(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    authenticate_admin(&state, &headers).await?;
    let result =
        sqlx::query("DELETE FROM dealer_contacts WHERE id = $1 AND deleted_at IS NOT NULL")
            .bind(id)
            .execute(&state.pool)
            .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn update_dealer_contact(
    Path(id): Path<i64>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateDealerContactRequest>,
) -> Result<StatusCode, ApiError> {
    authenticate_admin(&state, &headers).await?;
    let name = payload.name.trim();
    let phone = normalize_phone(Some(payload.phone.trim())).unwrap_or_default();
    let email = payload.email.as_deref().unwrap_or("").trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("担当者名を入力してください"));
    }
    let rows = sqlx::query(
        "UPDATE dealer_contacts SET name = $1, phone = $2, email = $3 WHERE id = $4 AND deleted_at IS NULL",
    )
    .bind(name)
    .bind(phone)
    .bind(email)
    .bind(id)
    .execute(&state.pool)
    .await?
    .rows_affected();
    if rows == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
