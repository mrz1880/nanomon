# Changelog

Notable changes to NanoMon. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/): while in 0.x, a change that breaks an
existing deployment bumps the minor version.

The GitHub release of each tag reuses its section of this file, so record changes
here, under `Unreleased`, as they land. Versions before 0.6.0 are only described
by their git tags and commit history.

## [Unreleased]

### Fixed

- The dashboard stayed on "Loading..." behind a reverse proxy that serves it
  under a path prefix (`/monitoring/`): it requested its script, stylesheet and
  API at the root of the site. It now requests them relative to the page.

- Container CPU % showed the average since the container started, not its
  current load: a container busy once could read 26% while idle. It is now
  measured between two collections, so it reads 0 on a container's first one.

### Changed

- `/api/containers` and `/api/containers/{name}` answer from the last
  collection instead of querying Docker, which cost two stats calls per running
  container on every request. Their data is up to one poll interval old, and the
  `timestamp` field now says when it was collected rather than when it was
  served.

## [0.6.0] - 2026-10-22

### Upgrade notes

- The image now runs as an unprivileged user (uid 10001). Grant it the group that
  owns the Docker socket, or container monitoring stops:
  `group_add: ["<gid>"]` in compose, `--group-add "$(stat -c %g /var/run/docker.sock)"`
  with `docker run`. Any file you mount must be readable by that uid or group.
- When `NANOMON_ALERT_CONFIG` is set, an unreadable or invalid file now stops
  NanoMon at startup instead of running without alerts. Check the logs after
  upgrading.
- Building from source needs Rust 1.88 or newer (was 1.83).

### Security

- Updated dependencies to clear 16 RustSec advisories, including 8 in `rustls`
  and `rustls-webpki` affecting the TLS checks of outgoing webhook and scrape
  calls.
- The runtime image moves from Debian 12 to Debian 13 (trixie).
- The production compose no longer publishes port 3000 on the LAN, which
  bypassed the reverse proxy's authentication.
- CI checks `Cargo.lock` against RustSec on every push and weekly, and workflow
  actions are pinned to commit SHAs.

### Fixed

- Container disk I/O read 0 on cgroup v2 hosts.
- Single-file bind mounts (`/etc/hosts`, a mounted config file) were listed as
  disks and could trip disk-usage alerts.

### Changed

- Dependencies: bollard 0.21, tower-http 0.7, nix 0.31, toml 1.1.
- Dependabot replaces Renovate for dependency updates, including the Dockerfile
  base images.

[Unreleased]: https://github.com/mrz1880/nanomon/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/mrz1880/nanomon/compare/v0.5.0...v0.6.0
