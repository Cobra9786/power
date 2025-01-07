use serde::Serialize;
use sqlx::prelude::FromRow;

#[derive(Default, Clone, Debug, FromRow, Serialize)]
pub struct LastIndexedBlock {
    pub network: String,
    pub height: u32,
    pub hash: String,
}

#[derive(Default, Clone, Debug, FromRow)]
pub struct Transaction {
    pub block: u32,
    pub tx_id: u32,
    pub tx_hash: String,
    pub raw_data: Vec<u8>,
}

#[derive(Default, Clone, Debug, FromRow, Serialize)]
pub struct Rune {
    pub rune: String,
    pub block: u32,
    pub tx_id: u32,
    pub max_supply: i64,
    pub minted: i64,
    pub in_circulation: i64,
    pub raw_data: Vec<u8>,
}

#[derive(Default, Clone, Debug, FromRow)]
pub struct RunesBalance {
    pub address: String,
    pub rune: String,
    pub balance: u64,
}

#[derive(Default, Clone, Debug, FromRow)]
pub struct RuneUtxo {
    pub block: u64,
    pub tx_id: u32,
    pub tx_hash: String,
    pub output_n: i32,
    pub rune: String,
    pub address: String,
    pub pk_script: String,
    pub amount: u64,
    pub spend: bool,
}

#[derive(Default, Clone, Debug, FromRow)]
pub struct RuneBalanceLog {
    pub block: u64,
    pub tx_id: u64,
    pub output_n: i32,
    pub rune: String,
    pub address: String,
    pub delta: i64,
    pub event: String,
}

#[derive(Default, Clone, Debug, FromRow)]
pub struct BtcWatchlist {
    pub address: String,
    pub balance: u64,
}

#[derive(Default, Clone, Debug, FromRow)]
pub struct BtcUtxo {
    pub block: u64,
    pub tx_id: u32,
    pub tx_hash: String,
    pub output_n: i32,
    pub rune: String,
    pub address: String,
    pub pk_script: String,
    pub amount: u64,
    pub spend: bool,
}
