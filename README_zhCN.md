# Lychee Dev

[English](README.md)

[![CI](https://github.com/Follen/Lychee-Dev/actions/workflows/ci.yml/badge.svg?branch=rust-refactor)](https://github.com/Follen/Lychee-Dev/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/lycheedev)](https://www.npmjs.com/package/lycheedev)
[![platform](https://img.shields.io/badge/platform-Windows_amd64-0078d4)](https://github.com/Follen/Lychee-Dev)
[![runtime dependencies](https://img.shields.io/badge/npm_runtime_dependencies-0-52a788)](packages/npm/lycheedev/package.json)
[![license](https://img.shields.io/badge/license-MIT-52a788)](LICENSE)

Lychee Dev 是面向开发者与 Agent 的魔兽世界调查工具：Rust 原生 CLI、游戏内 Lua
工作台插件、第一方 Agent Skill 与 npm 包。它把魔兽世界问题变成固定源码证据、
类型化游戏数据、导出的资源或指定客户端上的受权观察，并保留可复查的结果。

发布平台为 **Windows amd64**。产品版本只以
[release/version.json](release/version.json) 为准。

## 能力

| 领域 | 能回答的问题 |
| --- | --- |
| 源码 | 固定暴雪 UI 或第三方插件版本：声明、调用者、上下文、加载闭包、验证、秘密值传播与版本差异。 |
| 数据 | 类型化 DB2 记录与元数据：catalog、字段、外键、筛选、Join、聚合、分页、导出与 Hotfix 记录；法术、物品、生物、遭遇和装饰物报告。 |
| 资源 | Listfile 搜索、CASC 文件检查、原始导出、BLP2 转 PNG/无损 WebP；本地安装或显式 CDN 来源。 |
| 实机 | 通过现有插件与 Lycheedev mailbox protocol v1，在单个已验证客户端上执行有界 Lua 探针、读取已验证报告、恢复并收尾。 |
| 证据 | 固定 pin 与项目锁、内容寻址 capture、校验、打包与有界缓存管理。 |
| 交付 | 从发布包受管安装、更新和移除插件与 Skill；npm CLI 更新交接。 |
| 作者发布 | Rust 平台适配器提供作者查询、插件/配置/WA 发布、媒体、关系和选择性云备份。 |

源码、数据与资源工作不需要运行游戏；实机工作需要一个已登录且加载受管插件的客户端。

当前支持 Retail、Classic、Classic Era、Titan、正式 Forever 和 Anniversary 六个产品。
正式 Forever 的产品 ID 为 `forever`，官方远端产品/flavor 为 `wowf`，客户端目录为
`_forever_`；两个 Forever Beta 不在当前支持范围。各目标的版本与 Interface 约束读取
已安装 CLI 的产品目录。

作者发布正在整合到 `lycheedev publish`。先用 `lycheedev cli search "publish"`
发现可用命令，再读对应 schema 中的输入、认证、效果、计划和恢复说明。当前已有
CurseForge、NewBeeBox Creator/官方通道、DD、小黑盒，以及身份分开的 ModUs 主客户端/Creator；包含各平台支持的作者业务和选择性备份。当前交付与真实平台验收见 [todo.md](todo.md)。
上传复用原产物；Skill 只引导调用 CLI，不另外维护字段表。

## 环境要求

- Windows amd64（`x86_64-pc-windows-msvc`），当前唯一交付目标。
- [Node.js](https://nodejs.org/) **22.14.0 或更高**，用于 npm 启动器与打包工具；
  CLI 本体是原生可执行文件，不依赖运行时 npm 包。
- Source 仓库准备需要 `PATH` 中有 Git。
- 数据工作需要已安装客户端或显式选择的 CDN 来源；部分内容加密或不可用，会如实报告。
- 实机工作需要有受支持的客户端、运行中的游戏进程和受管插件。代码支持不等于实机验收。

## 安装与部署

将示例路径换成准备安装的已核验发布包。使用 registry 时指定该发布的明确版本或标签；
不带选择的包名可能装到另一个版本。

```powershell
npm install --global 'C:\Downloads\lycheedev-X.Y.Z.tgz' # 所选版本的发布包
lycheedev version --format json
```

npm 包包含原生 CLI、插件与 Agent Skill。安装 npm 包**不会**自动部署插件或 Skill：

```powershell
$release = Join-Path (npm root -g) 'lycheedev'
$client  = 'D:\Game\World of Warcraft\_retail_'   # 你的客户端目录
$skill   = Join-Path $env:USERPROFILE '.agents\skills\lycheedev'

lycheedev addon install --release $release --installation $client
lycheedev skill install --release $release --path $skill
```

先检查目标，之后用受管更新器更新：

```powershell
lycheedev schema update
lycheedev update --plan --format json
lycheedev update
```

`update` 同步 npm CLI 与受管 Skill/插件。`--release <root>` 用已安装的发布包离线
应用；重复的 `--path` / `--installation` 显式限定目标。文件完成不等于运行中的游戏
或已有 Agent 上下文已激活：客户端仍需要 reload。

`schema update` 解释当前 CLI 内嵌发布清单的渠道策略；候选选择与校验使用同一来源，
Skill 不指定工具版本或 npm tag。中断恢复保留原包和原目标。

## 发现能力

CLI 面向 Agent 自解释。先按任务搜索，再只读取需要的合同：

```powershell
lycheedev cli search "spell cooldown" --format json
lycheedev schema target resolve
lycheedev data agent schema
lycheedev describe --format json
lycheedev <command> --help
```

发现只读命令元数据：不创建工作区、不打开缓存、不启动 worker、不联网、不连接游戏。
完整已安装命令列表由 `lycheedev describe` 提供；Data 的权威 Agent 任务与会话合同由
`lycheedev data agent schema` 提供。

## 示例

### 源码

```powershell
lycheedev init
$source = lycheedev source sync --source wow-ui-source --product retail --ref refs/heads/live --format json | ConvertFrom-Json
$sourcePin = $source.result.id

lycheedev source query C_Spell.GetSpellInfo --snapshot $sourcePin --mode precise --topic api --limit 10 --format json
lycheedev source context --snapshot $sourcePin --symbol-id <symbolId> --format json
```

分支只解析一次并固定为不可变 pin；后续读取复用该 pin 或项目锁。精确文件、关系、
版本比较和插件加载闭包分别使用 `source inspect`、`source refs`、`source diff` 与
`source validate`。

### 数据

```powershell
$target  = lycheedev target resolve --installation $client --region cn --locale zhCN --format json | ConvertFrom-Json
$dataPin = $target.result.id

lycheedev data agent serve --snapshot $dataPin --installation $client --format jsonl
```

打开一个调用方拥有的 `data agent serve` 进程，按
`lycheedev schema data agent serve` 声明的类型化 JSONL 任务发送请求。会话持有固定
数据身份与结果句柄，供分页、导出和 capture 使用；关闭或 deadline 会结束该归属。
已发布的版本列表用 `target available`，命名目标配置用
`target add` / `target show` / `target list`。

### 资源

```powershell
lycheedev asset search --snapshot $dataPin --listfile community-csv --query "interface/icons" --format json
lycheedev asset export --snapshot $dataPin --installation $client --file-id 123456 --encoding png --output .\icon.png
```

默认导出原始字节；显式 encoding 才做图像转换。缺失名称、变体歧义、缺钥或超出上限
都是错误，不是空结果成功。

### 实机（仅受权任务）

```powershell
lycheedev live instances --format json
lycheedev doctor --pid <pid> --format json        # 只读就绪检查
lycheedev live connect --installation $client --format json
lycheedev live probe put --name <name> --file <probe.lua>
lycheedev live execute --session <CON-id> --probe <name> --request <key> --budget-seconds 30 --format json
lycheedev live disconnect <CON-id>
```

实机工作绑定单个已验证进程/runtime，并保留持久归属、请求身份与恢复 journal。
`doctor --fix` 只在受权安装/目标范围内修复；没有当前输入协议时，首次激活可能
需要一次手动 reload。已提交或结果未知的输入不会重放。`live status`、`live resume`
与 `live session` 用于检查和恢复原工作。

### 工作区、缓存与证据

```powershell
lycheedev doctor --format json
lycheedev cache status --data --format json
lycheedev evidence list --limit 20 --format json
lycheedev evidence verify <CAP-id>
```

`cache prune` 只回收普通可重建缓存；Data 派生对象回收是显式的 `--data-derived`
操作。capture 与已提交证据会保留到显式移除。Data 默认容量预算为十进制 5 GB
（5,000,000,000 字节），可用 `lycheedev config set` 配置。

## 结果与限制

- 普通命令返回版本化 `lycheedev.result.v1` envelope；JSONL 输出使用
  begin/record/end 帧，成功需要合法 end 帧。Data Agent 会话使用自己的版本化协议。
- `complete`、`partial`、`pending`、truncated 与 unknown 分别报告；空的 partial
  结果永远不能证明不存在。
- 退出码：`0` 完成，`2` 参数/选择，`3` 能力/环境，`4` 输入/协议，`5` 外部/业务失败，
  `6` pending，`7` cancelled，`8` internal。
- 会话结果句柄归会话所有；CAP 是已提交证据，可在会话结束后保留。分页与导出消费
  保留结果，不重新执行查询。
- 缺失内容、缺钥、覆盖不足和配额都以错误或显式缺口报告，不用其他 build、region、
  product 或 client 替换。

## 仓库结构

| 路径 | 内容 |
| --- | --- |
| [crates/lycheedev-cli](crates/lycheedev-cli/) | 命令目录、解析、发现、应用编排与公共输出。 |
| [crates/lycheedev-source](crates/lycheedev-source/) | 固定源码身份、加载闭包、分析、查询、关系与验证。 |
| [crates/lycheedev-emmy-adapter](crates/lycheedev-emmy-adapter/)、[crates/lycheedev-binding-contracts](crates/lycheedev-binding-contracts/) | 围绕固定 Emmy 上游引擎的窄适配层与共享类型。 |
| [crates/lycheedev-data](crates/lycheedev-data/) | CASC/WDC/BLTE 输入、Engine、DataFusion、Data Agent 会话、资源与 Hotfix。 |
| [crates/lycheedev-live](crates/lycheedev-live/) | 游戏实例绑定、Mailbox、输入、恢复、报告与收尾。 |
| [crates/lycheedev-delivery](crates/lycheedev-delivery/) | 受管安装、更新事务、插件/Skill 交付与 npm 交接。 |
| [crates/lycheedev-core](crates/lycheedev-core/) | 共享身份、错误、控制、工作区、存储与证据。 |
| [crates/lycheedev-windows](crates/lycheedev-windows/) | Windows 句柄、文件、进程、Job、IPC 与输入边界。 |
| [addon/](addon/) | 现有游戏内 Lua 工作台插件、客户端配置与 Mailbox v1。 |
| [skills/lycheedev](skills/lycheedev/) | 第一方 Agent Skill 及其按任务读取的 references。 |
| [packages/npm/lycheedev](packages/npm/lycheedev/) | npm 包与原生 CLI 启动器。 |
| [tools/](tools/)、[tests/](tests/) | 构建、打包、CI 与测试工具；夹具与测试目标。 |
| [release/](release/) | 版本源、发布工具身份、许可证补充清单与数据。 |

## 文档

- [DESIGN.md](DESIGN.md)：业务目标、架构边界、公开合同与 Skill/CLI 使用面。
- [AGENTS.md](AGENTS.md)：仓库工作规则、文档分工与验证政策。
- [todo.md](todo.md)：未完成的实现、清理、CI 与验证工作。
- [test.md](test.md)：当前测试用途、入口与覆盖。
- [handover.md](handover.md)：当前分支、协作边界、实际验证结果与阻塞。
- [skills/lycheedev/SKILL.md](skills/lycheedev/SKILL.md)：已安装的 Agent Skill。
- [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)：第三方组件与署名。

## 许可证

项目自有代码以 [MIT 许可证](LICENSE)分发。第三方组件保留各自许可证，见
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
