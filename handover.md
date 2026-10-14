# 当前交接：fix-3.2.0

更新日期：2026-10-14，Asia/Shanghai。只保留当前状态；协作规则见 [AGENTS.md](AGENTS.md#multi-agent-handover)，完整问题证据见 [审计报告](docs/Clean-repo-2026-10-14.md)。

## 分支、授权与状态

- [PR #13](https://github.com/Follen/Lychee-Dev/pull/13)，`codex/fix-3.2.0` → `main`，保持草稿。已推送首批产品修复 `81ccdd5` 和文档 `f7f2239`；R5 `2660a4ef` Windows CI 全部通过；R6 `9a02522f` 六个 CI 作业通过；R7 已推送 `f84cf0f4`，[当前 head Windows CI](https://github.com/Follen/Lychee-Dev/actions/runs/38540889167) 因 R4-C01 双卷探针失败，已完成并待修复重跑。
- R3/R4 的 13 项及 R5 的 5 项已交付；R6 又确认并实施 4 项新事项。R4-01 crash 分支和 D-06 清理残留沿用原 ID，不重复计数。前一批 42 项保留为基线，候选不当作确认问题。
- 追加任务：至少 R5/R6/R7 三轮，各由不同的新审计者检查架构、bug、死代码、Skill 编排及有明确收益、可验证的架构优化；持续审计→修复→验证→推送，直到独立复审不再发现可离线修复项。仅实机的新问题记录复验，不代替用户操作。
- 本任务允许提交、推送同一分支及更新 PR，阶段性进展同时汇报。未连接用户电脑/游戏、未发键或部署游戏目录；不合并、不发布。只使用仓库 handover.md，不使用 handoff Skill。
- 发行源仍 `3.1.2`、渠道 `next`、`latest=2.5.1`；`3.2.0` 仅为开发基线。

## 当前实施与文件边界

| 项目 | 当前结果 | 写入负责人 / 边界 |
| --- | --- | --- |
| R7-01 | Hotfix CSV 阻止覆盖当前 workspace 身份；公开 CLI 定向回归通过 | 协调者：command dispatch、records export boundary/SQL 共享检查与正式测试 |
| R3-03 / R8 归档依赖 | R7 scan 部分已推送；R8 普通 Hotfix、demux/image 依赖已修复，12 包/race 通过，整合中 | Data/evidence 组独占 records/evidence 生产与正式测试；无通用 JSON 扫描 |
| R7-02 | 两个零消费者 UI 方法及不可达 callback 已清理，Lua 全包通过 | 协调者：TreeView 与 Events 页面 |
| R4-C01 已确认 | 保留同卷/显式 output，跨卷默认改为目标侧恢复目录；已交叉编译/vet，待新 CI | doctor 组独占 live_doctor_windows.go 与对应测试 |
| R8-02 | Bundle 预发布取消检查已修，公开 CLI 正式/race ×3 通过 | 协调者独占 evidence/bundle.go 与 command/evidence_bundle_test.go |
| R7-03 附加测试清理 | 一处重复 SHA mismatch 表项、一个重复 locale key 已删除；build/vet 与定向六 profile 通过 | 测试专项组已冻结，未暂存；正式业务用例/fixture 保留 |
| 独立审计 / 文档 | R8 整合已完成；R9 已开始并复现 SQL query→CSV 依赖间隙，独立 Skill 真实离线深试用中 | 审计者只读；协调者独占审计、handover、todo、Changelog |

R6 `9a02522f` 六个 Windows CI 作业全部通过。R7 已推送 `f84cf0f4`，其原生 CI 的 contract/race/汇总因 R4-C01 失败；两处冗余输入清理和 R8 新修复尚未提交，不借旧绿色结果宣称当前验证完成。正式测试不依赖临时审计 fixture。

## 实际验证与限制

- 工具链：官方 SHA 校验 Go 1.27.1、Node 24.18.1、npm 11.18.0、固定源码构建 Lua 5.1.5；`LYCHEEDEV_REQUIRE_LUA51=1` 与明确解释器路径。LuaLS 3.19.1 Windows archive bytes/SHA 已校验，不等于原生进程运行。
- R7 build/vet、records/evidence/vault 12 包、依赖/scan 与 Hotfix CLI race 各 ×3、addon 34 suite × 六 profile = 204 组通过。旧实现的 workspace 覆盖和来源/页面删除负对照失败，新正式回归通过。
- R8 与两处冗余输入清理后的完整 Lua 必需命令 `go test -p=2 -parallel=4 -count=1 -timeout=20m ./...` 实际执行：30 测试包通过、command 1 包仅 3 个既有 Linux 平台失败、4 包无测试；2484 test/subtest pass、3 fail、28 skip。三个失败是 Native Live 参数检查两项和非 Windows LuaLS broker 的 Source prune，基线已复现；不称 Linux 全套通过。
- R6 已完成 [`9a02522f` 的 Windows CI](https://github.com/Follen/Lychee-Dev/actions/runs/38536137573) 六作业全部 success：Lua 必需完整 Go、race、process、addon、真实 tgz/隔离安装、汇总。R7 `f84cf0f4` contract 因 R4-C01 双卷探针失败，同卷通过；process/addon/package 通过；race 同样仅因 R4-C01 失败，无 detector 报告。修复后需要新 head 原生验证。
- R8 Node tools 全套76通过/6个Windows专属skip、0失败；覆盖率汇总10项通过，真实历史JSON兼容性解析无误。import 合同、文档本地引用、diff 检查通过。版本/Skill/命令目录输入未变。R5/R6 的计量、旧代码负对照与额外容量 reload 计时 fixture 修正详见审计，不复制运行历史。
- 真实 LuaLS/broker/安装包语义、外部 API/CASC oracle/图像/原 runtime 样本等可选集成仍缺输入。Windows 专属测试、交叉编译、缺环境 skip 与游戏验证分别记录，均不冒充当前通过。

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
| R4-C01 已确认，修复中 | Windows 双真实卷、隔离 release/client/project；同卷对照后跨卷跑默认 archive，PID=0；预期磁盘升级/旧树归档后在安全激活边界停止 | R7 CI 同卷通过；跨卷 Rel 失败，旧树完整、无 archive；修复后待复验 |
| LuaLS/broker/打包 Source 可选覆盖缺口 | Windows 固定可运行LuaLS、匹配release/CLI和docs；运行正式real integration，预期引用/关闭/取消/隔离及安装语义断言通过 | |
| 固定 API/DB2/CASC/图像及原 runtime replay 缺材料 | 取得匹配hash/build的固定资料，运行各正式oracle/replay测试；预期身份、原始字节、解码和根证据吻合，不把合成fixture当原始输入 | |
| Live read-only 可选覆盖缺口 | 后续授权的Windows/WoW固定目标；只读测试核对身份/窗口，不扩大输入许可 | |

## 下一步

1. 完成 R3-03 普通分页/R8-01 资产依赖、R8-02 Bundle 与已确认 R4-C01 修复（均已冻结），整合后推送同一 PR 并运行新 head Windows。
2. R9 主审与独立 Skill 真实离线试用并行；已确认 SQL query→CSV 间隙将最小修复，如有可离线项继续下一轮。协调者独占更新审计、todo、handover。
3. 冗余测试专项已删除两处重复输入并正式回归；下一固定提交在现有 Windows 完整作业中采集 Go 语句覆盖率，业务场景单列，当前未计量、不估算。
4. 暂存材料只在忽略的 `.tmp/pr13/`，任务结束后按已核实绝对路径清理本轮产物；不触碰用户项目、未解决恢复记录或保留的 Rust worktree。
