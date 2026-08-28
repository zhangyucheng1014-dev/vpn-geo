# vpn-geo

[![Release](https://img.shields.io/github/v/release/zhangyucheng1014-dev/vpn-geo?display_name=tag)](https://github.com/zhangyucheng1014-dev/vpn-geo/releases) [![License](https://img.shields.io/github/license/zhangyucheng1014-dev/vpn-geo)](LICENSE)

[中文](README.md) | **English**

`vpn-geo` is a lightweight NetworkManager user service for VPN users on Linux.
After the last active VPN disconnects, it waits for the normal route to settle,
checks the current public-IP country, and raises the matching configured VPN
profile's `connection.autoconnect-priority`.

It never connects, disconnects, or switches a VPN itself. It only changes the
autoconnect priority of profiles explicitly listed in the configuration.

## Quick usage

```bash
vpn-geo                  # run the background watcher
vpn-geo check            # validate the configuration
vpn-geo speed            # benchmark nearby configured nodes
vpn-geo speed apply      # benchmark and promote the fastest node
```

See [README.md](README.md) for the complete Chinese user guide.

## Why VPN users use it

When several NetworkManager VPN profiles are installed, reconnecting often
selects the same old profile even after your network location has changed.
`vpn-geo` watches for a real VPN disconnect, waits for the route to settle,
looks up the public-IP country, and raises the matching configured profile's
autoconnect priority for the next connection.

It does not connect, disconnect, or switch a VPN. It changes only
`connection.autoconnect-priority` for UUIDs explicitly listed in the config.

## Setup

```bash
nmcli -f NAME,UUID,TYPE connection show
mkdir -p ~/.config/vpn-geo
cp examples/config.toml ~/.config/vpn-geo/config.toml
$EDITOR ~/.config/vpn-geo/config.toml
vpn-geo check
systemctl --user enable --now vpn-geo.service
```

The configuration maps each NetworkManager profile UUID to a two-letter country
code such as `JP`, `KR`, or `US`. The daemon is event-driven while idle: it does
not poll NetworkManager, run `nmcli`, or make network requests until a real VPN
disconnect is observed.

## Commands

```bash
vpn-geo                  # run the background watcher
vpn-geo check            # validate configuration
vpn-geo speed            # benchmark nearby configured nodes (read-only)
vpn-geo speed apply      # benchmark and promote the fastest node
```

See the Chinese guide for installation variants, configuration reference,
behavior details, privacy notes, troubleshooting, and resource limits.
