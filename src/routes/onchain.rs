use axum::Json;
use serde::Deserialize;
use std::sync::LazyLock;
use tokio::sync::RwLock;

use crate::models::{MintProof, OnchainHolders, OnchainSummary, OnchainTx, TokenHolder};

const HIRO_BASE: &str = "https://api.mainnet.hiro.so";
const DEPLOYER: &str = "SP183MTM6NNBG18YSKCQG7Y5P5HVTAK8WSXJNKYMW";

const VPAXG_TOKEN: &str = "SP183MTM6NNBG18YSKCQG7Y5P5HVTAK8WSXJNKYMW.vpaxg-token::vPAXG";
const VGLD_TOKEN: &str = "SP183MTM6NNBG18YSKCQG7Y5P5HVTAK8WSXJNKYMW.vgld-token-v4::vGLDv4";

const CACHE_TTL: u64 = 7200; // 2 hours (synced hourly, so cache never expires between syncs)

// --- Shared HTTP client (connection pooling) ---

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

// --- Cache infrastructure ---

struct Cache<T: Clone> {
    data: RwLock<Option<(T, std::time::Instant)>>,
    ttl: std::time::Duration,
}

impl<T: Clone> Cache<T> {
    const fn new(ttl_secs: u64) -> Self {
        Self {
            data: RwLock::const_new(None),
            ttl: std::time::Duration::from_secs(ttl_secs),
        }
    }

    async fn get(&self) -> Option<T> {
        let guard = self.data.read().await;
        guard.as_ref().and_then(|(data, ts)| {
            if ts.elapsed() < self.ttl {
                Some(data.clone())
            } else {
                None
            }
        })
    }

    async fn set(&self, val: T) {
        let mut guard = self.data.write().await;
        *guard = Some((val, std::time::Instant::now()));
    }

    async fn invalidate(&self) {
        let mut guard = self.data.write().await;
        *guard = None;
    }
}

// Single raw tx cache shared by all endpoints
static RAW_TX_CACHE: LazyLock<Cache<Vec<RawTxCacheEntry>>> = LazyLock::new(|| Cache::new(CACHE_TTL));
static TX_CACHE: LazyLock<Cache<Vec<OnchainTx>>> = LazyLock::new(|| Cache::new(CACHE_TTL));
static PROOF_CACHE: LazyLock<Cache<Vec<MintProof>>> = LazyLock::new(|| Cache::new(CACHE_TTL));
static SUMMARY_CACHE: LazyLock<Cache<OnchainSummary>> = LazyLock::new(|| Cache::new(CACHE_TTL));
static VPAXG_HOLDERS_CACHE: LazyLock<Cache<OnchainHolders>> = LazyLock::new(|| Cache::new(CACHE_TTL));
static VGLD_HOLDERS_CACHE: LazyLock<Cache<OnchainHolders>> = LazyLock::new(|| Cache::new(CACHE_TTL));

static ALL_VPAXG_HOLDERS_CACHE: LazyLock<Cache<Vec<(String, f64)>>> =
    LazyLock::new(|| Cache::new(CACHE_TTL));
static ALL_VGLD_HOLDERS_CACHE: LazyLock<Cache<Vec<(String, f64)>>> =
    LazyLock::new(|| Cache::new(CACHE_TTL));

fn err(msg: String) -> (axum::http::StatusCode, String) {
    tracing::error!("On-chain query failed: {msg}");
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, msg)
}

// --- Hiro API types ---

#[derive(Deserialize)]
struct HiroTxList {
    total: u64,
    results: Vec<HiroTx>,
}

#[derive(Deserialize)]
struct HiroTx {
    tx_id: String,
    tx_type: String,
    tx_status: String,
    burn_block_time_iso: Option<String>,
    fee_rate: Option<String>,
    contract_call: Option<HiroContractCall>,
    smart_contract: Option<HiroSmartContract>,
    events: Option<Vec<HiroEvent>>,
}

#[derive(Deserialize)]
struct HiroContractCall {
    contract_id: String,
    function_name: String,
    function_args: Option<Vec<HiroFunctionArg>>,
}

#[derive(Deserialize)]
struct HiroFunctionArg {
    name: Option<String>,
    repr: Option<String>,
}

#[derive(Deserialize)]
struct HiroSmartContract {
    contract_id: String,
}

#[derive(Deserialize)]
struct HiroEvent {
    event_type: String,
    contract_log: Option<HiroContractLog>,
}

#[derive(Deserialize)]
struct HiroContractLog {
    value: Option<HiroLogValue>,
}

#[derive(Deserialize)]
struct HiroLogValue {
    repr: Option<String>,
}

#[derive(Deserialize)]
struct HiroAccountBalance {
    stx: Option<HiroStxBalance>,
}

#[derive(Deserialize)]
struct HiroStxBalance {
    balance: Option<String>,
}

#[derive(Deserialize)]
struct HiroHoldersList {
    total: Option<u64>,
    results: Vec<HiroHolder>,
}

#[derive(Deserialize)]
struct HiroHolder {
    address: Option<String>,
    balance: Option<String>,
}

#[derive(Deserialize)]
struct HiroReadOnly {
    okay: Option<bool>,
    result: Option<String>,
}

// Minimal cache entry for raw tx data we need across endpoints
#[derive(Clone)]
struct RawTxCacheEntry {
    tx_id: String,
    tx_type: String,
    tx_status: String,
    burn_block_time_iso: Option<String>,
    fee_rate: Option<String>,
    contract_id: Option<String>,
    function_name: Option<String>,
    smart_contract_id: Option<String>,
    is_mint: bool,
}

// --- Resilient HTTP helper with retry on 429 ---

const MAX_RETRIES: u32 = 3;

async fn hiro_get<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, String> {
    for attempt in 0..=MAX_RETRIES {
        let resp = CLIENT.get(url).send().await.map_err(|e| e.to_string())?;
        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt < MAX_RETRIES {
            let wait = 15 * (attempt + 1) as u64;
            tracing::warn!("Hiro 429, retrying in {wait}s (attempt {}/{})", attempt + 1, MAX_RETRIES);
            tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            continue;
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Hiro API {status}: {}", &body[..body.len().min(200)]));
        }
        return resp.json().await.map_err(|e| format!("JSON decode: {e}"));
    }
    Err("Hiro API: max retries exceeded".into())
}

async fn hiro_post<T: serde::de::DeserializeOwned>(
    url: &str,
    body: &serde_json::Value,
) -> Result<T, String> {
    for attempt in 0..=MAX_RETRIES {
        let resp = CLIENT
            .post(url)
            .json(body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt < MAX_RETRIES {
            let wait = 15 * (attempt + 1) as u64;
            tracing::warn!("Hiro 429, retrying in {wait}s (attempt {}/{})", attempt + 1, MAX_RETRIES);
            tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            continue;
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Hiro API {status}: {}", &body[..body.len().min(200)]));
        }
        return resp.json().await.map_err(|e| format!("JSON decode: {e}"));
    }
    Err("Hiro API: max retries exceeded".into())
}

// --- Data fetching (single call, cached) ---

async fn fetch_and_cache_txs() -> Result<Vec<RawTxCacheEntry>, String> {
    if let Some(cached) = RAW_TX_CACHE.get().await {
        return Ok(cached);
    }

    let mut all = Vec::new();
    let mut offset = 0u64;
    let mut page = 0u32;

    loop {
        if page > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        page += 1;

        let url = format!(
            "{}/extended/v1/address/{}/transactions?limit=50&offset={}",
            HIRO_BASE, DEPLOYER, offset
        );
        let resp: HiroTxList = hiro_get(&url).await?;

        let count = resp.results.len() as u64;
        for tx in resp.results {
            let is_mint = tx.tx_status == "success"
                && tx.contract_call.as_ref().map(|cc| {
                    cc.function_name == "execute" && cc.contract_id.contains("mint-with-proof")
                }).unwrap_or(false);

            all.push(RawTxCacheEntry {
                tx_id: tx.tx_id,
                tx_type: tx.tx_type,
                tx_status: tx.tx_status,
                burn_block_time_iso: tx.burn_block_time_iso,
                fee_rate: tx.fee_rate,
                contract_id: tx.contract_call.as_ref().map(|cc| cc.contract_id.clone()),
                function_name: tx.contract_call.as_ref().map(|cc| cc.function_name.clone()),
                smart_contract_id: tx.smart_contract.as_ref().map(|sc| sc.contract_id.clone()),
                is_mint,
            });
        }
        offset += count;
        if offset >= resp.total || count == 0 {
            break;
        }
    }

    RAW_TX_CACHE.set(all.clone()).await;
    Ok(all)
}

// --- Helpers ---

fn parse_clarity_tuple(repr: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let inner = repr
        .strip_prefix("(tuple ")
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(repr);

    let mut key = String::new();
    let mut val = String::new();
    let mut in_val = false;
    let mut depth = 0;

    for ch in inner.chars() {
        match ch {
            '(' if in_val => {
                depth += 1;
                val.push(ch);
            }
            ')' if in_val && depth > 0 => {
                depth -= 1;
                val.push(ch);
            }
            ')' if in_val => {
                map.insert(key.trim().to_string(), val.trim().to_string());
                key.clear();
                val.clear();
                in_val = false;
            }
            ' ' if !in_val && !key.is_empty() => {
                in_val = true;
            }
            '(' if !in_val => {}
            _ if in_val => val.push(ch),
            _ => key.push(ch),
        }
    }
    if !key.is_empty() && !val.is_empty() {
        map.insert(key.trim().to_string(), val.trim().to_string());
    }
    map
}

fn extract_u(s: &str) -> u64 {
    s.strip_prefix('u')
        .and_then(|n| n.parse::<u64>().ok())
        .unwrap_or(0)
}

fn extract_principal(s: &str) -> String {
    s.strip_prefix('\'').unwrap_or(s).to_string()
}

fn extract_string(s: &str) -> String {
    s.trim_matches('"').to_string()
}

fn get_arg(args: &[HiroFunctionArg], name: &str) -> u64 {
    args.iter()
        .find(|a| a.name.as_deref() == Some(name))
        .and_then(|a| a.repr.as_deref())
        .map(|r| extract_u(r))
        .unwrap_or(0)
}

fn get_str_arg(args: &[HiroFunctionArg], name: &str) -> String {
    args.iter()
        .find(|a| a.name.as_deref() == Some(name))
        .and_then(|a| a.repr.as_deref())
        .map(|r| r.trim_matches('"').to_string())
        .unwrap_or_default()
}

fn decode_clarity_uint(hex: &str) -> u64 {
    let hex = hex.strip_prefix("0x").unwrap_or(hex);
    if hex.len() < 6 || !hex.starts_with("0701") {
        return 0;
    }
    let uint_hex = &hex[4..];
    u64::from_str_radix(uint_hex, 16).unwrap_or(0)
}

async fn read_total_supply(contract: &str) -> Result<u64, String> {
    let url = format!(
        "{}/v2/contracts/call-read/{}/{}/get-total-supply",
        HIRO_BASE, DEPLOYER, contract
    );
    let body = serde_json::json!({ "sender": DEPLOYER, "arguments": [] });
    let resp: HiroReadOnly = hiro_post(&url, &body).await?;

    if resp.okay != Some(true) {
        return Ok(0);
    }
    let result = resp.result.unwrap_or_default();
    Ok(decode_clarity_uint(&result))
}

#[allow(dead_code)]
async fn fetch_holder_count(token_id: &str) -> Result<u64, String> {
    let url = format!(
        "{}/extended/v1/tokens/ft/{}/holders?limit=1",
        HIRO_BASE, token_id
    );
    let resp: HiroHoldersList = hiro_get(&url).await?;
    Ok(resp.total.unwrap_or(resp.results.len() as u64))
}

async fn fetch_stx_balance(address: &str) -> Result<f64, String> {
    let url = format!("{}/extended/v1/address/{}/balances", HIRO_BASE, address);
    let resp: HiroAccountBalance = hiro_get(&url).await?;
    let micro = resp
        .stx
        .and_then(|s| s.balance)
        .and_then(|b| b.parse::<f64>().ok())
        .unwrap_or(0.0);
    Ok(micro / 1_000_000.0)
}

// --- DB row types for sqlx ---

#[derive(sqlx::FromRow)]
struct MintProofRow {
    tx_id: String,
    block_time: String,
    product: String,
    order_id: String,
    recipient: String,
    mint_amount: i64,
    total_minted: i64,
    price: f64,
    coinbase_fee: f64,
    bitflow_fee: f64,
    token_type: String,
    amount_deposited: f64,
}

#[derive(sqlx::FromRow)]
struct TxRow {
    tx_id: String,
    block_time: String,
    tx_type: String,
    tx_status: String,
    contract: String,
    function: String,
    fee_stx: f64,
}

#[derive(sqlx::FromRow)]
struct HolderRow {
    address: String,
    balance: f64,
}

// --- Public route handlers ---

pub async fn get_summary(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
) -> Result<Json<OnchainSummary>, (axum::http::StatusCode, String)> {
    if let Some(cached) = SUMMARY_CACHE.get().await {
        return Ok(Json(cached));
    }

    let pool = &state.pool;

    let vpaxg_holders: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM onchain_holders WHERE token = 'vpaxg'",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| err(e.to_string()))?;

    let vgld_holders: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM onchain_holders WHERE token = 'vgld'",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| err(e.to_string()))?;

    let total_unique_wallets: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (SELECT DISTINCT address FROM onchain_holders UNION SELECT DISTINCT recipient FROM onchain_mint_proofs) w",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| err(e.to_string()))?;

    let mints_vpaxg: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM onchain_mint_proofs WHERE product = 'vPAXG'",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| err(e.to_string()))?;

    let mints_vgld: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM onchain_mint_proofs WHERE product = 'vGLD'",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| err(e.to_string()))?;

    let total_txs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM onchain_transactions")
        .fetch_one(pool)
        .await
        .map_err(|e| err(e.to_string()))?;

    let success_txs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM onchain_transactions WHERE tx_status = 'success'",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| err(e.to_string()))?;

    let failed_txs = total_txs - success_txs;

    // These 3 Hiro calls are lightweight read-only contract calls (not paginated)
    let vpaxg_supply = read_total_supply("vpaxg-token").await.unwrap_or(0) as f64 / 1_000_000.0;
    let vgld_supply = read_total_supply("vgld-token-v4").await.unwrap_or(0) as f64 / 100_000_000.0;
    let deployer_bal = fetch_stx_balance(DEPLOYER).await.unwrap_or(0.0);

    let summary = OnchainSummary {
        vpaxg_total_supply: vpaxg_supply,
        vgld_total_supply: vgld_supply,
        vpaxg_holder_count: (vpaxg_holders + 2) as u64, // +2 protocol wallets used for internal testing
        vgld_holder_count: vgld_holders as u64,
        total_unique_wallets: (total_unique_wallets + 2) as u64, // +2 protocol wallets
        total_onchain_txs: total_txs as u64,
        success_txs: success_txs as u64,
        failed_txs: failed_txs as u64,
        success_rate: if total_txs > 0 { success_txs as f64 / total_txs as f64 * 100.0 } else { 0.0 },
        total_mints_vpaxg: mints_vpaxg as u64,
        total_mints_vgld: mints_vgld as u64,
        deployer_stx_balance: deployer_bal,
    };

    SUMMARY_CACHE.set(summary.clone()).await;
    Ok(Json(summary))
}

pub async fn get_transactions(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
) -> Result<Json<Vec<OnchainTx>>, (axum::http::StatusCode, String)> {
    if let Some(cached) = TX_CACHE.get().await {
        return Ok(Json(cached));
    }

    let rows = sqlx::query_as::<_, TxRow>(
        "SELECT tx_id, block_time, tx_type, tx_status, contract, function, fee_stx FROM onchain_transactions ORDER BY block_time DESC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| err(e.to_string()))?;

    let parsed: Vec<OnchainTx> = rows
        .into_iter()
        .map(|r| OnchainTx {
            tx_id: r.tx_id,
            block_time: r.block_time,
            tx_type: r.tx_type,
            contract: r.contract,
            function: r.function,
            status: r.tx_status,
            fee_stx: r.fee_stx,
        })
        .collect();

    TX_CACHE.set(parsed.clone()).await;
    Ok(Json(parsed))
}

pub async fn get_proofs(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
) -> Result<Json<Vec<MintProof>>, (axum::http::StatusCode, String)> {
    if let Some(cached) = PROOF_CACHE.get().await {
        return Ok(Json(cached));
    }

    let rows = sqlx::query_as::<_, MintProofRow>(
        "SELECT tx_id, block_time, product, order_id, recipient, mint_amount, total_minted, price, coinbase_fee, bitflow_fee, token_type, amount_deposited FROM onchain_mint_proofs ORDER BY block_time DESC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| err(e.to_string()))?;

    let proofs: Vec<MintProof> = rows
        .into_iter()
        .map(|r| MintProof {
            tx_id: r.tx_id,
            block_time: r.block_time,
            product: r.product,
            order_id: r.order_id,
            recipient: r.recipient,
            mint_amount: r.mint_amount as u64,
            total_minted: r.total_minted as u64,
            price: r.price,
            coinbase_fee: r.coinbase_fee,
            bitflow_fee: r.bitflow_fee,
            token_type: r.token_type,
            amount_deposited: r.amount_deposited,
        })
        .collect();

    PROOF_CACHE.set(proofs.clone()).await;
    Ok(Json(proofs))
}

// --- Public helpers for cross-module use ---

async fn fetch_all_token_holders(
    token_id: &str,
    decimals: f64,
    cache: &Cache<Vec<(String, f64)>>,
) -> Result<Vec<(String, f64)>, String> {
    if let Some(cached) = cache.get().await {
        return Ok(cached);
    }

    let mut all = Vec::new();
    let mut offset = 0u64;
    let mut page = 0u32;
    loop {
        if page > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        page += 1;

        let url = format!(
            "{}/extended/v1/tokens/ft/{}/holders?limit=50&offset={}",
            HIRO_BASE, token_id, offset
        );
        let resp: HiroHoldersList = hiro_get(&url).await?;
        let count = resp.results.len() as u64;
        for h in resp.results {
            if let (Some(addr), Some(bal_str)) = (h.address, h.balance) {
                let bal = bal_str.parse::<f64>().unwrap_or(0.0) / decimals;
                if bal > 0.0 {
                    all.push((addr, bal));
                }
            }
        }
        offset += count;
        let total = resp.total.unwrap_or(0);
        if count == 0 || offset >= total {
            break;
        }
    }

    cache.set(all.clone()).await;
    Ok(all)
}

pub async fn fetch_vpaxg_holders() -> Result<Vec<(String, f64)>, String> {
    fetch_all_token_holders(VPAXG_TOKEN, 1_000_000.0, &ALL_VPAXG_HOLDERS_CACHE).await
}

pub async fn fetch_vgld_holders() -> Result<Vec<(String, f64)>, String> {
    fetch_all_token_holders(VGLD_TOKEN, 100_000_000.0, &ALL_VGLD_HOLDERS_CACHE).await
}

#[allow(dead_code)]
pub async fn fetch_unique_user_count() -> Result<u64, String> {
    let vpaxg = fetch_vpaxg_holders().await?;
    let vgld = fetch_vgld_holders().await?;
    let mut addrs = std::collections::HashSet::new();
    for (a, _) in vpaxg {
        addrs.insert(a);
    }
    for (a, _) in vgld {
        addrs.insert(a);
    }
    Ok(addrs.len() as u64)
}

pub async fn invalidate_all_caches() {
    RAW_TX_CACHE.invalidate().await;
    TX_CACHE.invalidate().await;
    PROOF_CACHE.invalidate().await;
    SUMMARY_CACHE.invalidate().await;
    VPAXG_HOLDERS_CACHE.invalidate().await;
    VGLD_HOLDERS_CACHE.invalidate().await;
    ALL_VPAXG_HOLDERS_CACHE.invalidate().await;
    ALL_VGLD_HOLDERS_CACHE.invalidate().await;
}


/// Processed raw transaction for DB storage and sync use.
#[derive(Clone)]
pub struct RawTxEntry {
    pub tx_id: String,
    pub block_time: String,
    pub tx_type: String,
    pub status: String,
    pub contract: String,
    pub function: String,
    pub fee_stx: f64,
    pub is_mint: bool,
    pub is_burn: bool,
    pub token: String,
}

/// Fetch all raw transactions from Hiro, processed into RawTxEntry.
pub async fn fetch_all_raw_txs() -> Result<Vec<RawTxEntry>, String> {
    let txs = fetch_and_cache_txs().await?;
    Ok(txs
        .iter()
        .map(|t| {
            let block_time = t
                .burn_block_time_iso
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(19)
                .collect::<String>();
            let fee = t
                .fee_rate
                .as_deref()
                .and_then(|f| f.parse::<f64>().ok())
                .unwrap_or(0.0)
                / 1_000_000.0;

            let (tx_type, contract, function) = if let Some(cid) = &t.contract_id {
                let c = cid.split('.').last().unwrap_or("").to_string();
                let f = t.function_name.clone().unwrap_or_default();
                ("contract_call".to_string(), c, f)
            } else if let Some(sc) = &t.smart_contract_id {
                let c = sc.split('.').last().unwrap_or("").to_string();
                ("deploy".to_string(), c, String::new())
            } else {
                (t.tx_type.clone(), String::new(), String::new())
            };

            let is_burn = t.tx_status == "success"
                && t.function_name.as_deref() == Some("burn")
                && t.contract_id
                    .as_ref()
                    .map(|c| c.contains("vpaxg") || c.contains("vgld"))
                    .unwrap_or(false);

            let token = if is_burn {
                if t.contract_id.as_deref().unwrap_or("").contains("vpaxg") {
                    "vpaxg"
                } else {
                    "vgld"
                }
            } else {
                ""
            };

            RawTxEntry {
                tx_id: t.tx_id.clone(),
                block_time,
                tx_type,
                status: t.tx_status.clone(),
                contract,
                function,
                fee_stx: fee,
                is_mint: t.is_mint,
                is_burn,
                token: token.to_string(),
            }
        })
        .collect())
}

#[allow(dead_code)]
/// Fetch raw tx list and return all mint tx_ids + all burn tx entries (tx_id, token).
/// Used by sync to discover which txs exist on-chain.
pub async fn fetch_tx_ids() -> Result<(Vec<String>, Vec<(String, String, String)>), String> {
    let txs = fetch_and_cache_txs().await?;

    let mint_ids: Vec<String> = txs
        .iter()
        .filter(|t| t.is_mint)
        .map(|t| t.tx_id.clone())
        .collect();

    let burn_entries: Vec<(String, String, String)> = txs
        .iter()
        .filter(|t| {
            t.tx_status == "success"
                && t.function_name.as_deref() == Some("burn")
                && t.contract_id
                    .as_ref()
                    .map(|c| c.contains("vpaxg") || c.contains("vgld"))
                    .unwrap_or(false)
        })
        .map(|t| {
            let token = if t.contract_id.as_deref().unwrap_or("").contains("vpaxg") {
                "vpaxg"
            } else {
                "vgld"
            };
            let block_time = t
                .burn_block_time_iso
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(19)
                .collect::<String>();
            (t.tx_id.clone(), token.to_string(), block_time)
        })
        .collect();

    Ok((mint_ids, burn_entries))
}

/// Fetch proof details for a specific set of tx_ids only.
pub async fn fetch_proofs_for_ids(tx_ids: &[String]) -> Result<Vec<MintProof>, String> {
    let mut proofs = Vec::new();
    for (i, tx_id) in tx_ids.iter().enumerate() {
        if i > 0 && i % 5 == 0 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        let url = format!("{}/extended/v1/tx/{}", HIRO_BASE, tx_id);
        let detail: HiroTx = match hiro_get(&url).await {
            Ok(tx) => tx,
            Err(e) => {
                tracing::warn!("Skipping mint tx {}: {e}", &tx_id[..12.min(tx_id.len())]);
                continue;
            }
        };

        let cc = match &detail.contract_call {
            Some(cc) => cc,
            None => continue,
        };

        let is_vgld = cc.contract_id.contains("vgld");
        let product = if is_vgld { "vGLD" } else { "vPAXG" }.to_string();
        let args = cc.function_args.as_deref().unwrap_or(&[]);

        let block_time = detail
            .burn_block_time_iso
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(19)
            .collect::<String>();

        let coinbase_fee_raw = get_arg(args, "coinbase-fee");
        let bitflow_fee_raw = get_arg(args, "bitflow-fee");

        let price = if is_vgld {
            let nav = get_arg(args, "nav-price-at-mint");
            nav as f64 / 1_000_000.0
        } else {
            let usd_value = get_arg(args, "usd-value");
            let paxg_amount = get_arg(args, "paxg-amount");
            let derived = if paxg_amount > 0 {
                usd_value as f64 / paxg_amount as f64
            } else {
                0.0
            };
            if derived < 3000.0 { 4700.0 } else { derived }
        };

        let coinbase_fee = coinbase_fee_raw as f64 / 1_000_000.0;
        let bitflow_fee = bitflow_fee_raw as f64 / 1_000_000.0;
        let token_type = get_str_arg(args, "token-type");
        let amount_deposited = get_arg(args, "amount-deposited") as f64 / 1_000_000.0;

        for event in detail.events.as_deref().unwrap_or(&[]) {
            if event.event_type != "smart_contract_log" {
                continue;
            }
            let repr = event
                .contract_log
                .as_ref()
                .and_then(|cl| cl.value.as_ref())
                .and_then(|v| v.repr.as_deref())
                .unwrap_or("");

            if !repr.contains("reserve-proof-added") {
                continue;
            }

            let fields = parse_clarity_tuple(repr);

            let mint_amount = if is_vgld {
                fields.get("vgld-minted").map(|s| extract_u(s)).unwrap_or(0)
            } else {
                fields.get("paxg-amount").map(|s| extract_u(s)).unwrap_or(0)
            };
            let total_minted = if is_vgld {
                fields.get("total-supply").map(|s| extract_u(s)).unwrap_or(0)
            } else {
                fields.get("total-reserves").map(|s| extract_u(s)).unwrap_or(0)
            };

            proofs.push(MintProof {
                tx_id: tx_id.clone(),
                block_time: block_time.clone(),
                product: product.clone(),
                order_id: fields
                    .get("order-id")
                    .map(|s| extract_string(s))
                    .unwrap_or_default(),
                recipient: fields
                    .get("user-address")
                    .map(|s| extract_principal(s))
                    .unwrap_or_default(),
                mint_amount,
                total_minted,
                price,
                coinbase_fee,
                bitflow_fee,
                token_type: token_type.clone(),
                amount_deposited,
            });
        }
    }
    Ok(proofs)
}

/// Fetch burn details for a specific set of burn tx entries only.
pub async fn fetch_burns_for_ids(
    entries: &[(String, String, String)],
) -> Result<Vec<BurnEntry>, String> {
    let mut burns = Vec::new();
    for (i, (tx_id, token, block_time)) in entries.iter().enumerate() {
        if i > 0 && i % 5 == 0 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        let url = format!("{}/extended/v1/tx/{}", HIRO_BASE, tx_id);
        let detail: HiroTx = match hiro_get(&url).await {
            Ok(d) => d,
            Err(_) => {
                burns.push(BurnEntry {
                    tx_id: tx_id.clone(),
                    block_time: block_time.clone(),
                    token: token.clone(),
                    amount: 0,
                });
                continue;
            }
        };

        let amount = detail
            .contract_call
            .as_ref()
            .and_then(|cc| cc.function_args.as_deref())
            .and_then(|args| {
                args.iter()
                    .find(|a| a.name.as_deref() == Some("amount"))
                    .and_then(|a| a.repr.as_deref())
                    .map(|r| extract_u(r))
            })
            .unwrap_or(0);

        burns.push(BurnEntry {
            tx_id: tx_id.clone(),
            block_time: block_time.clone(),
            token: token.clone(),
            amount: amount as i64,
        });
    }
    Ok(burns)
}

#[derive(Clone)]
pub struct BurnEntry {
    pub tx_id: String,
    pub block_time: String,
    pub token: String,
    pub amount: i64,
}

#[allow(dead_code)]
pub async fn fetch_burns_data() -> Result<Vec<BurnEntry>, String> {
    let txs = fetch_and_cache_txs().await?;
    let mut burns = Vec::new();

    let mut burn_idx = 0usize;
    for tx in &txs {
        if tx.tx_status != "success" {
            continue;
        }
        if tx.function_name.as_deref() != Some("burn") {
            continue;
        }
        let contract_id = match &tx.contract_id {
            Some(c) => c,
            None => continue,
        };
        let token = if contract_id.contains("vpaxg") {
            "vpaxg"
        } else if contract_id.contains("vgld") {
            "vgld"
        } else {
            continue;
        };

        if burn_idx > 0 && burn_idx % 5 == 0 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        burn_idx += 1;

        let block_time = tx
            .burn_block_time_iso
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(19)
            .collect::<String>();

        let url = format!("{}/extended/v1/tx/{}", HIRO_BASE, tx.tx_id);
        let detail: HiroTx = match hiro_get(&url).await {
            Ok(d) => d,
            Err(_) => {
                burns.push(BurnEntry {
                    tx_id: tx.tx_id.clone(),
                    block_time,
                    token: token.to_string(),
                    amount: 0,
                });
                continue;
            }
        };

        let amount = detail
            .contract_call
            .as_ref()
            .and_then(|cc| cc.function_args.as_deref())
            .and_then(|args| {
                args.iter()
                    .find(|a| a.name.as_deref() == Some("amount"))
                    .and_then(|a| a.repr.as_deref())
                    .map(|r| extract_u(r))
            })
            .unwrap_or(0);

        burns.push(BurnEntry {
            tx_id: tx.tx_id.clone(),
            block_time,
            token: token.to_string(),
            amount: amount as i64,
        });
    }

    burns.sort_by(|a, b| b.block_time.cmp(&a.block_time));
    Ok(burns)
}

// --- Holder endpoint (reads from DB) ---

pub async fn get_holders(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
    axum::extract::Path(token): axum::extract::Path<String>,
) -> Result<Json<OnchainHolders>, (axum::http::StatusCode, String)> {
    let cache = match token.as_str() {
        "vpaxg" => &*VPAXG_HOLDERS_CACHE,
        "vgld" => &*VGLD_HOLDERS_CACHE,
        _ => return Err((axum::http::StatusCode::BAD_REQUEST, "Invalid token".into())),
    };

    if let Some(cached) = cache.get().await {
        return Ok(Json(cached));
    }

    let label = match token.as_str() {
        "vpaxg" => "vPAXG",
        _ => "vGLD",
    };

    let rows = sqlx::query_as::<_, HolderRow>(
        "SELECT address, balance FROM onchain_holders WHERE token = $1 ORDER BY balance DESC",
    )
    .bind(&token)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| err(e.to_string()))?;

    let total_supply: f64 = rows.iter().map(|r| r.balance).sum();

    let holders: Vec<TokenHolder> = rows
        .iter()
        .map(|r| {
            let pct = if total_supply > 0.0 {
                r.balance / total_supply * 100.0
            } else {
                0.0
            };
            TokenHolder {
                address: r.address.clone(),
                balance: r.balance,
                percentage: pct,
            }
        })
        .collect();

    let top10_bal: f64 = holders.iter().take(10).map(|h| h.balance).sum();
    let top10_concentration = if total_supply > 0.0 {
        top10_bal / total_supply * 100.0
    } else {
        0.0
    };

    let result = OnchainHolders {
        token: label.to_string(),
        total_supply,
        holder_count: holders.len() as u64,
        holders,
        top10_concentration,
    };

    cache.set(result.clone()).await;
    Ok(Json(result))
}
