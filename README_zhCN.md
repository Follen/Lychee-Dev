# Lychee Dev Toolkit

面向魔兽世界插件工程的统一原生工具箱：一个 Go CLI（`lycheedev`）、一个游戏内
Lua 工作台插件、一个 agent skill。源码研究、游戏数据查询、资源导出和运行中
客户端调查共享同一工作空间、同一证据链和可复现的固定引用。

| 组成 | 位置 | 说明 |
| --- | --- | --- |
| CLI | `cmd/lycheedev`、`internal/` | 原生 Go 二进制：源码、数据、资源、游戏运行、证据与安装命令 |
| 插件 | `addon/` | 游戏内 `/dev` 工作台与 CLI 驱动的协议桥 |
| Skill | `skills/lycheedev/` | agent skill：工作流路由与证据纪律 |
| npm 包 | `packages/npm/lycheedev/` | 发行载体：各平台二进制 + 插件/skill 载荷，零运行时依赖 |

## 支持客户端（2.0 验收矩阵）

| 客户端 | Interface | 版本 |
| --- | --- | --- |
| 正式服（Midnight） | `120100` | `12.1.0` |
| 怀旧服（Mists） | `50504` | `5.5.4` |
| 巫妖王之怒怀旧服 | `38002` | `3.80.2` |

Forever（`16001`）代码路径保留在树中，但**未经验证，不随 2.0 验收**。

## 系统要求

**仅支持 Windows 10/11 x64。** 工具箱只发行并验收 Windows amd64；游戏自动化
需要已登录的桌面会话。

## 安装

```bash
npm install -g lycheedev          # CLI + 插件 + skill 载荷
lycheedev addon install --release <发行根> --installation <客户端目录>
lycheedev skill install --release <发行根> --path <父目录>
```

`npm install --ignore-scripts` 可用；启动器不需要安装脚本，也没有运行时 npm 依赖。

## 快速开始

```bash
lycheedev init                                        # 新格式工作空间（~/.lycheedev）
lycheedev target resolve --installation <客户端> --region cn --locale zhCN
lycheedev source query C_Spell.GetSpellInfo --snapshot <pin>
lycheedev data db2 schema Map --snapshot <pin> --cdn
lycheedev live connect --snapshot <pin>               # 自动发现、识别并在游戏内启用连接；无需手输 /dev connect
lycheedev live run --session <id> --file probe.lua
lycheedev doctor
```

`lycheedev describe --format json` 是机器可读命令目录；
[skills/lycheedev/references/commands.md](skills/lycheedev/references/commands.md)
由它生成。所有结果都是 `lycheedev.result.v1` 信封并携带证据捕获；所有游戏操作
都可以凭操作 ID 恢复。

## 开发

```bash
go build ./... && go vet ./...
LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...   # 全量矩阵；Windows 需要 Lua 5.1.5
node tools/version.mjs --check                     # 版本源一致性
node tools/skill-contract.mjs                      # skill 与命令面一致性
node tools/release.mjs assemble --out <目录> --npm-cli <npm-cli.js> --cgo zero
```

Windows CI 是发布门槛：五个必需 job（`windows-contract`、`windows-process`、
`windows-addon`、`windows-package`、`ci-required`）。
发布合同见 [docs/toolkit/release-2.0.1.md](docs/toolkit/release-2.0.1.md)，
验收矩阵见 [docs/toolkit/regression.md](docs/toolkit/regression.md)。

## 设计与状态

- [docs/toolkit/design.md](docs/toolkit/design.md) — 架构与契约
- [docs/toolkit/implementation-status.md](docs/toolkit/implementation-status.md) — 已验证事实与边界
- [docs/toolkit/capability-inventory.md](docs/toolkit/capability-inventory.md) — 旧能力映射

## 许可

MIT — 见 [LICENSE](LICENSE)。改编自同作者 wowdata 仓库的部分按
AGPL-3.0-or-later OR MIT 双许可；wowdoc 派生部分为 MIT。完整第三方清单见
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。每个发行版都附带对应源码。
