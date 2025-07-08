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

// Global HTTP/2 client with connection pooling
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
        .expect("Failed to create HTTP client")
});

// HTTP/1.1 only client for comparison
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

async fn rpc_call_with_precise_timing<T>(url: &str, method: &str, params: Option<Value>, prefer_http2: bool) -> Result<(T, u128), String>
where
    T: for<'de> Deserialize<'de>,
{
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Uuid::new_v4().to_string(),
        method: method.to_string(),
        params,
    };

    let client = if prefer_http2 {
        &HTTP_CLIENT
    } else {
        &HTTP1_CLIENT
    };

    // Pre-serialize to avoid timing serialization overhead
    let request_body = serde_json::to_string(&request).map_err(|e| e.to_string())?;

    // Measure ONLY the network round trip (like OpenResty does)
    let precise_start = Instant::now();
    let response = client
        .post(url)
        .header("Content-Type", "application/json")
        .body(request_body)  // Use pre-serialized body
        .send()
        .await
        .map_err(|e| e.to_string())?;
    
    // Stop timing immediately after response received
    let precise_latency = precise_start.elapsed().as_millis();

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    // Parse JSON outside of timing measurement
    let response_text = response.text().await.map_err(|e| e.to_string())?;
    let rpc_response: JsonRpcResponse<T> = serde_json::from_str(&response_text).map_err(|e| e.to_string())?;

    if let Some(error) = rpc_response.error {
        return Err(format!("RPC error {}: {}", error.code, error.message));
    }

    let result = rpc_response
        .result
        .ok_or_else(|| "Missing result in RPC response".to_string())?;

    Ok((result, precise_latency))
}

// Version that makes individual timed requests instead of concurrent
async fn get_single_request_timing(url: &str, prefer_http2: bool) -> Result<u128, String> {
    // Just measure a single getHealth call to get pure network timing
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
    )
    .await?;

    Ok((response.value.blockhash, network_latency))
}

async fn get_slot_http2(url: &str, prefer_http2: bool) -> Result<(u64, u128), String> {
    let (slot, network_latency): (u64, u128) = rpc_call_with_precise_timing(
        url,
        "getSlot",
        Some(json!([{"commitment": "finalized"}])),
        prefer_http2,
    )
    .await?;

    Ok((slot, network_latency))
}

async fn fetch_both_http2(url: &str, prefer_http2: bool) -> Result<(String, u64, u128), String> {
    // Make both requests concurrently using the same connection pool
    let (blockhash_result, slot_result) = tokio::join!(
        get_latest_blockhash_http2(url, prefer_http2),
        get_slot_http2(url, prefer_http2)
    );

    let (blockhash, blockhash_latency) = blockhash_result?;
    let (slot, slot_latency) = slot_result?;

    // Since requests run concurrently, the effective latency is the maximum of the two
    let effective_latency = std::cmp::max(blockhash_latency, slot_latency);

    Ok((blockhash, slot, effective_latency))
}

// Enhanced function with HTTP/2 connection reuse and OpenResty-accurate timing
pub async fn fetch_blockhash_and_slot(
    endpoint: RpcEndpoint,
    db: Arc<DB>,
) -> Result<(), Box<dyn std::error::Error>> {
    
    // Strategy: Get the data we need, but measure timing separately to match OpenResty
    let (blockhash, slot) = match fetch_both_http2(&endpoint.url, true).await {
        Ok((hash, slot_num, _)) => {  // Ignore the internal timing
            HTTP2_REQUESTS.fetch_add(1, Ordering::Relaxed);
            (hash, slot_num)
        }
        Err(e) => {
            // Try HTTP/1.1 with connection reuse
            match fetch_both_http2(&endpoint.url, false).await {
                Ok((hash, slot_num, _)) => {  // Ignore the internal timing
                    if HTTP2_REQUESTS.load(Ordering::Relaxed) < 5 {
                        eprintln!("[{}] HTTP/2 failed, using HTTP/1.1: {}", endpoint.nickname, e);
                    }
                    FALLBACK_REQUESTS.fetch_add(1, Ordering::Relaxed);
                    (hash, slot_num)
                }
                Err(_) => {
                    // Final fallback to original solana_client
                    eprintln!("[{}] Both HTTP/2 and HTTP/1.1 failed, using legacy client", endpoint.nickname);
                    
                    let client = RpcClient::new(endpoint.url.clone());
                    
                    let blockhash = match client.get_latest_blockhash() {
                        Ok(hash) => hash.to_string(),
                        Err(_) => "Unavailable".to_string(),
                    };
                    
                    let slot = match client.get_slot() {
                        Ok(slot) => slot,
                        Err(_) => {
                            println!(
                                "Error fetching slot from {}: request failed",
                                endpoint.nickname
                            );
                            0
                        }
                    };
                    
                    (blockhash, slot)
                }
            }
        }
    };
    
    // Get a separate, precise timing measurement that matches OpenResty
    let latency = match get_single_request_timing(&endpoint.url, true).await {
        Ok(precise_timing) => precise_timing,
        Err(_) => {
            // Fallback timing measurement
            match get_single_request_timing(&endpoint.url, false).await {
                Ok(timing) => timing,
                Err(_) => 1, // Default fallback
            }
        }
    };
    
    // Log connection stats every 50 requests
    let total_requests = HTTP2_REQUESTS.load(Ordering::Relaxed) + FALLBACK_REQUESTS.load(Ordering::Relaxed);
    if total_requests % 50 == 0 && total_requests > 0 {
        let http2_ratio = (HTTP2_REQUESTS.load(Ordering::Relaxed) * 100) / total_requests;
        println!("Protocol stats: {}% HTTP/2, {}% HTTP/1.1+Legacy ({} total) [{}]", 
            http2_ratio, 
            100 - http2_ratio,
            total_requests,
            endpoint.nickname
        );
    }
    
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
    
    let key = format!("{}:{}", endpoint.nickname, Utc::now().timestamp());
    let value = serde_json::to_string(&response)?;
    db.put(key.as_bytes(), value.as_bytes())?;
    
    println!(
        "[{}] Slot: {}, Blockhash: {} ({}ms)",
        endpoint.nickname, slot, blockhash, latency
    );
    
    Ok(())
}
