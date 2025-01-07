//#![allow(dead_code)]

#[macro_use]
extern crate log;

use clap::Parser;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

mod btc_utxo;
mod cache;
mod config;
mod db;
mod etcher;
mod indexer;
mod rest;
mod serde_utils;
mod service;
mod tx;
mod tx_cmd;
mod utils;

use rest::server::run_server;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// path to config file
    #[arg(short, long, default_value_t = String::from("config.toml"))]
    config: String,

    #[command(subcommand)]
    subcommand: Option<Subcommand>,
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let args = Args::parse();

    match args.subcommand {
        None => {
            let cfg = config::read_config(&args.config)?;
            run_app(cfg).await
        }
        Some(subcmd) => subcmd.run(&args.config).await,
    }
}

#[derive(Debug, Parser)]
enum Subcommand {
    #[command(about = "Etch new runes")]
    RunesEtching(etcher::EtchingCmd),

    #[command(about = "Send BTC transaction")]
    BtcTx(tx_cmd::BtcTxCmd),

    #[command(about = "Start API server only")]
    ApiServer,

    #[command(about = "Start indexer only")]
    Indexer,

    #[command(about = "Cleans all data from the index db")]
    ResetDB,

    #[command(about = "Generates new keypair")]
    GenKeypair,

    #[command(about = "Submit raw transaction")]
    SubmitRawTx(tx_cmd::SubmitRawTxCmd),

    #[command(about = "Send rune to address")]
    SendRunes(tx_cmd::SendRuneTxCmd),
    #[command(about = "Warm-up cache data")]
    WarmupCache,

    #[command(about = "test")]
    TestIndex,
}

impl Subcommand {
    async fn run(&self, cfg_path: &str) -> anyhow::Result<()> {
        match self {
            Subcommand::RunesEtching(cmd) => cmd.run(cfg_path).await,
            Subcommand::BtcTx(cmd) => cmd.run(cfg_path).await,
            Subcommand::SubmitRawTx(cmd) => cmd.run(cfg_path).await,
            Subcommand::SendRunes(cmd) => cmd.run(cfg_path).await,
            Subcommand::ApiServer => run_api_server(cfg_path).await,
            Subcommand::Indexer => run_indexer(cfg_path).await,
            Subcommand::ResetDB => reset_db(cfg_path).await,
            Subcommand::WarmupCache => warm_up_cache(cfg_path).await,
            Subcommand::GenKeypair => {
                generate_keypair().await;
                Ok(())
            }
            Subcommand::TestIndex => test_indexer(cfg_path).await,
        }
    }
}

async fn run_app(cfg: config::Config) -> anyhow::Result<()> {
    let repo: db::Repo = db::open_postgres_db(cfg.db).await?;
    let db = Arc::new(repo);
    let rcache = cache::CacheRepo::new(cfg.redis).await?;
    let service_state =
        service::StateProvider::new(db.clone(), rcache.clone(), cfg.indexers.disable_rune_log);

    let btc_indexer = indexer::BtcIndexer::new(&cfg.btc, &cfg.indexers, db.clone());
    let runes_indexer = indexer::EtchingIndexer::new(&cfg.btc, &cfg.indexers, service_state);

    let cancel = CancellationToken::new();

    let btc_handle = btc_indexer.start(cancel.clone());
    let indexer_handle = runes_indexer.start(cancel.clone());

    let signer = tx::signer::PKSigner::new_from_secret(
        cfg.btc.get_network(),
        &cfg.signature_provider.local.secret_key,
        tx::signer::AddressMode::new_from_str(&cfg.signature_provider.local.mode),
    )?;

    let btc_client = btc_utxo::UtxoClient::new(cfg.btc.utxo_provider.clone(), db.clone());
    let c = Arc::new(RwLock::new(rcache));
    let api_service = rest::api::Service::new(db.clone(), btc_client, cfg.btc.clone(), signer, c);
    let admin_api_service = rest::admin_api::Api::new(db.clone());

    match run_server(cfg.api, api_service, admin_api_service).await {
        Ok(_) => (),
        Err(err) => {
            error!("HTTP server failed: {:?}", err);
        }
    }
    // signal indexer task to stop running
    cancel.cancel();

    btc_handle.await.unwrap();
    indexer_handle.await.unwrap();

    log::info!("Application successfully shut down");

    Ok(())
}

async fn run_api_server(cfg_path: &str) -> anyhow::Result<()> {
    let cfg = config::read_config(cfg_path)?;
    let repo: db::Repo = db::open_postgres_db(cfg.db).await?;
    let db = Arc::new(repo);

    let signer = tx::signer::PKSigner::new_from_secret(
        cfg.btc.get_network(),
        &cfg.signature_provider.local.secret_key,
        tx::signer::AddressMode::new_from_str(&cfg.signature_provider.local.mode),
    )?;

    let cancel = CancellationToken::new();

    let tx_watchdog = service::tx_watchdog::TxWatchdog::new(&cfg.btc, db.clone());
    let watchdog_handle = tx_watchdog.start(cancel.clone());

    let btc_client = btc_utxo::UtxoClient::new(cfg.btc.utxo_provider.clone(), db.clone());
    let rcache = cache::CacheRepo::new(cfg.redis).await?;
    let c = Arc::new(RwLock::new(rcache));
    let api_service = rest::api::Service::new(db.clone(), btc_client, cfg.btc.clone(), signer, c);
    let admin_api_service = rest::admin_api::Api::new(db.clone());

    match run_server(cfg.api, api_service, admin_api_service).await {
        Ok(_) => (),
        Err(err) => {
            error!("HTTP server failed: {:?}", err);
        }
    }
    cancel.cancel();
    watchdog_handle.await.unwrap();

    log::info!("Application successfully shut down");

    Ok(())
}

async fn run_indexer(cfg_path: &str) -> anyhow::Result<()> {
    let cfg = config::read_config(cfg_path)?;

    let repo: db::Repo = db::open_postgres_db(cfg.db).await?;
    let db = Arc::new(repo);
    let rcache = cache::CacheRepo::new(cfg.redis).await?;
    let service_state =
        service::StateProvider::new(db.clone(), rcache, cfg.indexers.disable_rune_log);

    let btc_indexer = indexer::BtcIndexer::new(&cfg.btc, &cfg.indexers, db.clone());
    let runes_indexer = indexer::EtchingIndexer::new(&cfg.btc, &cfg.indexers, service_state);

    let cancel = CancellationToken::new();

    let btc_handle = btc_indexer.start(cancel.clone());
    let indexer_handle = runes_indexer.start(cancel.clone());

    tokio::signal::ctrl_c().await?;
    // signal indexer task to stop running
    cancel.cancel();

    btc_handle.await.unwrap();
    indexer_handle.await.unwrap();

    log::info!("Application successfully shut down");

    Ok(())
}

async fn reset_db(cfg_path: &str) -> anyhow::Result<()> {
    let mut cfg = config::read_config(cfg_path)?;
    cfg.db.automigrate = false;

    let repo: db::Repo = db::open_postgres_db(cfg.db).await?;

    repo.reset_schema().await?;
    repo.insert_seed_data().await?;

    for address in cfg.indexers.btc_watchlist {
        repo.insert_btc_balance(&address).await?;
    }

    let mut rcache = cache::CacheRepo::new(cfg.redis).await?;
    rcache.flush_all().await?;

    Ok(())
}

async fn warm_up_cache(cfg_path: &str) -> anyhow::Result<()> {
    let mut cfg = config::read_config(cfg_path)?;
    cfg.db.automigrate = false;

    let repo: db::Repo = db::open_postgres_db(cfg.db).await?;
    let db = Arc::new(repo);
    let rcache = cache::CacheRepo::new(cfg.redis).await?;
    let mut service_state =
        service::StateProvider::new(db.clone(), rcache, cfg.indexers.disable_rune_log);
    service_state.warm_up_cache().await?;

    Ok(())
}

async fn generate_keypair() {
    use bitcoin::secp256k1::{Secp256k1, SecretKey};
    use bitcoin::{key::KeyPair, key::UntweakedPublicKey, Address, PrivateKey};

    let secp = Secp256k1::new();
    let (secret_key, _) = secp.generate_keypair(&mut rand::thread_rng());
    let hex_secret = hex::encode(secret_key.secret_bytes());
    {
        println!("mainnet:");
        let pk = PrivateKey::new(secret_key, bitcoin::Network::Bitcoin);
        println!("  secret_key:\t{}", hex_secret);

        let address = Address::p2shwpkh(&pk.public_key(&secp), bitcoin::Network::Bitcoin).unwrap();
        println!("  p2shwpkh:\t{}", address);

        let kp = KeyPair::from_secret_key(&secp, &secret_key);
        let (untw_public_key, _) = UntweakedPublicKey::from_keypair(&kp);
        let address = Address::p2tr(&secp, untw_public_key, None, bitcoin::Network::Bitcoin);
        println!("  p2tr:    \t{}", address);
    }

    let data = hex::decode(hex_secret).unwrap();
    let recovered_secret = SecretKey::from_slice(&data).unwrap();
    {
        println!("testnet:");
        let pk = PrivateKey::new(recovered_secret, bitcoin::Network::Testnet);

        let address = Address::p2shwpkh(&pk.public_key(&secp), bitcoin::Network::Testnet).unwrap();
        println!("  p2shwpkh:\t{}", address);

        let kp = KeyPair::from_secret_key(&secp, &recovered_secret);
        let (untw_public_key, _) = UntweakedPublicKey::from_keypair(&kp);
        let address = Address::p2tr(&secp, untw_public_key, None, bitcoin::Network::Testnet);
        println!("  p2tr:    \t{}", address);
    }
    {
        println!("regtest");
        let pk = PrivateKey::new(recovered_secret, bitcoin::Network::Regtest);

        let address = Address::p2shwpkh(&pk.public_key(&secp), bitcoin::Network::Regtest).unwrap();
        println!("  p2shwpkh:\t{}", address);

        let kp = KeyPair::from_secret_key(&secp, &recovered_secret);
        let (untw_public_key, _) = UntweakedPublicKey::from_keypair(&kp);
        let address = Address::p2tr(&secp, untw_public_key, None, bitcoin::Network::Regtest);
        println!("  p2tr:    \t{}", address);
    }
}

async fn test_indexer(cfg_path: &str) -> anyhow::Result<()> {
    let mut cfg = config::read_config(cfg_path)?;
    cfg.indexers.runes_watchlist = Vec::new();

    let repo: db::Repo = db::open_postgres_db(cfg.db).await?;
    let db = Arc::new(repo);
    let rcache = cache::CacheRepo::new(cfg.redis).await?;
    let service_state =
        service::StateProvider::new(db.clone(), rcache, cfg.indexers.disable_rune_log);

    let mut runes_indexer = indexer::EtchingIndexer::new(&cfg.btc, &cfg.indexers, service_state);
    let txs = [
        //       "db163ceb4c7a29e5ae19422e5ff8d9e95106b526edb05a89178c71a97085e464",
        //        "a234999ee49a08e2180c286be5b9a2d6843e5ae6d6a3a247c539ab68e0c2d87e",
        //        "eb61fe7b3b1bff671368da52680b7ceaedde8a4b2e4515a154face1fed42fd06",
        //        "9bb71443a4db1b9a5cb2cc93306e99cc827aa3697c67038dfea22b661a78265c",
        //        "76f27c9fd7ec180da93a197e9f9113f85e46087f21aa011aed5fb4af652c5bc7",
        //        "714ff5e52cb87f37c22c65e0a88382a295c12eb76fb7c42a20b8f331a4d175d7",
        //        "7c3f237572ba92fdca987c80a2cf07aed5a6c1b3e944136e8137cbf03a520d58",
        //        "3153853571e43d3c290ceb325d8cb6d330fea733598ed2a2906914e7df200d6d",
        //        "a27c29d34cc0d1f7ffc83b50d05db9bf09db8fab638a1935b357a5f2c7b9e951",
        "7eaea16fd83205db18f8ae61f90620545cfd3749a7fa36093e97f390cda46511",
    ];

    for tx in txs.iter() {
        runes_indexer.process_tx(tx).await?;
    }

    Ok(())
}
