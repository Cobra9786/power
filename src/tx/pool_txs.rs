use std::sync::Arc;
use std::{collections::HashSet, str::FromStr};

use bitcoin::sighash::TapSighashType;
use bitcoin::{
    absolute::LockTime,
    address::NetworkChecked,
    psbt::{self},
    secp256k1::XOnlyPublicKey,
    Address, AddressType, Network, OutPoint, PublicKey, ScriptBuf, Transaction, TxOut,
};
use ordinals::{Edict, RuneId, Runestone};
use tokio::sync::RwLock;

use crate::cache::CacheRepo;
use crate::{
    btc_utxo::UtxoClient,
    db::Repo,
    service::entities::{BtcUtxo, RuneUtxo},
    tx::runes_txs,
};

pub struct PoolTxBuilder {
    db: Arc<Repo>,
    pub cache: Arc<RwLock<CacheRepo>>,
    utxo_provider: UtxoClient,
}

impl PoolTxBuilder {
    pub fn new(db: Arc<Repo>, cache: Arc<RwLock<CacheRepo>>, utxo_provider: UtxoClient) -> Self {
        Self {
            db,
            cache,
            utxo_provider,
        }
    }

    pub async fn collect_runes_utxo(
        &self,
        rune_name: &str,
        address: &str,
        amount: u128,
        locked_utxos: &HashSet<OutPoint>,
    ) -> anyhow::Result<Vec<RuneUtxo>> {
        let mut offset = 0;
        let mut collected_amount: u128 = 0;
        let mut result = Vec::new();

        'collector: loop {
            if collected_amount >= amount {
                break;
            }
            let db_utxos = self
                .db
                .select_runes_utxo_with_pagination(
                    rune_name,
                    Some(address.to_owned()),
                    "ASC",
                    100,
                    offset,
                )
                .await?;

            if db_utxos.is_empty() {
                anyhow::bail!(
                    "account({}) doesn't have enounght runes({}) utxos: has={} need={}",
                    address,
                    rune_name,
                    collected_amount,
                    amount
                )
            }

            for u in db_utxos.iter() {
                let op = u.out_point()?;
                if locked_utxos.contains(&op) {
                    continue;
                }
                let am = u128::from_str(&u.amount).unwrap_or_default();
                if am == 0 {
                    continue;
                }

                collected_amount += am;
                result.push(RuneUtxo::from(u));

                if collected_amount >= amount {
                    break 'collector;
                }
            }
            offset += 100;
        }
        Ok(result)
    }

    pub async fn collect_btc_utxo(
        &self,
        address: &str,
        amount: u64,
        locked_utxos: &HashSet<OutPoint>,
    ) -> anyhow::Result<Vec<BtcUtxo>> {
        let mut offset = 0;
        let mut collected_amount: u64 = 0;
        let mut result = Vec::new();

        'collector: loop {
            if collected_amount >= amount {
                break;
            }
            let db_utxos = self.utxo_provider.get_utxo(address, 40, offset).await?;
            if db_utxos.is_empty() {
                anyhow::bail!(
                    "account({}) doesn't have enounght btc utxos: has={} need={}",
                    address,
                    collected_amount,
                    amount
                )
            }

            for u in db_utxos.iter() {
                let op = u.out_point()?;
                if locked_utxos.contains(&op) {
                    continue;
                }

                collected_amount += u.amount as u64;
                result.push(BtcUtxo::from(u));

                if collected_amount >= amount {
                    break 'collector;
                }
            }
            offset += 40;
        }
        Ok(result)
    }

    pub async fn build_multi_asset_tx(
        &self,
        tx_params: TxParams,
        net: Network,
    ) -> anyhow::Result<PSBTContainer> {
        let (btc_amount, rune_amount) = (
            tx_params.btc_output.btc_amount,
            tx_params.rune_output.rune_amount,
        );

        let rune_name = tx_params.rune_input.rune_name.clone().unwrap();

        let mut cache = self.cache.write().await;
        let mut used_btc_utxos = cache
            .get_locked_utxos(tx_params.btc_input.address.to_string().as_str())
            .await?;

        if tx_params.btc_input.address != tx_params.btc_fee_input.address {
            let btc_utxos = cache
                .get_locked_utxos(tx_params.btc_input.address.to_string().as_str())
                .await?;
            for u in btc_utxos.into_iter() {
                used_btc_utxos.insert(u);
            }
        }

        let used_runes_utxos = cache
            .get_locked_utxos(tx_params.rune_input.address.to_string().as_str())
            .await?;

        for u in used_runes_utxos.into_iter() {
            used_btc_utxos.insert(u);
        }

        let mut builder_ctx = TxBuilderCtx::new(true);
        builder_ctx.used_btc_utxos = used_btc_utxos;

        // builder_ctx.used_btc_utxos = cache.get_locked_utxos(asset, address);
        // this is an amount for case if there isn't enough btc on the rune inputs
        // to cover 2 rune outputs (main + change).
        let mut btc_extra_amount: u64 = 0;

        // ---- set runes inputs  ----
        {
            let mut rune_in_amount: u128 = 0;
            let mut rune_btc_in_amount: u64 = 0;
            {
                let (rune_redeem_script, rune_tr_pubkey) =
                    tx_params.rune_input.psbt_input_extras(net)?;

                let address = tx_params.rune_input.address.to_string();
                let runes_utxo = self
                    .collect_runes_utxo(
                        &rune_name,
                        &address,
                        rune_amount,
                        &builder_ctx.used_btc_utxos,
                    )
                    .await?;

                let can_be_signed = tx_params.rune_input.can_be_signed;

                for u in runes_utxo {
                    if rune_in_amount > rune_amount {
                        break;
                    }

                    let (tx_in, tx_out) = u.tx_parent()?;

                    rune_in_amount += u.amount;
                    rune_btc_in_amount += u.btc_amount as u64;

                    builder_ctx
                        .runes_input_indexes
                        .push((builder_ctx.tx.input.len(), can_be_signed));
                    builder_ctx.tx.input.push(tx_in.clone());

                    builder_ctx.used_btc_utxos.insert(tx_in.previous_output);
                    builder_ctx
                        .new_used_btc_utxos
                        .insert((address.clone(), tx_in.previous_output));
                    builder_ctx
                        .parent_utxos
                        .push((can_be_signed, tx_out.clone()));

                    builder_ctx.psbt_inputs.push(psbt_input(
                        &tx_out,
                        &rune_redeem_script,
                        &rune_tr_pubkey,
                    ));
                }
            }

            warn!("RUNE_BTC_IN_AMOUNT = {}", rune_btc_in_amount);
            builder_ctx.btc_in += rune_btc_in_amount;
            // ----------------------------

            // ---- set runes outputs ----
            let rune = self.db.get_rune(&rune_name).await?;
            let edicts: Vec<Edict> = vec![Edict {
                id: RuneId {
                    block: rune.block as u64,
                    tx: rune.tx_id as u32,
                },
                amount: rune_amount,
                output: 1,
            }];

            builder_ctx.tx.output.push(TxOut {
                script_pubkey: tx_params.rune_output.address.script_pubkey(),
                value: runes_txs::RUNES_OUT_VALUE,
            });
            builder_ctx.btc_out += runes_txs::RUNES_OUT_VALUE;

            let mut rune_btc_change = rune_btc_in_amount - runes_txs::RUNES_OUT_VALUE;
            if rune_btc_change < runes_txs::RUNES_OUT_VALUE {
                btc_extra_amount = runes_txs::RUNES_OUT_VALUE - rune_btc_change;
                rune_btc_change = runes_txs::RUNES_OUT_VALUE;
            }

            let pointer = Some(builder_ctx.tx.output.len() as u32);
            builder_ctx.tx.output.push(TxOut {
                value: rune_btc_change,
                script_pubkey: tx_params.rune_input.address.script_pubkey(),
            });

            warn!("RUNE_BTC_CHANGE_AMOUNT = {}", rune_btc_change);
            builder_ctx.btc_out += rune_btc_change;

            let runestone = Runestone {
                edicts,
                etching: None,
                mint: None,
                pointer,
            };

            builder_ctx.tx.output[0].script_pubkey = runestone.encipher();

            warn!(
                "TX_SUMMARY 0:  btc_in={}/{} btc_out={}/{} delta={}",
                builder_ctx.btc_in,
                builder_ctx.tx.input.len(),
                builder_ctx.btc_out,
                builder_ctx.tx.output.len(),
                builder_ctx.btc_in - builder_ctx.btc_out
            );
        }
        // ------------------------------

        let mut service_fee: u64 = 0;

        if let Some(opts) = tx_params.service_fee {
            service_fee = ((btc_amount as f64 * opts.fee_precent) / 100.0).round() as u64;
            if service_fee < 2000 {
                service_fee = 1000 // prevent dust utxos
            }
            let am = service_fee / opts.destination.len() as u64;
            for a in opts.destination {
                builder_ctx.tx.output.push(TxOut {
                    value: am,
                    script_pubkey: a.script_pubkey(),
                });
            }
            builder_ctx.btc_out += service_fee;
        }

        let fee_rate = self.utxo_provider.get_fee().await?;
        // this is rough estimation of resulting fee for the tx
        // trying to guess resulting size of the fully set tx
        let fee = fee_rate * builder_ctx.tx.vsize() as u64 * 2; // 2 stands as rough estim for the size grow of the signed tx

        let total_fee: u64 = fee + service_fee + btc_extra_amount;

        warn!(
            "BTC_FEE = {} ->> fee_rate={} fee={} service_fee={} btc_extra={}",
            total_fee, fee_rate, fee, service_fee, btc_extra_amount
        );

        warn!(
            "TX_SUMMARY 1:  btc_in={}/{} btc_out={}/{} fee={}, total_fee={} delta={}",
            builder_ctx.btc_in,
            builder_ctx.tx.input.len(),
            builder_ctx.btc_out,
            builder_ctx.tx.output.len(),
            fee,
            total_fee,
            builder_ctx.btc_in - builder_ctx.btc_out
        );

        if tx_params.btc_input.address == tx_params.btc_fee_input.address {
            warn!(
                " 1 TRY TO ADD {} btc + {} fee = {}",
                btc_amount,
                total_fee,
                btc_amount + total_fee
            );
            self.add_btc_to_tx(
                net,
                &mut builder_ctx,
                tx_params.btc_input,
                Some(tx_params.btc_output),
                btc_amount + total_fee,
            )
            .await?;
        } else {
            warn!(" 2 TRY TO ADD btc = {}", btc_amount + total_fee);
            self.add_btc_to_tx(
                net,
                &mut builder_ctx,
                tx_params.btc_input,
                Some(tx_params.btc_output),
                btc_amount,
            )
            .await?;

            self.add_btc_to_tx(
                net,
                &mut builder_ctx,
                tx_params.btc_fee_input,
                None,
                total_fee,
            )
            .await?;
        }
        warn!(
            "TX_SUMMARY 2:  btc_in={}/{} btc_out={}/{} fee={}, total_fee={} delta={}",
            builder_ctx.btc_in,
            builder_ctx.tx.input.len(),
            builder_ctx.btc_out,
            builder_ctx.tx.output.len(),
            fee,
            total_fee,
            builder_ctx.btc_in - builder_ctx.btc_out
        );

        // ----------------------------

        let mut psbt = bitcoin::psbt::Psbt::from_unsigned_tx(builder_ctx.tx.clone())?;
        psbt.inputs = builder_ctx.psbt_inputs;

        // for (address, utxo) in builder_ctx.new_used_btc_utxos.iter() {
        //     let _ = cache.set_locked_utxo(address, utxo).await?;
        // }

        // ----------------------------
        Ok(PSBTContainer {
            btc_inputs: builder_ctx.btc_input_indexes,
            rune_inputs: builder_ctx.runes_input_indexes,
            tx: builder_ctx.tx,
            psbt,
            fee: total_fee,
            parent_utxos: builder_ctx.parent_utxos,
        })
    }

    async fn add_btc_to_tx(
        &self,
        net: Network,
        builder_ctx: &mut TxBuilderCtx,
        input_params: InputOpts,
        output: Option<OutputOpts>,
        btc_amount: u64,
    ) -> anyhow::Result<()> {
        let mut btc_in_amount = 0;
        {
            let (btc_redeem_script, btc_tr_pubkey) = input_params.psbt_input_extras(net)?;
            let address = input_params.address.to_string();
            let btc_utxo = self
                .collect_btc_utxo(&address, btc_amount, &builder_ctx.used_btc_utxos)
                .await?;

            let can_be_signed = input_params.can_be_signed;

            for u in btc_utxo.iter() {
                let (tx_in, tx_out) = u.tx_parent()?;

                if builder_ctx.used_btc_utxos.contains(&tx_in.previous_output) {
                    continue;
                }
                btc_in_amount += u.amount as u64;

                builder_ctx
                    .btc_input_indexes
                    .push((builder_ctx.tx.input.len(), can_be_signed));
                builder_ctx.tx.input.push(tx_in.clone());

                builder_ctx.used_btc_utxos.insert(tx_in.previous_output);
                builder_ctx
                    .new_used_btc_utxos
                    .insert((address.clone(), tx_in.previous_output));
                builder_ctx
                    .parent_utxos
                    .push((can_be_signed, tx_out.clone()));

                builder_ctx.psbt_inputs.push(psbt_input(
                    &tx_out,
                    &btc_redeem_script,
                    &btc_tr_pubkey,
                ));
            }
        }
        if btc_in_amount < btc_amount {
            anyhow::bail!(
                "not enough btc: has({}) < needs({})",
                btc_in_amount,
                btc_in_amount
            )
        }

        builder_ctx.btc_in += btc_in_amount;

        // -----------------------------

        // -----  set btc output -------
        if let Some(output) = output {
            builder_ctx.tx.output.push(TxOut {
                value: output.btc_amount,
                script_pubkey: output.address.script_pubkey(),
            });

            builder_ctx.btc_out += output.btc_amount;
        }

        let btc_change_value = btc_in_amount - btc_amount;
        if btc_change_value > 600 {
            builder_ctx.tx.output.push(TxOut {
                value: btc_change_value,
                script_pubkey: input_params.address.script_pubkey(),
            });

            builder_ctx.btc_out += btc_change_value;
        }

        Ok(())
    }
}

#[derive(Clone)]
pub struct PSBTContainer {
    pub psbt: psbt::Psbt,
    pub tx: Transaction,
    // (id_of_input, signable)
    pub rune_inputs: Vec<(usize, bool)>,
    // (id_of_input, signable)
    pub btc_inputs: Vec<(usize, bool)>,
    pub fee: u64,
    // (signable, tx_out)
    pub parent_utxos: Vec<(bool, TxOut)>,
}

pub struct TxParams {
    pub rune_input: InputOpts,
    pub btc_input: InputOpts,
    pub btc_fee_input: InputOpts,
    pub rune_output: OutputOpts,
    pub btc_output: OutputOpts,
    pub service_fee: Option<ServiceFeeParams>,
}

pub struct ServiceFeeParams {
    pub destination: Vec<Address>,
    pub fee_precent: f64,
}

pub struct OutputOpts {
    pub address: Address,
    pub rune_name: Option<String>,
    pub rune_amount: u128,
    pub btc_amount: u64,
}

pub struct InputOpts {
    pub address: Address<NetworkChecked>,
    pub original_public_key: Option<String>,
    pub can_be_signed: bool,
    pub rune_name: Option<String>,
}

impl InputOpts {
    pub fn psbt_input_extras(
        &self,
        net: Network,
    ) -> anyhow::Result<(Option<ScriptBuf>, Option<XOnlyPublicKey>)> {
        let Some(adt) = self.address.address_type() else {
            return Ok((None, None));
        };

        match adt {
            AddressType::P2wsh | AddressType::P2sh => {
                if self.original_public_key.is_none() {
                    anyhow::bail!("address type ({}) requires a valid original_pubkey", adt,)
                }
                let pubkey = self.original_public_key.clone().unwrap();

                let pk = PublicKey::from_str(&pubkey)?;
                let a = Address::p2wpkh(&pk, net)?;

                Ok((Some(a.script_pubkey()), None))
            }
            AddressType::P2tr => {
                if self.original_public_key.is_none() {
                    anyhow::bail!("address type ({}) requires a valid original_pubkey", adt,)
                }
                let pubkey = self.original_public_key.clone().unwrap();
                let xonly_pubkey = XOnlyPublicKey::from_str(&pubkey)?;

                Ok((None, Some(xonly_pubkey)))
            }
            _ => Ok((None, None)),
        }
    }
}

fn psbt_input(
    tx_out: &TxOut,
    redeem_script: &Option<ScriptBuf>,
    tap_key: &Option<XOnlyPublicKey>,
) -> bitcoin::psbt::Input {
    bitcoin::psbt::Input {
        witness_utxo: Some(tx_out.clone()),
        redeem_script: redeem_script.clone(),
        tap_internal_key: *tap_key,
        sighash_type: Some(bitcoin::psbt::PsbtSighashType::from_u32(
            TapSighashType::All as u32,
        )),
        ..Default::default()
    }
}

struct TxBuilderCtx {
    tx: Transaction,
    psbt_inputs: Vec<psbt::Input>,
    parent_utxos: Vec<(bool, TxOut)>,
    // rune utxos are also btc utxos,
    // this set is made to prevent adding them into the tx twice
    used_btc_utxos: HashSet<OutPoint>,
    // this separate hash set of outputs
    // that we write to cache when build tx
    new_used_btc_utxos: HashSet<(String, OutPoint)>,
    btc_input_indexes: Vec<(usize, bool)>,
    runes_input_indexes: Vec<(usize, bool)>,
    btc_in: u64,
    btc_out: u64,
}

impl TxBuilderCtx {
    fn new(add_runestone: bool) -> Self {
        let mut tx = Transaction {
            version: 2,
            lock_time: LockTime::ZERO,
            input: Vec::new(),
            output: Vec::new(),
        };

        if add_runestone {
            tx.output.push(
                // it will be OP_RETURN 13 Runic Magic
                TxOut {
                    value: 0,
                    script_pubkey: ScriptBuf::new(),
                },
            );
        }

        Self {
            tx,
            psbt_inputs: Vec::new(),
            parent_utxos: Vec::new(),
            used_btc_utxos: HashSet::new(),
            new_used_btc_utxos: HashSet::new(),
            btc_input_indexes: Vec::new(),
            runes_input_indexes: Vec::new(),
            btc_in: 0,
            btc_out: 0,
        }
    }
}
