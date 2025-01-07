use bitcoin::Amount;
use serde::Deserialize;
use std::{str::FromStr, sync::Arc};

use crate::{
    config::BtcUtxoProvider,
    db::{BtcBalance, BtcUtxo, Repo},
};

#[derive(Clone)]
pub enum UtxoClient {
    Local(Arc<Repo>),
    CryptoApis(CryptoApisClient),
}

impl UtxoClient {
    pub fn new(cfg: BtcUtxoProvider, db: Arc<Repo>) -> Self {
        match cfg.mode.as_str() {
            "cryptoapis" => Self::CryptoApis(CryptoApisClient::new(&cfg.api_key)),
            _ => Self::Local(db),
        }
    }

    pub async fn get_fee(&self) -> anyhow::Result<u64> {
        match self {
            Self::Local(_db) => Ok(37),
            Self::CryptoApis(ca_client) => ca_client.get_fee().await,
        }
    }

    pub async fn get_balance(&self, address: &str) -> anyhow::Result<BtcBalance> {
        match self {
            Self::Local(db) => Ok(db.get_btc_balance(address).await?),
            Self::CryptoApis(ca_client) => ca_client.get_balance(address).await,
        }
    }

    pub async fn get_utxo(
        &self,
        address: &str,
        limit: i32,
        offset: i32,
    ) -> anyhow::Result<Vec<BtcUtxo>> {
        match self {
            Self::Local(db) => Ok(db
                .select_btc_utxo_with_pagination(Some(address.to_owned()), "ASC", limit, offset)
                .await?),
            Self::CryptoApis(ca_client) => ca_client.get_utxo(address, limit, offset).await,
        }
    }
}

#[derive(Clone)]
pub struct CryptoApisClient {
    api_key: String,
}

impl CryptoApisClient {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_owned(),
        }
    }
    // returns sats/byte
    pub async fn get_fee(&self) -> anyhow::Result<u64> {
        let client = awc::Client::default();
        let url =
            "https://rest.cryptoapis.io/blockchain-data/bitcoin/mainnet/mempool/fees?context=rdx";
        let mut resp = client
            .get(url)
            .insert_header(("X-Api-Key", self.api_key.clone()))
            .send()
            .await
            .unwrap();

        let val = resp.json::<FeeRootResult>().await?;
        let btc_per_byte = val.data.item.fast;
        let fee = bitcoin::Amount::from_btc(f64::from_str(&btc_per_byte)?)?;
        Ok(fee.to_sat())
    }

    pub async fn get_balance(&self, address: &str) -> anyhow::Result<BtcBalance> {
        let client = awc::Client::default();
        let url = format!("https://rest.cryptoapis.io/blockchain-data/bitcoin/mainnet/addresses/{}/balance?context=rdx", address);
        let mut resp = client
            .get(&url)
            .insert_header(("X-Api-Key", self.api_key.clone()))
            .send()
            .await
            .unwrap();

        let val = resp.json::<BalanceResponse>().await?;
        let balance_str = val.data.item.confirmed_balance.amount;
        let balance = Amount::from_str_in(&balance_str, bitcoin::Denomination::Bitcoin)?;

        Ok(BtcBalance {
            address: address.to_owned(),
            id: 0,
            balance: balance.to_sat() as i64,
        })
    }

    pub async fn get_utxo(
        &self,
        address: &str,
        limit: i32,
        offset: i32,
    ) -> anyhow::Result<Vec<BtcUtxo>> {
        let client = awc::Client::default();
        let url = format!("https://rest.cryptoapis.io/blockchain-data/bitcoin/mainnet/addresses/{}/unspent-outputs?context=rdx&limit={}&offset={}", address, limit, offset);
        let mut resp = client
            .get(&url)
            .insert_header(("X-Api-Key", self.api_key.clone()))
            .send()
            .await
            .unwrap();

        let val = resp.json::<UtxoResponse>().await?;
        let sender_btc_address =
            bitcoin::Address::from_str(address)?.require_network(bitcoin::Network::Bitcoin)?;
        let pk_script = sender_btc_address.script_pubkey().to_hex_string();

        let result: Vec<BtcUtxo> = val
            .data
            .items
            .iter()
            .map(|e| -> BtcUtxo {
                let amount = Amount::from_str_in(&e.amount, bitcoin::Denomination::Bitcoin)
                    .unwrap()
                    .to_sat();

                BtcUtxo {
                    id: 0,
                    block: 0,
                    tx_id: 0,
                    tx_hash: e.transaction_id.clone(),
                    output_n: e.index as i32,
                    address: e.address.clone(),
                    pk_script: pk_script.clone(),
                    amount: amount as i64,
                    spend: false,
                }
            })
            .collect();
        Ok(result)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceResponse {
    pub api_version: String,
    pub request_id: String,
    pub context: String,
    pub data: BalanceData,
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceData {
    pub item: BalanceItem,
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BalanceItem {
    pub confirmed_balance: ConfirmedBalance,
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmedBalance {
    pub amount: String,
    pub unit: String,
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UtxoResponse {
    pub api_version: String,
    pub request_id: String,
    pub context: String,
    pub data: UtxoData,
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UtxoData {
    pub limit: i64,
    pub offset: i64,
    pub total: i64,
    pub items: Vec<Utxo>,
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Utxo {
    pub address: String,
    pub amount: String,
    pub index: i64,
    pub is_available: bool,
    pub is_confirmed: bool,
    pub timestamp: i64,
    pub transaction_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeeRootResult {
    pub api_version: String,
    pub request_id: String,
    pub context: String,
    pub data: FeeData,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeeData {
    pub item: Fee,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fee {
    pub unit: String,
    pub fast: String,
    pub slow: String,
    pub standard: String,
}
