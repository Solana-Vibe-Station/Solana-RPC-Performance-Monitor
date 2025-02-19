use axum::{
    routing::{get, get_service},
    Router, Json, extract::Query, extract::State,
};
use rocksdb::{DB, Options};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::collections::HashMap;
use tower_http::services::ServeDir;
use chrono::{Utc, Duration};
use std::net::SocketAddr;
use tokio::task;
use futures::future::join_all;
use std::time::Instant;
use solana_client::rpc_client::RpcClient;
use std::fs;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct RPCResponse {
    timestamp: f64,
    slot: u64,
    blockhash: String,
    latency_ms: u128,
    rpc_url: String,
    nickname: String,
}

#[derive(Debug, Deserialize)]
struct Config {
    rpc: RpcConfig,
}

#[derive(Debug, Deserialize)]
struct RpcConfig {
    endpoints: Vec<RpcEndpoint>,
}

#[derive(Debug, Deserialize, Clone)]
struct RpcEndpoint {
    url: String,
    nickname: String,
}

#[derive(Debug, Serialize)]
struct LeaderboardEntry {
    nickname: String,
    value: u64,
    latency_ms: u128,
    timestamp: f64,
}

#[derive(Debug, Serialize)]
struct ConsensusStats {
    fastest_rpc: String,
    slowest_rpc: String,
    fastest_latency: u128,
    slowest_latency: u128,
    consensus_blockhash: String,
    consensus_slot: u64,
    consensus_percentage: f64,
    total_rpcs: usize,
    average_latency: f64,
    slot_difference: i64,
    slot_skew: String,
    latency_leaderboard: Vec<LeaderboardEntry>,
    slot_leaderboard: Vec<LeaderboardEntry>,
}

fn setup_db() -> Arc<DB> {
    let mut opts = Options::default();
    opts.create_if_missing(true);
    opts.set_write_buffer_size(64 * 1024 * 1024);
    opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
    
    Arc::new(DB::open(&opts, "rpc_metrics.db").expect("Failed to open database"))
}

fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
    let config_str = fs::read_to_string("config.toml")?;
    let config: Config = toml::from_str(&config_str)?;
    Ok(config)
}

async fn fetch_blockhash_and_slot(endpoint: RpcEndpoint, db: Arc<DB>) -> Result<(), Box<dyn std::error::Error>> {
    println!("Querying {}: {}", endpoint.nickname, endpoint.url);
    let client = RpcClient::new(endpoint.url.clone());
    let start_time = Instant::now();
    
    let blockhash = match client.get_latest_blockhash() {
        Ok(hash) => hash.to_string(),
        Err(err) => format!("Error: {}", err),
    };
    
    let slot = match client.get_slot() {
        Ok(slot) => slot,
        Err(err) => {
            println!("Error fetching slot from {}: {}", endpoint.url, err);
            0
        }
    };
    
    let latency = start_time.elapsed().as_millis();
    
    let response = RPCResponse {
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64(),
        slot,
        blockhash: blockhash.clone(),
        latency_ms: latency,
        rpc_url: endpoint.url.clone(),
        nickname: endpoint.nickname.clone(),
    };
    
    let key = format!("{}:{}", endpoint.url, Utc::now().timestamp());
    let value = serde_json::to_string(&response)?;
    db.put(key.as_bytes(), value.as_bytes())?;
    
    println!("[{}] Slot: {}, Blockhash: {} ({}ms)", 
        endpoint.nickname, slot, blockhash, latency);
    
    Ok(())
}

fn calculate_consensus(responses: &[RPCResponse]) -> ConsensusStats {
    if responses.is_empty() {
        return ConsensusStats {
            fastest_rpc: String::from("No data"),
            slowest_rpc: String::from("No data"),
            fastest_latency: 0,
            slowest_latency: 0,
            consensus_blockhash: String::from("No data"),
            consensus_slot: 0,
            consensus_percentage: 0.0,
            total_rpcs: 0,
            average_latency: 0.0,
            slot_difference: 0,
            slot_skew: String::from("No data"),
            latency_leaderboard: Vec::new(),
            slot_leaderboard: Vec::new(),
        };
    }
    let mut blockhash_counts: HashMap<String, usize> = HashMap::new();
    let mut slot_counts: HashMap<u64, usize> = HashMap::new();
    let total_rpcs = responses.len();

    // Count occurrences of each blockhash and slot
    for response in responses {
        *blockhash_counts.entry(response.blockhash.clone()).or_insert(0) += 1;
        *slot_counts.entry(response.slot).or_insert(0) += 1;
    }

    // Find consensus values
    let consensus_blockhash = blockhash_counts
        .iter()
        .max_by_key(|&(_, count)| count)
        .map(|(hash, count)| (hash.clone(), *count))
        .unwrap_or((String::from("No consensus"), 0));

    let consensus_slot = slot_counts
        .iter()
        .max_by_key(|&(_, count)| count)
        .map(|(&slot, _)| slot)
        .unwrap_or(0);

    // Calculate consensus percentage
    let consensus_percentage = (consensus_blockhash.1 as f64 / total_rpcs as f64) * 100.0;

    // Find fastest and slowest RPCs with their latencies
    let fastest = responses
        .iter()
        .min_by_key(|r| r.latency_ms)
        .unwrap();

    let slowest = responses
        .iter()
        .max_by_key(|r| r.latency_ms)
        .unwrap();

    // Calculate slot differences and skew
    let slot_difference = fastest.slot as i64 - slowest.slot as i64;
    let slot_skew = if slot_difference == 0 {
        "No skew".to_string()
    } else if slot_difference > 0 {
        format!("Fastest ahead by {} slots", slot_difference.abs())
    } else {
        format!("Slowest ahead by {} slots", slot_difference.abs())
    };

    // Calculate average latency
    let average_latency = responses
        .iter()
        .map(|r| r.latency_ms as f64)
        .sum::<f64>() / total_rpcs as f64;

    // Create leaderboards
    let mut latency_leaderboard: Vec<LeaderboardEntry> = responses.iter()
        .map(|r| LeaderboardEntry {
            nickname: r.nickname.clone(),
            value: r.latency_ms as u64,
            latency_ms: r.latency_ms,
            timestamp: r.timestamp,
        })
        .collect();
    latency_leaderboard.sort_by_key(|entry| entry.value);
    latency_leaderboard.truncate(4); // Keep top 4

    let mut slot_leaderboard: Vec<LeaderboardEntry> = responses.iter()
        .map(|r| LeaderboardEntry {
            nickname: r.nickname.clone(),
            value: r.slot,
            latency_ms: r.latency_ms,
            timestamp: r.timestamp,
        })
        .collect();
    slot_leaderboard.sort_by(|a, b| b.value.cmp(&a.value));
    slot_leaderboard.truncate(4); // Keep top 4

    ConsensusStats {
        fastest_rpc: fastest.nickname.clone(),
        slowest_rpc: slowest.nickname.clone(),
        fastest_latency: fastest.latency_ms,
        slowest_latency: slowest.latency_ms,
        consensus_blockhash: consensus_blockhash.0,
        consensus_slot,
        consensus_percentage,
        total_rpcs,
        average_latency,
        slot_difference,
        slot_skew,
        latency_leaderboard,
        slot_leaderboard,
    }
}

async fn get_metrics(
    State(db): State<Arc<DB>>,
    Query(params): Query<HashMap<String, String>>
) -> Json<(Vec<RPCResponse>, ConsensusStats)> {
    let mut responses = Vec::new();
    
    let rpc_filter = params.get("rpc");
    let from_ts = params.get("from")
        .and_then(|ts| ts.parse::<i64>().ok());
    let to_ts = params.get("to")
        .and_then(|ts| ts.parse::<i64>().ok());
    
    // Get most recent response for each RPC
    let mut latest_by_rpc: HashMap<String, RPCResponse> = HashMap::new();
    
    let iter = db.iterator(rocksdb::IteratorMode::End);
    for item in iter {
        if let Ok((key, value)) = item {
            let key_str = String::from_utf8_lossy(&key);
            if let Ok(response) = serde_json::from_slice::<RPCResponse>(&value) {
                if !latest_by_rpc.contains_key(&response.rpc_url) {
                    latest_by_rpc.insert(response.rpc_url.clone(), response.clone());
                }
                
                if let Some((url, _)) = key_str.split_once(':') {
                    let matches_rpc = rpc_filter
                        .as_ref()
                        .map_or(true, |filter| url.contains(filter.as_str()));
                    let matches_time = match (from_ts, to_ts) {
                        (Some(from), Some(to)) => response.timestamp >= from as f64 && response.timestamp <= to as f64,
                        (Some(from), None) => response.timestamp >= from as f64,
                        (None, Some(to)) => response.timestamp <= to as f64,
                        (None, None) => true,
                    };
                    
                    if matches_rpc && matches_time {
                        responses.push(response);
                    }
                }
            }
        }
    }
    
    responses.sort_by(|a, b| b.timestamp.partial_cmp(&a.timestamp).unwrap_or(std::cmp::Ordering::Equal));
    
    // Calculate consensus based on latest response from each RPC
    let consensus_stats = calculate_consensus(&latest_by_rpc.values().cloned().collect::<Vec<_>>());
    
    // Remove sensitive rpc_url before returning the data
    let public_responses: Vec<RPCResponse> = responses
        .into_iter()
        .map(|mut r| {
            r.rpc_url = String::new();
            r
        })
        .collect();
    
    Json((public_responses, consensus_stats))
}

async fn cleanup_old_entries(db: Arc<DB>) -> Result<(), Box<dyn std::error::Error>> {
    let one_hour_ago = Utc::now() - Duration::hours(1);
    let one_hour_ago_ts = one_hour_ago.timestamp();

    let mut batch = rocksdb::WriteBatch::default();
    let iter = db.iterator(rocksdb::IteratorMode::Start);
    for item in iter {
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
    
    let config = match load_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Failed to load config: {}", e);
            return Ok(());
        }
    };

    std::fs::create_dir_all("static")?;
    std::fs::write(
        "static/index.html",
        include_str!("static/index.html")
    )?;

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
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await; // Run cleanup every minute
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
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
