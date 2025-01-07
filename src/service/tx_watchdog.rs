use bitcoin::Txid;
use bitcoincore_rpc::{Auth, Client, RpcApi};
use chrono::{TimeZone, Utc};
use std::time::Duration;
use std::{str::FromStr, sync::Arc};
use tokio::{task::JoinHandle, time::sleep};
use tokio_util::sync::CancellationToken;

use crate::{config, db};

pub struct TxWatchdog {
    db: Arc<db::Repo>,
    rpc: Client,
}

impl TxWatchdog {
    pub fn new(btc_cfg: &config::BTCConfig, db: Arc<db::Repo>) -> Self {
        let rpc = Client::new(
            &btc_cfg.address,
            Auth::UserPass(btc_cfg.rpc_user.clone(), btc_cfg.rpc_password.clone()),
        )
        .unwrap();

        Self { db, rpc }
    }

    pub fn start(self, cancel: CancellationToken) -> JoinHandle<()> {
        tokio::spawn(self.run(cancel.clone()))
    }

    async fn run(self, stop_signal: CancellationToken) {
        let mut indexer = self;
        loop {
            indexer.do_job().await;

            tokio::select! {
                _ = sleep(Duration::from_secs(30)) => {
                    continue;
               }

                _ = stop_signal.cancelled() => {
                    log::info!("gracefully shutting down cache purge job");
                    break;
                }
               // else => continue,
            };
        }
    }

    async fn do_job(&mut self) {
        let pending_txs = match self.db.select_pending_txs().await {
            Ok(txs) => txs,
            Err(err) => {
                error!("Failed to select pending txs: error={}", err);
                return;
            }
        };

        for tx in pending_txs.iter() {
            let txid = match Txid::from_str(&tx.tx_hash) {
                Ok(id) => id,
                Err(err) => {
                    error!("invalid tx_hash: tx_hash={} error={}", tx.tx_hash, err);
                    continue;
                }
            };

            let tx_info = match self.rpc.get_raw_transaction_info(&txid, None) {
                Ok(info) => info,
                Err(err) => {
                    let created_at = Utc.timestamp_opt(tx.created_at, 0).unwrap();
                    let now = Utc::now();

                    // Calculate the duration between now and created_at
                    let duration = now.signed_duration_since(created_at);

                    // Check if the duration is 1 hours or more
                    if duration >= chrono::TimeDelta::hours(1) {
                        error!(
                            "unable to get tx status: tx_hash={} error={}",
                            tx.tx_hash, err
                        );
                        self.fail_tx(tx).await;
                    }

                    continue;
                }
            };

            debug!(
                "Pending tx status: tx_hash={}  confirmations={}",
                tx.tx_hash,
                tx_info.confirmations.unwrap_or_default()
            );
            if tx_info.confirmations.unwrap_or_default() < 2 {
                continue;
            }

            let request = match self.db.get_liquidity_change_request(&tx.request_id).await {
                Ok(request) => request,
                Err(err) => {
                    error!(
                        "Can't get liquidity change request: context={} request_id={} error={}",
                        tx.context, tx.request_id, err
                    );
                    return;
                }
            };
            if request.is_add_liquidity() {
                self.process_change_liquidity(tx, &request, Action::AddLiquidity)
                    .await;
            }

            if request.is_direct_swap() {
                self.process_change_liquidity(tx, &request, Action::Swap)
                    .await;
            }

            if request.is_reverse_swap() {
                self.process_change_liquidity(tx, &request, Action::ReverseSwap)
                    .await;
            }

            if request.is_rm_liquidity() {
                self.process_change_liquidity(tx, &request, Action::RmLiquidity)
                    .await;
            }
        }
    }

    async fn fail_tx(&self, tx: &db::Transaction) {
        let mut dbtx = match self.db.pool.begin().await {
            Ok(tx) => tx,
            Err(err) => {
                error!("Can't begin new trasaction: error={}", err);
                return;
            }
        };

        if let Err(err) = self
            .db
            .update_submitted_tx(
                &mut dbtx,
                &tx.tx_hash,
                db::LiquidityChangeRequest::STATUS_FAILED,
            )
            .await
        {
            error!(
                "Failed to update submitted tx: context={} request_id={}  error={}",
                tx.context, tx.request_id, err
            );
            return;
        }

        if let Err(err) = self
            .db
            .update_liquidity_change_request(
                &mut dbtx,
                &tx.request_id,
                &tx.request_id,
                db::Transaction::STATUS_FAILED,
            )
            .await
        {
            error!(
                "Failed to update submitted tx: context={} request_id={}  error={}",
                tx.context, tx.request_id, err
            );
        }

        if let Err(err) = dbtx.commit().await {
            error!("Failed to commit dbtx: error={}", err);
        }
    }

    async fn process_change_liquidity(
        &self,
        tx: &db::Transaction,
        request: &db::LiquidityChangeRequest,
        action: Action,
    ) {
        let mut dbtx = match self.db.pool.begin().await {
            Ok(tx) => tx,
            Err(err) => {
                error!("Can't begin new trasaction: error={}", err);
                return;
            }
        };

        let mut trading_pair = match self.db.get_trading_pair_by_id(request.trading_pair).await {
            Ok(tp) => tp,
            Err(err) => {
                error!(
                    "Can't get trading pair: context={} request_id={} id={} error={}",
                    tx.context, tx.request_id, request.trading_pair, err
                );
                return;
            }
        };

        let base_delta = u128::from_str(&request.base_amount).unwrap_or_default();
        let quote_delta = u128::from_str(&request.quote_amount).unwrap_or_default();

        let pool_base_balance = u128::from_str(&trading_pair.base_balance).unwrap_or_default();
        let pool_quote_balance = u128::from_str(&trading_pair.quote_balance).unwrap_or_default();

        match action {
            Action::AddLiquidity => {
                trading_pair.base_balance = (pool_base_balance + base_delta).to_string();
                trading_pair.quote_balance = (pool_quote_balance + quote_delta).to_string();
            }
            Action::RmLiquidity => {
                trading_pair.base_balance = (pool_base_balance - base_delta).to_string();
                trading_pair.quote_balance = (pool_quote_balance - quote_delta).to_string();
            }
            Action::Swap => {
                // user send base asset and recived quote asset
                trading_pair.base_balance = (pool_base_balance + base_delta).to_string();
                trading_pair.quote_balance = (pool_quote_balance - quote_delta).to_string();
            }
            Action::ReverseSwap => {
                // user send quote asset and recived base asset
                trading_pair.base_balance = (pool_base_balance - base_delta).to_string();
                trading_pair.quote_balance = (pool_quote_balance + quote_delta).to_string();
            }
        }

        if action == Action::AddLiquidity || action == Action::RmLiquidity {
            let mut lp = match self
                .db
                .get_liquidity_provider(request.trading_pair, &request.base_address)
                .await
            {
                Ok(lp) => lp,
                Err(err) => {
                    error!(
                    "Failed to fetch liquidity provider: context={} request_id={} pair_id={} base_address={} error={}",
                    tx.context, tx.request_id, request.trading_pair, request.base_address, err
                );
                    return;
                }
            };
            let lp_base_balance = u128::from_str(&lp.base_amount).unwrap_or_default();
            let lp_quote_balance = u128::from_str(&lp.quote_amount).unwrap_or_default();

            match action {
                Action::AddLiquidity => {
                    lp.base_amount = (lp_base_balance + base_delta).to_string();
                    lp.quote_amount = (lp_quote_balance + quote_delta).to_string();
                }
                Action::RmLiquidity => {
                    lp.base_amount = (lp_base_balance - base_delta).to_string();
                    lp.quote_amount = (lp_quote_balance - quote_delta).to_string();
                }
                _ => (),
            }

            match self.db.update_liquidity_provider(&mut dbtx, &lp).await {
                Ok(_) => (),
                Err(err) => {
                    error!(
                    "Failed to update liquidity provider: context={} request_id={} pair_id={} base_address={} error={}",
                    tx.context, tx.request_id, request.trading_pair, request.base_address, err
                );
                    return;
                }
            }
        }

        match self.db.update_trading_pair(&mut dbtx, &trading_pair).await {
            Ok(_) => (),
            Err(err) => {
                error!(
                    "Failed to update trading pair: context={} request_id={} id={} error={}",
                    tx.context, tx.request_id, request.trading_pair, err
                );
                return;
            }
        };

        match self
            .db
            .update_liquidity_change_request(
                &mut dbtx,
                &request.req_uid,
                &tx.tx_hash,
                db::LiquidityChangeRequest::STATUS_DONE,
            )
            .await
        {
            Ok(_) => (),
            Err(err) => {
                error!("Failed to update liquidity_change request tx: context={} request_id={} id={} error={}",
                        tx.context, tx.request_id, request.trading_pair, err);
                return;
            }
        }

        match self
            .db
            .update_submitted_tx(&mut dbtx, &tx.tx_hash, db::Transaction::STATUS_MINED)
            .await
        {
            Ok(_) => (),
            Err(err) => {
                error!(
                    "Failed to update submitted tx: context={} request_id={} id={} error={}",
                    tx.context, tx.request_id, request.trading_pair, err
                );
            }
        }

        if let Err(err) = dbtx.commit().await {
            error!("Failed to commit dbtx: error={}", err);
        }
    }
}

#[derive(PartialEq)]
enum Action {
    AddLiquidity,
    RmLiquidity,
    Swap,
    ReverseSwap,
}
