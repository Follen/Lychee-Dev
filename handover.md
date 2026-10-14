# 当前交接：fix-3.2.0

更新日期：2026-10-14，Asia/Shanghai。只保留当前状态；协作规则见 [AGENTS.md](AGENTS.md#multi-agent-handover)，完整问题证据见 [审计报告](docs/Clean-repo-2026-10-14.md)。

## 分支、授权与状态

- [PR #13](https://github.com/Follen/Lychee-Dev/pull/13)，`codex/fix-3.2.0` → `main`，保持草稿。最新已推送R12测试修正/诊断 `787eff3b`（tree `f58ea9ca`），[该head原生CI](https://github.com/Follen/Lychee-Dev/actions/runs/38551956825)完整Go及16项集成通过，77.0095%阶段语句覆盖；仅独立WGC probe/汇总仍失败，R14继续定位。此前CI输入批次 `ebde90ed`（tree `fee83999`）的[本批原生CI](https://github.com/Follen/Lychee-Dev/actions/runs/38549548276)的contract失败：broker两子项编排冲突、独立WGC probe失败；其余四作业通过，R12测试编排修正及WGC诊断已推送，整套Linux2506pass/3既有平台fail/28skip，build/vet通过。产品修复R9 `ef9b0113`（tree `b05c5ce7`）的[该head Windows CI](https://github.com/Follen/Lychee-Dev/actions/runs/38547831499)六作业全部通过，阶段Go覆盖74.2544%。此前R8 `7566493c`六作业全通过，阶段Go覆盖73.9475%，不能外推为新head结果。
- [PR #12](https://github.com/Follen/Lychee-Dev/pull/12)已关闭、未合并；必要Definitions预算修复及正式回归在`ef9b0113`。旧地区槽位移植违反现行legacy合同，未移植，不是整批cherry-pick。
- R3/R4 的 13 项及 R5 的 5 项已交付；R6 又确认并实施 4 项新事项。R4-01 crash 分支和 D-06 清理残留沿用原 ID，不重复计数。前一批 42 项保留为基线，候选不当作确认问题。
- 追加任务：至少 R5/R6/R7 三轮，各由不同的新审计者检查架构、bug、死代码、Skill 编排及有明确收益、可验证的架构优化；持续审计→修复→验证→推送，直到独立复审不再发现可离线修复项。仅实机的新问题记录复验，不代替用户操作。
- 本任务允许提交、推送同一分支及更新 PR，阶段性进展同时汇报。未连接用户电脑/游戏、未发键或部署游戏目录；不合并、不发布。只使用仓库 handover.md，不使用 handoff Skill。
- 发行源仍 `3.1.2`、渠道 `next`、`latest=2.5.1`；`3.2.0` 仅为开发基线。

## 当前实施与文件边界

| 项目 | 当前结果 | 写入负责人 / 边界 |
| --- | --- | --- |
| R9-01 | SQL CSV capture 声明 query 依赖，同删除 lease 核验；正式定向通过、旧代码负对照失败 | 协调者：records/sql_export.go 与 navigatetest 正式回归 |
| R6-02 残留 | Context 摘要复用现有有界行索引；200片段正式回归及旧代码负对照完成 | 协调者：codebase/context、document_lines 与正式测试 |
| D-03/D-07 残留 | 退役 SavedVariables 读取孤岛及8个无消费者接口已删除；完整受影响包和该head原生CI通过 | 协调者：bridge/live/desktop/records/selection/vault 精确删除集 |
| R9-02 | Skill 实际试用复现 cold pin diff 缺索引准备，已复用 EnsureIndex，公开CLI/负对照/race通过 | Skill 试用组仅 comparison.go 与新正式 command 回归 |
| PR12 必要移植 | 仅移植冷读预算与正式回归；槽位旧修复与现行legacy兼容合同冲突，不移植 | 协调者：definitions两文件；新增六组CDN/远端测试锁定现有独立beta/legacy身份 |
| 独立审计 / 文档 | R9与99次CLI独立Skill试用完成；R10已纠正非必要槽位移植；898文件修正树整测2506通过/3既有Linux失败/28跳过，R10已完成且该范围无其他可操作离线修复 | 审计者只读；协调者独占审计、handover、todo、Changelog |

R8 的普通 Hotfix/资产依赖、Bundle取消、R4-C01宿主修复及两处冗余输入清理均已随 `7566493c` 推送。当前CI contract/addon/package/process已通过，2747 pass/0 fail/35 skip，race及汇总也已通过；R9已发布并获该head原生六作业通过，旧绿色结果不替代当前验证。正式测试不依赖临时审计 fixture。

## 实际验证与限制

- 工具链：官方 SHA 校验 Go 1.27.1、Node 24.18.1、npm 11.18.0、固定源码构建 Lua 5.1.5；`LYCHEEDEV_REQUIRE_LUA51=1` 与明确解释器路径。LuaLS 3.19.1 Windows archive bytes/SHA 已校验，不等于原生进程运行。
- R7 build/vet、records/evidence/vault 12 包、依赖/scan 与 Hotfix CLI race 各 ×3、addon 34 suite × 六 profile = 204 组通过。旧实现的 workspace 覆盖和来源/页面删除负对照失败，新正式回归通过。
- R8 与两处冗余输入清理后的完整 Lua 必需命令 `go test -p=2 -parallel=4 -count=1 -timeout=20m ./...` 实际执行：30 测试包通过、command 1 包仅 3 个既有 Linux 平台失败、4 包无测试；2484 test/subtest pass、3 fail、28 skip。三个失败是 Native Live 参数检查两项和非 Windows LuaLS broker 的 Source prune，基线已复现；不称 Linux 全套通过。
- R6 已完成 [`9a02522f` 的 Windows CI](https://github.com/Follen/Lychee-Dev/actions/runs/38536137573) 六作业全部 success：Lua 必需完整 Go、race、process、addon、真实 tgz/隔离安装、汇总。R7 `f84cf0f4` contract 因 R4-C01 双卷探针失败，同卷通过；process/addon/package 通过；race 同样仅因 R4-C01 失败，无 detector 报告。R8 `7566493c` contract已通过，真实D/C双卷默认/显式archive及同卷对照全通过，PID=0。
- R8阶段覆盖：Windows/amd64 Go1.27.1、Lua必需，2747 pass/0 fail/35 skip；23976/32423 Go产品语句=73.9475%。2747为事件pass、2503为叶节点pass，均非业务场景数。35skip中8helper有父测试通过，27真未跑，其中16项公开固定输入已安排补齐。实际merge checkout `d5f0d7b` 的tree `e827851d` 与PR head相同；[原始profile/events/identity/summary](https://github.com/Follen/Lychee-Dev/actions/runs/38544503854/artifacts/11883583264)。不同宿主、可选输入和helper入口skip不计通过，不代表业务/Lua/Node/游戏百分比。
- R9修正树build/vet、19个受影响包及定向race×3通过；完整Lua必需套件2506 pass/3既有Linux fail/28 skip，30包通过、command1包失败、4包无测试。Windows CI38547831499六作业全部通过，2769事件pass/0fail/35skip，23852/32122语句=74.2544%；35skip名称与R8相同，16项输入补齐批次尚未执行原生。
- R8 Node tools 全套76通过/6个Windows专属skip、0失败；覆盖率汇总10项通过，真实历史JSON兼容性解析无误。import 合同、文档本地引用、diff 检查通过。版本/Skill/命令目录输入未变。R5/R6 的计量、旧代码负对照与额外容量 reload 计时 fixture 修正详见审计，不复制运行历史。
- LuaLS/broker/固定API/DBD/CascLib共16个旧skip已在ebde90ed原生进入执行，除broker关闭的测试并行冲突外均通过；完整运行仍失败。WGC自建窗口CreateForWindow失败，原因待R12宿主/fixture诊断。安装图像/原runtime等真实资料及客户端条件仍单列，不都归为游戏限制。Windows 专属测试、交叉编译、缺环境 skip 与游戏验证分别记录，均不冒充当前通过。

## 待实机验证

代码修复、离线通过和实机验证分别记账。下列实际结果留空，只供后续明确授权的本机任务；各项详细复验见审计条目。共同前置为匹配版本的干净受管 CLI/addon、固定 installation/PID/创建时间/HWND、产品/构建与原 project/CON/request/期限，不重放 submitted/uncertain 输入。

| ID | 最小复验与预期 | 实机结果 |
| --- | --- | --- |
| R3-01 | Trace `GetTime.foo`/标量路径及正常方法；前者页内错误，后者正常追踪 | |
| R3-06 | 主窗口隐藏时开拾取后进入战斗；dock/键盘脚本立即关闭，不吞 F/Enter/Escape | |
| R4-01 | 无副作用请求跨 prepare/commit/confirm/release、原 CON resume；已知输入及恢复上界后采样间隔至少 100ms，无重放，reload 新 runtime 独立 | |
| R4-04 | 新配置 status、off/on、保存 off 后 reload；状态与 INPUT 生命周期一致 | |
| R4-07 | Objects 检查失败 tostring/普通错误及健康对象；无二次错误、失败明确、部分流/limited 保留 | |
| R5-01 | 只返回高精度有限数值和 `%.17g` 对照文本；报告数值保持一致，正常清理并断开 | |

七产品旧矩阵见 [todo.md](todo.md)，未扩展为本轮通过。

## 候选与特定环境复验

未确认风险不是已确认 bug，缺环境 skip 也不是通过。详细步骤见审计文末；均留给后续具备环境的任务，不要求现在执行。

| 项目 / 当前状态 | 所需环境、最小步骤与预期 | 实际结果 |
| --- | --- | --- |
| R3-C01 候选 | 固定真实 SpellMisc DB2/DBD/DifficultyID 多匹配资料；比较全表、导航和领域合同，证据冲突才升级 | |
| R3-C03 候选 | 先明确 instances 项目归属语义；用隔离 Windows owner fixture 对照展示与实际连接消费者，不能把被动字段当阻断 | |
| R4-C01 已确认，代码已修复 | Windows 双真实卷、隔离 release/client/project；同卷对照后跨卷跑默认 archive，PID=0；预期磁盘升级/旧树归档后在安全激活边界停止 | R7 CI 同卷通过；跨卷 Rel 失败，旧树完整、无 archive；R8双真实卷默认/显式及同卷对照通过 |
| LuaLS/broker/打包 Source 公开输入 | Windows固定LuaLS、匹配release/CLI和docs；运行正式集成，预期引用/关闭/取消/隔离及安装语义通过 | 787eff3b全部16项required公开集成通过，含broker关闭两子项；R12-01原生已验证 |
| 真实安装/index/图像及原runtime replay缺材料 | 取得匹配hash/build的获准原始资料后运行正式replay；预期身份、字节、解码和根证据吻合，不把合成fixture当原始输入。公开API/DBD/CascLib已在ebde90ed实际通过，不再列缺材料 | |
| WGC自建窗口失败，原因候选R12-C01 | Windows runner；记录session/桌面/WGC能力、原窗口及标准顶层窗口对照；预期在支持环境完成10次采帧/释放且无delegate残留 | ebde90ed原窗口CreateForWindow返回0x80070057；尚不能定性宿主或产品 |
| Live read-only 可选覆盖缺口 | 后续授权的Windows/WoW固定目标；只读测试核对身份/窗口，不扩大输入许可 | |

## 下一步

1. `ef9b0113`原生六CI通过、74.2544%阶段覆盖率已交付；下一批16项公开输入/WGC宿主验证后重测，不将阶段值当最终业务覆盖。
2. R12测试lease原生已通过，R13/R14在相应冻结范围无已确认离线产品缺陷。当前R15 SDK同进程对照906文件冻结、独立复审已完成；R16高风险回归隔离发现bootstrap回收失败恢复缺陷R16-01，修正中；R17-01用户实际Source外搜/猜raw案例由Skill专组只写Skill目录。三批不混淆验证，协调者独占共享文档。协调者负责workflow/tools/文档及原生重跑；WGC宿主已证实可见交互桌面且IsSupported=true，两自建窗口仍CreateForWindow失败，R14继续针对native调用核实；尚无具体定性。R9/必要PR12移植与文档已推送，R10已完成且本轮范围无其他可操作离线修复。当前独立CI准备组负责workflow及tools验证helper，16项公开LuaLS/API/DBD/CascLib输入准备已完成并冻结，固定文档/DBD本地通过；全新R11已完成，901文件起终SHA吻合，该检查范围无其他可操作离线修复；ebde90ed原生已执行，16项旧skip均进入测试；broker编排冲突与WGC自建窗口失败转R12，未把失败运行75.9013%当最终结果；协调者独占文档。按新head重测，不等用户游戏。
3. 冗余专项已清理两个重复输入；R9额外删除只服务退役解析器的测试，不删除仍有独立边界/故障/跨进程/平台价值的覆盖。阶段值73.9475%仅对应R8，最终修复后重测；业务场景单列。
4. 暂存材料只在忽略的 `.tmp/pr13/`，任务结束后按已核实绝对路径清理本轮产物；不触碰用户项目、未解决恢复记录或保留的 Rust worktree。
