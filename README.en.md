# vpn-geo 4.0

[![CI](https://github.com/zhangyucheng1014-dev/vpn-geo/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/zhangyucheng1014-dev/vpn-geo/actions/workflows/ci.yml) [![Release](https://img.shields.io/github/v/release/zhangyucheng1014-dev/vpn-geo?display_name=tag)](https://github.com/zhangyucheng1014-dev/vpn-geo/releases) [![License](https://img.shields.io/github/license/zhangyucheng1014-dev/vpn-geo)](LICENSE)

[中文](README.md) | **English**

`vpn-geo` has been fully rewritten again as a four-layer Cargo workspace: `vpn-geo-domain` owns I/O-free rules, `vpn-geo-app` owns use cases and ports, `vpn-geo-infra` owns external adapters, and `vpn-geo-cli` owns parsing and rendering. Every system interaction is injected through `async_trait` ports, so domain behavior is independently testable. Existing 1.x/2.x/3.x configurations and commands remain compatible.

All previous capabilities remain: event-driven NetworkManager VPN disconnect handling, stable-route waiting, GeoIP consensus, country-based autoconnect priority, state locking, regional mirrors, official mirror synchronization, mirror scoring, safe pacman application, VPN/proxy/direct diagnostics, and JSON output. The daemon never connects, disconnects, or switches a VPN.

## Architecture

- `crates/vpn-geo-domain`: configuration, catalog, GeoIP consensus, scoring, route classification, disconnect state machine; no Tokio/D-Bus/HTTP dependency.
- `crates/vpn-geo-app`: ports and use cases for `speed`, `route`, `doctor`, mirrors, and daemon scheduling.
- `crates/vpn-geo-infra`: reqwest/rustls, zbus D-Bus, nmcli, flock state, official mirror status, mirrorlist, config repository adapters.
- `crates/vpn-geo-cli`: clap commands, tables, JSON, logging, exit codes.

## Commands

```bash
vpn-geo                              # event-driven watcher
vpn-geo check [--json]
vpn-geo status [--json] [--no-dbus]
vpn-geo sources [--json]
vpn-geo doctor [--json] [--skip-network]
vpn-geo speed [apply] [--direct] [--json]
vpn-geo route [compare] [--direct] [--json]
vpn-geo mirror list [-r cn|hk|jp|us|global] [--json]
vpn-geo mirror speed [-r jp] [--direct] [--json]
vpn-geo mirror sync --regions cn,hk,jp,us --per-region 6 [--json] [--output FILE]
vpn-geo mirror best [-r cn] [--yes] [--json]
sudo vpn-geo mirror apply cn-tsinghua --yes
```

`mirror sync` reads the [official Arch mirror status](https://archlinux.org/mirrors/status/json/) and exports generated `[[mirrors]]` entries. `mirror best` scores throughput and latency; only `--yes` writes the pacman mirrorlist. The built-in catalog has 29 HTTPS sources across CN (8), HK (3), JP (7), US (10), and Global (1).

## Build

```bash
cargo fmt --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo build --locked --workspace --release
```

Arch Linux users can use the included `PKGBUILD`. Licensed under [MIT](LICENSE).
