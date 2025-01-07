use crate::{db, serde_utils::number_from_string};
use bitcoin::{
    locktime::absolute::LockTime, script::Builder, Address, Network, OutPoint, ScriptBuf, Sequence,
    Transaction, TxIn, TxOut, Txid, Witness,
};
use ordinals::{Artifact, Runestone, Terms};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub display_name: Option<String>,
    pub symbol: String,
    pub decimals: i32,
}

#[allow(dead_code)]
impl Asset {
    pub fn btc() -> Self {
        Self {
            name: "BTC".to_owned(),
            display_name: Some("Bitcoin".to_owned()),
            symbol: "B".to_owned(),
            decimals: 8,
        }
    }
    pub fn rune(name: &str, display_name: &str, symbol: &str, decimals: i32) -> Self {
        Self {
            name: name.to_owned(),
            display_name: Some(display_name.to_owned()),
            symbol: symbol.to_owned(),
            decimals,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuneEntity {
    pub rune: String,
    pub display_name: String,
    pub symbol: String,
    pub block: i64,
    pub tx_id: i32,
    pub mints: i32,
    #[serde(with = "number_from_string")]
    pub premine: u128,
    #[serde(with = "number_from_string")]
    pub burned: u128,
    #[serde(with = "number_from_string")]
    pub max_supply: u128,
    #[serde(with = "number_from_string")]
    pub minted: u128,
    #[serde(with = "number_from_string")]
    pub in_circulation: u128,
    pub divisibility: i32,
    pub turbo: bool,
    pub timestamp: i64,
    pub etching_tx: String,
    pub commitment_tx: String,
    pub terms: Option<ordinals::Terms>,
    pub raw_data: Vec<u8>,
}

impl From<db::Rune> for RuneEntity {
    fn from(source: db::Rune) -> Self {
        Self::from(&source)
    }
}

impl From<&db::Rune> for RuneEntity {
    fn from(source: &db::Rune) -> Self {
        Self {
            rune: source.rune.clone(),
            display_name: source.display_name.clone(),
            symbol: source.symbol.clone(),
            block: source.block,
            tx_id: source.tx_id,
            mints: source.mints,
            premine: u128::from_str(&source.premine).unwrap_or_default(),
            burned: u128::from_str(&source.burned).unwrap_or_default(),
            max_supply: u128::from_str(&source.max_supply).unwrap_or_default(),
            minted: u128::from_str(&source.minted).unwrap_or_default(),
            in_circulation: u128::from_str(&source.in_circulation).unwrap_or_default(),
            divisibility: source.divisibility,
            turbo: source.turbo,
            timestamp: source.timestamp,
            etching_tx: source.etching_tx.clone(),
            commitment_tx: source.commitment_tx.clone(),
            terms: RuneEntity::terms_from_data(&source.raw_data),
            raw_data: source.raw_data.clone(),
        }
    }
}

impl RuneEntity {
    pub fn add_mint(&mut self, amount: u128) -> bool {
        self.mints += 1;
        self.in_circulation += amount;
        let r = self.minted.checked_add(amount);
        self.minted = r.unwrap_or(self.minted);
        r.is_some()
    }

    pub fn burn(&mut self, amount: u128) -> bool {
        self.mints += 1;
        self.burned += amount;
        let r = self.in_circulation.checked_sub(amount);
        self.in_circulation = r.unwrap_or(self.in_circulation);
        r.is_some()
    }

    fn terms_from_data(data: &[u8]) -> Option<Terms> {
        let tx: Transaction = Transaction {
            version: 2,
            lock_time: LockTime::ZERO,
            input: Vec::new(),
            output: vec![TxOut {
                script_pubkey: ScriptBuf::from_bytes(data.to_vec()),
                value: 0,
            }],
        };

        let artifact = Runestone::decipher(&tx).unwrap();
        let Artifact::Runestone(runestone) = artifact else {
            return None;
        };

        runestone.etching.and_then(|e| e.terms)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    pub asset: Asset,
    pub address: String,
    #[serde(with = "number_from_string")]
    pub balance: u128,
}

impl Balance {
    pub fn increase(&mut self, amount: u128) -> bool {
        let r = self.balance.checked_add(amount);
        self.balance = r.unwrap_or(self.balance);
        r.is_some()
    }

    pub fn decrease(&mut self, amount: u128) -> bool {
        let r = self.balance.checked_sub(amount);
        self.balance = r.unwrap_or(self.balance);
        r.is_some()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuneUtxo {
    pub block: i64,
    pub tx_id: i32,
    pub tx_hash: String,
    pub output_n: i32,
    pub rune: String,
    pub address: String,
    pub pk_script: String,
    #[serde(with = "number_from_string")]
    pub amount: u128,
    pub btc_amount: i64,
    pub spend: bool,
}

impl std::convert::From<&db::RuneUtxo> for RuneUtxo {
    fn from(row: &db::RuneUtxo) -> RuneUtxo {
        RuneUtxo {
            block: row.block,
            tx_id: row.tx_id,
            tx_hash: row.tx_hash.clone(),
            output_n: row.output_n,
            rune: row.rune.clone(),
            address: row.address.clone(),
            pk_script: row.pk_script.clone(),
            amount: u128::from_str(&row.amount).unwrap_or_default(),
            btc_amount: row.btc_amount,
            spend: row.spend,
        }
    }
}

impl std::convert::From<&RuneUtxo> for db::RuneUtxo {
    fn from(row: &RuneUtxo) -> db::RuneUtxo {
        db::RuneUtxo {
            id: 0,
            block: row.block,
            tx_id: row.tx_id,
            tx_hash: row.tx_hash.clone(),
            output_n: row.output_n,
            rune: row.rune.clone(),
            address: row.address.clone(),
            pk_script: row.pk_script.clone(),
            amount: row.amount.to_string(),
            btc_amount: row.btc_amount,
            spend: row.spend,
        }
    }
}

impl RuneUtxo {
    pub fn tx_parent(&self) -> anyhow::Result<(TxIn, TxOut)> {
        let parent_in = TxIn {
            previous_output: OutPoint {
                txid: Txid::from_str(&self.tx_hash)?,
                vout: self.output_n as u32,
            },
            script_sig: Builder::new().into_script(),
            witness: Witness::new(),
            sequence: Sequence::ZERO,
        };

        let parent_out = TxOut {
            script_pubkey: ScriptBuf::from_hex(&self.pk_script.clone())?,
            value: self.btc_amount as u64,
        };

        Ok((parent_in, parent_out))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BtcUtxo {
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
    pub fn tx_parent(&self) -> anyhow::Result<(TxIn, TxOut)> {
        let parent_in = TxIn {
            previous_output: OutPoint {
                txid: Txid::from_str(&self.tx_hash)?,
                vout: self.output_n as u32,
            },
            script_sig: Builder::new().into_script(),
            witness: Witness::new(),
            sequence: Sequence::ZERO,
        };

        let parent_out = TxOut {
            script_pubkey: ScriptBuf::from_hex(&self.pk_script.clone())?,
            value: self.amount as u64,
        };

        Ok((parent_in, parent_out))
    }
}
impl std::convert::From<&db::BtcUtxo> for BtcUtxo {
    fn from(row: &db::BtcUtxo) -> BtcUtxo {
        BtcUtxo {
            block: row.block,
            tx_id: row.tx_id,
            tx_hash: row.tx_hash.clone(),
            output_n: row.output_n,
            address: row.address.clone(),
            pk_script: row.pk_script.clone(),
            amount: row.amount,
            spend: row.spend,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TradingPair {
    pub id: i64,
    pub base_asset: Asset,
    #[serde(with = "number_from_string")]
    pub base_balance: u128,
    #[serde(with = "number_from_string")]
    pub locked_base_balance: u128,
    pub quote_asset: Asset,
    #[serde(with = "number_from_string")]
    pub quote_balance: u128,
    #[serde(with = "number_from_string")]
    pub locked_quote_balance: u128,
    pub pool_address: String,
    pub fee_address: String,
    pub treasury_address: String,
    pub swap_fee_percent: f64,
}

impl TradingPair {
    pub fn new(row: &db::TradingPair, rune: &db::Rune) -> Self {
        Self {
            id: row.id,
            base_asset: Asset::rune(
                &rune.rune,
                &rune.display_name,
                &rune.symbol,
                rune.divisibility,
            ),
            base_balance: u128::from_str(&row.base_balance).unwrap_or_default(),
            locked_base_balance: u128::from_str(&row.locked_base_balance).unwrap_or_default(),
            quote_asset: Asset::btc(),
            quote_balance: u128::from_str(&row.quote_balance).unwrap_or_default(),
            locked_quote_balance: u128::from_str(&row.locked_quote_balance).unwrap_or_default(),
            pool_address: row.pool_address.clone(),
            fee_address: row.fee_address.clone(),
            treasury_address: row.treasury_address.clone(),
            swap_fee_percent: row.swap_fee_percent,
        }
    }

    pub fn get_pool_address(&self, net: Network) -> anyhow::Result<(Address, Address, Address)> {
        let pool_address = Address::from_str(&self.pool_address)?.require_network(net)?;
        let fee_address = Address::from_str(&self.fee_address)?.require_network(net)?;
        let treasury_address = Address::from_str(&self.treasury_address)?.require_network(net)?;

        Ok((pool_address, fee_address, treasury_address))
    }

    pub fn price(&self) -> f64 {
        if self.quote_balance == 0 {
            return 1.0;
        }
        self.base_balance as f64 / self.quote_balance as f64
    }

    pub fn price_in_units(&self) -> f64 {
        if self.quote_balance == 0 {
            return 1.0;
        }
        (self.base_balance as f64 / f64::powf(10.0, self.base_asset.decimals as f64))
            / (self.quote_balance as f64 / f64::powf(10.0, self.quote_asset.decimals as f64))
    }

    pub fn verify_rate(&self, base: u128, quote: u128) -> (bool, f64) {
        let stored_price = self.price();
        let given_price = base as f64 / quote as f64;

        if stored_price == given_price {
            return (true, 0.0);
        }

        let delta_percentage = ((given_price - stored_price) / stored_price).abs() * 100.0;

        (false, delta_percentage)
    }

    pub fn reverse_price(&self) -> f64 {
        if self.base_balance == 0 {
            return 1.0;
        }
        self.quote_balance as f64 / self.base_balance as f64
    }

    pub fn reverse_price_in_units(&self) -> f64 {
        if self.base_balance == 0 {
            return 1.0;
        }

        (self.quote_balance as f64 / f64::powf(10.0, self.quote_asset.decimals as f64))
            / (self.base_balance as f64 / f64::powf(10.0, self.base_asset.decimals as f64))
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TxInputData {
    pub tx_hash: String,
    pub vout: i32,
    pub pk_script: String,
    #[serde(with = "number_from_string")]
    pub value: u64,
}

#[cfg(test)]
mod tests {
    #[test]
    fn price_calculation() {
        use super::{Asset, TradingPair};
        let btc = Asset::btc();
        let rune = Asset::rune("RRR", "RRR", "r", 0);

        let mut tp = TradingPair {
            id: 0,
            base_asset: rune,
            quote_asset: btc,
            pool_address: "address".to_owned(),
            swap_fee_percent: 0.5,
            fee_address: "address".to_owned(),
            treasury_address: "address".to_owned(),
            base_balance: 40,
            quote_balance: 1,
            locked_base_balance: 0,
            locked_quote_balance: 0,
        };

        assert_eq!(tp.price(), 40.0);
        assert_eq!(tp.reverse_price(), 0.025);

        tp.base_balance = 0;
        tp.quote_balance = 0;
        assert_eq!(tp.price(), 1.0);
        assert_eq!(tp.reverse_price(), 1.0);

        tp.base_balance = 330;
        tp.quote_balance = 33000;
        assert_eq!(tp.price(), 0.01);
        assert_eq!(tp.price_in_units(), 1000000.0);

        let (ok, delta) = tp.verify_rate(23, 2310);
        assert!(!ok);
        assert!(delta > 0.1);

        tp.base_balance = 298;
        tp.quote_balance = 36200;
        assert_ne!(tp.price(), 0.01);

        let (ok, delta) = tp.verify_rate(82, 10000);
        assert!(!ok);
        assert!(delta > 0.1);
        println!("{}", delta)
    }

    #[test]
    fn balance_serialization() {
        use super::{Asset, Balance};
        let balance = Balance {
            asset: Asset::btc(),
            address: "valid_btc_address".to_owned(),
            balance: 1230000000123000000,
        };

        let ser_str = serde_json::to_string(&balance).unwrap();

        let b: Balance = serde_json::from_str(&ser_str).unwrap();

        assert_eq!(balance.address, b.address);
        assert_eq!(balance.balance, b.balance);
        //assert_eq!(balance.asset, b.asset);

        let balance = Balance {
            asset: Asset::rune("NOTBTC", "NO•BTC", "B", 12),
            address: "valid_btc_address".to_owned(),
            balance: 1230000000123000000,
        };

        let ser_str = serde_json::to_string(&balance).unwrap();

        let b: Balance = serde_json::from_str(&ser_str).unwrap();
        let o_name: String = "NOTBTC".to_string();
        let o_symbol = "B";
        let o_decimals = 12_i32;
        assert_eq!(balance.address, b.address);
        assert_eq!(balance.balance, b.balance);
        assert_eq!(b.asset.name, o_name);
        assert_eq!(b.asset.symbol, o_symbol);
        assert_eq!(b.asset.decimals, o_decimals);

        //assert_eq!(balance.asset, b.asset);
    }
}
