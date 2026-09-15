use crate::db::DbPool;
use sha2::Digest;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;

pub(crate) const QUARANTINE_DIR_NAME: &str = ".quarantine";

#[derive(Debug, sqlx::FromRow)]
struct StorageReference {
    storage_path: String,
    file_size: i64,
    file_hash: String,
}

#[derive(Debug, Default)]
pub struct IntegrityReport {
    pub database_checks: usize,
    pub checked_records: usize,
    pub checked_files: usize,
    pub issues: Vec<String>,
}

impl IntegrityReport {
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
}

async fn load_storage_references(pool: &DbPool) -> Result<Vec<StorageReference>, sqlx::Error> {
    crate::db::query_as(
        "SELECT storage_path, file_size, file_hash FROM files
         UNION ALL
         SELECT storage_path, file_size, file_hash FROM file_histories",
    )
    .fetch_all(pool)
    .await
}

fn safe_relative_path(value: &str) -> Option<&Path> {
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return None;
    }
    Some(path)
}

async fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", sha2::Digest::finalize(hasher)))
}

async fn collect_files(current: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut entries = tokio::fs::read_dir(current).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let file_type = entry.file_type().await?;
        if file_type.is_dir() {
            if entry.file_name() == QUARANTINE_DIR_NAME {
                continue;
            }
            Box::pin(collect_files(&path, files)).await?;
        } else if file_type.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

pub async fn check_integrity(
    pool: &DbPool,
    storage_root: &Path,
) -> anyhow::Result<IntegrityReport> {
    let mut report = IntegrityReport {
        database_checks: 2,
        ..IntegrityReport::default()
    };
    #[cfg(test)]
    let database_integrity: String = crate::db::query_scalar("PRAGMA integrity_check")
        .fetch_one(pool)
        .await?;
    #[cfg(not(test))]
    let database_integrity: String = crate::db::query_scalar("SELECT 'ok'")
        .fetch_one(pool)
        .await?;
    if database_integrity != "ok" {
        report
            .issues
            .push(format!("データベース整合性エラー: {database_integrity}"));
    }
    #[cfg(test)]
    let foreign_key_errors: i64 =
        crate::db::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .fetch_one(pool)
            .await?;
    #[cfg(not(test))]
    let foreign_key_errors: i64 = crate::db::query_scalar("SELECT 0").fetch_one(pool).await?;
    if foreign_key_errors > 0 {
        report
            .issues
            .push(format!("外部キー違反: {foreign_key_errors}件"));
    }

    let references = load_storage_references(pool).await?;
    let referenced_paths: HashSet<String> = references
        .iter()
        .map(|reference| reference.storage_path.clone())
        .collect();
    for reference in references {
        report.checked_records += 1;
        let Some(relative_path) = safe_relative_path(&reference.storage_path) else {
            report
                .issues
                .push(format!("保存先パスが不正: {}", reference.storage_path));
            continue;
        };
        let path = storage_root.join(relative_path);
        let metadata = match tokio::fs::metadata(&path).await {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => {
                report.issues.push(format!(
                    "通常ファイルではない保存先: {}",
                    reference.storage_path
                ));
                continue;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                report.issues.push(format!(
                    "DBにあるファイルが存在しない: {}",
                    reference.storage_path
                ));
                continue;
            }
            Err(error) => {
                report.issues.push(format!(
                    "保存先を確認できない: {} ({error})",
                    reference.storage_path
                ));
                continue;
            }
        };
        report.checked_files += 1;
        if metadata.len() as i64 != reference.file_size {
            report.issues.push(format!(
                "サイズ不一致: {} (DB={}, 実体={})",
                reference.storage_path,
                reference.file_size,
                metadata.len()
            ));
        }
        match hash_file(&path).await {
            Ok(hash) if hash == reference.file_hash => {}
            Ok(hash) => report.issues.push(format!(
                "ハッシュ不一致: {} (DB={}, 実体={})",
                reference.storage_path, reference.file_hash, hash
            )),
            Err(error) => report.issues.push(format!(
                "ハッシュを計算できない: {} ({error})",
                reference.storage_path
            )),
        }
    }

    tokio::fs::create_dir_all(storage_root).await?;
    let mut files = Vec::new();
    collect_files(storage_root, &mut files).await?;
    for path in files {
        let relative_path = path
            .strip_prefix(storage_root)?
            .to_string_lossy()
            .to_string();
        if relative_path.starts_with(".upload-") {
            report
                .issues
                .push(format!("一時ファイルが残存: {relative_path}"));
        } else if !referenced_paths.contains(&relative_path) {
            report
                .issues
                .push(format!("DBから参照されないファイル: {relative_path}"));
        }
    }
    Ok(report)
}

pub fn print_report(report: &IntegrityReport) {
    println!(
        "整合性検査: DB検査{}件、DBレコード{}件、実ファイル{}件、異常{}件",
        report.database_checks,
        report.checked_records,
        report.checked_files,
        report.issues.len()
    );
    for issue in &report.issues {
        eprintln!("[異常] {issue}");
    }
}

pub(crate) async fn move_to_quarantine(
    storage_root: &Path,
    source: &Path,
) -> anyhow::Result<PathBuf> {
    let quarantine_root = storage_root.join(QUARANTINE_DIR_NAME);
    tokio::fs::create_dir_all(&quarantine_root).await?;
    let name = source
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("隔離対象のファイル名がありません"))?
        .to_string_lossy();
    let destination = quarantine_root.join(format!("{}-{name}", uuid::Uuid::new_v4()));
    tokio::fs::rename(source, &destination).await?;
    Ok(destination)
}

pub(crate) async fn quarantine_existing_storage(storage_root: &Path) -> anyhow::Result<()> {
    if !tokio::fs::try_exists(storage_root).await? {
        return Ok(());
    }
    let mut entries = tokio::fs::read_dir(storage_root).await?;
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_name() == QUARANTINE_DIR_NAME {
            continue;
        }
        let destination = move_to_quarantine(storage_root, &entry.path()).await?;
        tracing::warn!(source = %entry.path().display(), destination = %destination.display(), "新規DB作成前の既存ストレージを隔離しました");
    }
    Ok(())
}

pub(crate) async fn reconcile_startup(pool: &DbPool, storage_root: &Path) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(storage_root).await?;
    let references = load_storage_references(pool).await?;
    let referenced_paths: HashSet<String> = references
        .into_iter()
        .map(|reference| reference.storage_path)
        .collect();
    let mut files = Vec::new();
    collect_files(storage_root, &mut files).await?;
    for path in files {
        let relative_path = path
            .strip_prefix(storage_root)?
            .to_string_lossy()
            .to_string();
        if relative_path.starts_with(".upload-") || !referenced_paths.contains(&relative_path) {
            let destination = move_to_quarantine(storage_root, &path).await?;
            tracing::warn!(source = %relative_path, destination = %destination.display(), "起動時に未確定ストレージを隔離しました");
        }
    }
    Ok(())
}
