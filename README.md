# vpn-geo 4.0：四层六边形架构全量重写

[![CI](https://github.com/zhangyucheng1014-dev/vpn-geo/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/zhangyucheng1014-dev/vpn-geo/actions/workflows/ci.yml) [![Release](https://img.shields.io/github/v/release/zhangyucheng1014-dev/vpn-geo?display_name=tag)](https://github.com/zhangyucheng1014-dev/vpn-geo/releases) [![License](https://img.shields.io/github/license/zhangyucheng1014-dev/vpn-geo)](LICENSE) [![Rust](https://img.shields.io/badge/Rust-1.85%2B-dea584?logo=rust&logoColor=white)](Cargo.toml)

**中文** | [English](README.en.md)

`vpn-geo` 已再次全量重写为四层 Cargo workspace：`domain` 是零 I/O 的纯规则层，`app` 是用例与端口，`infra` 是外部适配器，`cli` 只负责解析参数和渲染输出。所有系统交互都通过 `async_trait` 端口注入，业务逻辑可独立测试，并且保留了 1.x/2.x/3.x 配置与命令兼容性。

原有能力全部保留：事件驱动 NetworkManager VPN 断开处理、稳定路由等待、多 GeoIP 源共识、国家节点优先级、状态锁、区域镜像目录、官方源动态同步、镜像评分、pacman 安全切换、VPN/代理/直连诊断与 JSON 输出。守护进程不会主动连接、断开或切换 VPN；除 `mirror apply`/`mirror best --yes` 的 mirrorlist 写入外，只修改配置中明确列出的 UUID 的 `connection.autoconnect-priority`。

## 四层架构

- `crates/vpn-geo-domain`：配置解析、目录数据、GeoIP 共识、测速评分、网络状态判定和断开状态机；不依赖 Tokio、D-Bus 或 HTTP。
- `crates/vpn-geo-app`：`ConfigSource`、`GeoIpGateway`、`NetworkGateway`、`PriorityGateway`、`SpeedGateway`、`MirrorGateway`、`StateStore`、`VpnEventStream` 端口，以及 `speed`、`route`、`doctor`、`mirror`、守护调度用例。
- `crates/vpn-geo-infra`：reqwest/rustls、zbus D-Bus、nmcli、flock 状态文件、官方镜像状态、pacman mirrorlist、配置仓库等适配器。
- `crates/vpn-geo-cli`：clap 命令树、表格/JSON 输出、日志和退出码。

这种拆法把「做什么」与「怎么做」分开：替换 HTTP 库、D-Bus 实现或 CLI 不需要改领域规则；替换守护策略也不必碰 NetworkManager 适配器。

## 命令

```bash
vpn-geo                              # 后台事件监听；daemon 为显式别名
vpn-geo check [--json]               # 校验配置并显示架构/版本摘要
vpn-geo status [--json] [--no-dbus]  # VPN / 系统代理 / 设备 / 线路状态
vpn-geo sources [--json]             # GeoIP 源、超时和最小共识数
vpn-geo doctor [--json] [--skip-network]
vpn-geo speed [apply] [--direct] [--json]
vpn-geo route [compare] [--direct] [--json]
vpn-geo mirror list [-r cn|hk|jp|us|global] [--json]
vpn-geo mirror speed [-r jp] [--direct] [--json]
vpn-geo mirror sync --regions cn,hk,jp,us --per-region 6 [--json] [--output FILE]
vpn-geo mirror best [-r cn] [--yes] [--json]
sudo vpn-geo mirror apply cn-tsinghua --yes
```

`mirror sync` 从 [Arch Linux 官方镜像状态](https://archlinux.org/mirrors/status/json/) 按 score/delay/completion 筛选 active HTTPS 源，并可以导出 `[[mirrors]]` TOML。`mirror best` 按吞吐/延迟综合评分选优；加 `--yes` 才写入 pacman mirrorlist。所有测速与诊断命令默认只读。

## 内置镜像目录

内置 29 个 Arch Linux HTTPS 源：中国大陆 8、香港 3、日本 7、美国 10、全球 1。目录在 `crates/vpn-geo-domain/src/catalog.rs` 中以数据行维护，可按地区筛选或用 `[[mirrors]]` 完全覆盖。

## 配置

1.x/2.x/3.x 配置可直接读取；推荐 4.x 显式指定 `version = 4`：

```toml
version = 4
settle_delay = "3s"
quiet_period = "500ms"

[geoip]
urls = ["https://ipwho.is/", "https://ipapi.co/json/", "https://ip-api.com/json/?fields=status,countryCode,lat,lon,message"]
timeout = "10s"
min_agreement = 2

[benchmark]
candidate_countries = 3
samples = 3
request_timeout = "8s"
download_bytes = 10485760
parallel = 2

[[nodes]]
name = "jp-tokyo-1"
uuid = "REPLACE-WITH-NETWORKMANAGER-PROFILE-UUID"
country = "JP"
latitude = 35.6762
longitude = 139.6503
test_url = "https://replace-with-your-test-host.example/test-10MiB.bin"

[[mirrors]]
name = "my-mirror"
kind = "pacman"
region = "cn"
url = "https://example.test/archlinux/$repo/os/$arch"
test_url = "https://example.test/archlinux/core/os/x86_64/core.db.tar.gz"
priority = 100
enabled = true
```

`min_agreement` 是 GeoIP 多源最低共识票数。状态文件只保存上次成功国家，不保存公网 IP。

## 构建与验证

```bash
cargo fmt --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo build --locked --workspace --release
```

Arch Linux 可使用 `PKGBUILD`：`makepkg -si`，随后 `systemctl --user enable --now vpn-geo.service`。

项目使用 [MIT License](LICENSE)。
