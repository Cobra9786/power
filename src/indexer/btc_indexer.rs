use bitcoin::{Transaction, TxIn};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use std::sync::Arc;
use std::time::Duration;
use tokio::{task::JoinHandle, time::sleep};
use tokio_util::sync::CancellationToken;

use crate::{config, db, service::BtcIndexCache};

static BTC_INDEXER_ID: &str = "btc_indexer";

pub struct TxInfo {
    pub block: i64,
    pub tx_n: i32,
    pub txid: String,
    pub timestamp: i64,
    pub tx: Transaction,
}

pub struct BtcIndexer {
    net: bitcoin::Network,
    repo: Arc<db::Repo>,
    cfg: config::IndexersConfig,
    rpc: Client,
    state: BtcIndexCache,
}

impl BtcIndexer {
    pub fn new(
        btc_cfg: &config::BTCConfig,
        cfg: &config::IndexersConfig,
        repo: Arc<db::Repo>,
    ) -> Self {
        let net = btc_cfg.get_network();
        let rpc = Client::new(
            &btc_cfg.address,
            Auth::UserPass(btc_cfg.rpc_user.clone(), btc_cfg.rpc_password.clone()),
        )
        .unwrap();

        Self {
            net,
            repo,
            rpc,
            cfg: cfg.clone(),
            state: BtcIndexCache::default(),
        }
    }

    async fn init_state(&mut self) -> anyhow::Result<()> {
        let watchlist = self.repo.select_btc_balance().await?;
        self.state.init_btc_balances(self.net, watchlist);
        Ok(())
    }

    pub fn start(self, cancel: CancellationToken) -> JoinHandle<()> {
        // todo: use spawn_blocking
        tokio::spawn(self.run(cancel.clone()))
    }

    async fn run(self, stop_signal: CancellationToken) {
        let mut indexer = self;

        let last_block = match indexer.repo.get_last_indexed_block(BTC_INDEXER_ID).await {
            Ok(block) => block.height,
            Err(_) => 0,
        };

        let first_block = if last_block > indexer.cfg.btc_starting_height {
            last_block
        } else {
            indexer.cfg.btc_starting_height
        };

        let mut best_block = match indexer.rpc.get_block_count() {
            Ok(height) => height as i64,
            Err(err) => {
                error!("Can't get best BTC block error={}", err);
                error!("Indexing stopped");
                return;
            }
        };

        info!(
            "RPC init successful! best_block={} first_block={}",
            best_block, first_block
        );

        if let Err(err) = indexer.init_state().await {
            error!("Unable to init indexer state: error={}", err);
            return;
        }

        let mut current_block = first_block + 1;

        loop {
            best_block = match indexer.rpc.get_block_count() {
                Ok(height) => height as i64,
                Err(err) => {
                    error!("Can't get best BTC block error={}", err);
                    return;
                }
            };

            if best_block == current_block {
                tokio::select! {
                    _ = sleep(Duration::from_secs(10)) => {
                        continue;
                   }

                    _ = stop_signal.cancelled() => {
                        log::info!("gracefully shutting down cache purge job");
                        break;
                    }
                };
            }

            if let Some(hash) = indexer.index_block(current_block).await {
                match indexer
                    .repo
                    .update_last_indexed_block(current_block, BTC_INDEXER_ID)
                    .await
                {
                    Ok(_) => (),
                    Err(err) => {
                        error!("Can't get BTC block error={}, hash={}", err, hash);
                    }
                };

                current_block += 1;
            }

            tokio::select! {
                _ = sleep(Duration::from_millis(10)) => {
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

    async fn index_block(&mut self, height: i64) -> Option<String> {
        let block_hash = match self.rpc.get_block_hash(height as u64) {
            Ok(hash) => hash,
            Err(err) => {
                error!("Can't get BTC block hash error={}, height={}", err, height);
                return None;
            }
        };

        let block: bitcoin::Block = match self.rpc.get_by_id(&block_hash) {
            Ok(block) => block,
            Err(err) => {
                error!("Can't get BTC block error={}, hash={}", err, block_hash);
                return None;
            }
        };

        debug!(
            "Fetch new block: height={} hash={} tx_count={}",
            height,
            block_hash,
            block.txdata.len()
        );

        for (txi, tx) in block.txdata.iter().enumerate() {
            let tx_info = TxInfo {
                block: height,
                tx_n: txi as i32,
                txid: tx.txid().to_string(),
                tx: tx.clone(),
                timestamp: block.header.time as i64,
            };

            self.handle_btc_payments(&tx_info).await;
        }

        Some(block_hash.to_string())
    }

    async fn handle_btc_payments(&mut self, tx_info: &TxInfo) {
        for input in tx_info.tx.input.iter() {
            self.spent_btc_utxo(input).await;
        }

        for (vout, out) in tx_info.tx.output.iter().enumerate() {
            let Some((address, balance)) = self
                .state
                .increase_btc_balance_if_present(&out.script_pubkey, out.value as i64)
            else {
                continue;
            };

            if let Err(err) = self.repo.update_btc_balance(&address, balance).await {
                error!(
                    "Can't update btc balance: error={}, address={} balance={}",
                    err, &address, balance
                );
            }

            let btc_utxo: db::BtcUtxo = db::BtcUtxo {
                id: 0,
                block: tx_info.block,
                tx_id: tx_info.tx_n,
                tx_hash: tx_info.tx.txid().to_string(),
                output_n: vout as i32,
                address,
                pk_script: out.script_pubkey.to_hex_string(),
                amount: out.value as i64,
                spend: false,
            };

            if let Err(err) = self.repo.insert_btc_utxo(&btc_utxo).await {
                error!("Can't save new btc_utxo: error={}", err);
            }
        }
    }

    async fn spent_btc_utxo(&mut self, input: &TxIn) -> Option<()> {
        let parent_txid = input.previous_output.txid.to_string();
        let vout = input.previous_output.vout as i32;

        let Ok(utxo) = self.repo.get_btc_utxo(&parent_txid, vout).await else {
            return None;
        };

        if let Err(err) = self.repo.spent_btc_utxo(&parent_txid, vout).await {
            error!(
                "failed to mark rune utxo as spend: error={} tx_hash={} vout={}",
                err, parent_txid, vout
            );
        }

        let new_balance = self.state.decrease_btc_balance(&utxo.address, utxo.amount);

        if let Err(err) = self
            .repo
            .update_btc_balance(&utxo.address, new_balance)
            .await
        {
            error!(
                "failed to update balance: error={} address={}",
                err, &utxo.address
            );
        }
        None
    }
}
