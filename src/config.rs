use crate::models::RpcEndpoint;
use serde::Deserialize;
use std::fs;

#[derive(Deserialize, Debug)]
pub struct ServerConfig {
    pub listen_ip: Option<String>,
    pub port: Option<u16>,
}

#[derive(Deserialize, Debug)]
pub struct RpcConfig {
    pub endpoints: Vec<RpcEndpoint>,
}

#[derive(Deserialize, Debug)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub rpc: RpcConfig,
}

pub fn load_config() -> Result<AppConfig, Box<dyn std::error::Error>> {
    let config_str = fs::read_to_string("config.toml")?;
    let config: AppConfig = toml::from_str(&config_str)?;
    Ok(config)
}
