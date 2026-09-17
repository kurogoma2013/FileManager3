use super::*;
use crate::core::{
    can_delete_user_target, can_edit_user, can_manage_projects, can_move_file, can_upload_files,
};
use crate::db::DbPool;
use crate::models::ApiError;
use axum::body::to_bytes;
use axum::http::{header, HeaderMap, HeaderValue, Request};
use axum::response::IntoResponse;
use axum::Router;
use handlers::files::{encode_picture_to_webp, file_content_type, is_picture, is_video};
use handlers::projects::{
    normalize_phone, normalize_project_number, project_order_by, validate_project_location,
};
use handlers::users::valid_user_role;
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use std::path::PathBuf;
use std::sync::Once;
use templates::{
    detail_page, render_page, ADMIN_HTML, ADMIN_PROJECTS_HTML, DEALER_REGISTRATION_HTML,
    DETAIL_HTML, HELP_HTML, INDEX_HTML, LOGIN_HTML, PROJECT_RESULTS_HTML, USER_REGISTRATION_HTML,
};
use tower::ServiceExt;

const TEST_DATABASE_PREFIX: &str = "filemanager3_test_";
static STALE_TEST_DATABASES: Once = Once::new();

/// テスト用PostgreSQLの管理DB接続URL。
/// `TEST_DATABASE_URL`、`POSTGRES_PASSWORD`、`.env` の順に参照し、
/// 未設定なら `docker compose` の既定値へ接続する。
fn admin_database_url() -> String {
    if let Ok(url) = std::env::var("TEST_DATABASE_URL") {
        return url;
    }
    let password = std::env::var("POSTGRES_PASSWORD")
        .ok()
        .or_else(|| {
            std::fs::read_to_string(".env").ok().and_then(|content| {
                content.lines().find_map(|line| {
                    line.trim()
                        .strip_prefix("POSTGRES_PASSWORD=")
                        .map(|value| value.trim().to_string())
                })
            })
        })
        .unwrap_or_default();
    let host = std::env::var("TEST_DATABASE_HOST").unwrap_or_else(|_| "127.0.0.1:5432".into());
    format!(
        "postgres://filemanager3:{}@{host}/filemanager3",
        percent_encode(&password)
    )
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn test_database_url(name: &str) -> String {
    let admin_url = admin_database_url();
    let base = admin_url.rsplit_once('/').map(|(base, _)| base).unwrap();
    format!("{base}/{name}")
}

/// テストごとに専用データベースを作成して接続する。
/// 1時間以上前に作成されたテストDBは最初の呼び出しでまとめて削除する。
async fn test_pool() -> DbPool {
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&admin_database_url())
        .await
        .expect("テスト用PostgreSQLに接続できません（docker compose up -d を実行してください）");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut cleanup = false;
    STALE_TEST_DATABASES.call_once(|| cleanup = true);
    if cleanup {
        let names: Vec<String> = sqlx::query_scalar(
            "SELECT datname FROM pg_database WHERE datname LIKE 'filemanager3_test_%'",
        )
        .fetch_all(&admin)
        .await
        .unwrap();
        for name in names {
            let created = name
                .strip_prefix(TEST_DATABASE_PREFIX)
                .and_then(|rest| rest.split('_').next())
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            if created + 3600 < now {
                let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS \"{name}\""))
                    .execute(&admin)
                    .await;
            }
        }
    }
    let name = format!(
        "{TEST_DATABASE_PREFIX}{now}_{}",
        uuid::Uuid::new_v4().simple()
    );
    sqlx::query(&format!("CREATE DATABASE \"{name}\""))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&test_database_url(&name))
        .await
        .unwrap()
}

fn multipart_upload_body(
    boundary: &str,
    filename: &str,
    bytes: &[u8],
    tag: Option<&str>,
) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n");
    if let Some(tag) = tag {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"tag\"\r\n\r\n{tag}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

async fn upload_test_app(storage_prefix: &str) -> (Router, DbPool, PathBuf, String) {
    let pool = test_pool().await;
    let storage_dir = PathBuf::from(format!(
        "./target/{storage_prefix}-{}",
        uuid::Uuid::new_v4()
    ));
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: storage_dir.clone(),
        host: "127.0.0.1".to_string(),
        port: 3000,
        admin_password: None,
        secure_cookie: false,
    };
    let app = create_app(pool.clone(), &config).await.unwrap();
    sqlx::query(
        "INSERT INTO projects (project_number, name, kana) VALUES ('UP001', 'アップロード案件', 'アップロードアンケン')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let token = format!("upload-session-{}", uuid::Uuid::new_v4());
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES ($1, 1, CURRENT_TIMESTAMP + INTERVAL '1 hours')",
    )
    .bind(handlers::auth_handlers::hash_session_token(&token))
    .execute(&pool)
    .await
    .unwrap();
    (app, pool, storage_dir, token)
}

async fn post_upload(
    app: Router,
    token: &str,
    project_id: i64,
    filename: &str,
    bytes: &[u8],
    tag: Option<&str>,
) -> axum::response::Response {
    let boundary = format!("fm3-test-{}", uuid::Uuid::new_v4());
    let body = multipart_upload_body(&boundary, filename, bytes, tag);
    app.oneshot(
        Request::builder()
            .method("POST")
            .uri(format!("/api/projects/{project_id}/files"))
            .header(header::COOKIE, format!("fm3_session={token}"))
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(axum::body::Body::from(body))
            .unwrap(),
    )
    .await
    .unwrap()
}

async fn post_upload_with_category(
    app: Router,
    token: &str,
    project_id: i64,
    filename: &str,
    bytes: &[u8],
    tag: Option<&str>,
    category: &str,
) -> axum::response::Response {
    let boundary = format!("fm3-test-{}", uuid::Uuid::new_v4());
    let body = multipart_upload_body(&boundary, filename, bytes, tag);
    app.oneshot(
        Request::builder()
            .method("POST")
            .uri(format!(
                "/api/projects/{project_id}/files?category={category}"
            ))
            .header(header::COOKIE, format!("fm3_session={token}"))
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(axum::body::Body::from(body))
            .unwrap(),
    )
    .await
    .unwrap()
}

async fn post_upload_item(
    app: Router,
    token: &str,
    project_id: i64,
    filename: &str,
    bytes: &[u8],
    tag: Option<&str>,
) -> (axum::http::StatusCode, models::FileItem) {
    let response = post_upload(app, token, project_id, filename, bytes, tag).await;
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let item = serde_json::from_slice::<models::FileItem>(&body).unwrap();
    (status, item)
}

#[tokio::test]
async fn トップページ用ログイン画面を表示できる() {
    let pool = test_pool().await;
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: PathBuf::from("./target/test_storage_index"),
        host: "127.0.0.1".to_string(),
        port: 3000,
        admin_password: None,
        secure_cookie: false,
    };
    let app = create_app(pool, &config).await.unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("ログイン"));
}

#[test]
fn ログイン画面でパスワードの表示を切り替えられる() {
    let html = templates::render_page(LOGIN_HTML);
    assert!(html.contains("id=\"password-toggle\""));
    assert!(html.contains("パスワードを非表示"));
    assert!(html.contains("passwordInput.type==='text'"));
    assert!(html.contains("catch(_){} }form.username.addEventListener(\"input\""));
    assert!(html.contains(".password-field{position:relative;display:block"));
    assert!(html.contains(".password-toggle{position:absolute;right:0;top:0"));
    assert!(html.contains("class=\"password-field\" style=\"position:relative;display:block\""));
}

#[test]
fn メールアドレスはメーラー起動リンクとして表示される() {
    let detail = templates::render_page(DETAIL_HTML);
    let dealers = templates::render_page(DEALER_REGISTRATION_HTML);
    assert!(detail.contains("function emailAnchor(value)"));
    assert!(detail.contains("mailto:'+encodeURIComponent(email)"));
    assert!(detail.contains("${emailAnchor(detail.project.email)}"));
    assert!(dealers.contains("mailto:${encodeURIComponent(d.email)}"));
    assert!(dealers.contains("mailto:${encodeURIComponent(c.email)}"));
}

#[test]
fn 無操作1時間で自動ログアウトし操作中はセッションを延長する() {
    assert_eq!(models::SESSION_MAX_AGE_SECONDS, 60 * 60);
    let auth = include_str!("handlers/auth_handlers.rs");
    let routes = include_str!("routes.rs");
    let rendered = templates::render_page(INDEX_HTML);
    assert!(auth.contains("CURRENT_TIMESTAMP + INTERVAL '1 hour'"));
    assert!(auth.contains("pub async fn keepalive"));
    assert!(routes.contains("/api/session/keepalive"));
    assert!(auth.contains("session_expires_at"));
    assert!(rendered.contains("function activity()"));
    assert!(rendered.contains("Date.now()-lastActivity>=timeout"));
    assert!(rendered.contains("fetch('/api/session/keepalive'"));
    assert!(rendered.contains("location.href='/logout'"));
}

#[test]
fn webオフライン保存機能を提供しない() {
    let detail = templates::render_page(DETAIL_HTML);
    let routes = include_str!("routes.rs");
    assert!(!detail.contains("data-selected-action=\"offline-save\""));
    assert!(!detail.contains("window.loadOfflineProject"));
    assert!(!detail.contains("window.clearOfflineCache"));
    assert!(!detail.contains("navigator.serviceWorker.register"));
    assert!(!routes.contains("/offline-sw.js"));
}

#[test]
fn 物理削除は管理者向けの削除済みデータだけを対象にする() {
    let routes = include_str!("routes.rs");
    let core = include_str!("core.rs");
    let rendered_projects = templates::render_page(ADMIN_PROJECTS_HTML);
    let rendered_dealers = templates::render_page(DEALER_REGISTRATION_HTML);
    let rendered_users = templates::render_page(USER_REGISTRATION_HTML);

    assert!(core.contains("role == \"admin\""));
    assert!(routes.contains("/api/deleted/projects/:id"));
    assert!(routes.contains("/api/deleted/dealers/:id"));
    assert!(routes.contains("/api/deleted/files/:id"));
    assert!(routes.contains("/api/deleted/users/:id"));
    assert!(routes.contains("/permanent"));
    assert!(rendered_projects.contains("完全削除すると復元できません"));
    assert!(rendered_dealers.contains("完全削除すると復元できません"));
    assert!(rendered_users.contains("削除済みユーザー"));
}

#[test]
fn 日時表示は日本時間への変換を使用する() {
    let projects = include_str!("handlers/projects.rs");
    let notes = include_str!("handlers/notes.rs");
    let users = include_str!("handlers/users.rs");
    let maintenance = include_str!("maintenance.rs");
    let auth = include_str!("handlers/auth_handlers.rs");

    assert!(
        projects.contains("to_char(updated_at AT TIME ZONE 'Asia/Tokyo', 'YYYY-MM-DD HH24:MI:SS')")
    );
    assert!(
        notes.contains("to_char(pn.created_at AT TIME ZONE 'Asia/Tokyo', 'YYYY-MM-DD HH24:MI:SS')")
    );
    assert!(
        notes.contains("to_char(dn.created_at AT TIME ZONE 'Asia/Tokyo', 'YYYY-MM-DD HH24:MI:SS')")
    );
    assert!(users
        .contains("to_char(users.updated_at AT TIME ZONE 'Asia/Tokyo', 'YYYY-MM-DD HH24:MI:SS')"));
    assert!(maintenance
        .contains("to_char(deleted_at AT TIME ZONE 'Asia/Tokyo', 'YYYY-MM-DD HH24:MI:SS')"));
    assert!(auth.contains(
        "to_char(expires_at AT TIME ZONE 'Asia/Tokyo', 'YYYY-MM-DD\\\"T\\\"HH24:MI:SS') || '+09:00'"
    ));
}

#[test]
fn 仕様書リンクを全画面の管理サイドメニューへ補完する() {
    let html = templates::render_page(HELP_HTML);
    assert!(html.contains("ensureSpecificationsLink"));
    assert!(html.contains("ensureSpecificationsLink();setupMenuToggles();"));
    assert!(html.contains("href=\"/admin/specifications\""));
    assert!(html.contains("['01_overview','システム概要']"));
    assert!(html.contains("['06_configuration','設定・起動']"));
    assert!(html.contains("['07_android','Android版']"));
    assert!(html.contains("['10_docker_postgresql','Docker PostgreSQL']"));
    assert!(html.contains("['11_ubuntu_apache','Ubuntu・Apache 設定']"));
}

#[test]
fn 管理メニューと仕様書はクリックで開閉でき状態を維持する() {
    let html = templates::render_page(INDEX_HTML);
    assert!(html.contains("setupMenuToggles"));
    assert!(html.contains("admin-toggle-icon"));
    assert!(html.contains("adminMenuLink.classList.add('active')"));
    assert!(html.contains("fm3_admin_menu_open"));
    assert!(html.contains("fm3_spec_menu_open"));
    assert!(html.contains("updateAdminState"));
    assert!(html.contains("updateSpecState"));
    assert!(html.contains("spec-toggle-icon"));
    assert!(html.contains(".admin-toggle-icon{margin-left:auto"));
}

#[test]
fn ユーザー登録ダイアログでパスワード表示を切り替えられる() {
    let html = templates::render_page(USER_REGISTRATION_HTML);
    assert!(html.contains("id=\"user-password-toggle\""));
    assert!(html.contains("id=\"user-password\""));
    assert!(html.contains("パスワードを非表示"));
}

#[test]
fn ユーザーロールはownerを使わず操作権限を判定する() {
    assert!(valid_user_role("admin"));
    assert!(valid_user_role("member"));
    assert!(valid_user_role("viewer"));
    let permissions_spec = include_str!("../docs/02_permissions.md");
    assert!(!permissions_spec.contains("owner"));
    assert!(permissions_spec.contains("## 案件単位の権限"));
    assert!(permissions_spec.contains("## 管理画面"));
    assert!(permissions_spec.contains("## viewerの利用範囲"));
    assert!(!valid_user_role("owner"));
    assert!(!valid_user_role("manager"));
    assert!(can_upload_files("viewer"));
    assert!(can_move_file("viewer", "Picture"));
    assert!(!can_move_file("viewer", "Document"));
    assert!(can_upload_files("member"));
    assert!(can_manage_projects("member"));
    assert!(!can_manage_projects("viewer"));
}

#[test]
fn ユーザー管理画面にownerロールを表示しない() {
    assert!(!USER_REGISTRATION_HTML.contains("owner"));
    assert!(USER_REGISTRATION_HTML.contains("member（一般ユーザー）"));
    assert!(USER_REGISTRATION_HTML.contains("admin（管理ユーザー）"));
    assert!(USER_REGISTRATION_HTML.contains("viewer（閲覧ユーザー）"));
    assert!(USER_REGISTRATION_HTML.contains("deleteUser"));
    assert!(include_str!("handlers/users.rs").contains("UPDATE users SET active = FALSE"));
    assert!(USER_REGISTRATION_HTML.contains("u.passkey_count"));
    assert!(USER_REGISTRATION_HTML.contains("u.passkey_count>0"));
    let rendered = templates::render_page(USER_REGISTRATION_HTML);
    assert!(rendered.contains("openUserEdit(${u.id},'${jsEsc(u.username)}','${u.role}')"));
    assert!(!rendered.contains("openUserEdit(${u.id},'${esc(u.username)}','${roleLabel(u.role)}')"));
    assert!(!rendered.contains("管理者は削除不可"));
    assert!(rendered.contains("user-self-nav"));
    assert!(rendered.contains("loadCurrentUser"));
}

#[tokio::test]
async fn 管理ユーザーは管理者以外から削除できない() {
    let pool = test_pool().await;
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: PathBuf::from("./target/test_storage_admin_delete_guard"),
        host: "127.0.0.1".to_string(),
        port: 0,
        admin_password: None,
        secure_cookie: false,
    };
    let app = create_app(pool.clone(), &config).await.unwrap();
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES ('member-delete-test', 'test', 'member')")
        .execute(&pool)
        .await
        .unwrap();
    let member_token = "member-delete-test-token";
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES ($1, 2, CURRENT_TIMESTAMP + INTERVAL '1 hours')",
    )
    .bind(handlers::auth_handlers::hash_session_token(member_token))
    .execute(&pool)
    .await
    .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/users/1")
                .header(header::COOKIE, format!("fm3_session={member_token}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT active FROM users WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap()
    );
    assert!(!can_delete_user_target("member", "admin"));
    assert!(can_delete_user_target("admin", "admin"));
}

#[tokio::test]
async fn viewerは自分のユーザー情報だけ編集できる() {
    let pool = test_pool().await;
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: PathBuf::from("./target/test_storage_viewer_user_edit"),
        host: "127.0.0.1".to_string(),
        port: 0,
        admin_password: None,
        secure_cookie: false,
    };
    let app = create_app(pool.clone(), &config).await.unwrap();
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES ('viewer-edit-test', 'test', 'viewer'), ('member-edit-test', 'test', 'member')")
        .execute(&pool)
        .await
        .unwrap();
    let viewer_token = "viewer-edit-test-token";
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES ($1, 2, CURRENT_TIMESTAMP + INTERVAL '1 hours')",
    )
    .bind(handlers::auth_handlers::hash_session_token(viewer_token))
    .execute(&pool)
    .await
    .unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/users")
                .header(header::COOKIE, format!("fm3_session={viewer_token}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let users: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0]["id"], 2);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/users/2")
                .header(header::COOKIE, format!("fm3_session={viewer_token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(
                    r#"{"password":"viewer-new-password"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/users/1")
                .header(header::COOKIE, format!("fm3_session={viewer_token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(
                    r#"{"password":"other-new-password"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/users/2")
                .header(header::COOKIE, format!("fm3_session={viewer_token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from(r#"{"role":"admin"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
    assert!(can_edit_user(2, "viewer", 2));
    assert!(!can_edit_user(2, "viewer", 1));
}

#[test]
fn データベース整合性対策を適用する() {
    let migration = include_str!("../migrations/20260917000000_init.sql");
    let update_script = include_str!("../scripts/update.sh");
    let startup = include_str!("main.rs");
    assert!(migration.contains("REFERENCES dealers(name) ON UPDATE CASCADE ON DELETE CASCADE"));
    assert!(migration.contains("CREATE INDEX idx_files_hash_active"));
    assert!(include_str!("maintenance.rs").contains("STORAGE_OPERATION_LOCK"));
    assert!(include_str!("handlers/files.rs").contains("created_storage_path"));
    assert!(include_str!("handlers/auth_handlers.rs").contains("cleanup_expired_auth_state"));
    assert!(include_str!("integrity.rs").contains("QUARANTINE_DIR_NAME"));
    assert!(update_script.contains("storage.tar.gz"));
    assert!(update_script.contains("manifest.sha256"));
    assert!(update_script.contains("FILEMANAGER_STORAGE_DIR"));
    assert!(startup.contains("check-integrity"));
}

#[test]
fn ログイン画面はユーザー名入力後にパスキー認証を開始する() {
    assert!(render_page(LOGIN_HTML).contains("v20260915.01"));
    assert!(LOGIN_HTML.contains("startPasskeyLogin(true)"));
    assert!(LOGIN_HTML.contains("autoPasskeyLogin"));
    assert!(!LOGIN_HTML.contains("has_passkey"));
    assert!(LOGIN_HTML.contains("fm3_last_username"));
    assert!(LOGIN_HTML.contains("saveLastUsername"));
    assert!(LOGIN_HTML.contains("restoreLastUsername"));
    assert!(LOGIN_HTML.contains("id=\"passkey-hint\""));
    assert!(LOGIN_HTML.contains("パスキーで続行"));
    assert!(LOGIN_HTML.contains("autocomplete=\"username webauthn\""));
    assert!(LOGIN_HTML.contains("isConditionalMediationAvailable"));
    assert!(LOGIN_HTML.contains("options.mediation=\"conditional\""));
    assert!(LOGIN_HTML.contains("passkeyDomainAvailable"));
    assert!(LOGIN_HTML.contains("パスキーはIPアドレスでは使用できません"));
}

#[test]
fn googleログインは設定時だけログイン画面に表示する() {
    let enabled = templates::login_page_html_with_google(true);
    let disabled = templates::login_page_html_with_google(false);
    assert!(enabled.contains("id=\"google-login\""));
    assert!(enabled.contains("href=\"/auth/google\""));
    assert!(enabled.contains("Googleアカウントでログイン"));
    assert!(!disabled.contains("id=\"google-login\""));
}

#[test]
fn google_oauthの認証ルートと安全策を登録する() {
    let routes = include_str!("routes.rs");
    let auth = include_str!("handlers/auth_handlers.rs");
    let migration = include_str!("../migrations/20260917000000_init.sql");
    assert!(routes.contains("/auth/google"));
    assert!(routes.contains("/auth/google/callback"));
    assert!(auth.contains("oauth_states"));
    assert!(auth.contains("CURRENT_TIMESTAMP - INTERVAL '10 minutes'"));
    assert!(auth.contains("email_verified == Some(true)"));
    assert!(auth.contains("openidconnect.googleapis.com/v1/userinfo"));
    assert!(migration.contains("UNIQUE(provider, subject)"));
}

#[test]
fn スマホ向けに全インターフェース待ち受け時はlan用urlを表示できる() {
    let urls = webui_urls("0.0.0.0", 3000, false);
    assert!(urls.contains(&"https://127.0.0.1:3000".to_string()));
    assert!(urls.iter().all(|url| !url.contains("0.0.0.0")));
}

#[test]
fn 画面にlanとホスト名の接続先を表示しない() {
    let html = templates::render_page(LOGIN_HTML);
    assert!(html.contains(".top-access-url,.access-info{display:none!important}"));
    assert!(!LOGIN_HTML.contains("autofocus><div id=\"access-info\""));
    assert!(INDEX_HTML.contains("<span id=\"access-banner-text\" class=\"top-access-url\"></span><span class=\"top-user-name\" aria-label=\"ログインユーザー\"></span>"));
    assert!(PROJECT_RESULTS_HTML.contains("class=\"address-text\""));
    assert!(PROJECT_RESULTS_HTML.contains("class=\"position-text\""));
    assert!(!PROJECT_RESULTS_HTML.contains("<th>担当者</th>"));
    assert!(ADMIN_PROJECTS_HTML.contains("class=\\\"address-text\\\""));
    assert!(ADMIN_PROJECTS_HTML.contains("class=\\\"position-text\\\""));
    assert!(!ADMIN_PROJECTS_HTML.contains("<th>担当者</th>"));
    assert!(ADMIN_PROJECTS_HTML
        .contains("class=\"btn secondary deleted-toggle\" data-admin-only-control"));
    assert!(DEALER_REGISTRATION_HTML
        .contains("class=\"btn secondary deleted-toggle\" data-admin-only-control"));
    assert!(DETAIL_HTML
        .contains("class=\"btn secondary deleted-toggle\" type=\"button\" data-note-deleted"));
    assert!(ADMIN_PROJECTS_HTML.contains("class=\"head-actions\""));
    assert!(DEALER_REGISTRATION_HTML.contains("class=\"head-actions\""));
    assert!(DETAIL_HTML
        .contains("class=\"panel-actions\"><button class=\"btn secondary deleted-toggle\""));
    assert!(
        ADMIN_PROJECTS_HTML
            .find("data-admin-only-control onclick=\"loadDeletedProjects()\"")
            .unwrap()
            < ADMIN_PROJECTS_HTML.find("新規案件登録").unwrap()
    );
    assert!(
        DEALER_REGISTRATION_HTML
            .find("data-admin-only-control onclick=\"loadDeletedDealers()\"")
            .unwrap()
            < DEALER_REGISTRATION_HTML.find("新規販売店登録").unwrap()
    );
    assert!(ADMIN_PROJECTS_HTML.contains(".address-text{display:block"));
    assert!(ADMIN_PROJECTS_HTML.contains(".position-text{display:block"));
    for html in [
        INDEX_HTML,
        PROJECT_RESULTS_HTML,
        ADMIN_HTML,
        ADMIN_PROJECTS_HTML,
        DEALER_REGISTRATION_HTML,
        USER_REGISTRATION_HTML,
        DETAIL_HTML,
    ] {
        assert!(render_page(html).contains("v20260915.01"));
        assert!(html.contains("/* ui-consistency */"));
        assert!(html.contains("--ui-radius:8px"));
        assert!(html.contains(".table th,.table td{padding:12px 14px}"));
        assert!(html.contains("font-size:17px;color:#334155;font-weight:700"));
        assert!(html.contains("font-size:12px;padding:4px 9px"));
        assert!(html.contains("id=\"access-banner-text\""));
        assert!(html.contains("class=\"top-access-url\""));
        assert!(html.contains("class=\"top-user-name\""));
        assert!(html.contains(".topbar{position:sticky;top:0;z-index:12;"));
        assert!(html.contains(".topbar .user .top-user-name"));
        assert!(!html.contains("class=\"avatar\""));
        assert!(!html.contains(".avatar{"));
        assert!(
            !html.contains(".top-user-name{display:inline-flex;align-items:center;min-height:30px")
        );
        assert!(html.contains("fm3_top_username"));
        assert!(html.contains("fm3_top_access_url"));
        assert!(html.contains("setTopUserName"));
        assert!(html.contains("if(box.textContent!==value)box.textContent=value"));
        assert!(html.contains("loadTopAccessAddress"));
        assert!(html.contains("selectSideNavItem"));
        assert!(html.contains(".sidebar a[href=\"'+path+'\"]"));
        assert!(html.contains("item.classList.remove('active')"));
        assert!(html.contains("current.classList.add('active')"));
        assert!(!html.contains("fm3_focus_side_nav"));
        assert!(!html.contains("link.focus({preventScroll:true})"));
        assert!(!html.contains("<span>管理者</span>"));
        assert!(!html.contains("class=\"top-access-url\">確認中…</span>"));
        assert!(html.contains("white-space:pre-line"));
        assert!(html.contains(r#"📱 LAN・モバイル用アドレス\n"#));
    }
    assert!(!INDEX_HTML.contains("<a id=\"access-banner-text\""));
    assert!(!INDEX_HTML.contains("id=\"index-access-info\""));
    assert!(INDEX_HTML.contains("/api/access-urls"));
    assert!(!ADMIN_PROJECTS_HTML.contains("管理メニューに戻る"));
    assert!(!DEALER_REGISTRATION_HTML.contains("管理メニューに戻る"));
    assert!(!USER_REGISTRATION_HTML.contains("管理メニューに戻る"));
}

#[tokio::test]
async fn lanとモバイル用アドレスapiを取得できる() {
    let pool = test_pool().await;
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: PathBuf::from("./target/test_storage_access_urls"),
        host: "0.0.0.0".to_string(),
        port: 3000,
        admin_password: None,
        secure_cookie: false,
    };
    let state = Arc::new(AppState {
        pool,
        storage_root: config.storage_dir.clone(),
        access_urls: vec![
            "https://127.0.0.1:3000".to_string(),
            "https://192.168.11.21:3000".to_string(),
        ],
        webauthn: Arc::new(
            WebauthnBuilder::new("localhost", &Url::parse("http://localhost:3000").unwrap())
                .unwrap()
                .build()
                .unwrap(),
        ),
        secure_cookie: false,
    });
    let response = handlers::auth_handlers::access_urls(axum::extract::State(state))
        .await
        .0;
    assert_eq!(
        response.local_url.as_deref(),
        Some("https://127.0.0.1:3000")
    );
    assert_eq!(
        response.mobile_url.as_deref(),
        Some("https://192.168.11.21:3000")
    );
}

#[test]
fn 一括ファイル操作のapiルートが登録されている() {
    let source = include_str!("routes.rs");
    let files_source = include_str!("handlers/files.rs");
    assert!(source.contains("/api/files/batch-delete"));
    assert!(source.contains("batch_delete_files"));
    assert!(source.contains("/api/files/batch-move"));
    assert!(source.contains("batch_move_files"));
    assert!(source.contains("/api/files/batch-download"));
    assert!(source.contains("batch_download_files"));
    assert!(files_source.contains("build_zip_archive"));
    assert!(files_source.contains("application/zip"));
    assert!(files_source.contains("selected-files.zip"));
    assert!(!files_source.contains("document.querySelectorAll('a[download]').forEach"));
    assert!(source.contains("/api/files/batch-print"));
    assert!(source.contains("batch_print_files"));
}

#[test]
fn 同じハッシュの再アップロードは既存ファイルを返す() {
    let source = include_str!("handlers/files.rs");
    assert!(source.contains("async fn active_file_by_name("));
    assert!(source.contains("deleted_at IS NULL"));
    assert!(source.contains("return Ok((StatusCode::OK, Json(item)))"));
    assert!(source.contains("image::load_from_memory(bytes)"));
    assert!(source.contains("webp::Encoder::from_rgba"));
    assert!(!source.contains("preserve_jpeg"));
}

#[test]
fn 同名で内容が異なるファイルは履歴化して新しい版を登録する() {
    let source = include_str!("handlers/files.rs");
    assert!(source.contains("Sha256::digest(&final_bytes)"));
    assert!(source.contains("file_histories"));
    assert!(source.contains("old_version + 1"));
    assert!(source.contains("DELETE FROM file_search WHERE file_id = $1"));
}

#[test]
fn ファイルはハッシュ共通のストレージを参照する() {
    let source = include_str!("handlers/files.rs");
    assert!(source.contains("fn storage_path_for_hash("));
    assert!(source.contains("async fn shared_storage_path("));
    assert!(source.contains("source_hash"));
    assert!(source.contains("Sha256::digest(&bytes)"));
    assert!(source.contains("SELECT storage_path FROM files WHERE file_hash = $1"));
    assert!(source.contains("file_name, storage_path, file_type"));
    assert!(!source.contains("storage::project_root"));
}

#[test]
fn 旧バージョン管理と非対応ファイルのダウンロード導線がある() {
    let source = include_str!("handlers/files.rs");
    let html = DETAIL_HTML;
    assert!(source.contains("pub async fn list_file_history("));
    assert!(source.contains("pub async fn download_file_history("));
    assert!(source.contains("require_history_admin"));
    assert!(source.contains("user.1 != \"admin\""));
    assert!(source.contains("browser_openable"));
    assert!(source.contains("attachment; filename="));
    assert!(html.contains("旧バージョン管理"));
    assert!(html.contains("/api/file-histories/"));
    assert!(html.contains("data-file-history"));
    assert!(html.contains("currentUserRole===\"admin\""));
    assert!(html.contains("Number(file.version_number)>1"));
    assert!(html.contains("white-space:normal;overflow-wrap:anywhere;word-break:break-word"));
    assert!(html.contains("formatSize(f.file_size||0)"));
    assert!(html.contains("function updateViewerUi()"));
    assert!(html.contains("currentUserRole!==\"viewer\""));
    assert!(html.contains("[data-admin-only-control],.edit-btn"));
    assert!(html.contains("currentUserRole!==\"admin\""));
    assert!(html.contains("data-document-move"));
    assert!(html.contains("data-move-tab=\"documents\""));
    assert!(html.contains("data-admin-only-control href=\"/admin/projects"));
    assert!(html.contains("[data-document-move]"));
    assert!(html.contains("data-move-tab=\"${tab}\""));
    assert!(html.contains("data-note-open"));
    assert!(html.contains("data-note-delete"));
    assert!(ADMIN_PROJECTS_HTML.contains("data-admin-only-control"));
    assert!(DEALER_REGISTRATION_HTML.contains("data-admin-only-control"));
    assert!(ADMIN_PROJECTS_HTML
        .contains("name=\"assignee\" id=\"form-assignee\" list=\"assignees-list\""));
    assert!(ADMIN_PROJECTS_HTML.contains("<datalist id=\"assignees-list\"></datalist>"));
}

#[test]
fn 写真アップロードの本文サイズ上限を設定している() {
    let source = include_str!("routes.rs");
    assert!(source.contains("DefaultBodyLimit::max(100 * 1024 * 1024)"));
}

#[test]
fn ファイルのアップロード日時とユーザーの更新日時を表示する() {
    let detail = templates::render_page(DETAIL_HTML);
    let users = templates::render_page(USER_REGISTRATION_HTML);

    assert!(detail.contains("アップロード: ${formatUploadTime(f.created_at)}"));
    assert!(detail.contains("data-file-audit='"));
    assert!(detail.contains("/api/files/'+fileId+'/audit"));
    assert!(users.contains("<th>更新日時</th>"));
    assert!(users.contains("u.updated_at"));
    assert!(users.contains("user.updated_at"));
    assert!(!users.contains("<th>登録日時</th>"));
}

#[test]
fn web画面にコピーライトを表示する() {
    for html in [
        LOGIN_HTML,
        INDEX_HTML,
        ADMIN_HTML,
        ADMIN_PROJECTS_HTML,
        DEALER_REGISTRATION_HTML,
        USER_REGISTRATION_HTML,
        DETAIL_HTML,
        HELP_HTML,
    ] {
        let rendered = templates::render_page(html);
        assert!(rendered.contains("<body class=\"app-page\">"));
        assert!(rendered.contains("<footer class=\"app-copyright\">© 2026 FileManager3</footer>"));
    }
}

#[test]
fn モバイルの一覧を縦並びで表示する() {
    let projects = templates::render_page(ADMIN_PROJECTS_HTML);
    let detail = templates::render_page(DETAIL_HTML);
    let search = templates::render_page(INDEX_HTML);

    assert!(projects.contains(".table tbody tr{display:flex;flex-direction:column"));
    assert!(projects.contains("applyResponsiveTableLabels"));
    assert!(detail.contains(".file{flex-direction:column;align-items:stretch;gap:8px}"));
    assert!(detail.contains(".photo-grid{grid-template-columns:1fr;gap:12px}"));
    assert!(search.contains(".table tbody td{display:flex;align-items:flex-start"));
    assert!(search.contains("project-name-content"));
    assert!(search.contains("project-kana"));
    assert!(search.contains(".table tbody#results tr{margin-bottom:4px;padding:6px 8px}"));
    assert!(search.contains(".table tbody#results td{padding:4px 0!important}"));
    assert!(search.contains("id=\"mobile-search-sort\""));
    assert!(search.contains("検索結果のソート"));
    assert!(search.contains(".mobile-search-sort{display:none}"));
    assert!(search.contains("mobileSearchSort.value=queryParams.get('sort')||'updated_desc'"));
}

#[test]
fn デスクトップの右側メイン領域を横幅いっぱいに表示する() {
    let rendered = templates::render_page(INDEX_HTML);

    assert!(rendered.contains(
        "@media(min-width:821px){.main>.content{max-width:none;width:100%;margin-left:0;margin-right:0}}"
    ));
}

#[test]
fn デスクトップの位置情報を住所の右側に表示する() {
    let rendered = templates::render_page(DETAIL_HTML);

    assert!(rendered.contains(
        "@media(min-width:821px){.detail-location{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:16px}.detail-location>div+div{margin-top:0!important}}"
    ));
    assert!(rendered.contains("<span class=\"panel-sub\">住所</span>"));
    assert!(rendered.contains("<span class=\"panel-sub\">位置情報</span>"));
}

#[tokio::test]
async fn 書類アップロードは重複排除と版履歴を実際に保存する() {
    let (app, pool, storage_dir, token) = upload_test_app("test_storage_upload_document").await;
    sqlx::query("UPDATE projects SET updated_at = '2000-01-01 00:00:00' WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();
    let first_bytes = b"alpha searchable document";
    let first_hash = format!("{:x}", Sha256::digest(first_bytes));

    let (status, first) = post_upload_item(
        app.clone(),
        &token,
        1,
        "report.txt",
        first_bytes,
        Some("書類タグは保存しない"),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
    assert_eq!(first.version_number, 1);
    assert_eq!(
        sqlx::query_scalar::<_, Option<i64>>("SELECT uploaded_by FROM files WHERE id = $1")
            .bind(first.id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        Some(1)
    );
    assert_eq!(first.file_path, "report.txt");
    assert_eq!(first.file_type, "Document");
    assert_eq!(first.file_hash, first_hash);
    assert_eq!(first.source_hash, first_hash);
    assert_eq!(first.tag.as_deref(), Some(""));
    let updated_at: String =
        sqlx::query_scalar("SELECT updated_at::text FROM projects WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!updated_at.starts_with("2000-01-01"));

    let first_storage_path: String =
        sqlx::query_scalar("SELECT storage_path FROM files WHERE id = $1")
            .bind(first.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        tokio::fs::read(storage_dir.join(&first_storage_path))
            .await
            .unwrap(),
        first_bytes
    );
    let indexed_text: String =
        sqlx::query_scalar("SELECT content FROM file_search WHERE file_id = $1")
            .bind(first.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(indexed_text.contains("searchable document"));

    let (duplicate_status, duplicate) =
        post_upload_item(app.clone(), &token, 1, "report.txt", first_bytes, None).await;
    assert_eq!(duplicate_status, axum::http::StatusCode::OK);
    assert_eq!(duplicate.id, first.id);
    let active_reports: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM files WHERE file_name = 'report.txt' AND deleted_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(active_reports, 1);

    let (shared_status, shared) =
        post_upload_item(app.clone(), &token, 1, "copy.txt", first_bytes, None).await;
    assert_eq!(shared_status, axum::http::StatusCode::CREATED);
    assert_ne!(shared.id, first.id);
    let shared_storage_path: String =
        sqlx::query_scalar("SELECT storage_path FROM files WHERE id = $1")
            .bind(shared.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(shared_storage_path, first_storage_path);

    let second_bytes = b"beta updated searchable document";
    let (second_status, second) =
        post_upload_item(app.clone(), &token, 1, "report.txt", second_bytes, None).await;
    assert_eq!(second_status, axum::http::StatusCode::CREATED);
    assert_eq!(second.version_number, 2);
    assert_ne!(second.id, first.id);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM files WHERE file_name = 'report.txt' AND deleted_at IS NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM file_histories WHERE project_id = 1 AND file_name = 'report.txt' AND version_number = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, Option<i64>>("SELECT deleted_by FROM files WHERE id = $1")
            .bind(first.id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM file_search WHERE file_id = $1")
            .bind(first.id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(DISTINCT storage_path) FROM files")
            .fetch_one(&pool)
            .await
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn ファイルの論理削除者を保存する() {
    let (app, pool, _storage_dir, token) = upload_test_app("test_storage_file_delete_audit").await;
    let (_, item) =
        post_upload_item(app.clone(), &token, 1, "delete-me.txt", b"delete me", None).await;

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/files/{}", item.id))
                .header(header::COOKIE, format!("fm3_session={token}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::NO_CONTENT);
    assert_eq!(
        sqlx::query_scalar::<_, Option<i64>>(
            "SELECT deleted_by FROM files WHERE id = $1 AND deleted_at IS NOT NULL",
        )
        .bind(item.id)
        .fetch_one(&pool)
        .await
        .unwrap(),
        Some(1)
    );

    let audit_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/files/{}/audit", item.id))
                .header(header::COOKIE, format!("fm3_session={token}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(audit_response.status(), axum::http::StatusCode::OK);
    let audit: Vec<serde_json::Value> = serde_json::from_slice(
        &to_bytes(audit_response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(audit[0]["uploaded_by"], "admin");
    assert_eq!(audit[0]["deleted_by"], "admin");

    sqlx::query(
        "INSERT INTO users (username, password_hash, role) VALUES ('member-audit-test', 'test', 'member')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let member_token = "member-audit-test-token";
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES ($1, 2, CURRENT_TIMESTAMP + INTERVAL '1 hours')",
    )
    .bind(handlers::auth_handlers::hash_session_token(member_token))
    .execute(&pool)
    .await
    .unwrap();
    let forbidden = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/files/{}/audit", item.id))
                .header(header::COOKIE, format!("fm3_session={member_token}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden.status(), axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn 危険なファイル名のアップロードは保存前に拒否する() {
    let (app, pool, storage_dir, token) = upload_test_app("test_storage_upload_bad_name").await;
    let response = post_upload(app, &token, 1, "../secret.txt", b"secret", None).await;
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        String::from_utf8(body.to_vec()).unwrap(),
        "invalid filename"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM files")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    assert!(!storage_dir.join("secret.txt").exists());
}

#[tokio::test]
async fn 書類タブから画像をwebp変換して写真と同じ表示ができる() {
    let (app, pool, storage_dir, token) =
        upload_test_app("test_storage_upload_document_image").await;
    let mut image_bytes = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::new(2, 2))
        .write_to(
            &mut std::io::Cursor::new(&mut image_bytes),
            image::ImageFormat::Png,
        )
        .unwrap();

    let response = post_upload_with_category(
        app.clone(),
        &token,
        1,
        "document-image.png",
        &image_bytes,
        None,
        "documents",
    )
    .await;
    assert_eq!(response.status(), axum::http::StatusCode::CREATED);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let item = serde_json::from_slice::<models::FileItem>(&body).unwrap();
    assert_eq!(item.file_type, "Document");
    assert_eq!(item.file_path, "document-image.webp");

    let storage_path: String = sqlx::query_scalar("SELECT storage_path FROM files WHERE id = $1")
        .bind(item.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let stored_bytes = tokio::fs::read(storage_dir.join(&storage_path))
        .await
        .unwrap();
    assert_ne!(stored_bytes, image_bytes);
    assert_eq!(&stored_bytes[0..4], b"RIFF");
    assert_eq!(&stored_bytes[8..12], b"WEBP");

    let thumbnail_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/files/{}/thumbnail", item.id))
                .header(header::COOKIE, format!("fm3_session={token}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(thumbnail_response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        thumbnail_response.headers()[header::CONTENT_TYPE],
        "image/webp"
    );

    let open_response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/files/{}/open", item.id))
                .header(header::COOKIE, format!("fm3_session={token}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(open_response.status(), axum::http::StatusCode::OK);
    assert_eq!(open_response.headers()[header::CONTENT_TYPE], "image/webp");
    assert_eq!(
        open_response.headers()[header::CONTENT_DISPOSITION],
        "inline"
    );
}

#[test]
fn 認証済み画面はウィンドウを閉じた時に専用ログアウトapiへ送信する() {
    assert!(INDEX_HTML.contains("navigator.sendBeacon('/api/logout-on-close')"));
    assert!(INDEX_HTML.contains("fetch('/api/logout-on-close',{method:'POST',keepalive:true})"));
    assert!(!INDEX_HTML.contains("navigator.sendBeacon('/logout')"));
}

#[test]
fn passkey_status_apiを公開しない() {
    assert!(!include_str!("routes.rs").contains("/api/passkeys/login/status"));
    assert!(!LOGIN_HTML.contains("/api/passkeys/login/status"));
    assert!(!LOGIN_HTML.contains("if(false)"));
}

#[test]
fn 写真タブで動画形式を扱える() {
    assert!(is_picture("photo.jpg"));
    assert!(is_video("movie.mp4"));
    assert!(is_video("movie.MOV"));
    assert_eq!(file_content_type("movie.mp4"), "video/mp4");
    assert_eq!(file_content_type("movie.webm"), "video/webm");
    let source = DETAIL_HTML;
    assert!(source.contains("image/*,video/*"));
    assert!(source.contains("function isVideoFile(path)"));
    assert!(source.contains("video-thumb"));
}

#[test]
fn basic認証を実装しない() {
    let auth_source = include_str!("auth.rs");
    let handler_source = include_str!("handlers/auth_handlers.rs");
    assert!(!auth_source.contains("parse_basic_auth"));
    assert!(!handler_source.contains("AUTHORIZATION"));
}

#[test]
fn 未認証エラーはブラウザ標準ログインダイアログを要求しない() {
    let response = ApiError::Unauthorized.into_response();
    assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
    assert!(response.headers().get("www-authenticate").is_none());
}

#[tokio::test]
async fn ログアウトはログイン画面へ戻す() {
    let pool = test_pool().await;
    let state = Arc::new(AppState {
        pool,
        storage_root: PathBuf::from("./target/test_storage_logout"),
        access_urls: vec![],
        webauthn: Arc::new(
            WebauthnBuilder::new("localhost", &Url::parse("http://localhost:3000").unwrap())
                .unwrap()
                .build()
                .unwrap(),
        ),
        secure_cookie: false,
    });
    let response =
        handlers::auth_handlers::logout(axum::extract::State(state), axum::http::HeaderMap::new())
            .await
            .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::SEE_OTHER);
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::LOCATION)
            .unwrap(),
        "/"
    );
    let cookie = response
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(cookie.contains("fm3_session=;"));
}

#[tokio::test]
async fn ウィンドウ終了用ログアウトは更新用の短い猶予を設定する() {
    let pool = test_pool().await;
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: PathBuf::from("./target/test_storage_logout_on_close"),
        host: "127.0.0.1".to_string(),
        port: 0,
        admin_password: None,
        secure_cookie: false,
    };
    initialize_db(&pool, &config).await.unwrap();
    let token = "close-session-token";
    sqlx::query(
            "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES ($1, 1, CURRENT_TIMESTAMP + INTERVAL '1 hours')",
        )
        .bind(handlers::auth_handlers::hash_session_token(token))
        .execute(&pool)
        .await
        .unwrap();
    let state = Arc::new(AppState {
        pool: pool.clone(),
        storage_root: config.storage_dir.clone(),
        access_urls: vec![
            "http://127.0.0.1:3000".to_string(),
            "http://192.168.11.21:3000".to_string(),
        ],
        webauthn: Arc::new(
            WebauthnBuilder::new("localhost", &Url::parse("http://localhost:3000").unwrap())
                .unwrap()
                .build()
                .unwrap(),
        ),
        secure_cookie: false,
    });
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::COOKIE,
        format!("fm3_session={token}").parse().unwrap(),
    );

    let keepalive_response =
        handlers::auth_handlers::keepalive(axum::extract::State(state.clone()), headers.clone())
            .await
            .unwrap();
    assert_eq!(
        keepalive_response.status(),
        axum::http::StatusCode::NO_CONTENT
    );
    assert!(keepalive_response
        .headers()
        .contains_key(header::SET_COOKIE));
    let seconds_until_keepalive_expiry: f64 = sqlx::query_scalar(
        "SELECT EXTRACT(EPOCH FROM (expires_at - CURRENT_TIMESTAMP))::float8 FROM sessions",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(seconds_until_keepalive_expiry > 3590.0 && seconds_until_keepalive_expiry <= 3601.0);

    let status = handlers::auth_handlers::logout_on_close(axum::extract::State(state), headers)
        .await
        .unwrap();
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
    let seconds_until_expiry: f64 = sqlx::query_scalar(
        "SELECT EXTRACT(EPOCH FROM (expires_at - CURRENT_TIMESTAMP))::float8 FROM sessions",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(seconds_until_expiry > 0.0 && seconds_until_expiry <= 6.0);
}

#[tokio::test]
async fn セッションcookieからトークンを取得できる() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::COOKIE,
        "fm3_session=token-12345; other=dummy".parse().unwrap(),
    );
    assert_eq!(
        handlers::auth_handlers::session_token(&headers),
        Some("token-12345".to_string())
    );
}

#[tokio::test]
async fn settings_jsonの読み込みとデフォルト作成が正常に機能する() {
    let pool = test_pool().await;
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: PathBuf::from("./target/test_storage_settings"),
        host: "127.0.0.1".to_string(),
        port: 0,
        admin_password: Some("custom-pass-999".to_string()),
        secure_cookie: false,
    };
    initialize_db(&pool, &config).await.unwrap();
    let authed = auth::authenticate_credentials(&pool, "admin", "custom-pass-999")
        .await
        .unwrap();
    assert!(authed.is_some());
}

#[tokio::test]
async fn 管理者パスワードは起動時に上書きされない() {
    let pool = test_pool().await;
    let config1 = AppConfig {
        database_url: admin_database_url(),
        storage_dir: PathBuf::from("./target/test_storage_sync1"),
        host: "127.0.0.1".to_string(),
        port: 0,
        admin_password: Some("first-password".to_string()),
        secure_cookie: false,
    };
    initialize_db(&pool, &config1).await.unwrap();

    let config2 = AppConfig {
        database_url: admin_database_url(),
        storage_dir: PathBuf::from("./target/test_storage_sync2"),
        host: "127.0.0.1".to_string(),
        port: 0,
        admin_password: Some("updated-password".to_string()),
        secure_cookie: false,
    };
    initialize_db(&pool, &config2).await.unwrap();

    let old_auth = auth::authenticate_credentials(&pool, "admin", "first-password")
        .await
        .unwrap();
    assert!(old_auth.is_some());

    let new_auth = auth::authenticate_credentials(&pool, "admin", "updated-password")
        .await
        .unwrap();
    assert!(new_auth.is_none());
}

#[tokio::test]
async fn 販売店登録と重複防止が正常に動作する() {
    let pool = test_pool().await;
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: PathBuf::from("./target/test_storage_dealers"),
        host: "127.0.0.1".to_string(),
        port: 0,
        admin_password: None,
        secure_cookie: false,
    };
    initialize_db(&pool, &config).await.unwrap();

    sqlx::query("INSERT INTO dealers (name, address) VALUES ($1, $2)")
        .bind("テスト建材")
        .bind("東京都")
        .execute(&pool)
        .await
        .unwrap();

    let err = sqlx::query("INSERT INTO dealers (name, address) VALUES ($1, $2)")
        .bind("テスト建材")
        .bind("大阪府")
        .execute(&pool)
        .await;
    assert!(err.is_err());
}

#[tokio::test]
async fn 販売店一覧は登録件数を返す() {
    let pool = test_pool().await;
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: PathBuf::from("./target/test_storage_dealer_count"),
        host: "127.0.0.1".to_string(),
        port: 0,
        admin_password: None,
        secure_cookie: false,
    };
    initialize_db(&pool, &config).await.unwrap();

    sqlx::query("INSERT INTO dealers (name, address) VALUES ('件数販売店', '東京都')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO projects (project_number, name, kana, dealer) VALUES ('C001', '件数案件1', 'ケンスウアンケン1', '件数販売店')")
            .execute(&pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO projects (project_number, name, kana, dealer, deleted_at) VALUES ('C002', '件数案件2', 'ケンスウアンケン2', '件数販売店', CURRENT_TIMESTAMP)")
            .execute(&pool)
            .await
            .unwrap();

    let state = Arc::new(AppState {
        pool,
        storage_root: config.storage_dir,
        access_urls: vec![],
        webauthn: Arc::new(
            WebauthnBuilder::new("localhost", &Url::parse("http://localhost:3000").unwrap())
                .unwrap()
                .build()
                .unwrap(),
        ),
        secure_cookie: false,
    });

    let dealers = handlers::dealers::list_dealers(axum::extract::State(state))
        .await
        .unwrap()
        .0;
    let dealer = dealers.iter().find(|d| d.name == "件数販売店").unwrap();
    assert_eq!(dealer.project_count, 1);
}

#[tokio::test]
async fn 管理者専用の仕様書画面を公開する() {
    let routes = std::fs::read_to_string("src/routes.rs").unwrap();
    let specifications = std::fs::read_to_string("src/specifications.rs").unwrap();
    let admin_html = std::fs::read_to_string("src/templates/admin.html").unwrap();
    assert!(routes.contains("/admin/specifications"));
    assert!(routes.contains("specifications::index_page"));
    assert!(routes.contains("specifications::detail_page"));
    assert!(admin_html.contains("href=\"/admin/specifications\""));
    assert!(admin_html.contains("仕様書"));
    assert!(specifications.contains("if role != \"admin\""));
    assert!(specifications.contains("01_overview"));
    assert!(specifications.contains("06_configuration"));
    assert!(specifications.contains("07_android"));
    assert!(specifications.contains("10_docker_postgresql"));
    assert!(specifications.contains("11_ubuntu_apache"));
    assert!(specifications.contains("render_markdown(spec.body)"));
}

#[test]
fn android_apk配布ルートを公開する() {
    let routes = std::fs::read_to_string("src/routes.rs").unwrap();
    let android_handler = std::fs::read_to_string("src/handlers/android.rs").unwrap();
    let android_doc = std::fs::read_to_string("docs/07_android.md").unwrap();
    assert!(routes.contains("/api/android/latest"));
    assert!(routes.contains("/android/filemanager3-android-release.apk"));
    assert!(android_handler.contains("FILEMANAGER_ANDROID_RELEASE_DIR"));
    assert!(android_handler.contains("latest.json"));
    assert!(android_doc.contains("SHA-256"));
}

#[tokio::test]
async fn 案件検索画面のメニューと入力形式が要件どおりである() {
    // 案件検索（条件入力画面）の検証
    assert!(INDEX_HTML.contains("案件検索"));
    assert!(INDEX_HTML.contains("管理メニュー"));
    assert!(INDEX_HTML.contains("search-project-input"));
    assert!(INDEX_HTML.contains("name=\"search\""));
    assert!(INDEX_HTML.contains("viewport-fit=cover"));
    assert!(INDEX_HTML.contains("env(safe-area-inset-top)"));
    assert!(INDEX_HTML.contains("min-width: 44px"));
    assert!(INDEX_HTML.contains("class=\"top-logout\" href=\"/logout\""));
    assert!(INDEX_HTML.contains(".sidebar .brand > span, .sidebar .nav-label, .sidebar .nav-item span, .sidebar .sidebar-foot { display: block !important; }"));
    assert!(INDEX_HTML.contains("width: min(300px, 86vw)"));
    assert!(INDEX_HTML.contains("class=\"nav-item admin-nav-link\""));
    assert!(INDEX_HTML.contains("class=\"nav-item\" href=\"/admin/dealers\""));
    assert!(INDEX_HTML.contains("class=\"nav-item admin-only-nav\" href=\"/admin/users\""));
    assert!(INDEX_HTML.contains("role===\"admin\""));
    assert!(INDEX_HTML.contains("setAdminMenuVisibility"));
    assert!(INDEX_HTML.contains("var canManage=role===\"admin\"||role===\"member\""));
    assert!(INDEX_HTML
        .contains("if(role!==\"admin\")document.querySelectorAll(\"[data-admin-only-control]\")"));
    assert!(INDEX_HTML.contains("fm3_top_role"));
    assert!(INDEX_HTML.contains("loadResults()"));
    assert!(INDEX_HTML.contains("window.location.href='/projects/'+p.id"));
    assert!(!INDEX_HTML.contains("placeholder=\"例："));
    assert!(!INDEX_HTML.contains("Quick Menu"));
    assert!(!INDEX_HTML.contains("新規登録はこちら"));
    // 検索画面の下部に結果一覧テーブルを表示する
    assert!(INDEX_HTML.contains("案件番号 <span class=\"sort-indicator\">"));
    assert!(INDEX_HTML.contains("id=\"results\""));
    assert!(INDEX_HTML.contains("data-sort=\"number\""));
    assert!(INDEX_HTML.contains("data-sort=\"name\""));
    assert!(INDEX_HTML.contains("data-sort=\"updated\""));
    assert!(INDEX_HTML.contains("<th>登録データ</th>"));

    // 検索結果一覧画面の検証（INDEX_HTMLに一本化）
    assert!(PROJECT_RESULTS_HTML.contains("検索結果"));
    assert!(PROJECT_RESULTS_HTML.contains("該当件数"));
    assert!(PROJECT_RESULTS_HTML.contains("class=\"project-name\""));
    assert!(PROJECT_RESULTS_HTML.contains("class=\"project-number\""));
    assert!(PROJECT_RESULTS_HTML.contains("class=\"address-text\""));
    assert!(PROJECT_RESULTS_HTML.contains("class=\"dealer-text\""));
    assert!(PROJECT_RESULTS_HTML.contains("clickable-row"));
    assert!(PROJECT_RESULTS_HTML.contains("window.location.href='/projects/'+p.id"));
    assert!(PROJECT_RESULTS_HTML.contains("data-sort=\"number\""));
    assert!(PROJECT_RESULTS_HTML.contains("data-sort=\"name\""));
    assert!(PROJECT_RESULTS_HTML.contains("data-sort=\"updated\""));
    assert!(PROJECT_RESULTS_HTML.contains("<th>登録データ</th>"));
    assert!(PROJECT_RESULTS_HTML.contains("hideProjectPhoneColumn"));
    assert!(PROJECT_RESULTS_HTML.contains("th.dataset.sort"));
    assert!(PROJECT_RESULTS_HTML.contains("queryParams.set('sort'"));
    assert!(PROJECT_RESULTS_HTML.contains("loadResults()"));
    assert!(PROJECT_RESULTS_HTML.contains("sortKeys"));
    assert!(!PROJECT_RESULTS_HTML.contains("<th>担当者</th>"));
}

#[tokio::test]
#[allow(non_snake_case)]
async fn 単一テキストボックスのキーワード検索と複数語AND検索が動作する() {
    let pool = test_pool().await;
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: PathBuf::from("./target/test_storage_single_search"),
        host: "127.0.0.1".to_string(),
        port: 0,
        admin_password: None,
        secure_cookie: false,
    };
    initialize_db(&pool, &config).await.unwrap();

    // テストユーザー作成
    let user_id: i64 = sqlx::query_scalar("INSERT INTO users (username, password_hash, role) VALUES ('test_user', 'hash', 'admin') RETURNING id")
        .fetch_one(&pool)
        .await
        .unwrap();

    // セッショントークン作成
    let token = "test_token_12345";
    let token_hash = handlers::auth_handlers::hash_session_token(token);
    sqlx::query("INSERT INTO sessions (token_hash, user_id, expires_at) VALUES ($1, $2, CURRENT_TIMESTAMP + INTERVAL '1 hour')")
        .bind(token_hash)
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

    // 案件データの登録
    sqlx::query(
        "INSERT INTO projects (project_number, name, kana, address, dealer, assignee) VALUES
         ('001', '青山ビル改修', 'アオヤマビルカイシュウ', '東京都港区南青山1-1', '山田商事', '田中'),
         ('002', '青山レジデンス新築', 'アオヤマレジデンスシンチク', '東京都港区北青山2-2', '佐藤興業', '鈴木'),
         ('003', '新宿オフィス改修', 'シンジュクオフィスカイシュウ', '東京都新宿区西新宿3-3', '山田商事', '高橋')"
    ).execute(&pool).await.unwrap();

    let state = Arc::new(AppState {
        pool,
        storage_root: config.storage_dir,
        access_urls: vec![],
        webauthn: Arc::new(
            WebauthnBuilder::new("localhost", &Url::parse("http://localhost:3000").unwrap())
                .unwrap()
                .build()
                .unwrap(),
        ),
        secure_cookie: false,
    });

    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        HeaderValue::from_str(&format!("fm3_session={}", token)).unwrap(),
    );

    // 1. "青山" で検索 -> 001, 002
    let params = handlers::projects::SearchParams {
        search: Some("青山".to_string()),
        q: None,
        project_number: None,
        project_name: None,
        project_kana: None,
        address: None,
        dealer: None,
        sort: None,
    };
    let list = handlers::projects::search_projects(
        axum::extract::State(state.clone()),
        headers.clone(),
        axum::extract::Query(params),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(list.len(), 2);

    // 2. "青山 改修" でAND検索 -> 001 のみ
    let params_and = handlers::projects::SearchParams {
        search: Some("青山 改修".to_string()),
        q: None,
        project_number: None,
        project_name: None,
        project_kana: None,
        address: None,
        dealer: None,
        sort: None,
    };
    let list_and = handlers::projects::search_projects(
        axum::extract::State(state.clone()),
        headers.clone(),
        axum::extract::Query(params_and),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(list_and.len(), 1);
    assert_eq!(list_and[0].project_number, "001");

    // 3. 販売店 "山田商事" で検索 -> 001, 003
    let params_dealer = handlers::projects::SearchParams {
        search: Some("山田商事".to_string()),
        q: None,
        project_number: None,
        project_name: None,
        project_kana: None,
        address: None,
        dealer: None,
        sort: None,
    };
    let list_dealer = handlers::projects::search_projects(
        axum::extract::State(state.clone()),
        headers.clone(),
        axum::extract::Query(params_dealer),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(list_dealer.len(), 2);
}

#[test]
fn 案件検索のソート条件は許可された順序だけを使う() {
    assert_eq!(
        project_order_by(Some("updated_asc")),
        "updated_at ASC, id ASC"
    );
    assert_eq!(
        project_order_by(Some("name_desc")),
        "LOWER(name) DESC, id DESC"
    );
    assert_eq!(
        project_order_by(Some("unknown")),
        "updated_at DESC, id DESC"
    );
}

#[tokio::test]
async fn 案件登録と管理メニューの導線が要件どおりである() {
    assert!(ADMIN_HTML.contains("管理メニュー"));
    assert!(ADMIN_HTML.contains("案件管理"));
    assert!(ADMIN_HTML.contains("販売店管理"));
    assert!(ADMIN_HTML.contains("ユーザー管理"));
    assert!(!ADMIN_HTML.contains("<ctrl42>"));
    assert!(ADMIN_HTML.contains("<span>▥</span><span>販売店管理</span>"));
    assert!(!ADMIN_HTML.contains("Quick Menu"));
    assert!(!ADMIN_HTML.contains("登録件数"));
}

#[tokio::test]
async fn 管理メニュー配下に管理画面リンクがまとまっている() {
    assert!(ADMIN_HTML.contains("href=\"/admin/projects\""));
    assert!(ADMIN_HTML.contains("href=\"/admin/dealers\""));
    assert!(ADMIN_HTML.contains("href=\"/admin/users\""));
    assert!(ADMIN_HTML.contains("ユーザー管理"));
}

#[test]
fn 案件管理の一覧行はクリック遷移しない() {
    assert!(ADMIN_PROJECTS_HTML.contains("projectList.innerHTML = ''"));
    assert!(ADMIN_PROJECTS_HTML.contains("form.phone.value=p.phone||''"));
    assert!(ADMIN_PROJECTS_HTML.contains("form.email.value=p.email||''"));
    assert!(ADMIN_PROJECTS_HTML.contains("hideAdminProjectContactColumns"));
    assert!(ADMIN_PROJECTS_HTML.contains("toggleDeletedProjects"));
    assert!(ADMIN_PROJECTS_HTML.contains("削除済みを非表示"));
    assert!(ADMIN_PROJECTS_HTML.contains("<label>電話番号（任意）</label>"));
    assert!(ADMIN_PROJECTS_HTML.contains("normalizePhoneInput"));
    assert!(
        ADMIN_PROJECTS_HTML.contains("onblur=\"this.value=normalizeProjectNumber(this.value)\"")
    );
    assert!(
        !ADMIN_PROJECTS_HTML.contains("oninput=\"this.value=normalizeProjectNumber(this.value)\"")
    );
    assert!(ADMIN_PROJECTS_HTML.contains("name=\"project_number\" type=\"text\""));
    assert!(ADMIN_PROJECTS_HTML
        .contains("<button class=\"btn-sm\" onclick=\"openProjectEdit(${p.id})\""));
    assert!(ADMIN_PROJECTS_HTML.contains("width:min(420px,calc(100vw - 24px))"));
    assert!(ADMIN_PROJECTS_HTML.contains("padding:12px 16px"));
    assert!(ADMIN_PROJECTS_HTML.contains("margin-bottom:8px"));
    assert!(ADMIN_PROJECTS_HTML.contains("grid-template-columns:1fr;gap:8px"));
    assert!(ADMIN_PROJECTS_HTML.contains("<label>案件番号</label>"));
    assert!(ADMIN_PROJECTS_HTML.contains("justify-content:flex-end;gap:8px;margin-top:8px"));
    assert!(ADMIN_PROJECTS_HTML.contains("id=\"message\" style=\"margin:6px 0 0\""));
    assert!(ADMIN_PROJECTS_HTML.contains("<div><label>位置情報（任意）</label>"));
    assert!(!ADMIN_PROJECTS_HTML.contains("grid-template-columns:140px 1fr"));
    assert!(!ADMIN_PROJECTS_HTML.contains("grid-column:1/-1"));
    assert!(!ADMIN_PROJECTS_HTML.contains("grid-column:auto;padding:8px"));
    assert!(!ADMIN_PROJECTS_HTML.contains("placeholder=\"例："));
    assert!(!ADMIN_PROJECTS_HTML.contains("緯度経度（例："));
    assert!(!ADMIN_PROJECTS_HTML.contains("onclick=\"window.location.href='/projects/${p.id}'\""));
    assert!(!ADMIN_PROJECTS_HTML.contains("event.stopPropagation();openProjectEdit(${p.id})"));
}

#[test]
fn 販売店登録ダイアログは小さく右下にボタンを配置する() {
    assert!(DEALER_REGISTRATION_HTML.contains("width:min(420px,calc(100vw - 24px))"));
    assert!(DEALER_REGISTRATION_HTML.contains("border-radius:12px;padding:12px 16px"));
    assert!(DEALER_REGISTRATION_HTML.contains("grid-template-columns:1fr;gap:8px"));
    assert!(DEALER_REGISTRATION_HTML.contains("justify-content:flex-end;gap:8px;margin-top:8px"));
    assert!(DEALER_REGISTRATION_HTML.contains("id=\"message\" style=\"margin:6px 0 0\""));
    assert!(DEALER_REGISTRATION_HTML.contains("<th>登録件数</th>"));
    assert!(DEALER_REGISTRATION_HTML.contains("メールアドレス（任意）"));
    assert!(DEALER_REGISTRATION_HTML.contains("esc(d.email||'未登録')"));
    assert!(DEALER_REGISTRATION_HTML.contains("hideDealerContactColumns"));
    assert!(DEALER_REGISTRATION_HTML.contains("hideDealerContactColumns"));
    assert!(DEALER_REGISTRATION_HTML.contains("toggleDeletedDealers"));
    assert!(DEALER_REGISTRATION_HTML.contains("削除済みを非表示"));
    assert!(DEALER_REGISTRATION_HTML.contains("Number(d.project_count||0)"));
    assert!(DEALER_REGISTRATION_HTML
        .contains("const dealerForm = document.querySelector('#dealer-form')"));
    assert!(!DEALER_REGISTRATION_HTML.contains("placeholder=\"例："));
    assert!(!DEALER_REGISTRATION_HTML.contains("grid-column:1/-1"));
    assert!(USER_REGISTRATION_HTML.contains("grid-template-columns:1fr;gap:8px"));
    assert!(USER_REGISTRATION_HTML.contains("width:min(420px,calc(100vw - 24px))"));
    assert!(USER_REGISTRATION_HTML.contains("border-radius:12px;padding:12px 16px"));
    assert!(USER_REGISTRATION_HTML.contains("justify-content:flex-end;gap:8px;margin-top:8px"));
    assert!(USER_REGISTRATION_HTML.contains("id=\"user-submit\" style=\"margin-top:0\""));
    assert!(!USER_REGISTRATION_HTML.contains("width:min(620px,calc(100vw - 32px))"));
    assert!(!USER_REGISTRATION_HTML.contains("id=\"user-submit\" style=\"margin-top:16px\""));
    assert!(!USER_REGISTRATION_HTML.contains("placeholder=\"例："));
}

#[test]
fn location_exclusive_validation() {
    assert!(validate_project_location(None, None, None).is_ok());
    assert!(validate_project_location(Some(35.0), Some(139.0), None).is_ok());
    assert!(validate_project_location(None, None, Some("8Q7X+5V")).is_ok());
    assert!(validate_project_location(Some(35.0), Some(139.0), Some("8Q7X+5V")).is_err());
    assert!(validate_project_location(Some(999.0), Some(139.0), None).is_err());
}

#[test]
fn phone_number_normalization() {
    assert_eq!(
        normalize_phone(Some("０３－１２３４－５６７８")),
        Some("0312345678".to_string())
    );
    assert_eq!(
        normalize_phone(Some(" 03-1234 5678 ")),
        Some("0312345678".to_string())
    );
    assert_eq!(normalize_phone(Some("---")), None);
}

#[test]
fn project_number_normalization() {
    assert_eq!(normalize_project_number("９９９-Ａ"), "999");
    assert_eq!(normalize_project_number(" 12 34 "), "1234");
    assert_eq!(normalize_project_number("ＡＢＣ"), "");
}

#[test]
fn 担当者管理は登録ダイアログと編集操作を持つ() {
    assert!(DEALER_REGISTRATION_HTML.contains("id=\"contact-form-panel\""));
    assert!(DEALER_REGISTRATION_HTML.contains("openContactForm"));
    assert!(DEALER_REGISTRATION_HTML.contains("contactEditId ? 'PUT' : 'POST'"));
    assert!(DEALER_REGISTRATION_HTML.contains("normalizePhoneInput"));
    assert!(DEALER_REGISTRATION_HTML.contains("メールアドレス（任意）"));
    assert!(DEALER_REGISTRATION_HTML.contains("type=\"email\""));
    assert!(DEALER_REGISTRATION_HTML.contains("c.email || '未登録'"));
    assert!(DEALER_REGISTRATION_HTML.contains("dealerForm.phone.addEventListener('blur'"));
    assert!(DEALER_REGISTRATION_HTML.contains("justify-content:flex-end;margin-bottom:10px\"><button type=\"button\" class=\"btn\" onclick=\"openContactForm()\""));
    assert!(DEALER_REGISTRATION_HTML.contains("編集"));
    assert!(DEALER_REGISTRATION_HTML.contains("loadDeletedDealerNotes"));
    assert!(DEALER_REGISTRATION_HTML.contains("data-dealer-note-restore"));
}

#[test]
fn 詳細画面には案件詳細を含むタブがある() {
    let html = detail_page(1, "admin");
    let viewer_html = detail_page(1, "viewer");
    assert!(html.contains("案件詳細"));
    assert!(html.contains("detail.project.email"));
    assert!(html.contains("let detail,activeTab='overview'"));
    assert!(viewer_html.contains("currentUserRole='viewer'"));
    assert!(html.contains("data-tab=\"overview\""));
    assert!(html.contains("class=\"breadcrumb detail-top\""));
    assert!(html.contains("class=\"top-back\" id=\"detail-back-link\" href=\"/projects\""));
    assert!(html.contains("fm3_last_projects_url"));
    assert!(html.contains("id=\"detail-number\""));
    assert!(html.contains("class=\"detail-number\""));
    assert!(html.contains("number.textContent=detail.project.project_number"));
    assert!(html.contains("id=\"detail-title\""));
    assert!(html.contains("<h2>メモ</h2>"));
    assert!(!html.contains("案件メモ履歴"));
    assert!(html.contains("data-note-open"));
    assert!(html.contains("function openNoteDialog()"));
    assert!(html.contains("id=\\\"note-form\\\""));
    assert!(html.contains("data-note-deleted"));
    assert!(html.contains("/notes/deleted"));
    assert!(html.contains("data-note-restore"));
    assert!(html.contains("title.textContent=detail.project.name"));
    assert!(html.contains("<h2>案件情報</h2>"));
    assert!(html.contains("<h2>販売店情報</h2>"));
    assert!(html.contains(
        "<label>電話番号</label><strong>${esc(detail.project.phone||\"未登録\")}</strong>"
    ));
    assert!(html.contains("function mapUrl(value)"));
    assert!(html.contains("function phoneAnchor(value,label)"));
    assert!(html.contains("class=\"phone-link\""));
    assert!(html.contains("encodeURIComponent(phone)"));
    assert!(!html.contains("案件の基本情報"));
    assert!(!html.contains("販売店と担当者の情報"));
    assert!(!html.contains("住所・位置情報"));
    assert!(html.contains("<span class=\"panel-sub\">住所</span>"));
    assert!(html.contains("<span class=\"panel-sub\">位置情報</span>"));
    assert!(html.contains("class=\"detail-item detail-location\""));
    assert_eq!(html.matches("class=\"detail-card\"").count(), 2);
    assert!(!html.contains("案件検索に戻る"));
    assert!(!html.contains("location.href.replace('127.0.0.1','localhost')"));
    assert!(!html.contains("<section class=\"hero\""));
    assert!(!html.contains("<div class=\"hero\"><h1>"));
    assert!(html.contains("書類"));
    assert!(html.contains("写真"));
    assert!(html.contains("data-tab=\"documents\""));
    assert!(html.contains("data-tab=\"pictures\""));
    assert!(html.contains("id=\"upload-form\""));
    assert!(html.contains("data-upload-open=\"${tab}\""));
    assert!(html.contains("function uploadDialog(tab,allTags)"));
    assert!(html.contains("function uploadFilePreview(file,idx,preview,summaryEl)"));
    assert!(html.contains("upload-remove-btn"));
    assert!(html.contains("upload-preview-grid"));
    assert!(html.contains(".upload-preview-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(160px,1fr))"));
    assert!(html.contains("upload-preview-card"));
    assert!(html.contains("photo-thumb-container"));
    assert!(html.contains("previewObjectUrls"));
    assert!(html.contains("uploadSelectedFiles"));
    assert!(html.contains("function setUploadImagePreview(img,file,wrap)"));
    assert!(html.contains("URL.createObjectURL(file)"));
    assert!(html.contains("function addUploadFiles(newFiles,preview,summaryEl)"));
    assert!(html.contains("addUploadFiles(e.dataTransfer.files,preview,summary)"));
    assert!(html.contains("const files=uploadSelectedFiles.slice()"));
    assert!(html.contains("isImageFile"));
    assert!(html.contains("function uploadFilesInBackground(tab,files,tag)"));
    assert!(html.contains("Promise.all(files.map(async file"));
    assert!(html.contains("スキップ"));
    assert!(html.contains("if(res.status===200){skipped++;return}"));
    assert!(html.contains("data-photo-preview=\"${f.id}\""));
    assert!(html.contains("/api/files/${f.id}/open"));
    assert!(html.contains("preview-arrow-previous"));
    assert!(html.contains("preview-arrow-next"));
    assert!(html.contains("aria-label','前の写真'"));
    assert!(html.contains("aria-label','次の写真'"));
    assert!(html.contains("ArrowLeft"));
    assert!(html.contains("ArrowRight"));
    assert!(html.contains("aria-label=\"書類操作\""));
    assert!(html.contains("data-photo-menu-panel=\"${f.id}\""));
    assert!(html.contains("list=\"tag-history-list\""));
    assert!(!html.contains("id=\"tag-select-input\""));
    assert!(html.contains("function openUploadDialog(tab)"));
    assert!(html.contains("function closeUploadDialog()"));
    assert!(html.contains("role=\"dialog\""));
    assert!(html.contains("id=\"upload-tag\""));
    assert!(html.contains("data-photo-tag=\"\""));
    assert!(html.contains("全て"));
    assert!(html.contains("multiple accept=\"${accept}\""));
    assert!(html.contains("category=\"+encodeURIComponent(tab)"));
    assert!(html.contains(".pdf,.txt,.md,.csv,.doc,.docx,.xls,.xlsx,.ppt,.pptx,image/*,video/*"));
    assert!(html.contains("function fileName(path)"));
    assert!(html.contains("function isImageFilePath(path)"));
    assert!(html.contains("function imagePreviewCardHtml(f,menuLabel)"));
    assert!(html.contains("function openDocumentImagePreview(fileId)"));
    assert!(html.contains("data-document-image-preview"));
    assert!(html.contains("fileName(f.file_path)"));
    assert!(html.contains("function photoFilters(files)"));
    assert!(html.contains("function filterPhotos(tag)"));
    assert!(html.contains("function formatSize(bytes)"));
    assert!(html.contains("class=\"photo-meta\""));
    assert!(html.contains("class=\"photo-tag\""));
    assert!(html.contains("<option value=\"\">なし</option>"));
    assert!(html.contains("class=\"photo-menu-btn\""));
    assert!(html.contains("/api/files/${f.id}/download"));
    assert!(html.contains("const deleteButton=e.target.closest('[data-photo-delete]')"));
    assert!(html.contains("batch-move"));
    assert!(html.contains("var move=e.target.closest('[data-tag-edit]')"));
    assert!(html.contains("/api/files/${f.id}/thumbnail"));
}

#[tokio::test]
async fn 案件と販売店は論理削除で一覧対象から除外される() {
    let pool = test_pool().await;
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: PathBuf::from("./target/test_storage_softdelete"),
        host: "127.0.0.1".to_string(),
        port: 0,
        admin_password: None,
        secure_cookie: false,
    };
    initialize_db(&pool, &config).await.unwrap();

    sqlx::query("INSERT INTO projects (project_number, name, kana, address, dealer) VALUES ('001', 'テスト案件', 'テスト', '東京都', 'テスト販売店')")
            .execute(&pool)
            .await
            .unwrap();
    let dealer_id: i64 = sqlx::query_scalar(
        "INSERT INTO dealers (name, address) VALUES ('テスト販売店', '東京都') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(maintenance::soft_delete_project(&pool, 1).await.unwrap(), 1);
    let deleted_projects = maintenance::list_deleted_projects(&pool).await.unwrap();
    assert_eq!(deleted_projects.len(), 1);
    assert_eq!(deleted_projects[0].id, 1);
    assert!(!deleted_projects[0].deleted_at.is_empty());

    assert_eq!(maintenance::restore_project(&pool, 1).await.unwrap(), 1);
    assert!(maintenance::list_deleted_projects(&pool)
        .await
        .unwrap()
        .is_empty());

    assert_eq!(maintenance::soft_delete_project(&pool, 1).await.unwrap(), 1);
    assert_eq!(
        maintenance::permanently_delete_project(&pool, &config.storage_dir, 1)
            .await
            .unwrap(),
        1
    );
    let project_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE id = 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(project_count, 0);

    assert_eq!(
        maintenance::soft_delete_dealer(&pool, dealer_id)
            .await
            .unwrap(),
        1
    );
    let deleted_dealers = maintenance::list_deleted_dealers(&pool).await.unwrap();
    assert_eq!(deleted_dealers.len(), 1);
    assert_eq!(deleted_dealers[0].id, dealer_id);
    assert!(!deleted_dealers[0].deleted_at.is_empty());

    assert_eq!(
        maintenance::restore_dealer(&pool, dealer_id).await.unwrap(),
        1
    );
    assert!(maintenance::list_deleted_dealers(&pool)
        .await
        .unwrap()
        .is_empty());

    assert_eq!(
        maintenance::soft_delete_dealer(&pool, dealer_id)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        maintenance::permanently_delete_dealer(&pool, dealer_id)
            .await
            .unwrap(),
        1
    );
    let dealer_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dealers WHERE id = $1")
        .bind(dealer_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(dealer_count, 0);
}

#[tokio::test]
async fn 整合性検査はデータベースとストレージの不一致を検出する() {
    let pool = test_pool().await;
    let storage_dir = PathBuf::from(format!(
        "./target/test_storage_integrity_check-{}",
        uuid::Uuid::new_v4()
    ));
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: storage_dir.clone(),
        host: "127.0.0.1".to_string(),
        port: 0,
        admin_password: None,
        secure_cookie: false,
    };
    initialize_db(&pool, &config).await.unwrap();
    sqlx::query(
        "INSERT INTO projects (project_number, name, kana) VALUES ('CHECK001', '検査案件', 'ケンサアンケン')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO files (project_id, file_name, storage_path, file_type, file_size, file_hash, source_hash) VALUES (1, 'missing.txt', 'missing.txt', 'Document', 3, 'invalid-hash', 'invalid-source-hash')",
    )
    .execute(&pool)
    .await
    .unwrap();
    tokio::fs::write(storage_dir.join("orphan.txt"), b"orphan")
        .await
        .unwrap();

    let report = integrity::check_integrity(&pool, &storage_dir)
        .await
        .unwrap();
    assert!(!report.is_clean());
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.contains("DBにあるファイルが存在しない")));
    assert!(report
        .issues
        .iter()
        .any(|issue| issue.contains("DBから参照されないファイル")));
}

#[tokio::test]
async fn 起動時に未確定ストレージを隔離する() {
    let pool = test_pool().await;
    let storage_dir = PathBuf::from(format!(
        "./target/test_storage_startup_quarantine-{}",
        uuid::Uuid::new_v4()
    ));
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: storage_dir.clone(),
        host: "127.0.0.1".to_string(),
        port: 0,
        admin_password: None,
        secure_cookie: false,
    };
    initialize_db(&pool, &config).await.unwrap();
    tokio::fs::write(storage_dir.join(".upload-stale"), b"temporary")
        .await
        .unwrap();
    tokio::fs::write(storage_dir.join("orphan.txt"), b"orphan")
        .await
        .unwrap();

    integrity::reconcile_startup(&pool, &storage_dir)
        .await
        .unwrap();
    assert!(!storage_dir.join(".upload-stale").exists());
    assert!(!storage_dir.join("orphan.txt").exists());
    let mut entries = tokio::fs::read_dir(storage_dir.join(integrity::QUARANTINE_DIR_NAME))
        .await
        .unwrap();
    let mut quarantined = 0;
    while entries.next_entry().await.unwrap().is_some() {
        quarantined += 1;
    }
    assert_eq!(quarantined, 2);
}

#[tokio::test]
async fn 物理削除前にファイルを隔離してからデータベースを削除する() {
    let pool = test_pool().await;
    let storage_dir = PathBuf::from(format!(
        "./target/test_storage_delete_quarantine-{}",
        uuid::Uuid::new_v4()
    ));
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: storage_dir.clone(),
        host: "127.0.0.1".to_string(),
        port: 0,
        admin_password: None,
        secure_cookie: false,
    };
    initialize_db(&pool, &config).await.unwrap();
    sqlx::query(
        "INSERT INTO projects (project_number, name, kana) VALUES ('DELETE001', '削除案件', 'サクジョアンケン')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO files (project_id, file_name, storage_path, file_type, file_size, file_hash, source_hash, deleted_at) VALUES (1, 'delete.txt', 'delete.txt', 'Document', 6, 'delete-hash', 'delete-source-hash', CURRENT_TIMESTAMP)",
    )
    .execute(&pool)
    .await
    .unwrap();
    tokio::fs::write(storage_dir.join("delete.txt"), b"delete")
        .await
        .unwrap();

    assert_eq!(
        maintenance::permanently_delete_file(&pool, &storage_dir, 1)
            .await
            .unwrap(),
        1
    );
    assert!(!storage_dir.join("delete.txt").exists());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM files WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    let mut entries = tokio::fs::read_dir(storage_dir.join(integrity::QUARANTINE_DIR_NAME))
        .await
        .unwrap();
    assert!(entries.next_entry().await.unwrap().is_some());
}

#[tokio::test]
async fn ユーザー更新日時は初期管理者の登録時に保存する() {
    let pool = test_pool().await;
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: PathBuf::from("./target/test_storage_user_updated_at"),
        host: "127.0.0.1".to_string(),
        port: 0,
        admin_password: None,
        secure_cookie: false,
    };
    initialize_db(&pool, &config).await.unwrap();

    let updated_at: Option<String> =
            sqlx::query_scalar("SELECT to_char(updated_at AT TIME ZONE 'Asia/Tokyo', 'YYYY-MM-DD HH24:MI:SS') FROM users WHERE username = 'admin'")
                .fetch_one(&pool)
                .await
                .unwrap();
    assert!(updated_at.is_some());
}

#[tokio::test]
async fn ログインユーザー情報を取得できる() {
    let pool = test_pool().await;
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: PathBuf::from("./target/test_storage_me"),
        host: "127.0.0.1".to_string(),
        port: 0,
        admin_password: None,
        secure_cookie: false,
    };
    initialize_db(&pool, &config).await.unwrap();

    let user: (i64, String, String) =
        sqlx::query_as("SELECT id, username, role FROM users WHERE username = 'admin'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(user.1, "admin");
    assert_eq!(user.2, "admin");
}

#[tokio::test]
async fn 案件と販売店にメモを追加および履歴が取得できる() {
    let pool = test_pool().await;
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: PathBuf::from("./target/test_storage_notes"),
        host: "127.0.0.1".to_string(),
        port: 0,
        admin_password: None,
        secure_cookie: false,
    };
    initialize_db(&pool, &config).await.unwrap();

    sqlx::query(
        "INSERT INTO projects (project_number, name, kana) VALUES ('P001', 'テスト案件', 'テスト')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO dealers (name) VALUES ('テスト販売店')")
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO project_notes (project_id, content, created_by) VALUES (1, 'テスト案件メモ', 'admin')")
            .execute(&pool)
            .await
            .unwrap();

    let p_notes: Vec<(i64, String, String, String)> = sqlx::query_as(
        "SELECT pn.id, pn.content, COALESCE(u.username, pn.created_by), to_char(pn.created_at AT TIME ZONE 'Asia/Tokyo', 'YYYY-MM-DD HH24:MI:SS') FROM project_notes pn LEFT JOIN users u ON u.id::text = pn.created_by OR u.username = pn.created_by WHERE pn.project_id = 1",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(p_notes.len(), 1);
    assert_eq!(p_notes[0].1, "テスト案件メモ");
    assert_eq!(p_notes[0].2, "admin");

    sqlx::query("INSERT INTO dealer_notes (dealer_id, content, created_by) VALUES (1, 'テスト販売店メモ', 'admin')")
            .execute(&pool)
            .await
            .unwrap();

    let d_notes: Vec<(i64, String, String, String)> = sqlx::query_as(
        "SELECT dn.id, dn.content, COALESCE(u.username, dn.created_by), to_char(dn.created_at AT TIME ZONE 'Asia/Tokyo', 'YYYY-MM-DD HH24:MI:SS') FROM dealer_notes dn LEFT JOIN users u ON u.id::text = dn.created_by OR u.username = dn.created_by WHERE dn.dealer_id = 1",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(d_notes.len(), 1);
    assert_eq!(d_notes[0].1, "テスト販売店メモ");
    assert_eq!(d_notes[0].2, "admin");
}

#[tokio::test]
async fn 案件メモの追加で案件更新日時を更新する() {
    let pool = test_pool().await;
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: PathBuf::from("./target/test_storage_note_updated_at"),
        host: "127.0.0.1".to_string(),
        port: 0,
        admin_password: None,
        secure_cookie: false,
    };
    initialize_db(&pool, &config).await.unwrap();
    sqlx::query(
        "INSERT INTO projects (project_number, name, kana, updated_at) VALUES ('P002', '更新日時案件', 'コウシン', '2000-01-01 00:00:00')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let token = "note-updated-at-session";
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES ($1, 1, CURRENT_TIMESTAMP + INTERVAL '1 hour')",
    )
    .bind(handlers::auth_handlers::hash_session_token(token))
    .execute(&pool)
    .await
    .unwrap();
    let state = Arc::new(AppState {
        pool: pool.clone(),
        storage_root: config.storage_dir.clone(),
        access_urls: Vec::new(),
        webauthn: Arc::new(
            WebauthnBuilder::new("localhost", &Url::parse("http://localhost:3000").unwrap())
                .unwrap()
                .build()
                .unwrap(),
        ),
        secure_cookie: false,
    });
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        format!("fm3_session={token}").parse().unwrap(),
    );
    let status = handlers::notes::create_project_note(
        axum::extract::State(state),
        axum::extract::Path(1),
        headers,
        axum::Json(models::CreateNoteRequest {
            content: "更新日時を確認".to_string(),
        }),
    )
    .await
    .unwrap();
    assert_eq!(status, axum::http::StatusCode::CREATED);
    let updated_at: String =
        sqlx::query_scalar("SELECT updated_at::text FROM projects WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!updated_at.starts_with("2000-01-01"));
}

#[tokio::test]
async fn 案件登録および更新時に未登録の販売店が自動追加される() {
    let _ = tracing_subscriber::fmt::try_init();
    let pool = test_pool().await;
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: PathBuf::from("./target/test_storage_autodealer"),
        host: "127.0.0.1".to_string(),
        port: 0,
        admin_password: None,
        secure_cookie: false,
    };
    initialize_db(&pool, &config).await.unwrap();

    let state = Arc::new(AppState {
        pool: pool.clone(),
        storage_root: config.storage_dir.clone(),
        access_urls: vec![
            "http://127.0.0.1:3000".to_string(),
            "http://192.168.11.21:3000".to_string(),
        ],
        webauthn: Arc::new(
            WebauthnBuilder::new("localhost", &Url::parse("http://localhost:3000").unwrap())
                .unwrap()
                .build()
                .unwrap(),
        ),
        secure_cookie: false,
    });

    let token = "test-session-token-autodealer";
    sqlx::query(
            "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES ($1, 1, CURRENT_TIMESTAMP + INTERVAL '1 hours')",
        )
        .bind(handlers::auth_handlers::hash_session_token(token))
        .execute(&pool)
        .await
        .unwrap();

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::COOKIE,
        format!("fm3_session={token}").parse().unwrap(),
    );

    let req = models::CreateProjectRequest {
        project_number: "９９９-Ａ".to_string(),
        name: "新規自動販売店案件".to_string(),
        kana: "シンキジドウハンバイテンアンケン".to_string(),
        address: None,
        dealer: Some("新規自動販売店".to_string()),
        assignee: None,
        phone: Some("０３－１２３４－５６７８".to_string()),
        email: Some(" project@example.com ".to_string()),
        latitude: None,
        longitude: None,
        plus_code: None,
    };

    let res =
        handlers::projects::create_project(axum::extract::State(state), headers, axum::Json(req))
            .await;
    assert!(res.is_ok());

    let dealer: Option<(String,)> = sqlx::query_as(
        "SELECT name FROM dealers WHERE name = '新規自動販売店' AND deleted_at IS NULL",
    )
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert!(dealer.is_some());
    let phone: Option<String> =
        sqlx::query_scalar("SELECT phone FROM projects WHERE project_number = '999'")
            .fetch_optional(&pool)
            .await
            .unwrap();
    assert_eq!(phone.as_deref(), Some("0312345678"));
    let email: Option<String> =
        sqlx::query_scalar("SELECT email FROM projects WHERE project_number = '999'")
            .fetch_optional(&pool)
            .await
            .unwrap();
    assert_eq!(email.as_deref(), Some("project@example.com"));
}

#[tokio::test]
async fn 販売店登録でカナを保存および更新できる() {
    let pool = test_pool().await;
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: PathBuf::from("./target/test_storage_dealer_kana"),
        host: "127.0.0.1".to_string(),
        port: 0,
        admin_password: None,
        secure_cookie: false,
    };
    initialize_db(&pool, &config).await.unwrap();

    let state = Arc::new(AppState {
        pool: pool.clone(),
        storage_root: config.storage_dir.clone(),
        access_urls: vec![
            "http://127.0.0.1:3000".to_string(),
            "http://192.168.11.21:3000".to_string(),
        ],
        webauthn: Arc::new(
            WebauthnBuilder::new("localhost", &Url::parse("http://localhost:3000").unwrap())
                .unwrap()
                .build()
                .unwrap(),
        ),
        secure_cookie: false,
    });

    let token = "test-session-token-dealer-kana";
    sqlx::query(
            "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES ($1, 1, CURRENT_TIMESTAMP + INTERVAL '1 hours')",
        )
        .bind(handlers::auth_handlers::hash_session_token(token))
        .execute(&pool)
        .await
        .unwrap();

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::COOKIE,
        format!("fm3_session={token}").parse().unwrap(),
    );

    let req = models::CreateDealerRequest {
        name: "テスト販売店カナ".to_string(),
        kana: Some("テストハンバイテンカナ".to_string()),
        address: Some("東京都".to_string()),
        phone: Some("０３－１２３４－５６７８".to_string()),
        fax: Some("０３－９８７６－５４３２".to_string()),
        email: Some(" dealer@example.com ".to_string()),
    };

    let res = handlers::dealers::create_dealer(
        axum::extract::State(state.clone()),
        headers.clone(),
        axum::Json(req),
    )
    .await;
    assert!(res.is_ok());
    let created = res.unwrap().0;
    assert_eq!(created.kana.as_deref(), Some("テストハンバイテンカナ"));
    assert_eq!(created.phone.as_deref(), Some("0312345678"));
    assert_eq!(created.fax.as_deref(), Some("0398765432"));
    assert_eq!(created.email.as_deref(), Some("dealer@example.com"));

    sqlx::query(
        "INSERT INTO projects (project_number, name, dealer) VALUES ('D001', '紐付け確認案件', $1)",
    )
    .bind(&created.name)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO dealer_contacts (dealer_name, name, phone) VALUES ($1, '担当者A', '03-0000-0000')")
        .bind(&created.name)
        .execute(&pool)
        .await
        .unwrap();

    let contact_create = handlers::dealers::create_dealer_contact(
        axum::extract::Path(created.name.clone()),
        axum::extract::State(state.clone()),
        headers.clone(),
        axum::Json(models::CreateDealerContactRequest {
            name: "担当者B".to_string(),
            phone: "０３－１１１１－２２２２".to_string(),
            email: Some(" contact@example.com ".to_string()),
        }),
    )
    .await;
    assert_eq!(contact_create.unwrap(), axum::http::StatusCode::CREATED);
    let contact_id: i64 = sqlx::query_scalar(
        "SELECT id FROM dealer_contacts WHERE dealer_name = $1 AND name = '担当者B'",
    )
    .bind(&created.name)
    .fetch_one(&pool)
    .await
    .unwrap();
    let contact_email: String =
        sqlx::query_scalar("SELECT email FROM dealer_contacts WHERE id = $1")
            .bind(contact_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(contact_email, "contact@example.com");

    let contact_update = handlers::dealers::update_dealer_contact(
        axum::extract::Path(contact_id),
        axum::extract::State(state.clone()),
        headers.clone(),
        axum::Json(models::CreateDealerContactRequest {
            name: "担当者B".to_string(),
            phone: "０６－３３３３－４４４４".to_string(),
            email: Some("updated@example.com".to_string()),
        }),
    )
    .await;
    assert_eq!(contact_update.unwrap(), axum::http::StatusCode::NO_CONTENT);
    let contacts = handlers::dealers::list_dealer_contacts(
        axum::extract::Path(created.name.clone()),
        axum::extract::State(state.clone()),
        headers.clone(),
    )
    .await
    .unwrap()
    .0;
    let contact = contacts
        .iter()
        .find(|contact| contact.name == "担当者B")
        .unwrap();
    assert_eq!(contact.phone, "0633334444");
    assert_eq!(contact.email, "updated@example.com");

    let update_req = models::CreateDealerRequest {
        name: "テスト販売店カナ".to_string(),
        kana: Some("テストハンバイテンカナコウシン".to_string()),
        address: Some("東京都港区".to_string()),
        phone: Some("０６－１２３４－５６７８".to_string()),
        fax: Some("０６－９８７６－５４３２".to_string()),
        email: Some("dealer-updated@example.com".to_string()),
    };

    let update_res = handlers::dealers::update_dealer(
        axum::extract::State(state),
        axum::extract::Path(created.id),
        headers,
        axum::Json(update_req),
    )
    .await;
    if let Err(e) = &update_res {
        println!("update_res error: {:?}", e);
    }
    assert!(update_res.is_ok());
    assert_eq!(
        update_res.unwrap().0.kana.as_deref(),
        Some("テストハンバイテンカナコウシン")
    );
    let dealer_email: String = sqlx::query_scalar("SELECT email FROM dealers WHERE id = $1")
        .bind(created.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(dealer_email, "dealer-updated@example.com");
    let project_dealer: String =
        sqlx::query_scalar("SELECT dealer FROM projects WHERE project_number = 'D001'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(project_dealer, "テスト販売店カナ");
    let contact_dealer: String =
        sqlx::query_scalar("SELECT dealer_name FROM dealer_contacts WHERE name = '担当者A'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(contact_dealer, "テスト販売店カナ");
}

#[test]
fn lanモバイル接続がデフォルトのホスト設定になる() {
    let prev = std::env::var("HOST").ok();
    std::env::remove_var("HOST");
    let config = AppConfig::from_env();
    assert_eq!(config.host, "0.0.0.0");
    if let Some(val) = prev {
        std::env::set_var("HOST", val);
    }
}

#[test]
#[allow(non_snake_case)]
fn パスキーの外部OriginとHTTPS設定を環境変数で構成できる() {
    let routes = include_str!("routes.rs");
    let startup = include_str!("main.rs");
    assert!(routes.contains("WEBAUTHN_RP_ID"));
    assert!(routes.contains("WEBAUTHN_ORIGIN"));
    assert!(startup.contains("TLS_CERT_PATH"));
    assert!(startup.contains("TLS_KEY_PATH"));
    assert!(startup.contains("bind_rustls"));
}

#[tokio::test]
async fn 案件検索結果画面を認証済みで表示できる() {
    let pool = test_pool().await;
    let config = AppConfig {
        database_url: admin_database_url(),
        storage_dir: PathBuf::from("./target/test_storage_projects_page"),
        host: "127.0.0.1".to_string(),
        port: 3000,
        admin_password: None,
        secure_cookie: false,
    };
    let app = create_app(pool, &config).await.unwrap();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/projects")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("ログイン"));
}

#[test]
fn 様々なカラータイプの画像を安全にwebpへ変換できる() {
    // 1. RGB画像 (PNG)
    let rgb_img = image::RgbImage::new(10, 10);
    let mut rgb_bytes = Vec::new();
    image::DynamicImage::ImageRgb8(rgb_img)
        .write_to(
            &mut std::io::Cursor::new(&mut rgb_bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    let (name, data) = encode_picture_to_webp("sample.png", &rgb_bytes);
    assert_eq!(name, "sample.webp");
    assert!(!data.is_empty());
    assert_eq!(&data[0..4], b"RIFF");
    assert_eq!(&data[8..12], b"WEBP");

    // 2. グレースケール画像 (Luma8) - 以前はこれが原因で初期化エラーになっていた
    let luma_img = image::GrayImage::new(10, 10);
    let mut luma_bytes = Vec::new();
    image::DynamicImage::ImageLuma8(luma_img)
        .write_to(
            &mut std::io::Cursor::new(&mut luma_bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    let (luma_name, luma_data) = encode_picture_to_webp("grayscale.jpg", &luma_bytes);
    assert_eq!(luma_name, "grayscale.webp");
    assert!(!luma_data.is_empty());
    assert_eq!(&luma_data[0..4], b"RIFF");
    assert_eq!(&luma_data[8..12], b"WEBP");

    // 3. 不正な画像データ（フォールバック）
    let invalid_bytes = b"not an image binary";
    let (fallback_name, fallback_data) = encode_picture_to_webp("bad.jpg", invalid_bytes);
    assert_eq!(fallback_name, "bad.jpg");
    assert_eq!(fallback_data, invalid_bytes);
}
