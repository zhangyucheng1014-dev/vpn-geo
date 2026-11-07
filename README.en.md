# vpn-geo 3.0

[![CI](https://github.com/zhangyucheng1014-dev/vpn-geo/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/zhangyucheng1014-dev/vpn-geo/actions/workflows/ci.yml) [![Release](https://img.shields.io/github/v/release/zhangyucheng1014-dev/vpn-geo?display_name=tag)](https://github.com/zhangyucheng1014-dev/vpn-geo/releases) [![License](https://img.shields.io/github/license/zhangyucheng1014-dev/vpn-geo)](LICENSE)

[中文](README.md) | **English**

`vpn-geo` has been fully rewritten again as a Cargo workspace: `vpn-geo-core` owns configuration, structured errors, D-Bus, GeoIP consensus, benchmarks, state, and mirror services; `vpn-geo-cli` owns clap parsing and human/JSON presentation.

All previous behavior remains: event-driven NetworkManager VPN disconnect handling, stable-route waiting, multi-source GeoIP, country-based autoconnect priority, state locking, `check`, `speed`, `status`, `route`, 29 regional mirror sources, JSON output, and safe pacman mirrorlist application. The daemon never connects, disconnects, or switches a VPN and changes only `connection.autoconnect-priority` for configured UUIDs.

## New in 3.0

- Split core/CLI crates with structured `thiserror` errors and typed exit codes.
- clap command tree, including `doctor`, `mirror sync`, and `mirror best`.
- Data-driven built-in mirror catalog covering CN (8), HK (3), JP (7), US (10), and Global (1).
- Dynamic official mirror discovery from Arch Linux mirror status; results can be exported as `[[mirrors]]` TOML.
- Benchmark scoring by throughput and latency; `mirror best --yes` measures and applies the best pacman source.
- Configurable GeoIP `min_agreement`; existing 1.x/2.x configuration files remain valid.

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

`mirror sync` reads the [official Arch mirror status](https://archlinux.org/mirrors/status/json/) and filters active HTTPS sources by score, delay, and completeness. `mirror apply` and `mirror best --yes` write only a marked vpn-geo block, create a backup, and require root.

## Build

```bash
cargo fmt --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo build --locked --workspace --release
```

Arch Linux users can use the included `PKGBUILD`. Licensed under [MIT](LICENSE).
