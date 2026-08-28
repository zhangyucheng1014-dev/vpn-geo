# Security Policy

## Scope

`vpn-geo` only changes `connection.autoconnect-priority` for NetworkManager
profiles explicitly listed in the local configuration. It does not read or
modify VPN passwords, private keys, server addresses, routes, or DNS settings.

## Reporting a vulnerability

Please do not open a public issue for a security-sensitive report. Use
GitHub's private security advisory feature for this repository when available.
If that feature is unavailable, contact the maintainer through the email
address published in `PKGBUILD` (`zhangyucheng1014-dev@users.noreply.github.com`)
and include:

- a clear description of the impact;
- reproduction steps or a minimal proof of concept;
- the affected version and environment; and
- any suggested mitigation.

Allow reasonable time for a fix before publicly disclosing the issue. Do not
include real VPN credentials, private keys, or public-IP history in a report.
