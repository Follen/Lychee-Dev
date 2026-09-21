# Lychee Dev Toolkit 2.0 设计方案

状态：待实现的设计基线。日期：2026-09-21。

本文定义目标架构与契约，不代表下文的命令、类型、协议或并发保证已经实现。配套文件：[回归测试方案](regression.md)、[实施路线与能力映射](roadmap.md)、[Windows CI 与 2.0.0 发布规范](release-2.0.0.md)。本次只修改设计文档，不修改运行代码、安装内容或用户数据。

## 1. 产品决策

将源码研究、游戏数据、资源导出和游戏内调查合并为一个 Lychee Dev Toolkit。使用单仓库、单 Go module、单 CLI、单工作空间、单 skill 入口和统一版本发布。

- 宿主实现使用 Go 1.27；首个构建基线建议固定 Go 1.27.1。本机已观察到 Go 1.27.0。
- 游戏内执行端使用 WoW Lua 5.1 子集，继续遵守禁用零开销、事件驱动、secret 值检查与无污染约束。
- 用户入口为 `lycheedev`，npm 包为 `lycheedev`，插件安装目录为 `Lychee Dev/`。
- 本次正式发行固定为 `2.0.0`，Git tag 为 `v2.0.0`，正式 npm 发布为 `lycheedev@2.0.0`。
- 直接发行原生二进制及配套资源包；npm 仅是另一种分发渠道。npm 脚本不包含产品选择、业务处理或 Python 引导逻辑，保持无运行时 npm 依赖。
- 2.0.0 的单一 npm 包携带各平台二进制和共用资源，不使用 optionalDependencies 平台依赖链；平台专用原生压缩包独立提供。包体积和安装成本纳入验收。
- 发布程序和正式验证链路不依赖 Python，不调用旧的 wowdoc、wowdata、automation.py。Git 可作为源码同步的明确依赖；doctor 按能力检查，不阻断不需要 Git 的数据查询。
- 不提供旧命令、旧配置、旧私有协议或旧函数的兼容别名。历史用户数据不迁移、不自动导入。
- 无后台常驻服务要求。命令进程按需打开模块，任务状态由工作空间持久化。
- 游戏自动化首发支持 Windows amd64。Linux/macOS 提供已验证的离线、源码和数据能力；不把交叉编译成功描述为游戏自动化可用。

Go 版本依据：[官方发布记录](https://go.dev/doc/devel/release)。重写不需要依赖 1.27 独有语法，以可读性和可验证性为先。

## 2. 新命名规范

命名依据新职责重新定义，范围覆盖自有包、文件、类型、字段、函数、Lua 私有模块、命令、错误码、配置键和协议标识。迁入算法时也必须落入新接口及命名体系，不将原函数名作为保留接口。

允许保留产品名称、第三方署名、WoW API、Go 标准接口，以及 CASC、DB2、DBD、BLTE、BLP、UTF-16 等标准术语。标准 `Read`/`Close` 方法和普通局部变量遵守语言惯例，不通过生僻同义词制造新命名。

| 范围 | 新规范 |
| --- | --- |
| Go 包 | 职责明确的小写名，如 `selection`、`codebase`、`records`、`bridge` |
| Go 类型 | 名词，明确持有的职责，如 `Pinner`、`Browser`、`Executor`、`Archive` |
| Go 方法 | 具体动作，如 `PinSelection`、`FindSymbols`、`ExecuteProbe`、`ContinueRun` |
| 业务字段 | 表达真实语义，如 `sourceCommit`、`dataBuild`、`processStartedAt`，不用模糊 `version` |
| CLI | 名词组 + 动作，统一 kebab-case 参数；全局参数含义不随子命令变化 |
| JSON | lowerCamelCase；`schema` 为明确的格式版本；枚举使用 snake_case |
| 错误码 | `selection.ambiguous`、`bridge.receipt_mismatch` 等模块限定名 |
| Lua 私有模块 | `ns.ProbeQueue`、`ns.ExecutionGate`、`ns.CaptureWriter`、`ns.ReloadLatch`、`ns.ReceiptView` |
| 新数据命名空间 | `LycheeToolkitDB`；不读取 `LycheeDevDB`、`DumperDB` |
| 新游戏入口 | `/lychee` 打开工作台；`/lychee bridge ...` 为受协议约束的机器入口 |
| 新协议 | `lycheedev.result.v1`、`lycheedev.capture.v1`、`lycheedev.signal.v1` |

禁止进入正式实现的遗留入口包括 `RunWowdoc`、`NewRuntime`、`runAutomation`、`cmd_run`、`send_command_messages`、旧工具的宿主目录解析器和旧 npm 生命周期实现。审计允许它们出现在历史说明和测试来源清单中。

每个新符号必须说明所属职责。命名审计检查依赖和定义位置，不仅做字符串替换；不建立一个庞大的 `common`、`utils`、`Manager` 或全能 `Service`。

## 3. 仓库与依赖方向

```text
cmd/lycheedev/                 程序入口
internal/
  command/                    命令定义、参数、结果渲染
  actions/                    应用用例及模块装配
  selection/                  产品身份和不可变目标解析
  codebase/                   版本化源码检索与验证
  records/                    静态游戏表、SQL、领域关系
  changes/                    独立 Hotfix 查询
  assets/                     文件定位、解码与导出
  bridge/                     游戏会话和执行闭环
  evidence/                   证据归档与关联
  jobs/                       操作状态、资源准入、恢复记录
  vault/                      工作空间、对象、事务与锁
  desktop/                    Windows 原生输入和捕获实现
  delivery/                   插件、skill 与发行资源安装
addon/                        新命名后的 Lua 执行端和 UI
skills/lycheedev/              唯一的 skill 源
protocol/                     协议、JSON Schema、跨语言样本
catalog/                      产品目录和验证过的客户端事实
packages/npm/                 极薄安装与启动包装
tests/                        契约、跨进程、集成、实机测试
tools/                        Go 验证与生成工具
docs/
go.mod
```

允许的主依赖方向：

```text
command -> actions -> selection / codebase / records / changes / assets / bridge / evidence / delivery
assets -> records
bridge -> jobs / evidence / desktop
业务模块 -> selection / vault / 必需的 jobs 资源预算接口
jobs -> vault
selection -> catalog 的生成数据 / vault
vault -> 标准库及持久化实现
```

`records` 与 `changes` 不能相互导入。Hotfix 保留独立数据语义，不暗中覆盖静态 DB2 结果。

`desktop` 不依赖业务模块。`bridge` 定义自己消费的窄接口，Windows 实现和协议模拟器分别满足接口。其他模块优先使用具体类型，仅在真实变化点和外部依赖处增加接口。

业务模块不依赖 Cobra，不打印 stdout，不调用另一个 CLI，不自行读取环境变量和用户根目录。应用装配层注入所需对象；各命令只初始化实际使用的模块。数据查询不得创建捕获会话，源码查询不得创建游戏事件或后台输入对象。

## 4. 统一业务对象

| 对象 | 所有者 | 必须成立的不变量 |
| --- | --- | --- |
| `SelectionSpec` | selection | 用户意图，允许命名配置，但没有静默的产品默认值 |
| `PinnedSet` | selection | 已解析的不可变引用集合；以内容摘要标识，新增引用产生新集合 |
| `SourcePin` | selection | 仓库身份、分支来源、精确 Commit、解析器版本；Tag 只是可追溯标签 |
| `DataPin` | selection | 产品、地区、完整 Build、CASC 配置键、语言及 DBD 引用 |
| `ChangePin` | selection | Hotfix 提供方、产品、Build、地区、查询范围、抓取时间和内容摘要 |
| `WindowBinding` | bridge | 安装身份、PID、进程启动时间、HWND、可执行路径、角色和本次游戏标记 |
| `WorkRecord` | jobs | operationId、调用意图、固定引用、已确认阶段、未决外部效果及结果 |
| `CaptureRef` | evidence | 内容摘要、类型、来源定位、固定引用、完整/部分状态；不能仅有二维码 |

用户可维护可变目标配置；每次工作流必须先固定引用，并显式传递 `--snapshot`。后续动态加入源码或 Hotfix 时生成派生 PinnedSet，保留父引用和已有固定内容，不重新解析已固定的 latest。

源码 Commit、游戏 Build、插件版本和 Hotfix 时间分别存储。对应关系明确为 `exact`、`compatible`、`mismatch`、`unknown`；精确匹配无法获得时返回可操作错误，不自动借用最新源码。

纯源码研究只要求 SourcePin；纯数据查询只要求 DataPin；离线证据读取无需实时窗口。客户端目录只表示位置，身份依次来自本地产品元数据和 Build，目录名仅作有证据的最后回退。

产品映射、地区映射、支持能力和 TOC Interface 的已验证事实来自统一目录。生成 Go/Lua/文档需要的视图，禁止 skill 手写第二份事实表。复用测试轨道必须核对其实际内容。

## 5. 核心 Go 接口草案

以下符号是实现时的新命名基线，不是已存在的代码。请求类型承载有界参数，返回类型保留部分结果和来源信息。所有耗时入口接受 `context.Context`，流式资源由调用者明确 `Close`。

| 新类型 | 新方法 | 隐藏在接口后的工作 |
| --- | --- | --- |
| `selection.Pinner` | `PinSelection(ctx, SelectionSpec) (PinnedSet, error)` | 名称解释、身份校验、不可变引用解析 |
| `codebase.Browser` | `FindSymbols(ctx, SourcePin, SymbolQuery) (SymbolMatches, error)`；`ReadSpan(ctx, SourcePin, SpanQuery) (SourceExcerpt, error)` | 准备索引、查询排序、精确位置和摘要 |
| `codebase.Comparer` | `CompareTrees(ctx, SourcePair, ChangeQuery) (SourceDelta, error)` | 两个显式固定版本间的差异 |
| `codebase.Checker` | `CheckClosure(ctx, SourcePin, AddonInput) (LoadAssessment, error)` | TOC/XML 有序闭包、静态检查、动态未决项 |
| `records.Reader` | `DescribeTable(ctx, DataPin, TableQuery) (TableShape, error)`；`SelectRows(ctx, DataPin, ReadStatement) (RowCursor, error)` | 数据准备、表解析、只读查询、内存预算 |
| `records.Navigator` | `ResolveLinks(ctx, DataPin, EntityQuery) (EntityGraph, error)` | 法术、物品、生物、遭遇和装饰等关系 |
| `changes.Feed` | `ReadChanges(ctx, ChangeQuery) (ChangeBatch, error)` | 提供方差异、分页、原始记录及解码来源 |
| `assets.Exporter` | `LocateAsset(ctx, AssetQuery) (AssetLocation, error)`；`WriteAsset(ctx, ExportRequest) (ExportManifest, error)` | 文件定位、流式读取、转换与输出摘要 |
| `bridge.Binder` | `BindWindow(ctx, BindingRequest) (WindowBinding, error)` | 进程和角色验证、协议能力核对 |
| `bridge.Executor` | `ExecuteProbe(ctx, ProbeRequest) (ExecutionOutcome, error)`；`CollectFaults(ctx, FaultRequest) (ExecutionOutcome, error)`；`ContinueRun(ctx, WorkID) (ExecutionOutcome, error)`；`AbortRun(ctx, WorkID) (ExecutionOutcome, error)` | 一次完整游戏操作及其恢复和收尾 |
| `evidence.Archive` | `CommitCapture(ctx, CaptureDraft) (CaptureRef, error)`；`FetchCapture(ctx, CaptureID) (CaptureReader, error)`；`AssembleBundle(ctx, BundleRequest) (BundleManifest, error)` | 原始字节、元数据、保留关系与完整性 |
| `jobs.Book` | `BeginWork(ctx, WorkIntent) (WorkRecord, error)`；`AdvanceStage(ctx, StageChange) error`；`InspectWork(ctx, WorkID) (WorkRecord, error)` | 带前置阶段与世代检查的持久状态转换 |
| `vault.Store` | `PublishBlob(ctx, BlobInput) (BlobRef, error)`；`HoldResource(ctx, LockRequest) (ReleaseFunc, error)` | 校验后发布、跨进程排他与原子性 |
| `delivery.Installer` | `InspectRelease(ctx, InstallTarget) (InstallAssessment, error)`；`ApplyRelease(ctx, InstallPlan) (InstallReceipt, error)` | 清单、所有权、部署与协议兼容 |

低层 helper 以实际算法职责命名，在实现阶段随所属模块定义，不先制造数百个无实现的函数。旧函数名不会通过 wrapper、type alias 或内部兼容层延续。

## 6. CLI 设计

命令目录统一维护，可生成 `--help`、`describe --format json` 和 skill 命令参考。以下均为目标命令。

| 入口 | 动作 / 用途 |
| --- | --- |
| `init`、`doctor`、`version`、`describe` | 初始化、能力检查、版本、机器可读命令契约 |
| `target` | `list / add / show / resolve / remove`，目标配置与快照 |
| `project` | `init / lock / status`，项目级意图和不可变依赖 |
| `source` | `list / sync / index / query / inspect / diff / validate` |
| `data` | `sql / db2 / hotfix / spell / item / creature / encounter / decor` |
| `asset` | `search / inspect / export / demux`，文件、图标、图片、视频 |
| `live` | `instances / bind / run / bugs / reload / status / resume / cancel` |
| `evidence` | `list / show / verify / bundle / keep / remove` |
| `cache` | `status / verify / prune` |
| `addon`、`skill` | 各自 `install / status / remove` |

参数规则：`--home` 是完整工作空间根；`--snapshot` 接受 PinnedSet ID；`--session` 接受 WindowBinding ID；`--output` 是明确的产物位置；`--format` 统一为 text/json/jsonl；`--offline` 阻止联网。CSV 属于导出编码，通过 `--encoding csv` 选择，附带机器可读 manifest。

目标选择顺序为显式固定快照、显式目标配置、项目锁定。没有足够信息就返回缺失字段，不自动选择最后使用的窗口或写入共享的 current-target。固定快照与临时目标字段冲突时报错，不能静默覆盖。

源码验证支持单 TOC 和 `--matrix <file>`。移除旧工具独立的 install/update/profile/doctor/cache 生命周期，保留一份 Toolkit 行为。npm 自身更新交给 npm；独立发行包更新使用发行清单对应的安装流程。

示意工作流（ID 由上一步真实返回，以下不是可直接执行的现有命令）：

```text
lycheedev target resolve --target retail-cn --format json
lycheedev source query C_Spell.GetSpellInfo --snapshot <pin> --format json
lycheedev data sql --file query.sql --snapshot <pin> --format json
lycheedev live bind --pid <pid> --snapshot <pin> --format json
lycheedev live run --file probe.lua --snapshot <pin> --session <binding> --format json
lycheedev live resume <operation-id> --format json
lycheedev evidence bundle --ids <capture-a>,<capture-b> --output ./report
```

## 7. 统一结果与证据

JSON 外层字段固定为 `schema`、`ok`、`operationId`、`context`、`result`、`captures`、`warnings`、`error`。context 保存实际解析后的引用及匹配关系；error 包含 `code`、`message`、`stage`、`retryable`、`resumeOperationId`。恢复信息是受约束的数据，不是可以从外部响应直接执行的 shell 字符串。

- `ok` 表示请求的操作合同是否完成。游戏任务另有 `result.probeStatus`；探针报错也可以完成采集与收尾。
- 数据已读取但 ACK 清理未确认时，返回已有 capture、准确阶段与恢复 ID，不输出整个操作成功。
- `result.complete`、`truncated` 和截止原因不能省略；结果上限触发时不可把部分查询描述为完整结果。
- JSONL 使用带类型的 begin/record/end/error 帧；只有合法 end 且退出 0 才是完整成功，broken pipe 不伪造完成。
- 退出码：0 完成，2 参数/选择问题，3 能力/环境不满足，4 输入数据或协议无效，5 外部失败，6 未决或收尾未完成，7 显式取消，8 内部错误。具体根因以稳定错误码为准。
- stdout 只承载指定格式；进度和日志走 stderr；帮助和参数错误也遵守格式约定。

查询记录保存固定引用和来源。小结果可完整归档，大结果以流式产物及 manifest 保存；不为了统一证据而复制整个 DB2 库。预算不足时报告部分状态或明确失败。

Capture 使用 `CAP-<id>`，operation 使用 `OP-<id>`，窗口绑定使用 `WIN-<id>`，固定集合使用 `PIN-<digest>`。摘要校验描述原始字节，不通过 JSON 重排后再声称字节相同。

原始游戏报告在宿主保存并校验成功后才允许发送 received ACK。QR 只传信号、身份和摘要，不承载完整正文。角色、安装、游戏 Build、请求、代码摘要均须匹配，不再依赖 Agent 手工补齐校验。

校验分层：宿主归档与源码/资源对象使用 SHA-256；游戏侧的小型传输校验可以采用 UTF-8 字节数与 Adler-32，并由宿主验证后另算 SHA-256。二者字段名和用途必须区分，Adler-32 不作为身份认证。ProbeQueue 条目同时保留宿主代码摘要与游戏可核对的字节校验，不能仅回显一个摘要就声称实际执行代码已验证。

## 8. 新工作空间与旧数据隔离

```text
~/.lycheedev/
  workspace.json              workspaceId、格式与创建信息
  config.json                 用户偏好和资源预算
  state/toolkit.sqlite        目标、工作状态、证据目录和资源准入
  pins/                       不可变固定引用清单
  mirrors/                    Git 镜像
  blobs/                      持久内容对象
  indexes/                    可重建索引
  cache/                      可回收网络及派生数据
  runs/<operation-id>/        输入、诊断、恢复材料
  captures/<capture-id>/      完整报告与证据 manifest
  locks/
  tmp/
```

`--home > LYCHEEDEV_HOME > ~/.lycheedev`；环境变量的新含义是完整根路径，不延续旧的“用户主目录再拼 .lycheedev”语义。项目中使用 `lycheedev.json` 与 `lycheedev.lock.json` 保存意图和固定引用，不提交账号或机器路径。

元数据采用 SQLite 短事务；大对象不进入同一个数据库。源码索引仍可按仓库和 schema 分库。CASC 既有外部寻址键保留，内部存储清单负责映射和完整性，避免每次查询重新复制大文件。

全新安装不读取旧工具目录和旧环境变量。初始化遇到没有新 workspace 标记的占用目录时返回 `workspace.legacy_detected`；显式 fresh 初始化提供已解析的归档计划，将旧根隔离至一个确定的同级归档目录，再创建新根。不合并、不自动删除、不在 npm postinstall 中执行破坏性清理。归档切换中断必须可恢复。

旧 `~/.wowdoc`、`~/.wowdata` 和 `%LOCALAPPDATA%/LycheeDev/automation` 留在原处且不被读取。旧插件数据不导入 `LycheeToolkitDB`；不在运行中的客户端上改写 SV。新发布物不声明旧 SV 变量，也不包含迁移读取逻辑。

“旧数据不继承”仅指切入本 Toolkit。Toolkit 自身未来格式升级仍需有版本、原子性和未知新版本拒绝写入机制，不能每次升级都清空新用户数据。

普通缓存可淘汰；未完成操作、保留证据和固定对象由引用保护。清理与对象获取必须原子协调，不能在检查使用状态后出现删除竞态。外部输出默认拒绝覆盖，覆盖必须是显式选项。

## 9. 并发与资源隔离

不同 Agent 启动独立 CLI 进程。隔离依靠明确身份和跨进程协调，不依赖进程内 mutex。

| 资源 | 协调范围 | 规则 |
| --- | --- | --- |
| 固定快照、已提交对象 | 不可变引用 | 并行读取 |
| 同一待下载对象 | 对象键 | 单写者，其他进程有界等待并复用校验后对象 |
| 同一索引发布 | 仓库 + snapshot + schema | 临时构建，校验后原子切换；不长时间占元数据写锁 |
| 任务注册清单 | 安装身份 | 短写锁，按所有权及修订检查更新自己的条目 |
| 游戏输入与重载 | 实际窗口会话 | 整个操作持有排他执行权；冲突返回 busy 或显式有界等待 |
| 操作推进 | operationId + 阶段 + 世代 | CAS 检查，防止两个恢复者同时推进 |
| 元数据事务 | 单次事务 | 短锁；禁止持锁等待网络、二维码或游戏 |
| 产物提交 | 归档 ID / 输出路径 | 独立临时路径，原子提交，禁止静默覆盖 |
| 清理 | 对象与保留引用 | 使用中或未提交对象不可删除 |

两个窗口共用安装目录时，注册表保存多条独立请求；写锁释放后各窗口核对自己的请求代码摘要和加载确认。其他窗口加载到某条定义不构成执行授权，执行还要匹配窗口身份及 nonce。

锁的存活依据使用 OS 锁和 PID + 启动身份，不单靠文件年龄；不能因机器繁忙或租期到时抢走活进程的游戏输入权。宿主死亡释放 OS 锁后，未决 WorkRecord 仍阻止新执行绕过恢复流程。

窗口和安装写锁的键由主机上的规范安装路径及实际会话身份生成，不含 home 路径；同一主机上的两个 home 也不能同时获得同一窗口输入权。安装侧队列保存有限的 workspaceId/operationId 所有权记录；若未决操作属于另一工作空间，则返回 foreign_owner 并要求从原工作空间恢复，不依赖本 home 查不到历史就当作没有历史。全局共享的只有必要的协调标记，证据与业务数据库仍归各自 home。

固定全局锁顺序，并且不在持有注册表写锁时等待窗口锁；窗口锁可以包围短注册表事务，释放后再等待游戏信号。多资源获取失败释放已占资源。取消和超时不会释放给另一个进程后仍继续发送旧输入。

下载、解码和索引预算在工作空间层协调，不能每个 Agent 各自申请整台机器的预算。准入令牌通过短事务取得，等待过程不占数据库写锁。内存估计与 Go 内存软限制需配合测量，不能承诺精确的进程总内存硬上限。

普通游戏操作可与数据查询并行。性能调查显式申请 measurement 资源策略：暂停或限制 Toolkit 的重型工作，并记录实际并发负载。不同 home、其他软件及人工游戏操作不在同一协调范围内，报告必须声明该限制。

## 10. 游戏执行与恢复协议

新游戏协议由 CLI 与插件共同实现；双方交换 release、protocol、能力、游戏身份和会话 nonce。协议不匹配时停止该能力，不能向旧插件持续发送新命令。

正常阶段如下，输入侧加载和输出侧落盘重载分别记录，不能混为一次 reload：

```text
prepared -> load_requested -> loaded -> dispatch_requested -> reported
         -> flush_requested -> persisted -> verified -> ack_requested
         -> acknowledged -> cleaned
```

已有内置错误快照不需要临时探针加载。每次外部副作用前持久记录 intent，回执后再确认阶段。主状态另有 pending/running/unresolved/failed/cancelled/completed，不将所有失败压成一个布尔值。

- 启动任务分配新请求 ID；恢复沿用原请求 ID、代码摘要和原重载 nonce。
- 预期 reload 会改变插件的 Lua 会话标记；只有匹配原 reload nonce、相同进程/角色/Build 的 ready 信号才允许在原操作内更新 WindowBinding。无此关联的身份变化使绑定失效。
- 宿主在 PostMessage 成功后崩溃属于结果不确定；恢复先观察和核对，不自动补发执行。
- 不以“文件 mtime 改变”代替精确报告校验；读到半写文件时有界等待，保留原 intent。
- 不以“没有捕获到画面”代替“ACK 标记消失”；清理需要有效画面或对应协议证据。
- `ContinueRun` 只执行尚未完成且能够证明安全的阶段。无法判断时保留 unresolved。
- `AbortRun` 尝试协议支持的取消，读取取消确认并清理自有资源；不承诺强制中断任意同步 Lua 或杀死游戏进程。
- 同步 Lua 无法被宿主可靠抢占，超时只终止宿主等待。skill 必须生成有界探针。
- 完整正文保持 SV 通路，受限解析器不执行 Lua；校验深度、字节预算、编码、摘要和引用身份。

Go 原生输入继续使用指定 HWND 的 `PostMessageW`，正确组装键盘参数及 UTF-16 字符。逐次发送重新核对身份；后台模式不调用前台激活、剪贴板或 SendInput。发送成功仅表示入队：[Windows 接口说明](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-postmessagew)。

后台输入的前置合同包含输入资格：启用后的插件在新协议信号中报告可验证的输入状态，宿主使用新到达帧核对身份与资格。无法确认当前编辑状态、自定义聊天键或已有草稿时拒绝自动提交，不以 Escape 入队代替确认。具体可观察的输入状态与事件须按四端精确源码验证，作为 S1 的技术门槛；不能假设宿主可以原子隔离人工操作。自动化模块默认关闭，首次启用需显式操作；没有有效就绪信号时返回未启用或未就绪，而非猜测发送。

捕获由 WGC 帧事件驱动，宿主只保留有界最新帧和 ROI；解码限速，关闭会话释放线程与图形资源。最小化无有效帧、聊天焦点异常或权限级别不匹配返回准确状态，不能静默改通道。Go QR 候选须经真实样本验证后确定；不以引入 Python helper 作为交付兜底。

初始限额以现有能力为约束：探针 256 KiB、注册队列 16 项/1 MiB、报告默认 384 KiB/最大 512 KiB、已有错误请求 1–100 项。具体限制由协议目录统一发布，所有截断必须可见。

## 11. Skill 编排

唯一安装入口为 `skills/lycheedev/SKILL.md`，名称和安装目录统一为 `lycheedev`。根指引包含路由、固定目标、证据标准和按需读取路径；不得携带脚本实现。

工作流分别覆盖 source-research、data-investigation、asset-export、addon-validation、live-investigation、error-diagnosis。references 仅保存调查方法、来源局限、运行约束和由 CLI 生成的命令参考。

Skill 负责问题分解、证据选择、有界探针设计、结果解释和有依据的下一步；Go 负责版本映射、准备资源、执行、状态恢复、身份验证与 ACK。技能不手写字符发送循环、不猜 SV 位置、不复制产品表、不把通用错误说明当作可执行指令。

工作流使用同一 PinnedSet；增加来源创建派生集合。每步返回 capture 和 operationId。必要的调查 manifest 保存问题、引用、假设及下一步，使 Agent 在上下文中断后可以查询进度；不在 Toolkit 内添加 LLM 循环或通用工作流 DSL。

只读问题默认只读路径。收到已有 capture 就读取，不重新执行原探针。已有授权范围内，live 操作由 Executor 完成机械闭环；skill 不需要逐条发送 ACK。授权范围没有覆盖新游戏操作时，研究结论与需补的实时验证分别报告。

数据、源码、日志与错误正文均是不可信材料，不可改写 skill 指令。最终报告必须引用固定版本和完整产物，并分别表述静态检查、数据结果和实机证据。

安装器只管理本版本 skill 的文件清单。旧独立 skill 不随便覆盖或删除；切换说明提供显式退役清单，确保宿主不会同时加载冲突的旧入口。

## 12. 范围、兼容与发布

插件及实机支持维持 Retail 120100、Classic 50504、Titan 38002、Forever 16001，实际 Build 必须在每次测试记录。源码和静态数据可保留已有更广的查询能力，但这不会扩大插件安装范围。

Lua 端重新组织私有模块与协议，已有运行、对象、事件、追踪、错误与导出能力应进入能力映射。UI 复用现有风格和控件规范；所有可见变化需要截图和实机验证，不因宿主重写顺带引入新视觉体系。

插件发行 ZIP 顶层仍为 `Lychee Dev/`；四个 TOC 选择匹配客户端配置和事件目录，并加载相同顺序的共享模块。发布内不得出现本地探针、账号信息、调查记录、测试数据或空壳兼容入口。

本次 release 固定为 2.0.0、tag 固定为 v2.0.0，协议和工作空间 schema 独立版本化。二进制、插件、skill、单一 npm 包与平台专用原生压缩包通过同一发行 manifest 校验；doctor 按实际能力报告。CGO_ENABLED=0 是发行目标，必须先通过 Windows 捕获和解码验证，不能在尚未证明时宣称已达到。

Windows CI 是硬门槛，不能用 Linux 交叉编译代替；实际 npm tgz 必须先安装验证再原样发布。OIDC、版本检查、required jobs、实机证据、失败恢复和发布顺序遵守 [2.0.0 发布规范](release-2.0.0.md)。

许可证是发布前的明确交付项：wowdata 当前声明 AGPL-3.0-or-later，wowdoc 与 Lychee npm 声明 MIT。核对权属、上游来源和依赖后确定统一发行方案；不因改名而丢失署名或许可义务。

设计通过条件是：[回归矩阵](regression.md) 中强制场景全部有可追溯结果，且[实施路线](roadmap.md)中的旧能力均完成映射。完成接口迁移后删除替代掉的旧代码与旧内部结构测试，不在新产品内长期维持两套内核。
