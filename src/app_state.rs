use crate::auth;
use crate::config::AppConfig;
use crate::handlers::auth_handlers::cleanup_expired_auth_state;
use sqlx::sqlite::SqlitePool;
use std::path::{Path, PathBuf};

pub async fn prepare_database(pool: &SqlitePool, config: &AppConfig) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    cleanup_expired_auth_state(pool).await?;
    sqlx::query("PRAGMA optimize").execute(pool).await?;
    seed_admin_user(pool, config).await?;
    crate::integrity::reconcile_startup(pool, &config.storage_dir).await
}

pub async fn clear_storage_for_new_database(
    database_url: &str,
    storage_dir: &Path,
) -> anyhow::Result<()> {
    let Some(database_path) = database_url
        .strip_prefix("sqlite:")
        .map(|value| value.split('?').next().unwrap_or(value))
        .filter(|value| !value.is_empty() && *value != ":memory:")
        .map(PathBuf::from)
    else {
        return Ok(());
    };
    if database_path.exists() || !storage_dir.exists() {
        return Ok(());
    }
    crate::integrity::quarantine_existing_storage(storage_dir).await
}

async fn seed_admin_user(pool: &SqlitePool, config: &AppConfig) -> anyhow::Result<()> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    if count != 0 {
        return Ok(());
    }

    let raw_password = match config.admin_password.as_deref() {
        Some(password) if !password.is_empty() => password,
        #[cfg(test)]
        None => "admin1234",
        #[cfg(not(test))]
        None => {
            return Err(anyhow::anyhow!(
                "初回起動にはADMIN_PASSWORDの設定が必要です"
            ))
        }
        Some(_) => return Err(anyhow::anyhow!("ADMIN_PASSWORDは空にできません")),
    };
    let hash = auth::hash_password(raw_password)?;
    let webauthn_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO users (username, password_hash, role, webauthn_id, created_at, updated_at) VALUES (?, ?, 'admin', ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .bind("admin")
    .bind(hash)
    .bind(webauthn_id)
    .execute(pool)
    .await?;
    Ok(())
}
