# 当前交接：fix-3.2.0

更新日期：2026-10-14，Asia/Shanghai。只保留当前状态；协作规则见 [AGENTS.md](AGENTS.md#multi-agent-handover)，完整问题证据见 [审计报告](docs/Clean-repo-2026-10-14.md)。

## 分支、授权与状态

- [PR #13](https://github.com/Follen/Lychee-Dev/pull/13)，`codex/fix-3.2.0` → `main`，保持草稿。已推送首批产品修复 `81ccdd5` 和文档 `f7f2239`；本次提交包含 R5 全部修复，Linux 整合验证已结束，当前新代码 Windows CI 待执行。
- R3/R4 的 13 项确认问题已修复；R5 又确认 5 项新事项，另补齐 R4-01 crash 分支和 D-06 清理残留，不重复计数。前一批 42 项保留为基线，候选不当作确认问题。
- 追加任务：至少 R5/R6/R7 三轮，各由不同的新审计者检查架构、bug、死代码、Skill 编排及有明确收益、可验证的架构优化；持续审计→修复→验证→推送，直到独立复审不再发现可离线修复项。仅实机的新问题记录复验，不代替用户操作。
- 本任务允许提交、推送同一分支及更新 PR，阶段性进展同时汇报。未连接用户电脑/游戏、未发键或部署游戏目录；不合并、不发布。只使用仓库 handover.md，不使用 handoff Skill。
- 发行源仍 `3.1.2`、渠道 `next`、`latest=2.5.1`；`3.2.0` 仅为开发基线。

## 当前实施与文件边界

| 项目 | 当前结果 | 写入负责人 / 边界 |
| --- | --- | --- |
| R4-01 crash 残留 | 原 receipt 恢复后建立一次持久 host-uptime 上界，再等 +100ms；不重放、不换 request/期限 | cadence 组，已冻结；`internal/live/channel/` 与正式测试 |
| R5-01 | CaptureWriter 保留有限 binary64 数值，30 个值经真实 Go↔Lua 报告及 journal 恢复逐位相等 | 数值组，已冻结；`CaptureWriter.lua`、`tests/protocol/` |
| R5-02 / R5-03 | demux 按规范化目标目录串行发布完整帧/manifest；直接复用 CommitCapture 的 Blob，去掉重复写入 | demux 组，全部已冻结；`internal/records/` 与正式回归，保留 SQL 原 lease 身份 |
| R5-04 / R5-05 / D-06 | 修正 Skill 旧取消键/无 INPUT reload 说明；删除两个无调用 Stores helper 和注释锚点测试，保留真实 UI/locale/持久化覆盖 | 协调者；Skill、Stores、对应测试和全部共享文档 |
| R3/R4 已交付 | R3-01/02/03/04/05/06、R4-01/02/03/04/05/06/07；另修正旧打包 smoke 的 Definitions.lua 引用 | 代码 `81ccdd5`；详细逐项状态见审计，不重复列全表 |

R5 独立审计已完成，确认 2 个生产 bug、1 个可计量架构优化、1 个 Skill 合同错误、1 个窄死代码清理；新增项均有源码/真实路径证据，没有提出大范围重写。R6、R7 尚未启动。正式测试不依赖临时审计 fixture。

## 实际验证与限制

- 工具链：官方 SHA 校验的 Go 1.27.1、Node 24.18.1、npm 11.18.0、固定源码构建 Lua 5.1.5；`LYCHEEDEV_REQUIRE_LUA51=1` 和明确 `LYCHEEDEV_LUA51`。LuaLS 3.19.1 Windows archive bytes/SHA 校验通过。
- R5 定向：各代码批次 build/vet 通过；channel/journal 全包、protocol 全包及 addon 全包通过。新增 cadence 覆盖 bind/prepare/commit crash、旧日志、+99/+100 边界、重复 Load、not_sent、新 runtime；数值回归旧 writer 在 16/30 值上失败，修复后逐位正确。addon 34 suite × 六 profile = 204 组，无 Lua 跳过。
- R5 版本检查、Skill 84 命令/220 引用/0 违规、生成命令目录通过；受影响 Node 18 项中 15 通过、3 个 Windows launcher 专属跳过，未计为通过。demux 正式跨进程/独立目录/取消/原始 capture 一致性通过；staging 回归从 6 次降至 4 次且精确少写一次全部帧。demux 与 SQL export race 各重复三次通过。
- R5 整合 build/vet 通过；完整 Lua 必需 `go test -p=2 -parallel=4 -count=1 -timeout=20m ./...` 已实际执行（JSON 留证）：30 测试包通过、command 1 包失败、4 包无测试；2420 个 test/subtest pass、3 fail、28 skip。仅下述同样的 3 个基线 Linux 平台失败，无新增失败，不能称为 Linux 全套通过。
- 最近已完成 Windows 原生 CI：[`81ccdd5` 产品代码](https://github.com/Follen/Lychee-Dev/actions/runs/38528396047) 和 [`f7f2239` 文档 head](https://github.com/Follen/Lychee-Dev/actions/runs/38529520702) 各六作业全部 success，含 Lua 必需完整 Go、race、process、addon、真实 tgz/隔离安装。**这些结果不覆盖本次 R5 新代码**；新提交后重新核验 CI。
- 上批 Linux 完整套件实测 30 测试包通过、command 1 包失败、4 包无测试；3 个失败已在接手基线 overlay 原样复现：Native Live 参数检查两项、非 Windows LuaLS broker 的 Source prune。28 个 test/subtest skip 事件含 5 个子进程 helper，均未计为通过。上批 Node tools 66 通过/6 Windows 跳过；npm native launcher 1 通过/4 unsupported-platform 失败。对应 Windows 为 tools 71 通过/1 可选 archive 跳过、launcher 5 通过。
- Windows 交叉编译只计编译。真实 LuaLS/broker/安装包语义、外部 API/CASC oracle/图像样本与显式游戏安装等可选集成未在本 Linux 环境运行；不把旧结果、跳过或交叉编译当作当前通过。R4-03 是 Windows 磁盘 fixture，不属于游戏环境限制。

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

## 保留候选

- R3-C01：SpellMiscDetail/legacy golden 为单值，没有固定真实多行/DifficultyID fixture，领域违例证据不足。
- R3-C03：ForeignOwner 为被动展示，instances 无 `--project`；真实连接/doctor/activation 项目身份正确，未找到阻断消费者。
- R4-C01：doctor archive 跨 Windows 卷缺少合法双卷部署证据和支持合同；Linux 不能替代，不添加 unsupported 门禁。

## 下一步

1. 本批 R5 定向与整合验证已完成，推送同一草稿 PR 后核验新 head Windows CI，任何失败继续修复；不拿前一提交的绿色结果代替。
2. 在 R5 修复后代码上启动新的 R6 审计者，再以 R6 修复后代码启动不同的 R7 审计者；如仍有可离线项继续循环。协调者独占更新审计、todo、handover；每轮不追加流水账到本文件。
3. 暂存材料只在忽略的 `.tmp/pr13/`，任务结束后按已核实绝对路径清理本轮产物；不触碰用户项目、未解决恢复记录或保留的 Rust worktree。
