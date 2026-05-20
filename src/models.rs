use serde::Serialize;

#[derive(Serialize)]
pub struct OverviewResponse {
    pub total_tvl_usd: f64,
    pub total_users: i64,
    pub current_apy: f64,
    pub vgld_nav_price: f64,
    pub deposit_volume_24h_usd: f64,
    pub withdrawal_volume_24h_usd: f64,
    pub pending_deposits: i64,
    pub reserve_ratio: Option<f64>,
    pub total_paxg_oz: f64,
    pub gold_spot_price: f64,
    pub total_vgld_supply: f64,
}

// --- Phase 2: Business metrics ---

#[derive(Serialize)]
pub struct VolumePoint {
    pub date: String,
    pub deposit_count: i64,
    pub deposit_usd: f64,
    pub withdrawal_count: i64,
    pub withdrawal_usd: f64,
}

#[derive(Serialize)]
pub struct VolumeResponse {
    pub points: Vec<VolumePoint>,
    pub total_deposits_usd: f64,
    pub total_withdrawals_usd: f64,
    pub total_deposit_count: i64,
    pub total_withdrawal_count: i64,
}

#[derive(Serialize)]
pub struct RevenuePoint {
    pub date: String,
    pub coinbase_fees: f64,
    pub bitflow_fees: f64,
    pub profit_fees: f64,
    pub total: f64,
}

#[derive(Serialize)]
pub struct RevenueResponse {
    pub points: Vec<RevenuePoint>,
    pub total_coinbase_fees: f64,
    pub total_bitflow_fees: f64,
    pub total_profit_fees: f64,
    pub total_revenue: f64,
}

#[derive(Serialize)]
pub struct YieldRollPoint {
    pub date: String,
    pub spot_price: f64,
    pub futures_price: f64,
    pub basis_usd: f64,
    pub net_yield_usd: f64,
    pub cumulative_yield_per_vgld: f64,
    pub fees_usd: f64,
}

#[derive(Serialize)]
pub struct YieldResponse {
    pub rolls: Vec<YieldRollPoint>,
    pub current_apy: f64,
    pub cumulative_yield_per_vgld: f64,
    pub total_net_yield_usd: f64,
    pub last_roll_date: Option<String>,
    pub next_roll_date: Option<String>,
}

// --- Phase 3: User metrics ---

#[derive(Serialize)]
pub struct UserGrowthPoint {
    pub date: String,
    pub new_users: i64,
    pub cumulative_users: i64,
}

#[derive(Serialize)]
pub struct UserGrowthResponse {
    pub points: Vec<UserGrowthPoint>,
    pub total_users: i64,
}

#[derive(Serialize)]
pub struct ActiveUsersPoint {
    pub date: String,
    pub dau: i64,
}

#[derive(Serialize)]
pub struct ActiveUsersResponse {
    pub daily: Vec<ActiveUsersPoint>,
    pub wau: i64,
    pub mau: i64,
}

#[derive(Serialize)]
pub struct TopHolder {
    pub address: String,
    pub total_deposited_usd: f64,
    pub total_withdrawn_usd: f64,
    pub net_usd: f64,
    pub deposit_count: i64,
    pub first_deposit: String,
    pub last_deposit: String,
    pub products: Vec<String>,
}

// --- Bitflow metrics ---

#[derive(Serialize)]
pub struct BitflowDayPoint {
    pub date: String,
    pub usdc_volume: f64,
    pub bitflow_fee: f64,
    pub txn_count: i64,
}

#[derive(Serialize)]
pub struct BitflowResponse {
    pub points: Vec<BitflowDayPoint>,
    pub total_usdc_volume: f64,
    pub total_bitflow_fees: f64,
    pub total_txn_count: i64,
    pub avg_swap_size: f64,
}

// --- Mint/Burn accounting ---

#[derive(Serialize)]
pub struct MintBurnAccounting {
    pub vpaxg_mints: i64,
    pub vpaxg_burns: i64,
    pub vpaxg_net: i64,
    pub vpaxg_mint_amount: f64,
    pub vpaxg_burn_amount: f64,
    pub vpaxg_net_amount: f64,
    pub vgld_mints: i64,
    pub vgld_burns: i64,
    pub vgld_net: i64,
    pub vgld_mint_amount: f64,
    pub vgld_burn_amount: f64,
    pub vgld_net_amount: f64,
}

// --- Phase 4: Operational metrics ---

#[derive(Serialize)]
pub struct FunnelStage {
    pub status: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct FunnelResponse {
    pub stages: Vec<FunnelStage>,
    pub total: i64,
}

#[derive(Serialize)]
pub struct ProcessingTimePoint {
    pub date: String,
    pub p50_minutes: f64,
    pub p90_minutes: f64,
    pub p99_minutes: f64,
    pub count: i64,
}

#[derive(Serialize)]
pub struct ProcessingTimeResponse {
    pub points: Vec<ProcessingTimePoint>,
    pub overall_p50: f64,
    pub overall_p90: f64,
}

#[derive(Serialize)]
pub struct HedgeCoverageResponse {
    pub total_paxg_oz: f64,
    pub futures_short_oz: f64,
    pub coverage_ratio: f64,
    pub open_positions: i64,
    pub total_margin_usd: f64,
}

// --- Phase 4b: On-chain analytics ---

#[derive(Serialize, Clone)]
pub struct OnchainSummary {
    pub vpaxg_total_supply: f64,
    pub vgld_total_supply: f64,
    pub vpaxg_holder_count: u64,
    pub vgld_holder_count: u64,
    pub total_unique_wallets: u64,
    pub total_onchain_txs: u64,
    pub success_txs: u64,
    pub failed_txs: u64,
    pub success_rate: f64,
    pub total_mints_vpaxg: u64,
    pub total_mints_vgld: u64,
    pub deployer_stx_balance: f64,
}

#[derive(Serialize, Clone)]
pub struct OnchainTx {
    pub tx_id: String,
    pub block_time: String,
    pub tx_type: String,
    pub contract: String,
    pub function: String,
    pub status: String,
    pub fee_stx: f64,
}

#[derive(Serialize, Clone)]
pub struct MintProof {
    pub tx_id: String,
    pub block_time: String,
    pub product: String,
    pub order_id: String,
    pub recipient: String,
    pub mint_amount: u64,
    pub total_minted: u64,
    pub price: f64,
    pub coinbase_fee: f64,
    pub bitflow_fee: f64,
    pub token_type: String,
    pub amount_deposited: f64,
}

#[derive(Serialize, Clone)]
pub struct TokenHolder {
    pub address: String,
    pub balance: f64,
    pub percentage: f64,
}

#[derive(Serialize, Clone)]
pub struct OnchainHolders {
    pub token: String,
    pub total_supply: f64,
    pub holder_count: u64,
    pub holders: Vec<TokenHolder>,
    pub top10_concentration: f64,
}
