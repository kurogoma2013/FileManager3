use crate::db::DbPool;
use crate::models::{ApiError, DealerTrashItem, ProjectTrashItem};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static STORAGE_OPERATION_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

pub(crate) async fn storage_operation_guard() -> tokio::sync::MutexGuard<'static, ()> {
    STORAGE_OPERATION_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

pub(crate) async fn soft_delete_project(pool: &DbPool, id: i64) -> Result<u64, sqlx::Error> {
    let result = crate::db::query(
        "UPDATE projects SET deleted_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub(crate) async fn soft_delete_dealer(pool: &DbPool, id: i64) -> Result<u64, sqlx::Error> {
    let result = crate::db::query(
        "UPDATE dealers SET deleted_at = CURRENT_TIMESTAMP WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub(crate) async fn restore_project(pool: &DbPool, id: i64) -> Result<u64, sqlx::Error> {
    let result = crate::db::query(
        "UPDATE projects SET deleted_at = NULL, updated_at = CURRENT_TIMESTAMP WHERE id = ? AND deleted_at IS NOT NULL",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub(crate) async fn restore_dealer(pool: &DbPool, id: i64) -> Result<u64, sqlx::Error> {
    let result = crate::db::query(
        "UPDATE dealers SET deleted_at = NULL WHERE id = ? AND deleted_at IS NOT NULL",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub(crate) async fn permanently_delete_project(
    pool: &DbPool,
    storage_root: &Path,
    id: i64,
) -> Result<u64, ApiError> {
    let _storage_guard = storage_operation_guard().await;
    let storage_paths: Vec<String> = crate::db::query_scalar(
        "SELECT storage_path FROM files WHERE project_id = ?
         UNION SELECT storage_path FROM file_histories WHERE project_id = ?",
    )
    .bind(id)
    .bind(id)
    .fetch_all(pool)
    .await?;
    let quarantined =
        quarantine_unreferenced_storage_files(pool, storage_root, storage_paths, None, Some(id))
            .await?;
    let mut transaction = pool.begin().await?;
    let result =
        match crate::db::query("DELETE FROM projects WHERE id = ? AND deleted_at IS NOT NULL")
            .bind(id)
            .execute(&mut *transaction)
            .await
        {
            Ok(result) => result,
            Err(error) => {
                restore_quarantined_storage(&quarantined).await;
                return Err(error.into());
            }
        };
    if result.rows_affected() == 0 {
        restore_quarantined_storage(&quarantined).await;
        return Ok(0);
    }
    if let Err(error) = transaction.commit().await {
        restore_quarantined_storage(&quarantined).await;
        return Err(error.into());
    }
    Ok(result.rows_affected())
}

pub(crate) async fn permanently_delete_dealer(pool: &DbPool, id: i64) -> Result<u64, sqlx::Error> {
    let result = crate::db::query("DELETE FROM dealers WHERE id = ? AND deleted_at IS NOT NULL")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

struct QuarantinedStorage {
    original: PathBuf,
    quarantined: PathBuf,
}

async fn is_referenced_outside_target(
    pool: &DbPool,
    storage_path: &str,
    excluded_file_id: Option<i64>,
    excluded_project_id: Option<i64>,
) -> Result<bool, sqlx::Error> {
    match (excluded_file_id, excluded_project_id) {
        (Some(file_id), None) => {
            crate::db::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM files WHERE storage_path = ? AND id != ?
                    UNION ALL
                    SELECT 1 FROM file_histories WHERE storage_path = ? AND file_id != ?
                )",
            )
            .bind(storage_path)
            .bind(file_id)
            .bind(storage_path)
            .bind(file_id)
            .fetch_one(pool)
            .await
        }
        (None, Some(project_id)) => {
            crate::db::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM files WHERE storage_path = ? AND project_id != ?
                    UNION ALL
                    SELECT 1 FROM file_histories WHERE storage_path = ? AND project_id != ?
                )",
            )
            .bind(storage_path)
            .bind(project_id)
            .bind(storage_path)
            .bind(project_id)
            .fetch_one(pool)
            .await
        }
        (None, None) => {
            crate::db::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM files WHERE storage_path = ?
                    UNION ALL
                    SELECT 1 FROM file_histories WHERE storage_path = ?
                )",
            )
            .bind(storage_path)
            .bind(storage_path)
            .fetch_one(pool)
            .await
        }
        (Some(_), Some(_)) => unreachable!("fileとprojectの除外条件は同時に指定しない"),
    }
}

async fn quarantine_unreferenced_storage_files(
    pool: &DbPool,
    storage_root: &Path,
    storage_paths: impl IntoIterator<Item = String>,
    excluded_file_id: Option<i64>,
    excluded_project_id: Option<i64>,
) -> Result<Vec<QuarantinedStorage>, ApiError> {
    let mut unique_paths = HashSet::new();
    let mut quarantined = Vec::new();
    for storage_path in storage_paths {
        if !unique_paths.insert(storage_path.clone()) {
            continue;
        }
        let referenced = is_referenced_outside_target(
            pool,
            &storage_path,
            excluded_file_id,
            excluded_project_id,
        )
        .await?;
        if referenced {
            continue;
        }
        let relative_path = Path::new(&storage_path);
        if relative_path.is_absolute()
            || relative_path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            tracing::error!(storage_path, "unsafe storage path during physical deletion");
            restore_quarantined_storage(&quarantined).await;
            return Err(ApiError::Storage);
        }
        let path: PathBuf = storage_root.join(relative_path);
        let metadata = match tokio::fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) => {
                tracing::error!(storage_path, %error, "storage file is unavailable before quarantine");
                restore_quarantined_storage(&quarantined).await;
                return Err(ApiError::Storage);
            }
        };
        if !metadata.is_file() {
            tracing::error!(storage_path, "storage path is not a regular file");
            restore_quarantined_storage(&quarantined).await;
            return Err(ApiError::Storage);
        }
        match crate::integrity::move_to_quarantine(storage_root, &path).await {
            Ok(quarantined_path) => quarantined.push(QuarantinedStorage {
                original: path,
                quarantined: quarantined_path,
            }),
            Err(error) => {
                tracing::error!(storage_path, %error, "failed to quarantine storage file");
                restore_quarantined_storage(&quarantined).await;
                return Err(ApiError::Storage);
            }
        }
    }
    Ok(quarantined)
}

async fn restore_quarantined_storage(files: &[QuarantinedStorage]) {
    for file in files.iter().rev() {
        if let Err(error) = tokio::fs::rename(&file.quarantined, &file.original).await {
            tracing::error!(
                original = %file.original.display(),
                quarantined = %file.quarantined.display(),
                %error,
                "failed to restore quarantined storage file"
            );
        }
    }
}

pub(crate) async fn permanently_delete_file(
    pool: &DbPool,
    storage_root: &Path,
    id: i64,
) -> Result<u64, ApiError> {
    let _storage_guard = storage_operation_guard().await;
    let storage_paths: Vec<String> = crate::db::query_scalar(
        "SELECT storage_path FROM files WHERE id = ?
         UNION SELECT storage_path FROM file_histories WHERE file_id = ?",
    )
    .bind(id)
    .bind(id)
    .fetch_all(pool)
    .await?;
    let quarantined =
        quarantine_unreferenced_storage_files(pool, storage_root, storage_paths, Some(id), None)
            .await?;
    let mut transaction = pool.begin().await?;
    let result = match crate::db::query("DELETE FROM files WHERE id = ? AND deleted_at IS NOT NULL")
        .bind(id)
        .execute(&mut *transaction)
        .await
    {
        Ok(result) => result,
        Err(error) => {
            restore_quarantined_storage(&quarantined).await;
            return Err(error.into());
        }
    };
    if result.rows_affected() == 0 {
        restore_quarantined_storage(&quarantined).await;
        return Ok(0);
    }
    if let Err(error) = transaction.commit().await {
        restore_quarantined_storage(&quarantined).await;
        return Err(error.into());
    }
    Ok(result.rows_affected())
}

pub(crate) async fn list_deleted_projects(
    pool: &DbPool,
) -> Result<Vec<ProjectTrashItem>, sqlx::Error> {
    crate::db::query_as(
        "SELECT id, project_number, name, kana, address, phone, email, dealer, assignee, (SELECT phone FROM dealer_contacts c WHERE c.dealer_name = projects.dealer AND c.name = projects.assignee AND c.deleted_at IS NULL LIMIT 1) as assignee_phone, latitude, longitude, plus_code, strftime('%Y-%m-%d %H:%M:%S', deleted_at, '+9 hours') as deleted_at
         FROM projects
         WHERE deleted_at IS NOT NULL
         ORDER BY deleted_at DESC, id DESC",
    )
    .fetch_all(pool)
    .await
}

pub(crate) async fn list_deleted_dealers(
    pool: &DbPool,
) -> Result<Vec<DealerTrashItem>, sqlx::Error> {
    crate::db::query_as(
        "SELECT id, name, kana, address, phone, fax, email, strftime('%Y-%m-%d %H:%M:%S', deleted_at, '+9 hours') as deleted_at
         FROM dealers
         WHERE deleted_at IS NOT NULL
         ORDER BY deleted_at DESC, id DESC",
    )
    .fetch_all(pool)
    .await
}
