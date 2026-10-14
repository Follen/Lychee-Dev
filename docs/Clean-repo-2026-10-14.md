# Clean repo — 2026-10-14

本文件记录审计基线、改进清单及实施状态。日期按 Asia/Shanghai。下方审计结论对应原提交；实施结果单独记录，未验证事项不视为完成。

修复后追加审计已完成：三名 GPT-6.1-sol、xhigh 连续执行两轮全仓审计，第二轮轮换分区并交叉复核。新增 13 项确认问题（高 1、中 11、低 1），另保留 3 项候选；均追加在文末。追加审计阶段只确认问题；本轮已完成这 13 项代码修复并加入正式回归，当前离线验证、Windows CI 和待实机项分别记录在各条目及文末。前一批 42 项修复保持基线，不重复计数。

交接更新：当前代码基线及本文已按用户授权提交、推送到 `codex/fix-3.2.0`，交接 PR 为 [#13 · fix-3.2.0（草稿）](https://github.com/Follen/Lychee-Dev/pull/13)。当前分工、待办和验证边界统一见 [handover.md](../handover.md)；本文保留各阶段的审计和实施证据，PR 提交不代表新发现的问题已修复。

## 实施状态

2026-10-14，用户授权三名 GPT-6-astra、high 子代理并行修复，主代理负责统筹、集成和验证。修复阶段在原 main 工作区进行，没有另建 worktree、提交、推送或发布；后续按新增授权通过 `codex/fix-3.2.0` 交接 PR。Rust worktree 和未解决恢复记录继续保留。

| 分工 | 范围 | 状态 |
| --- | --- | --- |
| astra_fix_live_addon | Live、addon、bridge、desktop、当前协议回归与旧执行链退役 | 已完成，统一验收通过 |
| astra_fix_data_storage | Source、Data、selection、vault/evidence、共享解析与有效测试迁移 | 已完成，统一验收通过 |
| astra_fix_delivery_cli | CLI、delivery、npm/release、工具、Skill 与当前文档 | 已完成，统一验收通过 |
| 主代理 | 文件边界、跨模块依赖、合并检查、全局验证及本文件状态 | build/vet、完整 Go/Lua 与必要集成验收通过 |

确认问题按现有证据修复；候选先核实再决定，不因列入审计而添加未证明有用的保护机制。三组共用工作区，跨组文件由其所有者修改。

原 38 项确认问题已实施修复；C-01/03/05 经可控回归、C-06 经授权实机验证确认后也已修复，共 42 项确认问题。其余四项候选已明确合同或支持范围并落实实现。以下是当前实现结果，验证状态见文末；下方各项原始“验证”描述仍为审计时提出的验收方法。

| 项目 | 实现结果 |
| --- | --- |
| CR-01/02、AR-02、D-01/02/03/06 | 删除旧主动 Live、队列、Receiver、QR、聊天输入及 inputlab；保留历史记录、报告、取消/放弃和必要占用释放，Automation 只读展示历史。 |
| CR-03、C-03、D-04 | 诊断上限只截断日志；读取失败后核验发布路径，确认对象替换才重试；删除私有堆扫描、地址 hints 和全区域枚举。 |
| CR-04/05/06/07 | stage 拒绝源码重叠和未知非空目录；启动器遵守逐目录 PATH 顺序；移除不可执行参数；addon ZIP 携带项目许可。 |
| CR-08/09、AR-03 | 当前 Lua 测试共用必需解释器入口；公开 CLI 分派加入行为回归；发行组装要求九项真实 LuaLS、broker 和安装包语义测试执行且不跳过。 |
| SOL-01/02/03 | 同一 Metadata 写租约覆盖 blob 发布、引用提交和删除；删除代次留存；fresh 恢复采用锁内读取的完整原归档计划。 |
| SOL-04、R2-07/08 | 支持 Forever 家族 Source 与地区 Data 配对；target 先精确匹配、折叠多命中报歧义；新安装路径保存为绝对路径，旧相对记录须替换后使用。 |
| SOL-05/09 | 保留 helper 明确退出码；JSONL 经过最终来源复核与归档才输出 end，已开始的流在业务失败时输出一个 error。 |
| SOL-06、R2-09/10、C-07 | 升级准备先落恢复日志再发布目录；卸载接受已认可的旧槽 TOC；npm 更新采用进程持有的系统锁；默认发现排除已移除且无恢复事务的记忆目标。 |
| SOL-07、R1-01/02/03 | TOC 验证保留 Source 索引缺口；同步维护 doctor 使用的 tracked ref；正文搜索限于已索引内容；Context 跨 snippet 重置偏移。 |
| SOL-08、R1-06/07、C-04 | foreign-key 拒绝整数数组；UNION 保留重复显示列名并按列序号绑定；AVI 拒绝短 chunk header，支持有界 LIST rec 遍历。 |
| SOL-10/11 | doctor 沿原 CON 关闭再修复安装；关闭 bridge 取消异步资源，并阻止迟到回调重新启动展示。 |
| R1-04/05 | 同步、异步结果保留 false；prepare 的精确 slot_capacity 拒绝沿原请求和截止时间进入容量重载。 |
| R2-01/02/03/04/05/06 | 保留未来版本存储；搜索先排名再截断；导出固定执行源码；格式化异常保留诊断；探针列表接通公开上限并在分页前排除已删除名称。 |
| R2-11 | registry signatures 与 provenance 元数据分开报告；仅有签名不能通过 provenance 回读。 |
| AR-01、D-05/07 | 合并 addoncheck/codebase 底层解析与 closure walker；迁移有效 oracle 断言后删除自证台账及无调用符号，保留唯一行为覆盖与来源许可。 |
| C-02/08 | 新增 fullQueryComplete，保留 Wago 窗口与 dbcache 快照各自既有 complete 语义；download.workers 接入工作区实际 HTTP 并发上限。 |
| C-01 | 同一交换、runtime 与输入种类的已知零输入重试可分段保存完整状态；原期限、输入历史与未知输入不重发的约束保留。 |
| C-05 | 两个 broker 关闭入口等待已核验的原进程退出，再报告关闭完成；等待保留原截止时间，不终止进程。 |
| C-06 | FunctionTrace 在战斗检查、计时和观察回调前设置重入标志，异常后解除；同一亚服 Forever 客户端复验每次调用只记录一次。 |

八项候选均已处理。C-04 的 rec 支持以标准 AVI 结构 fixture 验证，不宣称取得了真实 WoW rec 样本；C-06 的实机结果仅覆盖下方明确列出的客户端与 API。

## 审计基线

| 项目 | 基线 |
| --- | --- |
| 分支 / 提交 | main / 1f7b5977bd509422128d7c14bc8c4aeabb1ca07f |
| 开发基线 / 发行版本源 | 3.2.0 / 3.1.2 |
| 首次审计 | 3 名 GPT-6-astra，max，完成两轮审计与交叉复核 |
| 补充审计 | 3 名 GPT-6.1-sol，xhigh，已完成；主代理核对后新增 11 项确认问题、2 项候选 |
| 追加第一轮 | 另 3 名 GPT-6.1-sol，xhigh，已完成；新增 7 项静态确认问题、3 项候选；退出码新触发并入 SOL-05 |
| 追加第二轮 | 再换 3 名 GPT-6.1-sol，xhigh，已完成；新增 11 项确认问题、3 项候选；其中 provenance 线索经主代理查阅 npm 官方资料后确认 |
| 审计阶段修改范围 | 仅本文件；后续代码修复已获授权，进度见上方实施状态 |
| 排除范围 | 明确保留的 .worktrees/rust-refactor；用户的活动项目与未解决恢复记录 |

证据来自当前源码、调用关系、测试内容、workflow 配置及注明的官方资料。源码证据链接固定到上述审计提交，避免修复或删除后错指当前文件；它们不作为修复完成的验收证据。主代理已复核全部确认项的触发条件和修复范围；去重后共 38 项确认问题、3 项架构债、7 组删除/迁移清单、8 项待验证或待明确合同的候选。候选不计入确认问题，删除清单不表示已经删除。

静态确认不代表实机复现。审计阶段曾用 SQLite 纯内存数据库复核代次重置，用简化 JavaScript 算法演算 Source 分页；当时未运行回归或修改代码。本轮实现与验证另见首尾两节。既有实机结果仍以 [todo.md](../todo.md) 的范围为准。

## 结论与实施顺序

审计时，旧 QR、Receiver、OP/BTP 执行流程及其测试尚未完整退役。部分测试主动关闭 SlotRuntime，再恢复旧启动流程；测试通过不能证明当前 Mailbox 行为。当前路径也存在独立缺陷，按下列顺序完成修复和清理。

| 顺序 | 工作 | 完成边界 |
| --- | --- | --- |
| 1 | 关闭旧执行入口；修复证据删除竞争、Metadata 代次、归档恢复和 stage 删除边界 | 防止错误发键、删除仍被使用的数据或覆盖新配置；恢复沿原记录继续 |
| 2 | 修复当前功能：升级/卸载恢复、doctor、异步关闭、Mailbox 容量、目标选择、Source/Data 结果及工作台持久化 | 合法操作可恢复；缺口不变成已证明不存在；结果与生成它的输入正确配对 |
| 3 | 退役旧实现、测试、模拟器、fixture 与验收入口 | 先迁出共享类型和有效行为测试，再删除整组；保留历史读取和必要的占用释放 |
| 4 | 修复命令合同、启动器、打包及有效测试入口，合并重复解析器，收拢文档/Skill | 公开行为与分派一致；验证接到当前实现；删除失效引用和重复步骤 |

优先级：高表示错误输入或数据完整性风险；中表示可触发的功能或交付缺陷；低表示维护问题。架构债单列，不以文件体积作为重写理由。

## 已确认问题

### CR-01 · 高 · 旧 reset 入口仍可发送旧组合键

- 证据：[命令合同](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/command_contract.go#L210)、[旧 Receiver 等待与 wake](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/receiver_transport.go#L339)、[当前 Controls](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Core/Controls.lua#L78)。
- 触发：代理或用户按 describe 选择无 CON 的 live reset，目标通过旧窗口与占用检查。
- 调用链：ResetWindow → sendBootstrapReceiver → sendReceiverInput → awaitReceiverWake。普通画面没有 receiver_ready 时，可以先发送旧 Ctrl+Alt+]，再等待 QR，最多尝试三次。
- 影响：这条路径没有当前 Mailbox INPUT 授权。当前 addon 又已拒绝旧 bridge 动作，无法靠该流程完成原生连接恢复，还可能触发旧组合键上的其他动作。
- 改进：从当前合同和分派移除旧 reset/bind 等执行入口；通过当前 doctor/resume/reload/disconnect 恢复。历史证据查询与旧执行分开。
- 验证：使用输入 spy，验证退役入口在捕获、占用和发键前停止；保留当前 CON 恢复测试。未执行实机误触复现。

### CR-02 · 高 · 旧 OP 恢复仍可发送 Esc 和聊天命令

- 证据：[resume 分派](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/resume_live.go#L32)、[prepared fixed reload](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/fixed_reload.go#L244)、[实际按键序列](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/desktop/receiver_input_windows.go#L88)。
- 触发：live resume OP-… 的记录是 prepared fixed-reload，且满足当前版本、原安装 commit、PID/创建时间、窗口和占用检查。
- 影响：恢复仍能进入 Esc×3、Enter、/reload、Enter，没有当前协议的新鲜 INPUT 检查。此结论不表示任意旧 OP 都能触发，也不适用于正常 CON 输入路径。
- 改进：退役旧执行分派及 Receiver 发键实现；保留历史状态、报告读取、释放占用和明确终止旧记录的能力。submitted/uncertain 不得因迁移而重新发送。
- 验证：旧记录 fixture 覆盖 prepared、submitted、uncertain；确认任何状态都不会恢复旧键序列。

### CR-03 · 中 · 诊断次数上限阻断有效 Mailbox 读取

- 证据：[第 257 次读取前返回错误](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/channel/native_windows.go#L115)、[生产入口设置 TraceDir](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/channel/project_windows.go#L260)、[固化错误行为的测试](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/channel/native_diagnostics_windows_test.go#L116)。
- 触发：同一个 Native 实例累计 256 次诊断读取；生产入口启用 TraceDir。
- 影响：下一次读取在 Mailbox.Lookup 前返回 live.channel_observation_budget，合法的长任务可能早于原业务期限停止。flushTraces 不重置计数。不能把它换算成固定多少秒，读取频率和每轮读取次数不同。
- 改进：限制诊断保存数量，明确标记截断；业务读取继续受原持久期限约束。
- 验证：超过 256 次观察后仍能读取终态，日志数量有界，原 deadline 不延长；保留取消和 flush 失败测试。

### CR-04 · 中 · stage 对任意输出目录先递归删除

- 证据：[live-stage.mjs](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/tools/live-stage.mjs#L40)。
- 触发：--out 指向源码目录或其他已有目录。
- 影响：工具在读取 addon/Skill 源码前递归删除输出目录；例如输出与 addon 重叠时，先删源码，再生成失败。未执行破坏性复现。
- 改进：允许新输出目录；覆盖时只接受可识别的旧 stage，拒绝源码重叠与未知非空目录。
- 验证：用隔离 fixture 覆盖新目录、旧 stage、未知目录及源码重叠；不得用真实仓库复现。

### CR-05 · 中 · Skill 启动器违背混合安装的 PATH 顺序

- 证据：[启动器](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/skills/lycheedev/scripts/lycheedev.mjs#L13)、[现有单 npm 目录测试](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/tools/skill-launcher.test.mjs#L8)。
- 触发：PATH 前部是当前 npm 安装，后部存在原生 exe。
- 影响：启动器先遍历整个 PATH 寻找 exe，可能绕过前部 npm 安装，使用不同版本或更新入口。当前机器没有已选错旧版本的证据。
- 改进：逐个 PATH 目录统一识别 exe 与受支持的 npm shim；保留 LYCHEEDEV_BIN 优先级和不经 shell 的参数转发，不改成自动选最新版。
- 验证：两个 fake CLI 覆盖 npm 在前和 exe 在前，检查实际选择、argv、退出码。

### CR-06 · 中 · fallback 公开参数与 Windows 分派不一致

- 证据：[合同](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/command_contract.go#L219)、[实际拒绝](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/live_channel_windows.go#L54)。
- 触发：按合同给 live reload fallback 传 --session 或 --wake-binding。
- 影响：唯一发行平台 Windows amd64 对非空值直接返回参数错误；文本合同检查仍可通过。
- 改进：删除失效公开参数并重新生成 Skill 目录；安装激活使用 installation/PID/request，已有 CON 使用 resume。
- 验证：合法激活组合与原 CON 恢复通过真实分派；退役参数有明确错误提示。

### CR-07 · 中 · 独立 addon ZIP 缺少项目 LICENSE

- 证据：[文件收集](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/tools/addon-package.mjs#L71)、[ZIP 白名单](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/tools/addon-package.mjs#L207)、[payload 对照](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/tools/release.mjs#L471)、[当前包装约定](licensing.md#L9)。
- 影响：独立发布的 addon ZIP 没有项目 LICENSE 和集中声明文件；直接补文件还会被验证器拒绝。MatrixSymbol 源码保留 BSD 声明，不能说所有第三方声明都丢失。
- 改进：统一 payload、收集器、ZIP 白名单及一致性检查。保留仍适用的源码内声明。
- 验证：现有包装测试检查 ZIP 文件清单和内容，与 npm addon payload 一致。

### CR-08 · 中 · QR 测试绕过 Lua 必需模式

- 证据：[findLua51](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/tests/qrfixture/qrfixture_test.go#L51)、[CI 解释器配置](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/.github/workflows/toolkit-ci.yml#L85)。
- 影响：两个 QR 回环测试只查 PATH，不读取 LYCHEEDEV_LUA51 或 LYCHEEDEV_REQUIRE_LUA51；PATH 无 Lua 5.1 时直接跳过。workflow 仅写解释器环境变量，未把它加入 PATH。
- 边界：不能仅凭当前配置声称历史每次 CI 都跳过；runner 的 PATH 可能已有解释器。
- 处理：完整退役 QR 时删除本组，不为待删除功能追加测试框架；若暂时保留，则必须统一解释器选择与 REQUIRE 行为。

### CR-09 · 中 · 可获得真实 LuaLS 的发行阶段未接通现有集成验收

- 证据：[发行阶段取得 LuaLS](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/tools/release.mjs#L425)、[安装包语义测试](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/source_package_test.go#L15)、[跨 CLI broker 测试](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/codebase/broker_real_test.go#L16)。
- 影响：部分测试仍依赖人工提供的环境变量，发行流程已取得固定 runtime，却没有运行对应 Source 语义、broker 和安装包路径；“CI 无法取得 payload”的注释已过期。
- 改进：复用现有发行验证阶段及已准备的 runtime/package，接通实际集成用例；外部客户端、桌面和 oracle 专项不强行塞入普通 CI。
- 验证：检查对应测试确实执行，不以包级 exit 0 或 skip 计为通过。

## 架构债

### AR-01 · 两套底层解析器已发生漂移

[delivery/addon_release.go](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/delivery/addon_release.go#L77) 仍调用 addoncheck，因此它不是死包。addoncheck 与 codebase 的 analyze、closure、toc、xml 去掉包名后相同；lua/facts 的 Scope 和 descriptors 的 Namespace 行为已不同。

建议抽取共享的低层解析与加载闭包实现。delivery 保留安装校验，codebase 保留固定来源、索引和语义职责。迁移调用者和两侧测试后再删重复文件，不追加一层平行兼容实现。

### AR-02 · 当前 Native 与旧执行引擎仍共存

命令合同、分派、addon TOC、AutomationView、delivery queue、evidence report 和测试同时保留旧执行概念。清理必须跨越这些调用者，不能仅删除带 QR 字样的文件。

关键切断点：[AutomationView](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Modules/AutomationView.lua#L312)、[旧安装 queue 检查](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/delivery/installation.go#L113)、[旧 report evidence](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/evidence/report.go#L18)。当前需要的历史展示、持久化读取和占用释放应与旧执行分开。

### AR-03 · 命令文本校验不足以证明分派可执行

[skill-contract.mjs](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/tools/skill-contract.mjs#L70) 检查命令和 flag 存在；生成器据此列出命令，但无法发现 CR-01、CR-06 的实际分派差异。建议针对已发现的不一致增加少量真实分派测试；不建立新的全量门禁框架。

## 删除与迁移清单

下列是建议执行范围，当前均未实施。测试之间互相引用不构成保留理由；仍保护当前行为的断言应迁到现有当前路径。

### D-01 · 旧 Live 根层测试与执行引擎

internal/live 根层的 47 个测试文件中，以下 43 个文件属于旧执行组。名单为 _test.go 基名，不含 channel、memory、journal 子目录：

```text
abandon_dispatch  abandon_recovery  abandon  cancel
bind_window  bootstrap_admission  bootstrap_probe  bootstrap_restage
budget  capture_region  connect  dispatch
execute_lifecycle  execute_probe  faults  finish_probe  hide_receipt
investigation_lifecycle  next_action  prepare_probe  probe_operation  probe
receiver_lifecycle  receiver_wake  reconnect_action  reload_upgrade
report_account  report_account_unix  report_account_windows
report  report_writers  reset_window  resume_live
run_lifecycle  run  saved_report  send_flush  send_probe
session_operation  standalone_reload  startup_beacon
window_session_compact  window_session
```

与对应 OP/BTP、光学 Receiver、ACK/finish/hide/reset、SavedVariables 传输和旧固定重载实现一起退役。历史记录展示、明确终止和释放占用仍需要的断言迁出后保留。

保留 client_window_test.go 和 probe_registry_test.go。拆分 discover_test.go、installation_path_test.go，保留被动发现、产品/地区安装身份及路径别名行为，去掉旧 QR /dev 模拟。

### D-02 · 旧跨语言协议与 QR

tests/protocol 下以下 21 个 Go 测试文件及对应 Lua 脚本成组退役：

```text
acknowledge  archival  atomic_live  cleanup  connect
identity  investigation  lifecycle  loaded_ready  optical_pair
persistence  probe  queue  receipt_hide  receipt_recovery
receipt_reset  receiver  reentry  report  runtime_epoch  symbol
```

同时移除 legacy_startup.lua、queue_ack.lua、receiver_rollback.lua、signal_helpers_test.go。六组脚本显式设置 SlotRuntime=nil 后装载历史适配器，见 [atomic_live.lua](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/tests/protocol/atomic_live.lua#L55)。

tests/qrfixture 整目录随 MatrixSymbol/QR 编解码退役。protocol 下旧 signal/receipt schema、samples、receiver golden 及其测试一起清理。同步更新 [版本同步清单](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/tools/version.mjs#L8) 和 addon TOC。

保留或拆分：

- memory_record_test.go：Lua MemoryProtocol 到 Go 的实际编解码。
- slot_driver_test.go / slot_server.lua：当前协议恢复；将旧 heap scan 模拟读取替换为当前 receipt/body 读取。
- receiver_bindings_test.go：名字虽旧，实际测试当前 F12/F11 双绑定、登录/战斗延迟、绑定回滚。
- input_state_test.go：保留当前 InputState，删除旧 TestSessionInputWait 与 input_wait.lua。
- character_state_test.go：将角色隔离、不导入旧执行、未来 schema 不覆盖的断言并入当前持久化测试。
- writer_test.go：保留 JSON/UTF-8/secret/预算/checksum，删除旧 EncodeSignal 压缩与 QR 身份部分。

### D-03 · 旧 bridge 测试与消费者

internal/bridge 下除 memory_record_test.go、slot_schema_test.go 外的 15 个测试文件，随旧 signal、receipt、receiver、queue、SavedVariables 实现退役。

先处理 delivery/queue、安装与发布检查、evidence/report 等消费者；不要留下无测试但继续参与当前安装的旧业务链。

### D-04 · 旧私有堆扫描与地址 hints

删除 hints_test.go、hints_performance_test.go、nearby_test.go、scan_test.go、simd_test.go 及仅供旧扫描器使用的实现。当前有真实 Process 却无 Mailbox 的 Native 路径会拒绝，旧 Find/FindNearby 分支主要由测试 Source 维持。

保留 lua_mailbox*_test.go、lua_root_scan_test.go、module_windows_test.go、bootstrap_test.go。拆分 records_test.go 与 process_windows_test.go，保留精确 BODY 授权、静态头检查、PID 创建时间及定点读取。

删除 scan.go / hints.go 前迁出当前 Mailbox 仍使用的 Source、Range、Region、Gap、Coverage、LookupResult 等类型。运行时指令/RIP 扫描继续保留。

### D-05 · 迁移台账与只测试自身的辅助代码

删除 [internal/parity/catalog.go](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/parity/catalog.go)、[tests/parity/coverage.json](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/tests/parity/coverage.json)，以及 parity_test.go 的两项 CoverageCatalog 测试、SemanticNormalizers 测试和专用 normalize。

这些只验证旧 wowdoc/wowdata → 2.0 迁移清单的文件/命令存在、条目数或人工 JSON，不调用对应产品行为。将 TestDemuxGoldenThroughParserAndCurrentCLI 的真实 CLI 部分迁到 command 测试，移除重复 parser/oracle 断言后可删除 tests/parity 目录。

保留 AVI 样本、oracle、来源/版权说明，以及 parser 格式边界、records 输出副作用、CLI 参数/输出的分层回归。

### D-06 · 旧验收工具、实验与注释测试

- 退役 tools/live-baseline.mjs，以及 baseline-common.mjs 的旧 assessLive/sameLiveEvidence 和配套 runner 自测。其 OP/display 结果模型不能验收当前 CON。
- BASE-13/14 的旧执行、BASE-15 的 BTP/writer、BASE-16 的旧 OP 新建与重驱动、REAL-04 的 RGB/45 秒 beacon 条目移除。保留当前被动发现、CON admission/log、跨进程锁及历史占用释放用例。
- 保留 tools/baseline.mjs 当前离线入口和通用 options、Go events、TAP 等测试。
- 退役 input_lab_adapter_windows.go、input_lab_windows_test.go 旧 Ctrl+Alt+] / Text 实验链；当前仍需要的消息参数与取消释放断言迁到 Mailbox 输入测试。
- 删除 tests/addon/addon_test.go 的 TestLocaleFilesAnchorMarker。它只要求为后续代理保留注释锚点；实际 locale 测试保留。
- 拆分 tests/addon/t_automation.lua 的旧 Queue/ProbeRunner/ACK/QR Notice 用例，保留当前工作台、native history 与结果/清理状态测试。

### D-07 · 无调用符号

已核对全仓引用，以下可按符号删除，不能因此删掉整个所在文件：

| 文件 | 符号 |
| --- | --- |
| internal/codebase/research.go | SearchSource、专用 SourceSearch 类型 |
| internal/codebase/compat.go | compatibilityKindError |
| internal/codebase/flow/execute.go | expressionHasSecret |
| internal/records/relational/values.go | booleanValue |
| internal/live/receiver_attempt.go | receiverAttemptBlocksReplay |
| internal/live/memory/scan.go | addGap |
| internal/desktop/input_lab_adapter_windows.go | LabKeyboardLayout、LabForeground |

## 测试入口与原七项问题的核对

| 问题 | 核对结论 |
| --- | --- |
| QR CI 静默 skip | 解释器选择缺陷确认；未核查历史每次 CI 的 PATH 和日志，不作“从未执行”的结论。按 D-02 退役。 |
| 21 测试 / 15 文件靠环境变量 | 当前口径为 27 个顶层测试 / 21 文件，排除 9 个由父测试自动启动的 subprocess helper。仅出现 env/skip 不能证明功能从不执行。 |
| AVI 三份重复 | 删 parity 中重复 parser 段；records 的发布/证据/取消及 CLI 合同保留。 |
| 四个 Lua 外壳只断言退出码 | Lua 脚本包含真实断言，不是空测试。按当前业务用途保留或随旧功能退役。 |
| 三份 luaRuntime | 实际为两份 luaRuntime 加 qrfixture.findLua51。QR 删除后合并剩余两份小型测试 helper。testkit 的引用数量本身不是问题。 |
| 两个 build tag 无入口 | 当前 runner 未执行 casc_oracle 下两个自包含测试；可接入离线验证。旧 lycheedev_input_lab 按 D-06 删除，不追加维护门禁。 |
| navigatetest 37 测试 | 保留当前 Inspect、SQL、导航覆盖；建议迁到 tests/records，并更新 baseline 与相对 fixture 路径。 |

27 个环境条件测试的组成：真实 LuaLS runtime 6 个；固定 API/完整 workspace 4 个；CLI broker/安装包语义 2 个；桌面/WGC/指定进程 6 个；客户端、内存回放、资产、安装、oracle 9 个。部分有手动 baseline 或专项 runner。Hotfix/Asset/Recovery 的 installed-launcher 分支由 install-smoke 等工具间接设置，不计作未接通。

## 文档、Skill 与仓库卫生

- 保留当前 README、Design、Changelog、todo、AGENTS，以及当前协议、开发、许可文档；不再整批重建有效文档。
- 删除 live-probes.md 中“addon 未加载也必须 fixed reload”的旧断言，改为指向 live-startup；实际当前激活路径已有 fresh INPUT 门控。
- 从命令合同移除旧执行面，再生成 commands.md。status/resume/session/execute/reload/bugs 同时服务 CON，不能整条删除。
- 将恢复细节集中于 live-recovery，启动激活集中于 live-startup，探针生命周期集中于 live-probes；安装参考保留部署与激活的区别。
- Source 的 LuaLS session 细节集中于 source-relations；SQL 输入与错误处理集中于 data-query-recipes，调用文档只保留入口。
- 新 home 的普通初始化应在共享入口说明，使 Data/Assets 与 Source 一致；不默认使用 --fresh。
- 删除 workflow 中失效的 release-2.0.1.md 与 owner-decision section 引用，指向当前文档。
- 删除 .gitattributes 对已不存在 docs/toolkit/research 的规则前，将 release.test.mjs 的 export-ignore 场景改为测试自己的合成规则；有效 Git archive/索引导出回归保留。
- 审计开始时根目录没有 .tmp；没有为本次审计创建临时目录。Rust 工作树明确保留。
- fixture 的历史 AGPL 来源、自有代码当前许可、游戏数据来源需分别核对；本次不擅改来源字段。GitHub.png、Logo.png 的来源记录仍是既有未决项。

## GPT-6.1-sol 补充审计：已核对发现

三名审计员均按用户指定使用 GPT-6.1-sol、xhigh。只读覆盖分工如下；覆盖表示追查相关边界，不表示逐行证明所有算法。

| 审计员 | 检查范围 |
| --- | --- |
| sol_architecture | CLI 合同/分派/错误分类、项目与 pin/target、workspace 初始化、Metadata CAS、lease、blob/capture/retention、缓存边界及其业务调用者 |
| sol_live | 当前 CON 状态机、输入意图与结果、报告/清理、原期限、槽竞争与运行时替换、内存读取、journal、Windows 输入、Lua 异步生命周期及相关旧入口 |
| sol_source_data | Source pin/索引/验证/batch/LuaLS、CASC/CDN、DB2/SQL/Hotfix/资产、受管安装、npm 更新、发行与 Skill 启动边界 |

主代理核对了下列每项的位置、触发条件和影响。重复发现的诊断 256 次上限、旧 OP 发键和 live-stage 删除问题已合并到 CR-02/03/04，不重复计数。

### SOL-01 · 高 · 删除 capture 与并发发布竞争，可使新证据引用缺失 blob

- 来源：sol_architecture；主代理核对结论：确认代码存在竞争窗口，未执行真实文件删除复现。
- 证据：[CommitCapture](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/evidence/archive.go#L93)、[RemoveCapture](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/evidence/archive.go#L257)、[PublishBlob 锁](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/vault/blob.go#L65)、[RemoveBlob 锁](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/vault/blob.go#L145)、[WriteMetadata 的范围](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/vault/access.go#L15)。
- 触发交错：已有 CAP-A；另一个 source inspect 等调用为相同字节创建 CAP-B。B 发布或复用 blob 后，A 扫描引用，尚未看见 B；B 随后提交 manifest；A 删除 CAP-A 和 blob。CAP-B 可以成功返回，之后 verify 却读不到内容。
- 原因：共享引用扫描、manifest 删除、blob 删除不是一个协调范围；各次 blob 文件锁在方法返回时释放。WriteMetadata 只是打开 Metadata，SQL CAS 只校验被删记录的代次，不能发现扫描后新增的引用。
- 改进：协调 capture 发布、引用保护和删除的整个生命周期，覆盖检查到提交/删除的间隙。不要以单次文件锁或最后再检查一次替代完整协调。
- 验证：用确定性 barrier 交错发布/删除，检查新 capture、keep 和业务引用完整；不在真实用户证据上制造竞争。

### SOL-02 · 中 · 删除再创建重置 generation，旧 CAS 可以覆盖新 target

- 来源：sol_architecture；主代理核对结论：确认，原 SQL 的纯内存最小验证通过。
- 证据：[Metadata 创建与更新](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/vault/metadata.go#L153)、[删除](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/vault/metadata.go#L199)、[PutTarget](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/selection/target.go#L136)、[RemoveTarget](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/selection/target.go#L315)。
- 触发交错：A 的 replace 读取 generation=1；B 删除该 key；C 创建同名配置，generation 又变成 1；A 使用旧 generation 的 UPDATE 成功覆盖 C。旧 remove 也可能错误匹配重建后的记录。
- 影响：单次 SQL 事务原子性没有解决跨删除/重建的代次复用；与“generations never reset”的内部约定不符。
- 主代理验证：在 SQLite :memory: 中执行代码中的 INSERT、DELETE、UPDATE，输出 staleCASAffectedRows=1，最终行为为 generation=2、value=stale replacement。没有创建数据库文件；这证明 SQL 的 ABA 行为，不代替完整 CLI 并发回归。
- 改进：保留跨删除的代次，或对 target 创建/替换/删除使用同一生命周期 lease。选择能覆盖真实调用者的最小修正。
- 验证：交错 delete/recreate 与旧 replace、旧 remove，旧操作必须冲突。

### SOL-03 · 中 · 并发 fresh 初始化可把归档日志改成错误路径

- 来源：sol_architecture；主代理核对结论：确认锁外计划与锁内恢复不一致，未执行文件系统复现。
- 证据：[锁外计划](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/vault/archive.go#L123)、[锁内只采用部分旧 journal](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/vault/archive.go#L179)、[后续写回](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/vault/archive.go#L214)。
- 触发：两个明确授权的 fresh 初始化调用在取锁前分别形成归档路径 X、Y。A 先执行；B 获取锁后只复制 A 的 State/CompletedAt，没有采用 A 的 ArchivePath。
- 影响：A 已完成时，B 可返回 complete 却报告不存在的 Y；A 移到 X 后中断时，B 可继续初始化并将日志写成 Y。原数据仍在 X，但恢复记录失去正确定位。
- 改进：取锁后采用完整的既有 journal，或在锁内重新解析计划。保持显式 fresh 的授权边界，不触碰其他目录。
- 验证：预先构造不同 ArchivePath 的两份计划，覆盖首调用已完成和 moved 后中断。

### SOL-04 · 中 · 合法 Forever 家族 Source 与地区 Data 无法锁入项目

- 来源：sol_architecture；主代理核对结论：确认，当前 CLI 可形成该组合，不仅限于历史 pin。
- 证据：[当前 Source 家族](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/codebase/catalog.go#L32)、[SourcePin 保留产品](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/codebase/prepare.go#L81)、[合法组合判定](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/selection/products.go#L47)、[pinProduct 优先 Source](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/selection/project.go#L207)、[项目冲突检查](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/selection/project.go#L129)。
- 触发：source sync 选择 forever；target resolve 从该 Source pin 组合 forever-cn-beta 或 forever-asia-beta 的 Data；项目按对应 beta 初始化，随后 project lock 选择组合 pin。
- 影响：pin 合法，但 pinProduct 返回 forever，项目是地区 beta，lock/status 报 selection.project_conflict。
- 改进：组合 pin 以 Data/Changes 的交付产品确定项目身份；Source-only 再使用 Source 家族。继续拒绝不同地区 Data 混用。
- 验证：两种 beta 分别覆盖家族 Source + 地区 Data 的 lock、status 和项目选择；不需要访问真实 CDN 来验证该身份比较。

### SOL-05 · 中 · 统一错误分派覆盖 helper 已确定的退出码

- 来源：sol_architecture；主代理核对结论：确认，未运行 CLI。
- 证据：[原生 update 返回 3](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/update.go#L24)、[只保留 code==2](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/entry.go#L956)、[默认改为 5](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/entry.go#L1020)。
- 触发：原生 lycheedev update 未传 --release 且不是 plan。helper 明确提示需要 npm launcher 或已安装 release，并返回环境/入口限制 3。
- 影响：统一分派把未匹配 sentinel 的错误改成外部失败 5，调用方无法按约定区分环境准备和业务失败。
- 改进：保留已经明确分类的非零退出码，或为该环境限制提供统一错误类型；避免 helper 与 dispatch 各自覆盖分类。
- 验证：经 Execute 检查完整错误输出和退出码，包含 update 缺少 release 的情况。

追加第一轮补充（sol_r1_source_workspace，主代理确认）：[named target 适配器](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/target_preparer.go#L27) 把指定 build 的 ErrBuildUnavailable/ErrRemoteObjectMissing 包成 selection.ErrTargetBuildUnavailable，原错误仅按 %v 输出；[queryFault](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/query.go#L76) 没有这个 selection sentinel，最终也落到 5/command.failed。修正时为该 sentinel 补上 3/selection.target_build_unavailable 映射，并通过公共 Execute 覆盖。不能笼统宣称直接 resolve 总是返回 3：[target.go](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/target.go#L62) 本身也有返回该 selection sentinel 的路径。

### SOL-06 · 中 · 升级准备中断后，相同更新持续进入无法恢复的目录

- 来源：sol_source_data；主代理核对结论：确认，未执行取消或退出故障注入。
- 证据：[先创建 archive 再准备 next](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/delivery/upgrade.go#L63)、[之后才写 replacement.json](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/delivery/upgrade.go#L77)、[恢复必须存在 journal](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/delivery/upgrade.go#L126)、[根据目录存在进入 resume](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/delivery/update.go#L221)。
- 触发：创建恢复目录后、完整 journal 写入前取消、退出或 InstallFresh 失败。
- 影响：保留下来的目录没有可用 journal；相同更新再次运行只因目录存在就进入 ResumeUpgrade，持续失败。旧目标此时尚未移动；npm 编排可能已更新 CLI，使 CLI 与受管目标版本不同。
- 改进：先在独立临时位置准备完整 next/journal 再发布，或提前记录可恢复的 preparing 状态。不得以删除未知 archive 作为修复。
- 验证：在 mkdir 后、next 完成后、journal 尚未完整时分别中断，以原参数重试，保持旧安装完整并完成更新。

### SOL-07 · 中 · TOC 验证把已知索引缺口当作不存在

- 来源：sol_source_data；主代理核对结论：确认覆盖信息在当前路径丢失，未创建实际超限 fixture。
- 证据：[索引保留跳过记录](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/codebase/index.go#L355)、[不完整 summary](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/codebase/index.go#L391)、[ValidateTOC 丢弃 summary](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/codebase/validate.go#L105)、[类别已出现即认为未命中可判定](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/codebase/compat.go#L141)、[类别标记](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/codebase/compat.go#L209)。
- 触发：generated API/XML 文件因大小或解析问题未完全分析，其他文件已有同类声明；addon 使用的事件/API/template 可能在未分析文件中。
- 影响：ValidateTOC/Matrix 可能输出硬性的 event_not_found、api_not_found、xml_template_not_found。命中缺失只说明索引没有记录，无法证明来源不存在。ValidateSourceTOC 的 capture 完整性还只取决于 Valid。
- 改进：将相关覆盖信息传入 resolver；缺口下的未命中保持 unresolved/partial，保留已发现的正向事实及独立 Interface 基线。
- 验证：现有 validate fixture 增加一个正常 generated 文件、一个包含目标声明但超限的文件；目标应 unresolved，不应被证明不存在。完整覆盖的真实缺失仍应报错。

### SOL-08 · 中 · foreign-key 接受整数数组后静默返回完整空集

- 来源：sol_source_data；主代理核对结论：确认字段准入与匹配器不一致。
- 证据：[foreign-key 入口](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/db2_query.go#L286)、[仅检查 Kind](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/db2_query.go#L420)、[只匹配标量整数](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/db2_query.go#L449)、[ErrFieldType 约定](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/navigate.go#L46)。
- 触发：选择整数数组字段，如 fixture 的 Flags。字段 Kind=int 获准进入，但解码为数组后无法匹配 scalarMatches 的任何分支。
- 影响：所有记录被过滤，得到 Count=0、Complete=true，而非不可用于该角色的字段错误。
- 改进：在现有字段准入处拒绝数组并返回 ErrFieldType。此项不要求新增数组外键语义。
- 验证：复用数组 fixture，数组查询明确失败；标量整数查询保持正常。

### SOL-09 · 中 · DB2 JSONL 提前输出 end，后置失败丢失错误帧

- 来源：sol_source_data；主代理核对结论：确认顺序和错误处理不一致。
- 证据：[先输出 end](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/db2_query.go#L398)、[之后归档](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/db2_query.go#L410)、[最终来源检查](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/navigate.go#L396)、[分派假定已有 error frame](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/dispatch_data.go#L266)。
- 触发：行输出后，CheckSource 因本地来源变化失败，或 manifest 归档失败。
- 影响：stdout 已有 complete end，调用却以非零退出；分派不会再输出错误内容，消费者收不到具体原因。遵循“合法 end 且 exit 0”的消费者仍能判断失败，因此不把它列为静默成功或高优先级数据损坏。
- 改进：必要的来源核验和归档成功后才输出 end；输出正常的已开始流发生后置错误时发送 error frame。
- 验证：分别令最终来源检查与归档失败，确认有 error、没有 end、退出非零；输出管道本身断开另按 IO 错误处理。

### SOL-10 · 中 · doctor 修复被“必须已经修复”的安装检查阻断

- 来源：sol_live；主代理核对结论：确认原 CON 与旧磁盘版本的恢复死结，未实机复现。
- 证据：[先恢复/关闭再安装](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/live_doctor_windows.go#L85)、[要求当前干净版本](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/channel/project_windows.go#L231)、[进入动作前执行 guard](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/channel/project_windows.go#L424)。
- 触发：本项目还拥有未关闭 CON，磁盘 managed addon 版本不等于当前 CLI；运行 doctor --fix。
- 影响：fix 先 Resume/Connect，再 Disconnect，再升级；恢复或关闭通过 drive 时先要求当前版本已安装，因而在实际安装分支前失败。已经关闭的 CON 和其他项目占用不属于本触发条件。
- 改进：为原 CON 观察、关闭及已证明退役提供恢复路径；新业务继续要求干净当前安装。不得跳过身份、丢弃日志或用新 CON 绕过未知输入。
- 验证：同项目 idle CON + 旧安装应能沿原连接清理、保留报告并升级；外部项目占用和 submitted/uncertain 输入仍受保护。

### SOL-11 · 中 · bridge off 后异步探针仍存活，完成时重新启动展示

- 来源：sol_live；主代理核对结论：确认关闭路径没有执行取消句柄，未在游戏中运行。
- 证据：[Controls 关闭](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Core/Controls.lua#L87)、[SlotRuntime.Close](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Bridge/SlotRuntime.lua#L156)、[异步资源](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Bridge/ProbeExecution.lua#L14)、[完成后 collecting](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Bridge/SlotProtocol.lua#L99)、[展示重新计时](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Bridge/ActivityView.lua#L65)。
- 触发：已启用 bridge 并运行 api:Async/Guard 探针，在完成前执行 bridge off 或 disconnect。
- 影响：Close 只取消输入保护，不取消执行的 timer、guardFrame、回调。后续完成/超时会进入 collecting，重新启动展示 OnUpdate 和 timer。此项针对启用后关闭，不指默认未启用状态。
- 改进：引擎保留执行取消句柄；关闭清理异步资源，迟到回调不能执行业务或重启展示。保留已产生的报告事实。
- 验证：现有 t_slot_runtime 覆盖正常完成、超时、Guard 事件与迟到回调；关闭后资源归零。

## 补充审计：保留候选与未纳入项

### C-01 · 已确认并修复 · 长时间实体按键阻塞耗尽日志

- 来源：sol_live；审计时保留为候选，后续经完整 Driver 恢复轨迹确认。
- 证据：[INPUT 周期](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Bridge/InputState.lua#L79)、[每次尝试保存事件](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/channel/input.go#L58)、[checkpoint 条件](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/channel/history.go#L16)、[1024 条日志上限](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/journal/memory_log.go#L134)、[原恢复预算](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/channel/budget.go#L9)。
- 模型：玩家持续按住修饰键；每份新鲜 INPUT 后发送器明确返回 not_sent，重试持续写 input_intent/input_observed；pending 阶段不能 checkpoint。多个短 resume 沿原 600 秒预算累积事件。
- 复现：可控时钟与零输入发送器配合真实 INPUT 解析、Continue/Load 和持久日志，第 341 次尝试在原 600 秒预算内的 170.5 秒触及 1024 条上限；恢复和 RequestClose 均失败。这是离线生产路径复现，未向游戏连续发键。
- 修复：同一交换、runtime 与输入种类的上次结果明确为 not_sent 且排队消息数为零时，复用既有日志分段机制保存完整状态，再追加下一次意图；不改变原期限和未知输入约束。
- 回归：[input_journal_test.go](../internal/live/channel/input_journal_test.go) 覆盖 600 次重试和分段后的 Load，第 601 次未知输入不重发、原 600 秒期限生效，关闭意图仍能保存。

### C-02 · 合同已明确 · 分别表达 Wago 窗口完成与全查询覆盖

- 来源：sol_source_data；主代理判定：注释、scope 和实现/测试含义有分歧，未直接判为查询错误。
- 证据：[Coverage 注释](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/hotfix_remote.go#L198)、[到达末页判完整](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/hotfix_wago.go#L338)、[续页测试](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/hotfix_wago_test.go#L203)。
- 事实：Page/Cursor 不在查询 scope 中，但续窗到达末页可 complete=true；已有测试明确接受该结果。当前证据也支持“当前窗口完成”的解释。
- 实施：保留 Wago 窗口和 dbcache 快照各自既有 complete 语义，新增 fullQueryComplete 表示全查询覆盖；结果、capture、分页提示与回归同步更新。

### 未升级为确认问题

- CacheManager 不同 hash 的预算竞争：尚未证明当前生产调用，作为调查线索。
- 固定 doctor probe 文件的并发误报、非 capture BlobRef 的保留范围：证据不足，未列入修复承诺。
- npm 更新读取 latest：当前安装参考明确规定该行为，本次不作为缺陷，也不改变渠道政策。
- 没有因“大文件”“名字有 legacy”“测试使用旧版本号”直接判死代码；按前文的当前用途与迁移边界处理。

## 追加第一轮：检查既有发现之外的路径

本轮使用三个新的审计员，均为 GPT-6.1-sol、xhigh，并先阅读前述发现作为排除表。主代理重新阅读下列调用链、边界与现有测试后确认；没有运行生产路径或修改测试。

| 审计员 | 覆盖范围 | 归并结果 |
| --- | --- | --- |
| sol_r1_storage_assets | CASC/CDN、BLTE、Encoding/Root、本地索引、DB2 解码、SQL、Hotfix overlay、图片及 AVI 发布 | R1-06/07；C-04；两项排除 |
| sol_r1_source_workspace | Source 同步、索引/正文搜索、Context 分页、LuaLS/broker、项目与 target 公共分派 | R1-01/02/03；SOL-05 的新增触发；C-05 |
| sol_r1_native_mailbox | 当前槽协议、共享安装、失败报告、容量轮换、指令/RIP 根扫描、Mailbox 快照、Windows 输入 | R1-04/05；C-03；排除 BODY 重复读取假设 |

### R1-01 · 中 · Source 同步与 doctor 读取不同 Git 引用

- 证据：[PrepareSource](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/codebase/prepare.go#L62)、[CheckMirrorHealth](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/codebase/mirror.go#L41)、[当前 doctor 入口](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/dispatch_workspace.go#L204)、[人工写 branch 的测试](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/codebase/mirror_test.go#L58)。
- 触发：新 home 经正常 source sync 同步产品分支，随后在声明该产品的项目中运行 doctor。
- 影响：sync 只 fetch 到 FETCH_HEAD 并保存 refs/pins/<commit>；health 却读 refs/heads/<track>。生产代码没有维护后者，因而同步成功仍被报成未初始化，也无法给出更新可用提示。现有 health 测试自行创建 branch，绕开了实际同步流程。当前公开命令中没有 source check，不把它列为受影响入口。
- 改进：在既有 source lease 内维护已同步产品分支的 tracking ref，health 读取同一事实；不可变 pin 保留。同步任意 tag/commit 不能冒充产品分支当前 head。
- 验证：本地 Git fixture 串起 PrepareSource 与离线 health，再移动 remote branch 核对在线 health；旧 pin 仍指向原 commit。

### R1-02 · 中 · 无 topic 的正文搜索会被 README 等未索引文件中断

- 证据：[正文 Git 搜索](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/codebase/body_search.go#L29)、[空 topic 接受任意路径](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/codebase/body_search.go#L111)、[索引文件范围](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/codebase/tree.go#L67)、[命中后加载 document](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/codebase/search.go#L328)、[缺少索引记录返回错误](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/codebase/index.go#L655)。
- 触发：固定 Git 来源含 README/JSON/LICENSE 文本；无 topic 的 precise 查询进入正文 fallback，或 explore 同时命中源码与这些文本；该命中进入当前返回页。
- 影响：git grep 搜索了未进入 document 索引的文件，随后 cache.document 返回 os.ErrNotExist，整个 search 或包含它的 batch 失败。目录 fixture 只枚举 document，未覆盖真实 Git 路径的差异。
- 改进：Git 正文候选与可返回的文档集合一致；普通未索引文本不成为候选，超限源码仍按既有覆盖规则报告缺口。不要靠吞掉所有 document 错误隐藏损坏。
- 验证：同一 Git fixture 的 Lua 与 README 含相同标记，查询正常返回 Lua；仅 README 含标记时返回正常空集，保留超限文档的 partial 行为。

### R1-03 · 中 · Context 续页把旧 snippet 偏移带到新 snippet

- 证据：[snippet 数量/行预算停止分支](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/codebase/context.go#L304)、[下一页应用偏移](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/codebase/context.go#L323)、[已有续页测试](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/codebase/context_test.go#L13)。
- 触发：小 MaxBytes 将定义切成多页；一页从定义尾部续读并完成，剩余字节尚有余量，但 Limit=1 使下一处关系 snippet 留待下页。
- 影响：新 site 的 cursor 继续写入旧 lineOffset/byteOffset。下页从旧行号/字节开始读新 snippet，漏掉关系位置，或因偏移不合法返回 ErrInvalidSearchCursor。现有行预算测试走的是已清零的另一分支。
- 改进：只有继续同一个 site 才保留输入偏移；转入新 site 的两个停止分支写 0,0。
- 验证：小字节预算、Limit=1、Depth≥1，逐页拼接并与无分页的 snippets 对照；覆盖下一处关系在同文件和不同文件。审计员做过 ASCII 算法的内存推演，不作为生产 Go/CLI 复现。

### R1-04 · 中 · Live 成功返回 false 时丢失结果

- 证据：[报告包装](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Bridge/SlotProtocol.lua#L89)、[同步结果入口](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Bridge/ProbeExecution.lua#L140)、[异步 Finish](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Bridge/ProbeExecution.lua#L103)、[编码器支持布尔值](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Bridge/CaptureWriter.lua#L73)。
- 触发：探针 return false，或异步 api:Finish(false)。
- 影响：result=ok and value or nil 把成功的 false 变成缺失字段。HEAD/BODY、确认和释放都可以正常完成，持久报告却丢失返回值。
- 改进：按 ok 显式赋值 result/error，保留 false。无需修改编码器或协议名称。
- 验证：现有 Lua 协议用例检查同步、异步 false 的 BODY 与最终报告包含 result=false；true、nil 和失败结果不回归。

### R1-05 · 中 · 编译失败使 operation 容量与槽位轮换判据错开

- 证据：[121 个 operation 上限](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Bridge/SlotProtocol.lua#L7)、[拒绝与编译失败也占名额](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Bridge/SlotProtocol.lua#L139)、[Go 跳过 commit](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/channel/driver.go#L375)、[仅按槽位决定重载](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/channel/wait.go#L199)、[持久拒绝直接返回](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/channel/driver.go#L167)。
- 触发：同一 runtime 完成并释放 120 个正常操作与 1 个编译失败操作。bind 加操作消耗 1+120×4+3=484 槽，NextSlot=485，operation 已达 121。
- 影响：第 122 个请求不会触发 Go 的 NextSlot>485 自动重载，Lua 却返回 slot_capacity；原 transaction 保留 rejected，resume 继续返回拒绝。纯成功路径的容量测试不能覆盖该轨迹。
- 改进：将 prepare 的精确 slot_capacity 回执作为该请求尚未执行的事实，完成原拒绝的持久处理后沿同一 request/预算启动容量重载；也可让 Lua 提供真实剩余 operation 容量供 Go 预判。不能仅提高上限或重放未知输入。
- 验证：在现有 Go/Lua 槽轮换用例加入一次编译失败，原请求在新 runtime 继续，原业务期限不延长。

### R1-06 · 中 · UNION ALL 错把重复输出列名判为歧义

- 证据：[最终 compound 投影](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/relational/compound.go#L97)、[按名称重新绑定](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/relational/rows.go#L15)、[生产执行入口](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/sql.go#L173)。
- 触发：SELECT ID, ID FROM ChrRaces WHERE ID=1 UNION ALL SELECT ID, ID FROM ChrRaces WHERE ID=2。每个分支合法，列宽一致。
- 影响：合并后再次按显示列名 ID 构造列引用，绑定器遇到两个 ID 返回 ambiguous，合法查询整体失败，即使没有 ORDER BY。
- 改进：最终投影使用列序号或内部唯一身份，显示名仍保留原值；用户写 ORDER BY ID 时，重复名字仍应按既有规则报歧义。
- 验证：重复输出名无排序查询成功；唯一 alias 的排序/LIMIT 正常；歧义排序仍失败。

### R1-07 · 中 · AVI 内部短 chunk header 被当成完整输入

- 证据：[movi 遍历条件与返回](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/video/avi.go#L235)、[complete 赋值](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/video/avi.go#L151)、[header LIST 相同边界](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/video/avi.go#L189)、[demux 归档](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/video_demux.go#L306)、[现有部分输出合同](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/skills/lycheedev/references/asset-export.md#L172)。
- 触发：有效 VP9 首帧之后，movi 内只有下一 chunk 的四字节 00db，没有 size；外层 LIST/RIFF 长度与实际文件一致。
- 影响：不足八字节时循环直接结束，没有检查残尾，ParseAVI 返回 complete=true；demux 可导出先前帧并发布完整 manifest。顶层与 header LIST 也有相同边界。allow-partial 当前只授权输出限量，不授权损坏输入。
- 改进：每个 chunk 遍历结束时检查游标到达其边界；保留已明确允许的最后一个奇数长度 chunk 缺 pad 特例。
- 验证：movi/header/top-level 各有 1–7 字节残尾时返回截断错误；完整 VP9、缺末尾 pad 特例和合法限量输出保持正常。

### C-03 · 已确认并修复 · publication 变化后旧对象失效被报成永久读取错误

- 来源：sol_r1_native_mailbox；审计时保留为候选，后续经可控读取 fixture 确认；不宣称复现了真实 GC 时序。
- 证据：[旧对象读取失败立即返回](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/memory/lua_mailbox.go#L445)、[成功后才核验路径](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/memory/lua_mailbox.go#L477)、[仅 PathChanged 等归为 pending](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/channel/wait.go#L18)、[现有 publication 测试](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/memory/lua_mailbox_publication_test.go#L17)。
- 候选轨迹：INPUT 已替换指针，旧 TString 在读取期间被回收/复用；旧头、长度或记录解析先失败，没有检查已有路径 guards，暂时变化被当作布局错误。
- 实施：六组 beforeRead fixture 覆盖 INPUT 替换与旧头、载荷不可读或布局失效；失败后核验 guards，路径确实改变才返回 PathChanged，稳定路径保留原故障。回归通过。

### C-04 · 已支持 · AVI movi 内嵌 LIST rec

- 来源：sol_r1_storage_assets；主代理确认 [enumerateFrames](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/video/avi.go#L240) 只处理直接 00dc/00db，嵌套 LIST 整段跳过。
- 原缺口：审计时未明确 flat-only，也没有实际 WoW 样本或既有验收证明必须支持 LIST rec，未将其称为已确认真实资源漏帧。
- 实施：按标准 AVI 结构支持有深度边界的 LIST rec 遍历，结构 fixture 与短 header/截断回归通过；仍无真实 WoW rec 样本，不扩大实机覆盖。

### 第一轮排除项

- 多文件 overwrite 留下旧 manifest：现有合同允许失败留下部分文件，并要求按 manifest 大小/hash 复核；没有整组原子回滚承诺，不据此单独报已宣告成功。
- filemeta 未准备 key-file：相关两个入口当前只有测试调用，未连接 CLI；不列生产故障，后续整理内部 API 时决定保留用途。
- BODY 双读必然超过 1 MiB：当前完整 BODY 只读一次，排除该假设。
- 未接入生产的 ResolveIntent/operation-reference helper，以及显式 static-only/partial、跨 home 只导入项目固定引用：未据此新增行为缺陷；内部 API 的保留需按用途判断。

### C-05 · 已确认并修复 · broker 关闭成功早于进程清理完成

- 来源：sol_r1_source_workspace；审计时保留为候选，后续经真实 named pipe、LuaLS 与受控调度屏障确认。
- 线索：[服务端 close 回应](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/source_broker.go#L409) 早于 handler 收尾、listener 取消；[客户端 close](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/source_broker.go#L537) 收到回应即返回。立即发起未命中语义缓存的 reuse 查询，可能碰到仍在退出的 broker。
- 边界：真实 LuaLS 与 worktree lease 在回应前已释放，符合 [关闭后回收 worktree 的用途](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/skills/lycheedev/references/source-research.md#L125)，不是 prune 占用问题；listener、broker owner 与 aggregate 租约仍有收尾窗口。
- 复现：在 closed 回应后暂停原 handler，旧客户端已返回成功，服务端明确读到随后 status 请求但无法响应，直至超时；第二屏障在 listener 关闭后确认两层 owner 租约仍被持有。
- 修复与回归：两个关闭入口按握手得到的 PID/创建时间等待原进程退出，取消只停止等待。覆盖两种屏障、原期限、PID 复用、关闭后重新连接与未缓存查询；见 [broker 关闭回归](../internal/command/source_broker_retirement_windows_test.go) 和 [进程等待回归](../internal/luals/broker_exit_windows_test.go)。前者已接入发行组装的九项必跑真实集成。

## 追加第二轮：新的三名 Sol 审计

本轮在第一轮写入后启动，三名代理均读取该文档作为排除清单，模型为 GPT-6.1-sol、推理强度 xhigh。与第一轮及最初补充审计的代理均不同。主代理重新读取调用者、实现和相邻测试，收录以下 11 项确认问题；3 项候选与排除项另列。没有调用更多审计轮次。

| 代理 | 范围 | 核对后结果 |
| --- | --- | --- |
| sol_r2_addon_workbench | Core、工作台模块、持久化、执行/序列化、对象搜索、UI 导出和启停 | R2-01 至 R2-04；C-06 |
| sol_r2_cli_integrity | CLI 参数、目标/pin/project、Data 准入、CacheManager、evidence | R2-05 至 R2-08；C-08 |
| sol_r2_distribution_skill | release/CI、npm 更新、受管部署与卸载、Skill 安装和编排 | R2-09 至 R2-11；C-07；R2-11 由候选经主代理核对后升级 |

### R2-01 · 中 · 打开工作台仍会改写更高版本的存储区

- 证据：[history 版本判断后继续规范化](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Core/Stores.lua#L275)、[exports 重建和裁剪](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Core/Stores.lua#L297)、[打开窗口初始化](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/UI/Window.lua#L218)、[现有保留未来版本数据的测试](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/tests/addon/t_stores.lua#L179)。
- 触发：根 schema 仍为 1，但 history 或 exports 的 section version 大于当前支持版本。打开工作台设置 writesBlocked 后，仍按 v1 结构处理原始数据。
- 影响：较长历史正文被裁剪，未知格式的导出记录被过滤。History.Clear、Exports.Delete/Clear 也未使用现有版本状态。这里确认的是已声明的降级兼容合同失效，不声称已经存在发布的 v2 数据损失。
- 改进：在持久化边界识别未来 section 后，停止对该 section 的规范化和裁剪；新增、清空、删除共同遵守该状态。UI 显示该 section 暂不可写，不把未知格式改造成 v1 默认值。
- 验证：扩展现有 future-version fixture，加入长正文及未知记录结构；初始化、读取及各修改入口后，原 section 深度相等；当前版本正常预算裁剪保留。

### R2-02 · 中 · 对象搜索先截断再排名，漏掉已扫描的精确匹配

- 证据：[排名合同](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Modules/ObjectInspector.lua#L260)、[任意 bucket 先占满总容量](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Modules/ObjectInspector.lua#L302)、[遍历结束后才排序](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Modules/ObjectInspector.lua#L369)、[UI 搜索入口](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/UI/Pages/Object.lua#L566)。
- 触发：SearchValue({AlphaPrefix=1, Child={Alpha=2}}, "alpha", 1)。根表先完整枚举，prefix 占满结果；随后访问 Child，精确 Alpha 虽计入 totalMatches，却不能入选。不依赖 Lua 哈希遍历顺序。
- 影响：返回较低排名结果并遗漏已找到的精确目标；默认 200 条时，200 个根表前缀匹配和一个子表精确匹配同样触发。
- 改进：有界保留按 rank/key 排序的最佳 N 项，或每个排名 bucket 保留 N 项再合并截断；保留原扫描深度、节点及截断说明。
- 验证：上述嵌套 fixture 与默认 200 条混合排名；同时保留现有少量排序、宽表限量和 secret 值测试。

### R2-03 · 中 · Run 导出把旧结果与新编辑器正文配对

- 证据：[结果只保留文本，代码写入历史](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/UI/Pages/Run.lua#L274)、[导出临时读取编辑器代码](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/UI/Pages/Run.lua#L404)、[清空只改变输入](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/UI/Pages/Run.lua#L309)、[现有证据配对测试](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/tests/addon/t_run.lua#L78)。
- 触发：执行 return 1 后，把编辑器改成 return 2 或清空，未再执行即点击保存。恢复历史后再编辑也可触发。
- 影响：run_result 的 payload 仍为旧结果，metadata.code 却成为新代码或空串，落盘证据错误归因。
- 改进：当前结果模型保存实际执行的 normalizedCode；执行完成及历史恢复同步更新，导出从结果模型取代码。
- 验证：执行 A → 编辑 B/清空 → 保存，以及恢复历史 → 编辑 → 保存；结果始终与 A 配对，保存不增加执行次数。

### R2-04 · 中 · 结果格式化抛错逃出 Execute，跳过结果和历史更新

- 证据：[仅 chunk 在 pcall 中](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Core/Execute.lua#L97)、[错误和结果格式化在其外](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Core/Execute.lua#L101)、[排序中的 tostring](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Core/Serializer.lua#L27)、[UI 直接调用 Execute](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/UI/Pages/Run.lua#L305)。
- 触发一：error(setmetatable({}, {__tostring=function() error("format failure") end}))，格式化错误对象再次抛错。
- 触发二：local k=setmetatable({}, {__tostring=function() error("format failure") end}); return {[k]=1, [{}]=2}。chunk 已成功，两键排序时 tostring(k) 抛错。此例无需依赖客户端是否提供 newproxy。
- 影响：Run 回调抛错，ApplyRunOutcome 未执行；结果、诊断和历史没有更新，用户 chunk 的副作用却可能已经发生。
- 改进：在结果格式化边界捕获格式化异常，返回可展示诊断，保留既有输出及实际执行事实；不能为补结果再次执行 chunk，也不需要给每层包一遍 pcall。
- 验证：错误对象格式化失败、成功结果排序失败、chunk 已产生一次副作用；UI 与历史保留诊断，执行次数仍为一次。

### R2-05 · 中 · live probe list 公布的 201–1000 上限不可用

- 证据：[公开合同为 1–1000](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/command_contract.go#L104)、[模块同样接受 1000](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/probe_registry.go#L105)、[解析器拒绝点](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/entry.go#L1523)、[该路由落入默认 200](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/entry.go#L1735)。
- 触发：live probe list --limit 201 至 1000，在读取 workspace 之前就返回参数错误。
- 改进：在既有解析上限中对齐该路由与合同，不扩大其他命令的限额，不另外搭建合同框架。
- 验证：经公共 Execute 入口检查 200、201、1000 合法，1001 拒绝；不能只直接测 ListProbes。

### R2-06 · 中 · 已删除探针先占分页窗口，活动探针被隐藏

- 证据：[先 LIMIT 后过滤 Active](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/probe_registry.go#L110)、[按 key 排序的文档页](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/vault/metadata.go#L218)、[结果没有 cursor/截断信息](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/live/probe_registry.go#L45)。
- 触发：正常 put a、z，再 remove a，live probe list --limit 1 成功返回 items=[]，活动 z 不显示。默认前 50 个名字均已删除时，默认列表也可为空。
- 影响：删除历史逐步挤掉活动名称；合同说的是列出活动名称，没有把 limit 定义成原始文档扫描预算。
- 改进：沿现有文档分页读取，填够活动 limit 或到达末尾；若设置额外扫描上限，必须提供截断和续读位置。保留 tombstone、不可变 revision 和历史操作引用。
- 验证：deleted a + active z、include-removed、全部删除和正常限量列表；当前只有唯一删除名的测试不足以覆盖。

### R2-07 · 中 · 大小写双名可创建，选择和删除却命中另一配置

- 证据：[允许大写](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/selection/target.go#L429)、[按原名称保存](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/selection/target.go#L137)、[多个匹配取首个忽略大小写的名称](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/selection/target.go#L244)、[删除也使用同一匹配器](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/selection/target.go#L295)。
- 触发：target add Foo 和 target add foo 均成功；之后指定 foo，匹配器取排序在前的 Foo。
- 影响：show/resolve 使用另一产品或地区配置；没有活动引用阻止删除时，remove foo 也删除 Foo 的配置和引用账本。固定 pin 内容不因此被删除。
- 改进：先匹配字节精确名称，再接受唯一的忽略大小写精确/前缀匹配；多个折叠精确匹配返回歧义。现有 Foo/foo 能继续各自访问，不强制重命名全部配置。
- 验证：Foo、foo 分别精确 show/resolve/remove；FOO 报歧义；保留普通唯一前缀和活动引用阻止删除行为。

### R2-08 · 中 · named target 的相对安装路径随后续 cwd 改变

- 证据：[保存原始 installation](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/dispatch_workspace.go#L54)、[直接序列化](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/selection/target.go#L132)、[原样交给本地解析器](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/target_preparer.go#L20)、[解析时才 Abs](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/installation_selection.go#L54)。
- 触发：在 A 目录添加 --installation ./game，之后在 B 目录用同一 home 解析该名称，实际读取 B/game。
- 影响：解析失败，或选中另一份同产品安装和 build。named target 允许重新观察 build，不意味着保存的安装位置应随 cwd 移动。
- 改进：创建/replace 时固定为当时绝对路径；无需强制目录存在或提前解析 symlink。旧相对配置没有记录原 cwd，不能猜测迁移位置，应列出并通过已有 replace 入口保存明确路径。
- 验证：两个 cwd、两份不同 build 的本地安装 fixture，添加后解析仍使用 A；绝对路径和 remote target 不受影响。

### R2-09 · 中 · 已认可的旧槽 TOC 无法卸载

- 证据：[认可精确旧 TOC](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/delivery/slots.go#L81)、[卸载初检使用 InspectSlots](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/delivery/slot_remove.go#L43)、[随后只接受当前 TOC hash](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/delivery/slot_remove.go#L166)、[公开 remove 调用](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/entry.go#L420)。
- 触发：主 addon 干净受管；slots.v3、500 槽、ready、无未解决预约；TOC 使用已支持的旧 Interface 或 transport 内容。正常 addon remove 初检通过。
- 影响：写入 removal intent、创建 slot archive 后，在首次移动之前返回 ErrConflict；原输出目录重试仍失败。未知文件修改不在这个结论内。
- 改进：卸载静态文件校验复用 slotStaticMatches 的精确认可集合；保留身份、payload hash、文件集合和预约检查，将原字节归档。
- 验证：旧 Interface/transport 卸载及部分移动后续跑；未知 TOC、Loader 修改仍拒绝。当前卸载测试只覆盖新生成 TOC。

### R2-10 · 中 · npm 死锁回收再次中断后无法自动恢复

- 证据：[空 reclaim.lock 的创建和 finally 删除](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/packages/npm/lycheedev/bin/update.mjs#L49)、[真实更新入口](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/packages/npm/lycheedev/bin/update.mjs#L96)、[现有单次死主锁恢复测试](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/tools/update-isolated.test.mjs#L134)。
- 触发：已有死进程 lock.json；重试成功创建 reclaim.lock 后、删除主锁之前被终止。
- 影响：第三次按原参数重试持续得到 EEXIST/update.busy；reclaim.lock 没有 owner 和恢复读取路径，固定 tarball/targets 无法继续。targets=[] 也成立，与 SOL-06 的 Go 升级 journal 缺口不同。
- 边界：若中断发生在主锁删除之后，下次可能正常取得主锁；不把所有回收中断都说成永久阻塞。
- 改进：让回收互斥随进程结束释放，或提供可核实死 owner 的恢复机制；保留两名回收者不能删除新 owner 主锁的保护。
- 验证：在明确 checkpoint 中断回收者，第三进程沿原 package/targets 恢复且不再查询 registry；并发回收不能误删新主锁。

### R2-11 · 中 · 发布回读把 registry signature 当作 provenance

- 证据：[signatures 替代 attestations 的分支](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/tools/release.mjs#L770)、[CI 将结果称为 provenance](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/.github/workflows/toolkit-release.yml#L516)。
- 主代理核对：npm 的 [registry signature](https://docs.npmjs.com/about-registry-signatures/) 签名覆盖包名、版本与 tarball integrity；[provenance attestation](https://docs.npmjs.com/generating-provenance-statements/) 提供源码和构建来源证明。官方验证输出也分别统计两者，不能相互替代。
- 触发：registry-view 的 name/version/integrity/repository 都匹配，没有 dist.attestations，但 dist.signatures 非空。
- 影响：校验返回 ok=true，并把 signatures 写入 attestations 字段，发布流程误称来源证明回读通过。没有证据表明某次已发布包真的缺 provenance，也不据此宣称 OIDC 发布失败。
- 改进：要求实际 provenance 元数据，registry signatures 单独报告；保留现有 tarball 身份校验。元数据存在性与密码学验证须准确命名，不用签名存在冒充构建来源已验证。
- 验证：身份匹配的 signatures-only fixture 拒绝；合法 attestations fixture 通过；身份或 integrity 不符继续拒绝。

### C-06 · 已确认并修复 · FunctionTrace 的 hook 回调重入

- 来源：sol_r2_addon_workbench；主代理确认 [OnCall](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Modules/FunctionTrace.lua#L60) 会调用 GetTime、InCombatLockdown、math.min/max，[Start](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/addon/Modules/FunctionTrace.lua#L93) 可选择这些函数，未有记录重入标志。
- 原缺口：[审计时的 stub](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/tests/addon/t_trace.lua#L194) 只保存回调，不能证明或排除递归。实机脚本先设置入口计数与停止条件，再按用户授权执行。
- 实机复现：亚服 Forever 1.60.1.70374、Ailuoli-Free、PID 36576，全程非战斗。GetTime/InCombatLockdown 的 global-string 和 table-method hook 均可重入；真实 Trace.Start 在修复前均触发第 3 次入口停止条件，guardCalls=3、cutoff=true、runningAfterCall=false。math.min/max 在该客户端明确禁止 hook，未新增兼容分支。
- 修复：记录前设置重入标志，以 pcall 确保记录或观察回调抛错后解除；离线 stub 改为实际执行 hook，旧实现稳定失败，修复后完整 t_trace.lua 通过并覆盖异常后恢复。
- 实机复验：通过受管安装部署，在同一 PID 上执行相同 Trace 脚本；两者均 guardCalls=2、cutoff=false、recorded=1、runningAfterCall=true。修复前后报告分别为 LMO-674610bd8cd42f880cc983016a42b40a、LMO-5ccd3b91a8e3581ccf1783122364b2ee，均 verified、report.ok=true、cleanup=complete。最后独立 reload 销毁临时 hooks，两条 CON 已关闭；doctor 全绿、unclaimed、500 槽、0 pending reservations。

### C-07 · 合同已明确 · 默认更新不重新安装已移除的记忆目标

- 来源：sol_r2_distribution_skill；主代理确认 [记忆目标直接进入更新](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/command/update.go#L69)，remove 未删除该库存，[absent 目标会安装](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/delivery/update.go#L227)。
- 可达轨迹：update 记住自定义目标 → 正常 remove → 无选择参数的 update，原目标重新出现；这里确认行为，未将其定性为 bug。
- 原缺口：[安装参考](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/skills/lycheedev/references/installation.md#L22) 说记忆用于默认发现，又只明确描述 absent explicit target 的安装；当时未明确库存是历史发现还是持续安装意图。
- 合同与实现：库存用于历史发现。默认发现剔除已移除且无活动恢复事务的记忆项，保留显式安装及原事务恢复；两种边界的回归通过。

### C-08 · 合同已明确 · download.workers 控制工作区下载并发上限

- 来源：sol_r2_cli_integrity；主代理确认 [字段称并发下载预算](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/vault/config.go#L50)，生产读取只在值为 1 时 [把查询槽从 2 改为 1](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/vault/data_resources.go#L74)；2–256 没有差别。在线 CDN fallback [固定 width=8](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/records/cdn_files.go#L377) 并发请求 archive。
- 反证：[准入注释](https://github.com/Follen/Lychee-Dev/blob/1f7b5977bd509422128d7c14bc8c4aeabb1ca07f/internal/vault/data_resources.go#L45) 明确选择保守的全查询预约；现有测试也验证查询串行化，不能直接当作下载预算实现遗漏。
- 合同与实现：正值限制工作区实际 Data HTTP 并发，不承诺充分用满。复用既有全查询预约：1 为单查询单下载；大于 1 时两个查询各分配 floor(n/2)、每个最多 8 个下载；0 保持自动值。配置已接到现有 CDN 调度，同时准入两次查询的回归确认聚合请求数不超过配置。

### 第二轮排除项与覆盖边界

- 未知 profile 下 Catalog 的 #nil 线索被当前 Startup/Controls 工作台入口阻断；没有可达用户路径，不计 bug。
- 持久 hook 与惰性安全驱动的残留不等于禁用状态仍工作；未据此新增生命周期问题。locale 键集及现有 L.* 引用未发现新的静态缺项。
- npm update 读取 latest 符合当前合同；release channel 身份绑定版本，latest=2.5.1 的政策没有因审计改变。
- old-moved、cleanup marker 存在真实恢复分支；未把 SOL-06/R2-10 泛化为所有升级都无法恢复。corresponding-source artifact 已提供 sha512，排除“seal 必然缺 hash 失败”。
- evidence bundle 的字节核验、去重和拒绝覆盖未新增确认问题；Data 嵌套调用已有 admitted 复用和失败释放路径。
- CacheManager.Put 没有当前生产调用；预算竞争、损坏文件保留、取消读被分类为损坏等库级线索没有升级为生产 bug，也不以无当前调用为由直接删除整个库。
- 当前 Skill 的旧执行编排、PATH 选择和安装问题已归入前文，未重复计数。审计覆盖不等于证明所有未报告路径都无缺陷。

## 审计阶段核对结论

审计阶段已逐项核对 38 项确认问题的入口、具体触发、影响边界和修改位置。它们已转为本轮实施项，当前结果见文首。运行时与恢复改动保留原请求、期限和已发生副作用，不靠重放未知输入或丢弃记录完成恢复。

原有表述已校正：环境变量门控的数量按 27 个顶层测试、21 个文件统计，子进程 helper 不算覆盖缺口；不能从当前 CI 推断所有历史发布从未运行 QR 测试；navigatetest 保留其唯一行为覆盖；无生产调用与死代码分别判断。退出码新增触发并入 SOL-05，没有重复计数；R1-01 的实际公开入口是 doctor，不虚构 source check 命令。

审计时旧 QR/Receiver/heap 路径仍有共享类型、历史读取或占用释放消费者。本轮已迁出必要部分，再退役实现、测试、fixture、命令和 Skill 引用，并处理 D-07 的无调用符号。Rust worktree 与未解决恢复记录继续保留。

审计时保留的 8 项候选已分别处理：C-01/03/05 经可控生产路径回归、C-06 经实机复现并修复；C-02/04/07/08 已明确合同或支持范围并落实实现。没有证据支持的假设不作为添加限制、保护框架或重写模块的理由。

## 处理核对表

已完成项依据本轮修改和实际回归；实机与离线覆盖分别列明。

- [x] CR-01/02、AR-02：关闭旧输入执行面，完成 D-01/02/03/06 的依赖拆分和退役。
- [x] SOL-01/02/03：修复证据生命周期、代次复用及归档原计划恢复。
- [x] CR-03/04、SOL-06/10/11：修复当前等待、stage、升级、doctor 和关闭生命周期。
- [x] SOL-04/05/07/08/09：修复产品锁、退出码、Source 覆盖和 Data 结果合同。
- [x] CR-05/06/07/08/09、AR-01/03：统一启动、公开合同、打包、Lua 必需模式、有效集成验证和共享解析器。
- [x] D-04/05/07：迁出共享读取类型，删除旧扫描、历史台账和无调用符号。
- [x] 文档/Skill 收拢、生成引用和链接检查完成；Rust 与用户恢复记录保留。
- [x] C-02：明确窗口与查询覆盖，保留已有 complete 语义并补充 fullQueryComplete。
- [x] C-01：复现原期限内实体键阻塞耗尽日志，修复分段并保留原期限与未知输入约束。
- [x] R1-01/02/03/06/07：修复 Source 状态、搜索与分页，以及 SQL/AVI 的具体数据边界。
- [x] R1-04/05：保留布尔结果，打通混合成功/编译失败的槽容量轮换。
- [x] C-03/04：可控读取 fixture 确认并修复发布路径竞争；支持标准 AVI rec 结构。
- [x] R2-01/03/04：保留未来版本存储，修复 Run 证据配对和格式化异常出口。
- [x] R2-02/05/06/07/08：修复搜索排名、探针列表、名称选择与持久安装路径。
- [x] R2-09/10/11：对齐卸载认可集合、修复 npm 回收锁恢复、纠正 provenance 回读。
- [x] C-07/08：明确并实现更新发现库存与下载并发上限合同。
- [x] C-05/06：复现并修复 broker 关闭交接与真实 hook 回调重入，完成关闭后查询与同客户端复验。

## 验证与交付边界

主批次验收基于当时实现冻结后的同一工作区，发行版本源保持 3.1.2。已通过：

- `go build ./...`、`go vet ./...`。
- `LYCHEEDEV_REQUIRE_LUA51=1` 下执行 `go test -json -p=2 -parallel=4 -count=1 -timeout=20m ./...`，退出 0；31 个测试包通过，4 个包无测试。Live channel 用时约 447 秒，delivery 约 816 秒。早期分组的短时限超时与并改期间编译失败不作为通过依据，由冻结后的完整运行覆盖。
- 从该次 Go 事件确认 BASE-01 至 BASE-21 全部通过；八项真实 LuaLS、broker 和安装包 Source 语义集成均实际执行且无子测试跳过。所用 LuaLS 为固定的 3.19.1，经过发行清单哈希核验；本地开发包明确标记 developmentOnly，未发布。
- `go test -tags=casc_oracle -p=1 -parallel=2 -count=1 -timeout=20m ./tests/data-regression/compare` 通过。这两项为自包含的参数/JSON 入口检查，不代表外部 CascLib 实际数据对照通过。
- Node 工具及 npm launcher 共 75 项实际通过、无跳过，包括 localhost fixture registry 和临时 prefix 内的真实 npm 升级、双客户端与 Skill 更新及中断恢复。
- 版本一致性、Skill 合同、生成命令目录和 Skill 格式校验通过；Skill 合同为 84 条命令、220 个引用、0 违规。文档本地链接与 `git diff --check` 通过。

跳过项没有计作覆盖：当前 Windows 权限不允许创建符号链接的 7 项用例；缺少固定 API 文档、完整 Retail 工作区、真实图片/CASC/索引/运行时 dump，以及未指定授权进程或交互桌面的可选测试。8 个子进程入口在顶层按设计跳过，其父测试已执行，不将这些入口重复列为能力缺口。

后续按用户授权补充 C-01/05/06：最终代码的 build/vet、command/luals 整包及新增确定性回归通过。最终开发包重新组装后，Live/channel、journal、codebase、addon/protocol 五个包与包内 CLI 语义测试通过；channel 用时约 334 秒。C-01 长重试、真实 LuaLS 映射、跨 CLI broker 与安装包语义均实际执行，无子测试跳过；仅保留已说明的符号链接权限及子进程 helper 跳过。新 broker 关闭回归及发布 workflow 结构检查通过，九项必跑真实集成均有通过结果。未重跑未受这些追加改动影响的 delivery 长套件和 Node 全套。

C-06 完成上述亚服 Forever 受管部署、实机前后对比及连接清理。主代理核对两条连接均 closed、报告 verified、cleanup complete；已按用户授权用固定范围脚本删除本次两个临时目录，包括测试包、探针和已结束的恢复单元。清理脚本自身也已删除。

本轮未重跑七产品实机矩阵，其他产品的证据保持原范围；未更新用户的 Skill 安装。修复完成时尚未提交，后续提交范围与待办见 handover.md；没有合并或发布。Rust worktree 与未解决恢复记录继续保留。

## 修复后追加审计：GPT-6.1-sol xhigh，两轮

基线为 2026-10-14 追加审计开始时的 main 工作区：当时 HEAD 为 1f7b5977bd509422128d7c14bc8c4aeabb1ca07f，包含上文已完成但未提交的修复。审计当前文件及实际调用关系，不以该 HEAD 的旧源码代替修复后的实现；新增证据使用当前文件引用与行号。

三名子代理按 Live/addon、Source/Data、CLI/交付/发行分区完成第一轮；第二轮轮换分区，交叉核实发现并补查遗漏。两轮均只读代码，使用隔离的离线 fixture 核实具体疑点；主代理完成去重、当前调用链与复现证据复核，并更新本文件。本次没有操作游戏、修改产品代码、提交或发布。

| 轮次 | 状态 | 确认问题 | 候选 |
| --- | --- | --- | --- |
| 第一轮（R3） | 已完成 | 6 项 | 3 项，交第二轮核实 |
| 第二轮（R4） | 已完成，分区交叉复核完成 | 7 项，含 R3-C02 升级确认 | 新增 1 项，第一轮另 2 项继续保留 |

最终合计 13 项确认问题，保留 3 项候选，候选未重复计入确认数量。

| 代理（均为 GPT-6.1-sol、xhigh） | 第一轮范围 | 第二轮范围 | 状态 |
| --- | --- | --- | --- |
| sol_audit_live | addon、Live、bridge、desktop、协议与相关测试 | CLI、delivery、npm/Skill、tools、release/CI、基础模块与相关测试 | 两轮完成 |
| sol_audit_data | Source、Data、selection、vault/evidence、LuaLS 与相关测试 | addon、Live、bridge、desktop、协议与相关测试 | 两轮完成 |
| sol_audit_delivery | CLI、delivery、npm/Skill、tools、release/CI、基础模块与相关测试 | Source、Data、selection、vault/evidence、LuaLS 与相关测试 | 两轮完成 |

### 新增发现

下列问题来自修复后的工作区，尚未实施修复。子代理执行隔离复现，主代理检查当前调用链与证据；第一轮六项均由另一名代理在第二轮交叉确认，未重复运行已通过的 fixture。未获得可达触发与实际错误证据的线索不计作确认问题。

#### R3-01 · 中 · Trace 输入的末级 owner 不是表时，错误逃出页面点击回调

- **本轮状态：代码已修复；Lua 5.1 六 profile 的真实 Trace 按钮回归通过；待实机验证。**
- 修复与正式验证：在函数目标解析边界保护末级读取、验证函数，移除 Start 的重复无保护索引；保留 frame、继承方法及可索引 userdata 的既有 hook 合同。正式回归：[t_trace_owner.lua](../tests/addon/t_trace_owner.lua)。
- 最小实机复验：同版受管 addon、非战斗状态下打开 Trace，依次开始 GetTime.foo、标量父路径和正常表方法。预期前两者显示原有失败状态且点击不抛错，正常目标仍能开始/停止。
- 实机结果：

- 证据：[FunctionTrace.lua:108–114](../addon/Modules/FunctionTrace.lua#L108)、[Trace.lua:219–225](../addon/UI/Pages/Trace.lua#L219)、[ObjectInspector.lua:199](../addon/Modules/ObjectInspector.lua#L199)。
- 触发：在工作台 Trace 输入 `GetTime.foo`，点击开始。路径语法合法，父路径解析为已存在的 GetTime 函数；末段不是可追踪函数，应通过现有失败结果提示用户。
- 影响：ResolveFunctionTarget 返回成功后，Start 直接执行 `owner[functionName]`，在返回 FUNCTION_NOT_FOUND 之前抛 Lua 错误；真实 OnClick 没有承接，页面的错误状态与按钮同步分支没有执行。
- 离线复现：在现有 t_trace.lua 的真实页面 fixture 中捕获输入框并触发 `traceButton:Click()`，观察得到 `attempt to index local 'owner' (a function value)`。仅观察用的外层 pcall 返回 false；未运行实机。
- 改进与回归：在函数目标解析边界拒绝不可索引的 owner，并返回既有错误；覆盖函数/标量父路径和正常表方法的真实按钮操作。此项发生在启动前，与 C-06 的 hook 回调重入独立。

#### R3-02 · 中 · Source context 的 flow 丢弃已选声明的位置

- **本轮状态：代码已修复；离线回归通过。**
- 修复与正式验证：flow 的 Symbol 选择同时保留已解析 Path/Line；精确 ID 不再重新产生同名歧义，只给名称仍报告真实歧义。正式回归：[context_test.go](../internal/codebase/context_test.go) 的 TestContextSourceFlowKeepsExactSymbolIDLocation、[flow_test.go](../internal/codebase/flow/flow_test.go) 的 TestFlowSymbolTargetPreservesExactDeclaration。

- 证据：[context.go:404](../internal/codebase/context.go#L404)、[flow/analyze.go:246–260](../internal/codebase/flow/analyze.go#L246)。
- 触发：同一 Lua 文件的两个不同作用域各有局部函数 `Same`；调用 `source context --symbol-id <精确声明> --flow`。Context 已按 SymbolID 选中正确文件和行。
- 影响：Context 将 Path、Line、Name 一起传给 flow，但 selectTargets 的非空 Symbol 分支只比较名称和文件，忽略 Line；精确声明被重新判成 ambiguous_symbol，随后 target_unresolved，传播分析没有执行。
- 离线复现：冻结 fixture 的 `main.lua:2` 声明正确选中，公开 ContextSource 返回 `coverage.paths=0` 和上述两个边界；同一源码仅按 Path+Line 调用分析则选中并执行，paths=1。没有依赖真实网络或 LuaLS。
- 改进与回归：选择 flow 目标时保留已解析的精确位置；用同文件不同作用域的同名声明回归 `--symbol-id --flow`，同时保留只给名称时应报告的真实歧义。

#### R3-03 · 高 · evidence remove 漏认 Hotfix scan 的已持久化来源引用

- **本轮状态：代码已修复；离线回归和相关 race 检查通过。**
- 修复与正式验证：新 checkpoint 使用既有裸 Capture ID 引用；删除检查同时识别历史 capture:CAP locator。正式回归：[hotfix_scan_test.go](../internal/records/hotfix_scan_test.go) 的 pending/取消、拒绝删除与原 checkpoint resume，以及 [archive_test.go](../internal/evidence/archive_test.go) 的历史 locator 生命周期。
- R7 复审又确认 Pages 依赖与发布间隙残留，沿用本 ID；现已加入不可变 Source/Pages 依赖、历史保护和缺页恢复错误，正式回归通过，当前发布/完整验证状态见文末 R7 条目。

- 证据：[hotfix_scan.go:140](../internal/records/hotfix_scan.go#L140)、[archive.go:323–343](../internal/evidence/archive.go#L323)、[公开删除入口 entry.go:773](../internal/command/entry.go#L773)。
- 触发：Hotfix scan 分页停止并保存 pending checkpoint 后，执行 `evidence remove` 删除该 checkpoint 的原始 Source capture，再按原 snapshot/from/resume 继续。
- 影响：checkpoint 的元数据把来源保存为正式 locator `capture:CAP-…`，删除检查只搜索带引号的裸 `CAP-…`，没有识别这条依赖。删除返回成功，原 resume 随后因来源记录缺失失败。
- 离线复现：三条 XFTH 记录、limit=1、maxPages=1，首次 pending 且 returned=1；删除原 Source 返回 manifestRemoved=true、blobRemoved=true；沿原 checkpoint 继续得到 `vault.record_missing`。
- 改进与回归：让 checkpoint 依赖以删除检查可识别的正式引用表达，并处理现有 capture locator；回归 pending scan 来源删除应拒绝、原 resume 可继续。本例是串行、已持久化引用，区别于 SOL-01 已修复的并发发布/删除竞争。

#### R3-04 · 中 · Windows Skill 的 npm 全局 prefix 查找无法执行

- **本轮状态：代码已修复；Windows 原生回归已由本次 CI 通过；Linux 跳过未计作通过。**
- 修复与正式验证：按 PATH 中 npm.cmd 位置解析 npm-cli.js，以 Node 和分离参数查询全局 prefix，不经 shell。正式回归：[skill-launcher.test.mjs](../tools/skill-launcher.test.mjs) 的自定义 prefix 不在 PATH、参数与退出码测试。

- 证据：[Skill 启动器 lycheedev.mjs:26–33](../skills/lycheedev/scripts/lycheedev.mjs#L26)、[实际回退入口:45](../skills/lycheedev/scripts/lycheedev.mjs#L45)、[现有启动器测试](../tools/skill-launcher.test.mjs)。
- 触发：Node/npm 在 PATH，自定义 npm 全局 prefix 没有加入 PATH，但其中已安装 Lychee Dev；LYCHEEDEV_BIN 未设置。
- 影响：fromPath 无命中后，fromNpmGlobal 以 `shell:false` 直接 spawnSync npm.cmd。在 Windows 返回 EINVAL，被当作未找到 CLI，启动器退出 3，提示重新安装。
- 离线复现：Windows x64、Node v24.18.1 的隔离 PATH/prefix fixture；prefix 查询脚本返回正确目录，目标 launcher 存在并可直接运行、保留参数和退出码 7；当前 Skill 启动器退出 3，底层为 `spawnSync npm.cmd EINVAL`。
- 改进与回归：复用 Node 执行解析出的 npm-cli.js 获取 prefix，保持参数分离；补全自定义 prefix 不在 PATH 的测试。CR-05 的逐目录 PATH 优先级修复仍有效，此处是独立回退分支。

#### R3-05 · 中 · game-root 更新中断后，默认恢复改用另一个 recovery 目录

- **本轮状态：代码已修复；离线回归及 delivery 整包通过。**
- 修复与正式验证：先固定实际 client 再生成去重/recovery 身份；按原目标识别旧 game-root replacement 与 cleanup marker，继续原事务；冲突发行保留原事务。正式回归：[update_addon_test.go](../internal/delivery/update_addon_test.go) 覆盖 root/client 一致性、old-moved 默认恢复、历史恢复目录和 cleanup，另见 [update_test.go](../internal/delivery/update_test.go)。

- 证据：[update.go:139–179](../internal/delivery/update.go#L139)、[库存保存:224](../internal/delivery/update.go#L224)、[恢复/首次安装分支:252–275](../internal/delivery/update.go#L252)、[upgrade.go:214](../internal/delivery/upgrade.go#L214)。
- 触发：原生 `update --release … --installation <game-root>`，根目录下只有一个合法客户端；更新在 old-moved 后中断，再执行默认目标发现的更新。
- 调用链：UpdateManaged 在解析客户端前计算 key，随后将 target.Path 固定为实际 client；recovery digest 仍用旧 key，库存却保存 client。DiscoverUpdateTargets 找到原事务对应的 client 后，再次 UpdateManaged 计算了另一个 recovery 目录。
- 影响：旧目标已移动，新的 recovery 不存在，被当成首次安装；调用报告 complete=true，原 replacement.json、previous、next 仍留下。随后按原 game-root 参数重试，原事务因目标和 next 同时存在而报 `replacement state is ambiguous`。
- 离线复现：复用现有 Release/Client fixture、真实 UpdateManaged，在 previous 落盘后取消；定向 overlay 单测确认 root/client 的 recovery 不同、默认调用完成、原 journal/next 保留及原参数重试冲突，退出 0、约 1.3 秒。
- 改进与回归：先固定实际 client 再计算去重身份和 recovery；发现既有 pending 事务时沿原恢复目录继续。覆盖 root/client 一致性、old-moved 后默认恢复和同客户端重复选择。直接 client 路径已有恢复分支；npm 更新会冻结 plan 返回的 client，不能将问题泛化到全部更新。与 SOL-06 的准备日志缺口、C-07 的发现合同独立。

#### R3-06 · 中 · 对象拾取器在进入战斗后继续显示并截获按键

- **本轮状态：代码已修复；Lua 5.1 六 profile 的真实拾取器回归通过；待实机验证。**
- 修复与正式验证：仅在首次创建 picker dock 时懒注册战斗关闭，主窗口已隐藏也立即停拾取、隐藏 dock 并解除键盘脚本。正式回归：[t_object_picker_combat.lua](../tests/addon/t_object_picker_combat.lua)，含禁用成本。
- 最小实机复验：同版受管 addon 中开始鼠标拾取，确认主窗口隐藏，然后进入战斗。预期 dock 立即隐藏、拾取停止；F/Enter/Escape 不被拾取器吞掉，离战后可重新启用。
- 实机结果：

- 证据：[Object.lua:389–414](../addon/UI/Pages/Object.lua#L389)、[Window.lua:377–389](../addon/UI/Window.lua#L377)、[停止拾取入口 Object.lua:373](../addon/UI/Pages/Object.lua#L373)。
- 触发：在工作台对象页开始鼠标拾取；主窗口按设计隐藏，独立的 UIParent dock 开启键盘处理，随后角色进入战斗。
- 调用链：StartPicker 先关闭主窗口并执行一次页面 shutdown，再开启 picker；战斗回调只在主窗口仍显示时 Hide，主窗口已隐藏便不会再次运行页面 shutdown。
- 影响：dock 与 OnKeyDown 留在战斗中，F/Enter/Escape 首次按下仍设置键盘传播为 false。F 随后因战斗状态停止拾取，但该次按键已被吞掉。此项不声称存在越过受保护 API 的实机操作。
- 离线复现：加载实际 foundation、Window、Object 页，点击真实 mouseButton，再执行 SetInCombat(true) 和 Safety.RunCombatShutdown。战斗后 dock 仍显示、OnKeyDown 仍存在；触发 F 后才隐藏，传播状态为 false。没有运行实机。
- 改进与回归：让已创建的拾取器参与战斗关闭，可在首次创建时懒注册，或让已有入口运行页面关闭职责；真实窗口已隐藏时进入战斗，dock 应立即隐藏并解除键盘脚本。保留从未启用的功能不创建框架与事件的合同。

### 第一轮待核实线索

以下三项在第一轮结束时尚未计入确认问题。第二轮检查正式合同、生产消费者与实际影响，并在各项中更新处理结果；复现某个内部行为不等于已经证明用户可达缺陷。

- **R3-C01 · 本轮复核后仍为候选：SpellMisc 多匹配的单值选择。** [navigate_spell.go:341](../internal/records/navigate_spell.go#L341) 与 [navigate_encounter.go:280](../internal/records/navigate_encounter.go#L280) 按 SpellID 限制为一条，忽略 extra。合法双行 WDC3 离线 fixture 返回 Complete=true、Partial=false、Truncated=false，只显示最低 ID 的一行。第二轮检查发现当前导航合同和 legacy oracle 都保留单个 Misc，仓库没有固定的真实多行/DifficultyID 资料；合成多行不足以证明领域合同违例，本轮再次检查确认 SpellMiscDetail 仍为单值，现有 synthetic golden 无 DifficultyID，仓库没有固定真实 SpellMisc DB2/DBD；需核实固定构建中的变体语义后再定性。
- **R3-C02 · 已确认，转 R4-01。** 第二轮沿完整生产交易链核实了 INPUT 的 100ms 间隔缺失；正式协议不按 exchange 限定，正常连续交易没有补足这项条件。仅计入 R4-01 一次，影响与离线证据见下文。
- **R3-C03 · 本轮复核后仍为候选：foreignOwner 的比较身份。** [Native owner](../internal/live/channel/project_windows.go#L100) 使用项目路径派生 ID，[instances](../internal/live/discover.go#L179) 则传入 home 的 vault WorkspaceID，再由 [windowOwnership](../internal/live/discovery_io.go#L23) 比较。两轮均未发现 ForeignOwner 阻断连接或恢复的生产消费者；公开 instances 也不接受 --project。它是被动诊断展示候选，本轮搜索仍只有 ForeignOwner 声明/赋值；Connect、doctor、activation 使用项目 workspaceID，update 只消费 Installations。需先明确实例列表中的“当前项目”语义。

### 第二轮新增确认问题

#### R4-01 · 中 · 新交易没有保留前次输入后 100ms 的采样条件

- **本轮状态：代码已修复；生产状态机、原日志恢复和 +99/+100ms 离线回归通过；待实机验证。**
- 修复与正式验证：InputAttempt 持久保留同 runtime 输入 watermark；跨 exchange 和发键前复核共同使用，not_sent 不丢失原 watermark；新 runtime 不继承旧时间，同 exchange 未知输入不重放。正式回归：[input_cadence_test.go](../internal/live/channel/input_cadence_test.go)。
- 最小实机复验：后续授权本机任务固定 installation/PID/创建时间/HWND、project/CON，非战斗且 INPUT ready。完成一个无副作用请求，检查 prepare/commit/confirm/release 及 resume 的持久输入记录：后续 sampleMillis 至少为上次 AtMillis+100，原 request/期限不变，无重放；再授权 reload/rebind，预期新 runtime 首次 bind 不继承旧 watermark。
- 实机结果：

- 证据：[正式输入合同](live-protocol.md#L66)、[input.go:117–130](../internal/live/channel/input.go#L117)、[采样判断:183](../internal/live/channel/input.go#L183)、[正常连续交易 driver.go:395–412](../internal/live/channel/driver.go#L395)、[Native 发键前复核 input_windows.go:108](../internal/live/channel/input_windows.go#L108)。
- 触发：同一 runtime 完成一次输入及交易后，进入另一个 nonce 的 prepare、commit 或 confirm；新 INPUT 的身份与 nextSlot 已更新、年龄仍在 500ms 内，但采样时间尚未晚于前次输入至少 100ms。
- 调用链：finishTransaction 清除 Transaction 并保留最近 Input，Run 随即推进下一阶段。submitInput 只在 previous.Exchange 等于当前 exchange 时继承 Outcome.AtMillis；新交易传入 after=0。Native 的发键前复核也使用 after=0，不补这项间隔。
- 影响：协议要求的前次输入后采样间隔在跨交易时没有执行，过早的新样本也能进入输入提交。此项不等同于重放未知输入、目标身份混淆或已观察到的实机业务失败。
- 离线证据：真实 submitInput 与 inputObservation 路径中，前次已提交输入时间为 1000，新 nonce 样本为 1050、当前时间为 1100；ObserveInput 收到 after=0，fixture 的输入方法被调用一次。第二轮核实正常阶段推进没有强制等待；Lua 回调末尾 [即时刷新 INPUT](../addon/Bridge/SlotRuntime.lua#L152)，不能把 pending 路径的等待当作所有输入都满足条件的证明。未向游戏发送输入。
- 改进与回归：在同一 runtime 内独立保留上次输入时间，跨 exchange 也用于采样新鲜度；同 exchange 的已提交/未知输入仍禁止重放。覆盖前次输入+99ms 与 +100ms 的跨交易样本，以及正常新 runtime 和原交易恢复。

#### R4-02 · 中 · 并发覆盖导出 SQL CSV 后，数据与完成记录不属于同一查询

- **本轮状态：代码已修复；跨进程、跨 home 的受控交错回归及 race 检查通过。**
- 修复与正式验证：复用 vault OS lease，在规范化后的外部目标父目录协调 CSV/manifest 整对发布；保留 data-first、manifest-last 和失败 partial 合同，锁文件按 OS lease 合同保留。正式回归：[sql_export_test.go](../internal/records/navigatetest/sql_export_test.go) 暂停 A 于两文件之间，验证 B 阻塞，然后分别成功并校验哈希与 capture 引用。

- 证据：[CSV 完成记录合同 sql_export.go:19](../internal/records/sql_export.go#L19)、[发布顺序与返回:121–134](../internal/records/sql_export.go#L121)、[单文件覆盖 export_destination.go:107](../internal/records/export_destination.go#L107)、[公开调用 entry.go:328](../internal/command/entry.go#L328)。
- 触发：两个不同的 SQL 查询同时通过 `--encoding csv --output <同一路径> --overwrite` 导出。每个数据文件与 manifest 分别发布，没有覆盖这两个文件发布过程的目标互斥。
- 影响：A 发布 CSV1，B 发布 CSV2 和 manifest2 并成功返回，再由 A 发布 manifest1 并成功返回。最终数据为查询 B，manifest 的 SHA256、QueryCapture 和 CSVCapture 仍指向查询 A；两个调用都声称完成，按 manifest 校验当前 CSV 会失败。
- 离线复现：真实 QueryData 分别执行 SELECT 1/2，真实 ExportQueryCSV 共享目标；在 A 完成数据发布后暂停，由 B 完成后再继续 A。两个 err 均为 nil，最终 CSV 为 `value\n2\n`，其哈希与 manifest 中的哈希不同。临时 overlay 单测通过，约 1.4 秒。
- 边界：该项针对显式允许覆盖的并发成功轨迹；不把默认拒绝覆盖、一次失败留下可识别的 partial，或文件被外部程序事后修改列为同一问题。它也不同于 SOL-01 的受管 evidence 归档引用竞争。
- 改进与回归：按规范化后的导出目标，为 CSV 与 manifest 的整个发布过程使用跨进程互斥；不同 home 导出同一外部路径也须协调。回归两份不同查询的受控交错，成功后的最终两个文件必须属于同一次导出。

#### R4-03 · 中 · doctor 首次升级旧版受管插件时，默认归档父目录不存在

- **本轮状态：代码已修复；正式 Windows 磁盘回归已由本次 CI 通过；此缺陷不依赖游戏验证。**
- 修复与正式验证：仅默认归档路径由 doctor 创建其管理的 repairs 父目录，不提前创建最终 archive。正式回归：[live_doctor_windows_test.go](../internal/command/live_doctor_windows_test.go) 的 TestLiveDoctorUpgradeCreatesDefaultArchiveParent，真实安装/升级后在 PID=0 的安全激活边界停止，无进程或输入操作。

- 证据：[live_doctor_windows.go:112–116](../internal/command/live_doctor_windows.go#L112)、[UpgradeInstallation 的路径解析:43](../internal/delivery/upgrade.go#L43)、[installationDestination:120](../internal/delivery/install.go#L120)。
- 触发：新项目首次运行 `live doctor --fix`，目标是干净但版本较旧的受管插件，当前 release 有效，用户没有指定 `--output`。
- 调用链：fixLiveDoctor 将默认 archive 指向项目下 `.lycheedev/live/repairs/<时间戳>`，却没有创建父目录；UpgradeAddon 进入 UpgradeInstallation 后，通过 EvalSymlinks 解析 archive 的父目录，立即返回目录不存在。
- 影响：默认修复流程在升级准备和激活前失败，即使旧安装完整、当前 release 合法也无法继续；要求用户手工创建内部 repairs 目录没有现行合同依据。
- 离线复现：真实 InstallAddon 建立合法 3.1.1 受管安装，检查确认 state=managed；新项目提供合法 3.1.2 release，调用正式 fixLiveDoctor helper，返回 os.ErrNotExist。仅创建默认父目录后，同一旧/新发行 fixture 的 UpgradeAddon 成功。定向 overlay 单测通过，约 0.3 秒，没有调用实际窗口或游戏输入。
- 改进与回归：由选择默认路径的修复入口创建其管理的父目录，再沿原受管升级流程推进；不要提前创建要求不存在的最终 archive 目录。覆盖新项目无 repairs 目录的真实修复分支。与 SOL-10 的原 CON 关闭被版本门禁阻断是独立问题。

#### R4-04 · 低 · 新账户默认 bridge 已开启，status 却显示关闭

- **本轮状态：代码已修复；Lua 5.1 六 profile 的真实启动/status/off/on 回归通过；待实机验证。**
- 修复与正式验证：Persistence 与 SlotRuntime 统一 nil 为默认开启。正式回归：[t_bridge_status.lua](../tests/addon/t_bridge_status.lua)，验证 status 和 INPUT 生命周期一致。
- 最小实机复验：使用隔离的新账号配置启动同版 addon，执行 /dev status，再显式 bridge off/on。预期默认 enabled=true 且有 INPUT；关闭为 false 并停止 INPUT，重新开启恢复，两者始终一致。
- 实机结果：

- 证据：[默认启动 SlotRuntime.lua:93](../addon/Bridge/SlotRuntime.lua#L93)、[状态读取 Persistence.lua:78–80](../addon/Core/Persistence.lua#L78)、[Controls status:36](../addon/Core/Controls.lua#L36)。
- 触发：全新账户没有保存 LycheeToolkitDB，正常 StartRuntime 后查询 `/dev status`。
- 影响：SlotRuntime 将 bridgeEnabled=nil 按默认开启处理并发布 INPUT；Persistence.BridgeEnabled 只将显式 true 视为开启，status 因而返回 enabled=false，诊断展示与实际状态相反。
- 离线复现：实际 Persistence、Startup、SlotRuntime、Controls 路径，确认选项为 nil、Mailbox.input 已存在，但 status.enabled=false；显式 bridge off/on 后状态与 INPUT 生命周期重新一致。Lua 5.1 fixture 退出 0。
- 改进与回归：状态查询与运行时采用相同的默认值解释；覆盖新账户、显式关闭和显式开启。此项仅为默认状态的诊断错误，不代表用户显式禁用后仍工作，也不涉及输入授权错误。

#### R4-05 · 中 · Source inspect 丢弃索引覆盖，把已跳过的文件报告为完整空结果

- **本轮状态：代码已修复；公开 InspectSource 和 capture 覆盖回归通过。**
- 修复与正式验证：传递已有索引 summary 的 Coverage/诊断，并以 summary.Complete 和截断共同决定完整性，保留可回答的有界结果。正式回归：[inspect_test.go](../internal/codebase/inspect_test.go) 覆盖目标跳过、其他文件跳过、真实无匹配及归档；旧 excerpt fixture 改为合法 Lua 注释，避免把语法错误误当完整索引。

- 证据：[inspect.go:56](../internal/codebase/inspect.go#L56)、[匹配范围:65–78](../internal/codebase/inspect.go#L65)、[完成标志:131](../internal/codebase/inspect.go#L131)、[超限文件诊断 index.go:354–366](../internal/codebase/index.go#L354)、[归档 usecases.go:103](../internal/codebase/usecases.go#L103)、[公开入口 entry.go:643](../internal/command/entry.go#L643)。
- 触发：固定 Source 快照中某个实际存在的 Lua 文件超过 32MiB 分析上限，索引明确记录 skipped 与诊断；调用 `source inspect --snapshot <pin> --target-path <该文件>`。
- 调用链：inspectPath 忽略 openIndex 返回的 summary，只扫描 document/asset；已跳过文件只有 diagnostic 记录，因此无匹配。返回前仅按 !Truncated 设置 Complete，未填 Coverage，随后将结果按 complete=true 归档。
- 影响：用户获得完整空集而没有该文件因上限被跳过的说明，capture 也宣称完整；这会把索引未覆盖误当作目标不存在。
- 离线复现：固定 fixture 的 ModelPaths.lua 为 33554433 字节；索引 skippedDocuments=1、diagnostics=1、complete=false。公开 Execute 入口退出 0，results=[]、complete=true、warnings=[]；Coverage 计数全为 0，capture.complete=true。没有 LuaLS 或网络依赖。
- 改进与回归：将已有 summary 的覆盖和诊断传到 inspect 结果，完成标志沿用当前 Source 查询合同；保留仍能回答的有界匹配。覆盖目标本身被跳过、其他文件被跳过及完整索引中的真实无匹配。SOL-07 已修复的是 TOC 验证的缺失判断，本项属于独立的公开 inspect 结果与归档。

#### R4-06 · 中 · SQL 标准输入等待不受整个查询超时控制

- **本轮状态：代码已修复；Linux 公共 Execute、真实继承管道、取消/期限/EOF/复用回归通过；Windows 路径及完整 Windows Go/race 套件也通过本次 CI。**
- 修复与正式验证：同一查询 context 控制 stdin；Unix 使用自有非阻塞描述符并还原共享标志，Windows 在固定线程取消同步/重叠 I/O 并等待取消任务退出，保留调用者 stdin。正式回归：[sql_stdin_test.go](../internal/command/sql_stdin_test.go) 同时覆盖已经写入 SQL 但未 EOF 的生产触发路径与普通文件输入；期限/取消仍返回 5/command.deadline、7/command.cancelled。

- 证据：[整个查询的超时合同 command_contract.go:331](../internal/command/command_contract.go#L331)、[创建期限 entry.go:173–180](../internal/command/entry.go#L173)、[公开调用:310](../internal/command/entry.go#L310)、[阻塞读取 query.go:213–214](../internal/command/query.go#L213)。
- 触发：`data sql --stdin --timeout-seconds 1` 从合法管道读取；生产者已写入完整 SQL，但仍保持写端打开，没有 EOF。
- 影响：Execute 已创建整个查询的期限，readSelectedDataQuery 却直接 io.ReadAll(os.Stdin)，没有接入 context。读操作在期限后继续阻塞，直到 EOF 或读取失败，用户指定的超时不能结束这次命令。
- 离线复现：公开 Execute、合法离线 pin 和 `SELECT ID FROM TestNames`，1 秒期限后到 1.5 秒仍未返回；关闭写端后立即返回退出码 5、command.deadline。定向 overlay 单测通过，约 2.5 秒。有限且正常 EOF 的输入不属于这一失败轨迹。
- 改进与回归：让标准输入读取使用同一查询 context，期限或取消到来时结束读取并清理读取任务；覆盖已经收到 SQL、写端保持开放的期限测试及正常 EOF 成功。取消缺少连接是同一代码边界，未另行宣称执行 Ctrl+C 实测。

#### R4-07 · 中 · 对象检查的流式序列化错误再次格式化失败，逃出点击回调

- **本轮状态：代码已修复；Lua 5.1 六 profile 的真实 Object 点击和流式回归通过；待实机验证。**
- 修复与正式验证：流式错误出口保护 tostring，异常错误对象回退短诊断；返回 false/错误时保留已生成的 text/textStream 和 limited，停止继续建树，让页面使用原有失败分支。正式回归：[t_object_stream_errors.lua](../tests/addon/t_object_stream_errors.lua)，含普通字符串错误、5KB 已生成片段及分块读取。
- 最小实机复验：同版受管 addon、非战斗状态，以隔离对象构造两个表键及嵌套失败 __tostring，再从 Objects 页点击 Inspect。预期点击不抛第二次 Lua 错误、显示失败提示、不创建成功记录；普通对象检查仍工作，已有流片段/limited 仍可读取。
- 实机结果：

- 证据：[Serializer.lua:281–285](../addon/Core/Serializer.lua#L281)、[ObjectInspector.lua:170–179](../addon/Modules/ObjectInspector.lua#L170)、[页面错误分支 Object.lua:451–458](../addon/UI/Pages/Object.lua#L451)、[真实按钮入口:548–553](../addon/UI/Pages/Object.lua#L548)。
- 触发：合法全局路径指向含两个表键的对象；其中一个键的 __tostring 抛出非字符串错误对象，而错误对象自身的 __tostring 也抛错。在 Objects 页面检查该路径。
- 调用链：Inspect 按钮经 BuildInspection 读取 Serializer.CreateStream；coroutine.resume 已捕获序列化错误，但 ReadChunk 的失败分支直接 tostring(segment)，格式化错误对象时再次抛错。
- 影响：Lua 错误逃出实际 OnClick，页面原有失败提示分支没有执行，检查未返回结构化失败结果。
- 离线复现：加载真实 ObjectInspector 与 Object 页面，捕获实际创建的输入框和 Inspect 按钮后点击；外层观察 pcall=false、`error formatting failed`。Lua 5.1 fixture 退出 0，未运行实机。
- 改进与回归：在流式序列化的既有错误出口安全地生成 errorMessage，错误对象无法转成文本时使用稳定的简短诊断；保留已经生成的片段及 limited 状态。覆盖真实页面点击与普通字符串错误。R2-04 已修复的是 Execute/Run 的格式化出口，此处是未经过 Execute 的独立 Object 流式入口。

### 第二轮候选与排除

- **R4-C01 · 已确认：doctor 默认归档与目标安装位于不同卷。** 原生验证于 R7 CI 完成，修复与实际轨迹见文末 R4-C01；以下保留最初候选证据。 [默认 archive](../internal/command/live_doctor_windows.go#L114) 取项目目录，游戏可以位于另一卷；当前升级会经过 [outsideDiscovery 的 filepath.Rel](../internal/delivery/upgrade.go#L225) 与 [MoveFileEx 目录发布](../internal/delivery/publish_windows.go#L14)。本轮复核合同只要求 archive 位于 discovery 外；Linux 环境未执行两个 Windows 卷上的合法部署 fixture，尚未核实完整失败轨迹与支持合同，保留候选、不添加 unsupported 限制，也不与 R4-03 的父目录缺失合并。
- **创建 claim 前的持久化窗口未计入问题。** CON/activation 文件落盘后、owner claim 发布前若创建失败，可能留下文件；当前没有证明正式创建合同要求恢复尚未成功返回的 claim，也没有实际复现可见业务后果，不据此添加恢复框架或清理限制。
- 未将可选外部 oracle/实机测试未执行、已有 legacy 单值转换约定、无当前调用的库方法、明确允许的 partial 输出或理论锁竞争当作新增 bug。旧 42 项修复状态保持不变。

### 追加审计的验证与交付边界

新增问题通过定向 Go overlay、公开 CLI fixture、Node 启动器 fixture 和实际 Lua 页面/启动路径核实；这些通过结果证明缺陷能够复现，不代表已修复。R4-01 复用了第一轮证据，再沿完整交易链确认协议门槛缺失。新报告没有把离线 fixture 当作实机证明。

两轮覆盖当前模块、公开入口、调用关系和对应测试合同；未重复运行既有完整 Go/Lua、Node 或七产品实机矩阵。产品源码、永久测试、发行版本、npm 标签、已有工作区修复与 Rust worktree 均未因本次审计改变。新增问题的修复、受影响回归及必要实机验收属于后续实施工作。

追加审计阶段仅更新此报告，本地引用与行号范围检查通过，工作区 git diff --check 通过。随后按用户授权创建交接文档、补充 AGENTS.md 约定，并将现有基线提交到交接分支；没有合并或发布。

六个追加审计临时目录已在代理结束后清理，删除范围仅为本任务的离线合成 fixture；有用结果保留在各条目中。当前用户项目、恢复记录与保留的 Rust worktree 没有清理。


### 本轮 R3/R4 修复交付

- 13 项确认问题的代码修复和正式回归已加入；旧 42 项维持基线，3 项候选保持候选，不因合成行为或平台缺失扩大定性。
- 额外修复既有 Windows CI 打包 smoke 的失效输入：不再读取已退役的 `addon/Bridge/Definitions.lua`，改为复用当前 TOC/Bindings 资源检查；正式 Node 回归覆盖 Native Mailbox load graph。这是交付验证修复，不计入 13 项。
- 修复提交 `81ccdd5b22ddb42991b60ecba12415a4a3b24f15` 的 [Windows CI](https://github.com/Follen/Lychee-Dev/actions/runs/38528396047) 全部通过：windows-contract（Lua 必需完整 Go 套件）、windows-process、windows-addon、windows-package、race、ci-required。本地结果及可选环境限制见 [handover.md](../handover.md)。历史主批次/审计结果不代替本轮验证，交叉编译和跳过不计作原生运行通过。
- R3-01、R3-06、R4-01、R4-04、R4-07 保留“待实机验证”；上方各条目已列前置条件、最小步骤、预期，实机结果留空。本轮没有连接游戏、发送按键、部署游戏目录或操作用户电脑。
- 只提交推送同一草稿 PR #13；不合并、不发布、不更改发行版本或 npm 标签，仍为 3.1.2 / next / latest=2.5.1。


## 后续循环审计：R5 起

按追加任务继续对架构、实际 bug、死代码、Skill 编排以及有明确收益且可验证的架构优化开展至少三轮独立复审，每轮使用不同审计者；每轮确认可离线处理的问题后实施、验证并推送同一草稿 PR，再交下一轮复审。至少三轮完成后，只有独立复审不再发现可离线修复项才结束；候选和仅需实机的项分别记录，不声称所有未知问题清零。

| 轮次 | 基线 | 状态 | 当前结果 |
| --- | --- | --- | --- |
| R5 | `81ccdd5` 产品代码；文档 head `f7f2239` | 修复已推送 `2660a4ef`，Windows CI 六作业全部通过 | 5 项新确认（2 bug、1 架构优化、1 Skill 合同、1 死代码）；R4-01、D-06 残留沿用原 ID |
| R6 | `2660a4ef` | 4 项修复及额外测试修正已推送 `9a02522f`，Windows CI 六作业全部通过 | 2 个 bug、1 个可计量架构优化、1 个死代码清理 |
| R7 | `9a02522f` | 已推送 `f84cf0f4`，原生 CI 确认 R4-C01 并失败；修复进下一批 | R4-C01 仅新增安全原生双卷 fixture，未执行前仍为候选 |

新发现的仅实机问题只记录前置、最小复验步骤、预期与空白实机结果，不操作游戏、不作无离线证据的修复；已有 R3/R4 实机待办保持上方状态。

### R5 复核：R4-01 缺失 outcome 的 crash 恢复残留

- 状态：代码已修复，正式 channel/journal 回归通过；沿用 R4-01，不重复计入新缺陷。
- 在真实 Connect/Load/原 receipt/PrepareRequest/Run 路径中，前次输入 intent 的样本为 900ms，实际输入为 1000ms，effect 后 outcome 尚未保存即 crash。原 receipt 恢复不重放，但下一 prepare 仍以旧样本 900 为 after，接受 1050ms 样本，仅晚于实际输入 50ms。
- 旧代码 fixture 观察到 inputs=2、after=[0,900,1100]；没有游戏输入。修复以当前经生产 decoder 核验的 INPUT 宿主 uptime 建立一次持久 `RecoveryAfterMillis` 上界，等到上界 +100ms；不把旧 pre-input 样本冒充输入时间。
- 正式 `input_cadence_test.go` 覆盖 bind/prepare/commit effect 后 crash、旧 journal、多次 Load、实际输入 +50ms 与恢复上界 +99/+100ms、not_sent 与新 runtime；原请求/CON/ticket/期限不变，原五个输入 nonce 各执行一次。channel/journal 全包及定向回归通过，Windows 交叉编译只计编译。
- 实机状态继续随 R4-01 留为待实机验证，实际结果：


### R5-01 · 中 · 有限浮点报告在 Lua 编码时静默丢失精度

- 状态：代码已修复，生产 Go↔Lua 报告回归通过；离线修复不依赖游戏环境。
- 触发：`CaptureWriter` 的非安全整数分支使用 Lua 5.1 `tostring`，只保留有限有效位；例如 `100000000000000.125` 变为 `1e+14`，`9007199254740992` 变为 `9007199254741000`。完整 Driver Connect → PrepareRequest → Run → Load 路径将错误数值当作有效完整报告持久化。
- 修复：仅将有限数值 fallback 改为 `string.format("%.17g", value)`，保留安全整数文本、secret/nonfinite 拒绝和既有大小预算；不改协议形状。
- 正式回归：`tests/protocol/numeric_report_test.go` 使用生产 Lua writer/slot 协议与 Go Driver，逐位比较正负数、边界值、次正规数、最大有限值和负零；同时核验 journal 恢复、完成后不重放与解绑。`tests/protocol/writer.lua` 检查精确预算和错误出口。30 个数值逐位相等；旧 writer 的同一路径在 16 个值上失败，证明回归覆盖原缺陷。
- 客户端整合待实机验证：使用匹配 CLI/addon、固定 PID/原 CON，运行只返回上述数值和 `%.17g` 对照字符串的无副作用探针，预期报告数值与文本一致、完成清理并断开。实机结果：

### R5-02 · 中 · 并发视频导出均成功却混配帧与 manifest

- 状态：代码已修复，records/evidence/vault 正式回归及定向 race ×3 通过；不依赖游戏环境。
- 触发：两个不同 home 对同一个输出目录执行 `data asset demux --overwrite`。A 写完帧后暂停，B 写完帧和 manifest 并成功，A 再写 manifest 并成功；最终 manifest 的 SHA 对应 A，而实际帧来自 B，两个调用均返回 complete。
- 修复：对规范化后的输出目录复用 vault OS lease，覆盖最终冲突复核、全部帧发布和最后 manifest 发布；同目录别名和不同 home 共用锁。锁文件留在目录外的父级，不混入导出产物。
- 相关 SQL CSV 使用同一小型 destination lease 辅助入口，保留原 `sql-csv` 锁身份与合同。不扩大失败/崩溃中途部分发布的合同。
- 正式回归覆盖跨进程/不同 home、路径别名、等待取消、no-overwrite、进程退出释放与最终帧/manifest 字节一致性。

### R5-03 · 低 · 视频帧重复发布 blob，产生额外完整写入与同步

- 状态：代码已优化，正式 staging 计量、capture 一致性与定向 race 回归通过；收益可离线计量。
- 证据：每帧先 `PublishBlob`，随后 `CommitCapture` 又发布相同字节；双帧真实 Demux 路径观察到 6 次发布、2616 staging bytes，所需仅源文件、两帧和 manifest 共 4 次、2436 bytes。重复路径还产生额外 hash、同步及校验。
- 修复：保留单一 `CommitCapture` 发布入口，以返回的已核验 `capture.Blob` 派生 manifest 的 Blob/SHA256。保留捕获 ID、来源、帧预算和字节完整性，不增设缓存或新持久层。
- 正式 `TestDemuxAssetStagesEachCaptureBlobOnce` 在现有 context 边界观察 staging 文件，不增加生产计数器：双帧只生成 4 个 blob stage，字节数精确等于 source+frames+manifest；旧代码负对照为 6 个 stage，多写 180 字节。另核验帧 capture/blob/manifest 与实际输出一致，不依赖临时 overlay。

### R5-04 · 中 · Skill 承诺已退役取消键及无 INPUT 的自动 reload

- 状态：说明已修正；Skill 84 命令/220 引用/0 违规及生成目录检查通过，已有 input-protection 六 profile 和 addon 全包通过。
- 证据：`skills/lycheedev/references/live-probes.md` 仍说 Ctrl+Alt+[ 释放输入保护，并称 addon 未加载也必须 fixed reload。当前 Native 仅 wake/reload 两键；正式 `t_input_protection.lua` 明确验证 ALT-CTRL-[ 不会释放保护，协议第 4 节要求无兼容 INPUT 时手动 reload。
- 修复：说明明确 `probe:ReleaseInput()`、完成/错误清理、deadline、战斗、离开世界和 runtime cancellation 的释放路径；没有额外取消绑定。自动 reload 仍需新鲜兼容 INPUT/实际绑定，无兼容运行时按 live-startup 请求手动 reload。未恢复旧快捷键或盲发降级。
- “addon 未加载 fixed reload”也是前文文档清理未落地残留，合并处理，不再次计算为基线修复。

### R5-05 · 低 · Stores 导出层无生产调用的独立辅助函数

- 状态：代码已清理，含 Stores/records UI/Run/locale 的 addon 全包回归通过。
- 证据：`Exports.Protect` 全仓无调用；`Exports.CanFit` 仅有两个自检，没有生产、文档或 Skill 消费者。当前唯一生产 `Exports.Add` 调用来自 UI/Export，不使用这两项。
- 修复：只删除这两个孤立 helper 及两条 CanFit 自检（46 行实现、4 行测试）。保留 pending/saved UI、既有 protect 数据路径、future-schema 写保护和真实预算/持久化测试；没有按“测试调用少”删整个模块。

### R5 复核：D-06 注释锚点自检残留

- 状态：残留已清理，addon 全包回归通过；沿用 D-06，不新增计数。
- 前文声称删除的 `TestLocaleFilesAnchorMarker` 实际仍存在。本次删除仅用于后续代理插入注释的断言；保留 `t_locale.lua` 的双语 key 对齐、回退和格式化行为回归。


### R5 验证与审计覆盖

- 定向修复验证：build/vet、channel/journal、protocol、addon、records 子树/evidence/vault 均通过；demux 和 SQL export 并发 race 各重复三次通过。数值/并发导出/blob staging 的旧代码负对照均实际失败，正式测试已覆盖触发路径。
- 受影响 Node 18 项：15 通过、3 个 Windows launcher 专属跳过，未计为通过。版本、Skill（84 命令/220 引用/0 违规）及生成命令目录通过。整合 build/vet 通过；Lua 必需完整 Go 套件实际执行为 30 包通过、command 1 包失败、4 包无测试，2420 test/subtest pass、3 fail、28 skip。失败仅为同样的基线 Linux Native admission 两项和 Source prune/LuaLS broker 一项，无新增失败；未称 Linux 全套通过。新 head `2660a4ef` 的 [Windows CI](https://github.com/Follen/Lychee-Dev/actions/runs/38532230482) 六作业全部 success：Lua 必需完整指定 Go 套件（31 测试包）、race、process、addon、真实 tgz/隔离安装和 aggregate；未借用旧 head 结果。
- 独立审计还抽查了 CLI 准入/JSONL/取消、Source 查询/分页/batch/恢复、SQL/DB2/Hotfix/capture、Native 回执/期限、addon 工作台、受管安装/npm 事务及 Skill 跨文档合同。没有把薄适配器、大文件或合法 partial 失败当作重写依据；未发现其他已确认的新项，不构成全状态零缺陷证明。
- R3-C01、R3-C03、R4-C01 未取得新增领域/消费者/双 Windows 卷证据，保持候选；本轮没有新增只能靠游戏才能确认的缺陷，R5-01 客户端整合和既有实机项结果仍留空。


### R6-01 · 中 · Git-backed Source 正文搜索漏掉有效中文文件名

- 状态：代码已修复，正式 codebase 全包及定向 race ×3 通过；不需要游戏环境。
- 触发与证据：真实 IndexSource → ReadSpan → Browser.Search 路径中，`测试.lua` 已索引且 ReadSpan 返回匹配注释，但 precise/exploratory 两种正文搜索均返回空结果，`Complete=true`、`Truncated=false`、无错误。Git 默认把非 ASCII 文件名输出为带引号的 octal escape，当前按冒号切割后被 sourcePath/topic 过滤掉。
- 根因：`internal/codebase/body_search.go` 使用带文本引号的 `git grep -n` 输出，未使用无歧义文件名分隔。不是 Lua 解析失败或未索引文件。
- 修复：使用 Git `-n -z` 的 NUL filename/line 分隔，校验 exact commit 前缀；保留 fixed commit、索引/topic/路径过滤、原输出预算与截断合同，没有放宽路径规则。
- 正式 `body_search_test.go` 覆盖生产 Git-backed 两种搜索模式、中文/引号/tab 文件名、colon/NUL snippet、二进制排除、no-match 和 bounds/truncation；新测试在旧实现仅返回普通 ASCII 路径时失败。


### R6-02 · 低 · Search 摘要反复整文件切分，绕过已有行索引缓存

- 状态：有测量证据的架构优化已实施，正式 codebase 全包及定向 race ×3 通过；不需要游戏环境。
- 真实 Browser.Search 对 1,165,200 字节合法 Lua 文件中的 200 个同名声明生成结果时，分配 302,403,328 字节；其中 200 次 `numberedLines` 整文件转字符串/CRLF 替换/Split 占 299,990,824 字节。当前查询已有带预算的 `documentLine`/`lineMemo`，摘要却没有复用。
- 离线原型复用现有行索引，200 个摘要逐字节相同、仅建立一次索引，分配 446,168 字节。原测试约 981ms、摘要旧路径约 820ms、原型约 31ms 仅为本 fixture 的指示性时间，非真实用户性能承诺。
- 修复：Search/Inspect 摘要与已有 documentLine 共用同一个有预算的行索引，不新增缓存层；保留原 8MiB/256 文件边界、CRLF/CR-CR-LF/末尾 CR 与空行语义、行数上限、hash、分页与覆盖合同。
- 正式 `document_lines_test.go` 的 200 结果公开 Search 路径：旧代码分配 302,369,464 字节，修复后 2,810,488 字节，只建立一次行索引，摘要/hash 相同；旧实现负对照失败。这里是受控 fixture 分配量，不宣称真实用户耗时同比下降。


### R6-03 · 中 · Hotfix CSV 独立发布路径缺少目标级互斥

- 状态：代码已修复，公开 CLI 跨进程回归、records/evidence/vault 12 包和 Hotfix/SQL 定向 race ×3 通过；最终整合已执行。不需要游戏环境。
- 真实触发：八个不同 home 的公开 `Execute(data hotfix --source raidbots ... --encoding csv --output 同路径)` 并发，均退出 0；第 47 次复现得到 manifest SHA `73edf...` 而 CSV SHA `3e87...`，均为 99 字节。原 helper 直接 `os.WriteFile` CSV，再 `O_TRUNC` sidecar，两者无共同 lease。与 SQL R4-02、demux R5-02 是不同实际 exporter，不重复计数。
- 修复：CLI 只调用 `records.ExportHotfixCSV(ctx, ...)`；复用 records 的规范化 destination、已有 OS lease 与逐文件完整 stage 发布。Hotfix/SQL 共用原 `sql-csv` 锁身份，不能按 provider 分成两个互不协调的锁。共同边界先复核两个目标，再发布 CSV 和最后 manifest；保留 Hotfix 默认覆盖、原 manifest schema、warnings/filter/cursor 与输出语义，不引入整组回滚框架。
- 正式 `hotfix_csv_export_test.go` 通过公开 Execute，在不同 home/子进程/规范化与 symlink 别名上暂停 CSV→manifest 窗口，断言另一个调用等待并响应原 deadline、未改变文件，之后两次成功且最终 bytes/hash/返回 manifest 一致；Windows case 别名留给新 CI。旧 command 实现负对照失败。
- 正式 `hotfix_export_test.go` 证明 Hotfix 等待既有 SQL lease、取消不写、隐式覆盖仍成立、编码错误/manifest 预检不替换旧 CSV，输出字节与原 encoder 完全一致。

### R6-04 · 低 · 三个没有当前消费者的内部 Lua 包装函数

- 状态：代码已清理，addon 全包 34 suite × 六 profile = 204 组通过，无 Lua 跳过。
- 全仓生产、测试、文档和 Skill 无调用：`diagnostics.FormatError` 只是转发 FormatAgentReport；`inspector.GetMouseFocusLabel` 是未使用的捕获包装；`page.OpenSelectedNodeDetail` 无调用且实际 tree context 直接绑定本地 OpenNodePopup。
- 仅删除这三个独立包装（约 19 行实现）；保留实际 FormatAgentReport、Compat.GetMouseFocus/CaptureMouseFocus、OpenNodePopup，以及 SearchPath、Stores 状态和 legacy report 消费者，不按模块名整体删除。

### R6 实际覆盖与验证

- 审计抽查 Source search/context/batch/line index，Data export/Hotfix/DB2，Native INPUT/report/recovery、probe 生命周期，受管替换/npm 更新及 Skill source/data/live/recovery 合同。只有上述 4 项得到确认；没有新增只能依靠游戏确认的缺陷。R3-C01/R3-C03/R4-C01 未取得新证据，仍为候选。
- Source 新旧代码负对照、codebase 全包（136 test/subtest pass；2 个真实 LuaLS/broker 可选集成缺环境 skip）及定向 race ×3 通过；Windows 未在本地冒称执行。Hotfix 真实并发已复现，正式跨进程测试、records/evidence/vault 12 包及 Hotfix/SQL race ×3 通过；addon 全包通过。代码批次 build/vet 通过，首次完整 Lua 必需 Go 整合已执行：29 包通过、2 包失败、4 包无测试；2438 test/subtest pass、4 fail、28 skip。除既有3个Linux平台失败，当时出现容量重载请求/期限保持测试失败（`TestAutomaticCapacityReloadContinuesOriginalRequest`）；后续已确认并修正单测计时条件，未发现对应生产缺陷，诊断负对照和最终完整重跑结果见下节。
- Skill 合同 84 命令/220 引用/0 违规及生成命令检查通过，本批没有改 Skill/命令目录或发行版本输入。本次提交包含 R6 全部修复及额外测试修正；R5 `2660a4ef` 绿色 CI 不能替代本批的新 head 验证，[`9a02522f` 的 Windows CI](https://github.com/Follen/Lychee-Dev/actions/runs/38536137573) 六作业全部通过：contract（Lua 必需完整 Go）、process、addon、race、package、ci-required。


### R6 额外整合测试修正：容量 reload 的计时完成条件

- 首次完整套件的额外失败不是已证实的生产缺陷：测试把 350ms 调用超时当成自动 reload 已完成，磁盘/调度稍慢便停在正常 bind/recovery 中间。诊断版 30 次有 5 次失败，精确 HEAD 依赖对照的原测试 30 次有 3 次失败；两者都保留原 request ID 和持久 deadline。
- 400ms runtime-observation 延迟负对照明确 `context deadline exceeded`、sameID/sameDeadline=true。只修改 `stop_reconcile_test.go`：达到新 runtime 的 prepare Publish 边界后取消，5 秒只作死锁兜底且到期必须失败。检查内存及 Load 后原 ID/request/两个期限、durable reload complete、精确 prepare nonce/runtime 和单次 bind 输入；不改生产时限、cadence、重放规则或共用 pending fixture。
- 修复单测 30/30、同样 400ms 延迟对照、channel/journal 全包、cadence/自动 reload/不重放 race ×3、build/vet 通过。修正后完整 Lua 必需套件已重新执行：30测试包通过、command 1包仅3个既有Linux平台失败、4包无测试；2439 test/subtest pass、3 fail、28 skip，无新增失败。保留首轮失败和负对照事实，未改生产行为或以重跑代替诊断。

### 特定环境与证据缺口的后续复验

以下不是新增已确认生产 bug。候选和可选覆盖缺口分别标注；本任务不要求用户现在执行，也未操作游戏/真实安装。未执行结果留空；已有六项待实机步骤见 R3/R4 对应条目、R5-01 及 handover。

| 项目 / 当前状态 | 必需环境或材料 | 最小步骤 | 预期 / 判定标准 | 实际结果 |
| --- | --- | --- | --- | --- |
| R4-C01 已由原生证据确认为宿主缺陷 | Windows 两个真实可写卷；隔离 old/new release/client/project；无需游戏 | 同卷对照与跨卷默认 archive，PID=0 | 应完成磁盘升级并仅在安全激活边界返回 explicit_target_required；修复后待复验 | R7 CI 同卷通过；D/C 不同 GUID 跨卷在 Rel 失败，旧树完整、archive 未发布 |
| R3-C01 候选，多 Misc 领域语义未确认 | 固定真实构建 SpellMisc DB2、匹配 DBD 与 DifficultyID 说明及多匹配 SpellID | 比较原表全部匹配、导航单值与领域定义，保存固定 hash/build 与差异 | 只有真实变体语义与现有单值合同冲突才升级为缺陷；合成双行不够 | |
| R3-C03 候选，foreignOwner 展示语义未确认 | 明确 instances 的项目归属合同；必要时 Windows 隔离窗口/owner fixture 与两个 project | 先确认“当前项目”基准；对照 instances 展示与 Connect/doctor/activation 的实际 owner 判断 | 被动字段差异不能当作连接阻断；有约定消费者且展示/行为违约才升级 | |
| 真实 LuaLS/broker/打包 Source 语义，可选覆盖缺口 | Windows；固定 LuaLS 3.19.1 可运行目录、匹配 release/CLI、固定生成定义与 Retail Source | 配置正式测试所列 runtime/release/docs 输入，运行 codebase/luals/command 的 real integration 测试 | LSP 引用/定义、关闭交接、取消、workspace 隔离和已安装 Source 语义符合对应断言；archive SHA 通过不能替代进程运行 | |
| API 文档与安装数据/图像/CASC oracle，可选覆盖缺口 | 固定文档、显式安装产品/build、本地 CASC index、icon.blp/png/webp、校验过的 CascLib oracle 与固定 DB2 definitions | 按对应测试环境变量选定固定资料，分别运行 environment/flow/records/archive/navigatetest/schema 测试 | 固定身份/原始字节/解码/导出与各 oracle 断言一致；不把缺样本 skip 当作通过或已确认 bug | |
| 原 runtime dump/header replay，可选覆盖缺口 | 已有、获准使用的原始 dump manifest/header/匹配 executable 及 hash；不采集新游戏数据 | 配置正式 memory replay 测试的固定输入，验证 root recipe 和捕获 header 对照 | 根定位证据及身份与固定原始数据吻合；合成 memory fixture 不代替原始 replay | |
| Live read-only 可选覆盖缺口 | 后续明确授权的 Windows/WoW 固定 PID/创建时间/HWND 和对应受管版本 | 只在后续本机任务运行显式只读目标测试，核实身份和可读窗口；不借此扩大输入许可 | 原身份/只读窗口信息正确；本次未执行，不要求现在启动游戏 | |


### R7 独立复审与正在实施项

基线为 `9a02522f`。另一位新审计者覆盖 Source 搜索/上下文/缓存，Data 查询/导出/Hotfix，Live 恢复与输入，addon 生命周期及 Skill 编排。确认下列两项新事项和 R3-03 残留；没有新增仅游戏可确认的缺陷。R7 仍发现可离线修复项，因此修复验证后必须继续新的独立 R8，不以达到三轮为停止条件。

#### R7-01 · 高 · Hotfix CSV 输出可破坏当前 workspace 身份

- 状态：代码已修复，公开 CLI 定向与 race ×3 回归通过；本批完整离线整合完成，当前 head Windows 验证待执行。
- 公开 `data hotfix --source raidbots --encoding csv --output <home>/workspace.json` 在合法三条记录 fixture 上退出 0、complete=true，141 字节 CSV 替换 workspace 身份；随后 `vault.OpenStore` 返回 `vault.unsupported_workspace`。这不是外部程序事后修改或 R6 并发问题。
- 修复：command 将已有解析 root 传给 records exporter；抽取 SQL 现有的规范化父路径 workspace 排除检查为同一小型边界，CSV 和 manifest 发布前复用。保留外部隐式覆盖、schema、编码字节和跨 home destination lease，不新增策略框架。
- 正式 `TestDataHotfixCSVRejectsManagedWorkspaceDestinations` 通过公开 Execute 覆盖 canonical/normalized/父目录 symlink，核对拒绝码、原 marker 字节、无 sidecar、原身份仍可用，并保留外部覆盖正常成功。

#### R3-03 残留 · 高 · checkpoint 的页面 capture 未受依赖保护

- 状态：代码已修复，records/evidence/vault 12 包与依赖/scan race ×3 通过，完整离线整合完成；沿用原 ID，不重复计数。
- 三条记录以 limit=1/maxPages=1 生成 pending scan；删除 Pages[0] 时 manifest/blob 都被移除。随后原 checkpoint resume 返回 complete=true/Returned=3，但首个页面 FetchCapture 为 `vault.record_missing`。既有 Source locator 修复有效，缺口在 Pages 只存于 blob，没有 metadata 依赖。
- 修复：可选 `CaptureRef.dependencies` 纳入不可变 hash，空列表省略以保持旧 manifest 身份；Source+Pages 在与删除相同的 metadata write lease 下验证并发布。旧 hotfix-scan 仅通过已核验、4MiB 有界正文识别 Pages，不扫描任意 JSON 引用；移除全部持有 checkpoint 后依赖自然释放，不改变用户 keep 语义。
- 正式回归及旧代码失败负对照覆盖 pending/complete、历史 checkpoint、已损坏历史恢复、Source/页面产生至 checkpoint 发布之间删除，以及 metadata 提交同租约竞争。错误保留原 checkpoint ID/原字节，不返回成功或新悬空 checkpoint。legacy manifest 身份另以旧结构独立编码回归。

#### R7-02 · 低 · 两处零消费者 UI 接口及不可达 callback

- 状态：代码已最小清理，addon 全包通过（34 suite × 六 profile = 204 组，无 Lua 跳过）。
- 全仓生产/测试/文档/Skill 没有 `TreeView.SetOnSelectionChanged` 与 `Events.FocusInput` 消费者；前者是 selectionChanged 唯一赋值入口，两个 callback 分支不可达。
- 仅删除两个方法、私有 callback 字段及分支。保留 selectedNode、高亮、测试使用的 GetSelectedNode、Object 实际使用的 SetOnNodeContext 和既有焦点事件。

R4-C01 仍为候选：update 的跨卷 target-volume recovery 明确支持游戏 D:/workspace C:，补充了合同证据。将利用已有 Windows CI 的安全磁盘 fixture，验证真实不同 volume GUID、同卷对照和跨卷默认 archive；PID=0 阻止 process discovery/input。未原生运行前不升级为已确认、不填通过。R3-C01/R3-C03 无新领域/消费者证据，继续保留。


### 冗余测试专项与覆盖率顺序

用户追加要求先审计并删除真正冗余测试，再计量覆盖率。独立专项已完成；不同入口、故障注入、跨进程、平台或 profile 合同没有仅因表面相似被删除。既有 D-06 注释锚点测试不重复计入本轮；新增清理见 R7-03。R8固定提交7566493c已启动原生计量，将分别报告 Go 语句覆盖与业务场景覆盖，记录平台、命令、范围、跳过和实机空白，不估算百分比。


### R7 批次实际验证

- build/vet 通过；records/evidence/vault 12 包、依赖/Hotfix scan race ×3、公开 Hotfix CLI race ×3 通过；addon 204 profile/suite 组通过。R7-01 的正式公开 CLI 测试在旧实现 canonical/normalized/parent-symlink 三项均失败，修复后三项及外部覆盖通过。R3-03 负对照与新回归见上述条目。
- Lua 5.1 必需完整命令 `go test -p=2 -parallel=4 -count=1 -timeout=20m ./...` 已执行并留 JSON 结果：30 测试包通过、command 1 包仅原有 3 个 Linux 平台失败、4 包无测试；2456 test/subtest pass、3 fail、28 skip，无新增离线失败。未把 Linux 全套写为通过，缺真实/可选输入的跳过不计成功。
- Windows doctor 双卷 fixture 已通过 Windows amd64 交叉编译与目标 vet，只计编译检查。CI 新增独立 verbose 磁盘测试步骤，即使完整套件失败仍保留实际卷身份/失败阶段/skip 证据；没有改游戏或生产行为。当前批原生验证未执行。
- 受影响 workflow Node 结构检查 8/8、import 合同、文档本地引用与 diff 检查通过。发行版本、命令目录和 Skill 输入没有改变，未重复无变化检查。


### R7-03 · 低 · 两处重复测试输入没有独立覆盖价值

- 状态：独立专项确认后已最小删除；build/vet 和受影响正式回归通过，已随7566493c提交。删除的是一个 Go table case 和一个 Lua 数组元素，不是两个测试函数或业务场景。
- `internal/delivery/release_source_test.go` 中 `commit mismatch`（合法 40 位零 SHA）与保留的 `commit not release`（合法 40 位 c SHA）从相同 fixture 调用同一函数，只断言 `ErrRelease`；都仅因 `source.Commit != releaseCommit` 失败，生产没有全零特殊语义。删除前者，保留 nil/正确身份/大小写/路径/digest 等边界。
- `tests/addon/t_locale.lua` 的 `legacyKeys` 把 `SAVE_TO_DISK` 列两次。相同 loop 对同一未改变的中英文表作相同非空断言；仅删除第二个元素，保留 suite、全部 key/placeholder/fallback 断言及六 profile。
- 范围与保留理由：对 334 Go 文件中识别的 1,238 个普通顶层 Test 作 token 候选筛查，没有完全相同函数体组；人工核对 Source/Data/Live/export 高相似断言、Lua 生命周期/协议/locale、12 个 Node test 文件及 npm launcher/install-smoke。fixture 字节 SHA 筛查没有相同文件组。词法初筛不是完整 AST/动态子测试等价证明，也不声称未知测试全部最小化。
- 明确保留：Source 的精确返回/固定引用与行索引成本；SQL 不同作用域/绑定/执行边界；Hotfix CLI、records 分页和 archive 持有的不同层；Live not_sent、focus blocked、已知时间与 crash 恢复上界；导出预检与 TOCTOU、跨 home/跨进程及 SQL/Hotfix 锁互通；Lua 输入采样/绑定/运行时不同层；原生 Windows、race、真实安装与打包。它们有不同触发或断言，不能因名称相似删除。
- 清理后完整整合已重新执行：30 测试包通过、command 1 包仍仅三个既有 Linux 平台失败、4 包无测试；2456 test/subtest pass、3 fail、28 skip，无新增失败。工作区代码指纹为 `f794297e2918bd99aff80d8b4aa5372c14cc428dafc551eb1335f5266303e70d`，尚未形成新提交，不冒称 clean commit。
- 正式验证：Go 1.27.1/Linux、Lua 5.1.5 必需模式，`go build ./...`、`go vet ./...` 通过；`go test -p=2 -parallel=4 -count=1 -timeout=5m ./internal/delivery -run '^TestValidateSourceArchive'` 三个顶层测试通过；`go test -p=2 -parallel=4 -count=1 -timeout=5m ./tests/addon -run '^TestWorkbenchLuaSuites$/^t_locale[.]lua$'` 六 profile 通过，无 skip。未冒称新的完整或 Windows 套件。

### 业务场景与计量口径

| 业务 | 正式测试覆盖的核心合同 | 当前限制 |
| --- | --- | --- |
| Source | 固定 commit/声明/字节，精确与探索搜索，Context/flow、分页、摘要与有界缓存 | 真实 LuaLS/broker/安装包语义需要对应固定运行输入 |
| Data | DB2/schema/SQL 绑定和作用域、Hotfix 固定源分页、取消和原 checkpoint 恢复、capture 依赖 | 固定真实 DB2/CASC/图像领域 oracle 材料仍有可选缺口 |
| Live 恢复 | 原 project/CON/request/期限、输入 intent/outcome、不重放、100ms cadence 与 crash 上界、owner 排他、报告与清理分离 | 当前六项客户端整合待实机，结果仍空；合成协议不证明游戏行为 |
| 导出 | CSV/manifest/frame/capture 字节一致、预检与发布 TOCTOU、跨 home/进程锁、目标身份保护 | 仅声明已运行平台/fixture，不承诺外部程序事后修改或任意崩溃全原子 |
| 交付/Skill | 固定发行身份、来源/归档白名单、npm/Skill launcher、真实 tgz/隔离安装、升级恢复和当前命令合同 | R4-C01 已确认并修复，双真实卷原生正向 fixture 正在 R8 CI |

表中有测试不等于业务场景已经完整覆盖；不存在可信的“业务覆盖率百分比”分母。Go 语句比例也不会代表 Lua/Node 覆盖或实机验收。最终计量将在冗余清理后的固定基线进行，保留精确命令/平台/代码身份和 skip/fail 说明。

R7 修复已推送 `f84cf0f4`，[该 Windows CI](https://github.com/Follen/Lychee-Dev/actions/runs/38540889167) 已完成：process/addon/package 通过；contract/race/汇总因 R4-C01 双卷探针失败，无 race detector 报告。上述两处测试清理已随后续7566493c提交。R8 独立复审基线为该相同代码树加两处清理；又复现普通 Hotfix 分页和资产归档依赖缺口，正在核实完整合同并实施，尚未达到停止条件。


## R8 独立复审与修复

基线 `f84cf0f4`（tree `d095d634`）加 R7-03 两处明确冗余输入清理。新审计者确认两项新问题及 R3-03 普通分页残留；抽查 Source 固定引用/batch/cache、Live 恢复/采样、addon 生命周期、交付和 Skill 未确认其他新项。没有为了分类齐全删除接口或增加框架；修复后继续独立 R9。

### R3-03 残留：普通 Hotfix page 的 continuation 来源

- 状态：代码已修复，真实 continuation/历史页/共享持有/发布间隙正式回归及 race 通过，沿用原 ID；本批整合进行中。
- `InspectHotfix` 不使用 scan 时，page blob 含 Source，但 metadata locator 是 dbcache digest。保留首 page 后删除 Source 成功；按正式 Skill 的 derived snapshot / `--from source.id` / `--after-index page.nextIndex` 继续返回 `vault.record_missing`。
- 最小边界：普通 hotfix-records 页声明既有 Source 依赖、同租约核验/发布；历史页仅按已知 kind/有界正文兼容，不重写原 ID/字节，不递归搜索任意 JSON。保留固定源、filters、分页与 whole-query coverage。

### R8-01 · 中 · 资产 manifest 的档案 capture 引用可悬空

- 状态：代码已修复，真实 raw/png export、local/CASC/partial demux、历史/当前持有和发布间隙回归及 race 通过；本批整合进行中。
- 实际 `DemuxAsset` 的完整 manifest 保留 SourceCapture/Frames；保留 manifest 后仍能删除 Source 或 frame capture，随后 archive Fetch 报 record_missing。`ExportAsset(png)` 的完整 manifest 同样不能保护 encoded ArtifactCapture。
- 档案合同来源为 `skills/lycheedev/references/asset-export.md` 的保留 artifact/manifest/captures、原 bytes 的 evidence verify，以及正式 video_demux 测试从 Source/Frame capture 取回精确字节。这里没有自动 demux-resume 合同，也没有声称外部导出文件永久存在。
- 修复复用已存在的不可变 dependencies 和同删除 lease 发布校验；只处理确切普通页/asset-export/demux 有界形状。Demux 的帧引用继续保留在已知 manifest 正文中，所有帧在相同租约下验证后发布，Source 用小型显式依赖，避免重复 15,000 个 ID 超过既有 1MiB metadata 上限。正式 15,000 帧 fixture 不生成外部帧文件，验证原 16MiB manifest 能正常提交并保护帧，未缩窄支持限制。
- 正式回归覆盖实际导出、共享持有、历史 IDs/字节、释放与六个真实生产提交间隙；旧生产实现及旧 archive 两组负对照失败，新回归通过。records/evidence/vault 12 包、依赖/scan/export/demux race ×3 和 vault lease/blob/metadata race ×3 通过，不新增通用引用框架。

### R8-02 · 中 · evidence bundle 最后压缩期间取消仍成功发布

- 状态：代码已修复；公开 CLI 正式回归与 race ×3 通过，整合尚待本批冻结。
- 真实 16MiB capture 在 `.lycheedev-bundle-*` 已写入超过 1MiB 时取消，原实现仍压缩完毕、link 最终 ZIP，Execute 返回 exit 0 / ok=true / complete=true；取消已先于返回约33ms。根因是最后一次 ReadBlob 后没有 context 检查。
- 修复只在 ZIP close、stage sync/close 完成后、os.Link 发布前检查已有 ctx；不承诺抢占压缩，不改变正常 ZIP bytes/选择/目标冲突语义。
- 正式 `TestEvidenceBundleCancellationBeforePublication` 在可读完整 ZIP stage 的公开 context 边界确定性取消，不依赖时间或生产 hook。核对 exit7/command.cancelled、无最终文件/无 stage 遗留，以及新 context 用原参数重试可成功。旧代码负对照失败；原16MiB真实取消复验变为 exit7、无输出，耗时/停止可见性如实记录。

### R4-C01 原生确认与最小修复

- 状态：由候选升级为确认的 Windows 宿主缺陷，代码已修复；Windows amd64 build/vet/test 交叉编译通过，原生正向验收待下一 head；不属于游戏验证。
- `f84cf0f4` 的 [Windows contract job](https://github.com/Follen/Lychee-Dev/actions/runs/38540889167/job/115775070032) 原生执行：两个不同 volume GUID，项目 D:、隔离 client C:；既有同卷测试和同卷对照通过。跨卷默认 archive 在 `outsideDiscovery` 的 filepath.Rel 失败，尚未发布 recovery archive，旧 3.1.1 receipt/树完全保留。PID 始终 0，未进行 process discovery 或输入。
- 合同依据 `internal/delivery/update.go` 已明确目标安装与 workspace 可以分属 D:/C:，恢复目录须留在目标卷并位于发现目录外。修复保留同卷项目默认和显式 --output 原值；不同 canonical path volume 的默认归档改为 client/Interface/.lycheedev-repairs/<timestamp>，位于 AddOns 发现范围之外且不占 update cleanup 的 .lycheedev-updates 命名空间；不加复制事务、unsupported 限制或删除旧恢复记录。
- 当前 CI 的 contract 因上述正式探针失败；已通过的 process/addon/package 不掩盖它，修复后必须以新 head 原生重跑，不能把这次失败改写成通过。


### R8 验证与覆盖率准备

- 冗余输入先删除并完整回归，再准备覆盖率留证。现有 Windows 完整 Go 作业改为同一次运行收集 atomic 跨包语句数据，测试选择仍 `./...`，计量仅 cmd/internal 产品包，排除 testkit；不增加第二套重复完整执行。
- 原始 profile、test JSON、准确 tested commit/tree 与 PR head、工具链/平台/命令、实际 exit 和所有选定包终止/skip 证据将保存到 `windows-go-coverage` artifact；汇总按语句数加权，不用函数百分比平均。Go 比例不覆盖 Lua/Node 或业务场景，不设任意比例门槛。
- 已在 R8 提交7566493c启动原生覆盖率采集，不提前报告数字。R8 普通 build/vet 与完整 Lua 必需整合已执行：30 测试包通过、command 1 包仅3个既有Linux平台失败、4包无测试；2484 test/subtest pass、3 fail、28 skip，无新增失败。[本批原生 CI/覆盖率](https://github.com/Follen/Lychee-Dev/actions/runs/38544503854)正在运行。


受影响 Node tools 全套实际76通过/6个Windows专属skip、0失败；覆盖率汇总工具的10项均通过；保留的真实整合 JSONL 与当前 go list 清单解析为 2456 pass / 3 fail / 28 skip、35 包且无事件解析错误（仅兼容性验证，不是新覆盖率测量）。没有把零语句/无 block 包臆测为 0% 或新增门槛。

R9 主审已开始，产品/正式测试按固定835文件指纹复核；另有独立 Skill 试用者按用户追加要求执行真实离线 Source/Data/Live 状态机/交付任务链，游戏环节仍只记录。R9 已复现 SQL 查询 capture 产生至 CSV 归档之间的删除间隙（R9-01），尚待下一批最小修复；此项不回填为 R8 已修。独立审计尚未达到无可离线事项的停止条件。


## R9 独立复审、Skill 实际试用与修复

独立新审计者读取 R8 冻结的835个代码/测试/工作流文件（随后发布为7566493c），另由不同试用者执行真实离线任务链。确认 R9-01、R9-02 两项新问题，以及 R6-02、D-03/D-07 残留；后者沿用原ID，不按每个死符号或旧事项重复计数。未新增仅游戏可确认的缺陷，现有六项实际结果仍空。修复后需新的独立 R10，未达到停止条件。

### R9-01 · 中 · SQL CSV 发布时未复核 query capture

- 状态：代码已修复，正式真实producer边界测试通过，旧代码负对照失败；批次整合进行中。
- 公开CLI先 QueryData 再 ExportQueryCSV，中间 metadata lease已释放。对真实SELECT结果正常RemoveCapture后再导出，原实现退出成功、manifest.complete=true、CSV可核验，但其中QueryCapture为record_missing。既有locator能保护发布后删除，不能覆盖发布前间隙。
- 最小修复为CSV CaptureDraft增加query依赖，复用Archive.CommitCapture既有同lease校验，不增加锁/通用引用框架。正式测试覆盖新目标与覆盖旧pair两分支：缺查询时返回缺失错误、不发布或改写任何原CSV/manifest；正常导出后保护query，移除导出capture后可释放。

### R6-02 残留：Context 复用行索引

- 真实Browser.Context对1,168,118字节合法Lua返回200片段、19,740字节正文，旧实现重复整文件string/CRLF/Split，实测302,193,008字节分配、行索引构建0次。Search/Inspect已有有界索引未接到Context。
- 代码已改用同一documentLineStarts，编号/CRLF渲染与Search/Inspect共用小helper；保留8MiB/256文件边界、原hash、line/byte cursor、UTF-8截断、CRLF/CR-CR-LF/末尾CR/空行和行数/字节上限。正式200片段测试使用宽松64MiB总分配上限，不用耗时门槛；旧实现负对照302,187,496字节并失败，新正式实测1,014,000字节、仅1次行索引，定向通过（受控fixture，不承诺真实用户耗时同比）。

### D-03/D-07 残留：退役解析器与零消费者接口

- 已删除bridge/persisted.go、savedstate.go及仅测该孤岛的两份测试；它们没有CLI、历史OP状态/retire、Native CON或Skill消费者。历史证据实际使用VerifyReport，保留report/signal/checkpoint/history路径与正式回归。
- AST候选经全仓消费者/接口核对，仅删除BeginBootstrapWindow、Book.FindRequest/SetGoal、Native.RetryFirstBinding、FrameStream.CaptureArea、CacheManager.Stream、selection.TargetSummary、navigator.openOptional，以及其专用area字段/赋值、PartialTableUnavailable常量/import。保留真实WGC ROI、cache Read/Retain、CON admission/BTP处置；BeginWork/LookupRequest/CommitReport仍构造实际历史消费者需要的fixture，保留。
- 精确13文件删除集为695行删除/2行格式调整。只测退役代码的测试随产品删除，区别于R7-03重复输入专项；没有因表面相似移除失败注入/跨进程/平台覆盖。独立overlay Linux build、Windows交叉build、剩余bridge/live/evidence行为测试和全套测试编译通过；正式批次完整行为验证正在执行。

### R9-02 · 中 · Skill 的两固定 pin diff 缺少按需索引准备

- 独立真实本地Git sync→diff试用：第二pin从未query/index时，按Skill运行diff退出5/index_not_ready；只为该pin执行index后，完全相同diff成功并返回一个document change。
- Design和source-preparation/source-research约定CLI按需准备mapping、index为可选warm-up；现有EnsureIndex注释也明确避免sync→research额外往返。CompareSource遗漏这一准备，而低层CompareTrees要求已有完整索引的合同本身正常。
- 状态：代码已修复，在metadata发布lease外复用EnsureIndex，固定两端commit、同请求cache，低层完整/兼容检查保留；正式cold→cold/warm→cold公开CLI、3种拒绝、capture provenance/verify及race×3通过。旧代码两种冷路径均失败；新home真实sync→diff→verify无需额外index通过。不以修改Skill转嫁额外步骤代替代码修复。

### PR12 必要修复移植（单独记账）

- PR12精确head3780b5d8的Definitions预算修复确有必要，已移植definitions.go及两项正式预算回归。冷HTTP最坏8MiB读取缓冲改ReserveScratch并释放；持久metadata/retained与热缓存相同按actual raw×4计费，失败不发布。
- 独立A/B用132字节两文件：旧冷路径16,777,612字节、热528字节；修复冷/热均528字节，临时峰8MiB且结束归零。在128MiB总预算已占120MiB时旧实现误失败，修复成功。PR13新正式定向通过，旧定义实现负对照失败。
- PR12地区槽位部分**不移植**：当前七产品模型已有forever-asia-beta与forever-cn-beta独立身份，availability已等价；两份现行Skill明确legacy forever远端保持亚洲发布槽，原remote_target正式cn fixture也使用亚洲槽。legacy+cn仍可调用并不能证明必须改成中国槽。初次移植曾改动该行为，R10通过当前合同与真实resolver fixture发现冲突，发布前已完整撤回，仅保留六组legacy/明确两beta×us/cn的CDN、远端、离线固定pin回归以锁定现有合同。没有改Skill来迁就非必要代码，未恢复两份旧文档。PR12在必要预算修复覆盖并推送确认后关闭。


### R8 精确提交阶段覆盖率与原生验证结果

- 阶段代码为PR head `7566493c0e0bcd92a739075107d923557ea2a14a`。Actions实际checkout merge commit `d5f0d7b975e6eceb0c2ed61bcdbb3597af27183c`；其tree `e827851dc4329e13ef2ffb0d04d6a671d3d21620` 与该head相同。R9仍在修改，以下不是最终修复树的比例。
- Windows/amd64、Go1.27.1，固定Lua5.1.5必需模式；同一完整运行：`go test -json -p=2 -parallel=4 -count=1 -timeout=20m -covermode=atomic -coverpkg=<cmd及internal产品包，排除testkit的精确清单> -coverprofile=<runner-temp>/cover.out ./...`。完整argv与28个instrumented包的清单在identity/summary。
- 结果：exit0，2747个test/subtest pass，0 fail，35 skip，0 incomplete。已覆盖23976/32423条Go产品语句，按语句数加权 **73.9475%**。重复块合并，不平均各包百分比；仅测试的navigatetest没有产品blocks，不虚构0%。cmd main自身4条未覆盖另记0%，不隐去。
- 核心包：codebase72.9203%，environment81.6327%，flow78.4615%；records81.3938%，relational84.4088%，schema86.4458%；live47.4240%，channel62.0391%，journal70.4293%，memory87.6972%；evidence78.9610%，delivery80.1714%，vault76.5371%，command59.6685%。这不是业务场景覆盖百分比，也不包括Lua/Node语句或游戏验收。
- 35个skip名称保留在summary/events，包括未配置真实LuaLS、固定API/DB2/CASC/图像/runtime输入、真实窗口/WGC、宿主8.3短路径能力，以及8个仅供子进程启动的helper入口；helper被父用例启动与顶层skip应分开理解。不得把这些都称作35项缺游戏测试，亦不把skip充成功。
- [原始coverage artifact](https://github.com/Follen/Lychee-Dev/actions/runs/38544503854/artifacts/11883583264)含profile、事件、identity、summary、stderr；profile SHA256 `5977396ef3e4343959e0d9e291c768d20800816c320c30660ff6061e15c5c20d`，events SHA256 `5ad351430bbefb3617b5ee701986814a6d3df9ae59900ef5e96f0a2292529f94`。contract作业success；process/addon/package也通过，随后race及汇总也通过，六作业全部success。
- R4-C01原生正向通过：[同一contract job](https://github.com/Follen/Lychee-Dev/actions/runs/38544503854/job/115787109354)真实D/C不同volume GUID，同卷对照、跨卷默认archive、跨卷显式archive均PASS；已归档旧3.1.1树、安装新3.1.2并仅在PID0安全激活边界停止。修复已获宿主验证，不再列为待游戏或宿主未执行；保留R7失败作为负对照。


### R9 Skill 实际任务覆盖（独立试用已完成）

- 独立冻结开发CLI共99次真实调用，77次exit0；其余包含特意的错误输入、跨scope cursor、缺pin、宿主能力、fixture纠正和R9-02复现，不能把调用次数当通过率。每步消费真实上步pin/symbolId/cursor/capture/文件产物，不只读Skill文本。
- Source：实际本地Git tag/commit同步、固定版本query/原始inspect、symbolId→refs/context/batch、跨scope拒绝后纠正、显式pin覆盖冲突project且lock不变、两版本diff。Data：名称歧义/SQL稳定分页/FK/CSV/JSONL；Hotfix归档后修改原文件仍可从固定source续页、scan原checkpoint恢复及显式effective overlay；evidence verify/bundle对象读取。
- 交付：实际Skill launcher/install/update/remove、隔离干净500-slot addon磁盘install/update、修改过的树拒绝覆盖和正常remove。全是新建隔离fixture，不是用户游戏目录或已发布Windows安装结果。
- Live：正式channel/protocol的264个节点通过，另用新合成跨语言任务跑9场景/10节点通过，保留原CON/request/deadline、已确认报告与pending cleanup区分。Linux capability拒绝如实保留，不作为新缺陷或Windows/game通过。唯一新确认编排缺口为R9-02；轻微帮助可解摩擦未膨胀为bug。
- 真实首次reload、战斗/受限焦点/最小化/按住修饰键、runtime退役与共享安装的复验，需要后续授权的固定Windows/WoW进程及干净受管版本；按原CON/request恢复，预期不越权输入、不重放、不删除其他owner。这些是既有待实机/可选覆盖的场景补充，不计新增确认缺陷，实际结果留空。正式Windows宿主/打包路径由本PR新CI另行证明。


### R9 批次验证进度

- 当前产品/正式测试/Skill/工作流共898文件修正后冻结，aggregate SHA256 `327283859ee3ebc7b474d4fc2853206ccdd199f4549f9381b40937e6f629229f`，parent7566493c；还未形成R9提交。新鲜R10正在对该固定树独立复审，不把旧R9的修复前审查当修复后验收。
- build/vet、Windows amd64交叉build，19个Source/records子包/evidence/bridge/live子包/vault/selection正式包通过。Context/SQL/预算/journal定向race×3通过；bridge在该regex下无匹配测试，明确不计其race覆盖，但其完整普通包测试通过。
- R9-02组另完成build/vet、完整codebase及公开CLI定向/race×3，源码和两端pin保持原固定身份。import合同和99个本地文档链接检查通过。修正树完整Lua必需套件已实际执行：2506 test/subtest pass、3个既有Linux fail、28 skip，30包通过/command1包失败/4包无测试；Windows必须以新提交的原生CI证明，R8覆盖率只是阶段值。


### R10 独立复核完成

- 新审计者起始核对898文件SHA全部吻合。先发现PR12旧地区别名移植与当前两份Skill/既有测试的兼容合同冲突；真实legacy forever/cn resolver在合法Asia fixture中访问CN槽而失败，证据不足以支持改变合同。发布前已撤回这部分产品变化，保留必要预算修复和新增原合同六组合回归。
- 修正只涉及此前移植的6文件，其余892个冻结文件保持原SHA；修正树aggregate见上。Skill无需修改，正式六组合已通过；上一临时树完整Lua必需运行2507 pass/3个既有Linux fail/28 skip仅作诊断，不冒称修正树整合结果。修正树build/vet及完整指定套件已重跑：2506 pass/3个既有Linux fail/28 skip，无新增失败；六组原身份合同race×3通过。
- 独立30000组Context新/旧差分涵盖UTF-8、CR/LF、范围、行/字节预算与cursor，均一致。AST初筛剩余GoString/MarshalJSON/heap方法有真实接口消费者，不删除。本轮其余已检查边界完成后未发现其他可操作离线修复项。终点898文件SHA与修正基线全部一致；这一结论不外推为全局零bug，也不替代新head Windows验证。新增coverage专项的16项公开输入准备单独继续。


### R8 语句覆盖与35个skip独立专项（后续准备项）

原始artifact已独立重算：23203个唯一block，696091条重复block记录按同源范围合并，23976/32423=73.9475%，未发现分母重复。2747个pass是test/subtest终态事件，其中244个为有子测试的容器；仅叶节点为2503 pass/35 skip，两者都不是独立业务场景数。`TestExtractPinnedUnitDocumentationWhenAvailable`父节点pass，但retail/forever两个子项均skip，不能称固定文档验证通过。

| 数量 | R8原始skip名称与原因 | 后续安排 |
| --- | --- | --- |
| 8 | `TestInstallationWorker`、`TestUpgradeWorker`、`TestAdmissionCrashHelper`、`TestWindowOwnerProcessHelper`、`TestWindowRunProcessHelper`、`TestNativeModuleProtectionChild`、`TestNativeModuleMappedChild`、`TestNativeMemoryChild`：只作为子进程入口 | 10个调用它们的父测试均在原始JSON实际pass；顶层skip正常，不删helper或计作缺环境 |
| 11 | `TestRealBrokerAcrossCLIProcesses`、`TestRealBrokerCloseWaitsForProcessCleanup`、`TestPackagedSourceSemanticCLIWhenAvailable`缺已编译CLI/verified release；`TestRealLuaLSMapsAPIReferencesAndDefinitions`、`TestRealRuntimePublicInterfaces`、`TestRealRuntimeOversizedLuaKeepsOrdinaryCoverage`、`TestRealWorkspaceSessionWarmAndCancellation`、`TestRealSessionCancelsAnInflightReadinessRequest`、`TestRealSessionEnvironmentAndConfigIsolation`缺真实LuaLS runtime；`TestRealRuntimeGeneratedEnvironment`缺runtime/docs；`TestRealRuntimeFullRetailAPIReferences`缺runtime/完整Retail源码/生成定义 | 公开固定输入可准备，其中9项已有release CI必跑流程；单独下一批前移复用，不等用户游戏 |
| 3 | `TestBuildPinnedUnitDocs`；`TestExtractPinnedUnitDocumentationWhenAvailable/retail`、`/forever`：未配置测试已固定commit/SHA的API文档 | 公开固定文档可离线准备 |
| 1 | `TestPinnedPublishedDefinitions`：固定WoWDBDefs网络输入未启用 | 固定commit可准备 |
| 1 | `TestCascLibRawFixtureOracle`：fixed CascLib executable未配置 | 仓库已有provenance DB2与build-oracle脚本，准备官方固定CascLib依赖 |
| 3 | `TestCaptureItemForExplicitProcess`、`TestCaptureFramesForExplicitProcess`缺显式授权测试PID；`TestWGCLifetimeBetweenSessions`缺交互桌面开关 | 优先尝试Windows CI的隔离自建窗口/桌面条件；无需WoW，不操作用户机器；实际不支持则单列宿主限制 |
| 1 | `TestClientWindowLiveReadOnly`：缺显式固定客户端只读目标 | 真正客户端输入条件，后续授权任务；本次结果留空 |
| 4 | `TestInstalledAssetCommand`、`TestInstalledBuildMetadata`、`TestInstalledIndexReadOnly`、`TestImageExportRealSample`：缺固定安装/产品/build、本地CASC index或icon.blp/png/webp样本 | 分别记录原始资料边界，不拿合成fixture伪装真实样本通过 |
| 2 | `TestLuaMailboxRootRecipeOriginalRuntimeDump`、`TestLuaMailboxRootRecipeCapturedRuntimeHeader`：缺原dump manifest/header/匹配hash executable | 后续提供既有获准原始输入再replay，不连接或采集游戏 |
| 1 | `TestInstallationLeaseShortPathSpelling`：runner卷禁用了8.3短文件名 | 不为减少skip修改宿主安全/卷策略，保留环境限制 |

因此35项中8个是已被父测试执行的helper入口，27个是真正未跑的顶层或叶测试；其中16个可用公开固定依赖/源码补齐。覆盖率不是“合格阈值”：优先补真实LuaLS/broker取消/恢复/隔离路径（当前source_broker.go、session.go语句覆盖较低），再固定领域输入；不为百分比添加无价值重复测试。build-tag专项与主覆盖运行范围不同，主profile未加入casc_oracle tag，不冒称它已计入分母。下一批新CI结果与skip变化将按精确提交更新。


R9修复及当前审计/交接/todo/Changelog已推送`ef9b0113e606db0fa6717f4203c93d6e11537216`（tree `b05c5ce71b114999bee7f8b6b1baf6448fac79c7`），[该head原生CI](https://github.com/Follen/Lychee-Dev/actions/runs/38547831499)进行中。PR12必要预算修复已从远端读取确认，PR12随后关闭、未合并；非必要地区槽位未移植。16项公开输入补齐是下一独立CI批次，尚未记作新增测试通过或新覆盖率。


R9 `ef9b0113` 的Windows CI38547831499现六作业全部success。Go产品语句23852/32122=74.2544%，2769 test/subtest事件pass、0fail、35skip；skip名称与R8逐项相同。实际merge `5bae9e984a7f3212208c986039b2b6b621ac9a3e` 的tree `b05c5ce7`与PR head相同，[原始artifact](https://github.com/Follen/Lychee-Dev/actions/runs/38547831499/artifacts/11885855845)。这是16项输入补齐前的R9阶段测量，不把分母随代码增删变化当作业务覆盖已完善，也不冒用R8的叶节点数作为本次叶节点数。


## R11：公开验证输入补齐与独立复审

### R11-01 · 验证准备项 · 16个已有正式集成缺公开固定输入

- 状态：准备实现与正式工具回归已完成，原生执行待本批新CI；不计作16个产品bug，不提前降低skip或提高覆盖率。
- 根因证据为R8原始事件的确切skip原因及现有release流程：9项LuaLS/broker测试已经在发行CI要求实际通过，却未在PR完整套件选入其固定runtime/release；其余API文档/完整Retail/DBD/CascLib也有固定公开来源，不能误称必须等用户游戏。
- 最小实施：现有完整instrumented Go运行前准备已有pins/checksums，复用live-stage、LuaLS完整性检查、native binary身份验证、生产environment.Build和既有CascLib builder；不新增第二套完整suite、不改发行策略/产品行为。验证15个现有顶层名称与必需子项（对应原16个skip叶/顶层事件），拒绝missing/skip/fail及缺package终止证据。WGC另跑一个自建窗口lifetime探针，失败显性并保留原始artifact，不自动把能力缺失计通过。
- 固定材料：Gethe Retail `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`、Forever `4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069`，Unit文档沿既有SHA；WoWDBDefs `83057bdc0cbe13062850ebf8ad530031e128a1cd`；CascLib `38a34665624b8775bb875274b36191b21c38d97b`。gcc/g++ preflight记录版本，缺工具明确失败，不安装替代来源。
- 已实际离线验证：固定文档Environment及Retail/Forever flow通过，固定DBD父测试与Map/SpellName/ChrClasses通过；生产生成器处理612文档/5873 mappings，保留真实partial-type覆盖receipt；build/vet及adapter vet通过。stage核对Windows PE、1285个resource hash和实际binary bytes/SHA/VCS身份，仅为构建形状，不计原生LuaLS通过。CascLib仅确认固定clean checkout与37个构建源码路径，不把Linux源码检查当Windowsoracle运行。
- 新增/受影响工具Node全套实际82 pass、6个Windows专属skip、0fail；定向harness/workflow14项通过。旧R8原始35skip事件被新required-integration核验正确拒绝，不能用父pass掩盖未执行。
- 当前901文件冻结aggregate `605dfe687ce6b6bcc4188e6322060b3e8f6b3afd310cd69ca3232ae198a0328c`，parentef9b0113；全新R11审计者独立检查该树，尚未出最终结论。后续新原生结果和skip变化按精确提交更新。


### R11 独立复审完成 / 原生执行中

- 901文件起终SHA和aggregate均与冻结基线一致；独立检查公开输入来源/身份、生成器partial合同、stage资源与native身份边界、正式集成选择和失败/skip证据链。本轮已检查范围未发现其他可操作离线修复项，不作全局零bug或原生通过声明。
- 实际CLI级负对照：缺必需子项、缺package终止及旧R8真实skip事件均拒绝；BOM/CRLF合法证据通过，无关helper skip不扩大门禁。同一严格coverage reader拒绝fail后pass洗白。独立核对1285资源hash、binary bytes/SHA/VCS、固定docs及生成定义receipt全部相符，complete=false如实保留。
- 本批已推送`ebde90ed346ff22d087af29a5975f7f3e672ad6e`（tree `fee83999d5581bc38ec8d790d49f3f7b05b23284`），[Windows CI38549548276](https://github.com/Follen/Lychee-Dev/actions/runs/38549548276)正在执行。实际skip/pass/覆盖率必须从本批artifact取得，不能按35−16预测后宣称通过。WGC另一次自建窗口测试不并入主profile/事件计数；CascLib missing-key子项仅证明Go严格拒绝，不称外部oracle同值验证。


### R11 新原生执行结果（ebde90ed，失败证据保留）

- CI38549548276：process、addon、package、race通过；contract及汇总失败。固定输入准备成功，原16个skip事件均进入执行；主套件2795事件pass、3父子fail、19skip（叶节点2542pass/2fail/19skip）。失败集中在`TestRealBrokerCloseWaitsForProcessCleanup`及其reply/listener两子项；其余公开输入集成通过。
- [原始coverage/required-integrations](https://github.com/Follen/Lychee-Dev/actions/runs/38549548276/artifacts/11886332314)的tree为fee83999、实际merge948f05bf，与head树一致。24381/32122=75.9013%仅失败运行诊断值，不作为最终验收覆盖。profile SHA2565363f8850ccaed1ed3027ae817faa1226a136bb5d0eec03a5a6e5522704bd189；events SHA256fbb21ebe3e97740bb367dca5d43af8ac25300e9989f138a373bbe8d3bf2dd138。
- [单独WGC自建窗口](https://github.com/Follen/Lychee-Dev/actions/runs/38549548276/artifacts/11885714946)在CreateCaptureItem失败，HRESULT0x80070057；未获得frame/lifetime通过证据。不能仅凭HRESULT把它称作缺交互桌面或游戏问题，继续检查fixture/interop/宿主证据。该结果不并入主profile。

## R12：新原生失败定位与最小修正

### R12-01 · 已确认测试编排缺陷 · 两个真实broker集成争用同一登录会话单实例

- 原生证据：codebase跨CLI测试worker存活10:30:12.700至10:30:17.675；command的reply/listener子进程在10:30:13.627/14.418结束，首事件为空，均未进入原关闭屏障断言。sourceBrokerMain在发listening前按既有合同TryAcquireLease登录会话唯一aggregate，失败立即退出。此前发行集成-p=1未触发这条并行路径。
- 已实施测试专用共享OS lease：仅这两个集成等待实际logon scope的独立test key，等原child/session清理完成再释放；复用现有lease，不修改产品aggregate限制、不隔离掉真实竞争域、不提高超时、不把完整套件降为-p=1。正式Windows跨进程/不同home回归核对gate互斥、cleanup释放及production aggregate不被占用；helper顶层明确skip，父测试运行才算覆盖。旧EOF诊断补上真实child exit/scanner error。
- build/vet和受影响离线31 pass/0fail/1可选runtime skip，Windows相关build/vet和三个测试程序交叉编译通过；均不替代Windows运行，原生重跑待结果。

### R12-C01 · 待核实 · WGC自建窗口CreateForWindow失败

- 已知interop GUID及IUnknown方法槽与SDK一致，原fixture为160×120可见WS_POPUP/WS_EX_NOACTIVATE/WS_EX_TOOLWINDOW。当前仅确认CI失败，尚未确认产品bug或宿主能力限制。独立desktop组检查标准窗口资格与真实session/桌面证据，必要时准备安全的自建窗口对照。
- 已准备仅test文件的失败后诊断：process session、station可见面、fixture desktop input、HWND可见/尺寸/style/cloak/affinity、manifest敏感的DWM值和IsSupported；失败后原HWND晚时点item与非tool标准顶层窗口item对照。原错误仍硬失败，控制成功不顶替lifetime通过；无输入、激活、窗口枚举或策略修改。Windows交叉build/vet和Linux desktop包通过，原生诊断尚未执行。
- 所需环境：Windows原生runner及测试自建窗口，无用户游戏或外部PID。最小步骤为记录session/桌面能力，分别对fixture及正常顶层窗口建item并采帧；预期能在可支持环境完成10轮生命周期且delegate归零，不能支持则保留明确宿主证据。当前失败记录如上，复验结果留空。


R12修正/诊断树904文件冻结aggregate `bd962ee611d60876f16d05daaefee5e3cd91b325663a186dc164d0494fffd210`，parentebde90ed；完整指定Lua必需Go套件已启动，另由全新R13审计者独立核验实际scope/cleanup/ABI/证据合同。诊断未运行前不把R12-C01定为环境限制，复审未完成不预填通过。


R12已推送`787eff3b4fdb5c49ef5da4164e684c569ca0e1e0`（tree `f58ea9ca57cbaaadecadab61e6506bab582a089d`），[新原生CI38551956825](https://github.com/Follen/Lychee-Dev/actions/runs/38551956825)运行中。该冻结代码build/vet通过；实际完整Lua必需Linux套件2506 pass/3既有平台fail/28skip，30包pass/command1包fail/4包无测试，无新增失败。Windows交叉编译不充原生通过；WGC原因和R13结论尚待结果。


## R13：R12固定修正树独立复审完成

- 新审计者起终核对904文件及aggregate bd962ee6均一致，发布后787eff3b与审查代码吻合。独立复核测试gate实际logon scope/独立key、两集成child/session退出先于gate释放、EOF证据，以及跨进程不同home回归。当前回归证明互斥和cleanup后可用；cleanup回调具体顺序另外是静态核对，未称作mutation验证。
- 对Microsoft SDK/API独立核对WGC statics/interop ABI、只读宿主信息及失败后自建标准窗口对照，未发现错槽/参数或越界操作；原失败不被诊断成功替代。该范围未发现其他可操作离线修复项。
- 新原生CI仍须确认R12-01实际通过，并取得R12-C01宿主证据。当前diagnostic hard-fail会传播至required contract，但不意味着现行发行合同新增了必须具备交互桌面的要求；依实际诊断作最小后续处理，不提前归因或静默skip。


### R12 原生重跑实际结果（787eff3b）

- CI38551956825完整Go及全部16项required公开集成通过，race/process/addon/package通过；仅独立WGC probe与汇总失败。R12-01两关闭子项和新跨进程gate父测试均通过，生产broker合同不变。
- Go主套件2800事件pass/0fail/20skip，叶节点2545pass/0fail/20skip；新增`TestSourceBrokerGateHelperProcess`的顶层skip对应父回归真实pass。因此20skip中9个helper，11项真实未跑：3个desktop诊断、1个客户端read-only、4个原安装/index/图像资料、2个原runtime replay、1个8.3卷能力。16个原公开输入缺口已全部实际执行通过，不能把额外helper入口误算为回退。
- 24737/32122=77.0095%为此精确head主套件Go语句覆盖；[artifact11886584881](https://github.com/Follen/Lychee-Dev/actions/runs/38551956825/artifacts/11886584881)，实际merge729e5813的tree f58ea9ca与head相同。主套件通过和独立WGC失败分别记账，整CI尚非全绿。
- [WGC原生artifact11886739257](https://github.com/Follen/Lychee-Dev/actions/runs/38551956825/artifacts/11886739257)：session2、WinSta0、WSF_VISIBLE=true、Default/UOI_IO=true；DWM=true、IsSupported=true均S_OK，原窗口和标准控制窗口均可见、未最小化/遮蔽、affinity0且尺寸有效，但两者CreateForWindow均0x80070057。这排除了此前“缺交互桌面”的无证据归因；尚不能定性产品或具体宿主限制。继续R14针对真实Go/native调用的独立定位，不改成静默skip。


## R14：WGC真实调用边界独立定位

- 新审计者独立核对SDK接口、当前Go 1.27.1 Windows-target escape analysis及真实日志：nativeInterface.call带uintptrescapes且不可inline，item/size/factory/HSTRING输出均上堆；实际GUID、槽位、参数及locked MTA生命周期未发现错误。不存在已证实的移动栈指针修复依据，也无官方合同支持一概禁止同进程HWND。
- 缺口是失败时线程本地restricted error及独立SDK调用结果。现有helper在返回前Release，不能假称事后诊断拿到原始error。下一最小验证由测试内同线程直接复用原factory/call路径，在失败后任何COM Release之前获取细节，再同进程/同MTA/同一个已知自建HWND调用SDK-typed小型DLL。仅辨别调用边界，不发送输入或操作游戏。

## R15：SDK对照验证准备与独立复审

- 新增仅测试的SDK/WRL adapter及Go result-layout正式回归：固定3660-byte结果、版本/IID核验、仅本进程窗口，COM/BSTR均释放；Go与SDK均清除陈旧error后立即读取当前失败细节，buffer原长度保留截断事实。原lifetime失败仍不变，对照成功不代替frame/lifetime通过。
- 仅复用runner预装MSVC/Windows SDK，编译日志、SDK/VC版本、source/DLL/hash随同原始WGCartifact保留；无下载安装、无产品依赖/发布内容或策略更改。Windows Go交叉build/vet、Linux desktop、import合同、workflow正式10项及YAML检查通过；本地无MSVC，原生C++编译和运行待新CI。
- 本批906文件冻结aggregate `f6529b24f2a1ebe2db02c8f85f73655a3e5a9a35b31fb28f8c2224d4f767acd5`（审查期间仅workflow及其正式测试增加显式临时工作目录，避免编译器默认旁产物污染checkout；其余904文件保持原SHA），parent787eff3b，新鲜R15审计者复核ABI/cleanup/PowerShell编排。原生未执行前不填写通过；SDK若同样失败只证明该runner/这两个fixture的官方API边界，不扩展为产品平台限制。

## R16：用户追加高风险覆盖补强（当前结论见R19–R22更正）

- 基于787eff3b实际未覆盖块及现有测试去重，隔离准备5个独立边界：Bundle完成stage后的目标抢占；archive已rename而journal仍planned；DeleteDocuments中途失败的文档与删除世代回滚；RecoverBinding拒绝bootstrap/Consumed失败后的原nonce恢复；finishRecovery报告保存失败后的原ticket/期限保全。只保留能证明新增边界的正式回归与负对照，不为百分比改产品合同。
- 当前仅在忽略的overlay中开发，不混入WGC固定树；实现、负对照、整合原生及最终覆盖均待后续实际结果。


R15独立复审完成：SDK header/IID、3660-byte ABI、restricted-error时机、COM/BSTR释放及PowerShell/cmd编排无新增确认问题。起点906文件吻合；终点只包含声明的2文件临时cwd修正，其余904未变，修正aggregate全匹配。正式workflow10项通过；MSVC/native运行仍待CI，不预填通过。

### R16-01 · R19已撤回当前缺陷分类 · 合成bootstrap拒绝恢复

- 新正式回归在787eff3b负对照复现：RecoverBinding验证并持久化精确slot_runtime_changed bootstrap拒绝后，若Consumed失败，transaction保持received；Load后实际Project→Continue路径直接重报拒绝，既不能完成回收/新绑定，也不能在closing分支关闭。wait.go仅input_attempted可进入原恢复入口，已核对真实消费者，不是任意错误状态假设。
- 下一隔离批次修正中：只消费已核实精确拒绝的原nonce，重试原槽回收，不重发不确定输入；之后依原closing意图退出或新challenge。修复/正式正反对照/原生结果待补。其他拟补存储/报告边界目前通过，不按用例数增加bug数。

## R17：用户实际Skill源码调查案例

### R17-01 · 已修复并推送0f26b9a7 · 可用Source调查仍外搜/猜raw路径

- 用户实际案例明确指定$lycheedev且Source可用，却额外搜GitHub并猜中404的raw路径，未增加有效证据、绕过固定版本调查。当前主Skill/source references未统一规定默认固定Source、空结果自查和外部例外的证据界线。
- 专项实施中，仅LycheeDev Skill目录：复用已有发现/固定版本/query/inspect，refs/context按需；无结果先核模式/关键词/版本/覆盖；只读工具返回或已核实路径；真实缺来源/版本/能力或用户明确要求才转外部并标缺口/证据身份。已足够证据直接回答，不增加审批、强制步骤或重复验证。修订后用该案例和例外行为独立复核、正式Skill/生成目录检查，再提交同PR。未修改Lychee Talent或连接游戏。


### R17-01 实施与验证完成

- 修改仅三份Skill文件：`skills/lycheedev/SKILL.md`用单一入口引用规则；`references/source-research.md`集中定义默认发现缺失来源/固定revision/query或inspect、refs/context按需，以及空结果核模式/topic/关键词/版本/coverage、禁止猜raw路径、明确外部例外；`references/source-preparation.md`把命名release的tag/ref发现收敛到同一规则，不把source list未列tag误当版本不存在。
- 同时去掉无条件target show/inspect式重复步骤：复用匹配pin、已返回路径和留存证据，足够即答。不新增审批、mandatory命令序列或版本替代；真正缺tag发现能力仍可用已核实外部元数据，再回source sync，证据与版本分开标明。
- Skill合同21文件、84命令、225引用、0违规；生成命令目录check、version check（3.1.2未变）、既有Skill合同测试2/2及diff检查通过。无CLI/API行为变化，不为纯指引重跑完整Go。未修改Lychee Talent、未连接游戏。

## R18：Skill源码路由独立场景复核

- 全新审计者只读检查三文件及source相关引用；起终SHA相同。17个决策场景覆盖用户实际可用Source仍GitHub/raw404案例、空查询/错误模式与topic/关键词/版本/coverage、足够证据/已返回路径、缺LuaLS但静态证据足够、真实来源/版本/能力缺口、用户明确外搜及命名tag发现例外。
- 本冻结范围无新增可操作冲突，无隐藏外搜回退、重复审批或强制步骤；生成命令目录未改。是文档规范与场景决策独立复核，不是实际模型执行轨迹重放，不保证未来每次执行必然遵守，也不声称已部署到用户环境。

### R16 当前实施状态（未包含在本次Skill提交）

- 六文件已从隔离验证复制到正式目录并冻结：evidence/bundle_publication_test.go，vault/archive_recovery_test.go、metadata_rollback_test.go，live/channel/recovery_boundary_test.go、driver.go、observations.go。实际回归依赖正式helper，不依赖临时overlay/fixture。
- R16-01已修正：复用原精确bootstrap拒绝的持久证据及旧nonce完成回收，再按原closing意图关闭或新challenge。退役前不可生成新输入，旧receipt暂态消失不阻断已核验恢复。
- 新确认R16-02：Consumed取消前提前写入Bound/operation stage/cleanup/Reload，使recordBlocker把逻辑已推进、旧transaction未退役一起落盘；重载后跳过原回收，非幂等commit后续确认停滞。最小修复将8处既有逻辑迁移集中到finishTransaction的内部callback，先Consumed成功再迁移/清transaction/原Save，不加新状态框架。
- 六个新顶层测试覆盖5个独立边界；18个回收子场景分别覆盖9种逻辑分支的回收前取消和回收后journal保存失败。旧实现11个叶节点负对照失败（2bootstrap+9取消），9个保存失败对照本就通过；四个定点故障mutation在预期数据保全断言失败，不按新增测试数增加bug数。
- Linux三受影响包364事件pass/0fail/0skip，六新测试race×3共90pass；build/vet与Windows测试交叉编译通过，正式文件无overlay定向重跑通过。完整Lua必需套件和全新R19独立复核进行中；未推送、未执行新head原生，不填写最终覆盖或客户端通过。


### R15 原生SDK对照结果与WGC收口

- 精确head `310a5d7db14d5e66179c3460eef0745eee0c356a`，tree `75770dbc98939433fd28df485d71c97e4dfc9b31`，[push CI38555308695](https://github.com/Follen/Lychee-Dev/actions/runs/38555308695)：完整Go及16项固定集成、race/process/addon/package通过；独立WGC和原汇总失败。主套件24734/32122=77.0002%，该阶段值不替代后续产品修正重测。
- [WGC artifact11890052167](https://github.com/Follen/Lychee-Dev/actions/runs/38555308695/artifacts/11890052167)下载SHA256 `a078f69c34da67d8fa16cb3222726bd8e2da4961337f9c6079bb8077370c0369`核对一致。Windows SDK10.0.26100.0/MSVC19.51.36260.0真实编译成功；两个自建HWND的Go和SDK共4次item调用均0x80070057，factory/QI成功，立即restricted-error retrieval/details均S_OK，文本均为“The parameter is incorrect.” / “Could not capture the given window.”，不是事后陈旧error。
- 这排除了当前证据中的Go特有ABI解释，未证明具体宿主/驱动/策略根因，也不证明一般Windows不支持。没有可离线确认的WGC产品修复依据，停止猜测式窗口样式试错。保留R12-C01为特定环境未完成验证，原测试失败不改成skip。
- 恢复既有发行合同边界：WGC单列非门禁观察，保留原test失败、exit1、raw events、SDK身份及醒目warning/summary；无效JSON、丢失/矛盾终态、编译/准备及artifact错误仍失败。复用Go coverage事件解析器的最小观察汇总工具有正式正反例；不把观察作业完成混称capture通过，不把独立probe并入主套件覆盖率。
- 后续复验环境：具备交互图形会话的Windows/amd64主机及Go1.27.1，无需WoW；记录OS/显示驱动/session/desktop信息。设置`LYCHEEDEV_TEST_DESKTOP=1`，运行`go test -v -count=1 -timeout=1m ./internal/desktop -run '^TestWGCLifetimeBetweenSessions$'`，只创建自己的窗口，不填写外部PID。预期10轮采帧/释放与delegate归零；如失败保留HRESULT与窗口信息，可使用仓内SDK适配器对照。实际复验结果：

## R19：高风险修正独立复审及更正

- 独立复核确认R16-02是真实回收取消边界缺陷：Native Consumed持有既有发布lease并幂等退役精确nonce，不触碰更新reservation；8处逻辑迁移后移符合真实消费者。独立三包364pass，九种回收前取消负对照失败，九种回收后保存失败对照通过。
- **撤回R16-01的当前确认缺陷分类。** 进一步核对生产者与受支持journal发现：`slot_runtime_changed`只有旧64槽/`lycheedev.slot.v1`协议曾产生，当前addon对其他runtime返回`slot_skipped_foreign`且不发布回执；当前协议/slot校验明确拒绝旧记录，没有使旧回执进入当前Connect的受支持迁移。前述fixture只证明合成状态，不能当真实触发。新增修复和用例撤回，不维持“两项当前bug”的计数。
- R19-01确认死代码：退役的RecoverBinding/ bootstrap_changed消费及专属测试已无当前生产者。清理范围不触碰RuntimeCandidate→RecoverRuntime、不扩大旧协议兼容；现代初始bind、closing、unknown slot及原请求不重放回归仍保留。
- R19-02另确认当前可达缺陷：unbind Consumed成功，Save遇到原关闭预算到期，deferred保存了Closing=true、Bound=false、transaction=nil与exhausted；Project Resume/Disconnect仍进入Continue，在纯终态记账之前再次被预算阻断。当前树和310a5d7负对照均复现；只允许已无输入/交易待做的完成记账，不能重新赋予操作预算。实施/正式验证待本批完成。

## R20：用户切换角色后旧连接closing现场专项（初始状态，后续摘录已补）

- 用户报告正式服12.1.0.69933、CLI3.1.2，角色切换与mailbox short_read/ineligible_read顺序未知，reload/resume无效、disconnect停在closing_exchange_unconfirmed。现场原CON日志/已安装命令输出尚未取得，不能把报告顺序或角色切换直接当根因。
- 已核实正式发布v3.1.2为旧三键/INPUT v3合同；PR13采用新两键协议且拒绝旧native日志，较新Skill不改变已装CLI能力。两版已有基于替代runtime证据或原进程结束的无输入退役路径，读取失败本身不证明旧环境已结束。后续仅按真实证据分析，不手工删claim/日志、不重放旧请求或以反复reload替代恢复。
- 本云无用户D盘日志读取权限，已由主聊天请求提供诊断材料；不连接游戏、不改Lychee Talent。离线独立组继续核对当前真实可达路径与正式fixture，现场因果/客户端验证保持未确认。


### R20 现场证据边界与已确认离线路径

- 已读取用户提供的230行命令输出摘录，**不是完整连接jsonl**。已装CLI确为发布v3.1.2/commit `9d3d3598`、200槽旧identity/hybrid INPUT，已装Skill却描述新Mailbox/500槽/live doctor；合同差异确认但不等于现场根因。原连接尚未bound；invoke submitted只证明6条消息入队，不证明执行。disconnect剩余约119973ms，因此不能用R19-02预算耗尽复现解释该次输出。
- 精确v3.1.2源码中Disconnect在普通Continue/旧exchange之前已有独立closeReplacedRuntime。`closing_exchange_unconfirmed`只说明随后进入旧回执路径，与“独立观察没有得到可接受替代证据后才失败”一致，不能断言旧回执必须先成功。Resume保留原角色；changed-GUID的新runtime只有在同进程两份新鲜递增INPUT证明后才可退役旧CON，不转移未知业务。历史identity/input字段不证明用户未reload，short_read/ineligible_read不证明原环境结束。
- R20-01已离线确认：Mailbox Lookup读取路径父节点后，旧state/globals/namespace/Mailbox/hash-node对象可能退役；最终字符串原本会核已记录guard，早期遍历错误却直接硬失败。5个真实路径切换fixture在旧代码均失败，配对稳定不可读控制仍应保留硬错。最小修复复用同一读取预算/guard的失败出口，只在路径变化被证明且进程身份仍有效时返回path_changed，不增加循环、输入或runtimeEnd权力；进程变化和取消不能被掩盖。正式memory测试已实施，最终整合验证待补。
- R20-02为已确认Skill合同指引缺口：doctor可用性须在首次诊断示例之前按已知installed describe/help选择，旧顶层doctor并不自动等同live doctor；同版本字符串不能证明同commit/协议。保留旧CON和匹配CLI诊断，不以新driver运行不兼容日志。状态中的历史字段、continuation=continue与retryable=false含义已明确，不盲目resume/reload循环。两份references合同21文件/84命令/228引用0违规、目录检查与2项正式测试通过。
- R20-03已确认并实施，由当前原函数离线抽取复现：独立退役预检的已证明path_changed没有沿现有pending合同分类，被包装成硬错误；仅修正无保存proof的观察分支，不吞普通坏读/权限/身份错误。正式Windows边界测试及原生结果待完成，抽取验证不冒充Windows执行。
- 最小补证：仅该CON的jsonl与同名target元数据；失败connect、首次reload后resume、no-cache resume、disconnect的result.scans所指子目录及时间（无指向则先给目录列表）；activation记录只在存在或被引用时补，history/artifact只按原日志引用补。无需业务源码。旧扫描trace不含新Error字段且省略record payload，某些历史freshness/准确失败地址可能不能回溯；未拿到的文件不声称已读。


### R19/R20 本批实现与正式验证

- R19-01七文件删除清单/回归已完成。全局vet发现独立审计最初清单漏掉`tests/protocol/slot_driver_test.go`的直接测试调用；保留这个有价值的当前Lua外来runtime无回执负例，改走真实Connect并断言原envelope/phase/runtime、未bound、无receipt及只发一次输入，不删有效覆盖。
- R19-02复用原budgetContext与next关闭迁移：只在Closing、未bound、无transaction、operation终态/无operation、reload/recovery完成或不存在时允许无输入记账；RuntimeEnd精确退役仍独立，原budget耗尽/回拨证据及caller取消保留。直接旧snapshot六类安全/十八类不安全控制，成功Consumed/Save取消实际边界以及现代未知bind恢复都加入正式测试。七个单guard移除mutation均在独立安全断言失败。
- 修正树全局build/vet通过；channel297事件pass/0fail/0skip，必需Lua跨语言tests/protocol16pass，六项定向channel race×3共150pass。旧310a5d7基线取消前9个边界失败、回收后9个保存失败控制通过；关闭终态旧码8个叶节点失败、18个unsafe控制保持通过。R16-01合成用例不再计入本批验证。
- R20-01正式memory整包通过；12个新叶节点（5路径变化+5稳定不可读+进程变化+取消），旧实现只失败5个变化边界；受影响memory race×3通过。R20-03实际Windows边界新增8叶：raw/wrapped path_changed变pending，short_read/access/process/protocol/parent取消/saved-proof清理错误保持硬错；当前函数的逐字可移植验证8叶通过，旧函数只失败2个gap控制，race×3通过。Windows测试程序交叉编译通过，仍须本批原生执行，未称Linux跑过Windows测试。
- WGC观察汇总复用既有Go事件parser，正式正反例区分完整pass/完整failed、compile错误、skip、截断、冲突、错test/exit及无效证据时的结果保全；fixture的warning/summary隔离，不污染CI观察报告。原SDK失败artifact实读重算仍为failed。workflow结构及该工具13项通过，完整Node工具85pass/6Windows-only skip/0fail（后续仅fixture输出隔离调整已定向复验）。
- R20 Skill两references已完成合同/目录正式检查；不把显示版本号当协议同一性，不把历史identity/input当fresh，不以continue覆盖具体硬错误。

## R21：修正整合树新鲜独立复审

- parent `0f26b9a7`，冻结924个代码/Skill/工具文件，aggregate `aacc1ac475a17f866699af0afe72caf086c3eda4d4586af147715c078e638cb0`。只排除协调者维护的五份状态文档；新审计者对实际产品、正式测试、Skill及WGC观察编排复核，完整Lua必需套件并行运行。
- 当前复审与整套结果尚待完成，不预填无缺陷或最终覆盖。原生Windows、当前skip和覆盖率将在精确新提交上重测；现场根因仍需用户原始诊断材料，七项客户端整合与WGC非游戏验证结果保持空白。


### R21 当前实际结果与新确认项

- 924文件树完整Lua必需Linux套件实际结束：2576事件pass、3个既有平台fail、28skip；30包pass、command1包fail、4无测试。失败仍仅TestLiveConnectArgumentAdmission、TestExecuteContractRejectsIncompleteRequestsBeforeWorkspaceAccess、TestSourceResearchThroughCLI；全局build/vet通过。Node工具85pass/6个Windows-only skip/0fail；版本3.1.2、Skill21文件/84命令/228引用、生成目录、import及本地文档引用通过。
- R21独立受影响证据/vault/channel/memory/Lua真实负例共81事件pass，WGC/workflow13项pass；未重复整套。**发现R21-01：已无输入/交易需要的quiescent关闭仍先做optional runtime替换预检，若返回已证明path_changed，公共wrapper提前pending，阻止R19允许的纯终态记账。** 不变runtime/no-proof对照能关闭，公开Resume/Disconnect均用该wrapper；逐字抽取Windows函数离线复现，不冒称Windows原生已执行。
- 最小修正范围为该wrapper及正式Windows回归：无saved RuntimeEnd且已有closeBookkeepingOnly证据时先复用Continue；不绕过Native/身份/lease准入，不跳过有proof的精确退役，不放行仍bound/transaction/unfinished operation/reload/recovery，不延长任何预算。修正/后续新鲜R22复核及原生验证待补，不能把本轮标为无发现。


### R21-01 实施完成，进入R22

- 仅两个Windows文件变化：`runtime_retirement_windows.go`在无RuntimeEnd且原closeBookkeepingOnly为真时先调用既有Continue；其余预检/退役路径不变。`runtime_retirement_windows_test.go`加入11个正式叶节点：3个终态/已核实报告/unknown结果正例、5个active-work负例、saved-proof正确路由、wrong-process proof拒绝和caller取消。共用原证据比较helper，不重复低层200ms计时测试。
- 精确R20旧freeze仅3个应完成终态场景失败，8个控制通过；新11例及实际R19到期→持久化→Load→wrapper链均通过，相关逐字可移植验证race×3通过，实际Windows测试程序交叉编译通过。尚不是原生通过；没有新增输入、claim绕过或预算更新。
- R22全新审计者在924文件新freeze aggregate `dcae36da3ad5bc001f5c4ac91bd50b5e098b9784ce91a5247787aa921f30aaeb`上复核；与R21只差上述2文件，剩余922文件SHA保持。完整Lua必需Go重跑已启动，复审与原生结果待回填。


## R22：最终恢复组合独立复核

- 冻结924文件 aggregate `dcae36da3ad5bc001f5c4ac91bd50b5e098b9784ce91a5247787aa921f30aaeb`；与R21仅两个Windows retirement文件不同。完整Lua必需Linux命令 `go test -p=2 -parallel=4 -count=1 -timeout=20m ./...` 已实际结束：2576 test/subtest pass、3既有平台fail、28skip，30包pass、command1包fail、4无测试；build/vet通过，起终924文件SHA无变化。三个失败仍仅LiveConnectArgumentAdmission、ExecuteContractRejectsIncompleteRequestsBeforeWorkspaceAccess及SourceResearchThroughCLI，不冒称Linux全套通过。
- 新鲜审计者已逐字抽取当前Windows retirement函数与完整正式retirement/closing-reload回归，并验证真正的到期→保存中断→Load→公共wrapper链；114事件pass/0fail/0skip。精确旧R21实现仍只在真实链及3个终态正例失败，8个排除/proof/取消控制通过。Windows原生执行待推送后的精确head CI，不以抽取或交叉编译代替。
- R22最终独立复核无新增可操作离线缺陷。消费者114事件、race 69事件、相邻Go/Lua 31事件及Node 13项全部通过；924文件和aggregate起终一致，无产品写入。覆盖真实消费者、saved proof/身份/取消/原期限、Skill兼容指引和WGC证据边界。此结论限冻结树的离线可确认范围；Windows原生、当前覆盖/skip待精确提交CI，七项客户端与WGC单列实测结果仍空白。


### R22 交付与精确提交验证

- 本批25文件已提交并推送 `a60e294f4d1fb8bba680827f3a8e32d8d16d06d9`，tree `7b10efc4bcd477ac6388792dbd10aba49c8aabdb`；远端CAS receipt、fetch与本地index tree一致。PR13保持draft，版本与发行渠道未变。
- [精确head Windows CI38562152610](https://github.com/Follen/Lychee-Dev/actions/runs/38562152610)现已完成，五项必需作业与ci-required全部success。独立WGC观察作业已完成，但底层capture仍为**failed/exit1**；[artifact11892222252](https://github.com/Follen/Lychee-Dev/actions/runs/38562152610/artifacts/11892222252)下载SHA256 `0376036f2109860287b4549306cf382412481a498203bf58db790ffeeab3fbbf`核对一致，result commit等于a60e294f。没有把非门禁作业success当作WGC通过，七客户端及后续交互Windows复验结果仍空白。
- Windows/amd64 Go1.27.1完整Lua必需主套件实际2893 test/subtest pass、2628叶节点pass、265容器pass、0fail、20skip；当前R16/R19/R20/R21正式存储/恢复/读取测试全部实际运行。16项固定公开集成及其子测试全通过；race、跨进程、addon与package通过。
- [覆盖artifact11892198497](https://github.com/Follen/Lychee-Dev/actions/runs/38562152610/artifacts/11892198497)下载SHA256 `166cfca49b34fc3747d33a4e191ffab04e400c23ad857fd2ddad59a1f89e962b`核对一致，testedCommit/tree精确等于a60e294f/7b10efc4。原profile/events输入hash一致，独立按位置OR合并711916个重复block行得到22965唯一block，**24753/32099语句=77.1146%**。atomic跨包cmd/internal（排除testkit），./...测试选择，原命令和身份在artifact；不包含Lua/Node语句、不自动instrument CLI子进程、不代表业务或游戏覆盖。
- skip仍20：9个helper入口由父测试执行；11个真可选项分别3desktop、1客户端read-only、4安装/index/图像原材料、2原runtime replay、1宿主8.3能力。WGC主套件skip与独立观察failed分别保留；没有把未跑写成通过。当前名称与原因如下。


| 当前skip测试 | 实际原因/独立覆盖 |
| --- | --- |
| TestInstallationWorker、TestUpgradeWorker、TestAdmissionCrashHelper、TestWindowOwnerProcessHelper、TestWindowRunProcessHelper | 子进程helper入口；对应父回归实际执行 |
| TestNativeModuleProtectionChild、TestNativeModuleMappedChild、TestNativeMemoryChild、TestSourceBrokerGateHelperProcess | 子进程入口；对应父回归实际执行 |
| TestCaptureItemForExplicitProcess、TestCaptureFramesForExplicitProcess | 未提供明确授权的进程ID，不能选择用户/游戏进程来消skip |
| TestWGCLifetimeBetweenSessions | 主套件不启用交互桌面；已在独立自建窗口观察实际执行，capture failed/exit1，不计主profile通过 |
| TestClientWindowLiveReadOnly | 缺明确live只读目标 |
| TestInstalledAssetCommand、TestInstalledBuildMetadata | 缺指定安装/product/build真实资料 |
| TestInstalledIndexReadOnly | 缺指定原local-index样本 |
| TestImageExportRealSample | 缺指定icon.blp/png/webp原样本 |
| TestLuaMailboxRootRecipeOriginalRuntimeDump | 缺原runtime dump manifest |
| TestLuaMailboxRootRecipeCapturedRuntimeHeader | 缺capture header、原dump manifest及hash匹配exe |
| TestInstallationLeaseShortPathSpelling | 当前卷未启用8.3短文件名；未为测试修改宿主策略 |

本批关闭可离线循环：R22独立复核无新增可操作问题，当前原生验证通过。R20现场根因仍未确认，七项客户端整合与WGC特定Windows复验均保留所需环境/步骤/预期，未执行的实际结果留空；不要求立即开游戏。
