# 当前交接：fix-3.2.0

更新日期：2026-10-14，Asia/Shanghai。只保留当前状态；协作规则见 [AGENTS.md](AGENTS.md#multi-agent-handover)，完整问题证据见 [审计报告](docs/Clean-repo-2026-10-14.md)。

## 分支、授权与状态

- [PR #13](https://github.com/Follen/Lychee-Dev/pull/13)，`codex/fix-3.2.0` → `main`，保持草稿。已推送首批产品修复 `81ccdd5` 和文档 `f7f2239`；R5 `2660a4ef` Windows CI 全部通过；R6 `9a02522f` 六个 CI 作业通过；R8 已推送 `7566493c`（tree `e827851d`），[当前 Windows CI](https://github.com/Follen/Lychee-Dev/actions/runs/38544503854) contract及双真实卷修复验证已通过，阶段Go语句覆盖73.9475%，六作业均success；R7 曾因 R4-C01 双卷探针失败，保留原始失败证据。
- R3/R4 的 13 项及 R5 的 5 项已交付；R6 又确认并实施 4 项新事项。R4-01 crash 分支和 D-06 清理残留沿用原 ID，不重复计数。前一批 42 项保留为基线，候选不当作确认问题。
- 追加任务：至少 R5/R6/R7 三轮，各由不同的新审计者检查架构、bug、死代码、Skill 编排及有明确收益、可验证的架构优化；持续审计→修复→验证→推送，直到独立复审不再发现可离线修复项。仅实机的新问题记录复验，不代替用户操作。
- 本任务允许提交、推送同一分支及更新 PR，阶段性进展同时汇报。未连接用户电脑/游戏、未发键或部署游戏目录；不合并、不发布。只使用仓库 handover.md，不使用 handoff Skill。
- 发行源仍 `3.1.2`、渠道 `next`、`latest=2.5.1`；`3.2.0` 仅为开发基线。

## 当前实施与文件边界

| 项目 | 当前结果 | 写入负责人 / 边界 |
| --- | --- | --- |
| R9-01 | SQL CSV capture 声明 query 依赖，同删除 lease 核验；正式定向通过、旧代码负对照失败 | 协调者：records/sql_export.go 与 navigatetest 正式回归 |
| R6-02 残留 | Context 摘要复用现有有界行索引；200片段正式回归及旧代码负对照完成 | 协调者：codebase/context、document_lines 与正式测试 |
| D-03/D-07 残留 | 退役 SavedVariables 读取孤岛及8个无消费者接口已删除；完整受影响验证中 | 协调者：bridge/live/desktop/records/selection/vault 精确删除集 |
| R9-02 | Skill 实际试用复现 cold pin diff 缺索引准备，已复用 EnsureIndex，公开CLI/负对照/race通过 | Skill 试用组仅 comparison.go 与新正式 command 回归 |
| PR12 必要移植 | 仅移植冷读预算与正式回归；槽位旧修复与现行legacy兼容合同冲突，不移植 | 协调者：definitions两文件；新增六组CDN/远端测试锁定现有独立beta/legacy身份 |
| 独立审计 / 文档 | R9与99次CLI独立Skill试用完成；R10已纠正非必要槽位移植；898文件修正树整测2506通过/3既有Linux失败/28跳过，R10已完成且该范围无其他可操作离线修复 | 审计者只读；协调者独占审计、handover、todo、Changelog |

R8 的普通 Hotfix/资产依赖、Bundle取消、R4-C01宿主修复及两处冗余输入清理均已随 `7566493c` 推送。当前CI contract/addon/package/process已通过，2747 pass/0 fail/35 skip，race及汇总也已通过；R9尚未发布，旧绿色结果不替代当前验证。正式测试不依赖临时审计 fixture。

## 实际验证与限制

- 工具链：官方 SHA 校验 Go 1.27.1、Node 24.18.1、npm 11.18.0、固定源码构建 Lua 5.1.5；`LYCHEEDEV_REQUIRE_LUA51=1` 与明确解释器路径。LuaLS 3.19.1 Windows archive bytes/SHA 已校验，不等于原生进程运行。
- R7 build/vet、records/evidence/vault 12 包、依赖/scan 与 Hotfix CLI race 各 ×3、addon 34 suite × 六 profile = 204 组通过。旧实现的 workspace 覆盖和来源/页面删除负对照失败，新正式回归通过。
- R8 与两处冗余输入清理后的完整 Lua 必需命令 `go test -p=2 -parallel=4 -count=1 -timeout=20m ./...` 实际执行：30 测试包通过、command 1 包仅 3 个既有 Linux 平台失败、4 包无测试；2484 test/subtest pass、3 fail、28 skip。三个失败是 Native Live 参数检查两项和非 Windows LuaLS broker 的 Source prune，基线已复现；不称 Linux 全套通过。
- R6 已完成 [`9a02522f` 的 Windows CI](https://github.com/Follen/Lychee-Dev/actions/runs/38536137573) 六作业全部 success：Lua 必需完整 Go、race、process、addon、真实 tgz/隔离安装、汇总。R7 `f84cf0f4` contract 因 R4-C01 双卷探针失败，同卷通过；process/addon/package 通过；race 同样仅因 R4-C01 失败，无 detector 报告。R8 `7566493c` contract已通过，真实D/C双卷默认/显式archive及同卷对照全通过，PID=0。
- R8阶段覆盖：Windows/amd64 Go1.27.1、Lua必需，2747 pass/0 fail/35 skip；23976/32423 Go产品语句=73.9475%。2747为事件pass、2503为叶节点pass，均非业务场景数。35skip中8helper有父测试通过，27真未跑，其中16项公开固定输入已安排补齐。实际merge checkout `d5f0d7b` 的tree `e827851d` 与PR head相同；[原始profile/events/identity/summary](https://github.com/Follen/Lychee-Dev/actions/runs/38544503854/artifacts/11883583264)。不同宿主、可选输入和helper入口skip不计通过，不代表业务/Lua/Node/游戏百分比。
- R9修正树build/vet、19个受影响包及定向race×3通过；完整Lua必需套件2506 pass/3既有Linux fail/28 skip，30包通过、command1包失败、4包无测试。新Windows CI待本批推送。
- R8 Node tools 全套76通过/6个Windows专属skip、0失败；覆盖率汇总10项通过，真实历史JSON兼容性解析无误。import 合同、文档本地引用、diff 检查通过。版本/Skill/命令目录输入未变。R5/R6 的计量、旧代码负对照与额外容量 reload 计时 fixture 修正详见审计，不复制运行历史。
- LuaLS/broker/固定API/DBD/CascLib共16个skip可准备公开固定输入，下一独立CI批次实施；WGC优先尝试CI自建窗口。安装图像/原runtime等真实资料及客户端条件仍单列，不都归为游戏限制。Windows 专属测试、交叉编译、缺环境 skip 与游戏验证分别记录，均不冒充当前通过。

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
| LuaLS/broker/打包 Source 可选覆盖缺口 | Windows 固定可运行LuaLS、匹配release/CLI和docs；运行正式real integration，预期引用/关闭/取消/隔离及安装语义断言通过 | |
| 固定 API/DB2/CASC/图像及原 runtime replay 缺材料 | 取得匹配hash/build的固定资料，运行各正式oracle/replay测试；预期身份、原始字节、解码和根证据吻合，不把合成fixture当原始输入 | |
| Live read-only 可选覆盖缺口 | 后续授权的Windows/WoW固定目标；只读测试核对身份/窗口，不扩大输入许可 | |

## 下一步

1. 跟进 `7566493c` 原生 CI，优先报告这个精确提交的阶段 Go 语句覆盖率与跨盘结果，不因 R9 新发现无限推迟计量。
2. 推送已验证R9修复/PR12必要预算移植与当前文档；R10已完成。并行补齐coverage专项确认可公开准备的真实LuaLS/固定API/DBD/CascLib输入，下一批原生重测，不等用户游戏。
3. 冗余专项已清理两个重复输入；R9额外删除只服务退役解析器的测试，不删除仍有独立边界/故障/跨进程/平台价值的覆盖。阶段值73.9475%仅对应R8，最终修复后重测；业务场景单列。
4. 暂存材料只在忽略的 `.tmp/pr13/`，任务结束后按已核实绝对路径清理本轮产物；不触碰用户项目、未解决恢复记录或保留的 Rust worktree。
