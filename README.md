# vpn-geo：让 VPN 节点选择少一点手动操作

[![Release](https://img.shields.io/github/v/release/zhangyucheng1014-dev/vpn-geo?display_name=tag)](https://github.com/zhangyucheng1014-dev/vpn-geo/releases) [![License](https://img.shields.io/github/license/zhangyucheng1014-dev/vpn-geo)](LICENSE) [![Go](https://img.shields.io/badge/Go-1.24%2B-00ADD8?logo=go&logoColor=white)](go.mod)

**中文** | [English](README.en.md)

> 给经常换 VPN 节点的人：VPN 断开后，`vpn-geo` 会根据当前网络所在国家，帮你把对应节点排到更靠前的位置。你不用每次都手动找同一个节点。

## 先看这里：普通用户怎么用？

如果你已经在 NetworkManager 里配置好了 VPN，通常只需要三步：

```bash
# 1. 查看 VPN profile 的 UUID
nmcli -f NAME,UUID,TYPE connection show

# 2. 创建配置并填入你的 VPN UUID
mkdir -p ~/.config/vpn-geo
cp examples/config.toml ~/.config/vpn-geo/config.toml
$EDITOR ~/.config/vpn-geo/config.toml

# 3. 检查并启动
vpn-geo check
systemctl --user enable --now vpn-geo.service
```

之后你照常使用 VPN 即可。断开 VPN 时程序会在后台完成判断，下一次 NetworkManager 自动连接时会优先考虑匹配国家的节点。

你不需要学习 D-Bus、Go 或 NetworkManager 的内部 API，也不需要修改 VPN 密码、服务器地址或路由。只要把 profile UUID 和国家代码填对即可。

### 它会不会替我连接 VPN？

不会。`vpn-geo` 只调整节点的自动连接优先级，不会主动连接、断开或切换 VPN。最终是否连接、什么时候连接，仍由 NetworkManager 和你正在使用的桌面/VPN 工具决定。

### 一句话判断是否适合你

如果你有多个 VPN 节点，而且经常遇到“断开后又连回旧节点”或“每次都要手动挑节点”，这个工具就是为这个场景准备的。

## 目录

- [普通用户怎么用？](#先看这里普通用户怎么用)
- [适合什么场景？](#适合什么场景)
- [它解决什么问题？](#它解决什么问题)
- [安全边界](#安全边界)
- [资源占用](#资源占用)
- [5 分钟开始使用](#5-分钟开始使用)
- [实际工作规则](#实际工作规则)
- [命令说明](#命令说明)
- [常见问题](#常见问题)
- [隐私和网络请求](#隐私和网络请求)

## 适合什么场景？

- 同时保存了多个国家或多个线路的 VPN profile。
- 经常手动断开、重连，或在不同 VPN 服务之间切换。
- VPN 断开后网络出口会变化，但 NetworkManager 仍按旧的固定优先级选择节点。
- 希望保留 NetworkManager 原有的自动连接逻辑，只动态改变节点顺序。
- 希望后台程序平时几乎不占 CPU，不轮询网络，也不持续请求公网服务。

典型流程：

```text
VPN 断开 -> 等待路由和 DNS 稳定 -> 查询公网 IP 国家
       -> 提升对应国家节点优先级 -> 下一次自动连接时优先选择
```

## 它解决什么问题？

VPN 用户频繁更换节点时，常见问题不是“不会连接”，而是“每次连接都要重新挑节点”：断开当前 VPN、查看线路、重新找节点，下一次重连又被旧的自动连接优先级带回原节点。

`vpn-geo` 会把这部分重复操作变成后台规则。它保留你现有的 profile、服务器地址、端口、密钥、DNS、路由和认证方式，只调整配置中明确列出的节点顺序。

需要明确的是：它不会主动替你连接、断开或切换 VPN。它做的是“为下一次自动连接准备更合理的优先级”，不会和你的 VPN 客户端争抢控制权，也不会在连接过程中反复重连。

## 安全边界

- 只监听 NetworkManager 的活动连接变化。
- 只处理“最后一个活动 VPN：已连接 -> 已断开”这一条边。
- 启动时只建立当前状态基线，不会启动一次查询。
- VPN 正在连接时不会触发公网 IP 查询。
- 只修改配置文件中列出的 UUID 的 `connection.autoconnect-priority`。
- 不读取或修改 VPN 密码、私钥、服务器地址、端口、路由、DNS 和认证信息。
- 查询失败、超时、返回格式错误时，不修改任何节点，也不保存错误状态。
- 没有匹配国家节点时，保持现有优先级不变。
- 状态文件只保存上一次成功处理的两位国家代码，不保存公网 IP。

## 资源占用

后台空闲时，程序只阻塞等待一个系统 D-Bus 事件：不轮询 NetworkManager、不持续解析 DNS、不持续访问互联网、不运行 `nmcli`，也不保留长期空闲的 HTTP 连接。连续的 NetworkManager 信号会合并，避免一次网络切换产生大量重复处理。

打包提供的 systemd user service 还设置了较低的 CPU/IO 优先级、64 MiB 内存软上限、128 MiB 内存硬上限和 32 个任务上限。测速命令是手动执行的临时任务，不受后台服务的资源限制。

## 5 分钟开始使用

### 1. 确认 NetworkManager profile

```bash
nmcli -f NAME,UUID,TYPE connection show
```

记下需要自动选择的 VPN profile UUID。`vpn-geo` 不会创建 profile，也不会验证 VPN 服务商账号是否可用。

### 2. 安装程序

从 [GitHub Releases](https://github.com/zhangyucheng1014-dev/vpn-geo/releases) 下载与 `uname -m` 对应的压缩包，先校验再安装：

```bash
sha256sum -c vpn-geo_VERSION_linux_amd64.tar.gz.sha256
tar -xzf vpn-geo_VERSION_linux_amd64.tar.gz
cd vpn-geo_VERSION_linux_amd64
install -Dm755 vpn-geo ~/.local/bin/vpn-geo
install -Dm644 vpn-geo.service ~/.config/systemd/user/vpn-geo.service
```

如果下载的压缩包同时提供了 `vpn-geo-user.service`，也可以使用它；源码目录中对应文件是 `packaging/vpn-geo-user.service`。两者都是 user service，区别只在于二进制安装位置：前者默认使用 `/usr/bin/vpn-geo`，后者默认使用 `~/.local/bin/vpn-geo`。

Arch Linux 也可以使用仓库中的 `PKGBUILD`：

```bash
makepkg -si
systemctl --user daemon-reload
systemctl --user enable --now vpn-geo.service
```

### 3. 创建配置文件

```bash
mkdir -p ~/.config/vpn-geo
cp examples/config.toml ~/.config/vpn-geo/config.toml
```

编辑配置，把示例 UUID 替换为真实值：

```toml
settle_delay = "3s"
quiet_period = "500ms"

[geoip]
url = "https://ipwho.is/"
timeout = "10s"

[[nodes]]
name = "jp-tokyo-1"
uuid = "替换为 NetworkManager 中的真实 UUID"
country = "JP"

[[nodes]]
name = "kr-seoul-1"
uuid = "替换为 NetworkManager 中的真实 UUID"
country = "KR"
```

`country` 使用 ISO 3166-1 alpha-2 两位国家代码，例如 `JP`、`KR`、`US`。同一个国家可以配置多个节点；自动处理会选择该国家配置中的第一个节点，手动测速可以比较同一国家的多个节点。

检查配置：

```bash
vpn-geo check
```

`check` 会在访问 NetworkManager 之前检查未知配置项、重复 UUID、国家代码、URL、坐标和测速参数。发现拼写错误时会直接失败，不会启动后台服务。

### 4. 启动后台服务

```bash
systemctl --user enable --now vpn-geo.service
systemctl --user status vpn-geo.service
journalctl --user -u vpn-geo.service -f
```

## 实际工作规则

后台服务只在“至少一个 VPN 已连接 -> 所有 VPN 都已断开”时工作。程序启动时若本来就是断开状态、普通网络变化、VPN 从断开变为连接、VPN 正在连接或重复状态上报，都不会触发处理。

断开后默认等待 3 秒，并要求网络事件安静 500 毫秒，避免读到旧路由或旧 DNS。查询失败不会自动重试，避免网络异常时不断唤醒和产生额外流量。

匹配到国家节点时，该节点会获得高于其他已管理节点的优先级，其他节点之间的相对顺序保持不变。上次成功处理的国家保存在 XDG state 目录中，同一国家再次出现时不会重复写入。

## 命令说明

```bash
vpn-geo                         # 启动后台监听
vpn-geo check                   # 校验配置，不访问 NetworkManager
vpn-geo speed                   # 测试最近国家的节点，只显示结果
vpn-geo speed apply             # 测试并提升测速最快的节点
vpn-geo speed --apply           # 与上一条等价
vpn-geo -c /path/to/config.toml check
vpn-geo -v                      # 打开调试日志
```

`speed` 是手动测速命令，不会自动激活任何 VPN profile。它使用当前网络出口访问节点的 `test_url`，测到的是当前线路的可达性、延迟和吞吐量，不是“逐个连接 VPN 后的真实隧道速度”。不加 `apply` 时完全只读。

节点要参加测速，需要配置：

```toml
latitude = 35.6762
longitude = 139.6503
test_url = "https://example.test/test-10MiB.bin"
```

## 常见问题

### 为什么断开 VPN 后没有马上看到连接变化？

这是预期行为。程序会先等待路由和 DNS 稳定，再查询公网 IP，默认至少等待 3 秒。如果网络一直处于变化状态，最多等待约 10 秒后放弃本次处理。

### 它会不会自动连接 VPN？

不会。它只调整自动连接优先级。是否连接、何时连接、使用哪个 profile，仍由 NetworkManager 和你的桌面/VPN 工具决定。

### 我配置了节点，但自动连接还是没有使用它？

请确认 UUID 是 NetworkManager profile 的 UUID、profile 的 `connection.autoconnect` 没有关闭、其他 profile 没有更高优先级或特殊连接条件，并检查 `journalctl --user -u vpn-geo.service -f` 和 `vpn-geo check` 的输出。

### 我只想手动测速，会修改 VPN 配置吗？

不带 `apply` 的 `vpn-geo speed` 完全只读；只有显式使用 `apply` 才会调整一个已配置节点的自动连接优先级。

## 隐私和网络请求

自动处理只在真实 VPN 断开后请求配置的 GeoIP 地址。请求内容不包含 VPN 密钥和 profile 详情；服务端通常会看到发起请求的公网 IP，这是公网 IP 定位服务本身的工作方式。你可以在 `[geoip]` 中配置兼容相同 JSON 返回字段的自有服务。

## 配置路径与开发检查

- 配置：`$XDG_CONFIG_HOME/vpn-geo/config.toml`，未设置时为 `~/.config/vpn-geo/config.toml`
- 状态：`$XDG_STATE_HOME/vpn-geo/`，未设置时为 `~/.local/state/vpn-geo/`
- 服务：`~/.config/systemd/user/vpn-geo.service` 或 Arch 软件包提供的 `/usr/lib/systemd/user/vpn-geo.service`

```bash
go test ./...
go vet ./...
go test -race ./...
go build ./cmd/vpn-geo
```

项目使用 MIT License。
