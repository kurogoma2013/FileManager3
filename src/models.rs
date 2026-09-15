use crate::db::DbPool;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use webauthn_rs::prelude::*;

pub const SESSION_COOKIE_NAME: &str = "fm3_session";
pub const SESSION_MAX_AGE_SECONDS: i64 = 60 * 60;

#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub storage_root: PathBuf,
    pub access_urls: Vec<String>,
    pub webauthn: Arc<Webauthn>,
    pub secure_cookie: bool,
}

#[derive(Debug)]
pub enum ApiError {
    Unauthorized,
    Forbidden,
    NotFound,
    BadRequest(&'static str),
    Storage,
    Database,
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        tracing::error!(error = %error, "database operation failed");
        Self::Database
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "認証が必要です").into_response(),
            Self::Forbidden => (StatusCode::FORBIDDEN, "アクセス権限がありません").into_response(),
            Self::NotFound => (StatusCode::NOT_FOUND, "見つかりません").into_response(),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
            Self::Storage => {
                (StatusCode::INTERNAL_SERVER_ERROR, "ストレージエラー").into_response()
            }
            Self::Database => (StatusCode::INTERNAL_SERVER_ERROR, "DBエラー").into_response(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct ProjectTrashItem {
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
    pub deleted_at: String,
    pub phone: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct CreateProjectRequest {
    pub project_number: String,
    pub name: String,
    pub kana: String,
    pub address: Option<String>,
    pub dealer: Option<String>,
    pub assignee: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub plus_code: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Dealer {
    pub id: i64,
    pub name: String,
    pub kana: Option<String>,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub fax: Option<String>,
    pub email: Option<String>,
    pub project_count: i64,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct DealerTrashItem {
    pub id: i64,
    pub name: String,
    pub kana: Option<String>,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub fax: Option<String>,
    pub email: Option<String>,
    pub deleted_at: String,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct CreateDealerRequest {
    pub name: String,
    pub kana: Option<String>,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub fax: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub role: String,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct UpdateUserRequest {
    pub password: Option<String>,
    pub role: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct FileItem {
    pub id: i64,
    pub version_number: i32,
    pub file_path: String,
    pub file_type: String,
    pub file_hash: String,
    pub source_hash: String,
    pub tag: Option<String>,
    pub file_size: i64,
    pub created_at: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct FileAuditItem {
    pub version_number: i32,
    pub file_path: String,
    pub uploaded_at: String,
    pub deleted_at: Option<String>,
    pub uploaded_by: Option<String>,
    pub deleted_by: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct CreateNoteRequest {
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct NoteItem {
    pub id: i64,
    pub content: String,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct BatchDeleteRequest {
    pub file_ids: Vec<i64>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct BatchMoveRequest {
    pub file_ids: Vec<i64>,
    pub tag: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct DealerContact {
    pub id: i64,
    pub dealer_name: String,
    pub name: String,
    pub phone: String,
    pub email: String,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct CreateDealerContactRequest {
    pub name: String,
    pub phone: String,
    pub email: Option<String>,
}
