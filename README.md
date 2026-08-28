# vpn-geo

`vpn-geo` is a NetworkManager user service for Arch Linux. It watches the
system D-Bus and, only when the last active VPN changes from connected to
disconnected, waits for the normal network route to settle, determines the
current public-IP country, and promotes the matching configured VPN profile.

The program never initiates, disconnects, or switches a VPN. It does not alter
VPN addresses, ports, secrets, routes, DNS, or authentication. The only
NetworkManager property it can write is `connection.autoconnect-priority` for a
profile explicitly listed in the configuration.

## Automatic behaviour

- Startup establishes a baseline and never causes a lookup.
- Only `connected -> disconnected` triggers processing.
- `disconnected -> connected` cancels pending work and does not trigger lookup.
- After disconnect, a configurable delay and quiet window avoid stale routes or DNS.
- GeoIP errors, timeouts, and malformed responses are logged without changes or retries.
- A matching node is assigned a priority above all other managed nodes; their relative order remains unchanged.
- The last successfully handled country is stored atomically under the XDG state directory.
- A non-blocking lock makes duplicate events and instances harmless.
- A country without a configured node changes neither priority nor saved state.

### Idle resource usage

While idle, the daemon blocks on one system D-Bus signal channel. It does not
poll NetworkManager, resolve DNS, call the Internet, or run `nmcli`. Signals are
limited to active-connection changes and coalesced for 100 ms during a network
transition, so a burst of NetworkManager updates causes only one state read.
The GeoIP client disables idle HTTP keep-alives because lookups are rare.

## Quick usage

```bash
vpn-geo                  # run the background watcher
vpn-geo check            # validate the configuration
vpn-geo speed            # show nearest-country benchmark results
vpn-geo speed apply      # benchmark and promote the fastest node
```

Automatic country-priority processing has no manual trigger; a real VPN
disconnect is the only trigger. `speed` is always manual and does not activate
VPN profiles. Without `apply`, it is read-only.

Use `-c PATH` to select another configuration and `-v` for debug logs. The
long forms `--config`, `--verbose`, and `speed --apply` remain supported.
Global options go before the command (`vpn-geo -v speed`); `speed` options go
after it (`vpn-geo speed --apply`). `check` rejects unknown positional
arguments and reports malformed configuration before any NetworkManager access.

## Installation

For local development:

```bash
go build -o vpn-geo ./cmd/vpn-geo
./vpn-geo -c examples/config.toml check
```

### Release binaries

The [GitHub Releases](https://github.com/zhangyucheng1014-dev/vpn-geo/releases)
page provides archives for Linux `amd64` (x86_64), `arm64` (aarch64), and
`armv7`. Download the archive matching `uname -m`, verify its adjacent
`.sha256` file, then install the binary and user-service unit:

```bash
tar -xzf vpn-geo_VERSION_linux_amd64.tar.gz
cd vpn-geo_VERSION_linux_amd64
sha256sum -c ../vpn-geo_VERSION_linux_amd64.tar.gz.sha256
install -Dm755 vpn-geo ~/.local/bin/vpn-geo
install -Dm644 vpn-geo.service ~/.config/systemd/user/vpn-geo.service
```

### Arch Linux with pacman

`pacman` installs an already-built package; it does not build `PKGBUILD`
files. Build the included package with `makepkg`, which calls `pacman` to
install missing dependencies, then use `pacman -U` for the generated package:

```bash
makepkg -s
sudo pacman -U vpn-geo-1.0.0-1-x86_64.pkg.tar.zst
systemctl --user daemon-reload
systemctl --user enable --now vpn-geo.service
```

For a local source build/install in one command, `makepkg -si` is equivalent
to building followed by the local package install. Before publishing this to
the AUR, replace `sha256sums=('SKIP')` with the checksum for the tagged source
archive and regenerate `.SRCINFO` with `makepkg --printsrcinfo > .SRCINFO`.

The packaged user service is deliberately lightweight while idle: it runs at
lower CPU/IO priority, caps memory at 128 MiB, caps tasks at 32, and configures
the Go runtime to return unused pages more aggressively. These limits affect
the service only; the manual `speed` command remains outside them.

## Configuration

```bash
mkdir -p ~/.config/vpn-geo
cp examples/config.toml ~/.config/vpn-geo/config.toml
nmcli -g UUID,TYPE connection show
vpn-geo check
```

Replace example UUIDs with the UUIDs of your NetworkManager-managed VPN
profiles. `country` uses an ISO 3166-1 alpha-2 code such as `JP`, `KR`, or `US`.
Configuration keys remain English because they are part of the TOML interface.
Unknown TOML keys are rejected to catch typos early. GeoIP and benchmark URLs
must be absolute `http://` or `https://` URLs; coordinates are validated when
provided. Benchmark limits are bounded to avoid accidental excessive network
usage.

Default paths:

- Configuration: `$XDG_CONFIG_HOME/vpn-geo/config.toml`, or `~/.config/vpn-geo/config.toml`.
- State: `$XDG_STATE_HOME/vpn-geo/state.json`, or `~/.local/state/vpn-geo/state.json`.
- User service: `vpn-geo.service`.

View logs with:

```bash
journalctl --user -u vpn-geo.service -f
```

## `speed` benchmark

The command reads the public-IP location, chooses the nearest configured node
for each nearby country, and downloads a bounded amount from each node's
`test_url`. It reports latency and estimated throughput over the current
network route. It does not measure a real VPN tunnel by connecting profiles.

Nodes need these optional fields to participate:

```toml
latitude = 35.6762
longitude = 139.6503
test_url = "https://example.test/test-10MiB.bin"
```

## Development

```bash
go test ./...
go vet ./...
go build ./cmd/vpn-geo
```

The project is released under the MIT License. The default public-IP provider
is `https://ipwho.is/`; a compatible endpoint can be configured.
