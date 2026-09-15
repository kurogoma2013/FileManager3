use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand_core::OsRng;
use sqlx::SqlitePool;

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?
        .to_string())
}

pub async fn authenticate_credentials(
    pool: &SqlitePool,
    username: &str,
    password: &str,
) -> anyhow::Result<Option<(i64, String)>> {
    let row: Option<(i64, String, String)> = sqlx::query_as(
        "SELECT id, password_hash, role FROM users WHERE username = ? AND active = 1",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?;
    let Some((id, password_hash, role)) = row else {
        tracing::warn!(username = %username, "user not found or inactive");
        let dummy_hash = PasswordHash::new("$argon2id$v=19$m=19456,t=2,p=1$w2N4VpQeN5C6Z3X8Y2qPZw$V9nO/9kQZ1aN5cM9T4jV7P1H9dG3zB8mX1yL3R4vE9c").unwrap();
        let _ = Argon2::default().verify_password(password.as_bytes(), &dummy_hash);
        return Ok(None);
    };
    let parsed = match PasswordHash::new(&password_hash) {
        Ok(parsed) => parsed,
        Err(err) => {
            tracing::error!(username = %username, error = %err, "failed to parse password hash");
            return Ok(None);
        }
    };
    let is_valid = Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok();
    if !is_valid {
        tracing::warn!(username = %username, "password verification failed");
    }
    Ok(is_valid.then_some((id, role)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn パスワードハッシュを生成できる() {
        let hash = hash_password("secret").unwrap();
        assert!(hash.starts_with("$argon2"));
    }
}
