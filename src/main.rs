mod models;
mod metrics;
mod config;
mod rpc;

use axum::{
    routing::{get, get_service},
    Router,
};
use rocksdb::{DB, Options};
use std::sync::Arc;
use std::net::SocketAddr;
use chrono::{Utc, Duration};
use tokio::task;
use futures::future::join_all;
use tower_http::services::ServeDir;

use crate::metrics::get_metrics;
use crate::models::RPCResponse;
use crate::config::load_config;
use crate::rpc::fetch_blockhash_and_slot;

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
    let db = setup_db();
    let config = load_config()?;

    std::fs::create_dir_all("static")?;
    std::fs::write("static/index.html", include_str!("static/index.html"))?;

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
        .route("/", get(|| async {
            axum::response::Redirect::to("/static/index.html")
        }))
        .route("/api/metrics", get(get_metrics))
        .nest_service("/static", get_service(ServeDir::new("static")))
        .with_state(db);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Server running on http://localhost:3000");
    axum::Server::bind(&addr).serve(app.into_make_service()).await?;

    Ok(())
}
