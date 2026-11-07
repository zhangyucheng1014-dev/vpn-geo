# Maintainer: zhangyucheng1014-dev <zhangyucheng1014-dev@users.noreply.github.com>
pkgname=vpn-geo
pkgver=2.0.0
pkgrel=1
pkgdesc='Event-driven NetworkManager VPN prioritizer, route diagnostics and mirror manager'
arch=('x86_64' 'aarch64' 'armv7h')
url='https://github.com/zhangyucheng1014-dev/vpn-geo'
license=('MIT')
depends=('networkmanager' 'ca-certificates')
makedepends=('rust' 'cargo')
source=("${pkgname}-${pkgver}.tar.gz::${url}/archive/refs/tags/v${pkgver}.tar.gz")
sha256sums=('SKIP')

build() {
  cd "${pkgname}-${pkgver}"
  export CARGO_TARGET_DIR=target
  cargo build --frozen --release --target-dir target
}

check() {
  cd "${pkgname}-${pkgver}"
  cargo test --frozen --all-targets
}

package() {
  cd "${pkgname}-${pkgver}"
  install -Dm755 target/release/vpn-geo "${pkgdir}/usr/bin/vpn-geo"
  install -Dm644 packaging/vpn-geo.service "${pkgdir}/usr/lib/systemd/user/vpn-geo.service"
  install -Dm644 examples/config.toml "${pkgdir}/usr/share/doc/vpn-geo/config.toml"
  install -Dm644 README.md "${pkgdir}/usr/share/doc/vpn-geo/README.md"
  install -Dm644 README.en.md "${pkgdir}/usr/share/doc/vpn-geo/README.en.md"
  install -Dm644 LICENSE "${pkgdir}/usr/share/licenses/vpn-geo/LICENSE"
}
