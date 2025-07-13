use crate::models::{RPCResponse, RpcEndpoint};
use chrono::Utc;
use rocksdb::DB;
use solana_client::rpc_client::RpcClient;
use std::sync::Arc;
use std::time::{Duration, Instant};
use once_cell::sync::Lazy;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;
use std::sync::atomic::{AtomicU64, Ordering};

// Connection statistics
static HTTP2_REQUESTS: AtomicU64 = AtomicU64::new(0);
static FALLBACK_REQUESTS: AtomicU64 = AtomicU64::new(0);

// Global HTTP/2 client
static HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    reqwest::ClientBuilder::new()
        .pool_idle_timeout(Duration::from_secs(300))
        .pool_max_idle_per_host(20)
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .tcp_keepalive(Duration::from_secs(30))
        .http2_keep_alive_interval(Duration::from_secs(30))
        .http2_keep_alive_timeout(Duration::from_secs(10))
        .http2_keep_alive_while_idle(true)
        .user_agent("solana-rpc-monitor/1.0")
        .build()
        .expect("Failed to create HTTP/2 client")
});

// HTTP/1.1 client fallback
static HTTP1_CLIENT: Lazy<Client> = Lazy::new(|| {
    reqwest::ClientBuilder::new()
        .http1_only()
        .pool_idle_timeout(Duration::from_secs(300))
        .pool_max_idle_per_host(20)
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .tcp_keepalive(Duration::from_secs(30))
        .user_agent("solana-rpc-monitor/1.0")
        .build()
        .expect("Failed to create HTTP/1.1 client")
});

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: String,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcResponse<T> {
    jsonrpc: String,
    id: String,
    result: Option<T>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

async fn rpc_call_with_precise_timing<T>(
    url: &str,
    method: &str,
    params: Option<Value>,
    prefer_http2: bool
) -> Result<(T, u128), String>
where
    T: for<'de> Deserialize<'de>,
{
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Uuid::new_v4().to_string(),
        method: method.to_string(),
        params,
    };

    let client = if prefer_http2 { &HTTP_CLIENT } else { &HTTP1_CLIENT };
    let request_body = serde_json::to_string(&request).map_err(|e| e.to_string())?;

    let precise_start = Instant::now();
    let response = client
        .post(url)
        .header("Content-Type", "application/json")
        .body(request_body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let precise_latency = precise_start.elapsed().as_millis();

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    let response_text = response.text().await.map_err(|e| e.to_string())?;
    let rpc_response: JsonRpcResponse<T> = serde_json::from_str(&response_text).map_err(|e| e.to_string())?;

    if let Some(error) = rpc_response.error {
        return Err(format!("RPC error {}: {}", error.code, error.message));
    }

    let result = rpc_response.result.ok_or_else(|| "Missing result in RPC response".to_string())?;
    Ok((result, precise_latency))
}

async fn get_single_request_timing(url: &str, prefer_http2: bool) -> Result<u128, String> {
    let (_result, timing): (Value, u128) = rpc_call_with_precise_timing(
        url,
        "getHealth",
        None,
        prefer_http2,
    ).await?;
    Ok(timing)
}

async fn get_latest_blockhash_http2(url: &str, prefer_http2: bool) -> Result<(String, u128), String> {
    #[derive(Deserialize)]
    struct BlockhashResponse {
        value: BlockhashValue,
    }

    #[derive(Deserialize)]
    struct BlockhashValue {
        blockhash: String,
    }

    let (response, network_latency): (BlockhashResponse, u128) = rpc_call_with_precise_timing(
        url,
        "getLatestBlockhash",
        Some(json!([{"commitment": "finalized"}])),
        prefer_http2,
    ).await?;

    Ok((response.value.blockhash, network_latency))
}

async fn get_slot_http2(url: &str, prefer_http2: bool) -> Result<(u64, u128), String> {
    let (slot, network_latency): (u64, u128) = rpc_call_with_precise_timing(
        url,
        "getSlot",
        Some(json!([{"commitment": "finalized"}])),
        prefer_http2,
    ).await?;
    Ok((slot, network_latency))
}

async fn fetch_both_http2(url: &str, prefer_http2: bool) -> Result<(String, u64, u128), String> {
    let (blockhash_result, slot_result) = tokio::join!(
        get_latest_blockhash_http2(url, prefer_http2),
        get_slot_http2(url, prefer_http2)
    );

    let (blockhash, blockhash_latency) = blockhash_result?;
    let (slot, slot_latency) = slot_result?;
    let effective_latency = std::cmp::max(blockhash_latency, slot_latency); // Worst-case latency

    Ok((blockhash, slot, effective_latency))
}

pub async fn fetch_blockhash_and_slot(
    endpoint: RpcEndpoint,
    db: Arc<DB>,
) -> Result<(), Box<dyn std::error::Error>> {
    let start_time = Instant::now();

    let (blockhash, slot, used_legacy) = match fetch_both_http2(&endpoint.url, true).await {
        Ok((hash, slot_num, _)) => {
            HTTP2_REQUESTS.fetch_add(1, Ordering::Relaxed);
            (hash, slot_num, false)
        }
        Err(e) => {
            eprintln!("[{}] HTTP/2 failed: {}", endpoint.nickname, e);
            match fetch_both_http2(&endpoint.url, false).await {
                Ok((hash, slot_num, _)) => {
                    FALLBACK_REQUESTS.fetch_add(1, Ordering::Relaxed);
                    (hash, slot_num, false)
                }
                Err(_) => {
                    let client = RpcClient::new(endpoint.url.clone());
                    let blockhash = client.get_latest_blockhash().map(|h| h.to_string()).unwrap_or("Unavailable".to_string());
                    let slot = client.get_slot().unwrap_or(0);
                    (blockhash, slot, true)
                }
            }
        }
    };

    let precise_latency = if used_legacy {
        match get_single_request_timing(&endpoint.url, false).await {
            Ok(timing) => timing,
            Err(_) => 999,
        }
    } else {
        match get_single_request_timing(&endpoint.url, true).await {
            Ok(timing) => timing,
            Err(_) => match get_single_request_timing(&endpoint.url, false).await {
                Ok(timing) => timing,
                Err(_) => 999,
            },
        }
    };

    let total_requests = HTTP2_REQUESTS.load(Ordering::Relaxed) + FALLBACK_REQUESTS.load(Ordering::Relaxed);
    if total_requests % 50 == 0 {
        let http2_ratio = (HTTP2_REQUESTS.load(Ordering::Relaxed) * 100) / total_requests;
        println!(
            "[{}] [{}] Protocol stats: {}% HTTP/2, {}% HTTP/1.1+Legacy ({} total)",
            Utc::now().to_rfc3339(),
            endpoint.nickname,
            http2_ratio,
            100 - http2_ratio,
            total_requests
        );
    }

    let total_latency = start_time.elapsed().as_millis();
    let request_start = Utc::now();

    let response = RPCResponse {
        timestamp: request_start.timestamp_millis(),
        slot,
        blockhash: blockhash.clone(),
        latency_ms: precise_latency,
        total_latency_ms: total_latency,
        rpc_url: endpoint.url.clone(),
        nickname: endpoint.nickname.clone(),
    };

    if response.slot == 0 || response.blockhash == "Unavailable" {
        eprintln!(
            "[{}] Skipping invalid response (slot: {}, blockhash: {})",
            endpoint.nickname, response.slot, response.blockhash
        );
        return Ok(());
    }

    let key = format!("{}:{}", endpoint.nickname, Utc::now().timestamp());
    let value = serde_json::to_string(&response)?;
    db.put(key.as_bytes(), value.as_bytes())?;

    println!(
        "[{}] [{}] Slot: {}, Blockhash: {} (precise={}ms, total={}ms)",
        Utc::now().to_rfc3339(),
        endpoint.nickname,
        slot,
        blockhash,
        precise_latency,
        total_latency
    );

    Ok(())
}
