# vpn-geo 3.0：全量重构的 Rust Workspace

[![CI](https://github.com/zhangyucheng1014-dev/vpn-geo/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/zhangyucheng1014-dev/vpn-geo/actions/workflows/ci.yml) [![Release](https://img.shields.io/github/v/release/zhangyucheng1014-dev/vpn-geo?display_name=tag)](https://github.com/zhangyucheng1014-dev/vpn-geo/releases) [![License](https://img.shields.io/github/license/zhangyucheng1014-dev/vpn-geo)](LICENSE) [![Rust](https://img.shields.io/badge/Rust-1.85%2B-dea584?logo=rust&logoColor=white)](Cargo.toml)

**中文** | [English](README.en.md)

`vpn-geo` 已完成第三次全量重写：从单 crate Rust 程序重构为 `vpn-geo-core` + `vpn-geo-cli` 的 Cargo workspace。核心领域逻辑、配置模型、服务层和命令行输出彻底分离；错误处理改为结构化 `thiserror`，CLI 改用 `clap`，镜像目录改为数据驱动，并支持从 Arch Linux 官方镜像状态动态生成候选源。

原有能力全部保留：NetworkManager VPN 断开事件、路由稳定等待、多 GeoIP 源、国家节点优先级、状态锁、`check`/`speed`/`speed apply`/`status`/`route`/`mirror`/`sources`、区域镜像源、JSON 输出、pacman 安全切换。程序仍然不会主动连接、断开或切换 VPN，也只修改配置中明确列出的 UUID 的 `connection.autoconnect-priority`。

## 3.0 架构

- `crates/vpn-geo-core`：配置、错误、HTTP、GeoIP 共识、NetworkManager D-Bus、守护状态机、测速、镜像目录、pacman 写入。
- `crates/vpn-geo-cli`：clap 命令解析、表格/JSON 输出、退出码和脚本接口。
- `Cargo.toml`：workspace 统一版本、依赖与 release profile；`vpn-geo-core` 在发布构建中使用更高优化级别。
- 结构化错误区分配置错误（退出码 2）、未找到（3）和运行时错误（1）。
- 测速结果包含 `latency_ms`、`mbps` 和综合评分；`mirror best` 会按吞吐/延迟评分选择最佳源。

## 命令

```bash
vpn-geo                              # 后台事件监听；daemon 为显式别名
vpn-geo check [--json]               # 校验配置
vpn-geo status [--json] [--no-dbus]  # VPN / 系统代理 / 设备 / 线路状态
vpn-geo sources [--json]             # GeoIP 源、超时和最小共识数
vpn-geo doctor [--json] [--skip-network]  # 配置、D-Bus、GeoIP、源可达性综合检查
vpn-geo speed [apply] [--direct] [--json]
vpn-geo route [compare] [--direct] [--json]
vpn-geo mirror list [-r cn|hk|jp|us|global] [--json]
vpn-geo mirror speed [-r jp] [--direct] [--json]
vpn-geo mirror sync --regions cn,hk,jp,us --per-region 6 [--json] [--output FILE]
vpn-geo mirror best [-r cn] [--yes] [--json]
sudo vpn-geo mirror apply cn-tsinghua --yes
vpn-geo -c /path/to/config.toml check
vpn-geo -v doctor                    # 调试日志
```

`mirror sync` 从 [Arch Linux 官方镜像状态](https://archlinux.org/mirrors/status/json/) 读取 active HTTPS 源，按 score/delay/completion 过滤并可按每地区数量导出 TOML 片段；`mirror best --yes` 会测速后应用得分最高的 pacman 源。普通 `mirror list/speed/sync/best` 不修改系统配置；`apply`/`best --yes` 需要 root 和显式确认。

## 内置镜像目录

内置 29 个 Arch Linux HTTPS 源：中国大陆 8、香港 3、日本 7、美国 10、全球 1。目录现在是 `crates/vpn-geo-core/src/config.rs` 中的一行一条 CSV 数据，新增源不再需要重复 Rust 构造代码。可用 `-r cn|hk|jp|us|global` 筛选，或用 `[[mirrors]]` 完全覆盖。

## 配置

1.x/2.x 配置可直接读取；推荐 3.x 显式指定：

```toml
version = 3
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

`min_agreement` 控制多 GeoIP 源的最低共识票数，默认 2，且不会超过源数量。状态文件只保存上次成功国家，不保存公网 IP。

## 构建与验证

```bash
cargo fmt --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
cargo build --locked --workspace --release
```

Arch Linux 可使用 `PKGBUILD`：`makepkg -si`，随后 `systemctl --user enable --now vpn-geo.service`。

项目使用 [MIT License](LICENSE)。
