use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;
use sqlx::prelude::FromRow;
use sqlx::{PgPool, Postgres, QueryBuilder, Result};

use crate::config::DBConfig;

mod models;
mod seed_data;

pub use models::*;

use seed_data::*;

static MIGRATOR: Migrator = sqlx::migrate!("src/db/migrations");

pub async fn open_postgres_db(config: DBConfig) -> Result<Repo> {
    let pool = PgPoolOptions::new()
        .max_connections(100)
        .connect(&config.dsn)
        .await?;
    let repo = Repo { pool };
    if config.automigrate {
        repo.migrate().await?;
    }
    Ok(repo)
}

#[derive(FromRow)]
struct Count {
    count: i64,
}

pub struct Repo {
    pub pool: PgPool,
}

impl Repo {
    pub async fn migrate(&self) -> Result<()> {
        MIGRATOR.run(&self.pool).await?;
        Ok(())
    }
    pub async fn reset_schema(&self) -> Result<()> {
        let _ = sqlx::query("DROP SCHEMA public CASCADE")
            .execute(&self.pool)
            .await?;

        let _ = sqlx::query("CREATE SCHEMA public")
            .execute(&self.pool)
            .await?;
        self.migrate().await?;
        Ok(())
    }

    pub async fn insert_seed_data(&self) -> Result<()> {
        let rune_row = reserved_rune();
        self.insert_rune(&rune_row).await?;
        Ok(())
    }

    pub async fn get_last_indexed_block(&self, indexer_id: &str) -> Result<LastIndexedBlock> {
        let result = sqlx::query_as::<_, LastIndexedBlock>(
            "SELECT * FROM last_indexed_block WHERE indexer = $1",
        )
        .bind(indexer_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(result)
    }

    pub async fn get_last_indexed_blocks(&self) -> Result<Vec<LastIndexedBlock>> {
        let result = sqlx::query_as::<_, LastIndexedBlock>("SELECT * FROM last_indexed_block")
            .fetch_all(&self.pool)
            .await?;

        Ok(result)
    }

    pub async fn update_last_indexed_block(&self, height: i64, indexer_id: &str) -> Result<()> {
        let _result = sqlx::query("UPDATE last_indexed_block SET height = $1 WHERE indexer = $2")
            .bind(height)
            .bind(indexer_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_rune(&self, rune: &str) -> Result<Rune> {
        let result = sqlx::query_as::<_, Rune>("SELECT * FROM runes WHERE rune = $1")
            .bind(rune)
            .fetch_one(&self.pool)
            .await?;

        Ok(result)
    }

    pub async fn get_rune_by_id(&self, block: i64, tx: i32) -> Result<Rune> {
        let result =
            sqlx::query_as::<_, Rune>("SELECT * FROM runes WHERE block = $1 AND tx_id = $2")
                .bind(block)
                .bind(tx)
                .fetch_one(&self.pool)
                .await?;

        Ok(result)
    }

    pub async fn list_runes(
        &self,
        order: &str,
        limit: i32,
        offset: i32,
        name: Option<String>,
    ) -> Result<Vec<Rune>> {
        let mut q: QueryBuilder<Postgres> = QueryBuilder::new("SELECT * FROM runes ");
        if let Some(np) = name {
            let p = format!("%{}%", np);
            q.push(" WHERE rune ILIKE ");
            q.push_bind(p.clone());
        }

        if order == "DESC" {
            q.push(" ORDER BY block DESC, tx_id DESC ");
        } else {
            q.push(" ORDER BY block ASC, tx_id ASC ");
        }
        q.push(" LIMIT ");
        q.push_bind(limit);
        q.push(" OFFSET ");
        q.push_bind(offset);

        let result = q.build_query_as::<Rune>().fetch_all(&self.pool).await?;
        Ok(result)
    }

    pub async fn count_runes(&self, name_filter: Option<String>) -> Result<i64> {
        let mut q: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT count(*) as count FROM runes ");
        if let Some(np) = name_filter {
            let p = format!("%{}%", np);
            q.push(" WHERE rune ILIKE ");
            q.push_bind(p.clone());
        }

        let result = q.build_query_as::<Count>().fetch_one(&self.pool).await?;

        Ok(result.count)
    }

    pub async fn search_runes(&self, pattern: &str) -> Result<Vec<Rune>> {
        let q = "SELECT * FROM runes WHERE rune ILIKE $1 ORDER BY block ASC, tx_id ASC LIMIT 50";
        let p = format!("{}%", pattern);
        let result = sqlx::query_as::<_, Rune>(q)
            .bind(&p)
            .fetch_all(&self.pool)
            .await?;
        Ok(result)
    }

    pub async fn insert_rune(&self, rune: &Rune) -> Result<()> {
        let _ = sqlx::query(
            "INSERT INTO runes (
                    rune,
                    display_name,
                    symbol,
                    block,
                    tx_id,
                    mints,
                    max_supply,
                    minted,
                    in_circulation,
                    divisibility,
                    turbo,
                    timestamp,
                    etching_tx,
                    commitment_tx,
                    raw_data,
                    premine,
                    burned)
                  VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)",
        )
        .bind(&rune.rune)
        .bind(&rune.display_name)
        .bind(&rune.symbol)
        .bind(rune.block)
        .bind(rune.tx_id)
        .bind(rune.mints)
        .bind(&rune.max_supply)
        .bind(&rune.minted)
        .bind(&rune.in_circulation)
        .bind(rune.divisibility)
        .bind(rune.turbo)
        .bind(rune.timestamp)
        .bind(&rune.etching_tx)
        .bind(&rune.commitment_tx)
        .bind(&rune.raw_data)
        .bind(&rune.premine)
        .bind(&rune.burned)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_rune_mint(
        &self,
        rune: &str,
        mints: i32,
        minted: &str,
        in_circulation: &str,
    ) -> Result<()> {
        let _ = sqlx::query(
            "UPDATE runes SET mints = $1, minted = $2, in_circulation = $3 WHERE rune = $4",
        )
        .bind(mints)
        .bind(minted)
        .bind(in_circulation)
        .bind(rune)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_rune_burned(
        &self,
        rune: &str,
        burned: &str,
        in_circulation: &str,
    ) -> Result<()> {
        let _ = sqlx::query("UPDATE runes SET burned = $1, in_circulation = $2 WHERE rune = $3")
            .bind(burned)
            .bind(in_circulation)
            .bind(rune)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn insert_rune_log(&self, entry: &RuneLog) -> Result<()> {
        let _ = sqlx::query(
            "INSERT INTO runes_log (tx_hash, rune, address, action, value)
             VALUES($1, $2, $3, $4, $5)",
        )
        .bind(&entry.tx_hash)
        .bind(&entry.rune)
        .bind(&entry.address)
        .bind(&entry.action)
        .bind(&entry.value)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn insert_rune_utxo(&self, rb: &RuneUtxo) -> Result<()> {
        let _ = sqlx::query(
            "INSERT INTO runes_utxos (
              block, tx_id, tx_hash, output_n, rune, address, pk_script, amount, btc_amount, spend)
             VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(rb.block)
        .bind(rb.tx_id)
        .bind(&rb.tx_hash)
        .bind(rb.output_n)
        .bind(&rb.rune)
        .bind(&rb.address)
        .bind(&rb.pk_script)
        .bind(&rb.amount)
        .bind(rb.btc_amount)
        .bind(rb.spend)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn count_runes_utxo(&self, rune: &str, address: Option<String>) -> Result<i64> {
        let mut q: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT count(*) as count FROM runes_utxos WHERE spend = false ");
        q.push(" AND rune = ");
        q.push_bind(rune);

        if let Some(a) = address {
            q.push(" AND address = ");
            q.push_bind(a);
        }

        let result = q.build_query_as::<Count>().fetch_one(&self.pool).await?;
        Ok(result.count)
    }

    pub async fn select_runes_utxo_with_pagination(
        &self,
        rune: &str,
        address: Option<String>,
        order: &str,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<RuneUtxo>> {
        let mut q: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT * FROM runes_utxos WHERE spend = false ");

        q.push(" AND rune = ");
        q.push_bind(rune);

        if let Some(a) = address {
            q.push(" AND address = ");
            q.push_bind(a);
        }

        if order == "DESC" {
            q.push(" ORDER BY block DESC, tx_id DESC  ");
        } else {
            q.push(" ORDER BY block ASC, tx_id ASC ");
        }
        q.push(" LIMIT ");
        q.push_bind(limit);
        q.push(" OFFSET ");
        q.push_bind(offset);

        let result = q.build_query_as::<RuneUtxo>().fetch_all(&self.pool).await?;
        Ok(result)
    }

    pub async fn spent_rune_utxo(&self, rune: &str, tx_hash: &str, vout: i32) -> Result<()> {
        let _ =
            sqlx::query("UPDATE runes_utxos SET spend = true WHERE tx_hash = $1 AND output_n = $2 AND rune = $3")
                .bind(tx_hash)
                .bind(vout)
            .bind(rune)
                .execute(&self.pool)
                .await?;

        Ok(())
    }

    pub async fn insert_runes_balance(
        &self,
        rune: &str,
        address: &str,
        balance: &str,
    ) -> Result<()> {
        let _ = sqlx::query(
            "INSERT INTO runes_balances (address, rune, balance)
             VALUES($1, $2, $3) ON CONFLICT DO NOTHING",
        )
        .bind(address)
        .bind(rune)
        .bind(balance)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_runes_balance(
        &self,
        rune: &str,
        address: &str,
        balance: &str,
    ) -> Result<()> {
        let _ =
            sqlx::query("UPDATE runes_balances SET balance = $1 WHERE address = $2 AND rune = $3")
                .bind(balance)
                .bind(address)
                .bind(rune)
                .execute(&self.pool)
                .await?;

        Ok(())
    }

    pub async fn get_runes_balances(&self, address: &str) -> Result<Vec<RunesBalance>> {
        let result =
            sqlx::query_as::<_, RunesBalance>("SELECT * FROM runes_balances WHERE address = $1")
                .bind(address)
                .fetch_all(&self.pool)
                .await?;
        Ok(result)
    }

    pub async fn get_rune_balance(&self, address: &str, rune: &str) -> Result<RunesBalance> {
        let result = sqlx::query_as::<_, RunesBalance>(
            "SELECT * FROM runes_balances WHERE address = $1 AND rune = $2",
        )
        .bind(address)
        .bind(rune)
        .fetch_one(&self.pool)
        .await?;
        Ok(result)
    }

    pub async fn count_runes_balances(&self, rune: &str) -> Result<i64> {
        let mut q: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT count(*) as count FROM runes_balances ");
        q.push(" WHERE rune = ");
        q.push_bind(rune);

        let result = q.build_query_as::<Count>().fetch_one(&self.pool).await?;

        Ok(result.count)
    }

    pub async fn select_runes_balances(
        &self,
        rune: &str,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<RunesBalance>> {
        let result = sqlx::query_as::<_, RunesBalance>(
            "SELECT * FROM runes_balances WHERE rune = $1 ORDER BY address ASC LIMIT $2 OFFSET $3",
        )
        .bind(rune)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(result)
    }

    pub async fn insert_btc_balance(&self, address: &str) -> Result<()> {
        let _ = sqlx::query(
            "INSERT INTO btc_watchlist (address, balance) VALUES ($1, 0) ON CONFLICT DO NOTHING",
        )
        .bind(address)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn select_btc_balance(&self) -> Result<Vec<BtcBalance>> {
        let result = sqlx::query_as::<_, BtcBalance>("SELECT * FROM btc_watchlist")
            .fetch_all(&self.pool)
            .await?;
        Ok(result)
    }

    pub async fn get_btc_balance(&self, address: &str) -> Result<BtcBalance> {
        let result =
            sqlx::query_as::<_, BtcBalance>("SELECT * FROM btc_watchlist WHERE address = $1")
                .bind(address)
                .fetch_one(&self.pool)
                .await?;
        Ok(result)
    }

    pub async fn update_btc_balance(&self, address: &str, balance: i64) -> Result<()> {
        let _ = sqlx::query("UPDATE btc_watchlist SET balance = $1 WHERE address = $2")
            .bind(balance)
            .bind(address)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn insert_btc_utxo(&self, rb: &BtcUtxo) -> Result<()> {
        let _ = sqlx::query(
            "INSERT INTO btc_utxos (
              block, tx_id, tx_hash, output_n, address, pk_script, amount, spend)
             VALUES($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(rb.block)
        .bind(rb.tx_id)
        .bind(&rb.tx_hash)
        .bind(rb.output_n)
        .bind(&rb.address)
        .bind(&rb.pk_script)
        .bind(rb.amount)
        .bind(rb.spend)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn select_btc_utxo(&self, address: &str) -> Result<Vec<BtcUtxo>> {
        let result = sqlx::query_as::<_, BtcUtxo>(
            "SELECT * FROM btc_utxos WHERE address = $1 AND spend = false",
        )
        .bind(address)
        .fetch_all(&self.pool)
        .await?;

        Ok(result)
    }

    pub async fn count_btc_utxo(&self, address: Option<String>) -> Result<i64> {
        let mut q: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT count(*) as count FROM btc_utxos WHERE spend = false ");
        if let Some(a) = address {
            q.push(" AND address = ");
            q.push_bind(a);
        }

        let result = q.build_query_as::<Count>().fetch_one(&self.pool).await?;
        Ok(result.count)
    }

    pub async fn select_btc_utxo_with_pagination(
        &self,
        address: Option<String>,
        order: &str,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<BtcUtxo>> {
        let mut q: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT * FROM btc_utxos WHERE spend = false ");
        if let Some(a) = address {
            q.push(" AND address = ");
            q.push_bind(a);
        }

        if order == "DESC" {
            q.push(" ORDER BY block DESC, tx_id DESC  ");
        } else {
            q.push(" ORDER BY block ASC, tx_id ASC ");
        }
        q.push(" LIMIT ");
        q.push_bind(limit);
        q.push(" OFFSET ");
        q.push_bind(offset);

        let result = q.build_query_as::<BtcUtxo>().fetch_all(&self.pool).await?;
        Ok(result)
    }

    pub async fn get_btc_utxo(&self, tx_hash: &str, vout: i32) -> Result<BtcUtxo> {
        let result = sqlx::query_as::<_, BtcUtxo>(
            "SELECT * FROM btc_utxos WHERE tx_hash = $1 AND output_n = $2",
        )
        .bind(tx_hash)
        .bind(vout)
        .fetch_one(&self.pool)
        .await?;

        Ok(result)
    }

    pub async fn spent_btc_utxo(&self, tx_hash: &str, vout: i32) -> Result<(), sqlx::Error> {
        let _ =
            sqlx::query("UPDATE btc_utxos SET spend = true WHERE tx_hash = $1 AND output_n = $2")
                .bind(tx_hash)
                .bind(vout)
                .execute(&self.pool)
                .await?;

        Ok(())
    }

    pub async fn select_trading_pairs(
        &self,
        order: &str,
        limit: i32,
        offset: i32,
        name: Option<String>,
    ) -> Result<Vec<TradingPair>> {
        let mut q: QueryBuilder<Postgres> = QueryBuilder::new("SELECT * FROM trading_pair ");
        if let Some(np) = name {
            let p = format!("{}%", np);
            q.push(" WHERE base_asset ILIKE ");
            q.push_bind(p.clone());
        }

        if order == "DESC" {
            q.push(" ORDER BY base_asset DESC ");
        } else {
            q.push(" ORDER BY base_asset ASC ");
        }
        q.push(" LIMIT ");
        q.push_bind(limit);
        q.push(" OFFSET ");
        q.push_bind(offset);

        let result = q
            .build_query_as::<TradingPair>()
            .fetch_all(&self.pool)
            .await?;
        Ok(result)
    }

    pub async fn count_trading_pair(&self, name_filter: Option<String>) -> Result<i64> {
        let mut q: QueryBuilder<Postgres> =
            QueryBuilder::new("SELECT count(*) as count FROM trading_pair ");
        if let Some(np) = name_filter {
            let p = format!("{}%", np);
            q.push(" WHERE base_asset ILIKE ");
            q.push_bind(p.clone());
        }

        let result = q.build_query_as::<Count>().fetch_one(&self.pool).await?;

        Ok(result.count)
    }

    pub async fn get_trading_pair(
        &self,
        base_asset: &str,
        quote_asset: &str,
    ) -> Result<TradingPair> {
        let mut q: QueryBuilder<Postgres> = QueryBuilder::new("SELECT * FROM trading_pair WHERE");
        q.push(" (base_asset = ");
        q.push_bind(base_asset);
        q.push(" AND quote_asset = ");
        q.push_bind(quote_asset);
        q.push(" ) OR (base_asset = ");
        q.push_bind(quote_asset);
        q.push(" AND quote_asset = ");
        q.push_bind(base_asset);
        q.push(" ) ");

        let result = q
            .build_query_as::<TradingPair>()
            .fetch_one(&self.pool)
            .await?;
        Ok(result)
    }

    pub async fn get_trading_pair_by_id(&self, id: i64) -> Result<TradingPair> {
        let mut q: QueryBuilder<Postgres> = QueryBuilder::new("SELECT * FROM trading_pair ");
        q.push(" WHERE id = ");
        q.push_bind(id);

        let result = q
            .build_query_as::<TradingPair>()
            .fetch_one(&self.pool)
            .await?;
        Ok(result)
    }

    pub async fn update_trading_pair(
        &self,
        tx: &mut sqlx::Transaction<'_, Postgres>,
        pair: &TradingPair,
    ) -> Result<()> {
        let _ = sqlx::query(
            "UPDATE trading_pair SET base_balance = $1, quote_balance = $2,
                locked_base_balance = $3, locked_quote_balance = $4 WHERE id = $5",
        )
        .bind(&pair.base_balance)
        .bind(&pair.quote_balance)
        .bind(&pair.locked_base_balance)
        .bind(&pair.locked_quote_balance)
        .bind(pair.id)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub async fn get_liquidity_provider(
        &self,
        pair_id: i64,
        address: &str,
    ) -> Result<LiquidityProvider> {
        let result = sqlx::query_as::<_, LiquidityProvider>(
            "SELECT * FROM liquidity_providers
             WHERE trading_pair = $1 AND (base_address = $2 OR quote_address = $2)",
        )
        .bind(pair_id)
        .bind(address)
        .fetch_one(&self.pool)
        .await?;

        Ok(result)
    }

    pub async fn update_liquidity_provider(
        &self,
        tx: &mut sqlx::Transaction<'_, Postgres>,
        row: &LiquidityProvider,
    ) -> Result<()> {
        let _ = sqlx::query(
            "UPDATE liquidity_providers SET base_amount = $1, quote_amount = $2 WHERE id = $3",
        )
        .bind(&row.base_amount)
        .bind(&row.quote_amount)
        .bind(row.id)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    pub async fn insert_liquidity_change_request(
        &self,
        row: &LiquidityChangeRequest,
    ) -> Result<()> {
        let _ = sqlx::query("INSERT INTO liquidity_change_requests
            ( req_uid, trading_pair, base_address, base_amount, quote_address, quote_amount, action, status, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)")
            .bind(&row.req_uid)
            .bind(row.trading_pair)
            .bind(&row.base_address)
            .bind(&row.base_amount)
            .bind(&row.quote_address)
            .bind(&row.quote_amount)
            .bind(&row.action)
            .bind(&row.status)
            .bind(row.created_at)
            .bind(row.updated_at)
            .execute(&self.pool).await?;

        Ok(())
    }

    pub async fn update_liquidity_change_request(
        &self,
        tx: &mut sqlx::Transaction<'_, Postgres>,
        request_id: &str,
        tx_hash: &str,
        status: &str,
    ) -> Result<()> {
        let _ = sqlx::query("UPDATE liquidity_change_requests SET tx_hash = $1, status = $2, updated_at = $3 WHERE req_uid = $4")
            .bind(tx_hash)
            .bind(status)
            .bind(chrono::Utc::now().timestamp())
            .bind(request_id)
            .execute(&mut **tx)
            .await?;

        Ok(())
    }

    pub async fn get_liquidity_change_request(
        &self,
        request_id: &str,
    ) -> Result<LiquidityChangeRequest> {
        let result = sqlx::query_as::<_, LiquidityChangeRequest>(
            "SELECT * FROM liquidity_change_requests WHERE req_uid = $1",
        )
        .bind(request_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(result)
    }

    pub async fn insert_submitted_tx(&self, tx: Transaction) -> Result<()> {
        let _ = sqlx::query(
            "INSERT INTO submitted_txs
            (tx_hash, raw_data, status, context, request_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&tx.tx_hash)
        .bind(&tx.raw_data)
        .bind(&tx.status)
        .bind(&tx.context)
        .bind(&tx.request_id)
        .bind(tx.created_at)
        .bind(tx.updated_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_submitted_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, Postgres>,
        tx_hash: &str,
        status: &str,
    ) -> Result<()> {
        let _ =
            sqlx::query("UPDATE submitted_txs SET status = $1, updated_at = $2 WHERE tx_hash = $3")
                .bind(status)
                .bind(chrono::Utc::now().timestamp())
                .bind(tx_hash)
                .execute(&mut **tx)
                .await?;

        Ok(())
    }

    pub async fn select_pending_txs(&self) -> Result<Vec<Transaction>> {
        let result = sqlx::query_as::<_, Transaction>(
            "SELECT * FROM submitted_txs WHERE status = 'pending'",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(result)
    }
}
