use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RPCResponse {
    pub timestamp: f64,
    pub slot: u64,
    pub blockhash: String,
    pub latency_ms: u128,
    pub rpc_url: String,
    pub nickname: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RpcEndpoint {
    pub url: String,
    pub nickname: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub rpc: RpcConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RpcConfig {
    pub endpoints: Vec<RpcEndpoint>,
}

#[derive(Debug, Serialize)]
pub struct LeaderboardEntry {
    pub nickname: String,
    pub value: u64,
    pub latency_ms: u128,
    pub timestamp: f64,
}

#[derive(Debug, Serialize)]
pub struct ConsensusStats {
    pub fastest_rpc: String,
    pub slowest_rpc: String,
    pub fastest_latency: u128,
    pub slowest_latency: u128,
    pub consensus_blockhash: String,
    pub consensus_slot: u64,
    pub consensus_percentage: f64,
    pub total_rpcs: usize,
    pub average_latency: f64,
    pub slot_difference: i64,
    pub slot_skew: String,
    pub latency_leaderboard: Vec<LeaderboardEntry>,
    pub slot_leaderboard: Vec<LeaderboardEntry>,
}
