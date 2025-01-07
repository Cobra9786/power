use actix_web::HttpResponse;
use bitcoin::address::NetworkChecked;
use bitcoin::{Address, Network};
use serde::Deserialize;
use std::str::FromStr;
use std::sync::Arc;

use super::errors;
use crate::{db::Repo, serde_utils::number_from_string, service::entities};

#[derive(Deserialize)]
pub struct SearchQuery {
    pub s: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PairRequest {
    pub base: String,
    pub quote: String,
}

impl PairRequest {
    pub async fn fetch_pair(&self, db: &Arc<Repo>) -> Result<entities::TradingPair, HttpResponse> {
        match db.get_trading_pair(&self.base, &self.quote).await {
            Ok(p) => {
                let rune = match db.get_rune(&p.base_asset).await {
                    Ok(p) => p,
                    Err(e) => match e {
                        sqlx::Error::RowNotFound => return Err(errors::ApiError::NotFound.into()),
                        _ => {
                            error!("request failed error={}", e);
                            return Err(errors::bad_request(
                                "can't rune asset",
                                Some(e.to_string()),
                            ));
                        }
                    },
                };

                Ok(entities::TradingPair::new(&p, &rune))
            }

            Err(e) => match e {
                sqlx::Error::RowNotFound => Err(errors::ApiError::NotFound.into()),
                _ => {
                    error!("request failed error={}", e);
                    Err(errors::bad_request("can't fetch pair", Some(e.to_string())))
                }
            },
        }
    }
}
#[derive(Deserialize)]
pub struct UtxoRequest {
    pub asset: String,
    pub address: String,
}

#[derive(Debug, Deserialize)]
pub struct PairPositionRequest {
    pub base: String,
    pub quote: String,
    pub address: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CalculateQuery {
    pub base: Option<String>,
    pub quote: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SubmitTxReq {
    pub psbt: String,
    pub request_id: Option<String>,
    pub context: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddLiquidityReq {
    pub base_address: String,
    pub base_address_pubkey: Option<String>,
    #[serde(with = "number_from_string")]
    pub base_amount: u128,
    pub quote_address: String,
    pub quote_address_pubkey: Option<String>,
    #[serde(with = "number_from_string")]
    pub quote_amount: u128,
}

impl AddLiquidityReq {
    pub fn parse_addresses(&self, net: Network) -> Result<(Address, Address), HttpResponse> {
        let rune_address = match decode_address(&self.base_address, net) {
            Ok(a) => a,
            Err(err) => {
                return Err(errors::bad_request(
                    "base_address in invalid",
                    Some(err.to_string()),
                ));
            }
        };

        let btc_address = match decode_address(&self.quote_address, net) {
            Ok(a) => a,
            Err(err) => {
                return Err(errors::bad_request(
                    "quote_address in invalid",
                    Some(err.to_string()),
                ));
            }
        };
        Ok((rune_address, btc_address))
    }
}

#[derive(Debug, Deserialize)]
pub struct RmLiquidityReq {
    pub base_address: String,
    #[serde(with = "number_from_string")]
    pub base_amount: u128,
    pub quote_address: String,
    #[serde(with = "number_from_string")]
    pub quote_amount: u128,
    pub fee_address: String,
    pub fee_address_pubkey: Option<String>,
}

impl RmLiquidityReq {
    pub fn extract_addresses(
        &self,
        net: Network,
    ) -> Result<(Address, Address, Address), HttpResponse> {
        let base_address = match decode_address(&self.base_address, net) {
            Ok(a) => a,
            Err(err) => {
                return Err(errors::bad_request(
                    "base_address in invalid",
                    Some(err.to_string()),
                ));
            }
        };
        let quote_address = match decode_address(&self.quote_address, net) {
            Ok(a) => a,
            Err(err) => {
                return Err(errors::bad_request(
                    "ask_address in invalid",
                    Some(err.to_string()),
                ));
            }
        };

        let fee_address = match decode_address(&self.fee_address, net) {
            Ok(a) => a,
            Err(err) => {
                return Err(errors::bad_request(
                    "fee_address in invalid",
                    Some(err.to_string()),
                ));
            }
        };

        Ok((base_address, quote_address, fee_address))
    }
}

#[derive(Clone, Default, Deserialize)]
pub struct SwapRequest {
    pub bid_asset: String,
    #[serde(with = "number_from_string")]
    pub bid_amount: u128,
    pub bid_address: String,
    pub bid_address_pubkey: Option<String>,
    pub ask_address: String,
    #[serde(with = "number_from_string")]
    pub ask_amount: u128,
    pub fee_address: String,
    pub fee_address_pubkey: Option<String>,
    pub rate: f64,
    pub slippage: f64,
    pub slippage_tolerance: bool,
}

impl SwapRequest {
    pub fn extract_addresses(
        &self,
        net: Network,
    ) -> Result<(Address, Address, Address), HttpResponse> {
        let bid_address = match decode_address(&self.bid_address, net) {
            Ok(a) => a,
            Err(err) => {
                return Err(errors::bad_request(
                    "base_address in invalid",
                    Some(err.to_string()),
                ));
            }
        };
        let ask_address = match decode_address(&self.ask_address, net) {
            Ok(a) => a,
            Err(err) => {
                return Err(errors::bad_request(
                    "ask_address in invalid",
                    Some(err.to_string()),
                ));
            }
        };

        let fee_address = match decode_address(&self.fee_address, net) {
            Ok(a) => a,
            Err(err) => {
                return Err(errors::bad_request(
                    "fee_address in invalid",
                    Some(err.to_string()),
                ));
            }
        };

        Ok((bid_address, ask_address, fee_address))
    }
}

pub fn decode_address(address: &str, net: Network) -> anyhow::Result<Address<NetworkChecked>> {
    Ok(Address::from_str(address)?.require_network(net)?)
}
