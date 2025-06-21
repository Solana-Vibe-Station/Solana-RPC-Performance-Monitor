mod config;
mod metrics;
mod models;
mod rpc;

use axum::{
    response::Redirect,
    routing::{get, get_service},
    Router,
};
use chrono::{Duration, Utc};
use clap::Parser;
use futures::future::join_all;
use rocksdb::{Options, DB};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::task;
use tower_http::services::ServeDir;

use crate::config::load_config;
use crate::config::AppConfig;
use crate::metrics::get_metrics;
use crate::models::RPCResponse;
use crate::rpc::fetch_blockhash_and_slot;

/// CLI arguments
#[derive(Parser)]
#[command(name = "SVS RPC Monitor", about = "Solana RPC Performance Monitor")]
struct Cli {
    /// IP address to bind the server to
    #[arg(long)]
    listen_ip: Option<String>,

    /// Port to bind the server to
    #[arg(long)]
    port: Option<u16>,
}

fn setup_db() -> Arc<DB> {
    let mut opts = Options::default();
    opts.create_if_missing(true);
    opts.set_write_buffer_size(64 * 1024 * 1024);
    opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
    Arc::new(DB::open(&opts, "rpc_metrics.db").expect("Failed to open database"))
}

async fn cleanup_old_entries(db: Arc<DB>) -> Result<(), Box<dyn std::error::Error>> {
    let one_hour_ago_ts = (Utc::now() - Duration::hours(1)).timestamp();
    let mut batch = rocksdb::WriteBatch::default();

    for item in db.iterator(rocksdb::IteratorMode::Start) {
        if let Ok((key, value)) = item {
            if let Ok(response) = serde_json::from_slice::<RPCResponse>(&value) {
                if response.timestamp < one_hour_ago_ts as f64 {
                    batch.delete(key);
                }
            }
        }
    }

    db.write(batch)?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();
    let db = setup_db();
    let mut config: AppConfig = load_config()?;

    // ✅ Override TOML config with CLI arguments
    if let Some(ip) = args.listen_ip {
        config.server.listen_ip = Some(ip);
    }
    if let Some(port) = args.port {
        config.server.port = Some(port);
    }

    std::fs::create_dir_all("static")?;
    std::fs::write("static/index.html", include_str!("static/index.html"))?;
    std::fs::write("static/dashboard.js", include_str!("static/dashboard.js"))?;
    std::fs::write("static/darkMode.js", include_str!("static/darkMode.js"))?;
    std::fs::write("static/styles.css", include_str!("static/styles.css"))?;
    std::fs::write("static/logo.svg", include_str!("static/logo.svg"))?;

    let db_clone = Arc::clone(&db);
    let endpoints = config.rpc.endpoints.clone();
    tokio::spawn(async move {
        loop {
            let tasks: Vec<_> = endpoints
                .clone()
                .into_iter()
                .map(|endpoint| {
                    let db = Arc::clone(&db_clone);
                    task::spawn(async move {
                        if let Err(e) = fetch_blockhash_and_slot(endpoint, db).await {
                            eprintln!("Error: {}", e);
                        }
                    })
                })
                .collect();

            join_all(tasks).await;
            tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
        }
    });

    let db_clone = Arc::clone(&db);
    tokio::spawn(async move {
        loop {
            if let Err(e) = cleanup_old_entries(db_clone.clone()).await {
                eprintln!("Error cleaning up old entries: {}", e);
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        }
    });

    let app = Router::new()
        .route("/", get(|| async { Redirect::to("/static/index.html") }))
        .route("/api/metrics", get(get_metrics))
        .nest_service("/static", get_service(ServeDir::new("static")))
        .with_state(db);

    let ip = config
        .server
        .listen_ip
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let port = config.server.port.unwrap_or(3000);
    let addr_str = format!("{}:{}", ip, port);

    let addr: SocketAddr = addr_str.parse()?;
    println!("🚀 Server running on http://{}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
