use std::str::FromStr;

use bitcoin::{Network, ScriptBuf, Txid};
use bitcoincore_rpc::{bitcoin, Auth, Client, RawTx, RpcApi};
use clap::Parser;
use ordinals::{Etching, Rune, SpacedRune, Terms};

use crate::{
    db,
    tx::runes_txs::{RunesTxBuilder, COMMITMENT_OUT_VALUE},
    tx::signer::{AddressMode, PKSigner},
    tx::utxo::Utxo,
};

#[derive(Debug, serde::Deserialize)]
struct RuneCSVRow {
    name: String,
    symbol: String,
    total_supply: u64,
}

#[derive(Debug, Parser)]
pub struct EtchingCmd {
    /// Path to file with runes to etch
    #[arg(short, long)]
    input_file: String,

    #[arg(long, default_value_t = 42.0)]
    fee: f64,

    #[arg(long, default_value_t = false)]
    submit: bool,

    #[arg(long, default_value_t = false)]
    submit_etch: bool,
}

impl EtchingCmd {
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
        if !self.input_file.is_empty() {
            let _etching_list = extract_etching_list(&self.input_file)?;
        }

        println!();

        let utxo = repo.select_btc_utxo(&signer.address.to_string()).await?;

        let mut etching = csv_to_etching(RuneCSVRow {
            name: "BOB•MINTING•BLOODY•RUNES".to_string(),
            symbol: "".to_string(),
            total_supply: 100000000000,
        })
        .unwrap();
        etching.terms = Some(Terms {
            amount: Some(1),
            cap: Some(200000000000),
            height: (Some(1), Some(1000005)),
            offset: (None, None),
        });

        let etching_list = vec![etching];

        let change_address = signer.address.clone();
        let commitment_pubkey = signer.xonly_pubkey();
        let builder = RunesTxBuilder::new(signer.net, commitment_pubkey, change_address, self.fee);
        let utxo = utxo
            .iter()
            .map(|e| Utxo {
                txid: Txid::from_str(&e.tx_hash).unwrap(),
                vout: e.output_n as u32,
                value: e.amount as u64,
                script_pubkey: ScriptBuf::from_hex(&e.pk_script).unwrap(),
            })
            .collect::<Vec<Utxo>>();

        let (unsigned_commit_tx, commit_tx_outs, parent_outs) =
            builder.create_commitment_tx(etching_list.clone(), utxo, COMMITMENT_OUT_VALUE);

        let commit_tx = signer.sign_tx(&unsigned_commit_tx, parent_outs)?;
        let commitment_txid = commit_tx.txid();

        println!("COMMIT TXID ->> {}", commit_tx.txid());
        println!("COMMIT TXID ->> {}", commitment_txid);
        println!("COMMIT RAW_TX ->> {}", commit_tx.raw_hex());

        println!();

        let rpc = Client::new(
            &cfg.btc.address,
            Auth::UserPass(cfg.btc.rpc_user.clone(), cfg.btc.rpc_password.clone()),
        )?;

        if self.submit {
            let tx_id = rpc.send_raw_transaction(commit_tx.raw_hex())?;
            println!("COMMIT TX ACCEPTED ->> {}", tx_id);
            // } else {
            // println!("{:#?}", commit_tx);
        }

        if self.submit_etch {
            loop {
                let tx_info = rpc.get_raw_transaction_info(&commitment_txid, None)?;
                println!(
                    "COMMITMENT_TX confirmations={}",
                    tx_info.confirmations.unwrap_or_default()
                );
                if tx_info.confirmations.unwrap_or_default() > 6 {
                    break;
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
            }
        }

        for etching in etching_list.iter() {
            let rune_name = etching.rune.unwrap().to_string().clone();
            println!("CREATE ETCHING_TX of the {} rune", rune_name);
            let commitment_out = commit_tx_outs.get(&rune_name).unwrap();

            let etching_tx = builder.create_etching_tx(
                etching,
                commitment_out.clone(),
                commitment_txid,
                signer.address.clone(),
            );
            println!(
                "COMMITMENT_ADDRESS ->> {}",
                commitment_out.commit_tx_address
            );

            let signed_etching_tx =
                builder.sign_etching_tx(&etching_tx, &signer.kp, commitment_out.clone(), 0);
            println!("ETCHING TXID ->> {}", signed_etching_tx.txid());
            println!("ETCHING RAW_TX ->> {}", signed_etching_tx.raw_hex());

            if self.submit_etch {
                let tx_id = rpc.send_raw_transaction(signed_etching_tx.raw_hex())?;
                println!("ETCHING TX ACCEPTED ->> {}", tx_id);
                // } else {
                // println!("{:#?}", signed_etching_tx);
            }
        }

        Ok(())
    }
}

fn extract_etching_list(path: &str) -> anyhow::Result<Vec<Etching>> {
    let mut reader = csv::Reader::from_path(path)?;

    let mut runes: Vec<Etching> = vec![];

    for row in reader.deserialize() {
        let rune_info: RuneCSVRow = row?;

        let Some(etch) = csv_to_etching(rune_info) else {
            continue;
        };
        runes.push(etch);
    }

    Ok(runes)
}

fn csv_to_etching(rune_info: RuneCSVRow) -> Option<Etching> {
    let mut rune_info = rune_info;
    rune_info.symbol = rune_info.symbol.replace(' ', "");
    if rune_info.symbol.chars().count() > 1 {
        println!(
            "invalid symbol -> '{}' {:?}",
            rune_info.name, rune_info.symbol
        );
        return None;
    }

    rune_info.name = rune_info.name.replace(' ', "");
    let sp = match SpacedRune::from_str(&rune_info.name) {
        Ok(sp) => sp,
        Err(err) => {
            println!("invalid name -> '{}', reason={}", rune_info.name, err);
            return None;
        }
    };

    let min_at_height = Rune::minimum_at_height(
        Network::Bitcoin,
        ordinals::Height(Rune::first_rune_height(Network::Bitcoin)),
    );
    if sp.rune < min_at_height {
        println!(
            "invalid name -> '{}' {}, reason=rune is less than minimum for next block",
            sp.rune,
            sp.rune.to_string().len()
        );
    }

    let etch = Etching {
        rune: Some(sp.rune),
        spacers: Some(sp.spacers),
        symbol: rune_info.symbol.chars().next(),
        premine: Some(rune_info.total_supply as u128),
        divisibility: Some(0),
        terms: None,
        turbo: true,
    };
    Some(etch)
}
