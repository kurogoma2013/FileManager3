use crate::models::ApiError;
use axum::{
    body::Body,
    http::header,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::{env, path::PathBuf};

const APK_FILE_NAME: &str = "filemanager-android-release.apk";

#[derive(Debug, Deserialize, Serialize)]
pub struct AndroidUpdateManifest {
    pub version_code: i64,
    pub version_name: String,
    pub download_url: String,
    pub sha256: String,
    pub size_bytes: usize,
    pub release_notes: String,
}

fn release_directory() -> PathBuf {
    env::var("FILEMANAGER_ANDROID_RELEASE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("dist/android-release"))
}

fn apk_path() -> PathBuf {
    release_directory().join(APK_FILE_NAME)
}

pub async fn latest() -> Result<Json<AndroidUpdateManifest>, ApiError> {
    let bytes = tokio::fs::read(release_directory().join("latest.json"))
        .await
        .map_err(|_| ApiError::NotFound)?;
    serde_json::from_slice(&bytes)
        .map(Json)
        .map_err(|_| ApiError::NotFound)
}

pub async fn release_apk() -> Result<Response, ApiError> {
    let bytes = tokio::fs::read(apk_path())
        .await
        .map_err(|_| ApiError::NotFound)?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                "application/vnd.android.package-archive",
            ),
            (header::CACHE_CONTROL, "no-store"),
        ],
        Body::from(bytes),
    )
        .into_response())
}
