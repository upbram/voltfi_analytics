use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Notify;

use crate::routes::onchain;

const SYNC_INTERVAL_SECS: u64 = 3600; // 1 hour

pub async fn run_migrations(pool: &PgPool) {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS onchain_holders (
            id SERIAL PRIMARY KEY,
            address TEXT NOT NULL,
            token TEXT NOT NULL,
            balance DOUBLE PRECISION NOT NULL,
            synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )"#,
    )
    .execute(pool)
    .await
    .expect("Failed to create onchain_holders table");

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS onchain_mint_proofs (
            tx_id TEXT PRIMARY KEY,
            block_time TEXT NOT NULL,
            product TEXT NOT NULL,
            order_id TEXT NOT NULL DEFAULT '',
            recipient TEXT NOT NULL,
            mint_amount BIGINT NOT NULL,
            total_minted BIGINT NOT NULL,
            price DOUBLE PRECISION NOT NULL DEFAULT 0,
            coinbase_fee DOUBLE PRECISION NOT NULL DEFAULT 0,
            bitflow_fee DOUBLE PRECISION NOT NULL DEFAULT 0,
            token_type TEXT NOT NULL DEFAULT '',
            amount_deposited DOUBLE PRECISION NOT NULL DEFAULT 0,
            synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )"#,
    )
    .execute(pool)
    .await
    .expect("Failed to create onchain_mint_proofs table");

    // Add columns if table already exists from a previous version
    for col in &[
        "ALTER TABLE onchain_mint_proofs ADD COLUMN IF NOT EXISTS token_type TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE onchain_mint_proofs ADD COLUMN IF NOT EXISTS amount_deposited DOUBLE PRECISION NOT NULL DEFAULT 0",
    ] {
        sqlx::query(col).execute(pool).await.ok();
    }

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS onchain_burns (
            tx_id TEXT PRIMARY KEY,
            block_time TEXT NOT NULL,
            token TEXT NOT NULL,
            amount BIGINT NOT NULL DEFAULT 0,
            synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )"#,
    )
    .execute(pool)
    .await
    .expect("Failed to create onchain_burns table");

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS onchain_transactions (
            tx_id TEXT PRIMARY KEY,
            block_time TEXT NOT NULL DEFAULT '',
            tx_type TEXT NOT NULL DEFAULT '',
            tx_status TEXT NOT NULL DEFAULT '',
            contract TEXT NOT NULL DEFAULT '',
            function TEXT NOT NULL DEFAULT '',
            fee_stx DOUBLE PRECISION NOT NULL DEFAULT 0,
            synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )"#,
    )
    .execute(pool)
    .await
    .expect("Failed to create onchain_transactions table");

    // Clean up any existing duplicates before adding unique constraint
    sqlx::query(
        r#"DELETE FROM onchain_holders a USING onchain_holders b
           WHERE a.id > b.id AND a.address = b.address AND a.token = b.token"#,
    )
    .execute(pool)
    .await
    .ok();

    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_onchain_holders_addr_token ON onchain_holders(address, token)",
    )
    .execute(pool)
    .await
    .ok();

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_onchain_mint_proofs_recipient ON onchain_mint_proofs(recipient)",
    )
    .execute(pool)
    .await
    .ok();

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_onchain_mint_proofs_block_time ON onchain_mint_proofs(block_time)",
    )
    .execute(pool)
    .await
    .ok();

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_onchain_transactions_block_time ON onchain_transactions(block_time)",
    )
    .execute(pool)
    .await
    .ok();
}

pub async fn sync_onchain_data(pool: &PgPool) -> Result<(), String> {
    tracing::info!("Starting on-chain data sync...");

    // Invalidate all in-memory caches so we fetch fresh data from Hiro
    onchain::invalidate_all_caches().await;

    // --- Sync holders (always a full snapshot, cheap — only a few pages) ---
    let vpaxg = onchain::fetch_vpaxg_holders().await?;
    let vgld = onchain::fetch_vgld_holders().await?;

    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM onchain_holders")
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    for (addr, bal) in &vpaxg {
        sqlx::query(
            "INSERT INTO onchain_holders (address, token, balance) VALUES ($1, 'vpaxg', $2)",
        )
        .bind(addr)
        .bind(bal)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }

    for (addr, bal) in &vgld {
        sqlx::query(
            "INSERT INTO onchain_holders (address, token, balance) VALUES ($1, 'vgld', $2)",
        )
        .bind(addr)
        .bind(bal)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }

    tx.commit().await.map_err(|e| e.to_string())?;

    // --- Sync ALL raw transactions to DB ---
    let all_raw_txs = onchain::fetch_all_raw_txs().await?;
    for t in &all_raw_txs {
        sqlx::query(
            r#"INSERT INTO onchain_transactions (tx_id, block_time, tx_type, tx_status, contract, function, fee_stx)
               VALUES ($1, $2, $3, $4, $5, $6, $7)
               ON CONFLICT (tx_id) DO NOTHING"#,
        )
        .bind(&t.tx_id)
        .bind(&t.block_time)
        .bind(&t.tx_type)
        .bind(&t.status)
        .bind(&t.contract)
        .bind(&t.function)
        .bind(t.fee_stx)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    }

    // --- Incremental sync for mint proofs ---
    let mint_ids: Vec<String> = all_raw_txs
        .iter()
        .filter(|t| t.is_mint)
        .map(|t| t.tx_id.clone())
        .collect();

    let existing_mint_ids: Vec<String> =
        sqlx::query_scalar("SELECT tx_id FROM onchain_mint_proofs")
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
    let existing_mint_set: std::collections::HashSet<&str> =
        existing_mint_ids.iter().map(|s| s.as_str()).collect();
    let new_mint_ids: Vec<String> = mint_ids
        .into_iter()
        .filter(|id| !existing_mint_set.contains(id.as_str()))
        .collect();

    // --- Incremental sync for burns (new + retry amount=0) ---
    let burn_entries: Vec<onchain::RawTxEntry> = all_raw_txs
        .iter()
        .filter(|t| t.is_burn)
        .cloned()
        .collect();
    let burn_ids_for_fetch: Vec<(String, String, String)> = burn_entries
        .iter()
        .map(|t| (t.tx_id.clone(), t.token.clone(), t.block_time.clone()))
        .collect();

    let existing_burn_ids: Vec<String> =
        sqlx::query_scalar("SELECT tx_id FROM onchain_burns")
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
    let existing_burn_set: std::collections::HashSet<&str> =
        existing_burn_ids.iter().map(|s| s.as_str()).collect();

    // Also re-fetch burns that have amount=0 (failed earlier due to 429)
    let zero_amount_burn_ids: Vec<String> =
        sqlx::query_scalar("SELECT tx_id FROM onchain_burns WHERE amount = 0")
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
    let zero_set: std::collections::HashSet<&str> =
        zero_amount_burn_ids.iter().map(|s| s.as_str()).collect();

    let new_burn_entries: Vec<(String, String, String)> = burn_ids_for_fetch
        .into_iter()
        .filter(|(id, _, _)| !existing_burn_set.contains(id.as_str()) || zero_set.contains(id.as_str()))
        .collect();

    tracing::info!(
        "Incremental sync: {} raw txs, {} new mints, {} new/retry burns to fetch",
        all_raw_txs.len(),
        new_mint_ids.len(),
        new_burn_entries.len()
    );

    // Fetch details only for new mint proofs
    let new_proofs = onchain::fetch_proofs_for_ids(&new_mint_ids).await?;
    for p in &new_proofs {
        sqlx::query(
            r#"INSERT INTO onchain_mint_proofs
                (tx_id, block_time, product, order_id, recipient, mint_amount, total_minted, price, coinbase_fee, bitflow_fee, token_type, amount_deposited)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
               ON CONFLICT (tx_id) DO UPDATE SET
                 token_type = EXCLUDED.token_type,
                 amount_deposited = EXCLUDED.amount_deposited"#,
        )
        .bind(&p.tx_id)
        .bind(&p.block_time)
        .bind(&p.product)
        .bind(&p.order_id)
        .bind(&p.recipient)
        .bind(p.mint_amount as i64)
        .bind(p.total_minted as i64)
        .bind(p.price)
        .bind(p.coinbase_fee)
        .bind(p.bitflow_fee)
        .bind(&p.token_type)
        .bind(p.amount_deposited)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    }

    // Fetch details for new burns + retry burns with amount=0
    let fetched_burns = onchain::fetch_burns_for_ids(&new_burn_entries).await?;
    for b in &fetched_burns {
        sqlx::query(
            r#"INSERT INTO onchain_burns (tx_id, block_time, token, amount)
               VALUES ($1, $2, $3, $4)
               ON CONFLICT (tx_id) DO UPDATE SET amount = EXCLUDED.amount"#,
        )
        .bind(&b.tx_id)
        .bind(&b.block_time)
        .bind(&b.token)
        .bind(b.amount)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    }

    let total_txs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM onchain_transactions")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    let total_proofs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM onchain_mint_proofs")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    let total_burns: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM onchain_burns")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;

    tracing::info!(
        "On-chain sync complete: {} vpaxg holders, {} vgld holders, {} txs, {} mint proofs ({} new), {} burns ({} fetched)",
        vpaxg.len(),
        vgld.len(),
        total_txs,
        total_proofs,
        new_proofs.len(),
        total_burns,
        fetched_burns.len()
    );
    Ok(())
}

pub fn spawn_hourly_sync(pool: PgPool, trigger: Arc<Notify>) {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(SYNC_INTERVAL_SECS)) => {},
                _ = trigger.notified() => {
                    tracing::info!("Manual refresh triggered");
                },
            }
            if let Err(e) = sync_onchain_data(&pool).await {
                tracing::error!("On-chain sync failed: {e}");
            }
        }
    });
}
