use serde::Deserialize;
use std::env;
use std::path::PathBuf;

const DEFAULT_SETTINGS_PATH: &str = "config/settings.json";

#[derive(Debug, Default, Deserialize)]
struct SettingsFile {
    database_url: Option<String>,
    storage_dir: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    secure_cookie: Option<bool>,
    tls_cert_path: Option<String>,
    tls_key_path: Option<String>,
    webauthn_rp_id: Option<String>,
    webauthn_origin: Option<String>,
    google_client_id: Option<String>,
    google_client_secret: Option<String>,
    google_redirect_uri: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GoogleOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub storage_dir: PathBuf,
    pub host: String,
    pub port: u16,
    pub admin_password: Option<String>,
    pub secure_cookie: bool,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let settings = Self::load_settings();
        let database_url = env_or_setting(
            "DATABASE_URL",
            settings.database_url,
            "postgres://filemanager3@127.0.0.1/filemanager3",
        );
        let storage_dir = PathBuf::from(env_or_setting(
            "FILE_STORAGE_DIR",
            settings.storage_dir,
            "./data/storage",
        ));
        let host = env_or_setting("HOST", settings.host, "0.0.0.0");
        let port = env::var("PORT")
            .ok()
            .and_then(|value| value.parse().ok())
            .or(settings.port)
            .unwrap_or(3000);
        let admin_password = env::var("ADMIN_PASSWORD").ok();
        let secure_cookie = env::var("SECURE_COOKIE")
            .ok()
            .map(|value| value == "true")
            .or(settings.secure_cookie)
            .unwrap_or(true);

        Self {
            database_url,
            storage_dir,
            host,
            port,
            admin_password,
            secure_cookie,
        }
    }

    pub fn setting(&self, key: &str) -> Option<String> {
        let settings = Self::load_settings();
        let value = match key {
            "TLS_CERT_PATH" => settings.tls_cert_path,
            "TLS_KEY_PATH" => settings.tls_key_path,
            "WEBAUTHN_RP_ID" => settings.webauthn_rp_id,
            "WEBAUTHN_ORIGIN" => settings.webauthn_origin,
            _ => None,
        };
        value.filter(|value| !value.trim().is_empty())
    }

    fn load_settings() -> SettingsFile {
        let path = env::var("FILEMANAGER_SETTINGS_PATH")
            .unwrap_or_else(|_| DEFAULT_SETTINGS_PATH.to_string());
        match std::fs::read_to_string(&path) {
            Ok(contents) => match serde_json::from_str(&contents) {
                Ok(settings) => settings,
                Err(error) => {
                    tracing::warn!(path = %path, error = %error, "設定JSONを読み込めません。既定値を使用します");
                    SettingsFile::default()
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => SettingsFile::default(),
            Err(error) => {
                tracing::warn!(path = %path, error = %error, "設定JSONを読み込めません。既定値を使用します");
                SettingsFile::default()
            }
        }
    }

    pub fn validate_transport(&self) -> anyhow::Result<()> {
        if !self.secure_cookie {
            return Err(anyhow::anyhow!(
                "HTTPは禁止されています。SECURE_COOKIE=trueを設定してください"
            ));
        }
        Ok(())
    }
}

pub fn google_oauth_config() -> Option<GoogleOAuthConfig> {
    let settings = AppConfig::load_settings();
    let client_id = env_or_setting("GOOGLE_CLIENT_ID", settings.google_client_id, "");
    let client_secret = env_or_setting("GOOGLE_CLIENT_SECRET", settings.google_client_secret, "");
    let redirect_uri = env_or_setting("GOOGLE_REDIRECT_URI", settings.google_redirect_uri, "");
    (!client_id.trim().is_empty()
        && !client_secret.trim().is_empty()
        && !redirect_uri.trim().is_empty())
    .then_some(GoogleOAuthConfig {
        client_id,
        client_secret,
        redirect_uri,
    })
}

fn env_or_setting(key: &str, setting: Option<String>, default: &str) -> String {
    env::var(key)
        .ok()
        .or(setting)
        .unwrap_or_else(|| default.to_string())
}
