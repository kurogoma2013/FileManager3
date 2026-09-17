use crate::auth;
use crate::config::AppConfig;
use crate::db::DbPool;
use crate::handlers::auth_handlers::cleanup_expired_auth_state;

pub async fn prepare_database(pool: &DbPool, config: &AppConfig) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    cleanup_expired_auth_state(pool).await?;
    sqlx::query("ANALYZE").execute(pool).await?;
    seed_admin_user(pool, config).await?;
    crate::integrity::reconcile_startup(pool, &config.storage_dir).await
}

async fn seed_admin_user(pool: &DbPool, config: &AppConfig) -> anyhow::Result<()> {
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
        "INSERT INTO users (username, password_hash, role, webauthn_id, created_at, updated_at) VALUES ($1, $2, 'admin', $3, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .bind("admin")
    .bind(hash)
    .bind(webauthn_id)
    .execute(pool)
    .await?;
    Ok(())
}
