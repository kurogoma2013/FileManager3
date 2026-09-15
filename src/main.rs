mod app_state;
mod auth;
mod config;
mod content;
mod core;
mod db;
mod handlers;
mod integrity;
mod maintenance;
mod models;
mod network;
mod routes;
mod specifications;
mod storage;
mod templates;

use app_state::clear_storage_for_new_database;
#[cfg(test)]
use app_state::prepare_database as initialize_db;
use config::AppConfig;
#[cfg(test)]
use models::AppState;
use network::{local_lan_ip, webui_urls};
use routes::create_app;
use std::env;
use std::net::SocketAddr;
#[cfg(test)]
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
#[cfg(test)]
use webauthn_rs::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = AppConfig::from_env();
    if env::args().nth(1).as_deref() == Some("check-integrity") {
        let pool = db::init_db(&config.database_url).await?;
        let report = integrity::check_integrity(&pool, &config.storage_dir).await?;
        integrity::print_report(&report);
        if !report.is_clean() {
            return Err(anyhow::anyhow!("ファイルとDBの整合性に異常があります"));
        }
        return Ok(());
    }
    config.validate_transport()?;
    prepare_runtime_directories(&config).await?;

    let pool = db::init_db(&config.database_url).await?;
    let app = create_app(pool, &config).await?;
    print_access_urls(&config);
    serve_https(app, &config).await
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "filemanager3=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}

async fn prepare_runtime_directories(config: &AppConfig) -> anyhow::Result<()> {
    clear_storage_for_new_database(&config.database_url, &config.storage_dir).await?;
    tokio::fs::create_dir_all(&config.storage_dir).await?;
    Ok(())
}

fn print_access_urls(config: &AppConfig) {
    println!("FileManager3 WebUI:");
    if config.host == "0.0.0.0" {
        println!("  [ローカル PC用]   https://127.0.0.1:{}", config.port);
        if let Some(ip) = local_lan_ip() {
            println!("  [LAN・スマホ用]   https://{ip}:{}", config.port);
        }
    } else {
        for url in webui_urls(&config.host, config.port, config.secure_cookie) {
            println!("  {url}");
        }
    }
}

async fn serve_https(app: axum::Router, config: &AppConfig) -> anyhow::Result<()> {
    let listener: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    let cert_path = env::var("TLS_CERT_PATH")
        .ok()
        .or_else(|| config.setting("TLS_CERT_PATH"))
        .ok_or_else(|| anyhow::anyhow!("HTTPSにはTLS_CERT_PATHが必要です"))?;
    let key_path = env::var("TLS_KEY_PATH")
        .ok()
        .or_else(|| config.setting("TLS_KEY_PATH"))
        .ok_or_else(|| anyhow::anyhow!("HTTPSにはTLS_KEY_PATHが必要です"))?;
    let tls_config =
        axum_server::tls_rustls::RustlsConfig::from_pem_file(cert_path, key_path).await?;
    axum_server::bind_rustls(listener, tls_config)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
