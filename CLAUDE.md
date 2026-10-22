# CLAUDE.md - NanoMon Project Context

## Project Overview

**NanoMon** is a lightweight monitoring tool for self-hosted servers and NAS devices with strict resource constraints (< 15 MB RAM, < 1% CPU). It monitors Docker containers, system processes, host metrics (CPU, RAM, load, disks, network), temperatures and systemd services, with alerting, a Prometheus endpoint, a clean web UI and REST API.

**Target environment**: Linux servers and NAS (Ugreen, Synology, Raspberry Pi, VPS, etc.) running Docker. NAS remains a primary use case, but the scope is general server monitoring.

## Architecture Principles

### Hexagonal Architecture (Ports & Adapters)

```
Domain (entities, value objects)
  ↓
Ports (trait definitions)
  ↓
Adapters (implementations: Docker, procfs, store)
  ↓
Application (orchestration: MonitoringService)
  ↓
Interface (HTTP API + Web UI)
```

**Key rule**: Dependencies point inward. Domain has zero external dependencies.

### Layer Responsibilities

1. **Domain** (`src/domain/`)
   - Pure business entities: `Host`, `Container`, `Process`, `Disk`, etc.
   - Value objects: `CpuMetrics`, `MemoryMetrics`, `LoadAverage`, etc.
   - Trait `MonitoredResource` for polymorphism
   - **No I/O, no async, no external crates except serde**

2. **Ports** (`src/ports/`)
   - Trait definitions: `ContainerSource`, `SystemSource`, `ProcessSource`, `MetricStore`
   - Define **what** the application needs, not **how**
   - All traits are `async_trait` and `Send + Sync`

3. **Adapters** (`src/adapters/`)
   - `DockerAdapter`: implements `ContainerSource` using bollard
   - `ProcfsAdapter`: implements `SystemSource` and `ProcessSource` by parsing `/proc` and `/sys`
   - `MemoryStore`: implements `MetricStore` with a ring buffer
   - **Only layer allowed to do I/O**

4. **Application** (`src/application/`)
   - `MonitoringService`: orchestrates calls to ports
   - Methods: `collect_all()`, `get_containers()`, `get_top_processes_by_cpu()`, etc.
   - Pure business logic, delegates to ports

5. **Interface** (`src/interface/`)
   - HTTP API (Axum): routes + handlers
   - Static web UI (HTML/CSS/JS)
   - Depends on application layer only

## Key Design Decisions

### Why procfs instead of a crate like `sysinfo`?
- **Control**: Direct parsing ensures minimal allocations
- **Size**: Avoid transitive dependencies that bloat the binary
- **Learning**: Understanding Linux internals is valuable for this use case

### Why no database?
- **Simplicity**: In-memory ring buffer is sufficient for MVP
- **Performance**: No I/O latency
- **Footprint**: SQLite adds ~1 MB to binary + runtime overhead
- **Future**: v0.2+ may add optional persistence

### Why vanilla JS instead of React/Vue?
- **Size**: Zero bundle, zero build step
- **Speed**: Instant load time
- **Simplicity**: No transpilation, no tooling
- **Target**: NAS users value simplicity over fancy UI

### Why Axum 0.8?
- **Size**: Smaller than actix-web or warp
- **Ergonomics**: Tower middleware ecosystem
- **Performance**: tokio-native, minimal overhead

## Code Style Guidelines

Language-wide Rust rules live in `~/.claude/rules/rust.md`, imported at the bottom of this file.
Only what is specific to NanoMon is listed here.

### Rust-specific additions

1. **Error handling**: Use `Result<T, Box<dyn std::error::Error>>` for simplicity in MVP (thiserror for domain errors in future)
2. **Async**: All I/O is async (tokio), domain layer is sync
3. **Lifetimes**: Avoid `'static` unless necessary (e.g., Arc<T>)
4. **Cloning**: Prefer `Arc` for shared ownership over `Clone` on large structs
5. **Traits**: Prefer `async_trait` over manual `Pin<Box<Future>>` for readability

### HTTP layer (Axum)

1. **Handler errors**: `AppError` and its `IntoResponse` impl exist in `interface/http/handlers.rs`,
   but no handler uses them yet — 10 handlers still build
   `(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()` inline. That duplicates the
   mapping and returns the raw internal error to the client. New handlers return
   `Result<_, AppError>`; convert an existing one whenever you touch it.
2. **Shared state**: handlers receive `MonitoringService` through the router state. No global
   mutable state, no lazy statics.
3. **Cross-cutting concerns** (request tracing, timeouts, compression) belong in `tower` layers in
   `interface/http/routes.rs`, not repeated per handler. Only `CorsLayer::permissive()` is wired
   today — which is another reason not to expose port 3000 publicly.
4. **Blocking work**: procfs reads are small and stay inline in the async path. Anything that can
   block for more than a few milliseconds (SMART, D-Bus, external HTTP scrape) goes to
   `tokio::task::spawn_blocking` or a background task: the collector shares the reactor with the
   API, and the CPU budget is < 1%.

## Testing Strategy

### Current state
- ~150 unit tests living in `#[cfg(test)]` modules next to the code
- Domain layer, procfs parsers/adapters, store and application services covered
  (~86% line coverage of the business logic; see **Quality tooling** below)
- Still missing: HTTP end-to-end tests, and the Docker adapter (needs a daemon)

### How to test adapters
- **procfs**: build a fake tree under `std::env::temp_dir()` and point
  `ProcfsConfig` at it. The `FakeProc` / `FakeFs` helpers in
  `adapters/procfs/{process,system}.rs` do this; no fixtures are committed, so
  there is nothing to keep in sync with a real machine.
- **Ports**: implement the trait with a mock struct (see the mocks in
  `application/monitoring.rs`) rather than reaching for a mocking crate.
- **Next**: HTTP tests driving an in-memory `axum::Router`, which would bring
  `interface/` into the coverage gate.

## Performance Targets

| Metric | Target | Measured |
|--------|--------|----------|
| RAM idle | < 10 MB | TBD |
| RAM under load | < 15 MB | TBD |
| CPU average | < 1% | TBD |
| API latency | < 50ms | TBD |
| Binary size (release) | < 10 MB | TBD |

**Measurement**: Use `docker stats nanomon` for RAM/CPU, `hyperfine` for latency, `ls -lh target/release/nanomon` for size.

## Common Tasks

### Adding a new metric (e.g., temperatures)

1. **Domain**: Add field to `Host` or create new entity `Temperature`
2. **Port**: Extend `SystemSource` trait with `async fn get_temperatures() -> Result<Vec<Temperature>>`
3. **Adapter**: Implement in `ProcfsSystemSource` by reading `/sys/class/thermal/`
4. **Application**: Add method to `MonitoringService`
5. **Interface**: Add API endpoint `/api/temperatures` + handler
6. **UI**: Add new tab or section in web UI

### Adding a new adapter (e.g., for systemd services)

1. **Port**: Define `trait ServiceSource { async fn list_services() -> Vec<Service> }`
2. **Adapter**: Implement using D-Bus client (e.g., `zbus` crate)
3. **Wire in `main.rs`**: Create adapter, pass to `MonitoringService`
4. **Application**: Add `get_services()` method
5. **Interface**: Add endpoint + UI

### Debugging procfs parsing

- Add `tracing::debug!("raw content: {}", content)` before parsing
- Set `NANOMON_LOG_LEVEL=debug`
- Use `docker exec -it nanomon cat /host/proc/stat` to inspect mounted files

## Environment Variables Reference

| Variable | Default | Used by | Notes |
|----------|---------|---------|-------|
| `NANOMON_PORT` | `3000` | main | HTTP listen port |
| `NANOMON_BIND` | `0.0.0.0` | main | Listen address (e.g. `127.0.0.1` for loopback-only) |
| `NANOMON_POLL_INTERVAL` | `10` | Collector | Slow-tier interval (disks, containers, processes, history) |
| `NANOMON_FAST_INTERVAL` | `1` | Collector | Fast-tier interval (live host metrics streamed via SSE) |
| `NANOMON_HIRES_SIZE` | `120` | Collector | High-resolution ring buffer for live charts (~2 min @ 1s) |
| `NANOMON_SSE_KEEPALIVE` | `15` | Interface | Keep-alive interval for the `/api/stream` SSE endpoint |
| `NANOMON_HISTORY_SIZE` | `360` | MemoryStore | Ring buffer size (1h @ 10s) |
| `NANOMON_PROCESS_LIMIT` | `20` | API handlers | Max processes returned |
| `DOCKER_HOST` | `unix:///var/run/docker.sock` | DockerAdapter | Docker socket path |
| `NANOMON_SERVICE_FILTER` | *(all)* | SystemctlAdapter | Comma-separated glob allow-list of units (e.g. `nginx,worker-*`) |
| `NANOMON_WATCH_DIRS` | *(none)* | FsDirectoryUsage | Comma-separated dirs to report per-directory usage (cached at poll interval) |
| `NANOMON_SCRAPE_URLS` | *(none)* | HttpScrapeSource | Comma-separated `job=url` external metric targets (Prometheus text / JSON) |
| `NANOMON_PROC_PATH` | `/proc` | ProcfsAdapter | Path to procfs (use `/host/proc` in Docker) |
| `NANOMON_SYS_PATH` | `/sys` | ProcfsAdapter | Path to sysfs (use `/host/sys` in Docker) |
| `NANOMON_HOSTNAME` | *(detected)* | ProcfsAdapter | Host name to report. A container cannot discover its host's: `/etc/hostname` is its own and `/proc/sys/kernel/hostname` is namespaced, so alerts otherwise carry the container ID |
| `NANOMON_LOG_LEVEL` | `info` | tracing | Log verbosity (trace/debug/info/warn/error) |

## Docker Deployment Notes

### Required mounts
- `/var/run/docker.sock:/var/run/docker.sock:ro` → Docker API
- `/proc:/host/proc:ro` → Host system metrics
- `/sys:/host/sys:ro` → Network/disk stats

### Why `pid: host`?
Without `pid: host`, the container sees only its own processes. With it, `/proc` contains all host processes, allowing process monitoring.

### Security considerations
- Container needs read access to Docker socket (risk: container escape)
- `pid: host` exposes all PIDs (risk: information disclosure)
- Use `read_only: true` + `tmpfs` to mitigate
- **Do not expose port 3000 publicly** (no auth in MVP)

## Alerting behaviour

A numeric rule notifies **once** when the value crosses its threshold and once
more when it comes back, staying silent in between however long the incident
lasts. Before this, a rule re-fired on every cooldown while breached, so a
single event produced dozens of identical notifications with no way to tell
when it ended.

- **Hysteresis**: the incident only closes once the value clears the threshold
  by a margin (5% of the threshold by default), so a value sitting on the limit
  cannot alternate between firing and resolved.
- **Severity** (`info` / `warning` / `critical`) drives the ntfy priority and
  tag, so a full disk does not arrive looking like a load spike. A resolution is
  always sent quietly.
- **A broken alert config stops the process**: when `NANOMON_ALERT_CONFIG` is
  set but the file is unreadable or invalid, NanoMon exits instead of running
  without alerts. The webhook URLs live in that file, so a warning in the logs
  was the only trace and nobody would have been notified.
- **A failed send does not close the incident**: `notified` stays false and the
  next cycle retries, so a webhook outage cannot silence an alert.
- Notification bodies carry the metric's business label and unit, the incident
  duration on resolution, and the level the hardware itself warns at when the
  sensor publishes one (`/sys/class/hwmon`). Pick thresholds against that number
  rather than a round one.

## Known Limitations (MVP)

1. **No authentication**: Anyone with network access can view metrics
2. **No persistence**: Restart = data loss
3. **No alerting**: Just monitoring, no notifications
4. **CPU % inaccurate on first call**: Needs delta between readings (fixed after first poll)
5. **Network throughput shows totals**: Not rate (needs previous snapshot for MB/s calculation)
6. **No multi-host support**: One instance per NAS

## Roadmap

### v0.1 (MVP) ✅
- Host, container, process, disk monitoring
- Web UI with 4 tabs
- REST API
- Docker deployment

### v0.2 (Planned)
- Temperatures (CPU via `/sys/class/thermal`, disks via SMART)
- Systemd service status (via D-Bus)
- Historical charts (use MemoryStore history)
- Alerting (threshold checks + webhook)
- Prometheus `/metrics` endpoint

### v0.3 (Future)
- Multi-host: agents report to central instance
- Persistence: optional SQLite backend
- Notifications: email, Slack, Discord
- HTTPS + basic auth

## Debugging Tips

### "Failed to connect to Docker daemon"
- Check socket path: `ls -la /var/run/docker.sock`
- Ensure container has access: `docker run --rm -v /var/run/docker.sock:/var/run/docker.sock:ro nanomon`
- On rootless Docker: `DOCKER_HOST=unix://$XDG_RUNTIME_DIR/docker.sock`

### "No processes found"
- Verify `pid: host` in docker-compose.yml
- Check mount: `docker exec nanomon ls /host/proc` should show PIDs

### "Permission denied" on procfs
- Use `sudo` when running native binary
- In Docker, ensure `/proc` is mounted read-only

### High memory usage
- Check `docker stats nanomon`
- Profile with: `cargo build --release && valgrind --tool=massif ./target/release/nanomon`
- Reduce `NANOMON_HISTORY_SIZE` if using store

## Building for ARM64

### Cross-compilation from x86_64
```bash
# Install cross
cargo install cross

# Build for ARM64
cross build --release --target aarch64-unknown-linux-gnu

# Or use Docker buildx
docker buildx build --platform linux/arm64 -t nanomon:arm64 .
```

### Native build on ARM64 NAS
```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Build
cargo build --release

# Binary at target/release/nanomon
```

## API Usage Examples

### cURL

```bash
# Health check
curl http://localhost:3000/api/health

# Host metrics
curl http://localhost:3000/api/host | jq

# Containers with stacks
curl http://localhost:3000/api/containers | jq '.stacks'

# Top 10 processes by memory
curl 'http://localhost:3000/api/processes?sort=memory&limit=10' | jq

# Dashboard (all data)
curl http://localhost:3000/api/dashboard | jq
```

### JavaScript (fetch)

```javascript
// Auto-refresh dashboard every 10s
async function fetchDashboard() {
  const response = await fetch('/api/dashboard');
  const data = await response.json();

  console.log(`Host: ${data.host.hostname}`);
  console.log(`CPU: ${data.host.cpu.usage_percent.toFixed(1)}%`);
  console.log(`RAM: ${data.host.memory.used_bytes / 1e9} GB`);
}

setInterval(fetchDashboard, 10000);
```

## Troubleshooting Build Issues

### `error: linking with cc failed`
- Install build essentials: `apt-get install build-essential` (TLS is rustls: no OpenSSL needed)

### Binary size too large (> 20 MB)
- Check release profile in Cargo.toml (should have `opt-level = "z"`, `lto = true`, `strip = true`)
- Use `cargo bloat --release` to find large dependencies
- Consider `upx --best target/release/nanomon` (compress binary)

## Contributing Guidelines

### Before submitting PR
1. Run `cargo fmt` (format)
2. Run `cargo clippy -- -D warnings` (lints)
3. Run `cargo test` (unit tests)
4. Run `make coverage` (line coverage of business logic must stay ≥ `COV_MIN`, default 60%)
5. Ensure binary size < 10 MB: `ls -lh target/release/nanomon`
6. Test in Docker: `docker compose up --build`

`make check` runs steps 1-4 together. CI enforces fmt + clippy + tests + the
coverage threshold (`.github/workflows/ci.yml`), and `cargo audit` checks
`Cargo.lock` against RustSec on every push and weekly
(`.github/workflows/audit.yml`).

### Quality tooling

Both gates are **ratchets**, set just below the current value: they exist to
stop regressions, not to describe ambition. Raise them when tests are added;
never lower one without discussing it.

| Metric | Current | Gate | Target |
|--------|---------|------|--------|
| Line coverage (business logic) | ~86% | `COV_MIN` = 80 | reached |
| Mutation score (killed / viable) | ~81% | `MUTATION_MIN` = 75 | reached |

Surviving mutants are concentrated in `systemd`, `dirusage`, `config`,
`collector` and the webhook/scrape I/O wrappers; the domain, procfs and
monitoring layers have none left.

- **Coverage** (`cargo-llvm-cov`): `make coverage` (summary + threshold) or
  `make coverage-html` (browsable report). The gate ignores `main.rs`, the HTTP
  `interface/` and port trait defs (`ports/`) — integration-test territory, so
  the percentage is stricter than a repo-wide number would be.
- **Mutation testing** (`cargo-mutants`): `make mutants`. Scope is business
  logic only (`.cargo/mutants.toml` excludes I/O adapters, interface, main).
  `scripts/mutation-score.sh` computes killed / viable and fails below
  `MUTATION_MIN`. Runs weekly + on demand in CI
  (`.github/workflows/mutants.yml`).

Don't chase 100% on either: mutants inside I/O functions (whose body can be
replaced by `Ok(())` / `vec![]`, e.g. `scrape()`, `send_alert()`, the collector
loops) are unkillable without integration tests.

The domain layer, the procfs parsers/adapters, the store and the application
services are covered. **Remaining gaps**, in order of return: (1) `collector.rs`
(~15%) — the two-speed loops need mocked sources plus time control;
(2) `config.rs` (~34%) — `from_env` mutates process-global state, so it needs
serialized tests; (3) `adapters/docker/client.rs` (0%) — bollard I/O, only
reachable with a real daemon; (4) HTTP integration tests against an in-memory
`axum::Router`, which would bring `interface/` into scope.

**Testing the procfs adapters**: build a throwaway tree under
`std::env::temp_dir()` and point `ProcfsConfig` at it (see the `FakeProc` /
`FakeFs` helpers in `adapters/procfs/{process,system}.rs`) — no fixtures are
checked into the repo.

### Code review checklist

The language-wide items (no `unwrap()`, doc comments, `Option` over sentinels, `tracing::error!`)
come from `~/.claude/rules/rust.md`. Specific to NanoMon:

- [ ] No panic can escape an adapter — a failing source must not take the collector loop down
- [ ] The binary stays under 10 MB and the RAM/CPU budget of the Performance Targets table holds

## Contact & Resources

- **Spec**: See `README.md` for high-level overview
- **Detailed spec**: See original specifications file in project root
- **Issues**: Track bugs/features in GitHub issues
- **Architecture**: Hexagonal pattern inspired by Alistair Cockburn's "Ports and Adapters"

---

**Last updated**: 2026-01-07 (MVP v0.1 complete)

## Shared rules

@~/.claude/rules/rust.md
