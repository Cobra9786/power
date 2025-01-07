use bitcoin::{
    absolute::LockTime, script::Builder, Address, Amount, OutPoint, ScriptBuf, Sequence,
    Transaction, TxIn, TxOut, Txid, Witness,
};
use bitcoincore_rpc::{Auth, Client, RawTx, RpcApi};
use ordinals::{Edict, RuneId, Runestone};
use std::{collections::HashSet, str::FromStr};

use crate::{
    db,
    tx::{
        runes_txs,
        signer::{AddressMode, PKSigner},
    },
};

#[derive(Debug, clap::Parser)]
pub struct BtcTxCmd {
    #[arg(long)]
    dest_address: Vec<String>,

    #[arg(long)]
    amount: u64,

    #[arg(long, default_value_t = 42.0)]
    fee: f64,

    #[arg(long, default_value_t = false)]
    submit: bool,
}

impl BtcTxCmd {
    pub async fn run(&self, config_path: &str) -> anyhow::Result<()> {
        let cfg = crate::config::read_config(config_path)?;
        let repo = db::open_postgres_db(cfg.db).await?;
        let net = cfg.btc.get_network();
        let signer = PKSigner::new_from_secret(
            net,
            &cfg.signature_provider.local.secret_key,
            AddressMode::new_from_str(&cfg.signature_provider.local.mode),
        )?;

        println!("{}", signer.address);

        let utxo = repo.select_btc_utxo(&signer.address.to_string()).await?;

        let mut inputs = Vec::new();
        let mut parent_outs = Vec::new();
        let mut outputs = Vec::new();
        let mut total_amount: u64 = 0;

        println!(
            "Selected {} UTXOs. Amount to send -> {}",
            utxo.len(),
            self.amount
        );

        for u in utxo {
            if total_amount > self.amount {
                break;
            }

            total_amount += u.amount as u64;

            parent_outs.push(TxOut {
                script_pubkey: ScriptBuf::from_hex(&u.pk_script)?,
                value: u.amount as u64,
            });

            inputs.push(TxIn {
                previous_output: OutPoint {
                    txid: Txid::from_str(&u.tx_hash)?,
                    vout: u.output_n as u32,
                },
                script_sig: Builder::new().into_script(),
                witness: Witness::new(),
                sequence: Sequence::ZERO,
            });
        }

        let base_out_val = self.amount / self.dest_address.len() as u64;
        for addr in self.dest_address.clone() {
            let address = Address::from_str(&addr)?.require_network(net)?;

            outputs.push(TxOut {
                script_pubkey: address.script_pubkey(),
                value: base_out_val,
            });
        }

        let mut tx = Transaction {
            version: 2,
            lock_time: LockTime::ZERO,
            input: inputs,
            output: outputs,
        };

        let fee_val = fee(self.fee, tx.vsize()).to_sat();

        println!("{} {}", total_amount, fee_val);

        if total_amount < self.amount + fee_val {
            error!(
                "BUG: to small input amount. in={} amount_to_send={} fee={}",
                total_amount, self.amount, fee_val
            );
            return Ok(());
        }

        let change_value = total_amount - (self.amount + fee_val);
        if change_value > 800 {
            tx.output.push(TxOut {
                value: change_value,
                script_pubkey: signer.address.script_pubkey(),
            })
        }

        println!(
            "PREPARING TX: -> size={} in={} fee={} out={}",
            tx.vsize(),
            total_amount,
            fee_val,
            self.amount + change_value,
        );

        let signed_tx = signer.sign_tx(&tx, parent_outs)?;

        println!("TX READY ->> {} {}", signed_tx.txid(), signed_tx.raw_hex());
        println!(
            "TX STATS: -> size={} in={} fee={} out={}",
            signed_tx.vsize(),
            total_amount,
            fee_val,
            self.amount + change_value,
        );

        if self.submit {
            let rpc = Client::new(
                &cfg.btc.address,
                Auth::UserPass(cfg.btc.rpc_user.clone(), cfg.btc.rpc_password.clone()),
            )?;

            let tx_id = rpc.send_raw_transaction(signed_tx.raw_hex())?;
            println!("TX ID ->> {}", tx_id);
        }

        Ok(())
    }
}

pub fn fee(fee_rate: f64, vsize: usize) -> Amount {
    Amount::from_sat((fee_rate * vsize as f64).round() as u64)
}

#[derive(Debug, clap::Parser)]
pub struct SubmitRawTxCmd {
    #[arg(long)]
    tx: String,
}

impl SubmitRawTxCmd {
    pub async fn run(&self, cfg_path: &str) -> anyhow::Result<()> {
        let cfg = crate::config::read_config(cfg_path)?;

        let rpc = Client::new(
            &cfg.btc.address,
            Auth::UserPass(cfg.btc.rpc_user.clone(), cfg.btc.rpc_password.clone()),
        )?;

        let tx_id = rpc.send_raw_transaction(self.tx.clone())?;
        println!("TX ID ->> {}", tx_id);

        Ok(())
    }
}

#[derive(Debug, clap::Parser)]
pub struct SendRuneTxCmd {
    #[arg(long)]
    dest_address: Vec<String>,

    #[arg(long)]
    rune: String,

    #[arg(long)]
    amount: u128,

    #[arg(long, default_value_t = 42.0)]
    fee: f64,

    #[arg(long, default_value_t = false)]
    submit: bool,
}

impl SendRuneTxCmd {
    pub async fn run(&self, config_path: &str) -> anyhow::Result<()> {
        let cfg = crate::config::read_config(config_path)?;
        let repo = db::open_postgres_db(cfg.db).await?;
        let net = cfg.btc.get_network();
        let signer = PKSigner::new_from_secret(
            net,
            &cfg.signature_provider.local.secret_key,
            AddressMode::new_from_str(&cfg.signature_provider.local.mode),
        )?;

        println!("Send {} runes form {}", self.rune, signer.address);
        let rune_info = repo.get_rune(&self.rune).await?;

        println!("RUNE EXIST");

        let runes_utxo = repo
            .select_runes_utxo_with_pagination(
                &self.rune,
                Some(signer.address.to_string()),
                "ASC",
                100,
                0,
            )
            .await?;

        println!(
            "Selected {} UTXOs. Amount to send -> {}",
            runes_utxo.len(),
            self.amount
        );

        let mut tx = Transaction {
            version: 2,
            lock_time: LockTime::ZERO,
            input: Vec::new(),
            output: vec![
                // it will be OP_RETURN 13 magic
                TxOut {
                    value: 0,
                    script_pubkey: ScriptBuf::new(),
                },
            ],
        };

        let mut parent_outs = Vec::new();
        let mut runes_in_amount: u128 = 0;
        let mut btc_in_amount: u64 = 0;
        let mut btc_input_set: HashSet<OutPoint> = HashSet::new();

        for u in runes_utxo {
            if runes_in_amount > self.amount {
                break;
            }

            runes_in_amount += u128::from_str(&u.amount).unwrap();
            btc_in_amount += u.btc_amount as u64;

            parent_outs.push(TxOut {
                script_pubkey: ScriptBuf::from_hex(&u.pk_script)?,
                value: u.btc_amount as u64,
            });
            let op = OutPoint {
                txid: Txid::from_str(&u.tx_hash)?,
                vout: u.output_n as u32,
            };

            tx.input.push(TxIn {
                previous_output: op,
                script_sig: Builder::new().into_script(),
                witness: Witness::new(),
                sequence: Sequence::ZERO,
            });

            btc_input_set.insert(op);
        }

        let mut btc_out_amount = runes_txs::RUNES_OUT_VALUE * self.dest_address.len() as u64;
        let rune_amount_per_out = self.amount / self.dest_address.len() as u128;

        let mut edicts: Vec<Edict> = Vec::new();
        for (id, addr) in self.dest_address.clone().iter().enumerate() {
            let address = Address::from_str(addr)?.require_network(net)?;

            edicts.push(Edict {
                id: RuneId {
                    block: rune_info.block as u64,
                    tx: rune_info.tx_id as u32,
                },
                amount: rune_amount_per_out,
                output: id as u32 + 1,
            });

            tx.output.push(TxOut {
                script_pubkey: address.script_pubkey(),
                value: runes_txs::RUNES_OUT_VALUE,
            });
        }

        let mut pointer: Option<u32> = None;
        if self.amount < runes_in_amount {
            btc_out_amount += runes_txs::RUNES_OUT_VALUE;
            tx.output.push(TxOut {
                value: runes_txs::RUNES_OUT_VALUE,
                script_pubkey: signer.address.script_pubkey(),
            });

            pointer = Some(tx.output.len() as u32);
        }

        let runestone = Runestone {
            edicts,
            etching: None,
            mint: None,
            pointer,
        };

        tx.output[0].script_pubkey = runestone.encipher();

        let fee_val = (fee(self.fee, tx.vsize()).to_sat() as f64 * 1.86) as u64; // TODO: fix fee estimation

        let btc_utxo = repo
            .select_btc_utxo_with_pagination(Some(signer.address.to_string()), "ASC", 20, 0)
            .await?;

        for u in btc_utxo.iter() {
            if btc_in_amount > btc_out_amount + fee_val {
                break;
            }

            let op = OutPoint {
                txid: Txid::from_str(&u.tx_hash)?,
                vout: u.output_n as u32,
            };

            if btc_input_set.contains(&op) {
                continue;
            }
            btc_in_amount += u.amount as u64;
            tx.input.push(TxIn {
                previous_output: op,
                script_sig: Builder::new().into_script(),
                witness: Witness::new(),
                sequence: Sequence::ZERO,
            });

            parent_outs.push(TxOut {
                script_pubkey: ScriptBuf::from_hex(&u.pk_script)?,
                value: u.amount as u64,
            });
        }

        if btc_in_amount < btc_out_amount + fee_val {
            error!(
                "BUG: to small input amount. in={} amount_to_send={} fee={}",
                btc_in_amount, self.amount, fee_val
            );
            return Ok(());
        }

        let btc_change_value = btc_in_amount - (btc_out_amount + fee_val);
        if btc_change_value > 800 {
            tx.output.push(TxOut {
                value: btc_change_value,
                script_pubkey: signer.address.script_pubkey(),
            })
        }

        println!(
            "PREPARING TX: -> size={} in={} fee={} out={}",
            tx.vsize(),
            btc_in_amount,
            fee_val,
            btc_out_amount + btc_change_value,
        );

        let signed_tx = signer.sign_tx(&tx, parent_outs)?;

        println!("TX READY ->> {} {}", signed_tx.txid(), signed_tx.raw_hex());
        println!(
            "TX STATS: -> size={} btc_in={} fee={} btc_out={} rune_in={} rune_out={} rune_change={}",
            signed_tx.vsize(),
            btc_in_amount,
            fee_val,
            btc_out_amount + btc_change_value,
           runes_in_amount,
            self.amount, runes_in_amount - self.amount,
        );

        if self.submit {
            let rpc = Client::new(
                &cfg.btc.address,
                Auth::UserPass(cfg.btc.rpc_user.clone(), cfg.btc.rpc_password.clone()),
            )?;

            let tx_id = rpc.send_raw_transaction(signed_tx.raw_hex())?;
            println!("TX ID ->> {}", tx_id);
        } else {
            let runestone = Runestone::decipher(&signed_tx).unwrap();
            print!("RUNESTONE ->> {:#?}", runestone);
        }

        Ok(())
    }
}
