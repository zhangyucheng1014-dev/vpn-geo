# vpn-geo 2.0

[![CI](https://github.com/zhangyucheng1014-dev/vpn-geo/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/zhangyucheng1014-dev/vpn-geo/actions/workflows/ci.yml) [![Release](https://img.shields.io/github/v/release/zhangyucheng1014-dev/vpn-geo?display_name=tag)](https://github.com/zhangyucheng1014-dev/vpn-geo/releases) [![License](https://img.shields.io/github/license/zhangyucheng1014-dev/vpn-geo)](LICENSE)

[中文](README.md) | **English**

`vpn-geo` is now a Rust rewrite (Tokio, `zbus`, `reqwest`, rustls) of the former Go 1.24 project. It preserves the original behavior: after the last active NetworkManager VPN disconnects, it waits for routes to settle, detects the public-IP country, and raises only `connection.autoconnect-priority` for configured profiles.

Version 2.0 adds concurrent multi-source GeoIP consensus, a configurable pacman/generic mirror catalog, mirror benchmarking and safe mirrorlist application, current-route versus no-proxy baseline tests, and VPN/system-proxy/direct status detection. Key commands support JSON output.

## Quick usage

```bash
vpn-geo                         # event-driven background watcher
vpn-geo check [--json]          # validate configuration
vpn-geo status [--json]         # inspect VPN, proxy and route state
vpn-geo sources [--json]        # show GeoIP sources
vpn-geo speed                   # benchmark nearest configured nodes
vpn-geo speed apply             # benchmark and promote the fastest node
vpn-geo route compare           # benchmark current route and no-proxy baseline
vpn-geo mirror list             # list built-in and configured mirrors
vpn-geo mirror speed            # benchmark mirrors (read-only)
sudo vpn-geo mirror apply tsinghua --yes
```

The daemon never connects, disconnects, or switches a VPN. It does not read or modify credentials, server addresses, routes, DNS, or any NetworkManager property other than the autoconnect priority of configured UUIDs. It remains idle on D-Bus signals; no polling and no network requests occur until a real VPN disconnect edge.

## New capabilities

- GeoIP: three default sources are queried concurrently with a majority vote; up to eight custom `urls` are supported, and the legacy single `url` key still works.
- Mirrors: five built-in Arch Linux sources plus custom `[[mirrors]]` entries. `mirror apply` changes only a marked vpn-geo block, creates a timestamped backup, and requires `--yes` and root. Generic sources are benchmark-only.
- Route: `status` reports active VPN profiles, proxy environment/GNOME proxy, devices, active links, and a confidence-tagged route classification. `route compare` measures both current and no-proxy paths without toggling VPN itself.
- Performance: one small Rust binary, single-thread Tokio runtime, debounced D-Bus signals, bounded concurrency, response/download limits, rustls, and a 96 MiB systemd memory cap.

Third-party HTTP/SOCKS proxy clients and tun modes can bypass NetworkManager, so status is a best-effort classification rather than a routing-table proof. A benchmark measures the current egress path, not the speed of each VPN tunnel after connecting to it.

## Build and install

```bash
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
```

Arch Linux users can run `makepkg -si` with the included `PKGBUILD`. Configuration lives at `$XDG_CONFIG_HOME/vpn-geo/config.toml` (or `~/.config/vpn-geo/config.toml`); state lives under `$XDG_STATE_HOME/vpn-geo/` and stores only the last successfully applied country code.

All 1.x configuration keys and commands remain compatible. See [README.md](README.md) for the complete Chinese guide.

Licensed under [MIT](LICENSE).
