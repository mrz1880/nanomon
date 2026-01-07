# CLAUDE.md - NanoMon Project Context

## Project Overview

**NanoMon** is a lightweight monitoring tool for NAS devices with strict resource constraints (< 15 MB RAM, < 1% CPU). It monitors Docker containers, system processes, host metrics (CPU, RAM, load, disks, network), with a clean web UI and REST API.

**Target environment**: ARM64 NAS (Ugreen, Synology, etc.) running Docker

## Architecture Principles

### Hexagonal Architecture (Ports & Adapters)

The project follows a strict hexagonal architecture to ensure:
- **Testability**: Business logic independent of I/O
- **Maintainability**: Clear boundaries between layers
- **Flexibility**: Easy to swap adapters (e.g., mock Docker for tests)

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

### From user's global CLAUDE.md

1. **No `any`**: Forbidden. Always type explicitly.
2. **Readonly fields**: If assigned only in constructor, mark `readonly` (N/A in Rust, but use `pub` fields sparingly)
3. **Optional chaining**: Prefer `Option` and `?` over explicit `match` when appropriate
4. **Design patterns**: GoF patterns are welcome but pragmatically (avoid over-engineering)
5. **Composition over inheritance**: Rust enforces this via traits

### Rust-specific additions

1. **Error handling**: Use `Result<T, Box<dyn std::error::Error>>` for simplicity in MVP (thiserror for domain errors in future)
2. **Async**: All I/O is async (tokio), domain layer is sync
3. **Lifetimes**: Avoid `'static` unless necessary (e.g., Arc<T>)
4. **Cloning**: Prefer `Arc` for shared ownership over `Clone` on large structs
5. **Traits**: Prefer `async_trait` over manual `Pin<Box<Future>>` for readability

## Testing Strategy (Future)

### Current state (MVP)
- No tests yet (pragmatic choice for speed)
- Code is structured to be testable

### Planned for v0.2
1. **Unit tests**: Domain layer (pure logic)
2. **Integration tests**: Adapters with fixtures (fake `/proc` files)
3. **E2E tests**: HTTP API with in-memory adapters

### How to test adapters
- Create `tests/fixtures/proc/` with snapshots of real `/proc` files
- Mock `ProcfsConfig` to point to fixture directory
- Example: `tests/fixtures/proc/stat`, `tests/fixtures/proc/1234/status`, etc.

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
| `NANOMON_POLL_INTERVAL` | `10` | - | Not used yet (no polling loop in MVP) |
| `NANOMON_HISTORY_SIZE` | `360` | MemoryStore | Ring buffer size (1h @ 10s) |
| `NANOMON_PROCESS_LIMIT` | `20` | API handlers | Max processes returned |
| `DOCKER_HOST` | `unix:///var/run/docker.sock` | DockerAdapter | Docker socket path |
| `NANOMON_PROC_PATH` | `/proc` | ProcfsAdapter | Path to procfs (use `/host/proc` in Docker) |
| `NANOMON_SYS_PATH` | `/sys` | ProcfsAdapter | Path to sysfs (use `/host/sys` in Docker) |
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
- Install build essentials: `apt-get install build-essential pkg-config libssl-dev`

### `error: failed to run custom build command for openssl-sys`
- Install OpenSSL dev: `apt-get install libssl-dev`
- Or use vendored OpenSSL: Add to Cargo.toml: `openssl = { version = "0.10", features = ["vendored"] }`

### Binary size too large (> 20 MB)
- Check release profile in Cargo.toml (should have `opt-level = "z"`, `lto = true`, `strip = true`)
- Use `cargo bloat --release` to find large dependencies
- Consider `upx --best target/release/nanomon` (compress binary)

## Contributing Guidelines

### Before submitting PR
1. Run `cargo fmt` (format)
2. Run `cargo clippy -- -D warnings` (lints)
3. Run `cargo test` (when tests exist)
4. Ensure binary size < 10 MB: `ls -lh target/release/nanomon`
5. Test in Docker: `docker compose up --build`

### Code review checklist
- [ ] No `unwrap()` or `expect()` in production code (use `?` or handle gracefully)
- [ ] All public APIs documented with `///` comments
- [ ] No panics in adapters (return `Result`)
- [ ] Errors logged with `tracing::error!` before returning
- [ ] Optional fields use `Option<T>`, required fields are non-optional

## Contact & Resources

- **Spec**: See `README.md` for high-level overview
- **Detailed spec**: See original specifications file in project root
- **Issues**: Track bugs/features in GitHub issues
- **Architecture**: Hexagonal pattern inspired by Alistair Cockburn's "Ports and Adapters"

---

**Last updated**: 2026-01-07 (MVP v0.1 complete)
