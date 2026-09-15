use crate::app_state::prepare_database;
use crate::config::AppConfig;
use crate::handlers;
use crate::models::AppState;
use crate::specifications;
use crate::templates;
use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, post, put},
    Router,
};
use sqlx::sqlite::SqlitePool;
use std::env;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use webauthn_rs::prelude::*;

/// DB初期化、共有状態の構築、HTTPルート登録をまとめて行う。
///
/// 起動処理は `main.rs` に残し、HTTPの構成はこのモジュールに集約することで、
/// ルート追加時にTLSやプロセス起動の処理へ影響を与えないようにする。
pub async fn create_app(pool: SqlitePool, config: &AppConfig) -> anyhow::Result<Router> {
    prepare_database(&pool, config).await?;
    let webauthn = build_webauthn(config)?;
    let state = Arc::new(AppState {
        pool,
        storage_root: config.storage_dir.clone(),
        access_urls: crate::network::webui_urls(&config.host, config.port, config.secure_cookie),
        webauthn,
        secure_cookie: config.secure_cookie,
    });

    Ok(register_routes(state))
}

fn build_webauthn(config: &AppConfig) -> anyhow::Result<Arc<Webauthn>> {
    let default_rp_id = (config.host == "0.0.0.0")
        .then(crate::network::local_host_name)
        .flatten();
    let default_rp_id = default_rp_id.as_deref().unwrap_or({
        if matches!(config.host.as_str(), "0.0.0.0" | "127.0.0.1" | "::1") {
            "localhost"
        } else {
            config.host.as_str()
        }
    });
    let rp_id = env::var("WEBAUTHN_RP_ID")
        .ok()
        .or_else(|| config.setting("WEBAUTHN_RP_ID"))
        .unwrap_or_else(|| default_rp_id.to_string());
    let origin = env::var("WEBAUTHN_ORIGIN")
        .ok()
        .or_else(|| config.setting("WEBAUTHN_ORIGIN"))
        .unwrap_or_else(|| format!("https://{}:{}", rp_id, config.port));
    let origin_url = Url::parse(&origin)?;
    let webauthn = WebauthnBuilder::new(&rp_id, &origin_url)?
        .rp_name("FileManager3")
        .build()?;
    Ok(Arc::new(webauthn))
}

fn register_routes(state: Arc<AppState>) -> Router {
    Router::new()
        // 画面と認証
        .route("/", get(handlers::auth_handlers::index_page))
        .route(
            "/login",
            get(templates::login_page).post(handlers::auth_handlers::login),
        )
        .route(
            "/auth/google",
            get(handlers::auth_handlers::google_login_start),
        )
        .route(
            "/auth/google/callback",
            get(handlers::auth_handlers::google_login_callback),
        )
        .route("/logout", get(handlers::auth_handlers::logout))
        .route(
            "/api/access-urls",
            get(handlers::auth_handlers::access_urls),
        )
        .route(
            "/api/login-usernames",
            get(handlers::users::list_local_login_usernames),
        )
        .route("/api/android/latest", get(handlers::android::latest))
        .route(
            "/android/filemanager3-android-release.apk",
            get(handlers::android::release_apk),
        )
        .route(
            "/api/logout-on-close",
            post(handlers::auth_handlers::logout_on_close),
        )
        .route(
            "/api/session/keepalive",
            post(handlers::auth_handlers::keepalive),
        )
        .route("/help", get(templates::help_page))
        .route("/projects", get(handlers::auth_handlers::projects_page))
        .route("/projects/:id", get(templates::detail_page_handler))
        .route("/admin", get(templates::admin_page))
        .route("/admin/specifications", get(specifications::index_page))
        .route(
            "/admin/specifications/:slug",
            get(specifications::detail_page),
        )
        .route("/admin/projects", get(templates::admin_projects_page))
        .route("/admin/users", get(templates::user_registration_page))
        .route("/admin/dealers", get(templates::dealer_registration_page))
        // 販売店・担当者・メモ
        .route("/api/me", get(handlers::auth_handlers::me))
        .route(
            "/api/dealers",
            get(handlers::dealers::list_dealers).post(handlers::dealers::create_dealer),
        )
        .route(
            "/api/dealers/:id",
            put(handlers::dealers::update_dealer).delete(handlers::dealers::delete_dealer),
        )
        .route(
            "/api/dealers/:name/contacts",
            get(handlers::dealers::list_dealer_contacts)
                .post(handlers::dealers::create_dealer_contact),
        )
        .route(
            "/api/contacts/:id",
            put(handlers::dealers::update_dealer_contact)
                .delete(handlers::dealers::delete_dealer_contact),
        )
        .route(
            "/api/deleted/contacts/:id",
            delete(handlers::dealers::permanently_delete_dealer_contact),
        )
        .route(
            "/api/dealers/:id/restore",
            post(handlers::dealers::restore_dealer),
        )
        .route(
            "/api/dealers/:id/notes",
            get(handlers::notes::list_dealer_notes).post(handlers::notes::create_dealer_note),
        )
        .route(
            "/api/dealers/:id/notes/deleted",
            get(handlers::notes::list_deleted_dealer_notes),
        )
        .route(
            "/api/dealers/:id/notes/:note_id",
            delete(handlers::notes::delete_dealer_note),
        )
        .route(
            "/api/dealers/:id/notes/:note_id/restore",
            post(handlers::notes::restore_dealer_note),
        )
        .route(
            "/api/dealers/:id/notes/:note_id/permanent",
            delete(handlers::notes::permanently_delete_dealer_note),
        )
        // 案件・案件メモ・権限
        .route(
            "/api/deleted/projects",
            get(handlers::projects::list_deleted_projects),
        )
        .route(
            "/api/deleted/dealers",
            get(handlers::dealers::list_deleted_dealers),
        )
        .route(
            "/api/deleted/dealers/:id",
            delete(handlers::dealers::permanently_delete_dealer),
        )
        .route(
            "/api/projects",
            get(handlers::projects::search_projects).post(handlers::projects::create_project),
        )
        .route(
            "/api/projects/:id",
            get(handlers::projects::get_project)
                .put(handlers::projects::update_project)
                .delete(handlers::projects::delete_project),
        )
        .route(
            "/api/projects/:id/restore",
            post(handlers::projects::restore_project),
        )
        .route(
            "/api/deleted/projects/:id",
            delete(handlers::projects::permanently_delete_project),
        )
        .route(
            "/api/projects/:id/notes",
            get(handlers::notes::list_project_notes).post(handlers::notes::create_project_note),
        )
        .route(
            "/api/projects/:id/notes/deleted",
            get(handlers::notes::list_deleted_project_notes),
        )
        .route(
            "/api/projects/:id/notes/:note_id",
            delete(handlers::notes::delete_project_note),
        )
        .route(
            "/api/projects/:id/notes/:note_id/restore",
            post(handlers::notes::restore_project_note),
        )
        .route(
            "/api/projects/:id/notes/:note_id/permanent",
            delete(handlers::notes::permanently_delete_project_note),
        )
        .route(
            "/api/projects/:id/permissions",
            post(handlers::users::grant_project_permission),
        )
        // ファイルと版履歴
        .route(
            "/api/projects/:id/files",
            post(handlers::files::upload_file),
        )
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .route("/api/files/search", get(handlers::files::search_files))
        .route(
            "/api/files/batch-delete",
            post(handlers::files::batch_delete_files),
        )
        .route(
            "/api/files/batch-move",
            post(handlers::files::batch_move_files),
        )
        .route(
            "/api/files/batch-download",
            get(handlers::files::batch_download_files),
        )
        .route(
            "/api/files/batch-print",
            get(handlers::files::batch_print_files),
        )
        .route(
            "/api/files/:id/download",
            get(handlers::files::download_file),
        )
        .route("/api/files/:id/open", get(handlers::files::open_file))
        .route(
            "/api/files/:id/history",
            get(handlers::files::list_file_history),
        )
        .route(
            "/api/files/:id/audit",
            get(handlers::files::list_file_audit),
        )
        .route(
            "/api/file-histories/:id/download",
            get(handlers::files::download_file_history),
        )
        .route("/api/files/:id/tag", put(handlers::files::update_file_tag))
        .route("/api/files/:id", delete(handlers::files::delete_file))
        .route(
            "/api/deleted/files/:id",
            delete(handlers::files::permanently_delete_file),
        )
        .route(
            "/api/files/:id/thumbnail",
            get(handlers::files::file_thumbnail),
        )
        // ユーザー・パスキー
        .route(
            "/api/users",
            get(handlers::users::list_users).post(handlers::users::create_user),
        )
        .route(
            "/api/deleted/users",
            get(handlers::users::list_deleted_users),
        )
        .route(
            "/api/users/:id",
            put(handlers::users::update_user).delete(handlers::users::delete_user),
        )
        .route(
            "/api/deleted/users/:id",
            delete(handlers::users::permanently_delete_user),
        )
        .route(
            "/api/users/:id/passkeys",
            delete(handlers::auth_handlers::delete_user_passkeys),
        )
        .route(
            "/api/users/:id/passkeys/register/start",
            post(handlers::auth_handlers::user_passkey_register_start),
        )
        .route(
            "/api/passkeys/register/finish",
            post(handlers::auth_handlers::passkey_register_finish),
        )
        .route(
            "/api/passkeys/login/start",
            post(handlers::auth_handlers::passkey_login_start),
        )
        .route(
            "/api/passkeys/login/finish",
            post(handlers::auth_handlers::passkey_login_finish),
        )
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static(
                "no-store, no-cache, must-revalidate, proxy-revalidate, max-age=0",
            ),
        ))
        .layer(tower_http::compression::CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
