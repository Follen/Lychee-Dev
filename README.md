# Lychee Dev Toolkit

<img src="addon/Media/Logo.png" width="80" alt="Lychee Dev" />

魔兽世界插件开发工具：一个 Go CLI、一套游戏内 Lua 工作台，以及配套 Agent Skill。用于查源码、读游戏数据、导出资源、作者发布和备份，以及验证真实客户端行为。

[设计](Design.md) · [Live 协议](docs/live-protocol.md) · [开发指南](docs/development.md) · [更新记录](Changelog.md) · [待办与验证范围](todo.md)

**当前发行版本为 3.2.9，npm 发行渠道为 `latest`。** 本版本变化见[更新记录](Changelog.md)。版本以 [release/version.json](release/version.json) 为准，发布产物见 [GitHub Releases](https://github.com/Follen/Lychee-Dev/releases)。

## 功能

| 入口 | 用途 |
| --- | --- |
| Source | 固定 Git 提交，搜索 Lua/XML/TOC，查定义、引用、上下文和数据流；支持批量请求 |
| Data | 从本地 CASC 或远端 CDN 读取固定构建的 DB2，执行只读 SQL，查关联记录 |
| Hotfix / Assets | 按固定来源查 Hotfix、搜索资源和导出支持的文件格式 |
| Publish | CurseForge、NewBee、网易 DD、小黑盒和 ModUs 的作者发布、选择性备份、读回与原请求恢复 |
| Live | 向选定客户端执行有预算的 Lua 探针，读取报告，完成释放、重载和中断恢复 |
| 游戏内工作台 | 输入 `/dev` 使用 Run、Diagnostics、Inspector、Events、Function Trace |
| Agent Skill | 按任务选择工具，保留来源、连接和请求，报告实际验证范围 |

Source、Data 和 Assets 不需要启动游戏。Live 需要 Windows amd64、已进入角色的客户端及配套受管插件。

## 支持的产品

| 产品 | CLI product | 常见客户端目录 |
| --- | --- | --- |
| 正式服 | `retail` | `_retail_` |
| 怀旧服 | `classic` | `_classic_` |
| 经典旧世 | `classic-era` | `_classic_era_` |
| 时光服 | `titan` | `_classic_titan_` |
| 无限服 | `forever` | `_forever_` |
| 周年服 | `anniversary` | `_anniversary_` |

目录用于发现安装，产品身份取自客户端元数据。正式无限服使用 `forever/wowf`；旧无限测试服退出新目标选择，历史 PIN、数据和原连接保留原身份。完整映射见 [设计](Design.md)。

历史七产品记录包含两个无限测试服的 Source、数据抽样和 500 槽实测；Retail 另有同安装双实例测试。正式无限服及其他待验场景按当前产品单列，实际通过范围见 [todo.md](todo.md)。

## 使用当前仓库

开发环境：Windows amd64、Git、Go 1.27.1、Node.js 22.14.0 或以上。

源码构建还需要固定GCC14.2；先按[开发环境准备](docs/development.md#环境与运行)设置CGO、CC和CXX，再执行：

```powershell
go build -o .tmp/lycheedev.exe ./cmd/lycheedev
./.tmp/lycheedev.exe version --format json
./.tmp/lycheedev.exe describe --format json
```

当前仓库通过 `discover` 按问题查找能力，`schema` 查看具体输入，`describe` 查看完整目录，具体命令也可使用 `--help`。开发版插件需通过配套 stage 和受管安装部署，步骤见 [开发指南](docs/development.md)。

## 使用 npm 发行包

```powershell
npm install --global lycheedev@latest --ignore-scripts
lycheedev version --format json
lycheedev describe --format json
```

包内包含原生 CLI、addon、Skill、LuaLS 工具资源和发行清单。安装 npm 包后，按目标路径部署组件：

```powershell
$release = Join-Path (npm root -g) 'lycheedev'
lycheedev addon install --release $release --installation 'D:/Game/World of Warcraft/_retail_'
lycheedev skill install --release $release --path (Join-Path $env:USERPROFILE '.agents/skills/lycheedev')
```

仓库文档可能领先于 npm 包，以已安装版本的 `version`、`describe` 和命令帮助为准。调查规则见 [LycheeDev Skill](skills/lycheedev/SKILL.md)。

## Live

协议名称为 **Lycheedev mailbox protocol v1**。

CLI 在运行时通过指令特征和 RIP 相对位移找到 Lua 根，沿 Lua state → globals → Mailbox 读取。500 个 LoD 槽承载指令，正式快捷键触发执行与重载：

| 动作 | 默认绑定 |
| --- | --- |
| 执行 CLI 指令 | Ctrl+Alt+F12 |
| 重载插件 | Ctrl+Alt+F11 |

实际键位由新鲜的 Mailbox INPUT 公布。普通编辑框选择性透传这两个动作，保留文字和焦点；CLI 发送输入前等待玩家松开相关实体按键。

每个客户端进程有独立连接，共享安装通过槽预约协调。中断沿原连接和请求恢复；报告核实与资源清理分别记录。已提交但结果未知的输入不会重放。

首次连接或异常先运行 `doctor`；需要修复时用 `doctor --fix`，归档并初始化500槽用 `doctor --fix --hard-reset`。首次安装尚无可验证 INPUT 时，需要在指定客户端手动重载一次。详细规则见 [Live 协议](docs/live-protocol.md)。

## 开发与许可

Agent 从 [lycheedev Skill](skills/lycheedev/SKILL.md) 开始。代码结构和合同见 [Design.md](Design.md)，测试与发布见 [开发指南](docs/development.md)。

项目代码采用 [MIT](LICENSE)。第三方组件、游戏元数据和素材说明见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
