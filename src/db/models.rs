use std::str::FromStr;

use bitcoin::{OutPoint, Txid};
use serde::Serialize;
use sqlx::prelude::FromRow;

#[derive(Default, Clone, Debug, FromRow, Serialize)]
pub struct LastIndexedBlock {
    pub indexer: String,
    pub height: i64,
}

#[derive(Default, Clone, Debug, FromRow)]
pub struct Transaction {
    pub tx_hash: String,
    pub raw_data: String,
    pub status: String, // pendig, invalid, mined
    pub context: String,
    pub request_id: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Transaction {
    pub const STATUS_PENDING: &'static str = "pending";
    pub const STATUS_MINED: &'static str = "mined";
    pub const STATUS_FAILED: &'static str = "failed";
}

#[derive(Default, Clone, Debug, FromRow, Serialize)]
pub struct Rune {
    pub id: i64,
    pub rune: String,
    pub display_name: String,
    pub symbol: String,
    pub block: i64,
    pub tx_id: i32,
    pub mints: i32,
    pub max_supply: String,
    pub premine: String,
    pub burned: String,
    pub minted: String,
    pub in_circulation: String,
    pub divisibility: i32,
    pub turbo: bool,
    pub timestamp: i64,
    pub etching_tx: String,
    pub commitment_tx: String,
    pub raw_data: Vec<u8>,
}

#[derive(Default, Clone, Debug, FromRow, Serialize)]
pub struct RuneShortRow {
    pub rune: String,
    pub block: i64,
    pub tx_id: i32,
    pub mints: i32,
    pub minted: String,
    pub in_circulation: String,
    pub raw_data: Vec<u8>,
}

#[derive(Default, Clone, Debug, FromRow, Serialize)]
pub struct RunesBalance {
    pub id: i64,
    pub address: String,
    pub rune: String,
    pub balance: String,
}

#[derive(Default, Clone, Debug, FromRow, Serialize)]
pub struct RuneUtxo {
    pub id: i64,
    pub block: i64,
    pub tx_id: i32,
    pub tx_hash: String,
    pub output_n: i32,
    pub rune: String,
    pub address: String,
    pub pk_script: String,
    pub amount: String,
    pub btc_amount: i64,
    pub spend: bool,
}

impl RuneUtxo {
    pub fn out_point(&self) -> anyhow::Result<OutPoint> {
        let txid = Txid::from_str(&self.tx_hash)?;
        Ok(OutPoint {
            txid,
            vout: self.output_n as u32,
        })
    }
}

#[derive(Default, Clone, Debug, FromRow)]
pub struct RuneLog {
    pub id: i64,
    pub tx_hash: String,
    pub rune: String,
    pub address: String,
    pub action: String,
    pub value: String,
}

impl RuneLog {
    pub const ETCHING: &'static str = "etching";
    pub const MINT: &'static str = "etching";
    pub const INCOME: &'static str = "income";
    pub const EXPENCE: &'static str = "expence";
}

#[derive(Default, Clone, Debug, FromRow, Serialize)]
pub struct BtcBalance {
    pub id: i64,
    pub address: String,
    pub balance: i64,
}

#[derive(Default, Clone, Debug, FromRow, Serialize)]
pub struct BtcUtxo {
    pub id: i64,
    pub block: i64,
    pub tx_id: i32,
    pub tx_hash: String,
    pub output_n: i32,
    pub address: String,
    pub pk_script: String,
    pub amount: i64,
    pub spend: bool,
}
impl BtcUtxo {
    pub fn out_point(&self) -> anyhow::Result<OutPoint> {
        let txid = Txid::from_str(&self.tx_hash)?;
        Ok(OutPoint {
            txid,
            vout: self.output_n as u32,
        })
    }
}
#[derive(Default, Clone, Debug, FromRow)]
pub struct TradingPair {
    pub id: i64,
    pub base_asset: String,
    pub quote_asset: String,
    pub pool_address: String,
    pub base_balance: String,
    pub quote_balance: String,
    pub locked_base_balance: String,
    pub locked_quote_balance: String,
    pub fee_address: String,
    pub treasury_address: String,
    pub swap_fee_percent: f64,
}

#[derive(Default, Clone, Debug, FromRow)]
pub struct PoolDeposit {
    pub trading_pair: i64,
    pub pool_address: String,
    pub sender: String,
    pub block: i64,
    pub tx_hash: String,
    pub asset: String,
    pub amount: String,
    pub tx_time: i64,
}

#[derive(Default, Clone, Debug, FromRow, Serialize)]
pub struct LiquidityProvider {
    pub id: i64,
    pub base_address: String,
    pub quote_address: String,
    pub trading_pair: i64,
    pub base_amount: String,
    pub quote_amount: String,
}

#[derive(Default, Clone, Debug, FromRow, Serialize)]
pub struct LiquidityChangeRequest {
    pub id: i64,
    pub req_uid: String,
    pub base_address: String,
    pub quote_address: String,
    pub trading_pair: i64,
    pub base_amount: String,
    pub quote_amount: String,
    pub action: String,
    pub status: String,
    pub tx_hash: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl LiquidityChangeRequest {
    pub const SWAP_DIRECT: &'static str = "swap-direct";
    pub const SWAP_REVERSE: &'static str = "swap-reverse";
    pub const ADD_LIQUIDITY: &'static str = "add";
    pub const REMOVE_LIQUIDITY: &'static str = "remove";

    pub const STATUS_NEW: &'static str = "new";
    pub const STATUS_DONE: &'static str = "done";
    pub const STATUS_FAILED: &'static str = "failed";

    pub fn is_add_liquidity(&self) -> bool {
        self.action.as_str() == Self::ADD_LIQUIDITY
    }
    pub fn is_direct_swap(&self) -> bool {
        self.action.as_str() == Self::SWAP_DIRECT
    }

    pub fn is_reverse_swap(&self) -> bool {
        self.action.as_str() == Self::SWAP_REVERSE
    }

    pub fn is_rm_liquidity(&self) -> bool {
        self.action.as_str() == Self::REMOVE_LIQUIDITY
    }
}
