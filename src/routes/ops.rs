use axum::extract::{Query, State};
use axum::Json;
use sqlx::PgPool;

use crate::models::{
    FunnelResponse, FunnelStage, HedgeCoverageResponse, ProcessingTimePoint,
    ProcessingTimeResponse,
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

pub async fn get_funnel(
    State(state): State<AppState>,
) -> Result<Json<FunnelResponse>, (axum::http::StatusCode, String)> {
    build_funnel(&state.pool).await.map(Json).map_err(|e| {
        tracing::error!("Funnel query failed: {e}");
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })
}

async fn build_funnel(pool: &PgPool) -> Result<FunnelResponse, sqlx::Error> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        r#"SELECT status, COUNT(*)
           FROM deposits
           GROUP BY status
           ORDER BY COUNT(*) DESC"#,
    )
    .fetch_all(pool)
    .await?;

    let total = rows.iter().map(|(_, c)| c).sum();
    let stages = rows
        .into_iter()
        .map(|(status, count)| FunnelStage { status, count })
        .collect();

    Ok(FunnelResponse { stages, total })
}

pub async fn get_processing_time(
    State(state): State<AppState>,
    Query(q): Query<PeriodQuery>,
) -> Result<Json<ProcessingTimeResponse>, (axum::http::StatusCode, String)> {
    let days = period_to_days(q.period.as_deref().unwrap_or("30d"));
    build_processing_time(&state.pool, days)
        .await
        .map(Json)
        .map_err(|e| {
            tracing::error!("Processing time query failed: {e}");
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })
}

async fn build_processing_time(
    pool: &PgPool,
    days: i64,
) -> Result<ProcessingTimeResponse, sqlx::Error> {
    let rows: Vec<(chrono::NaiveDate, f64, f64, f64, i64)> = sqlx::query_as(
        r#"SELECT DATE(created_at) as d,
                  PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY EXTRACT(EPOCH FROM (updated_at - created_at)) / 60.0),
                  PERCENTILE_CONT(0.9) WITHIN GROUP (ORDER BY EXTRACT(EPOCH FROM (updated_at - created_at)) / 60.0),
                  PERCENTILE_CONT(0.99) WITHIN GROUP (ORDER BY EXTRACT(EPOCH FROM (updated_at - created_at)) / 60.0),
                  COUNT(*)
           FROM deposits
           WHERE status = 'completed'
             AND created_at >= NOW() - make_interval(days => $1)
           GROUP BY d ORDER BY d"#,
    )
    .bind(days as i32)
    .fetch_all(pool)
    .await?;

    let points: Vec<ProcessingTimePoint> = rows
        .iter()
        .map(|(date, p50, p90, p99, count)| ProcessingTimePoint {
            date: date.format("%Y-%m-%d").to_string(),
            p50_minutes: *p50,
            p90_minutes: *p90,
            p99_minutes: *p99,
            count: *count,
        })
        .collect();

    let overall: (f64, f64) = sqlx::query_as(
        r#"SELECT
            COALESCE(PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY EXTRACT(EPOCH FROM (updated_at - created_at)) / 60.0), 0),
            COALESCE(PERCENTILE_CONT(0.9) WITHIN GROUP (ORDER BY EXTRACT(EPOCH FROM (updated_at - created_at)) / 60.0), 0)
           FROM deposits
           WHERE status = 'completed'
             AND created_at >= NOW() - make_interval(days => $1)"#,
    )
    .bind(days as i32)
    .fetch_optional(pool)
    .await?
    .unwrap_or((0.0, 0.0));

    Ok(ProcessingTimeResponse {
        points,
        overall_p50: overall.0,
        overall_p90: overall.1,
    })
}

pub async fn get_hedge_coverage(
    State(state): State<AppState>,
) -> Result<Json<HedgeCoverageResponse>, (axum::http::StatusCode, String)> {
    build_hedge_coverage(&state.pool).await.map(Json).map_err(|e| {
        tracing::error!("Hedge coverage query failed: {e}");
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })
}

async fn build_hedge_coverage(pool: &PgPool) -> Result<HedgeCoverageResponse, sqlx::Error> {
    let vault: (f64, f64) = sqlx::query_as(
        r#"SELECT COALESCE(total_paxg_oz::float8, 0),
                  COALESCE(futures_short_oz::float8, 0)
           FROM vault_state WHERE id = 1"#,
    )
    .fetch_optional(pool)
    .await?
    .unwrap_or((0.0, 0.0));

    let hedge_stats: (i64, f64) = sqlx::query_as(
        r#"SELECT COUNT(*),
                  COALESCE(SUM(margin_usd::float8), 0)
           FROM hedge_positions
           WHERE status = 'open'"#,
    )
    .fetch_optional(pool)
    .await?
    .unwrap_or((0, 0.0));

    let coverage_ratio = if vault.0 > 0.0 {
        vault.1 / vault.0
    } else {
        0.0
    };

    Ok(HedgeCoverageResponse {
        total_paxg_oz: vault.0,
        futures_short_oz: vault.1,
        coverage_ratio,
        open_positions: hedge_stats.0,
        total_margin_usd: hedge_stats.1,
    })
}
