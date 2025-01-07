use bitcoin::Txid;
use bitcoin::{opcodes, script::Instruction, Address, Transaction, TxOut};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use ordinals::{Artifact, Edict, RuneId, Runestone, SpacedRune};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::time::Duration;
use tokio::{task::JoinHandle, time::sleep};
use tokio_util::sync::CancellationToken;

use crate::{config, db, service::entities, service::StateProvider};

static ETCHING_INDEXER_ID: &str = "rune_etchings";

pub struct TxInfo {
    pub block: i64,
    pub tx_n: i32,
    pub txid: String,
    pub timestamp: i64,
    pub tx: Transaction,
}

#[derive(Default, Debug, Clone)]
struct RuneTxsStats {
    etches: u64,
    invalid_etches: u64,
    edicts: u64,
    invalid_edicts: u64,
    mints: u64,
    invalid_mints: u64,
    burned_txs: u64,
    cenotaphs: u64,
}

pub struct EtchingIndexer {
    net: bitcoin::Network,
    cfg: config::IndexersConfig,
    rpc: Client,

    service_repo: StateProvider,
    pending_txs: HashSet<String>,
    filter_runes: bool,
    runes_watchlist: HashSet<String>,
    runes_ids_watchlist: HashSet<RuneId>,
}

#[derive(Debug, Clone, Default)]
struct Allocation {
    edict: u128,
    mint: u128,
    etching: u128,
}

impl EtchingIndexer {
    pub fn new(
        cfg: &config::BTCConfig,
        icfg: &config::IndexersConfig,
        service_repo: StateProvider,
    ) -> Self {
        let net = cfg.get_network();
        let rpc = Client::new(
            &cfg.address,
            Auth::UserPass(cfg.rpc_user.clone(), cfg.rpc_password.clone()),
        )
        .unwrap();

        Self {
            net,
            cfg: icfg.clone(),
            rpc,
            service_repo,
            pending_txs: HashSet::new(),
            runes_ids_watchlist: HashSet::new(),
            runes_watchlist: HashSet::new(),
            filter_runes: !icfg.runes_watchlist.is_empty(),
        }
    }

    pub fn start(self, cancel: CancellationToken) -> JoinHandle<()> {
        // todo: use spawn_blocking
        tokio::spawn(self.run(cancel.clone()))
    }

    async fn run(self, stop_signal: CancellationToken) {
        let mut indexer = self;

        let last_block = match indexer
            .service_repo
            .db()
            .get_last_indexed_block(ETCHING_INDEXER_ID)
            .await
        {
            Ok(block) => block.height,
            Err(_) => 0,
        };

        let first_block = if last_block > indexer.cfg.runes_starting_height {
            last_block
        } else {
            indexer.cfg.runes_starting_height
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

        if indexer.filter_runes {
            for rune_name in indexer.cfg.runes_watchlist.iter() {
                match indexer.service_repo.db().get_rune(rune_name).await {
                    Ok(rune) => {
                        indexer.runes_watchlist.insert(rune_name.clone());
                        indexer.runes_ids_watchlist.insert(RuneId {
                            block: rune.block as u64,
                            tx: rune.tx_id as u32,
                        });
                    }
                    Err(err) => {
                        error!("Can't get rune({}) to filter by error={}", rune_name, err);
                        error!("Indexing stopped");
                        return;
                    }
                }
            }
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

            if let Some((hash, tx_count, stats)) = indexer.index_block(current_block).await {
                match indexer
                    .service_repo
                    .db()
                    .update_last_indexed_block(current_block, ETCHING_INDEXER_ID)
                    .await
                {
                    Ok(_) => (),
                    Err(err) => {
                        error!("Can't get BTC block error={}, hash={}", err, hash);
                    }
                };
                info!(
                    "Processed new block: height={} hash={} tx_count={}",
                    current_block, hash, tx_count
                );
                info!("Block stats: {:?}", stats);

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
            };
        }
    }

    async fn index_block(&mut self, height: i64) -> Option<(String, usize, RuneTxsStats)> {
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

        self.fetch_pending_txs().await;

        let mut stats = RuneTxsStats::default();
        for (txi, tx) in block.txdata.iter().enumerate() {
            let tx_info = TxInfo {
                block: height,
                tx_n: txi as i32,
                txid: tx.txid().to_string(),
                tx: tx.clone(),
                timestamp: block.header.time as i64,
            };

            if tx.is_coin_base() {
                continue;
            }

            self.extract_runestone(&tx_info, &mut stats).await;

            self.check_pending_txs(&tx_info).await;
        }

        Some((block_hash.to_string(), block.txdata.len(), stats))
    }

    async fn fetch_pending_txs(&mut self) {
        let Ok(tx_list) = self.service_repo.db().select_pending_txs().await else {
            error!("failed to select pending txs");
            return;
        };

        for tx in tx_list.iter() {
            self.pending_txs.insert(tx.tx_hash.clone());
        }
    }

    async fn check_pending_txs(&mut self, tx_info: &TxInfo) {
        if !self.pending_txs.contains(&tx_info.txid) {}
        // todo
    }

    pub async fn process_tx(&mut self, tx_hash: &str) -> anyhow::Result<()> {
        let tx_id: Txid = Txid::from_str(tx_hash)?;
        let tx_info = self.rpc.get_raw_transaction_info(&tx_id, None)?;
        let block_hash = tx_info.blockhash.unwrap();
        let header_info = self.rpc.get_block_header_info(&block_hash)?;

        let height = header_info.height;

        let block = self.rpc.get_block(&block_hash)?;
        for (txn, tx) in block.txdata.iter().enumerate() {
            if tx.txid().to_string().as_str() != tx_hash {
                continue;
            }

            self.extract_runestone(
                &TxInfo {
                    block: height as i64,
                    tx_n: txn as i32,
                    txid: tx_hash.to_owned(),
                    timestamp: block.header.time as i64,
                    tx: tx.clone(),
                },
                &mut RuneTxsStats::default(),
            )
            .await;
        }

        Ok(())
    }

    async fn extract_runestone(&mut self, tx_info: &TxInfo, stats: &mut RuneTxsStats) {
        let first_rune_height = ordinals::Rune::first_rune_height(self.net);
        if (first_rune_height as i64) > tx_info.block {
            return;
        }

        let input_runes_amounts = self.collect_and_spend_runes_inputs(&tx_info.tx).await;
        let mut allocated_runes: Vec<HashMap<String, Allocation>> =
            vec![HashMap::new(); tx_info.tx.output.len()];

        let artifact = match Runestone::decipher(&tx_info.tx) {
            Some(a) => a,
            None => {
                self.burn_all_inputs(tx_info, input_runes_amounts).await;
                return;
            }
        };

        match artifact {
            Artifact::Cenotaph(cenotaph) => {
                debug!(
                    "CENOTAPH was made: block={}:{} tx={} {:?}",
                    tx_info.block, tx_info.tx_n, tx_info.txid, cenotaph
                );
                stats.cenotaphs += 1;
                stats.burned_txs += 1;
                self.burn_all_inputs(tx_info, input_runes_amounts).await;
            }
            Artifact::Runestone(runestone) => {
                if !self.filter_runes && runestone.etching.is_some() {
                    if !self
                        .handle_rune_etching(tx_info, &runestone, &mut allocated_runes)
                        .await
                    {
                        stats.invalid_etches += 1;
                        stats.burned_txs += 1;
                        self.burn_all_inputs(tx_info, input_runes_amounts).await;
                        return;
                    };
                    stats.etches += 1;
                }
                if let Some(mint) = runestone.mint {
                    if !self
                        .handle_mint(tx_info, mint, runestone.pointer, &mut allocated_runes)
                        .await
                    {
                        stats.invalid_mints += 1;
                        stats.burned_txs += 1;

                        self.burn_all_inputs(tx_info, input_runes_amounts).await;
                        return;
                    };
                    stats.mints += 1;
                }

                if !runestone.edicts.is_empty() {
                    let len = runestone.edicts.len() as u64;
                    if !self
                        .handle_rune_edicts(tx_info, runestone.edicts, &mut allocated_runes)
                        .await
                    {
                        stats.invalid_edicts += len;
                        stats.burned_txs += 1;

                        self.burn_all_inputs(tx_info, input_runes_amounts).await;
                        return;
                    };

                    stats.edicts += len;
                }

                if !self
                    .apply_allocations(
                        &input_runes_amounts,
                        &allocated_runes,
                        tx_info,
                        runestone.pointer,
                    )
                    .await
                {
                    stats.burned_txs += 1;
                    self.burn_all_inputs(tx_info, input_runes_amounts).await;
                }
            }
        }
    }

    async fn handle_rune_etching(
        &mut self,
        tx_info: &TxInfo,
        runestone: &Runestone,
        allocated_runes: &mut [HashMap<String, Allocation>],
    ) -> bool {
        let Some(etching) = runestone.etching else {
            return false;
        };

        let (commitment_tx, rune) = if let Some(rune) = etching.rune {
            let height = ordinals::Height(tx_info.block as u32);
            let minimum = ordinals::Rune::minimum_at_height(self.net, height);

            let Some(comitment_tx) = self.validate_commitment(tx_info, rune) else {
                return false;
            };

            if rune < minimum || rune.is_reserved() {
                return false;
            }

            (comitment_tx, rune)
        } else {
            (
                "".to_string(),
                ordinals::Rune::reserved(tx_info.block as u64, tx_info.tx_n as u32),
            )
        };

        if self
            .service_repo
            .db()
            .get_rune(rune.to_string().as_str())
            .await
            .is_ok()
        {
            warn!(
                "Rune with such name({}) already exists. Invalid etching block={}:{}",
                rune, tx_info.block, tx_info.tx_n
            );
            return false;
        };

        debug!(
            "RUNE({}) was etched: rune_id={}:{} tx={}",
            rune, tx_info.block, tx_info.tx_n, tx_info.txid,
        );

        debug!(
            "RUNE({}) etching -> has_pointer={}, has_edicts={} has_mint={}",
            rune,
            runestone.pointer.is_some(),
            !runestone.edicts.is_empty(),
            runestone.mint.is_some()
        );

        let display_name = SpacedRune {
            rune,
            spacers: etching.spacers.unwrap_or_default(),
        };

        let max_supply = etching.supply().unwrap_or_default();
        let premine = etching.premine.unwrap_or_default();

        let rune_row = db::Rune {
            id: 0,
            rune: rune.to_string(),
            display_name: display_name.to_string(),
            symbol: etching.symbol.unwrap_or('¤').to_string(),
            block: tx_info.block,
            tx_id: tx_info.tx_n,
            mints: 0,
            max_supply: max_supply.to_string(),
            minted: premine.to_string(),
            premine: premine.to_string(),
            burned: "0".to_string(),
            in_circulation: premine.to_string(),
            divisibility: etching.divisibility.unwrap_or_default() as i32,
            turbo: etching.turbo,
            timestamp: tx_info.timestamp,
            etching_tx: tx_info.txid.to_string(),
            commitment_tx,
            raw_data: runestone.encipher().into_bytes(),
        };

        if let Err(err) = self.service_repo.store_new_rune(&rune_row).await {
            error!("Can't insert rune: error={} rune={:?}", err, rune_row);
            return true;
        }

        if premine == 0 {
            return true;
        }

        if let Some(vout) = extract_premine_address(runestone, &tx_info.tx) {
            let al = allocated_runes[vout as usize]
                .entry(rune_row.rune.clone())
                .or_default();
            al.etching += premine;
            return true;
        }
        if runestone.edicts.is_empty() {
            return false;
        }

        let mut has_some_outs = false;
        for edict in runestone.edicts.iter() {
            if edict.id.block != 0 || edict.id.tx != 0 {
                continue;
            }
            has_some_outs = true;
            if edict.output as usize == tx_info.tx.output.len() {
                // note that this allows `output == tx.output.len()`, which means to divide
                // amount between all non-OP_RETURN outputs
                let outs = get_non_opreturn_outputs(&tx_info.tx);

                let amount = edict.amount / outs.len() as u128;
                for (vout, _out) in outs.iter() {
                    let al = allocated_runes[*vout as usize]
                        .entry(rune.to_string())
                        .or_default();
                    al.etching += amount;
                }
            } else {
                let vout = edict.output;

                let al = allocated_runes[vout as usize]
                    .entry(rune.to_string())
                    .or_default();
                al.etching += edict.amount;
            }
        }

        has_some_outs
    }

    async fn handle_mint(
        &mut self,
        tx_info: &TxInfo,
        rune_id: RuneId,
        pointer: Option<u32>,
        allocated_runes: &mut [HashMap<String, Allocation>],
    ) -> bool {
        debug!(
            "RUNE was minted: block={}:{} tx={} {:?}:{:?}",
            tx_info.block, tx_info.tx_n, tx_info.txid, rune_id, pointer,
        );

        if self.filter_runes && !self.runes_ids_watchlist.contains(&rune_id) {
            return false;
        }

        let Ok(mut rune_info) = self
            .service_repo
            .get_rune_by_id(rune_id.block as i64, rune_id.tx as i32)
            .await
        else {
            return false;
        };

        let Some(terms) = rune_info.terms else {
            return false;
        };

        let amount = terms.amount.unwrap_or_default();
        let Some(vout) = get_change_output(&tx_info.tx, pointer) else {
            warn!(
                "RUNE mint tx has no change output block={}:{} tx={}",
                tx_info.block, tx_info.tx_n, tx_info.txid
            );
            return false;
        };

        rune_info.add_mint(amount);
        let _ = self.service_repo.update_rune_mint(&rune_info).await;

        let al = allocated_runes[vout as usize]
            .entry(rune_info.rune.clone())
            .or_default();
        al.mint += amount;
        true
    }

    async fn handle_rune_edicts(
        &mut self,
        tx_info: &TxInfo,
        edicts: Vec<Edict>,
        allocated_runes: &mut [HashMap<String, Allocation>],
    ) -> bool {
        for edict in edicts.iter() {
            if edict.id.block == 0 && edict.id.tx == 0 {
                // this is special edict related to etching
                continue;
            }
            debug!(
                "RUNE edict: block={} tx={} {:?}",
                tx_info.block, tx_info.tx_n, edict
            );

            if self.filter_runes && !self.runes_ids_watchlist.contains(&edict.id) {
                return false;
            }

            let Some(rune) = self.service_repo.get_rune_name_by_id(&edict.id).await else {
                error!(
                    "RUNE is not in cache! edict action {:?} block={}:{}",
                    edict, tx_info.block, tx_info.tx_n
                );
                return false;
            };

            if edict.output as usize == tx_info.tx.output.len() {
                // note that this allows `output == tx.output.len()`, which means to divide
                // amount between all non-OP_RETURN outputs
                let outs = get_non_opreturn_outputs(&tx_info.tx);

                let amount = edict.amount / outs.len() as u128;
                for (vout, _out) in outs.iter() {
                    let al = allocated_runes[*vout as usize]
                        .entry(rune.clone())
                        .or_default();
                    al.edict += amount;
                }
            } else {
                let vout = edict.output;

                let al = allocated_runes[vout as usize]
                    .entry(rune.clone())
                    .or_default();
                al.edict += edict.amount;
            }
        }

        true
    }

    async fn apply_allocations(
        &mut self,
        unalocated_runes: &HashMap<String, u128>,
        allocated_runes: &[HashMap<String, Allocation>],
        tx_info: &TxInfo,
        pointer: Option<u32>,
    ) -> bool {
        {
            let mut total_out: HashMap<String, u128> = HashMap::new();

            for a in allocated_runes.iter() {
                if a.is_empty() {
                    continue;
                }
                a.iter()
                    .for_each(|(k, al)| *total_out.entry(k.to_owned()).or_default() += al.edict)
            }

            for (k, out_value) in total_out.iter() {
                let in_value = match unalocated_runes.get(k) {
                    Some(b) => *b,
                    None => 0,
                };

                if *out_value > in_value {
                    debug!(
                        "trying to spend more than have {} out={} > in={}",
                        k, out_value, in_value
                    );
                    // trying to spend more than have
                    return false;
                }
            }
        }

        let mut unalocated_runes = unalocated_runes.clone();
        for (vout, a) in allocated_runes.iter().enumerate() {
            if a.is_empty() {
                continue;
            }

            let out = &tx_info.tx.output[vout];
            let address = match Address::from_script(&out.script_pubkey, self.net) {
                Ok(a) => a,
                Err(err) => {
                    error!("invalid allocation address: vout={} err={}", vout, err);
                    return false;
                }
            };

            for (rune, al) in a.iter() {
                let rune_utxo = entities::RuneUtxo {
                    block: tx_info.block,
                    tx_id: tx_info.tx_n,
                    tx_hash: tx_info.txid.clone(),
                    output_n: vout as i32,
                    rune: rune.clone(),
                    address: address.to_string(),
                    pk_script: out.script_pubkey.to_hex_string(),
                    amount: al.edict + al.mint + al.etching,
                    btc_amount: out.value as i64,
                    spend: false,
                };

                let action = if al.etching > 0 {
                    db::RuneLog::ETCHING
                } else if al.mint > 0 {
                    db::RuneLog::MINT
                } else {
                    db::RuneLog::INCOME
                };

                if let Err(err) = self
                    .service_repo
                    .store_new_runes_utxo(&rune_utxo, action)
                    .await
                {
                    error!("Failed to insert the rune utxo: error={}", err);
                }

                *unalocated_runes.entry(rune.to_owned()).or_default() -= al.edict;
            }
        }

        let Some(vout) = get_change_output(&tx_info.tx, pointer) else {
            debug!(
                "RUNE mint tx has no change output block={}:{} tx={}",
                tx_info.block, tx_info.tx_n, tx_info.txid
            );
            return false;
        };

        let out = &tx_info.tx.output[vout as usize];
        let address = match Address::from_script(&out.script_pubkey, self.net) {
            Ok(a) => a,
            Err(err) => {
                error!("can't parse change address: error={}", err);
                return false;
            }
        };

        for (rune, amount) in unalocated_runes.iter() {
            if *amount == 0 {
                continue;
            }

            debug!("Tx change {} {} goes to {}", rune, amount, address);
            let rune_utxo = entities::RuneUtxo {
                block: tx_info.block,
                tx_id: tx_info.tx_n,
                tx_hash: tx_info.txid.clone(),
                output_n: vout as i32,
                rune: rune.clone(),
                address: address.to_string(),
                pk_script: out.script_pubkey.to_hex_string(),
                amount: *amount,
                btc_amount: out.value as i64,
                spend: false,
            };

            if let Err(err) = self
                .service_repo
                .store_new_runes_utxo(&rune_utxo, db::RuneLog::INCOME)
                .await
            {
                error!("Failed to insert the rune utxo: error={}", err);
            }
        }

        true
    }

    async fn burn_all_inputs(
        &mut self,
        _tx_info: &TxInfo,
        input_runes_amounts: HashMap<String, u128>,
    ) {
        for (rune, amount) in input_runes_amounts.iter() {
            if let Err(err) = self.service_repo.burn_rune(rune, *amount).await {
                error!("Can't burn rune {} {} error={}", rune, amount, err);
            };
        }
    }

    async fn collect_and_spend_runes_inputs(&mut self, tx: &Transaction) -> HashMap<String, u128> {
        let mut input_amounts: HashMap<String, u128> = HashMap::new();

        for input in tx.input.iter() {
            // it doesn't matter whether this burn or
            // not we can mark inputs as spent and decrease balances
            let Some(utxo_list) = self
                .service_repo
                .spent_rune_utxo(input, tx.txid().to_string().as_str())
                .await
            else {
                continue;
            };
            for utxo in utxo_list.iter() {
                let value = input_amounts.entry(utxo.rune.clone()).or_default();
                *value += utxo.amount;
            }
        }

        input_amounts
    }

    fn validate_commitment(&self, tx_info: &TxInfo, rune: ordinals::Rune) -> Option<String> {
        let commitment = rune.commitment();

        for input in &tx_info.tx.input {
            // extracting a tapscript does not indicate that the input being spent
            // was actually a taproot output. this is checked below, when we load the
            // output's entry from the database
            let Some(tapscript) = input.witness.tapscript() else {
                continue;
            };

            for instruction in tapscript.instructions() {
                // ignore errors, since the extracted script may not be valid
                let Ok(instruction) = instruction else {
                    break;
                };

                let Some(pushbytes) = instruction.push_bytes() else {
                    continue;
                };

                if pushbytes.as_bytes() != commitment {
                    continue;
                }
                let commitment_tx = input.previous_output.txid;
                let commitment_tx_info = {
                    let res = self
                        .rpc
                        .get_raw_transaction_info(&input.previous_output.txid, None);
                    match res {
                        Ok(info) => info,
                        Err(err) => {
                            error!(
                                "Can't get parent_tx({}) for etching_tx({}) error={}",
                                input.previous_output.txid, tx_info.txid, err,
                            );
                            return None;
                        }
                    }
                };

                let taproot = commitment_tx_info.vout[input.previous_output.vout as usize]
                    .script_pub_key
                    .script()
                    .unwrap_or_default()
                    .is_v1_p2tr();

                if !taproot {
                    continue;
                }

                let commit_tx_height = match self
                    .rpc
                    .get_block_header_info(&commitment_tx_info.blockhash.unwrap())
                {
                    Ok(bh) => bh.height,
                    Err(err) => {
                        error!(
                            "Can't get block with commitment_tx({}) err={}",
                            commitment_tx, err
                        );
                        return None;
                    }
                };

                let confirmations = tx_info.block - commit_tx_height as i64 + 1;

                if confirmations >= Runestone::COMMIT_CONFIRMATIONS.into() {
                    return Some(commitment_tx.to_string());
                }
            }
        }

        None
    }
}

fn extract_premine_address(runestone: &Runestone, tx: &Transaction) -> Option<u32> {
    if let Some(pointer) = runestone.pointer {
        if (pointer as usize) > tx.output.len() {
            return None;
        }
        return Some(pointer);
    }

    let mut rune_out_found = false;
    for (vout, out) in tx.output.iter().enumerate() {
        let mut instructions = out.script_pubkey.instructions();

        // payload starts with OP_RETURN
        if instructions.next() != Some(Ok(Instruction::Op(opcodes::all::OP_RETURN))) {
            continue;
        }

        // followed by the protocol identifier, ignoring errors, since OP_RETURN
        // scripts may be invalid
        if instructions.next() != Some(Ok(Instruction::Op(Runestone::MAGIC_NUMBER))) {
            rune_out_found = true;
            continue;
        }

        if rune_out_found {
            return Some(vout as u32);
        }
    }

    None
}

fn get_change_output(tx: &Transaction, pointer: Option<u32>) -> Option<u32> {
    if let Some(pointer) = pointer {
        if (pointer as usize) > tx.output.len() {
            return None;
        }
        return Some(pointer);
    }

    for (id, out) in tx.output.iter().enumerate() {
        let mut instructions = out.script_pubkey.instructions();
        // payload starts with OP_RETURN
        if instructions.next() == Some(Ok(Instruction::Op(opcodes::all::OP_RETURN))) {
            continue;
        }

        return Some(id as u32);
    }

    None
}

fn get_non_opreturn_outputs(tx: &Transaction) -> Vec<(u32, TxOut)> {
    let mut res = Vec::new();

    for (id, out) in tx.output.iter().enumerate() {
        let mut instructions = out.script_pubkey.instructions();
        // payload starts with OP_RETURN
        if instructions.next() == Some(Ok(Instruction::Op(opcodes::all::OP_RETURN))) {
            continue;
        }

        res.push((id as u32, out.clone()));
    }

    res
}
