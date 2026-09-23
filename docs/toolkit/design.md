# Lychee Dev Toolkit 2.0 设计方案

状态：整体设计收敛后的实施基线。日期：2026-09-21。实际覆盖见 [implementation-status.md](implementation-status.md)。

本文定义目标架构与契约，不代表下文的命令、类型、协议或并发保证已经实现。配套文件：[回归测试方案](regression.md)、[实施路线与能力映射](roadmap.md)、[Windows CI 与 2.0.0 发布规范](release-2.0.0.md)。设计提交后，用户已授权创建分支、实施重写及真机测试；最新目标同时授权全部完成并通过测试后发布 npm 2.0.0，不得提前发布未完成实现。

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
- 产品中心是“选定开发目标，研究源码、查询数据、操作游戏并取得可追溯结果”。协议阶段不是公共产品对象；统一不要求只读查询经过游戏任务的持久状态机。
- 完成以端到端用户场景验收，不以新增检查、测试数量、目录数量或重复打包作为进度替代。
- 产品只支持 Windows amd64（2026-09-23 owner 裁决：清理 macOS 与 Linux 支持）。纯 Go 包保持可移植以便开发，但不构建、不发行、不验收非 Windows 平台。

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
| 游戏入口 | `/dev` 打开工作台；`/dev connect` / `disconnect` 控制连接；`/dev bridge ...`（含 `identify <nonce>` 身份标记）为受协议约束的机器入口 |
| 新协议 | `lycheedev.result.v1`、`lycheedev.capture.v1`、`lycheedev.signal.v1` |

禁止进入正式实现的遗留入口包括 `RunWowdoc`、`NewRuntime`、`runAutomation`、`cmd_run`、`send_command_messages`、旧工具的宿主目录解析器和旧 npm 生命周期实现。审计允许它们出现在历史说明和测试来源清单中。

每个新符号必须说明所属职责。命名审计检查依赖和定义位置，不仅做字符串替换；不建立一个庞大的 `common`、`utils`、`Manager` 或全能 `Service`。

## 3. 仓库与依赖方向

```text
cmd/lycheedev/                 程序入口
internal/
  command/                    命令定义、参数、结果渲染
  selection/                  产品身份和不可变目标解析
  codebase/                   源码研究用例及下载、索引、比较、验证
  records/                    数据用例及静态表、SQL、Hotfix、领域关系和资源访问
  live/                       游戏连接、完整运行、状态查询和恢复
    journal/                  游戏操作持久记录和排他执行权，不是通用任务框架
  bridge/                     Go/Lua 信号与报告编码；不编排业务
  evidence/                   证据归档与关联
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
command -> selection / codebase / records / live / evidence / delivery / vault
live -> 自己的 journal / bridge / evidence / desktop / delivery
codebase / records -> selection / vault / evidence
delivery -> 安装验证所需的 codebase / records / selection / bridge / vault
live/journal -> vault
selection -> catalog 的生成数据 / vault
vault -> 标准库及持久化实现
```

Hotfix 与静态 DB2 保留独立数据语义，不暗中覆盖；它们和资源导出共用 records 的寻址、下载与内容访问，不因命令分组不同建立重复内核。

本地 Hotfix 查询由 `records` 直接读取明确指定的 DBCache，验证整个文件的格式与
Build 后将原始字节写入统一 evidence；派生目标的 Changes 固定摘要和采集时间，
不修改原数据目标。后续筛选/分页从 source capture 读取同一份字节，不能针对正在
变化的客户端文件推进游标。保留物理记录、原始状态和 payload，不把“本地查询完整”
表述为服务器覆盖完整，也不把选定产品/语言/地区表述为该文件独立认证的事实。
命名表选择通过同一 Definitions 模块读取固定 commit，按精确 Build 选择字段定义；
没有 DB2 文件 ID 的 Hotfix 表也可读取定义，但不能据此进行静态 DB2 寻址。
Hotfix 只携带表哈希，命名选择遇到哈希碰撞时拒绝，不借用户指定的名字掩盖歧义。
`--table` 返回有效 payload 的解码字段，原始字节与非有效状态仍保留；
`--table-hash` 选择无需定义的原始查询。`--latest` 在表/记录筛选后、分页前选择
最大 signed push 的完整批次，不折叠同 ID 记录，不定义 DB2 覆盖策略。远程来源仍待实现。

本地目标准备由 records 读取已选客户端与 CASC 配置，selection 负责固定结果。
`target resolve --installation <client> --region <region> --locale <locale>`
不要求调用者拼装配置键；DBD 引用只在解析时固定，后续查询不追踪分支。
`--from <pin>` 扩展既有源码集合，不覆盖固定身份；数据查询同时接受该客户端目录
及明确的 CASC 根目录，避免调用方重复理解安装布局。安装与 live 使用同一客户端
身份读取实现，不各建一套产品/目录识别规则。远程身份用同一命令的
`--product <track> --region <region> --locale <locale>` 准备；`--build` 约束
该地区发布清单中的完整版本，不自动查历史或借用其他地区。在线分别观测版本/CDN
清单，完整校验配置后保存成一组离线观测；每次输出保留各自的固定身份，不从并发
缓存写入者取得另一目标。配置按内容键加锁、校验并复用，本地和远程共用配置解析。
离线必须有该工作空间已保存的观测，输出原观测时间，网络失败不静默转离线。
目标解析只准备元数据。DB2、SQL、资产检查及原始导出共用 `records.Reader.ReadFile`，
以 `--installation` 或 `--cdn` 选择实际来源；来源变化不复制查询、解码或证据逻辑。
CDN 内部处理索引、范围缓存和校验，skill 不编排这些细节。`--offline` 禁止网络，
不改变固定身份或静默切换来源。

`asset export --output <file>` 在同一读取链路取得完整原始字节，将来源及导出清单
归档，然后在输出父目录暂存并原子发布文件；默认不覆盖，显式 `--overwrite` 才
替换普通文件，不截断原文件或修改其其他硬链接。父目录必须存在，输出不得落入
受管理工作空间。manifest 记录输出路径、编码、字节数/摘要及来源 capture；
归档完整只证明清单和内容完整，不证明外部路径仍存在，也不单独证明文件已发布。
命令成功才确认发布完成。输出丢失时可检查目标文件及摘要，不盲目覆盖重试；
进程被强杀可能留下隐藏暂存文件。

同一导出用例接受 `--encoding raw|png|webp`，默认 raw，不按扩展名猜测。
BLP2 解码在 `records/texture` 内完成，只有接收字节、mip 与像素预算并返回像素的
小接口，不拥有下载、工作空间或游戏状态。图像转换复用原始读取和发布链路；
清单分别记录原始 source、编码后 content、转换参数和 RGBA 像素摘要。
PNG 与无损 WebP 返回原始、清单、编码产物三份证据，不建立另一套图像任务系统。
`--mipmap` 选原有层级，`--channels` 选择实际输出通道，单通道显示为不透明灰度。
像素预算在分配前检查，输出也受字节预算约束；WebP 编码器内部缓冲，取消在其
编码前后检查，不宣称硬 CPU 截止或总内存上限。文件名检索和视频 demux 仍需实现。

`desktop` 不依赖业务模块。`live` 集中拥有窗口会话、外部输入、协议推进和恢复；原生输入/捕获与模拟器在实际 I/O 处替换。其他模块优先使用具体类型，仅在真实变化点和外部依赖处增加接口。

业务模块不依赖 Cobra，不打印 stdout，不调用另一个 CLI，不自行读取环境变量和用户根目录。CLI 解析参数并直接调用完整用例，不设置另一层 actions 总管。各命令只初始化实际使用的模块；数据和源码查询不创建游戏会话。共享存储只封装存储，不解释游戏阶段。

## 4. 统一业务对象

| 对象 | 所有者 | 必须成立的不变量 |
| --- | --- | --- |
| `SelectionSpec` | selection | 用户意图，允许命名配置，但没有静默的产品默认值 |
| `PinnedSet` | selection | 已解析的不可变引用集合；以内容摘要标识，新增引用产生新集合 |
| `SourcePin` | selection | 仓库身份、分支来源、精确 Commit、解析器版本；Tag 只是可追溯标签 |
| `DataPin` | selection | 产品、地区、完整 Build、CASC 配置键、语言及 DBD 引用 |
| `ChangePin` | selection | Hotfix 提供方、产品、Build、地区、查询范围、抓取时间和内容摘要 |
| 连接记录 | live | 安装身份、PID、进程启动时间、HWND、可执行路径、角色及捕获区域；是重连目标，不是永久输入权限 |
| 操作记录 | live/journal | operationId、固定请求及已确认进度；唯一的持久推进依据，不成为所有查询的任务模型 |
| `CaptureRef` | evidence | 内容摘要、类型、来源定位、固定引用、完整/部分状态；不能仅有二维码 |

用户可维护可变目标配置；命令开始时解析并固定实际引用。简单调用可以使用项目目标，多 agent 工作流显式传递返回的固定 snapshot。固定引用之后不重新解析 latest。添加源码或 Hotfix 保留已有固定内容。源码、数据和实时客户端分别记录身份，不强迫它们共享一个版本号。

源码 Commit、游戏 Build、插件版本和 Hotfix 时间分别存储。对应关系明确为 `exact`、`compatible`、`mismatch`、`unknown`；精确匹配无法获得时返回可操作错误，不自动借用最新源码。

纯源码研究只要求 SourcePin；纯数据查询只要求 DataPin；离线证据读取无需实时窗口。客户端目录只表示位置，身份依次来自本地产品元数据和 Build，目录名仅作有证据的最后回退。

产品映射、地区映射、支持能力和 TOC Interface 的已验证事实来自统一目录。生成 Go/Lua/文档需要的视图，禁止 skill 手写第二份事实表。复用测试轨道必须核对其实际内容。

## 5. 模块对调用方的承诺

接口以调用方完成任务需要知道的信息为准，不预设一张类名/方法名清单。下表是职责合同；内部函数按实现命名，不要求为了新名字发明抽象。耗时入口接受 `context.Context`，模块关闭自己创建的 I/O 资源。

| 模块 | 调用方指定 | 模块负责 |
| --- | --- | --- |
| selection | 用户目标或固定引用 | 名称、项目配置及版本来源解析；输出确定的上下文 |
| codebase | 查询、文件范围、比较版本或插件路径 | 按需准备源码/索引，查询与验证并返回来源 |
| records | 数据问题或导出请求 | 本地/CDN 访问、格式解码、关系查询与资源输出；Hotfix 独立标注 |
| live | 连接和有界请求；恢复只需操作 ID | 身份重验、单窗口执行、协议、报告归档及可恢复收尾 |
| vault / evidence | 对象或结果及来源 | 原子存储、引用保留和完整性读取；不调度游戏 |
| delivery | 安装目标及发行版本 | 目标发现、内容验证、替换和恢复；不导入旧数据 |

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
| `live` | `instances / connect / bind / run / bugs / reload / status / resume / cancel`；`instances` 为发现+身份识别，`connect` 为整任务自动连接 |
| `evidence` | `list / show / verify / bundle / keep / remove` |
| `cache` | `status / verify / prune` |
| `addon`、`skill` | 各自 `install / status / remove` |

参数规则：`--home` 是完整工作空间根；`--snapshot` 接受 PinnedSet ID；`--session` 接受 WindowBinding ID；`--output` 是明确的产物位置；`--format` 统一为 text/json/jsonl；`--offline` 阻止联网。CSV 属于导出编码，通过 `--encoding csv` 选择，附带机器可读 manifest。

目标选择顺序为显式固定快照、显式目标配置、项目锁定。唯一匹配可以自动解析；歧义才要求选择。不使用全局“最后操作窗口”。连接建立后，run 只引用 session；安装目录、角色、nonce、截图区域不成为每次运行的参数。resume 只引用 operation ID，不能覆盖原请求身份。

已实现的项目入口：`project init --product <track> [--path <directory>]` 创建
`lycheedev.json` 产品声明，`project lock --snapshot <pin> [--path <directory>]`
保存完整固定集合，`project status` 只读检查。lock/status 未指定路径时发现当前目录
向上的最近项目。消费 `--snapshot` 的查询和 live bind 在省略该参数时使用
`--project <directory>` 或最近项目锁；显式 snapshot 优先且不读取项目文件。
live run/resume、双端 source diff 和证据读取不受项目默认值重定向。
锁不保存账号、安装路径或窗口，只保存固定引用；导入新 workspace 时验证内容 ID，
原始内容和父集合历史不随锁复制。缺失内容仍由已有准备/读取模块处理，不另造项目下载器。
项目写入按目录持有 OS 锁，声明不覆盖，更新锁文件通过临时文件发布；
`.lycheedev-locks/` 是本机协调文件，不提交版本控制。命名目标配置仍待实现。

首次连接由统一的“发现 → 身份识别 → 选择 → 自动连接 → 会话复用/恢复”用例完成。
这是用户 2026-09-23 的明确修订：替代此前“宿主只观察、不主动启用连接”的旧约束，
不得再要求用户手打 `/dev connect` 或识别指令。`live instances` 是发现入口，区分
“已安装”与“正在运行”：安装扫描只覆盖受支持产品与已知安装根，不做无界磁盘遍历；
在线候选逐窗口、逐个触发一次性身份标记（机器入口 `/dev bridge identify
<probeNonce>`，只回显产品/Build/角色/服务器/guid 观察与输入就绪事实，不建立会话、
不持久启用桥接、不执行任何调查探针），读取一轮后把有意义的
“产品/Build—角色—服务器—必要时安装位置”交给 skill 展示。识别与任务执行严格
分开：扫描期间未做选择，不构成对任何候选执行调查探针的授权。一扇窗口识别失败
不影响其他候选；身份缺失如实留空并给出明确状态（busy / no_actor /
identity_unreadable 等），不臆造角色信息；插件未加载、登录界面、黑屏、身份不可读
均给出明确状态，不随机选择或无限重试。同 Build、同名角色或同服务器的多个窗口保持
独立候选，PID/HWND 只是内部参数；唯一且匹配的候选自动选中，真实歧义才由用户选择，
绝不按序号猜测。用户选定后宿主重新核对该候选仍是原进程（PID/启动时间/HWND/路径）
与同一角色（guid），再自动完成 `/dev connect`、以新的 ready 画面核验并保存会话；
会话标识由插件生成，不作为 CLI 参数。`live connect` 是该完整用例的唯一入口
（约束参数 character/realm/pid/installation，或 `--session` 复用/重建）；`live bind`
保留为“只观察已显示回执”的手工路径，不获得新的发现状态。后续 agent 复用同一有效
选择与会话；进程重启、角色改变或断线由同一流程重建，仅剩真实歧义才再问；未决操作
仍由 `live resume <operation-id>` 从原记录恢复，不把重连做成重跑探针。扫描不得
覆盖其他 agent 正在显示的执行/完成回执或抢占其窗口所有权，必要时标记 busy/unidentified
并继续其他候选；多窗口输入逐个协调，不并发乱发按键。默认捕获选定窗口，可用
`--capture-area` 显式限制区域；`--region` 只表示数据地区。连接输出只显示连接 ID、
实际客户端/窗口、角色、目标和证据引用，协议字段保留在归档中。初次发现不放宽后续
执行及恢复的精确身份匹配。该流程仍需游戏实测，不将自有原生窗口或四端 Lua fixture
当作真机证据。

运行只需 session 和探针文件。宿主根据已验证角色及服务器，在选定客户端的
`WTF/Account` 下查找精确角色目录；唯一匹配的账号固定进原操作请求，恢复不重选。
这里只读取目录元数据，不读取旧 SavedVariables 作为账号证据。路径只是报告位置，
不是登录身份认证；落盘报告仍须通过请求/会话/正文校验。没有匹配、多账号匹配
或扫描预算超限时，在创建操作和游戏输入之前请求显式 `--account`，不选第一个或
最新文件。显式账号也支持首次登录尚无角色目录的情况。

源码验证支持单 TOC 和 `--matrix <file>`。移除旧工具独立的 install/update/profile/doctor/cache 生命周期，保留一份 Toolkit 行为。npm 自身更新交给 npm；独立发行包更新使用发行清单对应的安装流程。

示意工作流（ID 由上一步真实返回，以下不是可直接执行的现有命令）：

```text
lycheedev target resolve --target retail-cn --format json
lycheedev source query C_Spell.GetSpellInfo --snapshot <pin> --format json
lycheedev data sql --file query.sql --snapshot <pin> --format json
lycheedev live bind --pid <pid> --snapshot <pin> --format json
lycheedev live run --file probe.lua --session <binding> --format json
lycheedev live resume <operation-id> --format json
lycheedev evidence bundle --ids <capture-a>,<capture-b> --output ./report
```

## 7. 统一结果与证据

JSON 外层字段固定为 `schema`、`ok`、`operationId`、`context`、`result`、`captures`、`warnings`、`error`。context 保存实际解析后的引用及匹配关系；error 包含 `code`、`message`、`stage`、`retryable`、`resumeOperationId`。恢复信息是受约束的数据，不是可以从外部响应直接执行的 shell 字符串。

- `ok` 表示请求的操作合同是否完成。游戏任务另有 `result.probeStatus`；探针报错也可以完成采集与收尾。
- 游戏结果独立报告 `report.state`（unavailable / verified）及完整正文、来源 capture；收尾报告 `cleanup`（pending / complete）。`complete` 仅在报告验证与收尾都成立时为 true。这些是同一操作记录及归档的读取视图，不新增一套持久状态。
- 报告已验证但清理未确认时，仍返回可用报告与恢复 ID，不把报告描述为失败，也不输出整个操作完成。协议内部 observation/nonce/输入消息计数不作为普通结果暴露。
- `result.complete`、`truncated` 和截止原因不能省略；结果上限触发时不可把部分查询描述为完整结果。
- JSONL 使用带类型的 begin/record/end/error 帧；只有合法 end 且退出 0 才是完整成功，broken pipe 不伪造完成。
- 退出码：0 完成，2 参数/选择问题，3 能力/环境不满足，4 输入数据或协议无效，5 外部失败，6 未决或收尾未完成，7 显式取消，8 内部错误。具体根因以稳定错误码为准。
- stdout 只承载指定格式；进度和日志走 stderr；帮助和参数错误也遵守格式约定。

查询记录保存固定引用和来源。小结果可完整归档，大结果以流式产物及 manifest 保存；不为了统一证据而复制整个 DB2 库。预算不足时报告部分状态或明确失败。

Capture 使用 `CAP-<id>`，operation 使用 `OP-<id>`，窗口绑定使用 `SESSION-<id>`，固定集合使用 `PIN-<digest>`。摘要校验描述原始字节，不通过 JSON 重排后再声称字节相同。

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

下面是当前传输实现的内部步骤，不是面向用户的合同，也不是必须永久保留的架构。输入加载与输出持久化分属不同效果，不能伪装成一次成功：

```text
prepared -> load_requested -> loaded -> dispatch_requested -> reported
         -> flush_requested -> persisted -> verified -> ack_requested
         -> acknowledged -> cleaned
```

已有内置错误快照不需要临时探针加载。每次外部副作用前持久记录 intent，回执后再确认阶段。主状态另有 pending/running/unresolved/failed/cancelled/completed，不将所有失败压成一个布尔值。

收敛要求：live 单独拥有推进和恢复；每个已提交动作只有一个权威记录，不允许 Stage、观察布尔标记、会话副本和归档各自成为独立状态机。历史证据负责核验，当前输入资格仍由新画面确认。Run 与 Resume 进入同一执行内核。

同一事务保存的证据引用不再另配 requested/prepared/completed 布尔值：输入前
提交的 readiness 引用表示该动作已进入不可自动重发的区间；发送回执只记录诊断，
缺少回执不能推导为没有发送。队列修改先记录意图，再保存修改后的内容摘要；摘要
缺失时只允许核对并完成幂等文件修改，不证明游戏已加载或卸载。阶段表示协议进度，
执行状态表示正常或未决，两者不各自驱动另一套执行器。

报告获取和数据回收分开设计。优先验证“有界保留 + 确认 + 后续安全回收”，去掉仅为立即删除已归档报告而进行的重载；在真机与故障恢复证据证明替代方案前，保留现有清理路径，不能直接删除安全检查或宣称已减少重载。清理未完成不能抹去可用结果，也不能绕过尚未释放的窗口所有权开始新执行。

- 启动任务分配新请求 ID；恢复沿用原请求 ID、代码摘要和原重载 nonce。
- 预期 reload 会改变插件的 Lua 会话标记；只有匹配原 reload nonce、相同进程/角色/Build 的 ready 信号才允许在原操作内更新 WindowBinding。无此关联的身份变化使绑定失效。
- 宿主在 PostMessage 成功后崩溃属于结果不确定；恢复先观察和核对，不自动补发执行。
- 不以“文件 mtime 改变”代替精确报告校验；读到半写文件时有界等待，保留原 intent。
- 不以“没有捕获到画面”代替“ACK 标记消失”；清理需要有效画面或对应协议证据。
- `Resume` 只执行尚未完成且能够证明安全的阶段。无法判断时保留未决操作，不重发输入。
- 取消能力仍需实现：只能尝试协议支持的取消，读取确认并清理自有资源；不承诺强制中断任意同步 Lua 或杀死游戏进程。
- 同步 Lua 无法被宿主可靠抢占，超时只终止宿主等待。skill 必须生成有界探针。
- 完整正文保持 SV 通路，受限解析器不执行 Lua；校验深度、字节预算、编码、摘要和引用身份。

Go 原生输入继续使用指定 HWND 的 `PostMessageW`，正确组装键盘参数及 UTF-16 字符。逐次发送重新核对身份；后台模式不调用前台激活、剪贴板或 SendInput。发送成功仅表示入队：[Windows 接口说明](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-postmessagew)。

后台输入的前置合同包含输入资格：启用后的插件在新协议信号中报告可验证的输入状态，宿主使用新到达帧核对身份与资格。无法确认当前编辑状态、自定义聊天键或已有草稿时拒绝自动提交，不以 Escape 入队代替确认。具体可观察的输入状态与事件须按四端精确源码验证，作为 S1 的技术门槛；不能假设宿主可以原子隔离人工操作。自动化模块默认关闭，首次启用需显式操作；没有有效就绪信号时返回未启用或未就绪，而非猜测发送。唯一的受控例外是首次接入的 bootstrap 输入（2026-09-23 修订）：宿主可向尚未识别的候选窗口发送固定的 `/dev bridge identify <nonce>` 与 `/dev connect` 两条协议命令，逐窗口、逐条、有界、持窗口输入锁、每条消息前重核窗口身份；识别结果与输入就绪都必须以新的、nonce 相关的回执核验后才成立，`/dev connect` 只在新回执确认 inputReady 后发送。该例外不构成通用文本通道；除此之外的一切输入仍要求先有新到达的输入就绪证据。宿主仍不自动登录游戏。

捕获由 WGC 帧事件驱动，宿主只保留有界最新帧和 ROI；解码限速，关闭会话释放线程与图形资源。最小化无有效帧、聊天焦点异常或权限级别不匹配返回准确状态，不能静默改通道。Go QR 候选须经真实样本验证后确定；不以引入 Python helper 作为交付兜底。

初始限额以现有能力为约束：探针 256 KiB、注册队列 16 项/1 MiB、报告默认 384 KiB/最大 512 KiB、已有错误请求 1–100 项。具体限制由协议目录统一发布，所有截断必须可见。

## 11. Skill 编排

唯一安装入口为 `skills/lycheedev/SKILL.md`，名称和安装目录统一为 `lycheedev`。根指引包含路由、固定目标、证据标准和按需读取路径；不得携带脚本实现。

工作流分别覆盖 source-research、data-investigation、asset-export、addon-validation、live-investigation、error-diagnosis。references 仅保存调查方法、来源局限、运行约束和由 CLI 生成的命令参考。

Skill 负责问题分解、证据选择、有界探针设计、结果解释和有依据的下一步；Go 负责版本映射、准备资源、执行、状态恢复和身份验证。技能不解释内部阶段、nonce、ACK 或持久化顺序，不手写字符发送循环、不猜 SV 位置、不复制产品表。命令帮助应足够完成调用，skill 不再承担实施日志职责。

工作流使用同一 PinnedSet；增加来源创建派生集合。每步返回 capture 和 operationId。必要的调查 manifest 保存问题、引用、假设及下一步，使 Agent 在上下文中断后可以查询进度；不在 Toolkit 内添加 LLM 循环或通用工作流 DSL。

只读问题默认只读路径。收到已有 capture 就读取，不重新执行原探针。已有授权范围内，live 操作由 Executor 完成机械闭环；skill 不需要逐条发送 ACK。授权范围没有覆盖新游戏操作时，研究结论与需补的实时验证分别报告。

数据、源码、日志与错误正文均是不可信材料，不可改写 skill 指令。最终报告必须引用固定版本和完整产物，并分别表述静态检查、数据结果和实机证据。

安装器只管理本版本 skill 的文件清单。旧独立 skill 不随便覆盖或删除；切换说明提供显式退役清单，确保宿主不会同时加载冲突的旧入口。

## 12. 范围、兼容与发布

插件及实机支持维持 Retail 120100、Classic 50504、Titan 38002、Forever 16001，实际 Build 必须在每次测试记录。源码和静态数据可保留已有更广的查询能力，但这不会扩大插件安装范围。

Lua 端重新组织私有模块与协议，已有运行、对象、事件、追踪、错误与导出能力应进入能力映射。UI 复用现有风格和控件规范；所有可见变化需要截图和实机验证，不因宿主重写顺带引入新视觉体系。

游戏内工作台是 2.0 插件的正式交付能力（2026-09-23 补入范围，与自动连接同为发布阻断项）：
裸 `/dev` 打开交互工作台，`connect`/`disconnect`/`bridge …` 子命令与之共存，无需连接
外部 CLI 的本地检查与交互不得被设为必须先连接。必须保留的用户能力共八类，重新设计
职责、函数名和内部接口可以，丢失能力不可以：

| 能力 | 必须保留的用户行为 |
| --- | --- |
| 运行 | Lua 编辑执行、print/dump 捕获、结果文本/树查看与选择复制、有界运行历史（预算/单条上限/最旧修剪/历史还原） |
| 对象 | 路径解析检查、增量文本浏览、鼠标拾取、有界搜索、节点文本查看与逐节点导出 |
| 事件 | 每客户端事件目录与筛选、选择语义、有界监听（记录环/参数上限）、载荷详情 |
| 追踪 | 函数路径追踪启停、调用记录与参数详情、有界记录环 |
| 诊断 | 错误采集（BugGrabber 为采集源，软集成、缺失明确不可用）、作用域/关键字筛选、Agent 报告详情、有界错误快照 |
| 导出 | 各来源落盘导出、证据信封与票号（单调不复用）、预算与 protected/pending 生命周期、记录页查看/复制/删除交互 |
| 自动化 | 队列/执行/历史的游戏内查看与执行体验（与 CLI 驱动共用同一游戏侧能力）；不恢复任务注册表兼容面 |
| 关于 | 版本/环境信息、双语界面（zhCN 基表 + enUS 覆盖）、完整导航 |

游戏内工作台与外部 CLI 使用统一的游戏侧能力实现：UI 不复制执行/诊断逻辑，CLI 不用通用
Lua 入口冒充已有的专业功能；不能简单挂回整套旧插件来凑齐功能。新数据模型
（`LycheeToolkitDB`）不导入 `LycheeDevDB`/`DumperDB` 旧用户数据。全部能力继续遵守
禁用零开销（首次 `/dev` 前零 frame/事件/hook）、事件驱动、战斗与 secret 值处理、
无污染、稳定布局与双语一致规则；逐项验收矩阵见 regression.md 的 WKB 用例。

插件发行 ZIP 顶层仍为 `Lychee Dev/`；四个 TOC 选择匹配客户端配置和事件目录，并加载相同顺序的共享模块。发布内不得出现本地探针、账号信息、调查记录、测试数据或空壳兼容入口。

本次 release 固定为 2.0.0、tag 固定为 v2.0.0，协议和工作空间 schema 独立版本化。二进制、插件、skill、单一 npm 包与平台专用原生压缩包通过同一发行 manifest 校验；doctor 按实际能力报告。CGO_ENABLED=0 是发行目标，必须先通过 Windows 捕获和解码验证，不能在尚未证明时宣称已达到。

Windows CI 是硬门槛，不能用 Linux 交叉编译代替；实际 npm tgz 必须先安装验证再原样发布。OIDC、版本检查、required jobs、实机证据、失败恢复和发布顺序遵守 [2.0.0 发布规范](release-2.0.0.md)。

许可证是发布前的明确交付项：wowdata 当前声明 AGPL-3.0-or-later，wowdoc 与旧 Lychee
npm 包声明 MIT（新 `lycheedev` 包在许可裁决前为 UNLICENSED/private）。2026-09-23
许可闭环审计已完成第三方清单与逐文件派生分类（详见
`THIRD_PARTY_NOTICES.md` 与 `packages/npm/lycheedev/THIRD_PARTY_NOTICES`）：wowdata
派生代码已逐文件标注 AGPL-3.0-or-later，wowdoc 派生代码保留 MIT 署名；combined-work
发行方案由 owner 裁决（保守路径为 AGPL 合并作品 + 对应源码交付）。不因改名而丢失
署名或许可义务。

设计通过条件是：[回归矩阵](regression.md) 中强制场景全部有可追溯结果，且[实施路线](roadmap.md)中的旧能力均完成映射。完成接口迁移后删除替代掉的旧代码与旧内部结构测试，不在新产品内长期维持两套内核。
