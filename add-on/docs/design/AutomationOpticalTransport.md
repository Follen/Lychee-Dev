# 自动化任务与重载落盘方案

状态：插件与 Python 脚本已按本方案实现，完成了独立复核、离线端到端验收，并在 Retail `12.1.0` build `69587` 上跑通一次真机端到端链路（2026-09-13；证据见第 12、13 节）。仍待覆盖的真机项见第 11、13 节末尾。

本方案根据用户最新决定全面修订：所有结果统一通过 `/reload` 写入 SavedVariables；执行中显示色块，完成后显示一张简短通知二维码，Python 识别通知后重载并读取对应 Ticket。本文替代此前的二维码结果分片及按结果大小分流方案。

文档位于现有 `lychee-dev-performance` worktree，属于 `codex/remove-performance` 分支。保留已完成的 Performance 移除与 **Lychee Dev skill** 命名。实现已落地：插件 Lua、Skill 脚本与 `references/automation.md` 均已写入，依赖按第 6.1 节锁定版本安装并用于离线核验；真机结论见第 13 节。

## 1. 整体流程

系统由三个载体组成：Lychee Dev 插件、**Lychee Dev skill**、Skill 内的 Python 脚本。Agent 准备任务，Python 投递并读取结果，插件执行并记录证据。

```text
Agent 准备任务
    ↓
Python 更新 auto/auto.lua 中对应 task ID 区块
    ↓
输入侧 /reload：加载任务定义，但不执行
    ↓
/dev auto run <task-id>
    ↓
执行中：显示色块
    ↓
执行结束 → 清理探针 → 冻结报告 → 写入内存 SV 与任务索引
    ↓
完成通知二维码：Ticket + task ID + 执行 ID + 时间戳
    ↓
Python 识别并校验通知，记录即将执行的 reload
    ↓
输出侧 /reload：由 WoW 将 SavedVariables 写入磁盘
    ↓
Python 解析 SV → 找到对应 Ticket → 核对身份与正文完整性
    ↓
保存本地结果与日志 → Agent 读取完整报告
```

`/dev auto bug <n>` 是内置任务，直接快照已有错误记录，省去更新任务文件和输入侧重载；完成后走完全相同的通知、输出侧重载、SV 读取流程。

每个新结果均通过输出侧重载落盘，结果大小不决定传输方式。二维码不承载报告正文，不需要分片、轮播、重组或压缩传输。外部读取成功记在 Python 日志里，无需额外向游戏发送 ack，也不为确认状态再重载一次。

## 2. 两个主要入口与任务区块

### 2.1 任务定义文件

固定文件为 `add-on/Modules/Automation/auto/auto.lua`，归属 Automation 模块，列入全部三个 TOC。发布时该文件只包含空登记表。安装目录中的文件可由 Skill 编排 Agent 更新任务区块。

任务源码作为转义后的字符串保存，加载文件时只登记数据。登录、重载、打开页面均不能执行任务。以下为结构示意，字段与已实现写入器一致（`Modules/Automation/auto/auto.lua` + `registry.py`）：

```lua
local ADDON_NAME, ns = ...
ns.AutomationTaskDefinitions = {}

-- BEGIN LYCHEE DEV TASK example-task
ns.AutomationTaskDefinitions["example-task"] = {
    schema = 1,
    requestId = "host-generated-unique-id",
    revision = "source-revision",
    kind = "lua",
    createdAt = 0,
    expiresAt = 0,
    sourceBytes = 0,
    sourceChecksum = "checksum-of-source-bytes",
    source = "return { ok = true }",
}
-- END LYCHEE DEV TASK example-task
```

- task ID 使用 `[A-Za-z0-9_-]{1,64}`，标识所属任务区块，可读且不含区块分隔符。
- `requestId` 标识该区块的一次执行请求。由 Python 生成唯一值；更新源码或主动重跑必须使用新的请求 ID。
- `revision` 和源码长度/校验和用于确认加载内容；描述表还包含预期客户端身份、到期时间及输出上限。
- 相同 `requestId` 在当前会话中只能执行一次；再次调用返回已有状态或完成通知，不重复执行源码。
- 一个 task ID 可用于后续修订，但历史记录按执行 ID 保存，不能被同名区块覆盖。历史中同时保留 task ID、执行 ID 和 revision。
- 生成的 Lua 外壳保持 Lua 5.1 兼容及 ASCII；源码中的引号、换行、非 ASCII 字节和分隔标记由写入器正确转义。

### 2.2 多 Agent 写入方式

Agent 各自负责对应 task ID 区块，但通过同一个 Python 写入入口更新文件：`task upsert --id ... --source-file ...`。不能并行整文件覆盖。

写入器获取安装目录级操作系统独占锁，读取最新文件，核对原 revision，只替换目标区块，保留其他区块与顺序。随后验证生成语法和总量，在同目录写临时文件、刷新并原子替换，最后记录新文件哈希。进程退出时锁自动释放。

解析器只接受自身生成的数据语法，不执行 Lua。重复 ID、损坏标记、revision 冲突或路径越界时明确失败并保留旧文件。解析实际安装路径时处理符号链接和目录联接。

初始上限：16 个区块、登记表合计 1 MiB、单任务源码 256 KiB。可以批量更新区块后统一进行一次输入侧重载。移除任务必须明确指定区块；不能清理其他 Agent 正在使用的任务。发布打包始终生成空登记表，不把本地调查源码装进 ZIP。

### 2.3 命令约定

| 命令 | 行为 |
| --- | --- |
| `/dev` | 保持现有窗口切换行为。 |
| `/dev auto run <task-id>` | 验证已加载区块，执行其当前请求；没有加载该 ID、已过期、身份不符或系统忙碌时明确失败。 |
| `/dev auto bug <n>` | 导出最近 n 条已保存错误记录，自动生成本次任务和执行 ID。 |
| `/dev auto bug <n> <request-id>` | Python 可选用的关联形式，由外部提供唯一请求 ID；简短用户命令仍然有效。 |
| `/dev auto status <task-id>` | 查询状态；已完成且结果仍存在时，可重新显示同一完成通知。查询不执行源码。 |
| `/dev auto show <ticket>` | 为保留的历史结果显示通知二维码，用于恢复读取，不重新执行原任务。 |
| `/dev auto cancel <task-id>` | 请求协作式取消，清理自有工作，尽可能保存取消报告并显示其 Ticket。 |
| `/dev auto stop` | 隐藏通知 UI，不表示结果已读取，不取消运行中的任务，也不删除待落盘记录。 |

严格验证参数数量和内容后才激活功能。无参数 `/dev` 不进入自动化流程。同一时刻只允许一个执行在跑；已完成但尚未收到主机读取回执的结果最多积压 3 条（`MAX_UNACKNOWLEDGED_RESULTS`），达到上限后新请求返回忙碌状态。主机可用 `/dev auto ack <ticket> received|failed` 立即释放名额，无需重载。

## 3. 执行生命周期与错误导出

### 3.1 执行与完成

普通 Run 的 `ns.Execute` 会在 44,000 字节处截断输出，不能用于完整自动化结果。新增独立执行上下文，提供完整输出收集、`Finish(value)`、`Fail(message)`、取消状态和清理注册。

同步任务返回即结束；异步任务必须在事件回调中显式完成或失败。异步回调单独捕获错误，不能依赖最外层 `pcall`。首次进入终态后冻结结果，迟到回调和重复完成调用无效。

执行结束后的顺序固定为：停止自有事件/回调/计时工作 → 生成最终报告 → 保存证据与任务索引 → 保护待落盘 Ticket → 显示完成通知。只有这一步成功后，Python 才可以执行输出侧重载。成功、失败、取消均可生成结果 Ticket，通知本身不代表执行成功。

任务上下文允许访问 WoW 全局对象，与现有运行器类似，并非安全沙箱。Agent 必须生成有明确边界且可清理的调查代码。任意死循环不能被此架构强制中断，任意外部写入也不能自动回滚；不能使用调试钩子假装实现强制超时。

默认通过游戏事件完成调查，不轮询完成状态。确需测量时间区间的探针使用有明确期限、可取消的机制。主机等待超时只表示等待结束或状态未知，不能自动重跑任务。TTL 在任务入口检查，无需插件常驻定时器。

### 3.2 错误记录来源

现有 `Modules/Diagnostics.lua` 读取 `BugGrabber:GetDB()`，三个 TOC 均依赖 `!BugGrabber`。错误页面已经提供正文、调用栈、局部变量、时间、会话和重复计数。

在 Diagnostics 增加 `SnapshotRecentErrors(n, scope)`。错误提供方查找及字段兼容集中在该模块，Automation 不另建错误采集器，也不需要打开 Errors 页面。

`/dev auto bug <n>` 的默认语义：

- n 必须是 1–100 的整数，不静默修正无效值。
- 范围为提供方当前保留的全部会话；按现有存储倒序选择最多 n 条。
- 报告明确记录 `ordering = "provider_storage_reverse"`。实施时核对已安装 BugGrabber 的真实排序；不能把插入顺序冒充最后发生顺序。
- 立即复制 `message`、`stack`、`locals`、存在时的 `source`，以及 `time`、`session`、`counter`，避免后续错误更新改变快照。
- 补充请求数量、返回数量、可用记录数、当前会话、提供方版本、客户端环境和快照时间。
- 零记录是成功的空报告；提供方不存在或调用失败是独立错误。
- 不清空、不重置错误，不改变 BugGrabber 状态。

重复错误可能聚合为一条带计数的记录。因此 n 表示已保存记录条数，不能还原未保留的每一次异常。缺失的栈或局部变量标记为缺失，不编造。

机器导出不调用可能截断字段的 `FormatAgentReport`。读取秘密值之前先检查，无法取得的字段要使报告明确标记为不完整。结构化字段规范化保存，不能用 `tostring(table)` 替代内容。

## 4. 色块和单张通知二维码

### 4.1 状态显示

左上角保留三个初始尺寸为 4×4 物理像素的色块，以 RGB 顺序表示执行中。范围包含探针运行、报告整理和保存准备，不能提前表示完成。

结果登记成功后隐藏执行色块，在左上角同一位置显示一张静态完成通知二维码（色块让位，二者不同时占用该角落）。无需结果就绪后轮播，不需要每秒切图，也不需要显示计时器。二维码保持到输出侧重载、显式隐藏或安全清理。

如果连最小失败报告都无法保存，不能显示包含虚构 Ticket 的完成二维码。页面显示明确错误，可将色块改为约定的错误状态；Python 记录通知失败或超时，不擅自重载并宣称完成。

色块只帮助识别状态，不能用于推断 Ticket。Python 只有在识别并校验完成二维码后才能发起结果落盘重载。色块布局、物理像素映射和容差需要按 DPI/UI 缩放实测。

### 4.2 二维码内容

通知采用简短 UTF-8 JSON，固定协议版本，不做压缩。字段示例：

```json
{"v":1,"ticket":"LYCHEE-20260912-180000-0001","task":"inspect-example","run":"unique-request-id","ts":1789207200}
```

| 字段 | 含义 |
| --- | --- |
| `v` | 通知协议版本，首版为 1。 |
| `ticket` | 完整结果在 SavedVariables 中的唯一查找键。 |
| `task` | 原始 task ID；内置错误任务也有明确任务标识。 |
| `run` | 本次执行请求 ID，区分同名任务修订、重跑和旧画面。 |
| `ts` | 对应证据记录的创建时间戳；用于交叉核验，不单独作为身份依据。 |

保留 `run` 是为了避免同名任务和旧 Ticket 混淆。其余字段、执行状态、环境、正文长度和校验和都放在 SV 记录里。通知不携带报告、源码、文件路径、分片信息或外部命令。

限制 JSON 为 512 字节以内，各字段有独立长度与类型校验。通知超限时明确失败，不静默截短身份。使用普通二维码字节模式和 M 纠错等级，尺寸由实际矩阵及校准像素比例决定。

### 4.3 复用现成二维码代码

参考 `D:/Code/Lychee/Addon/Lychee/Libs/qrencode.lua`，保留许可与署名，改为命名空间内的按需初始化。现有库加载时构建 256×256 查找表，且存在需检查的全局集成/赋值，必须避免禁用时的额外成本。

显示参考 `Addon/Lychee/Modules/QRCode/Display.lua` 的矩阵方向 `matrix[x][y] > 0`。现有实现每次新建黑块纹理，改为有上限的复用池，并保留至少四模块白色静区。通知只需在内容改变时编码和渲染一次。

本版不需要 LibDeflate、报告压缩容器或二维码数据分片。保留已有源码作为参考即可，不引入无用途的运行时依赖。

## 5. SavedVariables 数据库规划

### 5.1 总体原则

完整结果继续使用已有证据仓库：

```text
LycheeDevDB.exports.records[TICKET]
LycheeDevDB.exports.records[TICKET].payload.content
```

不新建第二套结果数据库，不将完整报告重复存入普通 Run 历史。新增 `LycheeDevDB.automation` 只管理任务执行索引。任务源码仍以任务文件和主机源码产物为准，数据库保存其身份、revision 和校验信息。

顶层 schema 从 7 升至 8；`exports.version` 保持 2，证据外壳继续使用 `lychee.evidence.v1`。自动化索引独立设置版本 1，报告正文使用 `lychee.automation.result.v1`。

### 5.2 结构示意

下面展示字段关系，示例值不代表真实记录：

```lua
LycheeDevDB = {
    schemaVersion = 8,
    history = {},
    exports = {
        version = 2,
        nextId = 1,
        totalBytes = 0,
        order = { "LYCHEE-20260912-180000-0001" },
        records = {
            ["LYCHEE-20260912-180000-0001"] = {
                schema = "lychee.evidence.v1",
                ticket = "LYCHEE-20260912-180000-0001",
                createdAt = 1789207200,
                source = {
                    kind = "automation_result",
                    title = "inspect-example",
                },
                payload = {
                    mediaType = "application/json",
                    encoding = "utf-8",
                    content = "<complete UTF-8 JSON report>",
                    byteCount = 0,
                },
                environment = {},
                metadata = {
                    taskId = "inspect-example",
                    executionId = "unique-request-id",
                    revision = "source-revision",
                    resultSchema = "lychee.automation.result.v1",
                    status = "succeeded",
                    complete = true,
                    checksumAlgorithm = "adler32",
                    contentChecksum = "00000000",
                },
            },
        },
    },
    automation = {
        version = 1,
        nextLocalRequestId = 1,
        order = { "unique-request-id" },
        records = {
            ["unique-request-id"] = {
                taskId = "inspect-example",
                executionId = "unique-request-id",
                revision = "source-revision",
                kind = "lua",
                status = "succeeded",
                startedAt = 1789207190,
                finishedAt = 1789207200,
                ticket = "LYCHEE-20260912-180000-0001",
                resultAvailable = true,
                errorCode = nil,
            },
        },
    },
}
```

索引以执行 ID 为键，`order` 最新在前；task ID 只是原始任务关联，不能作为覆盖历史的主键。待运行/运行中条目可以暂时没有 Ticket。内置错误任务使用同一结构，`kind` 为 `bug_snapshot`；证据 `source.kind` 可沿用已有 `error_log`，正文仍明确标注自动化报告版本。

`environment` 复用现有客户端版本、build、Interface、语言等字段，并在报告中加入本次角色/服务器身份，用于核对配置选中的客户端。新增游戏接口必须验证全部目标版本。

### 5.3 完整报告正文

`payload.content` 保存一份完整 UTF-8 JSON 报告，包含：

- 报告版本、task ID、执行 ID、revision、任务类型。
- 执行开始/结束时间、终态、`complete` 和明确的不完整原因。
- 来源信息：源码字节数、校验和、任务描述身份；内置 bug 任务保存 n、范围和排序语义。
- 完整 stdout、结构化返回值，或完整错误快照。
- 执行失败详情及有上限的生命周期日志。
- 客户端/角色环境及与调查有关的上下文。

正文不包含其自身的 Ticket 和校验和，避免自引用；这些字段放在外层记录。主机结果同时保存外层身份和原始正文。

采用严格规范化：有限数值、布尔值、有效文本、显式 null、连续数组和字符串键对象；保留多返回值数量。二进制字符串使用明确标记的 Base64 表示，不能按猜测编码转码。循环引用、不支持的对象、秘密值和超限都必须明确失败或标记不完整。

复用现有序列化基础时检查 `limited`/错误标记，不能把 `<cycle>`、省略项或截断文本当作完整数据。现有 `SerializeForExport` 同步读完整个流，不能仅加协程外壳就声称分步处理。参考 Lychee 的 JSON 工具时必须补足控制字符转义和不支持类型处理。

正文完整性使用实际 UTF-8 字节长度及 Adler-32，存入外层 metadata。该校验用于检测截断和数据损坏，不作为身份认证。可用独立轻量实现，无需为了校验和引入压缩库。主机读取成功后另计算 SHA-256 记录产物身份。

### 5.4 写入顺序与待落盘保护

新增数据库操作 `CommitAutomationResult(executionId, report, summary)`，由一个模块负责完整提交，调用者不自行拼接多处写入。

1. 先完成报告序列化、大小核验和校验和计算，准备证据及索引更新。
2. 预先检查容量与可淘汰记录，确认本次提交可成功。可预留少量空间用于失败报告。
3. 在一个不让出执行权的提交步骤中分配 Ticket，插入完整证据并更新执行索引。提交错误时回退本次半成品，不能留下索引有 Ticket 但正文不存在的状态。
4. 为该 Ticket 设置仅在当前内存会话有效的待落盘保护，所有相关删除和清理入口都识别这一保护。
5. 完成后返回 Ticket，Controller 再生成通知二维码。

现有 `AddExport` 会在插入后立即执行自动清理，因此不能仅在调用结束后再补保护标记。实施时须将“预留容量、插入时保护、清理、索引提交”整合为数据库操作，同时保持普通导出原有行为。

待落盘记录在输出侧重载前不能被普通导出预算清理、删除或一键清空移除。UI 对这类操作给出“先完成落盘或明确放弃”的状态。已完成但未收到读取回执的结果最多积压 3 条，达到上限才拒绝新请求；主机通过 `/dev auto ack` 报告读取结果即可立即释放名额，因此单个漏掉的回执不会锁死整个会话。

保护状态只存在于运行时。正常重载后，数据库从 SV 载入，旧会话保护自然结束。插件可以说明记录已从保存数据载入，但不能因此声称 Python 已读取。若游戏在写盘前崩溃，报告可能丢失；主机日志必须保留这一不确定性。

### 5.5 初始容量与清理规则

| 数据 | 初始预算 | 超限行为 |
| --- | --- | --- |
| 普通 Run 历史 | 保持现有 16 MiB 估算预算 | 沿用现有历史清理规则。 |
| 所有完整导出正文 | 共用现有 16 MiB / 200 条 | 按最旧记录清理，跳过待落盘保护；无法腾出空间则提交失败。 |
| 单份自动化报告 | 1 MiB 序列化后正文 | 返回明确的输出超限失败报告；不能静默截断后标记成功。 |
| 自动化任务索引 | 100 条，合计 256 KiB 估算上限 | 清理最旧非活动摘要，不删除仍受保护的执行。 |
| 生命周期日志 | 每次最多 128 条 / 16 KiB | 单独记录日志限制标志；不能混同于调查 stdout 的完整性。 |
| 主机运行产物 | 初始总量 128 MiB / 100 次已完成运行 | 只清理已完成的最旧运行；未解决运行不自动删除，无法腾出空间则暂停新投递。 |

单报告上限是运行资源约束，不是传输方式阈值。全部尺寸都使用同一条重载落盘通路。需要更大报告时，应显式提高支持上限并验证序列化、重载时长和总体保留预算。

`exports.totalBytes` 当前只统计 `payload.content` 的字节数，不代表 Lua 总内存或 SV 文件大小；转义、字段名和表结构会增加磁盘体积。索引使用独立估算预算。不能把多个预算相加就声称是严格的进程内存上限。

生命周期日志只存入最终完整报告一份；运行期间保存在临时上下文。索引只保存状态、时间和简短错误码，不复制日志、正文或大段源码。

导出被清理后，相关索引的 `resultAvailable` 变为 false；历史仍可显示任务摘要，但显示“结果已过期”。索引被清理不必删除仍在通用导出仓库中的证据。任何读操作均重新核验 Ticket 是否存在，不能依赖过期布尔值。索引范围很小，可在实际删除或初始化时同步修正，不做周期扫描。

### 5.6 迁移与恢复

迁移在所属插件 `ADDON_LOADED` 之后、首次需要数据库时执行；不在文件作用域读取 SV。7→8 只新增自动化索引及必要字段，不改写已有报告正文，不删除历史 Performance 证据。迁移幂等，保留未知字段及较新版本数据；遇到不支持的较新自动化版本时明确停用新写入，不降级覆盖。

恢复时校验索引/order/Ticket 关系，重算派生计数，标记已过期结果。从旧会话载入的 `running` 等非终态只能标记为“执行已中断/结果未知”，不能自动继续或重新执行，也不自动显示通知。

同一请求在当前会话内保证不重复执行；跨崩溃由于 SV 未必写盘，不能保证严格只执行一次。主机在发出 run 命令前持久化执行意图，恢复后遇到不确定状态必须先查日志和现有 SV，不能自动重跑。

## 6. Python：WGC 通知识别与重载

### 6.1 按窗口句柄捕获

采用 WGC 的 `CreateForWindow(HWND)` 路线，参考 `D:/Code/Lychee/APP/src/scanner/wgc_capture.cpp`。只借鉴捕获方式，不引入该 C++/Qt 应用。微软接口按 HWND 创建目标窗口捕获项。[Microsoft CreateForWindow 文档](https://learn.microsoft.com/en-us/windows/win32/api/windows.graphics.capture.interop/nf-windows-graphics-capture-interop-igraphicscaptureiteminterop-createforwindow)

Python 候选依赖为 `windows-capture`。已查看的主分支接口有 `window_hwnd`、帧到达回调、NumPy 数据访问及更新间隔参数。依赖已按实际安装版本锁定：`zxing-cpp==3.1.1`、`windows-capture==2.0.1`、`numpy==2.5.3`、`opencv-python==5.0.0.93`（`Lychee Dev skill/scripts/requirements.txt`），并已用真实包核验本方案使用的入口：`read_barcodes`、`frame_handler`/`closed_handler`、`minimum_update_interval`、`Frame.frame_buffer` 与 `Frame.convert_to_bgr()`。库底层可以是原生扩展，项目自有脚本仍使用 Python。[Python 接口源码](https://github.com/NiiightmareXD/windows-capture/blob/main/windows-capture-python/windows_capture/__init__.py)

使用帧到达回调，截取左上角区域（色块与完成通知共用该角落）。初始以约 100 ms 为处理间隔目标；现在只读一张静态通知，无需承诺 50 ms 高速接收。只保留最新待处理 ROI，不积压帧。跨线程前复制所需小区域，明确原生帧生命周期。通知锚点与主机 ROI 必须指向同一角落，两侧各自写死会让接收路径静默失效，因此主机侧测试会同时校验两处。

NumPy 裁剪发生在 CPU 可见图像上，不代表只从 GPU 读回这一小块。需要实测 WGC 完整帧读回成本，并与二维码解码成本区分。二维码使用 Python ZXing 绑定读取原始内容，再按严格 UTF-8 JSON 解析，不猜测转码。[ZXing Python 接口](https://github.com/zxing-cpp/zxing-cpp/blob/master/wrappers/python/README.md)

窗口身份由 PID、进程创建时间、可执行文件路径和 HWND 共同确认。缩放、窗口尺寸或捕获原点改变时重新校准。窗口关闭或句柄复用时立即停止，不能静默切换其他窗口。最小化、遮挡、独占全屏、HDR 等行为需实测后再声明支持。

### 6.2 发送命令与重载去重

输入侧和输出侧都使用受控的“聚焦 → 校验前台 → 剪贴板粘贴 → Enter”流程。每次发键前重新确认窗口身份和前台归属。保存与恢复剪贴板时，只恢复仍由本次操作占用的内容。

`SendInput` 返回成功不等于游戏处理成功，Windows 前台限制也不能忽略。此前无响应的按键尝试没有被本方案视为已解决问题。实施时必须验证真实命令输入和重载流程。

识别到完成通知后，依次核验版本、字段、预期 task/run、当前目标身份，然后将通知及 `reload_requested` 意图持久化到主机日志，再发送一次 `/reload`。

以 `(installation, requestId, ticket)` 作为去重键。同一静态二维码被重复识别时，不重复重载；脚本重启后也先读取日志。若原重载是否送达不明，先检查目标 SV，不能盲目重新运行任务。必要时明确恢复一次重载尝试，但不得形成无限重载循环。

进入战斗、前台被用户切走、窗口身份改变等情况暂停命令输入；保持已完成结果和恢复信息。重载由外部脚本在任务清理完成后发起，插件本身不自动调用重载。

## 7. Python：定位 SV、读取 Ticket 与完整性校验

### 7.1 安装与账号绑定

每个本地配置记录 WoW 客户端根目录、插件安装路径、目标客户端类型、SV 文件路径及可获得的角色/服务器身份。SV 路径不来自二维码，也不能仅凭窗口标题猜测。

当前 TOC 使用账号级 `SavedVariables`。典型候选为所选客户端下 `WTF/Account/<account>/SavedVariables/Lychee Dev.lua`，实际路径必须确认。首次缺少绑定时，可以只读列举该客户端的账号级候选，并用精确 Ticket 与执行身份确认唯一文件；匹配多个时停止并要求明确账号。不能选择 `.bak` 或因为某文件“最新”就当作正确结果。

同一插件安装目录的任务文件可能被多个客户端读取，同一账号级 SV 也可能被多个游戏进程写入。v1 对安装目录和 SV 写入域分别加锁，并拒绝会导致这些资源并发写入的编排会话；只锁 HWND 不足够。

### 7.2 等待落盘

发出输出侧重载后，主机在明确超时范围内等待文件写入。文件修改时间和大小变化只作为唤醒线索，不能作为完成证明。

读取一份独立文件快照，比较读取前后的文件标识/大小/时间，并对解析不完整、文件替换或临时不可读做有上限重试。若读取内容含正确的新 Ticket、完整报告和匹配身份，即使整个游戏界面尚未重载完成，也可以认定结果已落盘；开始下一次命令前仍需等待客户端可交互。

主机只读取 SV，不能为了补记录或修改接收状态而写回正在使用的 SV 文件。`.bak` 仅用于人工诊断历史，不满足本次新结果的接收条件。

### 7.3 受限解析与校验

使用受限 Lua 数据解析器读取 SavedVariables，支持实际保存格式的赋值、字面量表、字符串转义和基础值，不接受函数、调用或任意表达式。不能用 Lua `dofile`、Python `eval` 或执行整个 SV 的方式解析。

解析器设置文件大小、嵌套深度、条目数和字符串上限，能够处理 WoW 写出的字符串转义和 UTF-8 字节。保留正文原始字节，不能擅自统一换行或文本编码后再核对长度。解析器上限要考虑整个 SV 的其他历史数据和转义膨胀，不能简单等同于单报告 1 MiB。

验证顺序：

1. 只认绑定路径中的 `LycheeDevDB`，验证可支持的顶层结构。
2. 精确查找 `exports.records[ticket]`，验证 `schema`、外层 `ticket`、`createdAt` 与通知一致。
3. 验证 metadata 及内层报告的 task ID、执行 ID、revision 与主机投递日志一致；手工任务则与本次接受的通知及用户目标关联。
4. 校验 `payload.encoding`、媒体类型、真实字节数与 `byteCount`，重新计算正文校验和。
5. 解析内层 JSON，验证报告版本、终态、`complete`、客户端与角色环境，以及请求类型和参数。
6. 原子保存通知、完整证据外壳、未经改写的正文和读取日志，计算 SHA-256。
7. 主机标记 `received`；将执行成功、执行失败、结果不完整分开报告给 Agent。

校验失败不得退回读取“最近一条记录”。找到 Ticket 但字段不匹配，属于身份冲突或数据损坏；找不到则继续有上限等待或返回落盘失败。收到失败报告意味着证据读取成功、任务执行失败，两者不能混为一谈。

## 8. Automation 页面与状态表达

沿用现有控件、字体、间距、颜色和列表复用方式，首次访问时创建页面。左侧显示当前及历史执行，右侧展示任务身份、执行日志、结果详情和 Ticket。

建议列表字段：task ID、执行 ID 简写、任务类型、开始/结束时间、执行状态、结果是否仍可用。详情展示 revision、请求参数、完整结果入口、失败原因和持久化提示。

支持执行已加载任务、取消、查看报告、重新显示通知、隐藏通知、清理历史。选择条目或打开页面不能执行任务。正文分页展示只影响显示，不截断保存数据。关闭主窗口不能意外取消活动任务或移除结果通知；实际停用和战斗清理遵守统一生命周期。

| 位置 | 状态表达 |
| --- | --- |
| 插件执行状态 | `running`、`finalizing`、`succeeded`、`failed`、`cancelled`、`interrupted`。 |
| 结果生命周期 | 由 `ns.AutomationResultState` 单点判定：`pending`（待落盘）、`flushed`（已落盘待读）、`received`（主机已读取）、`failed`（主机读取失败）、`expired`（证据已被清理，仅剩索引摘要）。页面据此显示对应文案。 |
| Python 流程状态 | `prepared`、`start_sent`、`waiting_notice`、`reload_requested`、`waiting_sv`、`received` 或明确失败原因。 |

插件没有观察主机是否读取成功的渠道，因此由主机主动回报：`/dev auto ack <ticket> received|failed`。这条命令走现有聊天命令通道，插件当场记录 `receivedAt` 与 `receivedStatus`（随索引持久化），并立即释放一个积压名额，不需要重载、回写文件或第三次重载。`resultAvailable` 仍然只表示插件能否找到该证据；外部接收状态由上述生命周期表达。

## 9. 模块与文件范围

| 文件/模块 | 职责 |
| --- | --- |
| `Modules/Automation/auto/auto.lua` | 固定任务登记表，只包含生成数据。 |
| `Modules/Automation/Controller.lua` | 命令分派、任务身份、执行上下文、终态与清理、通知触发。 |
| `Modules/Automation/Report.lua` | 完整报告规范化、序列化、容量检查和正文校验；不负责二维码传输。 |
| `Modules/Diagnostics.lua` | 增加错误快照读取接口，兼容处理集中于此。 |
| `Core/Database.lua` | schema 8 迁移、执行索引、原子逻辑提交、待落盘保护及清理一致性。 |
| `Core/Bootstrap.lua` | `/dev auto ...` 参数路由，保留原有 `/dev` 行为。 |
| `Core/Compatibility.lua` / 客户端 profile | 集中处理已验证的像素/缩放等客户端差异。 |
| `Libs/AutomationQR.lua` | 按需初始化、带许可声明的二维码库适配。 |
| `UI/AutomationOverlay.lua` | 执行色块和单张完成二维码，静态更新、有界纹理池。 |
| `UI/Pages/Automation.lua` | 任务列表、详情和操作，复用共享 UI。 |
| `UI/MainWindow.lua` / `Core/Locale_enUS.lua` | 页面入口和新增文案；核对本地化索引要求。 |
| `Lychee Dev skill/scripts/automation.py` | 唯一 CLI 入口：任务更新、run、bugs、接收、恢复、状态。 |
| `Lychee Dev skill/scripts/automation/registry.py` | 区块语法、文件锁、revision 校验和原子写入。 |
| `Lychee Dev skill/scripts/automation/windows.py` | HWND 身份、WGC、ROI 校准、前台和剪贴板命令输入。 |
| `Lychee Dev skill/scripts/automation/saved_variables.py` | 受限 Lua 解析、精确 Ticket 提取和完整性校验。 |
| `Lychee Dev skill/scripts/automation/session.py` | 主机日志、重载去重、超时、恢复和产物管理。 |
| `Lychee Dev skill/references/automation.md` | Agent 使用流程、命令、完整结果解释和异常恢复。 |
| 三个 TOC、测试及 `tools/Package.ps1` | 相同共享加载顺序、客户端矩阵、空登记表发布和依赖许可打包。 |

不新增独立脚本项目或常驻服务，Python 脚本归属 Skill。主机日志、运行环境和结果保存在配置的本地数据目录中，不写入受版本管理的 Skill 源码或插件 ZIP。

三个 TOC 按仓库规则选择对应 profile 和事件目录，共享模块顺序保持一致。现有 TOC 中事件数据位置与指南“紧随 Compatibility”的表述存在差异；实施时结合 BuildMatrixTests 明确统一，不能声称当前文件已经满足一个实际不存在的顺序。

## 10. 实施编排

1. **数据协议与数据库。** 定义通知及报告格式、执行身份、索引、提交和保护规则；先验证旧数据迁移及清理一致性。
2. **任务入口与错误快照。** 实现登记表写入、命令路由、显式执行、异步完成/取消、完整报告和 bug 快照。源码加载后不自动执行。
3. **色块与完成通知 UI。** 适配本地二维码库，实现按需初始化和纹理复用，新增 Automation 页面。
4. **Python 只读接收。** 先以手动命令和手动重载验证 WGC 通知、SV 解析、Ticket 匹配和完整结果落盘，隔离键盘输入问题。
5. **输入与输出侧重载编排。** 加入前台校验、剪贴板、日志先行、一次通知一次重载、超时及恢复。确认多个实例不会误投递。
6. **Skill 与发布。** 更新使用说明及脚本依赖锁定，同步已安装 Skill 副本，完成三端验证和中性任务文件打包。

可并行实施时，数据库由一人维护，Controller/Report、Python、UI 各自使用互斥文件范围；TOC、主窗口接线、文案和正式验证由主 Agent 集成。本节描述的是实施编排方式，实际实施与独立复核的证据见第 12 节。

## 11. 验收与证据边界

| 范围 | 必须验证的结果 |
| --- | --- |
| 原有回归 | `powershell -NoProfile -ExecutionPolicy Bypass -File add-on/tests/TestAll.ps1` 全部三端通过，补充新行为的有效测试。 |
| 默认关闭 | 不创建自动化 Frame、事件、钩子、轮询或大查找表；显式请求后才创建，停用后停止自有工作。 |
| 任务区块 | 并发更新不丢其他区块；冲突和损坏可识别；异常退出后文件仍为完整旧版或新版；源码不能逃逸到顶层执行。 |
| 执行 | 同步/异步成功失败、重复请求、取消、迟到回调、过期、忙碌和崩溃不确定性均符合约定。 |
| 错误导出 | 验证 n、空记录、提供方失败、聚合计数、跨会话、长调用栈/局部变量、快照后变化与秘密值。 |
| 数据库 | 迁移幂等；老记录不丢失；正文只保存一份；清理和删除保护待落盘 Ticket；索引失效准确反映。 |
| 通知 | 成功/失败/取消均能关联正确 Ticket；无 Ticket 不能伪造完成；重复二维码只触发一次输出侧重载。 |
| SV 解析 | 使用真实 WoW 保存样例验证转义、UTF-8、长字符串、文件替换、半写、旧文件、`.bak`、身份冲突和非法语法。 |
| 完整结果 | 原始字节数与校验和一致；正文未被历史/展示截断；超限报告明确失败或不完整。 |
| 重载链路 | run 路径需要输入侧和输出侧重载；bug 路径只需输出侧重载；超时恢复不重新执行任务。 |
| 多窗口 | PID/HWND 复用、多个账号、共享安装目录、共享 SV 写入域、前台变化都不会误发命令或读错文件。 |
| 资源 | 静态通知不反复编码；纹理数量稳定；WGC 帧不积压；报告处理、重载耗时及 SV 增长均有测量。 |
| 游戏与视觉 | 三端准确 build 下的修改前后截图，代表性 UI 缩放/DPI；进出战斗无受保护操作、污染或秘密值错误。 |
| 包装与发布 | 三 TOC 选择正确 profile/catalog，共享顺序一致；ZIP 含空任务登记表、必要库与许可，不含任务源码、主机数据或 Skill 脚本。 |

沿用已核对的客户端基线：

| 客户端 | Interface | wowdoc 源码提交 |
| --- | --- | --- |
| Retail 12.1.0 | 120100 | `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b` |
| Classic 5.5.4 | 50504 | `1028c1e687f721ba9d3af14d1b12a5745e4227c7` |
| Titan 3.80.2 | 38002 | `825d29d3662b372f0bead725ee6abd339e4a77b5` |

此前核对中，`GetEffectiveScale`、`SetTexCoord`、`C_Timer.After` 在三端均有精确源码命中；`GetPhysicalScreenSize` 只在所查 Retail 索引中精确命中，不能据此判断其他端不存在。像素换算集中到 Compatibility，新增接口实施时继续按精确基线核对。

仍需实测：DPI/UI 缩放下的色块与二维码可扫性、进出战斗与前台切换、最小化/遮挡/独占全屏/HDR 行为、`.bak`/半写/文件替换、报告容量对应的重载耗时，以及 `bug` 快照路径的真实数据形状。已完成的真机验证见第 13 节；离线侧结论仅限第 12 节列出的证据范围。

## 12. 实施状态与离线验收证据

本节记录 2026-09-12 的实际实施与独立复核结果。所有结论都来自可复现的命令，未包含真机声明。

已完成的实现：

- 插件侧：`Modules/Automation/Controller.lua`、`Report.lua`、`auto/auto.lua`、`Modules/Diagnostics.lua` 的错误快照、`Core/Database.lua` 的 schema 8 迁移/执行索引/原子提交/待落盘保护、`Core/Bootstrap.lua` 的 `/dev auto` 路由、`Libs/AutomationQR.lua` 的懒初始化适配、`UI/AutomationOverlay.lua`、`UI/Pages/Automation.lua` 及三个 TOC。
- 主机侧：`Lychee Dev skill/scripts/automation.py` 及其 `automation/` 包、`requirements.txt`、`references/automation.md`，并已同步到已安装的 `lychee-dev` skill 副本。

离线验收证据（全部可重跑）：

| 验证项 | 结果 |
| --- | --- |
| `add-on/tests/TestAll.ps1` 仓库矩阵 | Retail/Classic/Titan 三端全部通过，含打包测试（38 个运行时文件），退出码 0 |
| Lua 5.1 语法（`luac -p`，Lua 5.1.5） | 41 个文件全部通过 |
| Python 语法与离线自检 | 7 个文件语法通过；`selftest_offline.py` 在真实依赖下完整通过 |
| 默认关闭零成本 | 文件作用域不创建 frame/事件/钩子/轮询；加载 `AutomationQR.lua` 约 95 KB，首次 `Encode` 后才升至约 1255 KB（256×256 查找表确在 `BuildEncoder()` 内） |
| UTF-8 校验边界 | 独立的 23 例边界测试全部通过（合法：U+0080、U+0800、U+10FFFF、U+1F600；非法：overlong、surrogate、超上限、非法前导、截断） |
| 二维码真实编解码往返 | Lua `AutomationQR` 编码 113 字节通知得 45×45 矩阵，真实 zxing-cpp 3.1.1 在每模块 2/3/5/8/12 像素五档下均逐字节还原；空白图返回空（fail-open） |
| 产物字节保真 | 修复前文本模式写入 56→60 字节并引入 CRLF 且 SHA-256 不符；修复后与记录值逐字节一致，CLI `sv read` 端到端 byteCount 与磁盘一致 |
| 任务区块写入器 | 多区块互不干扰、`--expect-revision` 冲突被拒且旧文件哈希不变、路径越界与非法 ID 被拒、生成结果通过 Lua 5.1 语法校验、非 ASCII/CRLF 源码按字节保真转义 |
| 依赖锁定 | `requirements.txt` 锁定经真实安装核验的四个版本 |

本轮修复的真实缺陷（均已由实现侧复核并加测试）：`zxingcpp.read_bars` 等三处不存在的捕获 API、首次取帧即结束导致长任务无法取得通知、产物换行转换破坏字节身份、`\ddd` 转义最多 3 位被误拒、`verify_evidence`/`parse_inner_report` 校验缺口、通知未与预期 task/run 交叉核验、`send` 绕过日志与去重、registry 中失效的 schema/kind 校验、符号链接/目录联接未解析、写后未记录文件哈希、CLI 缺参抛裸 traceback、ctypes 未声明 restype/argtypes、UTF-8 校验只查第 2 字节、提交失败未落到终态、`PhysicalUnit` 回退量纲相反、提交存在性检查失败无回滚、`__pycache__` 未忽略、缺失依赖清单。

仍未验证（不得据本节推断为已通过）：

- 真机 WGC 捕获启动、HWND 身份、SendInput 投递是否被游戏处理、真实 SavedVariables 写盘时序与转义风格、`.bak`/半写/文件替换、DPI/UI 缩放下的色块与二维码可扫性、进出战斗与前台切换、报告容量对应的重载耗时。
- 输入侧 `/reload`（更新任务区块后）仍由操作者手动执行；`run` 不自动发起输入侧重载。
- 三个 TOC 的事件目录位置与仓库指南措辞的差异（第 9 节）仍未统一；当前三端一致且 `BuildMatrixTests` 通过。

## 13. 真机端到端验证（2026-09-13，Retail 12.1.0 build 69587）

本节记录一次真实客户端的完整链路验证，全部结论来自现场产生的文件与解码结果，不是推断。环境：Retail `12.1.0` / Interface `120100` / build `69587`，窗口 `0x12410C7A`，2560×1440，活动账号 `214262721#1`。

执行顺序与观察结果：

| 步骤 | 证据 |
| --- | --- |
| 任务登记表写入已安装插件 | `automation.py task upsert` 返回 `fileHash`，`task list` 显示 `probe-e2e / req-probe-20260912 / rev-probe-1 / 1472B`，生成文件通过 Lua 5.1 语法校验 |
| 输入侧 `/reload` 由脚本发送 | `send` 返回 `sent`；随后 SavedVariables 被真实改写，说明客户端执行了重载 |
| 任务执行 | 发送 `/dev auto run probe-e2e` 后任务执行成功并生成完成通知 |
| WGC 真机捕获 | 8 秒内收到 78 帧、0 错误；窗口截图与 ROI 已落盘 |
| 通知二维码解码 | 首次尝试即在 ROI 中解出 `{"v":1,"ticket":"LYCHEE-20260913-000150-0011","task":"probe-e2e","run":"req-probe-20260912","ts":1789228910}` |
| 输出侧 `/reload` 落盘 | SavedVariables 由 249,720 增至 252,380 字节（+2,660），时间戳随之更新 |
| 迁移与索引 | 真实文件中 `schemaVersion = 8`，`automation.version = 1`，索引键为 `req-probe-20260912`，`status = "succeeded"`、`resultAvailable = true`；旧数据（`schemaVersion 7` 的历史导出）保留 |
| Ticket 读取与校验 | 受限解析器读出记录，`verify_evidence` 无失败；`byteCount = 1171` 与正文真实 UTF-8 字节数一致，Adler-32 `df04a2cb` 匹配 |
| 报告完整性 | 内层 `lychee.automation.result.v1` 的 `taskId/executionId/revision/requestType` 与投递参数逐项一致；`complete = true`、`outputTruncated = false`；探针 stdout 5 行（含 `cleanup ran`）与返回值（`mathOK`、`stringUpper`、`tableConcat`、`character`/`realm` 等）与探针源码逐项对应；正文不含自身 Ticket 或校验和 |

真机验证发现并修复的逻辑缺陷：

- **真实 SavedVariables 含显式 `nil`**。客户端会为已声明但本次会话不存在的全局写出 `DumperDB = nil`，而受限解析器原先拒绝 `nil`，导致读取真实文件直接失败（合成样例没有这一形态）。现按 Lua 语义处理：顶层 `X = nil` 视为该全局不存在，表内 `["key"] = nil` 视为键不存在；已加回归测试并做负向对照。

真机验证确立的运行约束：

- **`SendInput` 必须前台**：它把事件注入系统输入队列，只到达前台窗口。焦点在别处时 `SetForegroundWindow` 会被 Windows 拒绝，因此实现先尝试常规置前，失败后用一次 ALT 解锁（`keybd_event(VK_MENU)`），并且**始终复核前台窗口**；拿不到焦点就明确失败，绝不把按键送进别的窗口。
- **聊天命令顺序必须是「回车 → 粘贴 → 回车」**。原先的顺序是「粘贴 → 回车」，结果是客户端打开了空聊天框而命令没有执行（现场现象：回车打开了聊天框但里面没有内容）。
- **消息投递对 WoW 无效**：`send --mode messages` 用 `PostMessageW`（`WM_CHAR` + `WM_KEYDOWN`）投递按键，无需前台；实测按键被投递但游戏无反应，SavedVariables 未改写。现代客户端走原始输入通道，忽略窗口消息队列，因此可靠路径只有「前台 + `SendInput`」。该模式保留为显式选项并在文档中标注不适用于 WoW。
- **命令输入仍然不过游戏自己的解析器**：斜杠命令能否执行取决于客户端状态（例如锁定 UI），主机侧只能保证按键送达。

仍未覆盖的真机项：DPI/UI 缩放下的可扫性、进出战斗与前台切换、最小化/遮挡/独占全屏/HDR、文件替换与半写、`.bak`、报告容量对应的重载耗时、以及 `bug` 快照路径的真实数据形状。
