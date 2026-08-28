# Maintainer: zhangyucheng1014-dev <zhangyucheng1014-dev@users.noreply.github.com>
pkgname=vpn-geo
pkgver=1.0.0
pkgrel=1
pkgdesc='Move NetworkManager VPN profiles to the preferred country after VPN disconnects'
arch=('x86_64' 'aarch64' 'armv7h')
url='https://github.com/zhangyucheng1014-dev/vpn-geo'
license=('MIT')
depends=('networkmanager' 'ca-certificates')
makedepends=('go')
source=("${pkgname}-${pkgver}.tar.gz::${url}/archive/refs/tags/v${pkgver}.tar.gz")
sha256sums=('5a8a0f6100c6b6bc21ebf4afdc6647b67059879c4bf1b7d6522f2da90b010515')

build() {
  cd "${pkgname}-${pkgver}"
  export CGO_ENABLED=0
  go build -trimpath -ldflags="-s -w" -o "${pkgname}" ./cmd/${pkgname}
}

check() {
  cd "${pkgname}-${pkgver}"
  go test ./...
}

package() {
  cd "${pkgname}-${pkgver}"
  install -Dm755 "${pkgname}" "${pkgdir}/usr/bin/${pkgname}"
  install -Dm644 packaging/${pkgname}.service "${pkgdir}/usr/lib/systemd/user/${pkgname}.service"
  install -Dm644 examples/config.toml "${pkgdir}/usr/share/doc/${pkgname}/config.toml"
  install -Dm644 README.md "${pkgdir}/usr/share/doc/${pkgname}/README.md"
  install -Dm644 LICENSE "${pkgdir}/usr/share/licenses/${pkgname}/LICENSE"
  install -Dm644 README.zh-CN.md "${pkgdir}/usr/share/doc/${pkgname}/README.zh-CN.md"
}
