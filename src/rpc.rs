use crate::models::{RPCResponse, RpcEndpoint};
use chrono::Utc;
use rocksdb::DB;
use solana_client::rpc_client::RpcClient;
use std::sync::Arc;
use std::time::Instant;

pub async fn fetch_blockhash_and_slot(
    endpoint: RpcEndpoint,
    db: Arc<DB>,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = RpcClient::new(endpoint.url.clone());
    let start_time = Instant::now();

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

    let key = format!("{}:{}", endpoint.nickname, Utc::now().timestamp());
    let value = serde_json::to_string(&response)?;
    db.put(key.as_bytes(), value.as_bytes())?;

    println!(
        "[{}] Slot: {}, Blockhash: {} ({}ms)",
        endpoint.nickname, slot, blockhash, latency
    );
    Ok(())
}
