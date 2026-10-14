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

- 证据：[hotfix_scan.go:140](../internal/records/hotfix_scan.go#L140)、[archive.go:323–343](../internal/evidence/archive.go#L323)、[公开删除入口 entry.go:773](../internal/command/entry.go#L773)。
- 触发：Hotfix scan 分页停止并保存 pending checkpoint 后，执行 `evidence remove` 删除该 checkpoint 的原始 Source capture，再按原 snapshot/from/resume 继续。
- 影响：checkpoint 的元数据把来源保存为正式 locator `capture:CAP-…`，删除检查只搜索带引号的裸 `CAP-…`，没有识别这条依赖。删除返回成功，原 resume 随后因来源记录缺失失败。
- 离线复现：三条 XFTH 记录、limit=1、maxPages=1，首次 pending 且 returned=1；删除原 Source 返回 manifestRemoved=true、blobRemoved=true；沿原 checkpoint 继续得到 `vault.record_missing`。
- 改进与回归：让 checkpoint 依赖以删除检查可识别的正式引用表达，并处理现有 capture locator；回归 pending scan 来源删除应拒绝、原 resume 可继续。本例是串行、已持久化引用，区别于 SOL-01 已修复的并发发布/删除竞争。

#### R3-04 · 中 · Windows Skill 的 npm 全局 prefix 查找无法执行

- **本轮状态：代码已修复；Windows 原生回归待本次 CI，Linux 未计作通过。**
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

- **本轮状态：代码已修复；正式 Windows 磁盘回归待本次 CI；此缺陷不依赖游戏验证。**
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

- **本轮状态：代码已修复；Linux 公共 Execute、真实继承管道、取消/期限/EOF/复用回归通过；Windows 路径待本次 CI。**
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

- **R4-C01 · 候选：doctor 默认归档与游戏位于不同卷。** [默认 archive](../internal/command/live_doctor_windows.go#L114) 取项目目录，游戏可以位于另一卷；当前升级会经过 [outsideDiscovery 的 filepath.Rel](../internal/delivery/upgrade.go#L225) 与 [MoveFileEx 目录发布](../internal/delivery/publish_windows.go#L14)。本轮复核合同只要求 archive 位于 discovery 外；Linux 环境未执行两个 Windows 卷上的合法部署 fixture，尚未核实完整失败轨迹与支持合同，保留候选、不添加 unsupported 限制，也不与 R4-03 的父目录缺失合并。
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
- 当前代码的本地整套检查及本次提交的 Windows CI 状态见 [handover.md](../handover.md)。历史主批次/审计结果不代替本轮验证，交叉编译和跳过不计作原生运行通过。
- R3-01、R3-06、R4-01、R4-04、R4-07 保留“待实机验证”；上方各条目已列前置条件、最小步骤、预期，实机结果留空。本轮没有连接游戏、发送按键、部署游戏目录或操作用户电脑。
- 只提交推送同一草稿 PR #13；不合并、不发布、不更改发行版本或 npm 标签，仍为 3.1.2 / next / latest=2.5.1。
