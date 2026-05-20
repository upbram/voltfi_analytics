const API_BASE = '';

function setKpi(id, value, sub) {
    const card = document.getElementById(id);
    if (!card) return;
    card.classList.remove('skeleton');
    card.querySelector('.kpi-value').textContent = value;
    if (sub) {
        let subEl = card.querySelector('.kpi-sub');
        if (!subEl) {
            subEl = document.createElement('div');
            subEl.className = 'kpi-sub';
            card.appendChild(subEl);
        }
        subEl.textContent = sub;
    }
}

function setKpiClass(id, cls) {
    const card = document.getElementById(id);
    if (!card) return;
    card.classList.remove('positive', 'warning', 'negative');
    if (cls) card.classList.add(cls);
}

async function fetchOverview() {
    const errorBanner = document.getElementById('overview-error');
    try {
        const res = await fetch(API_BASE + '/api/analytics/overview');
        if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
        const d = await res.json();

        setKpi('kpi-tvl', formatUsd(d.total_tvl_usd));
        setKpi('kpi-users', d.total_users.toLocaleString());
        setKpi('kpi-apy', formatPercent(d.current_apy));
        setKpiClass('kpi-apy', 'positive');
        setKpi('kpi-nav', '$' + d.vgld_nav_price.toFixed(4));
        setKpi('kpi-deposits-24h', formatUsd(d.deposit_volume_24h_usd));
        setKpi('kpi-withdrawals-24h', formatUsd(d.withdrawal_volume_24h_usd));

        setKpi('kpi-pending', d.pending_deposits.toString());
        if (d.pending_deposits > 0) setKpiClass('kpi-pending', 'warning');

        if (d.reserve_ratio !== null) {
            setKpi('kpi-reserve', d.reserve_ratio.toFixed(4) + 'x');
            setKpiClass('kpi-reserve', d.reserve_ratio >= 1.0 ? 'positive' : 'negative');
        } else {
            setKpi('kpi-reserve', '—', 'No proof yet');
        }

        setKpi('kpi-paxg', d.total_paxg_oz.toFixed(4) + ' oz');
        setKpi('kpi-gold', formatUsd(d.gold_spot_price), '/oz');
        setKpi('kpi-supply', formatNumber(d.total_vgld_supply, 4));

        errorBanner.style.display = 'none';

        document.getElementById('last-updated').textContent =
            'Updated ' + new Date().toLocaleTimeString();
    } catch (err) {
        console.error('Failed to fetch overview:', err);
        errorBanner.textContent = 'Failed to load data: ' + err.message;
        errorBanner.style.display = 'block';

        document.querySelectorAll('.kpi-card.skeleton').forEach(card => {
            card.classList.remove('skeleton');
            card.querySelector('.kpi-value').textContent = '—';
        });
    }
}

// --- Phase 2: Business data fetching ---

let currentPeriod = '30d';
let businessLoaded = false;
let usersLoaded = false;
let opsLoaded = false;
let bitflowLoaded = false;
let onchainLoaded = false;

async function fetchBusiness() {
    const errorBanner = document.getElementById('business-error');
    try {
        const [volRes, revRes, yldRes] = await Promise.all([
            fetch(`${API_BASE}/api/analytics/volume?period=${currentPeriod}`),
            fetch(`${API_BASE}/api/analytics/revenue?period=${currentPeriod}`),
            fetch(`${API_BASE}/api/analytics/yield`),
        ]);

        if (!volRes.ok) throw new Error(`Volume: HTTP ${volRes.status}`);
        if (!revRes.ok) throw new Error(`Revenue: HTTP ${revRes.status}`);
        if (!yldRes.ok) throw new Error(`Yield: HTTP ${yldRes.status}`);

        const [volData, revData, yldData] = await Promise.all([
            volRes.json(), revRes.json(), yldRes.json(),
        ]);

        renderVolumeChart(volData);
        renderRevenueChart(revData);
        renderYieldChart(yldData);

        if (errorBanner) errorBanner.style.display = 'none';
        businessLoaded = true;
    } catch (err) {
        console.error('Failed to fetch business data:', err);
        if (errorBanner) {
            errorBanner.textContent = 'Failed to load business data: ' + err.message;
            errorBanner.style.display = 'block';
        }
    }
}

// --- Phase 3: Users data fetching ---

async function fetchUsers() {
    const errorBanner = document.getElementById('users-error');
    try {
        const [growthRes, activeRes, holdersRes] = await Promise.all([
            fetch(`${API_BASE}/api/analytics/users/growth?period=90d`),
            fetch(`${API_BASE}/api/analytics/users/active?period=30d`),
            fetch(`${API_BASE}/api/analytics/users/top-holders?limit=20`),
        ]);

        if (!growthRes.ok) throw new Error(`Growth: HTTP ${growthRes.status}`);
        if (!activeRes.ok) throw new Error(`Active: HTTP ${activeRes.status}`);
        if (!holdersRes.ok) throw new Error(`Holders: HTTP ${holdersRes.status}`);

        const [growthData, activeData, holdersData] = await Promise.all([
            growthRes.json(), activeRes.json(), holdersRes.json(),
        ]);

        setKpi('kpi-total-users-tab', growthData.total_users.toLocaleString());
        setKpi('kpi-wau', activeData.wau.toLocaleString());
        setKpi('kpi-mau', activeData.mau.toLocaleString());

        renderGrowthChart(growthData);
        renderDauChart(activeData);
        renderHoldersTable(holdersData);

        if (errorBanner) errorBanner.style.display = 'none';
        usersLoaded = true;
    } catch (err) {
        console.error('Failed to fetch users data:', err);
        if (errorBanner) {
            errorBanner.textContent = 'Failed to load user data: ' + err.message;
            errorBanner.style.display = 'block';
        }
    }
}

// --- Phase 4: Ops data fetching ---

function formatMinutes(val) {
    if (val >= 60) return (val / 60).toFixed(1) + 'h';
    return val.toFixed(1) + 'm';
}

async function fetchOps() {
    const errorBanner = document.getElementById('ops-error');
    try {
        const [funnelRes, procRes, hedgeRes] = await Promise.all([
            fetch(`${API_BASE}/api/analytics/ops/funnel`),
            fetch(`${API_BASE}/api/analytics/ops/processing-time?period=30d`),
            fetch(`${API_BASE}/api/analytics/ops/hedge-coverage`),
        ]);

        if (!funnelRes.ok) throw new Error(`Funnel: HTTP ${funnelRes.status}`);
        if (!procRes.ok) throw new Error(`Processing: HTTP ${procRes.status}`);
        if (!hedgeRes.ok) throw new Error(`Hedge: HTTP ${hedgeRes.status}`);

        const [funnelData, procData, hedgeData] = await Promise.all([
            funnelRes.json(), procRes.json(), hedgeRes.json(),
        ]);

        setKpi('kpi-paxg-held', hedgeData.total_paxg_oz.toFixed(6) + ' oz');
        setKpi('kpi-futures-short', hedgeData.futures_short_oz.toFixed(6) + ' oz');
        const coveragePct = (hedgeData.coverage_ratio * 100).toFixed(1) + '%';
        setKpi('kpi-hedge-ratio', coveragePct);
        setKpiClass('kpi-hedge-ratio', hedgeData.coverage_ratio >= 0.95 ? 'positive' : hedgeData.coverage_ratio >= 0.5 ? 'warning' : 'negative');
        setKpi('kpi-open-positions', hedgeData.open_positions.toString(), '$' + hedgeData.total_margin_usd.toFixed(2) + ' margin');
        setKpi('kpi-p50', formatMinutes(procData.overall_p50));
        setKpi('kpi-p90', formatMinutes(procData.overall_p90));

        renderFunnelChart(funnelData);
        renderProcessingChart(procData);
        renderHedgeChart(hedgeData);

        if (errorBanner) errorBanner.style.display = 'none';
        opsLoaded = true;
    } catch (err) {
        console.error('Failed to fetch ops data:', err);
        if (errorBanner) {
            errorBanner.textContent = 'Failed to load ops data: ' + err.message;
            errorBanner.style.display = 'block';
        }
    }
}

// --- Bitflow tab ---

let bitflowVolumeChart = null;
let bitflowFeesChart = null;

async function fetchBitflow() {
    const errorBanner = document.getElementById('bitflow-error');
    try {
        const res = await fetch(`${API_BASE}/api/analytics/bitflow`);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const data = await res.json();

        setKpi('kpi-bf-total-usdc', formatUsd(data.total_usdc_volume));
        setKpi('kpi-bf-total-fees', formatUsd(data.total_bitflow_fees));
        setKpi('kpi-bf-total-txns', data.total_txn_count.toString());
        setKpi('kpi-bf-avg-size', formatUsd(data.avg_swap_size));

        renderBitflowCharts(data);
        renderBitflowTable(data.points);

        if (errorBanner) errorBanner.style.display = 'none';
        bitflowLoaded = true;
    } catch (err) {
        console.error('Failed to fetch Bitflow data:', err);
        if (errorBanner) {
            errorBanner.textContent = 'Failed to load Bitflow data: ' + err.message;
            errorBanner.style.display = 'block';
        }
    }
}

function renderBitflowCharts(data) {
    const labels = data.points.map(p => p.date);
    const volumes = data.points.map(p => p.usdc_volume);
    const fees = data.points.map(p => p.bitflow_fee);

    const volCtx = document.getElementById('bitflow-volume-chart');
    if (bitflowVolumeChart) bitflowVolumeChart.destroy();
    bitflowVolumeChart = new Chart(volCtx, {
        type: 'bar',
        data: {
            labels,
            datasets: [{
                label: 'USDC Volume',
                data: volumes,
                backgroundColor: 'rgba(99, 102, 241, 0.7)',
                borderColor: 'rgb(99, 102, 241)',
                borderWidth: 1,
            }],
        },
        options: {
            responsive: true,
            maintainAspectRatio: false,
            plugins: { legend: { display: false } },
            scales: {
                y: {
                    beginAtZero: true,
                    ticks: { callback: v => '$' + v.toFixed(0) },
                },
            },
        },
    });

    const feeCtx = document.getElementById('bitflow-fees-chart');
    if (bitflowFeesChart) bitflowFeesChart.destroy();
    bitflowFeesChart = new Chart(feeCtx, {
        type: 'bar',
        data: {
            labels,
            datasets: [{
                label: 'Bitflow Fee',
                data: fees,
                backgroundColor: 'rgba(245, 158, 11, 0.7)',
                borderColor: 'rgb(245, 158, 11)',
                borderWidth: 1,
            }],
        },
        options: {
            responsive: true,
            maintainAspectRatio: false,
            plugins: { legend: { display: false } },
            scales: {
                y: {
                    beginAtZero: true,
                    ticks: { callback: v => '$' + v.toFixed(2) },
                },
            },
        },
    });
}

function renderBitflowTable(points) {
    const tbody = document.getElementById('bitflow-tbody');
    if (!tbody) return;
    if (points.length === 0) {
        tbody.innerHTML = '<tr><td colspan="4" class="empty-cell">No Bitflow swap data</td></tr>';
        return;
    }
    tbody.innerHTML = [...points].reverse().map(p => `<tr>
        <td>${p.date}</td>
        <td>${formatUsd(p.usdc_volume)}</td>
        <td>$${p.bitflow_fee.toFixed(4)}</td>
        <td>${p.txn_count}</td>
    </tr>`).join('');
}

// --- Phase 4b: On-chain data fetching ---

function shortAddr(a) {
    if (!a || a.length < 14) return a || '—';
    return a.slice(0, 6) + '...' + a.slice(-4);
}

function explorerLink(txId) {
    const short = txId.slice(0, 10) + '...';
    return `<a href="https://explorer.hiro.so/txid/${txId}?chain=mainnet" target="_blank" class="explorer-link">${short}</a>`;
}

async function fetchOnchain() {
    const errorBanner = document.getElementById('onchain-error');
    const errors = [];

    const [summaryRes, proofsRes, txsRes, vpaxgHoldersRes, vgldHoldersRes, mintBurnRes] = await Promise.all([
        fetch(`${API_BASE}/api/analytics/onchain/summary`).catch(e => e),
        fetch(`${API_BASE}/api/analytics/onchain/proofs`).catch(e => e),
        fetch(`${API_BASE}/api/analytics/onchain/transactions`).catch(e => e),
        fetch(`${API_BASE}/api/analytics/onchain/holders/vpaxg`).catch(e => e),
        fetch(`${API_BASE}/api/analytics/onchain/holders/vgld`).catch(e => e),
        fetch(`${API_BASE}/api/analytics/mint-burn`).catch(e => e),
    ]);

    if (summaryRes.ok) {
        try {
            const summary = await summaryRes.json();
            setKpi('kpi-vpaxg-supply', summary.vpaxg_total_supply.toFixed(6) + ' oz');
            setKpi('kpi-vgld-supply', summary.vgld_total_supply.toFixed(4) + ' vGLD');
            setKpi('kpi-vpaxg-holders', summary.vpaxg_holder_count.toString());
            setKpi('kpi-vgld-holders', summary.vgld_holder_count.toString());
            setKpi('kpi-total-unique-wallets', summary.total_unique_wallets.toString());
            setKpi('kpi-total-txs', summary.total_onchain_txs.toString(), `${summary.success_txs} ok / ${summary.failed_txs} failed`);
            setKpi('kpi-success-rate', summary.success_rate.toFixed(1) + '%');
            setKpiClass('kpi-success-rate', summary.success_rate >= 80 ? 'positive' : summary.success_rate >= 50 ? 'warning' : 'negative');
            setKpi('kpi-mints-vpaxg', summary.total_mints_vpaxg.toString());
            setKpi('kpi-mints-vgld', summary.total_mints_vgld.toString());
            setKpi('kpi-deployer-bal', summary.deployer_stx_balance.toFixed(2) + ' STX');
        } catch (e) { errors.push('Summary: ' + e.message); }
    } else {
        errors.push('Summary: HTTP ' + (summaryRes.status || 'network error'));
    }

    if (proofsRes.ok) {
        try {
            const proofs = await proofsRes.json();
            renderProofsTable(proofs);
        } catch (e) { errors.push('Proofs: ' + e.message); }
    } else {
        errors.push('Proofs: HTTP ' + (proofsRes.status || 'network error'));
    }

    if (txsRes.ok) {
        try {
            const txs = await txsRes.json();
            renderTxsTable(txs);
        } catch (e) { errors.push('Txs: ' + e.message); }
    } else {
        errors.push('Txs: HTTP ' + (txsRes.status || 'network error'));
    }

    if (vpaxgHoldersRes.ok) {
        try { renderTokenHolders('vpaxg', await vpaxgHoldersRes.json()); }
        catch (e) { errors.push('vPAXG holders: ' + e.message); }
    }
    if (vgldHoldersRes.ok) {
        try { renderTokenHolders('vgld', await vgldHoldersRes.json()); }
        catch (e) { errors.push('vGLD holders: ' + e.message); }
    }

    if (mintBurnRes.ok) {
        try {
            const mb = await mintBurnRes.json();
            renderMintBurnTable(mb);
        } catch (e) { errors.push('Mint/Burn: ' + e.message); }
    } else {
        errors.push('Mint/Burn: HTTP ' + (mintBurnRes.status || 'network error'));
    }

    if (errors.length > 0) {
        console.error('On-chain partial failures:', errors);
        if (errorBanner) {
            errorBanner.textContent = 'Some on-chain data failed to load: ' + errors.join('; ');
            errorBanner.style.display = 'block';
        }
    } else {
        if (errorBanner) errorBanner.style.display = 'none';
    }
    onchainLoaded = true;
}

function renderMintBurnTable(data) {
    const tbody = document.getElementById('mint-burn-tbody');
    if (!tbody) return;
    tbody.innerHTML = `
        <tr>
            <td><span class="badge badge-green">vPAXG</span></td>
            <td>${data.vpaxg_mints}</td>
            <td>${data.vpaxg_mint_amount.toFixed(6)} oz</td>
            <td>${data.vpaxg_burns}</td>
            <td>${data.vpaxg_burn_amount.toFixed(6)} oz</td>
            <td>${data.vpaxg_net}</td>
            <td><strong>${data.vpaxg_net_amount.toFixed(6)} oz</strong></td>
        </tr>
        <tr>
            <td><span class="badge badge-orange">vGLD</span></td>
            <td>${data.vgld_mints}</td>
            <td>${data.vgld_mint_amount.toFixed(6)}</td>
            <td>${data.vgld_burns}</td>
            <td>${data.vgld_burn_amount.toFixed(6)}</td>
            <td>${data.vgld_net}</td>
            <td><strong>${data.vgld_net_amount.toFixed(6)}</strong></td>
        </tr>
    `;
}

function renderProofsTable(proofs) {
    const tbody = document.getElementById('proofs-tbody');
    if (!tbody) return;
    if (proofs.length === 0) {
        tbody.innerHTML = '<tr><td colspan="11" class="empty-cell">No mint proofs found</td></tr>';
        return;
    }
    tbody.innerHTML = proofs.map(p => {
        const dec = p.product === 'vGLD' ? 8 : 6;
        const minted = (p.mint_amount / Math.pow(10, dec)).toFixed(dec);
        const totalMinted = (p.total_minted / Math.pow(10, dec)).toFixed(dec);
        const priceLabel = p.product === 'vGLD' ? ' NAV' : '/oz';
        const priceStr = p.price > 0
            ? '$' + p.price.toLocaleString(undefined, {minimumFractionDigits: 2, maximumFractionDigits: 2}) + priceLabel
            : '—';
        const cbFee = '$' + p.coinbase_fee.toFixed(2);
        const bfFee = '$' + p.bitflow_fee.toFixed(2);
        const depAmt = p.amount_deposited > 0
            ? '$' + p.amount_deposited.toFixed(2) + (p.token_type ? ' ' + p.token_type.toUpperCase() : '')
            : '—';
        return `<tr>
            <td>${formatTime(p.block_time)}</td>
            <td><span class="badge badge-${p.product === 'vPAXG' ? 'green' : 'orange'}">${p.product}</span></td>
            <td title="${p.order_id}">${p.order_id.slice(0, 8)}...</td>
            <td title="${p.recipient}">${shortAddr(p.recipient)}</td>
            <td>${depAmt}</td>
            <td>${minted}</td>
            <td>${totalMinted}</td>
            <td>${priceStr}</td>
            <td>${cbFee}</td>
            <td>${bfFee}</td>
            <td>${explorerLink(p.tx_id)}</td>
        </tr>`;
    }).join('');
}

function renderTxsTable(txs) {
    const tbody = document.getElementById('txs-tbody');
    if (!tbody) return;
    if (txs.length === 0) {
        tbody.innerHTML = '<tr><td colspan="7" class="empty-cell">No transactions found</td></tr>';
        return;
    }
    tbody.innerHTML = txs.map(tx => {
        const statusClass = tx.status === 'success' ? 'badge-green' : 'badge-red';
        const typeLabel = tx.tx_type === 'deploy' ? 'Deploy' : tx.tx_type === 'token_transfer' ? 'Transfer' : 'Call';
        return `<tr>
            <td>${formatTime(tx.block_time)}</td>
            <td>${typeLabel}</td>
            <td>${tx.contract || '—'}</td>
            <td>${tx.function || '—'}</td>
            <td><span class="badge ${statusClass}">${tx.status}</span></td>
            <td>${tx.fee_stx.toFixed(4)} STX</td>
            <td>${explorerLink(tx.tx_id)}</td>
        </tr>`;
    }).join('');
}

function renderTokenHolders(token, data) {
    const tbody = document.getElementById(`${token}-holders-tbody`);
    if (!tbody) return;
    if (data.holders.length === 0) {
        tbody.innerHTML = '<tr><td colspan="3" class="empty-cell">No holders</td></tr>';
        return;
    }
    tbody.innerHTML = data.holders.map(h => `<tr>
        <td title="${h.address}">${shortAddr(h.address)}</td>
        <td>${h.balance.toFixed(6)}</td>
        <td>${h.percentage.toFixed(1)}%</td>
    </tr>`).join('');
}

// Period selector
document.querySelectorAll('.period-btn').forEach(btn => {
    btn.addEventListener('click', () => {
        document.querySelectorAll('.period-btn').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        currentPeriod = btn.dataset.period;
        fetchBusiness();
    });
});

// Tab switching
document.querySelectorAll('.nav-item:not(.disabled)').forEach(item => {
    item.addEventListener('click', (e) => {
        e.preventDefault();
        const tab = item.dataset.tab;

        document.querySelectorAll('.nav-item').forEach(n => n.classList.remove('active'));
        item.classList.add('active');

        document.querySelectorAll('.tab-content').forEach(t => t.classList.remove('active'));
        const target = document.getElementById('tab-' + tab);
        if (target) target.classList.add('active');

        document.querySelector('.page-title').textContent =
            tab.charAt(0).toUpperCase() + tab.slice(1);

        if (tab === 'business' && !businessLoaded) fetchBusiness();
        if (tab === 'users' && !usersLoaded) fetchUsers();
        if (tab === 'operations' && !opsLoaded) fetchOps();
        if (tab === 'bitflow' && !bitflowLoaded) fetchBitflow();
        if (tab === 'onchain' && !onchainLoaded) fetchOnchain();
    });
});

// Refresh button — triggers on-chain sync then reloads all data
document.getElementById('refresh-btn').addEventListener('click', async () => {
    const btn = document.getElementById('refresh-btn');
    btn.disabled = true;
    btn.textContent = 'Syncing...';

    try {
        const res = await fetch(API_BASE + '/api/analytics/refresh', { method: 'POST' });
        if (!res.ok) {
            const msg = await res.text();
            console.error('Refresh sync failed:', msg);
        }
    } catch (err) {
        console.error('Refresh sync error:', err);
    }

    btn.disabled = false;
    btn.textContent = 'Refresh';

    fetchOverview();
    const activeTab = document.querySelector('.nav-item.active')?.dataset.tab;
    if (activeTab === 'business') fetchBusiness();
    if (activeTab === 'users') { usersLoaded = false; fetchUsers(); }
    if (activeTab === 'operations') fetchOps();
    if (activeTab === 'bitflow') { bitflowLoaded = false; fetchBitflow(); }
    if (activeTab === 'onchain') { onchainLoaded = false; fetchOnchain(); }
});

// Initial load
fetchOverview();
