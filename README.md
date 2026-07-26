# NanoMon

<div align="center">

**🚀 Lightweight NAS monitoring tool with < 15 MB RAM footprint**

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.83%2B-orange.svg)](https://www.rust-lang.org/)
[![Docker](https://img.shields.io/badge/docker-ready-blue.svg)](https://hub.docker.com/)

*Monitor your NAS without breaking a sweat*

[Features](#features) • [Quick Start](#quick-start) • [API](#api-endpoints) • [Architecture](#architecture) • [Contributing](#contributing)

</div>

---

## 📖 Overview

NanoMon is a **minimalist monitoring solution** designed for **resource-constrained environments** like NAS devices (Ugreen, Synology, QNAP, TrueNAS, etc.). It provides real-time insights into:

- 🐳 Docker containers (with Compose stack grouping)
- 💻 System processes (CPU, memory usage)
- 📊 Host metrics (CPU, RAM, load average, uptime)
- 💾 Disk usage across mount points
- 🌐 Network interface statistics

All through a **clean web interface** and **REST API**, with a footprint smaller than your morning coffee break.

## ✨ Features

### Performance
- ⚡ **< 15 MB RAM** footprint in production
- 🎯 **< 1% CPU** usage on average
- 📦 **< 10 MB** binary size (stripped)
- 🔥 **< 50ms** API latency

### Monitoring Capabilities
- 🐳 **Docker containers**: Real-time CPU, memory, network, block I/O stats
- 📚 **Stack grouping**: Automatic grouping by `docker-compose` projects
- 🔍 **Process inspector**: Top N processes by CPU or memory
- 💻 **Host metrics**: CPU breakdown (user/sys/iowait), RAM, swap, load average
- 💾 **Disk usage**: All mount points with usage percentages
- 🌐 **Network stats**: Interface-level RX/TX bytes and errors

### User Experience
- 🎨 **Modern dark theme**: Easy on the eyes during late-night debugging
- 📱 **Responsive design**: Works on desktop, tablet, and mobile
- 🔄 **Auto-refresh**: Configurable polling interval
- 🚀 **Zero-dependency frontend**: Vanilla JS, no build step

### Technical
- 🏗️ **Hexagonal architecture**: Clean, testable, maintainable codebase
- 🦀 **Written in Rust**: Memory-safe, fast, concurrent
- 🔌 **REST API**: Full JSON API for custom integrations
- 🐧 **ARM64 support**: Native support for ARM-based NAS devices

## 🚀 Quick Start

### Method 1: Docker Compose (Recommended)

```bash
# Clone the repository
git clone https://github.com/mrz1880/nanomon.git
cd nanomon

# Start NanoMon
docker compose up -d

# View logs
docker compose logs -f
```

Open **http://localhost:3000** in your browser 🎉

### Method 2: Docker Run

```bash
docker run -d \
  --name nanomon \
  -p 3000:3000 \
  -v /var/run/docker.sock:/var/run/docker.sock:ro \
  -v /proc:/host/proc:ro \
  -v /sys:/host/sys:ro \
  -e NANOMON_PROC_PATH=/host/proc \
  -e NANOMON_SYS_PATH=/host/sys \
  --pid=host \
  nanomon:latest
```

### Method 3: Build from Source

```bash
# Prerequisites: Rust 1.83+
cargo build --release

# Run (requires root for /proc and Docker socket access)
sudo ./target/release/nanomon
```

**Note**: The web interface will be available at `http://localhost:3000` by default.

## ⚙️ Configuration

Customize NanoMon via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `NANOMON_PORT` | `3000` | HTTP server port |
| `NANOMON_BIND` | `0.0.0.0` | Listen address (e.g. `127.0.0.1` for loopback-only behind a reverse proxy) |
| `NANOMON_POLL_INTERVAL` | `10` | Slow-tier interval in seconds (disks, containers, processes, history) |
| `NANOMON_FAST_INTERVAL` | `1` | Fast-tier interval in seconds (live host metrics streamed over SSE) |
| `NANOMON_HIRES_SIZE` | `120` | High-resolution points kept for live charts (~2 min @ 1s) |
| `NANOMON_SSE_KEEPALIVE` | `15` | Keep-alive interval in seconds for the `/api/stream` SSE endpoint |
| `NANOMON_HISTORY_SIZE` | `360` | Number of snapshots to keep (1h @ 10s interval) |
| `NANOMON_PROCESS_LIMIT` | `20` | Maximum processes to display in UI |
| `DOCKER_HOST` | `unix:///var/run/docker.sock` | Docker socket path |
| `NANOMON_PROC_PATH` | `/proc` | Path to procfs (use `/host/proc` in Docker) |
| `NANOMON_SYS_PATH` | `/sys` | Path to sysfs (use `/host/sys` in Docker) |
| `NANOMON_LOG_LEVEL` | `info` | Logging verbosity (`trace`/`debug`/`info`/`warn`/`error`) |
| `NANOMON_ENABLE_SYSTEMD` | `false` | Enable systemd service monitoring (`/api/services`, service metrics/alerts) |
| `NANOMON_SERVICE_FILTER` | *(all)* | Comma-separated glob allow-list of units (e.g. `nginx,worker-*`); restricts `/api/services`, the UI, `/metrics`, and alerts |

**Example** (custom port):
```bash
NANOMON_PORT=8080 docker compose up -d
```

## 🔌 API Reference

NanoMon exposes a REST API for programmatic access:

| Endpoint | Description |
|----------|-------------|
| `GET /api/health` | Health check (service status) |
| `GET /api/host` | Host metrics (CPU, RAM, load, uptime) |
| `GET /api/containers` | All containers with stats, grouped by Compose stacks |
| `GET /api/processes?sort={cpu\|memory}&limit=N` | Top N processes sorted by CPU or memory |
| `GET /api/disks` | Disk usage for all mount points |
| `GET /api/network` | Network interface statistics (RX/TX bytes, errors) |
| `GET /api/services` | Systemd unit states (requires `NANOMON_ENABLE_SYSTEMD=true`) |
| `GET /api/dashboard` | **Aggregated view** (all metrics in one call) |
| `GET /metrics` | Prometheus exposition (host, disks, network, containers, temps, `nanomon_service_state`) |

### Example: Host Metrics

**Request:**
```bash
curl http://localhost:3000/api/host | jq
```

**Response:**
```json
{
  "timestamp": "2026-01-07T14:32:00Z",
  "hostname": "ugreen-nas",
  "uptime_seconds": 1234567,
  "load_average": { "one": 0.5, "five": 0.8, "fifteen": 1.2 },
  "cpu": {
    "usage_percent": 23.5,
    "user_percent": 15.2,
    "system_percent": 8.3,
    "iowait_percent": 2.1
  },
  "memory": {
    "used_bytes": 4294967296,
    "total_bytes": 8589934592,
    "available_bytes": 4294967296,
    "cached_bytes": 1073741824,
    "swap_used_bytes": 0
  }
}
```

## 📦 Deployment on NAS Devices

### General Instructions

1. **Copy files to your NAS:**
   ```bash
   scp -r . your-nas:~/nanomon/
   ```

2. **SSH into your NAS:**
   ```bash
   ssh your-nas
   ```

3. **Start NanoMon:**
   ```bash
   cd ~/nanomon
   docker compose up -d
   ```

### Device-Specific Notes

#### Ugreen NAS
- ✅ Tested on DXP4800 Plus (ARM64)
- Use default `docker-compose.yml` configuration

#### Synology
- ✅ Works with DSM 7.x
- Ensure Docker package is installed via Package Center
- May need to adjust paths in `docker-compose.yml` if using non-standard volumes

#### QNAP
- ✅ Works with QTS 5.x
- Install Container Station from App Center
- Use the Docker Compose method

#### TrueNAS Scale
- ✅ Native Docker support
- Works out of the box with Docker Compose

## 🏗️ Architecture

NanoMon follows **Hexagonal Architecture** (Ports & Adapters) for clean separation of concerns:

```
┌──────────────────────────────────────────────────────┐
│                    Interface Layer                   │
│  ┌──────────────┐  ┌──────────────────────────────┐ │
│  │  HTTP API    │  │      Static Web UI           │ │
│  │  (Axum)      │  │   (HTML/CSS/JS)              │ │
│  └──────┬───────┘  └──────────────────────────────┘ │
├─────────┼────────────────────────────────────────────┤
│         ▼            Application Layer               │
│  ┌──────────────────────────────────────────────┐   │
│  │        MonitoringService                      │   │
│  │  - collect_all()                             │   │
│  │  - get_containers()                          │   │
│  │  - get_top_processes()                       │   │
│  └──────────────────────────────────────────────┘   │
├──────────────────────────────────────────────────────┤
│                     Ports (Traits)                   │
│  ContainerSource │ SystemSource │ ProcessSource     │
├──────────────────────────────────────────────────────┤
│                     Adapters                         │
│  ┌───────────┐  ┌───────────┐  ┌──────────────┐    │
│  │  Docker   │  │  Procfs   │  │ MemoryStore  │    │
│  │ (bollard) │  │ (/proc)   │  │ (ring buffer)│    │
│  └───────────┘  └───────────┘  └──────────────┘    │
└──────────────────────────────────────────────────────┘
```

**Benefits:**
- ✅ **Testable**: Mock adapters for unit tests
- ✅ **Maintainable**: Clear boundaries between layers
- ✅ **Flexible**: Easy to add new data sources (e.g., systemd, SMART)

### Technology Stack

| Component | Technology | Why |
|-----------|-----------|-----|
| **Language** | Rust 2021 | Memory safety, performance, concurrency |
| **HTTP Server** | Axum 0.8 | Lightweight, tokio-native, ergonomic |
| **Docker Client** | Bollard 0.18 | Official async Docker client |
| **Async Runtime** | Tokio 1.x | Industry standard for async Rust |
| **Frontend** | Vanilla JS | Zero dependencies, instant load |
| **Serialization** | Serde | Fast, compile-time safety |

## 🛠️ Development

### Prerequisites

- Rust 1.83+ ([install](https://rustup.rs/))
- Docker (for container monitoring)

### Build

```bash
cargo build --release
```

### Run locally

```bash
# Requires root for /proc access and Docker socket
sudo cargo run
```

### Project Structure

```
src/
├── domain/          # Business entities (Container, Process, Host, etc.)
├── ports/           # Trait definitions (interfaces)
├── adapters/        # Port implementations
│   ├── docker/      # Docker client (bollard)
│   ├── procfs/      # System metrics (/proc, /sys)
│   └── store/       # In-memory storage
├── application/     # Business logic (MonitoringService)
├── interface/       # HTTP API and web UI
└── main.rs          # Composition root
```

### Code Quality

```bash
# Format
cargo fmt

# Lint
cargo clippy

# Test (when available)
cargo test
```

## 🗺️ Roadmap

- [x] **v0.1** (MVP): Core monitoring (host, containers, processes, disks)
- [ ] **v0.2**: Temperatures (CPU, disks), SMART health, systemd services, historical charts
- [ ] **v0.3**: Alerting, Prometheus `/metrics` endpoint, multi-host support
- [ ] **v0.4**: Authentication, HTTPS, email/webhook notifications

See [CLAUDE.md](CLAUDE.md) for detailed development context.

## 🤝 Contributing

Contributions are welcome! Here's how you can help:

1. **Report bugs**: Open an [issue](https://github.com/mrz1880/nanomon/issues)
2. **Suggest features**: Describe your use case
3. **Submit PRs**: Fork, branch, code, test, and submit!

**Guidelines:**
- Follow Rust conventions (`cargo fmt`, `cargo clippy`)
- Keep binary size minimal (run `cargo bloat` to check)
- Update documentation for new features
- Add tests for critical paths (when test suite exists)

## 📄 License

MIT License - see [LICENSE](LICENSE) for details.

## 🙏 Acknowledgments

- Inspired by the need for lightweight monitoring on ARM NAS devices
- Built with ❤️ using Rust and the tokio ecosystem
- Special thanks to the Bollard, Axum, and Serde communities

---

<div align="center">

**If NanoMon helped you, consider ⭐ starring the repo!**

Made with 🦀 by the community

</div>
