use serde::Deserialize;
use std::fs;

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    pub api: APIConfig,
    pub btc: BTCConfig,
    pub db: DBConfig,
    pub redis: RedisConfig,
    pub indexers: IndexersConfig,
    pub signature_provider: SignatureProvider,
}

#[derive(Deserialize, Clone, Debug)]
pub struct APIConfig {
    pub listen_address: String,
    pub port: i32,
    pub cors_domain: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedisConfig {
    pub address: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct BTCConfig {
    pub network: Option<String>,
    pub address: String,
    pub rpc_user: String,
    pub rpc_password: String,
    pub utxo_provider: BtcUtxoProvider,
}

impl BTCConfig {
    pub fn get_network(&self) -> bitcoin::Network {
        let Some(net) = self.network.clone() else {
            return bitcoin::Network::Bitcoin;
        };

        match net.as_str() {
            "mainnet" => bitcoin::Network::Bitcoin,
            "testnet" => bitcoin::Network::Testnet,
            "regtest" => bitcoin::Network::Regtest,
            _ => bitcoin::Network::Bitcoin,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct DBConfig {
    pub dsn: String,
    pub automigrate: bool,
}

#[derive(Deserialize, Clone, Debug)]
pub struct IndexersConfig {
    pub btc_starting_height: i64,
    pub runes_starting_height: i64,
    pub handle_edicts: bool,
    pub disable_rune_log: bool,
    pub btc_watchlist: Vec<String>,
    pub runes_watchlist: Vec<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct SignatureProvider {
    pub local: LocalSigner,
}

#[derive(Deserialize, Clone, Debug)]
pub struct LocalSigner {
    pub address: String,
    pub secret_key: String,
    pub mode: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct BtcUtxoProvider {
    pub mode: String,
    pub api_key: String,
}

pub fn read_config(path: &str) -> Result<Config, std::io::Error> {
    let contents = fs::read_to_string(path)?;

    let config: Config = toml::from_str(&contents).unwrap();
    Ok(config)
}
