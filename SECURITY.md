# Security Policy

`vpn-geo` only changes `connection.autoconnect-priority` for NetworkManager profiles explicitly listed in the local configuration during normal daemon operation. It never reads or modifies VPN passwords, private keys, server addresses, routes, or DNS settings.

`speed apply` promotes one configured node. `mirror apply NAME --yes` and `mirror best --yes` are the only commands that write outside the state directory: they edit a clearly marked block in `/etc/pacman.d/mirrorlist`, create a backup, and refresh pacman databases. They require root, an explicit confirmation, and a configured `kind = "pacman"` source. `mirror sync --output FILE` writes a generated TOML fragment only to the requested path. All other benchmarks, diagnostics, and status commands are read-only with respect to system configuration.

GeoIP services see the requester's public IP. The state file stores only a country code, never the public IP. Responses are limited to 1 MiB and must provide a valid two-letter country code and coordinates.

Please do not open a public issue for a security-sensitive report. Use GitHub's private security advisory feature for this repository when available, or contact the maintainer through the address published in `PKGBUILD`. Include impact, reproduction steps, affected version/environment, and any suggested mitigation. Do not include real credentials, keys, or public-IP history.
