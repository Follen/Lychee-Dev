# 当前交接：fix-3.2.0

更新日期：2026-10-14，Asia/Shanghai。只保留当前状态；协作规则见 [AGENTS.md](AGENTS.md#multi-agent-handover)，完整问题证据见 [审计报告](docs/Clean-repo-2026-10-14.md)。

## 分支、授权与状态

- [PR #13](https://github.com/Follen/Lychee-Dev/pull/13)，`codex/fix-3.2.0` → `main`，保持草稿。已推送首批产品修复 `81ccdd5` 和文档 `f7f2239`；R5 修复已推送 `2660a4ef`，Linux 整合验证已结束，[该 head Windows CI](https://github.com/Follen/Lychee-Dev/actions/runs/38532230482) 六作业全部通过。
- R3/R4 的 13 项及 R5 的 5 项已交付；R6 又确认并实施 4 项新事项。R4-01 crash 分支和 D-06 清理残留沿用原 ID，不重复计数。前一批 42 项保留为基线，候选不当作确认问题。
- 追加任务：至少 R5/R6/R7 三轮，各由不同的新审计者检查架构、bug、死代码、Skill 编排及有明确收益、可验证的架构优化；持续审计→修复→验证→推送，直到独立复审不再发现可离线修复项。仅实机的新问题记录复验，不代替用户操作。
- 本任务允许提交、推送同一分支及更新 PR，阶段性进展同时汇报。未连接用户电脑/游戏、未发键或部署游戏目录；不合并、不发布。只使用仓库 handover.md，不使用 handoff Skill。
- 发行源仍 `3.1.2`、渠道 `next`、`latest=2.5.1`；`3.2.0` 仅为开发基线。

## 当前实施与文件边界

| 项目 | 当前结果 | 写入负责人 / 边界 |
| --- | --- | --- |
| R6-01 / R6-02 | Git 正文搜索无歧义读取 Unicode/引号/tab 文件名；Search/Inspect 复用已有有预算的行索引 | Source 组，已冻结；body_search/search/document_lines/inspect 与两份正式测试 |
| R6-03 | Hotfix CSV 和 SQL 共用现有目标 lease/完整 stage 发布，CLI 不再自行截断两个文件 | 协调者，已冻结；command dispatch、records CSV 边界与公开 CLI/records 正式回归 |
| R6-04 | 删除 3 个零消费者 Lua 包装函数，真实 Diagnostics/Objects 路径不变 | 协调者，已冻结；Diagnostics/ObjectInspector/Object 页面 |
| R6 独立审计 / 文档 | 新审计者完成 4 项确认，未证明新的实机专属缺陷；协调者独占共享文档 | 审计者只读；审计、handover、todo、Changelog 由协调者维护 |
| 已交付基线 | R3/R4 13 项及 R5-01 至 R5-05、R4-01/D-06 残留均已修复 | 逐项证据见审计；`2660a4ef` 当前 Windows CI 已全部通过 |

R6 产品/测试及额外计时 fixture修正已冻结；最终整合验证已完成，本次提交包含全部修复，新提交的 Windows CI 待运行。R7 尚未启动，将使用不同的新审计者在 R6 修复后代码上复审。正式测试不依赖临时审计 fixture。

## 实际验证与限制

- 工具链：官方 SHA 校验的 Go 1.27.1、Node 24.18.1、npm 11.18.0、固定源码构建 Lua 5.1.5；`LYCHEEDEV_REQUIRE_LUA51=1` 和明确 `LYCHEEDEV_LUA51`。LuaLS 3.19.1 Windows archive bytes/SHA 已校验。
- R6 build/vet 批次通过；codebase 全包 136 test/subtest pass、2 个真实 LuaLS/broker 可选缺环境 skip；Source 定向 race ×3 通过。公开 200 结果 Search fixture 分配从旧 302,369,464 降为 2,810,488 字节，只有一次行索引构建，内容/hash/分页一致；旧实现负对照失败。
- R6 Hotfix 公开 CLI 跨进程/不同 home/规范化及 symlink 别名、等待 deadline 与最终 CSV/manifest 一致性通过；records/evidence/vault 12 包通过；Hotfix CLI、records/SQL 导出 race 各重复三次通过。旧 command 实现负对照失败，既有隐式覆盖/schema/原编码字节保留。
- R6 addon 全包通过，34 suite × 六 profile = 204 组，无 Lua 跳过。Skill 84 命令/220 引用/0 违规、生成目录检查通过；本批没有改 Skill/命令目录或版本输入。
- R6 最终整合 build/vet 通过；完整 Lua 必需 `go test -p=2 -parallel=4 -count=1 -timeout=20m ./...` 实际执行（JSON 留证）：30 测试包通过、command 1 包仅 3 个既有 Linux 平台失败、4 包无测试；2439 test/subtest pass、3 fail、28 skip。没有新增失败，仍不称 Linux 全套通过。
- 首轮整合曾额外失败于容量 reload 测试，诊断证明350ms 计时完成条件不可靠，原 ID/期限不变；仅修正正式测试为实际 prepare 边界终止，并核对持久Load状态。旧HEAD负对照3/30失败，新测试30/30、400ms 延迟对照、channel/journal及race×3通过；修正后已重新执行上述完整套件。
- 最近已完成原生 Windows 验证为 [`2660a4ef` 的 CI](https://github.com/Follen/Lychee-Dev/actions/runs/38532230482)：contract（Lua 必需完整指定 Go，31 测试包）、race、process、addon、真实 tgz/隔离安装、aggregate 六作业全部 success；不替代尚未提交的 R6 代码。
- R5 Linux 完整套件实测 30 测试包通过、command 1 包失败、4 包无测试；2420 test/subtest pass、3 fail、28 skip。3 个失败已在接手基线复现：Native Live 参数检查两项、非 Windows LuaLS broker 的 Source prune；不抑制、不称全套通过。skip 含 helper/外部资料/真实 runtime 等，均不计通过。
- Windows 交叉编译只计编译。真实 LuaLS/broker/安装包语义、外部 API/CASC oracle/图像样本、显式游戏安装等可选集成在此 Linux 环境仍缺输入；旧结果/跳过不冒充当前通过。R4-03 是 Windows 磁盘 fixture，不属于游戏环境限制。

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
| R4-C01 候选 | Windows 双真实卷、隔离 release/client/project；同卷对照后跨卷跑默认archive磁盘升级，停在PID=0，不需游戏；按实际Rel/MoveFileEx/恢复轨迹和支持合同定性 | |
| LuaLS/broker/打包 Source 可选覆盖缺口 | Windows 固定可运行LuaLS、匹配release/CLI和docs；运行正式real integration，预期引用/关闭/取消/隔离及安装语义断言通过 | |
| 固定 API/DB2/CASC/图像及原 runtime replay 缺材料 | 取得匹配hash/build的固定资料，运行各正式oracle/replay测试；预期身份、原始字节、解码和根证据吻合，不把合成fixture当原始输入 | |
| Live read-only 可选覆盖缺口 | 后续授权的Windows/WoW固定目标；只读测试核对身份/窗口，不扩大输入许可 | |

## 下一步

1. 本批 R6 整合已完成，提交推送同一草稿 PR 后核验新 head Windows CI；失败则继续修复。
2. R6 独立审计已完成；下一轮在其修复后代码上启动另一位新 R7 审计者；如仍有可离线项继续循环。协调者独占更新审计、todo、handover；每轮不追加流水账到本文件。
3. 暂存材料只在忽略的 `.tmp/pr13/`，任务结束后按已核实绝对路径清理本轮产物；不触碰用户项目、未解决恢复记录或保留的 Rust worktree。
