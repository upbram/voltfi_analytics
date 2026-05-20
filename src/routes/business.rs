use axum::extract::{Query, State};
use axum::Json;
use sqlx::PgPool;

use crate::models::{
    BitflowDayPoint, BitflowResponse, MintBurnAccounting, RevenuePoint, RevenueResponse,
    VolumePoint, VolumeResponse, YieldResponse, YieldRollPoint,
};
use crate::AppState;

#[derive(serde::Deserialize)]
pub struct PeriodQuery {
    pub period: Option<String>,
}

fn period_to_days(period: &str) -> i64 {
    match period {
        "7d" => 7,
        "30d" => 30,
        "90d" => 90,
        "all" => 3650,
        _ => 30,
    }
}

pub async fn get_volume(
    State(state): State<AppState>,
    Query(q): Query<PeriodQuery>,
) -> Result<Json<VolumeResponse>, (axum::http::StatusCode, String)> {
    let days = period_to_days(q.period.as_deref().unwrap_or("30d"));
    build_volume(&state.pool, days).await.map(Json).map_err(|e| {
        tracing::error!("Volume query failed: {e}");
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })
}

async fn build_volume(pool: &PgPool, days: i64) -> Result<VolumeResponse, sqlx::Error> {
    let deposit_rows: Vec<(chrono::NaiveDate, i64, f64)> = sqlx::query_as(
        r#"SELECT DATE(created_at) as d,
                  COUNT(*),
                  COALESCE(SUM(
                      CASE WHEN token_type = 'stx' THEN stx_amount::float8
                           ELSE usdcx_amount::float8 END
                  ), 0)
           FROM deposits
           WHERE created_at >= NOW() - make_interval(days => $1)
             AND status != 'failed'
           GROUP BY d ORDER BY d"#,
    )
    .bind(days as i32)
    .fetch_all(pool)
    .await?;

    let withdrawal_rows: Vec<(chrono::NaiveDate, i64, f64)> = sqlx::query_as(
        r#"SELECT DATE(created_at) as d,
                  COUNT(*),
                  COALESCE(SUM(vgld_amount::float8), 0)
           FROM withdrawals
           WHERE created_at >= NOW() - make_interval(days => $1)
             AND status != 'failed'
           GROUP BY d ORDER BY d"#,
    )
    .bind(days as i32)
    .fetch_all(pool)
    .await?;

    let mut date_map: std::collections::BTreeMap<String, VolumePoint> =
        std::collections::BTreeMap::new();

    for (date, count, usd) in &deposit_rows {
        let key = date.format("%Y-%m-%d").to_string();
        let entry = date_map.entry(key.clone()).or_insert(VolumePoint {
            date: key,
            deposit_count: 0,
            deposit_usd: 0.0,
            withdrawal_count: 0,
            withdrawal_usd: 0.0,
        });
        entry.deposit_count = *count;
        entry.deposit_usd = *usd;
    }

    for (date, count, usd) in &withdrawal_rows {
        let key = date.format("%Y-%m-%d").to_string();
        let entry = date_map.entry(key.clone()).or_insert(VolumePoint {
            date: key,
            deposit_count: 0,
            deposit_usd: 0.0,
            withdrawal_count: 0,
            withdrawal_usd: 0.0,
        });
        entry.withdrawal_count = *count;
        entry.withdrawal_usd = *usd;
    }

    let points: Vec<VolumePoint> = date_map.into_values().collect();

    let total_deposits_usd = points.iter().map(|p| p.deposit_usd).sum();
    let total_withdrawals_usd = points.iter().map(|p| p.withdrawal_usd).sum();
    let total_deposit_count = points.iter().map(|p| p.deposit_count).sum();
    let total_withdrawal_count = points.iter().map(|p| p.withdrawal_count).sum();

    Ok(VolumeResponse {
        points,
        total_deposits_usd,
        total_withdrawals_usd,
        total_deposit_count,
        total_withdrawal_count,
    })
}

pub async fn get_revenue(
    State(state): State<AppState>,
    Query(q): Query<PeriodQuery>,
) -> Result<Json<RevenueResponse>, (axum::http::StatusCode, String)> {
    let days = period_to_days(q.period.as_deref().unwrap_or("30d"));
    build_revenue(&state.pool, days).await.map(Json).map_err(|e| {
        tracing::error!("Revenue query failed: {e}");
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })
}

async fn build_revenue(pool: &PgPool, days: i64) -> Result<RevenueResponse, sqlx::Error> {
    let deposit_fees: Vec<(chrono::NaiveDate, f64, f64)> = sqlx::query_as(
        r#"SELECT block_time::timestamp::date as d,
                  COALESCE(SUM(coinbase_fee), 0),
                  COALESCE(SUM(bitflow_fee), 0)
           FROM onchain_mint_proofs
           WHERE block_time::timestamp >= NOW() - make_interval(days => $1)
           GROUP BY d ORDER BY d"#,
    )
    .bind(days as i32)
    .fetch_all(pool)
    .await?;

    let withdrawal_fees: Vec<(chrono::NaiveDate, f64)> = sqlx::query_as(
        r#"SELECT DATE(created_at) as d,
                  COALESCE(SUM(voltfi_fee_usd::float8), 0)
           FROM withdrawals
           WHERE created_at >= NOW() - make_interval(days => $1)
             AND status = 'completed'
           GROUP BY d ORDER BY d"#,
    )
    .bind(days as i32)
    .fetch_all(pool)
    .await?;

    let mut date_map: std::collections::BTreeMap<String, RevenuePoint> =
        std::collections::BTreeMap::new();

    for (date, cb_fee, bf_fee) in &deposit_fees {
        let key = date.format("%Y-%m-%d").to_string();
        let entry = date_map.entry(key.clone()).or_insert(RevenuePoint {
            date: key,
            coinbase_fees: 0.0,
            bitflow_fees: 0.0,
            profit_fees: 0.0,
            total: 0.0,
        });
        entry.coinbase_fees = *cb_fee;
        entry.bitflow_fees = *bf_fee;
    }

    for (date, profit_fee) in &withdrawal_fees {
        let key = date.format("%Y-%m-%d").to_string();
        let entry = date_map.entry(key.clone()).or_insert(RevenuePoint {
            date: key,
            coinbase_fees: 0.0,
            bitflow_fees: 0.0,
            profit_fees: 0.0,
            total: 0.0,
        });
        entry.profit_fees = *profit_fee;
    }

    let mut points: Vec<RevenuePoint> = date_map.into_values().collect();
    for p in &mut points {
        p.total = p.coinbase_fees + p.bitflow_fees + p.profit_fees;
    }

    let total_coinbase_fees = points.iter().map(|p| p.coinbase_fees).sum();
    let total_bitflow_fees = points.iter().map(|p| p.bitflow_fees).sum();
    let total_profit_fees = points.iter().map(|p| p.profit_fees).sum();
    let total_revenue = points.iter().map(|p| p.total).sum();

    Ok(RevenueResponse {
        points,
        total_coinbase_fees,
        total_bitflow_fees,
        total_profit_fees,
        total_revenue,
    })
}

pub async fn get_yield(
    State(state): State<AppState>,
) -> Result<Json<YieldResponse>, (axum::http::StatusCode, String)> {
    build_yield(&state.pool).await.map(Json).map_err(|e| {
        tracing::error!("Yield query failed: {e}");
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })
}

async fn build_yield(pool: &PgPool) -> Result<YieldResponse, sqlx::Error> {
    let rolls: Vec<(
        chrono::DateTime<chrono::Utc>,
        f64, f64, f64, f64, f64, f64,
    )> = sqlx::query_as(
        r#"SELECT roll_date,
                  spot_price::float8,
                  futures_price::float8,
                  basis_usd::float8,
                  net_yield_usd::float8,
                  cumulative_yield_per_vgld::float8,
                  COALESCE(fees_usd::float8, 0)
           FROM vault_rolls
           ORDER BY roll_date ASC"#,
    )
    .fetch_all(pool)
    .await?;

    let roll_points: Vec<YieldRollPoint> = rolls
        .iter()
        .map(|(date, spot, futures, basis, net, cum, fees)| YieldRollPoint {
            date: date.format("%Y-%m-%d").to_string(),
            spot_price: *spot,
            futures_price: *futures,
            basis_usd: *basis,
            net_yield_usd: *net,
            cumulative_yield_per_vgld: *cum,
            fees_usd: *fees,
        })
        .collect();

    let vault: (f64, f64, Option<chrono::DateTime<chrono::Utc>>, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as(
            r#"SELECT COALESCE(current_apy::float8, 8.0),
                      COALESCE(cumulative_yield_per_vgld::float8, 0),
                      last_roll_date,
                      next_roll_date
               FROM vault_state WHERE id = 1"#,
        )
        .fetch_optional(pool)
        .await?
        .unwrap_or((8.0, 0.0, None, None));

    let total_net_yield_usd = roll_points.iter().map(|r| r.net_yield_usd).sum();

    Ok(YieldResponse {
        rolls: roll_points,
        current_apy: vault.0,
        cumulative_yield_per_vgld: vault.1,
        total_net_yield_usd,
        last_roll_date: vault.2.map(|d| d.format("%Y-%m-%d").to_string()),
        next_roll_date: vault.3.map(|d| d.format("%Y-%m-%d").to_string()),
    })
}

pub async fn get_bitflow(
    State(state): State<AppState>,
) -> Result<Json<BitflowResponse>, (axum::http::StatusCode, String)> {
    build_bitflow(&state.pool).await.map(Json).map_err(|e| {
        tracing::error!("Bitflow query failed: {e}");
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })
}

async fn build_bitflow(pool: &PgPool) -> Result<BitflowResponse, sqlx::Error> {
    let rows: Vec<(chrono::NaiveDate, f64, f64, i64)> = sqlx::query_as(
        r#"SELECT block_time::timestamp::date as d,
                  COALESCE(SUM(amount_deposited), 0),
                  COALESCE(SUM(bitflow_fee), 0),
                  COUNT(*)
           FROM onchain_mint_proofs
           WHERE token_type = 'usdcx'
             AND amount_deposited > 0
           GROUP BY d
           ORDER BY d"#,
    )
    .fetch_all(pool)
    .await?;

    let points: Vec<BitflowDayPoint> = rows
        .iter()
        .map(|(date, vol, fee, count)| BitflowDayPoint {
            date: date.format("%Y-%m-%d").to_string(),
            usdc_volume: *vol,
            bitflow_fee: *fee,
            txn_count: *count,
        })
        .collect();

    let total_usdc_volume: f64 = points.iter().map(|p| p.usdc_volume).sum();
    let total_bitflow_fees: f64 = points.iter().map(|p| p.bitflow_fee).sum();
    let total_txn_count: i64 = points.iter().map(|p| p.txn_count).sum();
    let avg_swap_size = if total_txn_count > 0 {
        total_usdc_volume / total_txn_count as f64
    } else {
        0.0
    };

    Ok(BitflowResponse {
        points,
        total_usdc_volume,
        total_bitflow_fees,
        total_txn_count,
        avg_swap_size,
    })
}

pub async fn get_mint_burn_accounting(
    State(state): State<AppState>,
) -> Result<Json<MintBurnAccounting>, (axum::http::StatusCode, String)> {
    build_mint_burn_accounting(&state.pool)
        .await
        .map(Json)
        .map_err(|e| {
            tracing::error!("Mint/burn accounting query failed: {e}");
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })
}

async fn build_mint_burn_accounting(pool: &PgPool) -> Result<MintBurnAccounting, sqlx::Error> {
    // vPAXG mints (count and total amount in token units — 6 decimals)
    let (vpaxg_mints, vpaxg_mint_amount): (i64, f64) = sqlx::query_as(
        r#"SELECT COUNT(*), COALESCE(SUM(mint_amount::float8 / 1000000.0), 0)
           FROM onchain_mint_proofs WHERE product = 'vPAXG'"#,
    )
    .fetch_one(pool)
    .await
    .unwrap_or((0, 0.0));

    // vPAXG burns (count and total amount — 6 decimals)
    let (vpaxg_burns, vpaxg_burn_amount): (i64, f64) = sqlx::query_as(
        r#"SELECT COUNT(*), COALESCE(SUM(amount::float8 / 1000000.0), 0)
           FROM onchain_burns WHERE token = 'vpaxg'"#,
    )
    .fetch_one(pool)
    .await
    .unwrap_or((0, 0.0));

    // vGLD mints (count and total amount — 8 decimals)
    let (vgld_mints, vgld_mint_amount): (i64, f64) = sqlx::query_as(
        r#"SELECT COUNT(*), COALESCE(SUM(mint_amount::float8 / 100000000.0), 0)
           FROM onchain_mint_proofs WHERE product = 'vGLD'"#,
    )
    .fetch_one(pool)
    .await
    .unwrap_or((0, 0.0));

    // vGLD burns (count and total amount — 8 decimals)
    let (vgld_burns, vgld_burn_amount): (i64, f64) = sqlx::query_as(
        r#"SELECT COUNT(*), COALESCE(SUM(amount::float8 / 100000000.0), 0)
           FROM onchain_burns WHERE token = 'vgld'"#,
    )
    .fetch_one(pool)
    .await
    .unwrap_or((0, 0.0));

    Ok(MintBurnAccounting {
        vpaxg_mints,
        vpaxg_burns,
        vpaxg_net: vpaxg_mints - vpaxg_burns,
        vpaxg_mint_amount,
        vpaxg_burn_amount,
        vpaxg_net_amount: vpaxg_mint_amount - vpaxg_burn_amount,
        vgld_mints,
        vgld_burns,
        vgld_net: vgld_mints - vgld_burns,
        vgld_mint_amount,
        vgld_burn_amount,
        vgld_net_amount: vgld_mint_amount - vgld_burn_amount,
    })
}
