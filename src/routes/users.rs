use axum::extract::{Query, State};
use axum::Json;
use sqlx::PgPool;

use crate::models::{
    ActiveUsersPoint, ActiveUsersResponse, TopHolder, UserGrowthPoint, UserGrowthResponse,
};
use crate::AppState;

#[derive(serde::Deserialize)]
pub struct PeriodQuery {
    pub period: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct LimitQuery {
    pub limit: Option<i64>,
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

pub async fn get_growth(
    State(state): State<AppState>,
    Query(q): Query<PeriodQuery>,
) -> Result<Json<UserGrowthResponse>, (axum::http::StatusCode, String)> {
    let days = period_to_days(q.period.as_deref().unwrap_or("90d"));
    build_growth(&state.pool, days).await.map(Json).map_err(|e| {
        tracing::error!("User growth query failed: {e}");
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })
}

async fn build_growth(pool: &PgPool, days: i64) -> Result<UserGrowthResponse, sqlx::Error> {
    // Each wallet's first mint = when they became a "user"
    let rows: Vec<(chrono::NaiveDate, i64)> = sqlx::query_as(
        r#"SELECT first_seen::date, COUNT(*) FROM (
            SELECT recipient, MIN(block_time::timestamp)::date as first_seen
            FROM onchain_mint_proofs
            WHERE recipient != ''
            GROUP BY recipient
        ) t
        WHERE first_seen >= NOW() - make_interval(days => $1)
        GROUP BY first_seen::date ORDER BY first_seen::date"#,
    )
    .bind(days as i32)
    .fetch_all(pool)
    .await?;

    let total_before: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM (
            SELECT recipient, MIN(block_time::timestamp)::date as first_seen
            FROM onchain_mint_proofs
            WHERE recipient != ''
            GROUP BY recipient
        ) t
        WHERE first_seen < NOW() - make_interval(days => $1)"#,
    )
    .bind(days as i32)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let mut cumulative = total_before;
    let points: Vec<UserGrowthPoint> = rows
        .iter()
        .map(|(date, count)| {
            cumulative += count;
            UserGrowthPoint {
                date: date.format("%Y-%m-%d").to_string(),
                new_users: *count,
                cumulative_users: cumulative,
            }
        })
        .collect();

    // Use the same total as overview: union of holders + mint recipients + 2 protocol wallets
    let total_unique: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (SELECT DISTINCT address FROM onchain_holders UNION SELECT DISTINCT recipient FROM onchain_mint_proofs WHERE recipient != '') w",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(cumulative);

    Ok(UserGrowthResponse {
        total_users: total_unique + 2, // +2 protocol wallets used for internal testing
        points,
    })
}

pub async fn get_active(
    State(state): State<AppState>,
    Query(q): Query<PeriodQuery>,
) -> Result<Json<ActiveUsersResponse>, (axum::http::StatusCode, String)> {
    let days = period_to_days(q.period.as_deref().unwrap_or("30d"));
    build_active(&state.pool, days).await.map(Json).map_err(|e| {
        tracing::error!("Active users query failed: {e}");
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })
}

async fn build_active(pool: &PgPool, days: i64) -> Result<ActiveUsersResponse, sqlx::Error> {
    let daily_rows: Vec<(chrono::NaiveDate, i64)> = sqlx::query_as(
        r#"SELECT block_time::timestamp::date as d, COUNT(DISTINCT recipient)
        FROM onchain_mint_proofs
        WHERE recipient != ''
          AND block_time::timestamp >= NOW() - make_interval(days => $1)
        GROUP BY d ORDER BY d"#,
    )
    .bind(days as i32)
    .fetch_all(pool)
    .await?;

    let daily: Vec<ActiveUsersPoint> = daily_rows
        .iter()
        .map(|(date, count)| ActiveUsersPoint {
            date: date.format("%Y-%m-%d").to_string(),
            dau: *count,
        })
        .collect();

    let wau: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(DISTINCT recipient)
        FROM onchain_mint_proofs
        WHERE recipient != ''
          AND block_time::timestamp >= NOW() - INTERVAL '7 days'"#,
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let mau: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(DISTINCT recipient)
        FROM onchain_mint_proofs
        WHERE recipient != ''
          AND block_time::timestamp >= NOW() - INTERVAL '30 days'"#,
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    Ok(ActiveUsersResponse { daily, wau, mau })
}

pub async fn get_top_holders(
    State(state): State<AppState>,
    Query(q): Query<LimitQuery>,
) -> Result<Json<Vec<TopHolder>>, (axum::http::StatusCode, String)> {
    let limit = q.limit.unwrap_or(20).min(100);
    build_top_holders(&state.pool, limit).await.map(Json).map_err(|e| {
        tracing::error!("Top holders query failed: {e}");
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })
}

async fn build_top_holders(pool: &PgPool, limit: i64) -> Result<Vec<TopHolder>, sqlx::Error> {
    // Aggregate balances per address across both tokens, sorted by combined balance
    let rows: Vec<(String, f64, String)> = sqlx::query_as(
        r#"SELECT address,
                  SUM(balance) as total_bal,
                  STRING_AGG(DISTINCT token, ',') as products
           FROM onchain_holders
           GROUP BY address
           ORDER BY total_bal DESC
           LIMIT $1"#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut holders = Vec::with_capacity(rows.len());
    for (addr, total_bal, products) in rows {
        // Enrich with mint proof data
        let mint_info: Option<(i64, Option<String>, Option<String>)> = sqlx::query_as(
            r#"SELECT COUNT(*),
                      MIN(block_time),
                      MAX(block_time)
               FROM onchain_mint_proofs
               WHERE recipient = $1"#,
        )
        .bind(&addr)
        .fetch_optional(pool)
        .await?;

        let (count, first, last) = mint_info.unwrap_or((0, None, None));

        holders.push(TopHolder {
            address: addr,
            total_deposited_usd: total_bal,
            total_withdrawn_usd: 0.0,
            net_usd: total_bal,
            deposit_count: count,
            first_deposit: first
                .map(|s| s.chars().take(10).collect())
                .unwrap_or_default(),
            last_deposit: last
                .map(|s| s.chars().take(10).collect())
                .unwrap_or_default(),
            products: products.split(',').map(|s| s.to_string()).collect(),
        });
    }

    Ok(holders)
}
