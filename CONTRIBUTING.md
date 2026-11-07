# Contributing to vpn-geo

The project is a Rust workspace with `vpn-geo-core` and `vpn-geo-cli`. Development requires Rust 1.85 or newer and, for daemon integration testing, Linux with NetworkManager.

Run the same checks used by CI:

```bash
cargo fmt --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo build --locked --workspace --release
```

Keep changes focused and explain user-visible behavior. Changes to configuration validation, NetworkManager behavior, priority handling, GeoIP parsing, mirrorlist edits, or benchmark accounting need focused tests. Never commit VPN credentials, private keys, profile exports, or personal network data.

Use short, imperative commit subjects, for example `fix: ignore inactive VPN profiles`.
