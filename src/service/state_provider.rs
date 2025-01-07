use std::str::FromStr;
use std::sync::Arc;

use super::entities::{self, Asset, Balance, RuneEntity};
use crate::cache::CacheRepo;
use crate::db;
use crate::db::Repo;

pub struct StateProvider {
    db: Arc<Repo>,
    cache: CacheRepo,
    disable_rune_log: bool,
}

impl StateProvider {
    pub fn new(db: Arc<Repo>, cache: CacheRepo, disable_rune_log: bool) -> Self {
        Self {
            db,
            cache,
            disable_rune_log,
        }
    }

    pub fn db(&self) -> Arc<Repo> {
        self.db.clone()
    }

    pub async fn warm_up_cache(&mut self) -> anyhow::Result<()> {
        let runes_count = self.db.count_runes(None).await?;
        let mut rune_offset = 0_i32;
        let limit = 10000_i32;

        info!(
            "Starting data ingestion to the cache: runes_count={}",
            runes_count
        );

        'runes_loop: loop {
            if rune_offset as i64 > runes_count {
                break 'runes_loop;
            }
            let runes = self.db.list_runes("ASC", limit, rune_offset, None).await?;
            for rune in runes.iter() {
                let r = entities::RuneEntity::from(rune);
                self.cache.set_rune(&r).await?;

                let utxo_count = self.db.count_runes_utxo(&rune.rune, None).await?;
                let mut utxo_offset = 0_i32;
                info!("     ---->: utxo_count={}", utxo_count);
                'utxo_loop: loop {
                    if utxo_offset as i64 > utxo_count {
                        break 'utxo_loop;
                    }

                    let utxos = self
                        .db
                        .select_runes_utxo_with_pagination(
                            &rune.rune,
                            None,
                            "ASC",
                            limit,
                            utxo_offset,
                        )
                        .await?;
                    for utxo in utxos.iter() {
                        let u = entities::RuneUtxo::from(utxo);
                        self.cache.set_runes_utxo(&u).await?;
                    }

                    utxo_offset += limit;
                }

                let balance_count = self.db.count_runes_balances(&rune.rune).await?;
                let mut balance_offset = 0_i32;

                info!("     ---->: balance_count={}", balance_count);
                'balance_loop: loop {
                    if balance_offset as i64 > balance_count {
                        break 'balance_loop;
                    }

                    let balances = self
                        .db
                        .select_runes_balances(&rune.rune, limit, balance_offset)
                        .await?;
                    for balance in balances.iter() {
                        let b = entities::Balance {
                            asset: entities::Asset::rune(
                                &rune.rune,
                                &rune.display_name,
                                &rune.symbol,
                                rune.divisibility,
                            ),
                            address: balance.address.clone(),
                            balance: u128::from_str(&balance.balance).unwrap_or_default(),
                        };
                        self.cache.set_balance(&b).await?;
                    }

                    balance_offset += limit;
                }
            }
            rune_offset += limit;
            info!("------")
        }

        Ok(())
    }

    pub async fn get_rune_by_name(&mut self, rune: &str) -> anyhow::Result<entities::RuneEntity> {
        let cache_result = self.cache.get_rune(rune).await;
        if let Ok(r) = cache_result {
            return Ok(r);
        }

        let rune_row = self.db.get_rune(rune).await?;
        let r = entities::RuneEntity::from(rune_row);
        self.cache.set_rune(&r).await?;
        Ok(r)
    }

    pub async fn get_rune_by_id(
        &self,
        block: i64,
        tx: i32,
    ) -> anyhow::Result<entities::RuneEntity> {
        let rune_row = self.db.get_rune_by_id(block, tx).await?;

        Ok(entities::RuneEntity::from(rune_row))
    }

    pub async fn get_rune_name_by_id(&mut self, rune_id: &ordinals::RuneId) -> Option<String> {
        if let Ok(rune) = self.cache.get_rune_name(rune_id.block, rune_id.tx).await {
            return Some(rune);
        }
        None
    }

    pub async fn store_new_rune(&mut self, rune_row: &db::Rune) -> anyhow::Result<()> {
        self.db.insert_rune(rune_row).await?;

        self.cache
            .set_rune(&RuneEntity::from(rune_row.clone()))
            .await?;
        Ok(())
    }

    pub async fn burn_rune(&mut self, rune: &str, amount: u128) -> anyhow::Result<()> {
        let mut rune_info = self.get_rune_by_name(rune).await?;
        rune_info.burn(amount);

        self.cache.set_rune(&rune_info).await?;
        self.db
            .update_rune_burned(
                rune,
                rune_info.burned.to_string().as_str(),
                rune_info.in_circulation.to_string().as_str(),
            )
            .await?;

        Ok(())
    }
    pub async fn update_rune_mint(&mut self, rune: &RuneEntity) -> anyhow::Result<()> {
        self.cache.set_rune(rune).await?;
        self.db
            .update_rune_mint(
                &rune.rune,
                rune.mints,
                rune.minted.to_string().as_str(),
                rune.in_circulation.to_string().as_str(),
            )
            .await?;

        Ok(())
    }

    pub async fn get_rune_balance(&mut self, rune: &str, address: &str) -> Balance {
        if let Ok(balance) = self.cache.get_balance(address, rune).await {
            return balance;
        }

        if let Ok(balance) = self.db.get_rune_balance(address, rune).await {
            let rune_data = self.get_rune_by_name(rune).await.unwrap();

            return Balance {
                asset: Asset {
                    name: rune_data.rune,
                    display_name: Some(rune_data.display_name),
                    symbol: rune_data.symbol,
                    decimals: rune_data.divisibility,
                },
                address: address.to_owned(),
                balance: u128::from_str(&balance.balance).unwrap_or_default(),
            };
        }

        let rune_data = self.get_rune_by_name(rune).await.unwrap();

        Balance {
            asset: Asset {
                name: rune_data.rune,
                display_name: Some(rune_data.display_name),
                symbol: rune_data.symbol,
                decimals: rune_data.divisibility,
            },
            address: address.to_owned(),
            balance: 0,
        }
    }

    pub async fn store_new_runes_utxo(
        &mut self,
        utxo: &entities::RuneUtxo,
        action: &str,
    ) -> anyhow::Result<()> {
        // 1. + balance in the cache
        // 2. update balance in the db
        // 3.1 store new utxo in the db
        // 3.2 TODO: add utxo to the cache
        // 4. write rune_log

        let mut balance = self.get_rune_balance(&utxo.rune, &utxo.address).await;

        let new_balance = balance.balance == 0;
        if new_balance {
            let _ = self
                .db
                .insert_runes_balance(&utxo.rune, &utxo.address, "0")
                .await;
        }

        balance.increase(utxo.amount);

        let res = self
            .db
            .update_runes_balance(
                &utxo.rune,
                &utxo.address,
                balance.balance.to_string().as_str(),
            )
            .await;
        if let Err(err) = res {
            error!(
                "failed to update balance: error={} address={} rune={}",
                err, utxo.address, utxo.rune
            );
            return Err(err.into());
        }

        if let Err(err) = self.cache.set_balance(&balance).await {
            error!(
                "failed to update balance in cache: error={} rune={} address={}",
                err, &utxo.rune, &utxo.address
            );
        };
        let db_row = utxo.into();
        if let Err(err) = self.db.insert_rune_utxo(&db_row).await {
            error!("failed to insert runes utxo: error={}", err);
            return Err(err.into());
        }

        if let Err(err) = self.cache.set_runes_utxo(utxo).await {
            error!("failed to insert runes utxo to cache: error={}", err);
            return Err(err.into());
        }

        if self.disable_rune_log {
            return Ok(());
        }

        let log = db::RuneLog {
            id: 0,
            tx_hash: utxo.tx_hash.clone(),
            rune: utxo.rune.clone(),
            address: utxo.address.clone(),
            value: utxo.amount.to_string(),
            action: action.to_string(),
        };

        if let Err(err) = self.db.insert_rune_log(&log).await {
            error!("failed to insert rune log: error={}", err);
            return Err(err.into());
        }
        Ok(())
    }

    pub async fn spent_rune_utxo(
        &mut self,
        input: &bitcoin::TxIn,
        new_tx_id: &str,
    ) -> Option<Vec<entities::RuneUtxo>> {
        let parent_txid = input.previous_output.txid.to_string();
        let vout = input.previous_output.vout;

        //        let Ok(utxos) = self.db.get_runes_utxo(&parent_txid, vout).await else {

        let mut utxos = match self.cache.get_runes_utxos(&parent_txid, vout).await {
            Ok(u) => u,
            Err(err) => {
                error!("can't get utxo from cache error={}", err);
                return None;
            }
        };
        if utxos.is_empty() {
            return None;
        }

        let mut res_list = Vec::new();
        for utxo in utxos.iter_mut() {
            if let Err(err) = self
                .db
                .spent_rune_utxo(&utxo.rune, &parent_txid, vout as i32)
                .await
            {
                error!(
                    "failed to mark rune utxo as spend: error={} tx_hash={} vout={}",
                    err, parent_txid, vout
                );
            }
            utxo.spend = true;
            let _ = self.cache.set_runes_utxo(utxo).await;

            let mut balance = self.get_rune_balance(&utxo.rune, &utxo.address).await;
            if !balance.decrease(utxo.amount) {
                error!("WTF?!");
                continue;
            }

            if let Err(err) = self
                .db
                .update_runes_balance(&utxo.rune, &utxo.address, &balance.balance.to_string())
                .await
            {
                error!(
                    "failed to update balance: error={} rune={} address={}",
                    err, &utxo.rune, &utxo.address
                );
            }
            if let Err(err) = self.cache.set_balance(&balance).await {
                error!(
                    "failed to update balance in cache: error={} rune={} address={}",
                    err, &utxo.rune, &utxo.address
                );
            };

            if self.disable_rune_log {
                res_list.push(utxo.clone());
                return Some(res_list);
            }

            let res = self
                .db
                .insert_rune_log(&db::RuneLog {
                    id: 0,
                    tx_hash: new_tx_id.to_string(),
                    rune: utxo.rune.clone(),
                    address: utxo.address.clone(),
                    action: db::RuneLog::EXPENCE.into(),
                    value: utxo.amount.to_string(),
                })
                .await;
            match res {
                Ok(_) => res_list.push(utxo.clone()),
                Err(err) => {
                    error!(
                        "failed to add rune log: error={} tx_hash={}",
                        err, new_tx_id,
                    );
                    return None;
                }
            }
        }

        Some(res_list)
    }
}
