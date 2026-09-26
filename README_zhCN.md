<div align="center">
  <img src="addon/Media/Logo.png" width="88" alt="Lychee Dev" />
  <h1>Lychee Dev Toolkit</h1>
  <p><strong>从 WoW 源码、游戏数据，到可验证的游戏内结果。</strong></p>
  <p>为魔兽世界插件开发提供原生 CLI、游戏内工作台与 Agent skill。</p>

[![npm](https://img.shields.io/npm/v/lycheedev?color=ef6b78&label=npm)](https://www.npmjs.com/package/lycheedev)
[![CI](https://github.com/Follen/Lychee-Dev/actions/workflows/toolkit-ci.yml/badge.svg?branch=main)](https://github.com/Follen/Lychee-Dev/actions/workflows/toolkit-ci.yml)
[![Release](https://github.com/Follen/Lychee-Dev/actions/workflows/toolkit-release.yml/badge.svg)](https://github.com/Follen/Lychee-Dev/actions/workflows/toolkit-release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-52a788)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows_amd64-0078d4?logo=windows)](#环境要求)
[![Node](https://img.shields.io/badge/node-%E2%89%A522.14.0-417e38?logo=nodedotjs&logoColor=white)](#安装)
[![Runtime dependencies](https://img.shields.io/badge/npm_runtime_dependencies-0-52a788)](packages/npm/lycheedev/package.json)
[![Agent skill](https://img.shields.io/badge/Agent_skill-included-8563bd)](#给-agent)

[English](README.md) · **简体中文**

[给开发者](#给开发者) · [给 Agent](#给-agent) · [安装](#安装) · [参与开发](#参与开发) · [回归证据](docs/toolkit/business-regression-2026-09-26.md)
</div>

---

## 给开发者

查找暴雪 API 的定义和调用位置，查询法术与物品背后的数据，导出贴图，再到真实客户端调查插件行为。Lychee Dev 将这些工作放进同一个工作空间，保留固定源码提交、明确的游戏 Build 和原始证据。

**源码、数据与资源工作流不需要在线游戏，也不依赖游戏插件。** 需要游戏内调查时，再安装工作台。

### 能做什么

| 工作流 | 具体功能 | 典型问题 |
| --- | --- | --- |
| **源码研究** | 同步暴雪 UI 和受支持的插件仓库；索引 Lua/XML/TOC；精确或探索查询；查看符号和原文；比较固定版本 | 这个 API 在哪里使用？不同客户端改了什么？ |
| **插件验证** | 检查 TOC/XML 加载链、语法、源码名称引用；按明确配置验证多个客户端 | 是否漏了加载文件？Interface 声明是否匹配？ |
| **DB2 与 SQL** | 类型化 schema/记录、字段搜索、外键筛选、分页、JSONL 流、只读 SQL 和 CSV 导出 | 哪些记录符合这个条件？ |
| **游戏业务数据** | 法术引用、光环和召唤；物品模型、geoset 和纹理；生物 display/模型；副本遭遇；住宅装饰 | 这个 ID 关联了哪些数据和资源？ |
| **Hotfix** | 显式选择 Wago、本地 DBCache 或 Raidbots 输入；筛选、有界分页与结果归档 | 这个来源对指定 Build 和范围报告了什么变化？ |
| **资源工具** | 社区 listfile 搜索；本地 CASC/CDN 检查；原始文件导出；BLP2 转 PNG/无损 WebP；有界 VP9 AVI 解复用 | 找到这个图标，并导出真实像素。 |
| **游戏内工作台** | `/dev` 的 Run、诊断、对象检查、事件、函数追踪和导出工具 | 检查对象、观察事件，或调查插件错误。 |
| **游戏自动化** | 识别客户端、加载有界 Lua 探针、验证报告、ACK 并清除回执；恢复中断操作 | 在指定角色上完成检查并带回结果。 |
| **证据与项目** | 固定项目引用、归档 capture、校验与打包；明确的缓存预算和清理 | 能否复现这个结论、检查原始依据？ |

这些功能承接原来的 **wowdoc/wowdata 业务**，不会包装调用退役工具，也不会导入它们的旧工作空间。

### 环境要求

- **Windows 10/11 x64**。发行产品仅支持 Windows amd64。
- npm 分发需要 **Node.js ≥22.14.0**；CLI 本体是 Go 原生程序，也可从 [Releases](https://github.com/Follen/Lychee-Dev/releases) 获取原生 ZIP。
- 源码准备需要 **Git**；数据工作流按所选来源需要本地游戏文件或网络。
- 游戏内调查需要运行并已登录的客户端，以及已加载的插件。

| 客户端 | Interface 基线 | 验收范围 |
| --- | --- | --- |
| 正式服 Retail | `120100` | 支持矩阵 |
| Classic / 熊猫人 | `50504` | 支持矩阵 |
| Titan / 时光服 | `38002` | 支持矩阵 |
| Forever / 无限服 | `16001` | 保留实验路径；有有限实测记录，不属于完整验收矩阵 |

客户端身份来自安装元数据和 Build 证据，不能只看目录名。具体基线与验证边界见[实施状态](docs/toolkit/implementation-status.md)。

### 安装

PowerShell：

```powershell
npm install --global lycheedev --ignore-scripts
lycheedev version --format json
lycheedev describe --format json
```

npm 包包含 CLI、插件和 skill 资源；安装 npm 包**不会自动部署**游戏插件或 skill：

```powershell
$release = Join-Path (npm root -g) 'lycheedev'
$client = 'D:\Game\World of Warcraft\_retail_'  # 改成你的客户端目录
$skill = Join-Path $env:USERPROFILE '.agents\skills\lycheedev'

lycheedev addon install --release $release --installation $client
lycheedev skill install --release $release --path $skill
```

以上是首次安装命令。已有安装先用 `addon status` / `skill status` 检查；受管升级通过 `--output <恢复目录>` 保留旧文件，恢复目录需与目标同盘，且位于正在使用的 AddOns/skills 目录之外。被修改或未受管的安装需要明确处理，不能直接覆盖受管文件。

安装后在客户端加载插件，用 `/dev` 打开工作台。磁盘安装成功不代表运行中的游戏已经加载新版。

### 研究一个 API

```powershell
$source = lycheedev source sync --source wow-ui-source --product retail --ref refs/heads/live --format json | ConvertFrom-Json
$sourcePin = $source.result.id
lycheedev source index --snapshot $sourcePin --format json
lycheedev source query C_Spell.GetSpellInfo --snapshot $sourcePin --mode precise --topic api --limit 10 --format json
```

分支只解析一次，后续沿用返回的不可变 pin。研究历史版本时传入精确 commit 或完整 tag ref；用 `source inspect` 读取原始行，用 `source diff --from <pin-a> --to <pin-b>` 比较版本。

### 查询真实游戏数据

```powershell
# 按调查目标选择地区与语言。
$target = lycheedev target resolve --installation $client --region cn --locale zhCN --format json | ConvertFrom-Json
$dataPin = $target.result.id
lycheedev data db2 schema ChrClasses --snapshot $dataPin --installation $client --format json
lycheedev data sql --snapshot $dataPin --installation $client --sql 'SELECT ID, Name_lang FROM ChrClasses LIMIT 5' --format json
```

将 `--installation` 换成 `--cdn` 可显式选择远程数据。`--offline` 只复用已校验的缓存，缺失时失败，不会偷偷换成其他 Build。

### 从记录导出图标

```powershell
$classes = lycheedev data db2 --snapshot $dataPin --installation $client --table ChrClasses --limit 1 --format json | ConvertFrom-Json
$iconId = $classes.result.page.rows[0].IconFileDataID
lycheedev asset export --snapshot $dataPin --installation $client --file-id $iconId --encoding png --output '.\class-icon.png' --format json
```

`asset search` 按名称、文本或扩展名查候选文件 ID，`asset inspect` 校验选定的 CASC 内容。导出时明确选择 `raw`、`png` 或 `webp`，文件名后缀不会自动决定编码。

> **验证边界。** 部分表和资源含有 CLI 无法取得密钥的加密区段，读取受阻不等于空表。Hotfix 分页可能不完整，静态验证也不能证明运行时或污染安全。[业务回归报告](docs/toolkit/business-regression-2026-09-26.md) 分别记录真实输入、fixture 覆盖和受阻项目。

## 给 Agent

安装 [lycheedev skill](skills/lycheedev/SKILL.md) 后即可发现这些工作流，默认启用自动触发。skill 负责选择调查路线；原生 CLI 负责传输、持久化、完整性检查和恢复。

可以这样提问：

- “在固定的正式服源码中找到这个 API 的定义和调用位置。”
- “查询这个 Build 的法术记录，保留缺失字段，并导出结果。”
- “找到这张贴图，导出 PNG，并附上来源身份。”
- “在这个角色上执行有界探针，读完报告、ACK，再清掉二维码。”

### 发现、固定、执行、收口

1. 用 `lycheedev describe --format json` 查询当前已安装 CLI 的实际能力，只读取相关的 [skill 参考](skills/lycheedev/references/commands.md)。
2. 只解析任务所需的源码或数据身份，相关调用复用同一个 pin；不能将精确版本替换为 `latest`。
3. 游戏输入必须处于授权范围内，并绑定选定窗口与角色。中断后保留 operation ID。
4. 阅读结果、capture、warning 与完整性字段；完成已授权操作后再给最终答复。

### 二维码是中间状态，不是完成条件

```text
连接 → 注册探针 → 加载 → 运行 → 读取已验证报告 → ACK → 隐藏回执
                    │                          │
                    └──── 按 operation ID 恢复 ─┘
```

`live run` 在报告验证后、ACK 前返回。**Agent 必须继续执行**：保存报告与 capture，对同一 operation 调用 `live ack`，结束时调用 `live hide`。不要让用户扫码，也不要为正常收口反复索要确认；用户明确要求暂停或保留报告时例外。

| 证据 | 可以说明什么 |
| --- | --- |
| 二维码出现、命令已发送、探针已加载 | 仅完成了中间步骤，不是调查结果 |
| `report.state: verified` | 报告可用，但可能仍需收口 |
| `cleanup: complete` | 已 ACK，并释放该操作的窗口所有权 |
| `live hide` 成功且 `result.cleared: true` | 已观察到最终回执清除 |
| 合法 JSONL `end` 帧且退出码为 0 | 数据流按所报告的范围完成 |

遇到 pending 或不确定的输入结果，检查并恢复原 operation；不要新建替代探针、切换角色、盲目 reload 或自动 abandon。如果确实需要外部操作才能继续，明确报告阻塞原因、已验证结果和保留的 ID，作为**未完成交接**。详见 [live 编排](skills/lycheedev/references/live-investigation.md)。

CLI 结果采用 `lycheedev.result.v1`。退出码区分参数错误（`2`）、能力限制（`3`）、无效输入（`4`）、外部失败（`5`）、待恢复（`6`）、取消（`7`）和内部错误（`8`）；即使成功，也必须查看范围和完整性。

## 参与开发

```powershell
go build ./...
go vet ./...
$env:LYCHEEDEV_REQUIRE_LUA51 = '1'
go test -parallel=4 -count=1 ./...
node --test tools/*.test.mjs
node tools/version.mjs --check
node tools/skill-contract.mjs
```

Lua 测试需要 Lua 5.1；`node tests/tools/build-lua.mjs` 可构建固定版本的解释器。Windows CI 同时检查进程锁、安装与分发行为和数据竞争，不替代真实游戏验收。

| 模块 | 位置 |
| --- | --- |
| CLI 与模块 | [`cmd/lycheedev/`](cmd/lycheedev/) · [`internal/`](internal/) |
| 游戏工作台与桥接 | [`addon/`](addon/) |
| Agent 工作流 | [`skills/lycheedev/`](skills/lycheedev/) |
| npm 分发 | [`packages/npm/lycheedev/`](packages/npm/lycheedev/) |
| 合同与验证 | [设计](docs/toolkit/design.md) · [状态](docs/toolkit/implementation-status.md) · [回归矩阵](docs/toolkit/regression.md) |
| 发行 | [发布合同](docs/toolkit/release-2.0.3.md) · [GitHub Releases](https://github.com/Follen/Lychee-Dev/releases) |

## 许可

[MIT](LICENSE)。从作者 wowdata 项目移入的组件采用 **AGPL-3.0-or-later OR MIT** 双许可，wowdoc 来源部分采用 MIT。详见[第三方说明](THIRD_PARTY_NOTICES.md)。每次发行附带对应源码。
