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
            case 'containers':
                await loadContainers();
                break;
            case 'processes':
                await loadProcesses();
                break;
            case 'disks':
                await loadDisks();
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
        `user ${data.cpu.user_percent.toFixed(1)}% · sys ${data.cpu.system_percent.toFixed(1)}% · iowait ${(data.cpu.iowait_percent || 0).toFixed(1)}%`;

    // Update RAM
    const ramPercent = (data.memory.used_bytes / data.memory.total_bytes * 100).toFixed(1);
    document.getElementById('ram-percent').textContent = ramPercent + '%';
    document.getElementById('ram-bar').style.width = ramPercent + '%';
    document.getElementById('ram-details').textContent =
        `${formatBytes(data.memory.used_bytes)} / ${formatBytes(data.memory.total_bytes)}`;

    // Update network (placeholder - needs calculation from previous values)
    document.getElementById('net-rx').textContent = '0 MB/s';
    document.getElementById('net-tx').textContent = '0 MB/s';
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
                    <span class="stack-name">▼ ${stack.name}</span>
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

function renderContainer(container, isTopConsumer = false) {
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
                    <span>▼ ${formatBytes(container.network.rx_bytes)}</span>
                    <span>▲ ${formatBytes(container.network.tx_bytes)}</span>
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
