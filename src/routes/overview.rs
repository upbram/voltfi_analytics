use axum::extract::State;
use axum::Json;
use sqlx::PgPool;

use crate::models::OverviewResponse;
use crate::AppState;

pub async fn get_overview(
    State(state): State<AppState>,
) -> Result<Json<OverviewResponse>, (axum::http::StatusCode, String)> {
    let result = build_overview(&state.pool).await.map_err(|e| {
        tracing::error!("Overview query failed: {e}");
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {e}"),
        )
    })?;

    Ok(Json(result))
}

async fn build_overview(pool: &PgPool) -> Result<OverviewResponse, sqlx::Error> {
    let total_users: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (SELECT DISTINCT address FROM onchain_holders UNION SELECT DISTINCT recipient FROM onchain_mint_proofs) all_wallets",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let total_paxg_oz: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(balance), 0) FROM onchain_holders WHERE token = 'vpaxg'",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0.0);

    let total_vgld_supply: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(balance), 0) FROM onchain_holders WHERE token = 'vgld'",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0.0);

    let gold_spot_price: f64 = sqlx::query_scalar(
        "SELECT COALESCE(price, 0) FROM onchain_mint_proofs WHERE product = 'vPAXG' ORDER BY block_time DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?
    .unwrap_or(0.0);

    let vgld_nav_price: f64 = sqlx::query_scalar(
        "SELECT COALESCE(price, 0) FROM onchain_mint_proofs WHERE product = 'vGLD' ORDER BY block_time DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?
    .unwrap_or(1.0);

    let total_tvl_usd = total_paxg_oz * gold_spot_price + total_vgld_supply * vgld_nav_price;

    let now_minus_24h = (chrono::Utc::now() - chrono::Duration::hours(24))
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string();

    let deposit_volume_24h: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount_deposited), 0) FROM onchain_mint_proofs WHERE block_time >= $1",
    )
    .bind(&now_minus_24h)
    .fetch_one(pool)
    .await
    .unwrap_or(0.0);

    let withdrawal_volume_24h: f64 = sqlx::query_scalar(
        "SELECT COALESCE(COUNT(*), 0)::float8 FROM onchain_burns WHERE block_time >= $1",
    )
    .bind(&now_minus_24h)
    .fetch_one(pool)
    .await
    .unwrap_or(0.0);

    Ok(OverviewResponse {
        total_tvl_usd,
        total_users: total_users + 2, // +2 protocol wallets used for internal testing
        current_apy: 8.0,
        vgld_nav_price,
        deposit_volume_24h_usd: deposit_volume_24h,
        withdrawal_volume_24h_usd: withdrawal_volume_24h,
        pending_deposits: 0,
        reserve_ratio: None,
        total_paxg_oz,
        gold_spot_price,
        total_vgld_supply,
    })
}
