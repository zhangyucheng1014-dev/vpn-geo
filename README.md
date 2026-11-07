# vpn-geo 2.0：Rust 重写的 VPN 节点优先级与网络诊断工具

[![CI](https://github.com/zhangyucheng1014-dev/vpn-geo/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/zhangyucheng1014-dev/vpn-geo/actions/workflows/ci.yml) [![Release](https://img.shields.io/github/v/release/zhangyucheng1014-dev/vpn-geo?display_name=tag)](https://github.com/zhangyucheng1014-dev/vpn-geo/releases) [![License](https://img.shields.io/github/license/zhangyucheng1014-dev/vpn-geo)](LICENSE) [![Rust](https://img.shields.io/badge/Rust-1.85%2B-dea584?logo=rust&logoColor=white)](Cargo.toml)

**中文** | [English](README.en.md)

`vpn-geo` 原来是 Go 1.24 项目，现已完整重写为 Rust（Tokio + `zbus` + `reqwest` + rustls）。旧版所有核心能力都保留：NetworkManager VPN 断开后按公网 IP 国家调整节点自动连接优先级、`check`、节点 `speed`/`speed apply`、状态文件、事件去抖和只改优先级的边界。

2.0 新增：多 GeoIP 源并发共识、更多 pacman/通用镜像源、镜像测速与安全切换、当前线路 / 直连基准测速、设备当前是 VPN / 系统代理 / 直连的状态检测，以及关键命令的 JSON 输出。

## 普通用户如何使用

```bash
nmcli -f NAME,UUID,TYPE connection show
mkdir -p ~/.config/vpn-geo
cp examples/config.toml ~/.config/vpn-geo/config.toml
$EDITOR ~/.config/vpn-geo/config.toml
vpn-geo check
systemctl --user enable --now vpn-geo.service
```

后台服务仍然是事件驱动：空闲时不轮询 NetworkManager、不运行 `nmcli`、不请求公网服务。只有「最后一个活动 VPN 从连接变为断开」才会等待路由稳定、查询公网 IP 国家并调整优先级。程序不会主动连接、断开或切换 VPN，也不会读写 VPN 密码、密钥、服务器地址、路由或 DNS。

## 命令

```bash
vpn-geo                         # 启动后台监听；daemon 是显式别名
vpn-geo check [--json]          # 校验配置
vpn-geo status [--json]         # 查看当前 VPN、系统代理、设备与线路判断
vpn-geo sources [--json]        # 列出 GeoIP 源
vpn-geo speed                   # 就近节点测速（只读）
vpn-geo speed apply             # 测速并提升最快节点的自动连接优先级
vpn-geo speed --direct          # GeoIP 查询绕过 HTTP 代理设置
vpn-geo route                   # 测当前线路延迟/吞吐，并显示线路状态
vpn-geo route compare           # 额外测一个 no-proxy 直连基准
vpn-geo mirror list             # 列出内置/自定义的 pacman 与通用源
vpn-geo mirror speed            # 镜像源测速（只读）
vpn-geo mirror speed --direct   # 绕过系统代理测镜像源
sudo vpn-geo mirror apply tsinghua --yes   # 应用 pacman 源（需要 root）
vpn-geo -c /path/to/config.toml check
vpn-geo -v status               # 调试日志
```

`speed`、`route`、`mirror speed`、`status`、`check` 支持 `--json`，方便脚本处理。旧版 `speed --apply` 和 `-c`、`-v` 均兼容。

## 新特性说明

### 多 GeoIP 源与更多源

默认并发查询 `ipwho.is`、`ipapi.co`、`ip-api.com` 三个兼容 JSON 格式的服务，采用多数票共识；单个源失败不会中断处理。可在 `[geoip]` 中用 `urls = [...]` 自定义最多 8 个源，也可以使用旧版 `url = "..."`。查询响应限制为 1 MiB，禁止保留空闲连接，超时和异常响应不会修改任何节点或状态。

`mirror` 内置清华、中科大、阿里云、RsProxy、Cloudflare 五个 Arch Linux 源，并支持配置任意数量的 `[[mirrors]]`：

```toml
[[mirrors]]
name = "my-mirror"
kind = "pacman"            # 或 generic
url = "https://example.test/archlinux/$repo/os/$arch"
test_url = "https://example.test/archlinux/core/os/x86_64/core.db.tar.gz"
priority = 100
enabled = true
```

`mirror apply` 只修改 `/etc/pacman.d/mirrorlist` 中明确标记的 `# BEGIN/END vpn-geo managed mirrors` 区块，会先创建带时间戳的备份，再执行 `pacman -Syy`。必须显式加 `--yes`；普通 `mirror speed` 完全只读。需要 root 权限，且只对 `kind = "pacman"` 的源生效。

### 走 VPN / 直连测速与设备状态

`status` 通过 NetworkManager 系统 D-Bus 读取活动连接与 VPN 标志，结合代理环境变量和 GNOME 代理设置，输出 `VPN`、`System proxy`、`Direct` 或组合判断。`route` 测当前线路的延迟和吞吐，`route compare` 额外用 `no_proxy()` 客户端测直连基准；它不会替用户切换 VPN。

注意：HTTP/SOCKS 代理客户端或第三方 tun 模式可能绕过 NetworkManager，因此状态检测带 `confidence` 字段；`Direct` 表示「未检测到 VPN 或系统代理」，不等同于内核路由表的绝对证明。测速得到的是当前出口到测试 URL 的结果，不是逐个连接 VPN 后的真实隧道速度。

## 性能与实现

- Rust 单静态逻辑二进制，release 使用 `opt-level = "z"`、LTO、单 codegen unit、strip 和 `panic = "abort"`。
- 使用 Tokio 单线程 runtime；空闲时只等待系统 D-Bus 信号，不保持 HTTP 空闲连接。
- `zbus` 原生 D-Bus，避免每次状态变化启动额外进程；Geolocation 多源并发、节点优先级读取并发、测速受 `parallel` 信号量限制。
- 请求体限制、下载字节上限、样本数/并发数上限和超时共同防止异常源拖垮后台服务。
- systemd user service 的内存上限从 128 MiB 收紧到 96 MiB，CPU/IO 权重保持低优先级。

## 配置与安装

- 配置：`$XDG_CONFIG_HOME/vpn-geo/config.toml` 或 `~/.config/vpn-geo/config.toml`
- 状态：`$XDG_STATE_HOME/vpn-geo/` 或 `~/.local/state/vpn-geo/`（只保存上次成功国家，不保存公网 IP）
- 服务：`~/.config/systemd/user/vpn-geo.service` 或软件包提供的 `/usr/lib/systemd/user/vpn-geo.service`

Arch Linux 可直接使用 `PKGBUILD`：

```bash
makepkg -si
systemctl --user daemon-reload
systemctl --user enable --now vpn-geo.service
```

源码构建：

```bash
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
```

## 兼容性

1.x 的 `settle_delay`、`quiet_period`、`[geoip] url/timeout`、`[benchmark]` 和 `[[nodes]] name/uuid/country/latitude/longitude/test_url` 全部保留；新增 `[geoip] urls`、`[[mirrors]]` 和新命令是可选能力。状态文件 schema v1 继续可读。未知配置键、重复 UUID/源名、非法国家代码、URL、坐标和测速参数仍会在 `check` 阶段直接失败。

项目使用 [MIT License](LICENSE)。
