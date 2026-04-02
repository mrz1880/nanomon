// State
let currentTab = 'host';
let currentSort = 'cpu';
let currentContainerSort = 'cpu';
let refreshInterval = 10000; // 10 seconds
let intervalId = null;

// Initialize
document.addEventListener('DOMContentLoaded', () => {
    initTabs();
    initControls();
    startRefresh();
});

// Tab navigation
function initTabs() {
    const tabs = document.querySelectorAll('.tab-btn');
    tabs.forEach(tab => {
        tab.addEventListener('click', () => {
            const tabName = tab.dataset.tab;
            switchTab(tabName);
        });
    });
}

function switchTab(tabName) {
    currentTab = tabName;

    // Update buttons
    document.querySelectorAll('.tab-btn').forEach(btn => {
        btn.classList.toggle('active', btn.dataset.tab === tabName);
    });

    // Update content
    document.querySelectorAll('.tab-content').forEach(content => {
        content.classList.toggle('active', content.id === `${tabName}-tab`);
    });

    // Load data for new tab
    loadData();
}

// Controls
function initControls() {
    // Process sort buttons
    const sortBtns = document.querySelectorAll('.sort-btn');
    sortBtns.forEach(btn => {
        btn.addEventListener('click', () => {
            currentSort = btn.dataset.sort;
            sortBtns.forEach(b => b.classList.remove('active'));
            btn.classList.add('active');
            loadProcesses();
        });
    });

    // Container sort buttons
    const containerSortBtns = document.querySelectorAll('.container-sort-btn');
    containerSortBtns.forEach(btn => {
        btn.addEventListener('click', () => {
            currentContainerSort = btn.dataset.sort;
            containerSortBtns.forEach(b => b.classList.remove('active'));
            btn.classList.add('active');
            loadContainers();
        });
    });
}

// Data loading
function startRefresh() {
    loadData();
    if (intervalId) clearInterval(intervalId);
    intervalId = setInterval(loadData, refreshInterval);
}

async function loadData() {
    try {
        switch (currentTab) {
            case 'host':
                await loadHost();
                break;
            case 'charts':
                await loadCharts();
                break;
            case 'containers':
                await loadContainers();
                break;
            case 'processes':
                await loadProcesses();
                break;
            case 'disks':
                await loadDisks();
                break;
            case 'services':
                await loadServices();
                break;
        }
    } catch (error) {
        console.error('Error loading data:', error);
    }
}

// Load host metrics
async function loadHost() {
    const response = await fetch('/api/host');
    const data = await response.json();

    // Update hostname and uptime
    document.getElementById('hostname').textContent = data.hostname;
    document.getElementById('uptime').textContent = formatUptime(data.uptime_seconds);

    // Update load average
    document.getElementById('load-1').textContent = data.load_average.one.toFixed(2);
    document.getElementById('load-5').textContent = data.load_average.five.toFixed(2);
    document.getElementById('load-15').textContent = data.load_average.fifteen.toFixed(2);

    // Update CPU
    const cpuPercent = data.cpu.usage_percent.toFixed(1);
    document.getElementById('cpu-percent').textContent = cpuPercent + '%';
    document.getElementById('cpu-bar').style.width = cpuPercent + '%';
    document.getElementById('cpu-details').textContent =
        `user ${data.cpu.user_percent.toFixed(1)}% \u00b7 sys ${data.cpu.system_percent.toFixed(1)}% \u00b7 iowait ${(data.cpu.iowait_percent || 0).toFixed(1)}%`;

    // Update RAM
    const ramPercent = (data.memory.used_bytes / data.memory.total_bytes * 100).toFixed(1);
    document.getElementById('ram-percent').textContent = ramPercent + '%';
    document.getElementById('ram-bar').style.width = ramPercent + '%';
    document.getElementById('ram-details').textContent =
        `${formatBytes(data.memory.used_bytes)} / ${formatBytes(data.memory.total_bytes)}`;

    // Update network (placeholder - needs calculation from previous values)
    document.getElementById('net-rx').textContent = '0 MB/s';
    document.getElementById('net-tx').textContent = '0 MB/s';

    // Update temperatures
    renderTemperatures(data.temperatures || []);
}

// Render temperature sensors
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

// Load and render charts
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

    // CPU chart
    drawChart('chart-cpu', labels, [
        { values: data.cpu, color: '#58a6ff', label: 'CPU %' }
    ], { min: 0, max: 100, suffix: '%' });

    // Memory chart
    const memPercent = data.memory_used.map((used, i) =>
        data.memory_total > 0 ? (used / data.memory_total * 100) : 0
    );
    drawChart('chart-memory', labels, [
        { values: memPercent, color: '#3fb950', label: 'RAM %' }
    ], { min: 0, max: 100, suffix: '%' });

    // Load chart
    const maxLoad = Math.max(
        ...data.load_1, ...data.load_5, ...data.load_15, 1
    );
    drawChart('chart-load', labels, [
        { values: data.load_1, color: '#f85149', label: '1m' },
        { values: data.load_5, color: '#d29922', label: '5m' },
        { values: data.load_15, color: '#58a6ff', label: '15m' },
    ], { min: 0, max: Math.ceil(maxLoad * 1.2) });
}

// Generic Canvas chart renderer
function drawChart(canvasId, labels, datasets, options) {
    const canvas = document.getElementById(canvasId);
    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;

    // Handle high-DPI displays
    const rect = canvas.getBoundingClientRect();
    canvas.width = rect.width * dpr;
    canvas.height = rect.height * dpr;
    ctx.scale(dpr, dpr);

    const width = rect.width;
    const height = rect.height;
    const padding = { top: 20, right: 60, bottom: 30, left: 50 };
    const chartWidth = width - padding.left - padding.right;
    const chartHeight = height - padding.top - padding.bottom;

    // Clear
    ctx.clearRect(0, 0, width, height);

    const min = options.min ?? 0;
    const max = options.max ?? 100;
    const range = max - min || 1;

    // Grid lines
    ctx.strokeStyle = '#21262d';
    ctx.lineWidth = 1;
    const gridLines = 4;
    for (let i = 0; i <= gridLines; i++) {
        const y = padding.top + (chartHeight / gridLines) * i;
        ctx.beginPath();
        ctx.moveTo(padding.left, y);
        ctx.lineTo(padding.left + chartWidth, y);
        ctx.stroke();

        // Y-axis labels
        const value = max - (range / gridLines) * i;
        ctx.fillStyle = '#8b949e';
        ctx.font = '11px sans-serif';
        ctx.textAlign = 'right';
        ctx.fillText(value.toFixed(0) + (options.suffix || ''), padding.left - 5, y + 4);
    }

    // X-axis labels (show ~6 labels)
    const labelStep = Math.max(1, Math.floor(labels.length / 6));
    ctx.textAlign = 'center';
    for (let i = 0; i < labels.length; i += labelStep) {
        const x = padding.left + (i / (labels.length - 1 || 1)) * chartWidth;
        ctx.fillStyle = '#8b949e';
        ctx.fillText(labels[i], x, height - 8);
    }

    // Draw datasets
    datasets.forEach(dataset => {
        ctx.strokeStyle = dataset.color;
        ctx.lineWidth = 2;
        ctx.beginPath();

        dataset.values.forEach((value, i) => {
            const x = padding.left + (i / (dataset.values.length - 1 || 1)) * chartWidth;
            const y = padding.top + chartHeight - ((value - min) / range) * chartHeight;

            if (i === 0) {
                ctx.moveTo(x, y);
            } else {
                ctx.lineTo(x, y);
            }
        });

        ctx.stroke();

        // Fill area under the line (only for single dataset)
        if (datasets.length === 1) {
            const lastX = padding.left + chartWidth;
            ctx.lineTo(lastX, padding.top + chartHeight);
            ctx.lineTo(padding.left, padding.top + chartHeight);
            ctx.closePath();
            ctx.fillStyle = dataset.color + '20';
            ctx.fill();
        }
    });

    // Legend (for multi-dataset charts)
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

// Load containers
async function loadContainers() {
    const response = await fetch('/api/containers');
    const data = await response.json();

    const containersList = document.getElementById('containers-list');

    if (data.stacks.length === 0 && data.containers.length === 0) {
        containersList.innerHTML = '<p class="loading">No containers found</p>';
        return;
    }

    let html = '';

    // Sort stacks based on current sort mode
    const sortedStacks = [...data.stacks];
    sortStacks(sortedStacks, currentContainerSort);

    // Group by stacks
    sortedStacks.forEach((stack, stackIdx) => {
        const stackContainers = data.containers.filter(c => c.stack === stack.name);

        // Sort containers within stack
        sortContainers(stackContainers, currentContainerSort);

        // Highlight top stack only when sorting by CPU or Memory
        const isTopStack = stackIdx === 0 && (currentContainerSort === 'cpu' || currentContainerSort === 'memory');

        html += `
            <div class="stack ${isTopStack ? 'top-consumer' : ''}">
                <div class="stack-header">
                    <span class="stack-name">\u25bc ${stack.name}</span>
                    <div class="stack-stats">
                        <span>${stack.containers_running}/${stack.containers_total} running</span>
                        <span>CPU ${stack.cpu_percent.toFixed(1)}%</span>
                        <span>RAM ${formatBytes(stack.memory_bytes)}</span>
                    </div>
                </div>
                ${stackContainers.map((c, idx) => renderContainer(c, idx === 0 && c.state === 'running' && (currentContainerSort === 'cpu' || currentContainerSort === 'memory'))).join('')}
            </div>
        `;
    });

    // Standalone containers
    const standalone = data.containers.filter(c => !c.stack);
    if (standalone.length > 0) {
        sortContainers(standalone, currentContainerSort);

        html += standalone.map((c, idx) => `
            <div class="stack">
                ${renderContainer(c, idx === 0 && c.state === 'running' && (currentContainerSort === 'cpu' || currentContainerSort === 'memory'))}
            </div>
        `).join('');
    }

    containersList.innerHTML = html;
}

function sortStacks(stacks, sortBy) {
    stacks.sort((a, b) => {
        switch(sortBy) {
            case 'cpu':
                return b.cpu_percent - a.cpu_percent;
            case 'memory':
                return b.memory_bytes - a.memory_bytes;
            case 'name':
                return a.name.localeCompare(b.name);
            default:
                return 0;
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
            case 'name':
                return a.name.localeCompare(b.name);
            default:
                return 0;
        }
    });
}

function renderContainer(container, isTopConsumer) {
    isTopConsumer = isTopConsumer || false;
    const statusClass = container.state === 'running' ? 'running' : 'stopped';
    return `
        <div class="container-item ${isTopConsumer ? 'top-consumer' : ''}">
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
        </div>
    `;
}

// Load processes
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

// Load disks
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
                <td>
                    <span class="${getUsageClass(usagePercent)}">${usagePercent}%</span>
                </td>
            </tr>
        `;
    }).join('');
}

// Load services
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

    // Group by state
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
            <thead>
                <tr>
                    <th>SERVICE</th>
                    <th>STATE</th>
                    <th>SUB</th>
                    <th>DESCRIPTION</th>
                </tr>
            </thead>
            <tbody>
                ${services.map(s => {
                    const stateClass = s.state === 'active' ? 'text-success'
                        : s.state === 'failed' ? 'text-danger'
                        : 'text-secondary';
                    return `
                        <tr>
                            <td>${s.name}</td>
                            <td><span class="${stateClass}">${s.state}</span></td>
                            <td>${s.sub_state}</td>
                            <td class="text-secondary">${s.description}</td>
                        </tr>
                    `;
                }).join('')}
            </tbody>
        </table>
    `;
}

// Utilities
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
    } else {
        return `up ${hours}:${minutes.toString().padStart(2, '0')}`;
    }
}

function getUsageClass(percent) {
    if (percent < 70) return 'text-success';
    if (percent < 90) return 'text-warning';
    return 'text-danger';
}
