mod config;
mod db;
mod models;
mod routes;
mod sync;

use axum::{extract::State, routing::{get, post}, Json, Router};
use std::sync::Arc;
use tokio::sync::Notify;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
    pub sync_trigger: Arc<Notify>,
}

async fn trigger_refresh(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    match sync::sync_onchain_data(&state.pool).await {
        Ok(()) => Ok(Json(serde_json::json!({"status": "ok", "message": "Sync complete"}))),
        Err(e) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Sync failed: {e}"),
        )),
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    dotenvy::dotenv().ok();
    let cfg = config::Config::from_env();
    let pool = db::create_pool(&cfg.database_url).await;

    tracing::info!("Connected to PostgreSQL");

    // Run schema migrations for on-chain sync tables
    sync::run_migrations(&pool).await;

    // Run initial sync before server starts so data is available immediately
    if let Err(e) = sync::sync_onchain_data(&pool).await {
        tracing::error!("Initial on-chain sync failed: {e}");
    }

    // Spawn background hourly sync (no initial sync — already done above)
    let sync_trigger = Arc::new(Notify::new());
    sync::spawn_hourly_sync(pool.clone(), sync_trigger.clone());

    let state = AppState {
        pool: pool.clone(),
        sync_trigger,
    };

    let api = Router::new()
        .route("/api/analytics/overview", get(routes::overview::get_overview))
        .route("/api/analytics/volume", get(routes::business::get_volume))
        .route("/api/analytics/revenue", get(routes::business::get_revenue))
        .route("/api/analytics/yield", get(routes::business::get_yield))
        .route("/api/analytics/users/growth", get(routes::users::get_growth))
        .route("/api/analytics/users/active", get(routes::users::get_active))
        .route("/api/analytics/users/top-holders", get(routes::users::get_top_holders))
        .route("/api/analytics/bitflow", get(routes::business::get_bitflow))
        .route("/api/analytics/mint-burn", get(routes::business::get_mint_burn_accounting))
        .route("/api/analytics/ops/funnel", get(routes::ops::get_funnel))
        .route("/api/analytics/ops/processing-time", get(routes::ops::get_processing_time))
        .route("/api/analytics/ops/hedge-coverage", get(routes::ops::get_hedge_coverage))
        .route("/api/analytics/onchain/summary", get(routes::onchain::get_summary))
        .route("/api/analytics/onchain/transactions", get(routes::onchain::get_transactions))
        .route("/api/analytics/onchain/proofs", get(routes::onchain::get_proofs))
        .route("/api/analytics/onchain/holders/{token}", get(routes::onchain::get_holders))
        .route("/api/analytics/refresh", post(trigger_refresh));

    let app = api
        .fallback_service(ServeDir::new("static"))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("0.0.0.0:{}", cfg.port);
    tracing::info!("Analytics dashboard running on http://localhost:{}", cfg.port);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
