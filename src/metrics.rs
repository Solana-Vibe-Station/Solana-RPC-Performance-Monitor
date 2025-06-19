use std::collections::HashMap;
use std::sync::Arc;
use axum::{extract::{Query, State}, Json};
use rocksdb::DB;

use crate::models::{RPCResponse, LeaderboardEntry, ConsensusStats};

pub fn calculate_consensus(responses: &[RPCResponse]) -> ConsensusStats {
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

    for response in responses {
        *blockhash_counts.entry(response.blockhash.clone()).or_insert(0) += 1;
        *slot_counts.entry(response.slot).or_insert(0) += 1;
    }

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

    let consensus_percentage = (consensus_blockhash.1 as f64 / total_rpcs as f64) * 100.0;

    let fastest = responses
        .iter()
        .min_by_key(|r| r.latency_ms)
        .unwrap();

    let slowest = responses
        .iter()
        .max_by_key(|r| r.latency_ms)
        .unwrap();

    let slot_difference = fastest.slot as i64 - slowest.slot as i64;
    let slot_skew = if slot_difference == 0 {
        "No skew".to_string()
    } else if slot_difference > 0 {
        format!("Fastest ahead by {} slots", slot_difference.abs())
    } else {
        format!("Slowest ahead by {} slots", slot_difference.abs())
    };

    let average_latency = responses
        .iter()
        .map(|r| r.latency_ms as f64)
        .sum::<f64>() / total_rpcs as f64;

    let mut latency_leaderboard: Vec<LeaderboardEntry> = responses.iter()
        .map(|r| LeaderboardEntry {
            nickname: r.nickname.clone(),
            value: r.latency_ms as u64,
            latency_ms: r.latency_ms,
            timestamp: r.timestamp,
        })
        .collect();
    latency_leaderboard.sort_by_key(|entry| entry.value);
    latency_leaderboard.truncate(4);

    let mut slot_leaderboard: Vec<LeaderboardEntry> = responses.iter()
        .map(|r| LeaderboardEntry {
            nickname: r.nickname.clone(),
            value: r.slot,
            latency_ms: r.latency_ms,
            timestamp: r.timestamp,
        })
        .collect();
    slot_leaderboard.sort_by(|a, b| b.value.cmp(&a.value));
    slot_leaderboard.truncate(4);

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

pub async fn get_metrics(
    State(db): State<Arc<DB>>,
    Query(params): Query<HashMap<String, String>>,
) -> Json<(Vec<RPCResponse>, ConsensusStats)> {
    let mut responses = Vec::new();
    let rpc_filter = params.get("rpc");
    let from_ts = params.get("from").and_then(|ts| ts.parse::<i64>().ok());
    let to_ts = params.get("to").and_then(|ts| ts.parse::<i64>().ok());

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

    let consensus_stats = calculate_consensus(&latest_by_rpc.values().cloned().collect::<Vec<_>>());

    let public_responses: Vec<RPCResponse> = responses
        .into_iter()
        .map(|mut r| {
            r.rpc_url = String::new();
            r
        })
        .collect();

    Json((public_responses, consensus_stats))
}
