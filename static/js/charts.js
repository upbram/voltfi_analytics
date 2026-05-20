// --- Shared formatters ---

const MONTHS = ['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec'];

function formatTime(iso) {
    if (!iso) return '—';
    const d = new Date(iso.endsWith('Z') ? iso : iso + 'Z');
    if (isNaN(d)) return iso;
    const mon = MONTHS[d.getUTCMonth()];
    const day = d.getUTCDate();
    const h = String(d.getUTCHours()).padStart(2, '0');
    const m = String(d.getUTCMinutes()).padStart(2, '0');
    return `${mon} ${day}, ${h}:${m}`;
}

function formatDate(iso) {
    if (!iso) return '—';
    const d = new Date(iso);
    if (isNaN(d)) return iso;
    const mon = MONTHS[d.getUTCMonth()];
    const day = d.getUTCDate();
    const yr = d.getUTCFullYear();
    return `${mon} ${day}, ${yr}`;
}

function formatUsd(val) {
    if (val >= 1_000_000) return '$' + (val / 1_000_000).toFixed(2) + 'M';
    if (val >= 1_000) return '$' + (val / 1_000).toFixed(1) + 'K';
    return '$' + val.toFixed(2);
}

function formatNumber(val, decimals = 2) {
    if (val >= 1_000_000) return (val / 1_000_000).toFixed(decimals) + 'M';
    if (val >= 1_000) return (val / 1_000).toFixed(decimals) + 'K';
    return val.toFixed(decimals);
}

function formatPercent(val) {
    if (val === null || val === undefined) return '—';
    return val.toFixed(2) + '%';
}

// --- Chart config ---

const CHART_COLORS = {
    deposit: '#22c55e',
    depositBg: 'rgba(34, 197, 94, 0.15)',
    withdrawal: '#ef4444',
    withdrawalBg: 'rgba(239, 68, 68, 0.15)',
    coinbase: '#3b82f6',
    coinbaseBg: 'rgba(59, 130, 246, 0.3)',
    bitflow: '#a855f7',
    bitflowBg: 'rgba(168, 85, 247, 0.3)',
    profit: '#f0b90b',
    profitBg: 'rgba(240, 185, 11, 0.3)',
    yield: '#22c55e',
    yieldBg: 'rgba(34, 197, 94, 0.1)',
    cumulative: '#f0b90b',
    cumulativeBg: 'rgba(240, 185, 11, 0.1)',
    grid: 'rgba(42, 53, 80, 0.5)',
    text: '#8b9dc3',
};

const CHART_DEFAULTS = {
    responsive: true,
    maintainAspectRatio: false,
    plugins: {
        legend: {
            labels: { color: CHART_COLORS.text, boxWidth: 12, padding: 16 },
        },
        tooltip: {
            backgroundColor: '#1a2235',
            borderColor: '#2a3550',
            borderWidth: 1,
            titleColor: '#f0f4f8',
            bodyColor: '#8b9dc3',
            padding: 10,
        },
    },
    scales: {
        x: {
            ticks: { color: CHART_COLORS.text, maxRotation: 45 },
            grid: { color: CHART_COLORS.grid },
        },
        y: {
            ticks: { color: CHART_COLORS.text },
            grid: { color: CHART_COLORS.grid },
        },
    },
};

let volumeChart = null;
let revenueChart = null;
let yieldChart = null;
let growthChart = null;
let dauChart = null;
let funnelChart = null;
let processingChart = null;
let hedgeChart = null;

function destroyChart(chart) {
    if (chart) chart.destroy();
    return null;
}

function renderVolumeChart(data) {
    const ctx = document.getElementById('volume-chart');
    if (!ctx) return;
    volumeChart = destroyChart(volumeChart);

    const labels = data.points.map(p => p.date.slice(5));
    volumeChart = new Chart(ctx, {
        type: 'bar',
        data: {
            labels,
            datasets: [
                {
                    label: 'Deposits',
                    data: data.points.map(p => p.deposit_usd),
                    backgroundColor: CHART_COLORS.depositBg,
                    borderColor: CHART_COLORS.deposit,
                    borderWidth: 1,
                    borderRadius: 4,
                },
                {
                    label: 'Withdrawals',
                    data: data.points.map(p => p.withdrawal_usd),
                    backgroundColor: CHART_COLORS.withdrawalBg,
                    borderColor: CHART_COLORS.withdrawal,
                    borderWidth: 1,
                    borderRadius: 4,
                },
            ],
        },
        options: {
            ...CHART_DEFAULTS,
            plugins: {
                ...CHART_DEFAULTS.plugins,
                tooltip: {
                    ...CHART_DEFAULTS.plugins.tooltip,
                    callbacks: {
                        label: (ctx) => `${ctx.dataset.label}: $${ctx.raw.toFixed(2)}`,
                    },
                },
            },
        },
    });

    const summary = document.getElementById('volume-summary');
    if (summary) {
        summary.innerHTML = `
            <span class="summary-item positive">Deposits: ${formatUsd(data.total_deposits_usd)} (${data.total_deposit_count})</span>
            <span class="summary-item negative">Withdrawals: ${formatUsd(data.total_withdrawals_usd)} (${data.total_withdrawal_count})</span>
        `;
    }
}

function renderRevenueChart(data) {
    const ctx = document.getElementById('revenue-chart');
    if (!ctx) return;
    revenueChart = destroyChart(revenueChart);

    const labels = data.points.map(p => p.date.slice(5));
    revenueChart = new Chart(ctx, {
        type: 'bar',
        data: {
            labels,
            datasets: [
                {
                    label: 'Coinbase Fees',
                    data: data.points.map(p => p.coinbase_fees),
                    backgroundColor: CHART_COLORS.coinbaseBg,
                    borderColor: CHART_COLORS.coinbase,
                    borderWidth: 1,
                    borderRadius: 4,
                    stack: 'fees',
                },
                {
                    label: 'Bitflow Fees',
                    data: data.points.map(p => p.bitflow_fees),
                    backgroundColor: CHART_COLORS.bitflowBg,
                    borderColor: CHART_COLORS.bitflow,
                    borderWidth: 1,
                    borderRadius: 4,
                    stack: 'fees',
                },
                {
                    label: 'Profit Fees (20%)',
                    data: data.points.map(p => p.profit_fees),
                    backgroundColor: CHART_COLORS.profitBg,
                    borderColor: CHART_COLORS.profit,
                    borderWidth: 1,
                    borderRadius: 4,
                    stack: 'fees',
                },
            ],
        },
        options: {
            ...CHART_DEFAULTS,
            scales: {
                ...CHART_DEFAULTS.scales,
                x: { ...CHART_DEFAULTS.scales.x, stacked: true },
                y: { ...CHART_DEFAULTS.scales.y, stacked: true },
            },
            plugins: {
                ...CHART_DEFAULTS.plugins,
                tooltip: {
                    ...CHART_DEFAULTS.plugins.tooltip,
                    callbacks: {
                        label: (ctx) => `${ctx.dataset.label}: $${ctx.raw.toFixed(2)}`,
                    },
                },
            },
        },
    });

    const summary = document.getElementById('revenue-summary');
    if (summary) {
        summary.innerHTML = `
            <span class="summary-item" style="color:#3b82f6">Coinbase: ${formatUsd(data.total_coinbase_fees)}</span>
            <span class="summary-item" style="color:#a855f7">Bitflow: ${formatUsd(data.total_bitflow_fees)}</span>
            <span class="summary-item" style="color:#f0b90b">Profit: ${formatUsd(data.total_profit_fees)}</span>
            <span class="summary-item">Total: ${formatUsd(data.total_revenue)}</span>
        `;
    }
}

function renderYieldChart(data) {
    const ctx = document.getElementById('yield-chart');
    if (!ctx) return;
    yieldChart = destroyChart(yieldChart);

    if (data.rolls.length === 0) {
        const summary = document.getElementById('yield-summary');
        if (summary) summary.innerHTML = '<span class="summary-item">No roll data yet</span>';
        return;
    }

    const labels = data.rolls.map(r => r.date.slice(5));
    yieldChart = new Chart(ctx, {
        type: 'line',
        data: {
            labels,
            datasets: [
                {
                    label: 'Net Yield (USD)',
                    data: data.rolls.map(r => r.net_yield_usd),
                    borderColor: CHART_COLORS.yield,
                    backgroundColor: CHART_COLORS.yieldBg,
                    fill: true,
                    tension: 0.3,
                    pointRadius: 4,
                    pointHoverRadius: 6,
                    yAxisID: 'y',
                },
                {
                    label: 'Cumulative Yield / vGLD',
                    data: data.rolls.map(r => r.cumulative_yield_per_vgld),
                    borderColor: CHART_COLORS.cumulative,
                    backgroundColor: CHART_COLORS.cumulativeBg,
                    fill: false,
                    tension: 0.3,
                    pointRadius: 3,
                    borderDash: [5, 3],
                    yAxisID: 'y1',
                },
            ],
        },
        options: {
            ...CHART_DEFAULTS,
            scales: {
                x: CHART_DEFAULTS.scales.x,
                y: {
                    ...CHART_DEFAULTS.scales.y,
                    position: 'left',
                    title: { display: true, text: 'Net Yield (USD)', color: CHART_COLORS.text },
                },
                y1: {
                    ...CHART_DEFAULTS.scales.y,
                    position: 'right',
                    title: { display: true, text: 'Cumulative / vGLD', color: CHART_COLORS.text },
                    grid: { drawOnChartArea: false },
                },
            },
        },
    });

    const summary = document.getElementById('yield-summary');
    if (summary) {
        summary.innerHTML = `
            <span class="summary-item positive">APY: ${data.current_apy.toFixed(1)}%</span>
            <span class="summary-item">Total Net Yield: ${formatUsd(data.total_net_yield_usd)}</span>
            <span class="summary-item">Cumulative/vGLD: ${data.cumulative_yield_per_vgld.toFixed(6)}</span>
            ${data.last_roll_date ? `<span class="summary-item">Last Roll: ${formatDate(data.last_roll_date)}</span>` : ''}
            ${data.next_roll_date ? `<span class="summary-item">Next Roll: ${formatDate(data.next_roll_date)}</span>` : ''}
        `;
    }
}

// --- Phase 3: User charts ---

function renderGrowthChart(data) {
    const ctx = document.getElementById('growth-chart');
    if (!ctx) return;
    growthChart = destroyChart(growthChart);

    const labels = data.points.map(p => p.date.slice(5));
    growthChart = new Chart(ctx, {
        type: 'bar',
        data: {
            labels,
            datasets: [
                {
                    label: 'New Users',
                    data: data.points.map(p => p.new_users),
                    backgroundColor: CHART_COLORS.depositBg,
                    borderColor: CHART_COLORS.deposit,
                    borderWidth: 1,
                    borderRadius: 4,
                    yAxisID: 'y',
                    order: 2,
                },
                {
                    label: 'Cumulative',
                    data: data.points.map(p => p.cumulative_users),
                    borderColor: CHART_COLORS.cumulative,
                    backgroundColor: 'transparent',
                    type: 'line',
                    tension: 0.3,
                    pointRadius: 2,
                    borderWidth: 2,
                    yAxisID: 'y1',
                    order: 1,
                },
            ],
        },
        options: {
            ...CHART_DEFAULTS,
            scales: {
                x: CHART_DEFAULTS.scales.x,
                y: {
                    ...CHART_DEFAULTS.scales.y,
                    position: 'left',
                    title: { display: true, text: 'New / Day', color: CHART_COLORS.text },
                },
                y1: {
                    ...CHART_DEFAULTS.scales.y,
                    position: 'right',
                    title: { display: true, text: 'Cumulative', color: CHART_COLORS.text },
                    grid: { drawOnChartArea: false },
                },
            },
        },
    });
}

function renderDauChart(data) {
    const ctx = document.getElementById('dau-chart');
    if (!ctx) return;
    dauChart = destroyChart(dauChart);

    const labels = data.daily.map(p => p.date.slice(5));
    dauChart = new Chart(ctx, {
        type: 'line',
        data: {
            labels,
            datasets: [
                {
                    label: 'DAU',
                    data: data.daily.map(p => p.dau),
                    borderColor: CHART_COLORS.blue,
                    backgroundColor: 'rgba(59, 130, 246, 0.1)',
                    fill: true,
                    tension: 0.3,
                    pointRadius: 3,
                    pointHoverRadius: 5,
                },
            ],
        },
        options: {
            ...CHART_DEFAULTS,
            plugins: {
                ...CHART_DEFAULTS.plugins,
                annotation: {
                    annotations: {
                        wauLine: {
                            type: 'line',
                            yMin: data.wau,
                            yMax: data.wau,
                            borderColor: CHART_COLORS.cumulative,
                            borderDash: [6, 3],
                            borderWidth: 1,
                            label: { display: true, content: `WAU: ${data.wau}`, position: 'end' },
                        },
                    },
                },
            },
        },
    });
}

function renderHoldersTable(holders) {
    const tbody = document.getElementById('holders-tbody');
    if (!tbody) return;

    if (holders.length === 0) {
        tbody.innerHTML = '<tr><td colspan="8" class="empty-cell">No holder data yet</td></tr>';
        return;
    }

    tbody.innerHTML = holders.map(h => {
        const addr = h.address.length > 14
            ? h.address.slice(0, 6) + '...' + h.address.slice(-4)
            : h.address;
        const netClass = h.net_usd >= 0 ? 'positive' : 'negative';
        return `<tr>
            <td title="${h.address}">${addr}</td>
            <td>${formatUsd(h.total_deposited_usd)}</td>
            <td>${formatUsd(h.total_withdrawn_usd)}</td>
            <td class="${netClass}">${formatUsd(h.net_usd)}</td>
            <td>${h.deposit_count}</td>
            <td>${h.products.join(', ')}</td>
            <td>${formatDate(h.first_deposit)}</td>
            <td>${formatDate(h.last_deposit)}</td>
        </tr>`;
    }).join('');
}

// --- Phase 4: Ops charts ---

const FUNNEL_ORDER = [
    'initiated', 'pending', 'swap_pending', 'sent_to_exchange',
    'gold_acquired', 'completed', 'failed', 'refunded'
];

const STATUS_COLORS = {
    initiated: '#8b9dc3',
    pending: '#3b82f6',
    swap_pending: '#a855f7',
    sent_to_exchange: '#f0b90b',
    gold_acquired: '#22c55e',
    completed: '#16a34a',
    failed: '#ef4444',
    refunded: '#f97316',
};

function renderFunnelChart(data) {
    const ctx = document.getElementById('funnel-chart');
    if (!ctx) return;
    funnelChart = destroyChart(funnelChart);

    const sorted = data.stages.sort((a, b) => {
        const ai = FUNNEL_ORDER.indexOf(a.status);
        const bi = FUNNEL_ORDER.indexOf(b.status);
        return (ai === -1 ? 99 : ai) - (bi === -1 ? 99 : bi);
    });

    const labels = sorted.map(s => s.status.replace(/_/g, ' '));
    const counts = sorted.map(s => s.count);
    const colors = sorted.map(s => STATUS_COLORS[s.status] || '#8b9dc3');

    funnelChart = new Chart(ctx, {
        type: 'bar',
        data: {
            labels,
            datasets: [{
                label: 'Deposits',
                data: counts,
                backgroundColor: colors.map(c => c + '40'),
                borderColor: colors,
                borderWidth: 1,
                borderRadius: 4,
            }],
        },
        options: {
            ...CHART_DEFAULTS,
            indexAxis: 'y',
            plugins: {
                ...CHART_DEFAULTS.plugins,
                legend: { display: false },
                tooltip: {
                    ...CHART_DEFAULTS.plugins.tooltip,
                    callbacks: {
                        label: (ctx) => {
                            const pct = data.total > 0 ? (ctx.raw / data.total * 100).toFixed(1) : 0;
                            return `${ctx.raw} deposits (${pct}%)`;
                        },
                    },
                },
            },
        },
    });
}

function renderProcessingChart(data) {
    const ctx = document.getElementById('processing-chart');
    if (!ctx) return;
    processingChart = destroyChart(processingChart);

    if (data.points.length === 0) return;

    const labels = data.points.map(p => p.date.slice(5));
    processingChart = new Chart(ctx, {
        type: 'line',
        data: {
            labels,
            datasets: [
                {
                    label: 'p50',
                    data: data.points.map(p => p.p50_minutes),
                    borderColor: '#22c55e',
                    backgroundColor: 'transparent',
                    tension: 0.3,
                    pointRadius: 3,
                    borderWidth: 2,
                },
                {
                    label: 'p90',
                    data: data.points.map(p => p.p90_minutes),
                    borderColor: '#f0b90b',
                    backgroundColor: 'transparent',
                    tension: 0.3,
                    pointRadius: 3,
                    borderWidth: 2,
                },
                {
                    label: 'p99',
                    data: data.points.map(p => p.p99_minutes),
                    borderColor: '#ef4444',
                    backgroundColor: 'transparent',
                    tension: 0.3,
                    pointRadius: 3,
                    borderWidth: 2,
                    borderDash: [5, 3],
                },
            ],
        },
        options: {
            ...CHART_DEFAULTS,
            scales: {
                x: CHART_DEFAULTS.scales.x,
                y: {
                    ...CHART_DEFAULTS.scales.y,
                    title: { display: true, text: 'Minutes', color: CHART_COLORS.text },
                },
            },
        },
    });
}

function renderHedgeChart(data) {
    const ctx = document.getElementById('hedge-chart');
    if (!ctx) return;
    hedgeChart = destroyChart(hedgeChart);

    hedgeChart = new Chart(ctx, {
        type: 'doughnut',
        data: {
            labels: ['Hedged (Short)', 'Unhedged'],
            datasets: [{
                data: [
                    data.futures_short_oz,
                    Math.max(0, data.total_paxg_oz - data.futures_short_oz),
                ],
                backgroundColor: ['rgba(34, 197, 94, 0.3)', 'rgba(239, 68, 68, 0.3)'],
                borderColor: ['#22c55e', '#ef4444'],
                borderWidth: 2,
            }],
        },
        options: {
            responsive: true,
            maintainAspectRatio: false,
            cutout: '60%',
            plugins: {
                legend: {
                    labels: { color: CHART_COLORS.text, boxWidth: 12, padding: 16 },
                },
                tooltip: {
                    ...CHART_DEFAULTS.plugins.tooltip,
                    callbacks: {
                        label: (ctx) => `${ctx.label}: ${ctx.raw.toFixed(6)} oz`,
                    },
                },
            },
        },
    });
}
