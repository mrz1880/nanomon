// State
let currentRoute = { page: 'host', param: null };
let currentSort = 'cpu';
let currentContainerSort = 'cpu';
let refreshInterval = 10000;
let intervalId = null;
// Cache containers data for detail views
let cachedContainersData = null;

// Initialize
document.addEventListener('DOMContentLoaded', () => {
    initTabs();
    initControls();
    window.addEventListener('hashchange', onHashChange);
    onHashChange(); // Route from initial URL
    startRefresh();
});

// ---- Routing ----

function parseHash() {
    const hash = location.hash.slice(1) || 'host';
    const parts = hash.split('/');
    return { page: parts[0], param: parts.slice(1).join('/') || null };
}

function navigate(page, param) {
    if (param) {
        location.hash = `#${page}/${param}`;
    } else {
        location.hash = `#${page}`;
    }
}

function onHashChange() {
    const route = parseHash();
    currentRoute = route;

    // Hide all tab content
    document.querySelectorAll('.tab-content').forEach(el => el.classList.remove('active'));

    // Update tab buttons
    const mainPage = route.page === 'stack' || route.page === 'container' ? 'containers' : route.page;
    document.querySelectorAll('.tab-btn').forEach(btn => {
        btn.classList.toggle('active', btn.dataset.tab === mainPage);
    });

    // Show correct content
    if (route.page === 'container' || route.page === 'stack') {
        document.getElementById('detail-tab').classList.add('active');
    } else {
        const tabEl = document.getElementById(`${route.page}-tab`);
        if (tabEl) tabEl.classList.add('active');
    }

    loadData();
}

// ---- Tab navigation ----

function initTabs() {
    document.querySelectorAll('.tab-btn').forEach(tab => {
        tab.addEventListener('click', () => navigate(tab.dataset.tab));
    });
}

// ---- Controls ----

function initControls() {
    document.querySelectorAll('.sort-btn').forEach(btn => {
        btn.addEventListener('click', () => {
            currentSort = btn.dataset.sort;
            document.querySelectorAll('.sort-btn').forEach(b => b.classList.remove('active'));
            btn.classList.add('active');
            loadProcesses();
        });
    });

    document.querySelectorAll('.container-sort-btn').forEach(btn => {
        btn.addEventListener('click', () => {
            currentContainerSort = btn.dataset.sort;
            document.querySelectorAll('.container-sort-btn').forEach(b => b.classList.remove('active'));
            btn.classList.add('active');
            loadContainers();
        });
    });
}

// ---- Data loading ----

function startRefresh() {
    if (intervalId) clearInterval(intervalId);
    intervalId = setInterval(loadData, refreshInterval);
}

async function loadData() {
    try {
        switch (currentRoute.page) {
            case 'host': await loadHost(); break;
            case 'charts': await loadCharts(); break;
            case 'containers': await loadContainers(); break;
            case 'processes': await loadProcesses(); break;
            case 'disks': await loadDisks(); break;
            case 'services': await loadServices(); break;
            case 'container': await loadContainerDetail(currentRoute.param); break;
            case 'stack': await loadStackDetail(currentRoute.param); break;
        }
    } catch (error) {
        console.error('Error loading data:', error);
    }
}

// ---- Host ----

async function loadHost() {
    const response = await fetch('/api/host');
    const data = await response.json();

    document.getElementById('hostname').textContent = data.hostname;
    document.getElementById('uptime').textContent = formatUptime(data.uptime_seconds);

    document.getElementById('load-1').textContent = data.load_average.one.toFixed(2);
    document.getElementById('load-5').textContent = data.load_average.five.toFixed(2);
    document.getElementById('load-15').textContent = data.load_average.fifteen.toFixed(2);

    const cpuPercent = data.cpu.usage_percent.toFixed(1);
    document.getElementById('cpu-percent').textContent = cpuPercent + '%';
    document.getElementById('cpu-bar').style.width = cpuPercent + '%';
    document.getElementById('cpu-details').textContent =
        `user ${data.cpu.user_percent.toFixed(1)}% \u00b7 sys ${data.cpu.system_percent.toFixed(1)}% \u00b7 iowait ${(data.cpu.iowait_percent || 0).toFixed(1)}%`;

    const ramPercent = (data.memory.used_bytes / data.memory.total_bytes * 100).toFixed(1);
    document.getElementById('ram-percent').textContent = ramPercent + '%';
    document.getElementById('ram-bar').style.width = ramPercent + '%';
    document.getElementById('ram-details').textContent =
        `${formatBytes(data.memory.used_bytes)} / ${formatBytes(data.memory.total_bytes)}`;

    document.getElementById('net-rx').textContent = '0 MB/s';
    document.getElementById('net-tx').textContent = '0 MB/s';

    renderTemperatures(data.temperatures || []);
}

function renderTemperatures(temperatures) {
    const section = document.getElementById('temperatures-section');
    const grid = document.getElementById('temperatures-grid');

    if (temperatures.length === 0) {
        section.style.display = 'none';
        return;
    }

    section.style.display = 'block';
    grid.innerHTML = temperatures.map(t => {
        const tempClass = t.critical_celsius && t.current_celsius >= t.critical_celsius ? 'text-danger'
            : t.high_celsius && t.current_celsius >= t.high_celsius ? 'text-warning'
            : 'text-success';

        const thresholdInfo = [];
        if (t.high_celsius) thresholdInfo.push(`high: ${t.high_celsius.toFixed(0)}\u00b0C`);
        if (t.critical_celsius) thresholdInfo.push(`crit: ${t.critical_celsius.toFixed(0)}\u00b0C`);

        return `
            <div class="temp-card">
                <div class="temp-label">${t.label}</div>
                <div class="temp-value ${tempClass}">${t.current_celsius.toFixed(1)}\u00b0C</div>
                ${thresholdInfo.length > 0 ? `<div class="temp-thresholds">${thresholdInfo.join(' / ')}</div>` : ''}
            </div>
        `;
    }).join('');
}

// ---- Charts ----

async function loadCharts() {
    const response = await fetch('/api/history?duration=3600');
    const data = await response.json();

    if (data.timestamps.length === 0) {
        document.querySelectorAll('.chart-card canvas').forEach(canvas => {
            const ctx = canvas.getContext('2d');
            ctx.clearRect(0, 0, canvas.width, canvas.height);
            ctx.fillStyle = '#8b949e';
            ctx.font = '14px sans-serif';
            ctx.textAlign = 'center';
            ctx.fillText('Collecting data...', canvas.width / 2, canvas.height / 2);
        });
        return;
    }

    const labels = data.timestamps.map(t => {
        const d = new Date(t);
        return `${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}`;
    });

    drawChart('chart-cpu', labels, [
        { values: data.cpu, color: '#58a6ff', label: 'CPU %' }
    ], { min: 0, max: 100, suffix: '%' });

    const memPercent = data.memory_used.map(used =>
        data.memory_total > 0 ? (used / data.memory_total * 100) : 0
    );
    drawChart('chart-memory', labels, [
        { values: memPercent, color: '#3fb950', label: 'RAM %' }
    ], { min: 0, max: 100, suffix: '%' });

    const maxLoad = Math.max(...data.load_1, ...data.load_5, ...data.load_15, 1);
    drawChart('chart-load', labels, [
        { values: data.load_1, color: '#f85149', label: '1m' },
        { values: data.load_5, color: '#d29922', label: '5m' },
        { values: data.load_15, color: '#58a6ff', label: '15m' },
    ], { min: 0, max: Math.ceil(maxLoad * 1.2) });
}

function drawChart(canvasId, labels, datasets, options) {
    const canvas = document.getElementById(canvasId);
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;

    const rect = canvas.getBoundingClientRect();
    canvas.width = rect.width * dpr;
    canvas.height = rect.height * dpr;
    ctx.scale(dpr, dpr);

    const width = rect.width;
    const height = rect.height;
    const padding = { top: 20, right: 60, bottom: 30, left: 50 };
    const chartWidth = width - padding.left - padding.right;
    const chartHeight = height - padding.top - padding.bottom;

    ctx.clearRect(0, 0, width, height);

    const min = options.min ?? 0;
    const max = options.max ?? 100;
    const range = max - min || 1;

    ctx.strokeStyle = '#21262d';
    ctx.lineWidth = 1;
    const gridLines = 4;
    for (let i = 0; i <= gridLines; i++) {
        const y = padding.top + (chartHeight / gridLines) * i;
        ctx.beginPath();
        ctx.moveTo(padding.left, y);
        ctx.lineTo(padding.left + chartWidth, y);
        ctx.stroke();

        const value = max - (range / gridLines) * i;
        ctx.fillStyle = '#8b949e';
        ctx.font = '11px sans-serif';
        ctx.textAlign = 'right';
        ctx.fillText(value.toFixed(0) + (options.suffix || ''), padding.left - 5, y + 4);
    }

    const labelStep = Math.max(1, Math.floor(labels.length / 6));
    ctx.textAlign = 'center';
    for (let i = 0; i < labels.length; i += labelStep) {
        const x = padding.left + (i / (labels.length - 1 || 1)) * chartWidth;
        ctx.fillStyle = '#8b949e';
        ctx.fillText(labels[i], x, height - 8);
    }

    datasets.forEach(dataset => {
        ctx.strokeStyle = dataset.color;
        ctx.lineWidth = 2;
        ctx.beginPath();

        dataset.values.forEach((value, i) => {
            const x = padding.left + (i / (dataset.values.length - 1 || 1)) * chartWidth;
            const y = padding.top + chartHeight - ((value - min) / range) * chartHeight;
            if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
        });

        ctx.stroke();

        if (datasets.length === 1) {
            const lastX = padding.left + chartWidth;
            ctx.lineTo(lastX, padding.top + chartHeight);
            ctx.lineTo(padding.left, padding.top + chartHeight);
            ctx.closePath();
            ctx.fillStyle = dataset.color + '20';
            ctx.fill();
        }
    });

    if (datasets.length > 1) {
        let legendX = width - padding.right + 5;
        datasets.forEach((dataset, i) => {
            const legendY = padding.top + i * 18;
            ctx.fillStyle = dataset.color;
            ctx.fillRect(legendX, legendY, 10, 10);
            ctx.fillStyle = '#c9d1d9';
            ctx.font = '11px sans-serif';
            ctx.textAlign = 'left';
            ctx.fillText(dataset.label, legendX + 14, legendY + 9);
        });
    }
}

// ---- Containers list ----

async function loadContainers() {
    const response = await fetch('/api/containers');
    const data = await response.json();
    cachedContainersData = data;

    const containersList = document.getElementById('containers-list');

    if (data.stacks.length === 0 && data.containers.length === 0) {
        containersList.innerHTML = '<p class="loading">No containers found</p>';
        return;
    }

    let html = '';
    const sortedStacks = [...data.stacks];
    sortStacks(sortedStacks, currentContainerSort);

    sortedStacks.forEach((stack, stackIdx) => {
        const stackContainers = data.containers.filter(c => c.stack === stack.name);
        sortContainers(stackContainers, currentContainerSort);
        const isTopStack = stackIdx === 0 && (currentContainerSort === 'cpu' || currentContainerSort === 'memory');

        html += `
            <div class="stack ${isTopStack ? 'top-consumer' : ''}">
                <a class="stack-header" href="#stack/${encodeURIComponent(stack.name)}">
                    <span class="stack-name">\u25bc ${stack.name}</span>
                    <div class="stack-stats">
                        <span>${stack.containers_running}/${stack.containers_total} running</span>
                        <span>CPU ${stack.cpu_percent.toFixed(1)}%</span>
                        <span>RAM ${formatBytes(stack.memory_bytes)}</span>
                    </div>
                </a>
                ${stackContainers.map((c, idx) => renderContainerRow(c, idx === 0 && c.state === 'running' && (currentContainerSort === 'cpu' || currentContainerSort === 'memory'))).join('')}
            </div>
        `;
    });

    const standalone = data.containers.filter(c => !c.stack);
    if (standalone.length > 0) {
        sortContainers(standalone, currentContainerSort);
        html += standalone.map((c, idx) => `
            <div class="stack">
                ${renderContainerRow(c, idx === 0 && c.state === 'running' && (currentContainerSort === 'cpu' || currentContainerSort === 'memory'))}
            </div>
        `).join('');
    }

    containersList.innerHTML = html;
}

function renderContainerRow(container, isTopConsumer) {
    isTopConsumer = isTopConsumer || false;
    const statusClass = container.state === 'running' ? 'running' : 'stopped';
    return `
        <a class="container-item ${isTopConsumer ? 'top-consumer' : ''}" href="#container/${encodeURIComponent(container.name)}">
            <div class="container-name">
                <span class="status-dot ${statusClass}"></span>
                <span>${container.name}</span>
            </div>
            <div class="container-metrics">
                ${container.state === 'running' ? `
                    <span>CPU ${container.cpu.usage_percent.toFixed(1)}%</span>
                    <span>RAM ${formatBytes(container.memory.used_bytes)}</span>
                    <span>\u25bc ${formatBytes(container.network.rx_bytes)}</span>
                    <span>\u25b2 ${formatBytes(container.network.tx_bytes)}</span>
                ` : `<span class="text-secondary">stopped</span>`}
            </div>
        </a>
    `;
}

function sortStacks(stacks, sortBy) {
    stacks.sort((a, b) => {
        switch(sortBy) {
            case 'cpu': return b.cpu_percent - a.cpu_percent;
            case 'memory': return b.memory_bytes - a.memory_bytes;
            case 'name': return a.name.localeCompare(b.name);
            default: return 0;
        }
    });
}

function sortContainers(containers, sortBy) {
    containers.sort((a, b) => {
        switch(sortBy) {
            case 'cpu':
                const cpuA = a.state === 'running' ? a.cpu.usage_percent : -1;
                const cpuB = b.state === 'running' ? b.cpu.usage_percent : -1;
                return cpuB - cpuA;
            case 'memory':
                const memA = a.state === 'running' ? a.memory.used_bytes : -1;
                const memB = b.state === 'running' ? b.memory.used_bytes : -1;
                return memB - memA;
            case 'name': return a.name.localeCompare(b.name);
            default: return 0;
        }
    });
}

// ---- Container detail ----

async function loadContainerDetail(name) {
    const response = await fetch(`/api/containers/${encodeURIComponent(name)}`);
    if (!response.ok) {
        renderDetailView(`
            <nav class="breadcrumb"><a href="#containers">Containers</a> / <span>${name}</span></nav>
            <p class="loading">Container "${name}" not found</p>
        `);
        return;
    }
    const c = await response.json();

    const statusClass = c.state === 'running' ? 'running' : 'stopped';
    const createdDate = new Date(c.created_at);
    const createdStr = createdDate.toLocaleDateString() + ' ' + createdDate.toLocaleTimeString();

    let html = `
        <nav class="breadcrumb">
            <a href="#containers">Containers</a>
            ${c.stack ? ` / <a href="#stack/${encodeURIComponent(c.stack)}">${c.stack}</a>` : ''}
            / <span>${c.name}</span>
        </nav>

        <div class="detail-header">
            <div class="detail-title">
                <span class="status-dot ${statusClass}"></span>
                <h2>${c.name}</h2>
                <span class="detail-state ${statusClass}">${c.state}</span>
            </div>
        </div>

        <div class="detail-info">
            <div class="info-row"><span class="info-label">Image</span><span class="info-value">${c.image}</span></div>
            <div class="info-row"><span class="info-label">ID</span><span class="info-value mono">${c.id}</span></div>
            ${c.stack ? `<div class="info-row"><span class="info-label">Stack</span><span class="info-value"><a href="#stack/${encodeURIComponent(c.stack)}">${c.stack}</a></span></div>` : ''}
            <div class="info-row"><span class="info-label">Created</span><span class="info-value">${createdStr}</span></div>
        </div>
    `;

    if (c.state === 'running') {
        html += `
            <div class="metrics-grid" style="margin-top: 20px;">
                <div class="metric-card">
                    <h3>CPU</h3>
                    <div class="progress-bar">
                        <div class="progress-fill" style="width: ${Math.min(c.cpu.usage_percent, 100).toFixed(1)}%"></div>
                    </div>
                    <div class="metric-details">
                        <span>${c.cpu.usage_percent.toFixed(2)}%</span>
                        <span class="text-secondary">user ${c.cpu.user_percent.toFixed(1)}% \u00b7 sys ${c.cpu.system_percent.toFixed(1)}%</span>
                    </div>
                </div>

                <div class="metric-card">
                    <h3>MEMORY</h3>
                    ${c.memory.total_bytes > 0 ? `
                        <div class="progress-bar">
                            <div class="progress-fill" style="width: ${(c.memory.used_bytes / c.memory.total_bytes * 100).toFixed(1)}%"></div>
                        </div>
                        <div class="metric-details">
                            <span>${formatBytes(c.memory.used_bytes)}</span>
                            <span class="text-secondary">/ ${formatBytes(c.memory.total_bytes)}</span>
                        </div>
                    ` : `
                        <div class="metric-details"><span>${formatBytes(c.memory.used_bytes)}</span></div>
                    `}
                </div>

                <div class="metric-card">
                    <h3>NETWORK</h3>
                    <div class="net-stats">
                        <div>\u25bc <span>${formatBytes(c.network.rx_bytes)}</span></div>
                        <div>\u25b2 <span>${formatBytes(c.network.tx_bytes)}</span></div>
                    </div>
                    ${(c.network.rx_errors > 0 || c.network.tx_errors > 0) ? `
                        <div class="text-secondary" style="text-align:center; margin-top:8px; font-size:12px;">
                            errors: rx ${c.network.rx_errors} / tx ${c.network.tx_errors}
                        </div>
                    ` : ''}
                </div>

                <div class="metric-card">
                    <h3>BLOCK I/O</h3>
                    <div class="net-stats">
                        <div>R <span>${formatBytes(c.block_io.read_bytes)}</span></div>
                        <div>W <span>${formatBytes(c.block_io.write_bytes)}</span></div>
                    </div>
                </div>
            </div>
        `;
    }

    renderDetailView(html);
}

// ---- Stack detail ----

async function loadStackDetail(stackName) {
    const response = await fetch('/api/containers');
    const data = await response.json();

    const stack = data.stacks.find(s => s.name === stackName);
    const containers = data.containers.filter(c => c.stack === stackName);

    if (!stack || containers.length === 0) {
        renderDetailView(`
            <nav class="breadcrumb"><a href="#containers">Containers</a> / <span>${stackName}</span></nav>
            <p class="loading">Stack "${stackName}" not found</p>
        `);
        return;
    }

    sortContainers(containers, 'cpu');

    let html = `
        <nav class="breadcrumb">
            <a href="#containers">Containers</a> / <span>${stackName}</span>
        </nav>

        <div class="detail-header">
            <h2>${stackName}</h2>
            <div class="stack-stats">
                <span>${stack.containers_running}/${stack.containers_total} running</span>
                <span>CPU ${stack.cpu_percent.toFixed(1)}%</span>
                <span>RAM ${formatBytes(stack.memory_bytes)}</span>
            </div>
        </div>

        <div class="stack-containers-list">
    `;

    containers.forEach(c => {
        const statusClass = c.state === 'running' ? 'running' : 'stopped';
        const createdDate = new Date(c.created_at);

        html += `
            <div class="stack-container-card">
                <a class="stack-container-header" href="#container/${encodeURIComponent(c.name)}">
                    <div class="container-name">
                        <span class="status-dot ${statusClass}"></span>
                        <span class="container-title">${c.name}</span>
                    </div>
                    <span class="text-secondary">${c.image}</span>
                </a>
                ${c.state === 'running' ? `
                    <div class="stack-container-metrics">
                        <div class="mini-metric">
                            <span class="mini-label">CPU</span>
                            <span class="mini-value">${c.cpu.usage_percent.toFixed(2)}%</span>
                        </div>
                        <div class="mini-metric">
                            <span class="mini-label">RAM</span>
                            <span class="mini-value">${formatBytes(c.memory.used_bytes)}</span>
                        </div>
                        <div class="mini-metric">
                            <span class="mini-label">NET \u25bc</span>
                            <span class="mini-value">${formatBytes(c.network.rx_bytes)}</span>
                        </div>
                        <div class="mini-metric">
                            <span class="mini-label">NET \u25b2</span>
                            <span class="mini-value">${formatBytes(c.network.tx_bytes)}</span>
                        </div>
                        <div class="mini-metric">
                            <span class="mini-label">I/O R</span>
                            <span class="mini-value">${formatBytes(c.block_io.read_bytes)}</span>
                        </div>
                        <div class="mini-metric">
                            <span class="mini-label">I/O W</span>
                            <span class="mini-value">${formatBytes(c.block_io.write_bytes)}</span>
                        </div>
                    </div>
                ` : `<div class="stack-container-metrics"><span class="text-secondary">stopped \u00b7 created ${createdDate.toLocaleDateString()}</span></div>`}
            </div>
        `;
    });

    html += '</div>';
    renderDetailView(html);
}

function renderDetailView(html) {
    document.getElementById('detail-content').innerHTML = html;
}

// ---- Processes ----

async function loadProcesses() {
    const response = await fetch(`/api/processes?sort=${currentSort}&limit=20`);
    const data = await response.json();
    const tbody = document.getElementById('processes-list');

    if (data.processes.length === 0) {
        tbody.innerHTML = '<tr><td colspan="5" class="loading">No processes found</td></tr>';
        return;
    }

    tbody.innerHTML = data.processes.map(p => `
        <tr>
            <td>${p.pid}</td>
            <td>${p.user}</td>
            <td>${p.cpu_percent.toFixed(1)}%</td>
            <td>${p.memory_percent.toFixed(1)}%</td>
            <td>
                ${p.command}
                ${p.container_id ? '<span class="text-secondary"> [C]</span>' : ''}
            </td>
        </tr>
    `).join('');
}

// ---- Disks ----

async function loadDisks() {
    const response = await fetch('/api/disks');
    const data = await response.json();
    const tbody = document.getElementById('disks-list');

    if (data.disks.length === 0) {
        tbody.innerHTML = '<tr><td colspan="5" class="loading">No disks found</td></tr>';
        return;
    }

    tbody.innerHTML = data.disks.map(d => {
        const usagePercent = (d.used_bytes / d.total_bytes * 100).toFixed(1);
        return `
            <tr>
                <td>${d.mount_point}</td>
                <td>${formatBytes(d.total_bytes)}</td>
                <td>${formatBytes(d.used_bytes)}</td>
                <td>${formatBytes(d.available_bytes)}</td>
                <td><span class="${getUsageClass(usagePercent)}">${usagePercent}%</span></td>
            </tr>
        `;
    }).join('');
}

// ---- Services ----

async function loadServices() {
    const response = await fetch('/api/services');
    const data = await response.json();
    const content = document.getElementById('services-content');

    if (!data.available) {
        content.innerHTML = '<p class="loading">Systemd not available. Set NANOMON_ENABLE_SYSTEMD=true to enable.</p>';
        return;
    }

    if (data.services.length === 0) {
        content.innerHTML = '<p class="loading">No services found</p>';
        return;
    }

    const failed = data.services.filter(s => s.state === 'failed');
    const active = data.services.filter(s => s.state === 'active');
    const inactive = data.services.filter(s => s.state === 'inactive');
    const other = data.services.filter(s => !['active', 'inactive', 'failed'].includes(s.state));

    let html = '';
    if (failed.length > 0) {
        html += `<h3 class="services-section-title text-danger">Failed (${failed.length})</h3>`;
        html += renderServiceTable(failed);
    }
    if (active.length > 0) {
        html += `<h3 class="services-section-title text-success">Active (${active.length})</h3>`;
        html += renderServiceTable(active);
    }
    if (other.length > 0) {
        html += `<h3 class="services-section-title text-warning">Other (${other.length})</h3>`;
        html += renderServiceTable(other);
    }
    if (inactive.length > 0) {
        html += `<h3 class="services-section-title text-secondary">Inactive (${inactive.length})</h3>`;
        html += renderServiceTable(inactive);
    }
    content.innerHTML = html;
}

function renderServiceTable(services) {
    return `
        <table class="services-table">
            <thead><tr><th>SERVICE</th><th>STATE</th><th>SUB</th><th>DESCRIPTION</th></tr></thead>
            <tbody>
                ${services.map(s => {
                    const stateClass = s.state === 'active' ? 'text-success' : s.state === 'failed' ? 'text-danger' : 'text-secondary';
                    return `<tr>
                        <td>${s.name}</td>
                        <td><span class="${stateClass}">${s.state}</span></td>
                        <td>${s.sub_state}</td>
                        <td class="text-secondary">${s.description}</td>
                    </tr>`;
                }).join('')}
            </tbody>
        </table>
    `;
}

// ---- Utilities ----

function formatBytes(bytes) {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
}

function formatUptime(seconds) {
    const days = Math.floor(seconds / 86400);
    const hours = Math.floor((seconds % 86400) / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    if (days > 0) {
        return `up ${days} days, ${hours}:${minutes.toString().padStart(2, '0')}`;
    }
    return `up ${hours}:${minutes.toString().padStart(2, '0')}`;
}

function getUsageClass(percent) {
    if (percent < 70) return 'text-success';
    if (percent < 90) return 'text-warning';
    return 'text-danger';
}
