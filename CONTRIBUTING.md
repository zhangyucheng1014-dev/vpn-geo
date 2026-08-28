# Contributing to vpn-geo

Thanks for helping improve `vpn-geo`. Keep changes focused and explain the
user-visible behavior in the pull request description.

## Development setup

- Go 1.24 or newer
- Linux with NetworkManager for daemon integration testing

Run the same checks used by CI before opening a pull request:

```bash
go vet ./...
go test ./...
CGO_ENABLED=0 go build -trimpath ./cmd/vpn-geo
```

Use `gofmt` on changed Go files. Changes to configuration validation,
NetworkManager behavior, or priority handling should include focused tests.

## Pull requests

- Describe the problem and the intended behavior.
- Keep unrelated formatting or dependency changes out of the patch.
- Do not include VPN credentials, private keys, profile exports, or personal
  network data in commits or test fixtures.
- Update the relevant README or example configuration when behavior changes.

## Commit messages

Use a short, imperative subject, for example `fix: ignore inactive VPN
profiles`. Keep the subject under about 72 characters when practical.
