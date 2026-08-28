# vpn-geo

`vpn-geo` 是一个面向 Arch Linux 和 NetworkManager 的 VPN 节点优先级管理器。
它监听系统 D-Bus，仅在最后一个活动 VPN 从“已连接”变为“已断开”时，等待网络稳定、查询当前公网 IP 所在国家，并把匹配国家的 VPN 节点提升到优先级第一位。

程序不会主动连接、断开或切换 VPN，也不会修改 VPN 地址、端口、密钥、路由、DNS 或认证信息。它唯一能够写入 NetworkManager 的属性是配置中明确列出的节点的 `connection.autoconnect-priority`。

## 自动工作逻辑

- 启动时只读取当前 VPN 状态建立基线，不会凭空触发查询。
- 只有“已连接 -> 已断开”会触发处理。
- “已断开 -> 已连接”不会触发，并会取消正在等待的处理。
- 断开后默认等待 3 秒，并要求网络事件安静 500 毫秒，避免读到旧路由或旧 DNS。
- 公网 IP 查询有严格超时；网络错误、API 错误或解析错误只记日志，不改配置、不重试。
- 命中节点会被设置为高于其他已管理节点的优先级，其他节点相对顺序保持不变。
- 上次成功处理的国家保存在 XDG 状态目录中。同一国家再次出现时不会产生写入。
- 重复事件使用非阻塞锁，重复处理不会造成状态或配置不一致。
- 没有对应国家的节点时，优先级和状态文件都不修改。

### 空闲功耗和内存

程序空闲时只阻塞等待一个系统 D-Bus 信号通道，不轮询 NetworkManager，不解析
DNS，不访问网络，也不运行 `nmcli`。监听范围限制为活动连接变化；网络切换期间
连续信号会合并（100 毫秒内只读取一次状态），避免频繁唤醒。由于公网 IP 查询
很少发生，HTTP 客户端会关闭空闲连接，避免长期保留连接资源。

## 最简使用方法

```bash
vpn-geo                  # 启动后台监听（systemd 服务通常会自动执行）
vpn-geo check            # 检查配置文件
vpn-geo speed            # 测试距离最近的国家节点，只查看结果
vpn-geo speed apply      # 测试并提升测速最快的节点
```

`check` 和 `speed` 是手动命令。自动根据国家调整优先级没有手动触发命令，唯一触发点是真实的 VPN 断开事件。

可选参数：

```bash
vpn-geo -c /path/to/config.toml check
vpn-geo -v
```

`-c` 指定配置文件，`-v` 打开调试日志。对应的长参数 `--config`、`--verbose` 和 `speed --apply` 仍然兼容。
全局参数放在子命令之前（例如 `vpn-geo -v speed`），`speed` 的参数放在子命令之后（例如 `vpn-geo speed --apply`）。`check` 会拒绝多余的位置参数，并在访问 NetworkManager 前报告配置错误。

## 安装

本地开发构建：

```bash
go build -o vpn-geo ./cmd/vpn-geo
./vpn-geo -c examples/config.toml check
```

### 从 Release 安装

[GitHub Releases](https://github.com/zhangyucheng1014-dev/vpn-geo/releases)
提供 Linux `amd64`（x86_64）、`arm64`（aarch64）和 `armv7` 三种架构的压缩包。
下载与 `uname -m` 对应的文件，先校验同名 `.sha256` 文件，再安装二进制和 user service：

```bash
sha256sum -c vpn-geo_VERSION_linux_amd64.tar.gz.sha256
tar -xzf vpn-geo_VERSION_linux_amd64.tar.gz
cd vpn-geo_VERSION_linux_amd64
install -Dm755 vpn-geo ~/.local/bin/vpn-geo
install -Dm644 vpn-geo.service ~/.config/systemd/user/vpn-geo.service
```

### Arch Linux：使用 pacman

`pacman` 用于安装已构建的软件包，不能直接构建 `PKGBUILD`。应先用 `makepkg`
构建；它会通过 `pacman` 安装缺失依赖。随后使用 `pacman -U` 安装生成的本地包：

```bash
makepkg -s
sudo pacman -U vpn-geo-1.0.0-1-x86_64.pkg.tar.zst
systemctl --user daemon-reload
systemctl --user enable --now vpn-geo.service
```

本地构建并立即安装可以使用 `makepkg -si`，相当于构建后安装本地包。未来提交
AUR 前，需要更新软件包版本和对应 tag 源码压缩包的校验值，并执行
`makepkg --printsrcinfo > .SRCINFO` 更新 `.SRCINFO`。

打包提供的 user service 在空闲时采用较低 CPU/IO 优先级，内存上限为 128 MiB、任务数上限为 32，并让 Go 运行时更积极地归还未使用的内存页。这些限制只作用于后台服务，手动执行的 `speed` 命令不受其限制。

`PKGBUILD` 中的 GitHub 地址、维护者和校验值需要在发布正式版本时按实际仓库更新。

## 配置

```bash
mkdir -p ~/.config/vpn-geo
cp examples/config.toml ~/.config/vpn-geo/config.toml
nmcli -g UUID,TYPE connection show
vpn-geo check
```

编辑 `~/.config/vpn-geo/config.toml`，把示例 UUID 替换成 NetworkManager 中实际的 VPN profile UUID，并填写节点国家代码（例如 `JP`、`KR`、`US`）。配置项名称保留英文，因为它们是 TOML 接口的一部分。
未知 TOML 配置项会被拒绝，以便尽早发现拼写错误。公网 IP 和测速地址必须是完整的 `http://` 或 `https://` URL；填写坐标时会校验经纬度范围。测速参数也有上限，避免误配置造成过大的网络流量。

默认路径：

- 配置：`$XDG_CONFIG_HOME/vpn-geo/config.toml`，未设置时为 `~/.config/vpn-geo/config.toml`
- 状态：`$XDG_STATE_HOME/vpn-geo/state.json`，未设置时为 `~/.local/state/vpn-geo/state.json`
- systemd user service：`vpn-geo.service`

查看服务日志：

```bash
journalctl --user -u vpn-geo.service -f
```

## `speed` 测速

`speed` 只手动运行。它读取公网 IP 的国家和经纬度，在每个最近国家中选择一个最近节点，然后从节点配置的 `test_url` 下载有限大小的数据，显示延迟和估算速度。

测速使用当前网络出口，不会自动激活任何 VPN profile。因此它是线路可达性测试，不是“逐个连接 VPN 后测真实隧道吞吐量”。只有输入 `vpn-geo speed apply`，才会把测速成功且速度最快的节点提升；不带 `apply` 时完全只读。

节点测速需要配置：

```toml
latitude = 35.6762
longitude = 139.6503
test_url = "https://example.test/test-10MiB.bin"
```

## 开发与检查

```bash
go test ./...
go vet ./...
go build ./cmd/vpn-geo
```

项目使用 MIT License。默认公网 IP 服务为 `https://ipwho.is/`，可在配置中替换为兼容相同 JSON 字段的服务。
