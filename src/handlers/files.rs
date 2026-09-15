use crate::content;
use crate::core::{can_manage_projects, can_move_file, can_upload_files};
use crate::handlers::auth_handlers::{
    authenticate_admin, authenticate_headers, require_project_access,
};
use crate::maintenance;
use crate::models::{
    ApiError, AppState, BatchDeleteRequest, BatchMoveRequest, FileAuditItem, FileItem,
};
use crate::storage;
use axum::{
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::DbPool;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct FileSearchResult {
    pub file_id: i64,
    pub project_id: i64,
    pub filename: String,
    pub snippet: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct FileHistoryItem {
    pub id: i64,
    pub version_number: i32,
    pub file_path: String,
    pub file_type: String,
    pub file_hash: String,
    pub source_hash: String,
    pub file_size: i64,
    pub tag: String,
    pub archived_at: String,
}

fn storage_path_for_hash(file_hash: &str, filename: &str) -> String {
    let extension = std::path::Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension.is_empty() {
        file_hash.to_string()
    } else {
        format!("{file_hash}.{extension}")
    }
}

async fn shared_storage_path(
    pool: &DbPool,
    file_hash: &str,
) -> Result<Option<String>, sqlx::Error> {
    crate::db::query_scalar(
        "SELECT storage_path FROM files WHERE file_hash = ? AND deleted_at IS NULL LIMIT 1",
    )
    .bind(file_hash)
    .fetch_optional(pool)
    .await
}

async fn active_file_by_source_hash(
    pool: &DbPool,
    project_id: i64,
    source_hash: &str,
    file_type: &str,
) -> Result<Option<i64>, sqlx::Error> {
    crate::db::query_scalar(
        "SELECT id FROM files WHERE project_id = ? AND source_hash = ? AND file_type = ? AND deleted_at IS NULL LIMIT 1",
    )
    .bind(project_id)
    .bind(source_hash)
    .bind(file_type)
    .fetch_optional(pool)
    .await
}

async fn load_file_item(pool: &DbPool, id: i64) -> Result<FileItem, sqlx::Error> {
    crate::db::query_as::< FileItem>(
        "SELECT id, version_number, file_name AS file_path, file_type, file_hash, source_hash, tag, file_size,
            strftime('%Y-%m-%d %H:%M:%S', created_at, '+9 hours') AS created_at
         FROM files WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct FileSearchParams {
    pub q: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateFileTagRequest {
    pub tag: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct BatchFilesQuery {
    pub file_ids: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct UploadFileQuery {
    pub category: Option<String>,
}

pub fn is_picture(filename: &str) -> bool {
    matches!(
        std::path::Path::new(filename)
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())
            .as_deref(),
        Some("jpg") | Some("jpeg") | Some("png") | Some("webp") | Some("gif")
    )
}

pub fn is_video(filename: &str) -> bool {
    matches!(
        std::path::Path::new(filename)
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())
            .as_deref(),
        Some("mp4") | Some("m4v") | Some("mov") | Some("webm") | Some("ogv")
    )
}

fn is_picture_or_video(filename: &str) -> bool {
    is_picture(filename) || is_video(filename)
}

pub fn encode_picture_to_webp(filename: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    let stem = std::path::Path::new(filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(filename);

    if let Ok(image) = image::load_from_memory(bytes) {
        let rgba = image.to_rgba8();
        let (width, height) = (rgba.width(), rgba.height());
        let encoder = webp::Encoder::from_rgba(&rgba, width, height);
        let webp_data = encoder.encode(75.0);
        (format!("{stem}.webp"), webp_data.to_vec())
    } else {
        (filename.to_string(), bytes.to_vec())
    }
}

pub fn display_file_name(path: &str) -> String {
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(path);
    let uuid_prefix_len = 37;
    if name.len() > uuid_prefix_len && name.as_bytes().get(36) == Some(&b'_') {
        name[uuid_prefix_len..].to_string()
    } else {
        name.to_string()
    }
}

struct ZipEntry {
    name: String,
    bytes: Vec<u8>,
}

struct ZipCentralEntry {
    name: String,
    crc32: u32,
    size: u32,
    offset: u32,
}

fn write_u16_le(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_u32_le(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn zip_u16(value: usize) -> Result<u16, ApiError> {
    u16::try_from(value).map_err(|_| ApiError::BadRequest("ZIP化できる件数を超えています"))
}

fn zip_u32(value: usize) -> Result<u32, ApiError> {
    u32::try_from(value).map_err(|_| ApiError::BadRequest("ZIP化できるサイズを超えています"))
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn unique_zip_name(name: &str, used: &mut HashSet<String>) -> String {
    let clean = name.replace(['/', '\\'], "_");
    if used.insert(clean.clone()) {
        return clean;
    }
    let path = std::path::Path::new(&clean);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(&clean);
    let extension = path.extension().and_then(|value| value.to_str());
    for index in 2.. {
        let candidate = match extension {
            Some(extension) if !extension.is_empty() => format!("{stem} ({index}).{extension}"),
            _ => format!("{stem} ({index})"),
        };
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!("infinite range always returns a unique ZIP filename")
}

fn build_zip_archive(entries: Vec<ZipEntry>) -> Result<Vec<u8>, ApiError> {
    let mut out = Vec::new();
    let mut central_entries = Vec::with_capacity(entries.len());
    let dos_time = 0;
    let dos_date = 0x0021;
    for entry in entries {
        let name_bytes = entry.name.as_bytes();
        let name_len = zip_u16(name_bytes.len())?;
        let size = zip_u32(entry.bytes.len())?;
        let offset = zip_u32(out.len())?;
        let crc32 = crc32(&entry.bytes);

        write_u32_le(&mut out, 0x0403_4b50);
        write_u16_le(&mut out, 20);
        write_u16_le(&mut out, 0x0800);
        write_u16_le(&mut out, 0);
        write_u16_le(&mut out, dos_time);
        write_u16_le(&mut out, dos_date);
        write_u32_le(&mut out, crc32);
        write_u32_le(&mut out, size);
        write_u32_le(&mut out, size);
        write_u16_le(&mut out, name_len);
        write_u16_le(&mut out, 0);
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(&entry.bytes);

        central_entries.push(ZipCentralEntry {
            name: entry.name,
            crc32,
            size,
            offset,
        });
    }

    let central_offset = zip_u32(out.len())?;
    for entry in &central_entries {
        let name_bytes = entry.name.as_bytes();
        let name_len = zip_u16(name_bytes.len())?;
        write_u32_le(&mut out, 0x0201_4b50);
        write_u16_le(&mut out, 20);
        write_u16_le(&mut out, 20);
        write_u16_le(&mut out, 0x0800);
        write_u16_le(&mut out, 0);
        write_u16_le(&mut out, dos_time);
        write_u16_le(&mut out, dos_date);
        write_u32_le(&mut out, entry.crc32);
        write_u32_le(&mut out, entry.size);
        write_u32_le(&mut out, entry.size);
        write_u16_le(&mut out, name_len);
        write_u16_le(&mut out, 0);
        write_u16_le(&mut out, 0);
        write_u16_le(&mut out, 0);
        write_u16_le(&mut out, 0);
        write_u32_le(&mut out, 0);
        write_u32_le(&mut out, entry.offset);
        out.extend_from_slice(name_bytes);
    }
    let central_size = zip_u32(out.len() - central_offset as usize)?;
    let entry_count = zip_u16(central_entries.len())?;
    write_u32_le(&mut out, 0x0605_4b50);
    write_u16_le(&mut out, 0);
    write_u16_le(&mut out, 0);
    write_u16_le(&mut out, entry_count);
    write_u16_le(&mut out, entry_count);
    write_u32_le(&mut out, central_size);
    write_u32_le(&mut out, central_offset);
    write_u16_le(&mut out, 0);
    Ok(out)
}

pub fn file_content_type(path: &str) -> &'static str {
    match std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("mp4") | Some("m4v") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("webm") => "video/webm",
        Some("ogv") => "video/ogg",
        Some("pdf") => "application/pdf",
        Some("txt") | Some("md") | Some("csv") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn browser_openable(path: &str) -> bool {
    matches!(
        file_content_type(path),
        "image/jpeg"
            | "image/png"
            | "image/webp"
            | "image/gif"
            | "application/pdf"
            | "text/plain; charset=utf-8"
            | "video/mp4"
            | "video/quicktime"
            | "video/webm"
            | "video/ogg"
    )
}

async fn active_file_by_name(
    pool: &DbPool,
    project_id: i64,
    filename: &str,
    file_type: &str,
) -> Result<Option<(i64, i32, String, String, String, i64)>, sqlx::Error> {
    crate::db::query_as(
        "SELECT id, version_number, storage_path, file_type, file_hash, file_size FROM files
         WHERE project_id = ? AND deleted_at IS NULL
           AND file_name = ? AND file_type = ?
         ORDER BY version_number DESC, id DESC LIMIT 1",
    )
    .bind(project_id)
    .bind(filename)
    .bind(file_type)
    .fetch_optional(pool)
    .await
}

async fn require_history_admin(
    headers: &HeaderMap,
    state: &AppState,
    project_id: i64,
) -> Result<(), ApiError> {
    require_project_access(headers, state, project_id).await?;
    let user = authenticate_headers(&state.pool, headers)
        .await
        .map_err(|_| ApiError::Database)?
        .ok_or(ApiError::Unauthorized)?;
    if user.1 != "admin" {
        return Err(ApiError::Forbidden);
    }
    Ok(())
}

fn parse_file_ids(value: &str) -> Vec<i64> {
    value
        .split(',')
        .filter_map(|part| part.trim().parse::<i64>().ok())
        .filter(|id| *id > 0)
        .collect()
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

async fn batch_file_rows(
    pool: &DbPool,
    file_ids: &[i64],
) -> Result<Vec<(i64, String, String, i64)>, ApiError> {
    if file_ids.is_empty() {
        return Err(ApiError::BadRequest("ファイルを選択してください"));
    }
    let mut rows = Vec::with_capacity(file_ids.len());
    for file_id in file_ids {
        let row: Option<(i64, String, String, i64)> = crate::db::query_as(
        "SELECT id, file_name, file_type, project_id FROM files WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(file_id)
        .fetch_optional(pool)
        .await?;
        rows.push(row.ok_or(ApiError::NotFound)?);
    }
    Ok(rows)
}

pub async fn file_project_id(pool: &DbPool, id: i64) -> Result<i64, ApiError> {
    crate::db::query_scalar("SELECT project_id FROM files WHERE id = ? AND deleted_at IS NULL")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(ApiError::NotFound)
}

pub async fn require_upload_access(
    headers: &HeaderMap,
    state: &AppState,
    project_id: i64,
) -> Result<(i64, String), ApiError> {
    let user = authenticate_headers(&state.pool, headers)
        .await
        .map_err(|_| ApiError::Database)?
        .ok_or(ApiError::Unauthorized)?;
    let exists: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM projects WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(project_id)
    .fetch_one(&state.pool)
    .await?;
    if exists == 0 {
        return Err(ApiError::NotFound);
    }
    if can_upload_files(&user.1)
        || crate::db::query_scalar::< i64>(
            "SELECT COUNT(*) FROM project_permissions pp JOIN projects p ON p.id = pp.project_id WHERE pp.user_id = ? AND pp.project_id = ? AND p.deleted_at IS NULL",
        )
        .bind(user.0)
        .bind(project_id)
        .fetch_one(&state.pool)
        .await?
            > 0
    {
        Ok(user)
    } else {
        Err(ApiError::Forbidden)
    }
}

pub async fn file_thumbnail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let file_info: Option<(String, i64)> = crate::db::query_as(
        "SELECT storage_path, project_id FROM files WHERE id = ? AND file_type IN ('Picture', 'Document') AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let (storage_path, project_id) = file_info.ok_or(ApiError::NotFound)?;
    require_project_access(&headers, &state, project_id).await?;
    let content_type = match std::path::Path::new(&storage_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => return Err(ApiError::NotFound),
    };
    let bytes = tokio::fs::read(state.storage_root.join(storage_path))
        .await
        .map_err(|_| ApiError::NotFound)?;
    Ok(([(header::CONTENT_TYPE, content_type)], Body::from(bytes)).into_response())
}

pub async fn upload_file(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<i64>,
    headers: HeaderMap,
    Query(query): Query<UploadFileQuery>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<FileItem>), ApiError> {
    let user = require_upload_access(&headers, &state, project_id).await?;
    let _storage_guard = maintenance::storage_operation_guard().await;
    let exists: i64 = crate::db::query_scalar(
        "SELECT COUNT(*) FROM projects WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(project_id)
    .fetch_one(&state.pool)
    .await?;
    if exists == 0 {
        return Err(ApiError::NotFound);
    }

    let mut tag = String::new();
    let mut file_info: Option<(String, String, i64, String, String)> = None;
    let mut temporary_path: Option<std::path::PathBuf> = None;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::BadRequest("multipartが不正です"))?
    {
        if field.name() == Some("file") {
            let mut filename = field
                .file_name()
                .map(str::to_string)
                .ok_or(ApiError::BadRequest("ファイル名がありません"))?;
            storage::safe_filename(&filename).map_err(ApiError::BadRequest)?;

            let is_pic_or_video = is_picture_or_video(&filename);
            let file_type = match query.category.as_deref() {
                Some("documents") => "Document",
                Some("pictures") if is_pic_or_video => "Picture",
                Some("pictures") => {
                    return Err(ApiError::BadRequest(
                        "写真・動画には画像または動画を指定してください",
                    ));
                }
                Some(_) => return Err(ApiError::BadRequest("アップロード先が不正です")),
                None if is_pic_or_video => "Picture",
                None => "Document",
            };

            tokio::fs::create_dir_all(&state.storage_root)
                .await
                .map_err(|_| ApiError::Storage)?;

            let mut final_size = 0i64;
            let final_hash;
            let source_hash;

            if is_picture(&filename) {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|_| ApiError::BadRequest("ファイルを読み込めません"))?
                    .to_vec();
                source_hash = format!("{:x}", Sha256::digest(&bytes));
                if let Some(existing_id) =
                    active_file_by_source_hash(&state.pool, project_id, &source_hash, file_type)
                        .await?
                {
                    touch_project_updated_at(&state.pool, project_id).await?;
                    let item = load_file_item(&state.pool, existing_id).await?;
                    return Ok((StatusCode::OK, Json(item)));
                }
                let (final_filename, final_bytes) =
                    tokio::task::spawn_blocking(move || encode_picture_to_webp(&filename, &bytes))
                        .await
                        .map_err(|_| ApiError::Storage)?;

                filename = final_filename;
                final_size = final_bytes.len() as i64;
                final_hash = format!("{:x}", Sha256::digest(&final_bytes));
                let temp_path = state
                    .storage_root
                    .join(format!(".upload-{}", uuid::Uuid::new_v4()));
                tokio::fs::write(&temp_path, &final_bytes)
                    .await
                    .map_err(|_| ApiError::Storage)?;
                temporary_path = Some(temp_path);
            } else {
                let temp_path = state
                    .storage_root
                    .join(format!(".upload-{}", uuid::Uuid::new_v4()));
                let mut file = tokio::fs::File::create(&temp_path)
                    .await
                    .map_err(|_| ApiError::Storage)?;
                let mut hasher = Sha256::new();

                while let Some(chunk) = field
                    .chunk()
                    .await
                    .map_err(|_| ApiError::BadRequest("読み込みエラー"))?
                {
                    tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
                        .await
                        .map_err(|_| ApiError::Storage)?;
                    hasher.update(&chunk);
                    final_size += chunk.len() as i64;
                }
                final_hash = format!("{:x}", hasher.finalize());
                source_hash = final_hash.clone();
                temporary_path = Some(temp_path);
            }
            file_info = Some((
                filename,
                file_type.to_string(),
                final_size,
                final_hash,
                source_hash,
            ));
        } else if matches!(field.name(), Some("tag") | Some("tags")) {
            tag = field
                .text()
                .await
                .map_err(|_| ApiError::BadRequest("タグを読み込めません"))?
                .trim()
                .to_string();
        }
    }

    let Some((filename, file_type, file_size, file_hash, source_hash)) = file_info else {
        return Err(ApiError::BadRequest("fileフィールドが必要です"));
    };
    let temporary_path = temporary_path.ok_or(ApiError::Storage)?;

    if file_type != "Picture" {
        tag.clear();
    }

    let existing = active_file_by_name(&state.pool, project_id, &filename, &file_type).await?;
    if let Some((existing_id, _, _, _, existing_hash, _)) = &existing {
        if !existing_hash.is_empty() && existing_hash == &file_hash {
            let _ = tokio::fs::remove_file(&temporary_path).await;
            touch_project_updated_at(&state.pool, project_id).await?;
            let item = load_file_item(&state.pool, *existing_id).await?;
            return Ok((StatusCode::OK, Json(item)));
        }
    }

    let mut created_storage_path = None;
    let relative_path =
        if let Some(existing_path) = shared_storage_path(&state.pool, &file_hash).await? {
            let _ = tokio::fs::remove_file(&temporary_path).await;
            existing_path
        } else {
            let path = storage_path_for_hash(&file_hash, &filename);
            let destination = state.storage_root.join(&path);
            tokio::fs::rename(&temporary_path, &destination)
                .await
                .map_err(|_| ApiError::Storage)?;
            created_storage_path = Some(destination);
            path
        };

    let persistence_result: Result<i64, ApiError> = async {
        let mut transaction = state.pool.begin().await?;
        let version_number = if let Some((old_id, old_version, _, _, _, _)) = existing {
        crate::db::query(
            "INSERT INTO file_histories (file_id, project_id, version_number, file_name, storage_path, file_type, file_hash, source_hash, file_size, tag)
             SELECT id, project_id, version_number, file_name, storage_path, file_type, file_hash, source_hash, file_size, tag
             FROM files WHERE id = ?",
        )
        .bind(old_id)
        .execute(&mut *transaction)
        .await?;
        crate::db::query(
            "UPDATE files SET deleted_at = CURRENT_TIMESTAMP, deleted_by = ? WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(user.0)
        .bind(old_id)
        .execute(&mut *transaction)
        .await?;
        crate::db::query("DELETE FROM file_search WHERE file_id = ?")
            .bind(old_id)
            .execute(&mut *transaction)
            .await?;
        old_version + 1
    } else {
        1
        };
        let file_id = crate::db::query_scalar::< i64>(
            "INSERT INTO files (project_id, file_name, storage_path, file_type, version_number, tag, file_size, file_hash, source_hash, uploaded_by) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(project_id)
    .bind(&filename)
    .bind(&relative_path)
    .bind(&file_type)
    .bind(version_number)
    .bind(&tag)
    .bind(file_size)
    .bind(&file_hash)
    .bind(&source_hash)
    .bind(user.0)
    .fetch_one(&mut *transaction)
    .await?;

        let storage_path = state.storage_root.join(&relative_path);
        let filename_clone = filename.clone();
        let text = tokio::task::spawn_blocking(move || {
        content::extract_text(&storage_path, &filename_clone)
            .ok()
            .flatten()
            .unwrap_or_default()
        })
        .await
        .map_err(|_| ApiError::Storage)?;

        crate::db::query(
        "INSERT INTO file_search (file_id, project_id, filename, content) VALUES (?, ?, ?, ?)",
    )
    .bind(file_id)
    .bind(project_id)
    .bind(&filename)
    .bind(text)
        .execute(&mut *transaction)
        .await?;
        crate::db::query("UPDATE projects SET updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(project_id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(file_id)
    }
    .await;
    let file_id = match persistence_result {
        Ok(file_id) => file_id,
        Err(error) => {
            if let Some(path) = created_storage_path {
                let _ = tokio::fs::remove_file(path).await;
            }
            return Err(error);
        }
    };
    let item = crate::db::query_as::< FileItem>(
        "SELECT id, version_number, file_name AS file_path, file_type, file_hash, source_hash, tag, file_size,
            strftime('%Y-%m-%d %H:%M:%S', created_at, '+9 hours') AS created_at FROM files WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(file_id)
    .fetch_one(&state.pool)
    .await?;
    tracing::debug!(file_id, "file stored");
    Ok((StatusCode::CREATED, Json(item)))
}

async fn touch_project_updated_at(pool: &DbPool, project_id: i64) -> Result<(), ApiError> {
    crate::db::query("UPDATE projects SET updated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn download_file(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let file_info: Option<(String, i64, String)> = crate::db::query_as(
        "SELECT storage_path, project_id, file_name FROM files WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let (storage_path, project_id, file_name) = file_info.ok_or(ApiError::NotFound)?;
    require_project_access(&headers, &state, project_id).await?;
    let file = tokio::fs::File::open(state.storage_root.join(&storage_path))
        .await
        .map_err(|_| ApiError::NotFound)?;
    let stream = tokio_util::io::ReaderStream::new(file);
    let filename = display_file_name(&file_name);
    let disposition = format!(
        "attachment; filename=\"{}\"",
        filename.replace(['\\', '"'], "_")
    );
    Response::builder()
        .header(header::CONTENT_TYPE, file_content_type(&storage_path))
        .header(header::CONTENT_DISPOSITION, disposition)
        .body(Body::from_stream(stream))
        .map_err(|_| ApiError::Storage)
}

pub async fn open_file(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let file_info: Option<(String, i64, String)> = crate::db::query_as(
        "SELECT storage_path, project_id, file_name FROM files WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let (storage_path, project_id, file_name) = file_info.ok_or(ApiError::NotFound)?;
    require_project_access(&headers, &state, project_id).await?;
    let file = tokio::fs::File::open(state.storage_root.join(&storage_path))
        .await
        .map_err(|_| ApiError::NotFound)?;
    let stream = tokio_util::io::ReaderStream::new(file);
    let disposition = if browser_openable(&storage_path) {
        "inline".to_string()
    } else {
        format!(
            "attachment; filename=\"{}\"",
            display_file_name(&file_name).replace(['\\', '"'], "_")
        )
    };
    Response::builder()
        .header(header::CONTENT_TYPE, file_content_type(&storage_path))
        .header(header::CONTENT_DISPOSITION, disposition)
        .body(Body::from_stream(stream))
        .map_err(|_| ApiError::Storage)
}

pub async fn list_file_history(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Json<Vec<FileHistoryItem>>, ApiError> {
    let file_info: Option<(String, i64)> = crate::db::query_as(
        "SELECT file_name, project_id FROM files WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let (file_name, project_id) = file_info.ok_or(ApiError::NotFound)?;
    require_history_admin(&headers, &state, project_id).await?;
    let histories = crate::db::query_as::< FileHistoryItem>(
        "SELECT id, version_number, file_name AS file_path, file_type, file_hash, source_hash, file_size, tag, archived_at
         FROM file_histories
         WHERE project_id = ? AND file_name = ?
         ORDER BY version_number DESC, id DESC",
    )
    .bind(project_id)
    .bind(&file_name)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(histories))
}

pub async fn list_file_audit(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Json<Vec<FileAuditItem>>, ApiError> {
    authenticate_admin(&state, &headers).await?;
    let file = crate::db::query_as::<(i64, String, String)>(
        "SELECT project_id, file_name, file_type FROM files WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    let audit = crate::db::query_as::<FileAuditItem>(
        "SELECT f.version_number, f.file_name AS file_path,
            strftime('%Y-%m-%d %H:%M:%S', f.created_at, '+9 hours') AS uploaded_at,
            strftime('%Y-%m-%d %H:%M:%S', f.deleted_at, '+9 hours') AS deleted_at,
            uploaded.username AS uploaded_by,
            deleted.username AS deleted_by
         FROM files f
         LEFT JOIN users uploaded ON uploaded.id = f.uploaded_by
         LEFT JOIN users deleted ON deleted.id = f.deleted_by
         WHERE f.project_id = ? AND f.file_name = ? AND f.file_type = ?
         ORDER BY f.version_number DESC, f.id DESC",
    )
    .bind(file.0)
    .bind(file.1)
    .bind(file.2)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(audit))
}

pub async fn download_file_history(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let history: Option<(String, i64, String)> = crate::db::query_as(
        "SELECT storage_path, project_id, file_name FROM file_histories WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    let (storage_path, project_id, file_name) = history.ok_or(ApiError::NotFound)?;
    require_history_admin(&headers, &state, project_id).await?;
    let file = tokio::fs::File::open(state.storage_root.join(&storage_path))
        .await
        .map_err(|_| ApiError::NotFound)?;
    let stream = tokio_util::io::ReaderStream::new(file);
    let filename = display_file_name(&file_name);
    let disposition = format!(
        "attachment; filename=\"{}\"",
        filename.replace(['\\', '"'], "_")
    );
    Response::builder()
        .header(header::CONTENT_TYPE, file_content_type(&storage_path))
        .header(header::CONTENT_DISPOSITION, disposition)
        .body(Body::from_stream(stream))
        .map_err(|_| ApiError::Storage)
}

pub async fn update_file_tag(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Json(request): Json<UpdateFileTagRequest>,
) -> Result<Json<FileItem>, ApiError> {
    let file_type: String =
        crate::db::query_scalar("SELECT file_type FROM files WHERE id = ? AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(ApiError::NotFound)?;
    let project_id = file_project_id(&state.pool, id).await?;
    let user = require_upload_access(&headers, &state, project_id).await?;
    if !can_move_file(&user.1, &file_type) {
        return Err(ApiError::Forbidden);
    }
    crate::db::query("UPDATE files SET tag = ? WHERE id = ? AND deleted_at IS NULL")
        .bind(request.tag.trim())
        .bind(id)
        .execute(&state.pool)
        .await?;
    let item = crate::db::query_as::< FileItem>(
        "SELECT id, version_number, file_name AS file_path, file_type, file_hash, source_hash, tag, file_size,
            strftime('%Y-%m-%d %H:%M:%S', created_at, '+9 hours') AS created_at FROM files WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(item))
}

pub async fn delete_file(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let project_id = file_project_id(&state.pool, id).await?;
    let user = require_upload_access(&headers, &state, project_id).await?;
    if !can_manage_projects(&user.1) {
        return Err(ApiError::Forbidden);
    }
    let result = crate::db::query(
        "UPDATE files SET deleted_at = CURRENT_TIMESTAMP, deleted_by = ? WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(user.0)
    .bind(id)
    .execute(&state.pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn permanently_delete_file(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    authenticate_admin(&state, &headers).await?;
    if maintenance::permanently_delete_file(&state.pool, &state.storage_root, id).await? == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn batch_delete_files(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<BatchDeleteRequest>,
) -> Result<StatusCode, ApiError> {
    let rows = batch_file_rows(&state.pool, &request.file_ids).await?;
    let mut deleted_by = None;
    for (_, _, _, project_id) in &rows {
        let user = require_upload_access(&headers, &state, *project_id).await?;
        if !can_manage_projects(&user.1) {
            return Err(ApiError::Forbidden);
        }
        deleted_by = Some(user.0);
    }
    let Some(deleted_by) = deleted_by else {
        return Ok(StatusCode::NO_CONTENT);
    };
    for (file_id, _, _, _) in rows {
        crate::db::query(
            "UPDATE files SET deleted_at = CURRENT_TIMESTAMP, deleted_by = ? WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(deleted_by)
        .bind(file_id)
        .execute(&state.pool)
        .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn batch_move_files(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<BatchMoveRequest>,
) -> Result<StatusCode, ApiError> {
    let rows = batch_file_rows(&state.pool, &request.file_ids).await?;
    let tag = request.tag.unwrap_or_default().trim().to_string();
    for (_, _, _, project_id) in &rows {
        let user = require_upload_access(&headers, &state, *project_id).await?;
        if !rows
            .iter()
            .all(|(_, _, file_type, _)| can_move_file(&user.1, file_type))
        {
            return Err(ApiError::Forbidden);
        }
    }
    for (file_id, _, _, _) in rows {
        crate::db::query("UPDATE files SET tag = ? WHERE id = ? AND deleted_at IS NULL")
            .bind(&tag)
            .bind(file_id)
            .execute(&state.pool)
            .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn batch_download_files(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<BatchFilesQuery>,
) -> Result<Response, ApiError> {
    let file_ids = parse_file_ids(&query.file_ids);
    let rows = batch_file_rows(&state.pool, &file_ids).await?;
    for (_, _, _, project_id) in &rows {
        require_project_access(&headers, &state, *project_id).await?;
    }
    let mut used_names = HashSet::new();
    let mut entries = Vec::with_capacity(rows.len());
    for (file_id, file_path, _, _) in rows {
        let storage_path: String = crate::db::query_scalar(
            "SELECT storage_path FROM files WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(file_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError::NotFound)?;
        let bytes = tokio::fs::read(state.storage_root.join(storage_path))
            .await
            .map_err(|_| ApiError::NotFound)?;
        entries.push(ZipEntry {
            name: unique_zip_name(&display_file_name(&file_path), &mut used_names),
            bytes,
        });
    }
    let zip = build_zip_archive(entries)?;
    Response::builder()
        .header(header::CONTENT_TYPE, "application/zip")
        .header(
            header::CONTENT_DISPOSITION,
            r#"attachment; filename="selected-files.zip""#,
        )
        .body(Body::from(zip))
        .map_err(|_| ApiError::Storage)
}

pub async fn batch_print_files(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<BatchFilesQuery>,
) -> Result<Response, ApiError> {
    let file_ids = parse_file_ids(&query.file_ids);
    let rows = batch_file_rows(&state.pool, &file_ids).await?;
    for (_, _, _, project_id) in &rows {
        require_project_access(&headers, &state, *project_id).await?;
    }
    let content = rows
        .into_iter()
        .map(|(file_id, file_path, _, _)| {
            let name = html_escape(&display_file_name(&file_path));
            let url = format!("/api/files/{file_id}/download");
            match file_content_type(&file_path) {
                content_type if content_type.starts_with("image/") => {
                    format!(r#"<section><h2>{name}</h2><img src="{url}" alt="{name}"></section>"#)
                }
                "application/pdf" => {
                    format!(r#"<section><h2>{name}</h2><iframe src="{url}"></iframe></section>"#)
                }
                _ => format!(r#"<section><h2>{name}</h2><a href="{url}">{name}</a></section>"#),
            }
        })
        .collect::<Vec<_>>()
        .join("");
    let html = format!(
        r#"<!doctype html><html lang="ja"><head><meta charset="utf-8"><title>一括印刷</title><style>body{{font-family:-apple-system,BlinkMacSystemFont,"Helvetica Neue","Noto Sans JP",sans-serif;margin:24px}}section{{break-after:page;margin-bottom:24px}}img,iframe{{max-width:100%;width:100%;border:0}}iframe{{height:90vh}}@media print{{button{{display:none}}body{{margin:0}}}}</style></head><body><button onclick="window.print()">印刷</button>{content}<script>setTimeout(()=>window.print(),800);</script></body></html>"#
    );
    Response::builder()
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(Body::from(html))
        .map_err(|_| ApiError::Storage)
}

pub async fn search_files(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<FileSearchParams>,
) -> Result<Json<Vec<FileSearchResult>>, ApiError> {
    let _user = authenticate_headers(&state.pool, &headers)
        .await
        .map_err(|_| ApiError::Unauthorized)?
        .ok_or(ApiError::Unauthorized)?;
    let query = content::fts_query(&params.q);
    if query.is_empty() {
        return Ok(Json(Vec::new()));
    }
    #[cfg(test)]
    let results = crate::db::query_as::<FileSearchResult>(
        "SELECT fs.file_id, fs.project_id, fs.filename,
                snippet(file_search, 3, '<mark>', '</mark>', '…', 12) AS snippet
         FROM file_search fs
         JOIN files f ON f.id = fs.file_id
         JOIN projects p ON p.id = fs.project_id
         WHERE file_search MATCH ? AND f.deleted_at IS NULL AND p.deleted_at IS NULL
         ORDER BY rank LIMIT 100",
    )
    .bind(query)
    .fetch_all(&state.pool)
    .await?;
    #[cfg(not(test))]
    let results = crate::db::query_as::<FileSearchResult>(
        "SELECT fs.file_id, fs.project_id, fs.filename,
                left(fs.content, 160) AS snippet
         FROM file_search fs
         JOIN files f ON f.id = fs.file_id
         JOIN projects p ON p.id = fs.project_id
         WHERE to_tsvector('simple', coalesce(fs.filename, '') || ' ' || coalesce(fs.content, ''))
             @@ plainto_tsquery('simple', ?) AND f.deleted_at IS NULL AND p.deleted_at IS NULL
         ORDER BY fs.file_id DESC LIMIT 100",
    )
    .bind(query)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(results))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_archive_contains_all_entries() {
        let zip = build_zip_archive(vec![
            ZipEntry {
                name: "first.txt".to_string(),
                bytes: b"hello".to_vec(),
            },
            ZipEntry {
                name: "second.txt".to_string(),
                bytes: b"world".to_vec(),
            },
        ])
        .unwrap();
        assert!(zip.starts_with(b"PK\x03\x04"));
        assert!(zip.windows(b"first.txt".len()).any(|w| w == b"first.txt"));
        assert!(zip.windows(b"second.txt".len()).any(|w| w == b"second.txt"));
        assert!(zip.windows(4).any(|w| w == b"PK\x01\x02"));
        assert!(zip.windows(4).any(|w| w == b"PK\x05\x06"));
        assert_eq!(crc32(b"hello"), 0x3610_a686);
    }

    #[test]
    fn duplicate_zip_names_are_renamed() {
        let mut used = HashSet::new();
        assert_eq!(unique_zip_name("report.pdf", &mut used), "report.pdf");
        assert_eq!(unique_zip_name("report.pdf", &mut used), "report (2).pdf");
        assert_eq!(
            unique_zip_name("dir/report.pdf", &mut used),
            "dir_report.pdf"
        );
    }
}
