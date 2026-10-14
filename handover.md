# 当前交接：fix-3.2.0

更新日期：2026-10-14，Asia/Shanghai。只维护当前状态，不追加运行流水；规则见 [AGENTS.md](AGENTS.md#multi-agent-handover)。

## 分支与范围

- [PR #13 · fix-3.2.0](https://github.com/Follen/Lychee-Dev/pull/13)，分支 `codex/fix-3.2.0`，目标 `main`，保留草稿。
- 本轮从最新 head `c772fc1868330a7335be9676bf3fe28af30fda9b` 接手。前一批 42 项修复保持基线，不重复全仓审计。
- R3/R4 的 **13 项确认问题代码已修复**，已加入正式测试；**3 项候选仍为候选**。逐项证据、状态和复验见 [审计报告](docs/Clean-repo-2026-10-14.md#本轮-r3r4-修复交付)。
- 用户授权提交、推送同一分支并更新 PR；没有合并、发布或游戏操作。本轮未连接用户电脑/游戏、未发键、未部署游戏目录。
- `3.2.0` 仍是开发基线；发行源 `3.1.2`、渠道 `next`、`latest=2.5.1` 均未改变。

## 已完成代码与协作边界

| 组 | 已修复 ID 与结果 | 负责人 / 文件边界 |
| --- | --- | --- |
| Source / Data / 存储 | **R3-03** pending Hotfix 来源删除与原 checkpoint 恢复；R3-02 精确声明 flow；R4-02 跨进程 CSV/manifest 成对发布；R4-05 inspect/capture 覆盖 | Source/Data 组，已冻结；`internal/codebase/`、`internal/records/`、`internal/evidence/` 与正式测试 |
| Live / addon | R4-01 同 runtime 跨交易输入采样及原日志 watermark；R3-01 Trace 错误出口；R3-06 战斗停拾取；R4-04 默认 bridge 状态；R4-07 流式错误与部分结果 | Live/addon 组，已冻结；`internal/live/channel/`、`addon/`、`tests/addon/`、`tests/protocol/` |
| CLI / 交付 / Skill | R3-05 root/client 更新身份及历史事务；R3-04 Windows npm prefix；R4-03 默认 repairs 父目录；R4-06 stdin 查询期限、取消与读任务清理 | CLI/交付组，已冻结；`internal/command/`（独占 `entry.go`）、`internal/delivery/`、Skill launcher 与测试 |
| 集成 / 文档 | 当前 TOC 代替已退役 Definitions.lua 的打包 smoke 输入；受影响验证、全套验证、同 PR 交付与文档 | 协调者；`handover.md`、审计、`todo.md`、`Changelog.md`、包装 smoke 与正式 Node 测试 |

新测试不依赖已删除的审计临时 fixture。导出锁复用现有 OS lease 并在目标父目录保留锁文件，确保跨 home/进程协调；未扩大 crash/partial 合同。Trace 仍支持正常 frame、继承和可索引 owner；未加新的 unsupported 限制。

## 本轮实际验证

- Linux 云端：校验官方 SHA 的 Go 1.27.1、Node 24.18.1、npm 11.18.0；按仓库入口构建校验固定源码的 Lua 5.1.5；LuaLS 3.19.1 Windows archive 的 bytes/SHA 与固定清单一致。
- `go build ./...`、`go vet ./...` 通过；受影响 evidence、records、flow、codebase、navigatetest、delivery、channel、journal、live、addon/protocol 包通过。新增 capture/CSV 并发与 SQL stdin 重复 race 检查通过。
- Lua 必需：`LYCHEEDEV_REQUIRE_LUA51=1`、明确的 `LYCHEEDEV_LUA51`。最终 addon/protocol 定向回归通过，34 个 Lua suite × 六 profile = 204 组，含 24 组新增回归，无跳过；不代表实机通过。
- 全仓 `go test -p=2 -parallel=4 -count=1 -timeout=20m ./...` 已在 Linux 执行（以 JSON 记录结果）。结果为 **30 个测试包通过、command 1 包失败**，另 4 包无测试；28 个 test/subtest skip 事件（含 5 个只供子进程调用的 helper），均未计为通过。command 的 3 项 Linux 平台失败在接手基线 overlay 上原样复现，未抑制：Native Live 参数检查两项需要 Windows；Source prune 的 LuaLS broker 在非 Windows 返回 unavailable。
- Node tools：72 项中 66 通过、6 项 Windows 专属跳过、0 失败。合并运行 npm native launcher 时另有 1 通过、4 项因 Linux unsupported-platform 失败，不能称为 Windows 通过。
- 版本检查仍为 3.1.2；Skill 合同 84 条命令 / 220 个引用 / 0 违规，生成命令目录与 import 合同通过。Windows command/channel 测试已交叉编译，**只计编译**。
- Windows 原生验证：待本次提交的既有 `toolkit foundation` CI。须检查同一代码提交的 windows-contract（Lua 必需整套）、windows-process、windows-addon、windows-package、race 和 ci-required；不能沿用 c772fc18 的旧检查。旧 head 的 windows-package 因读取退役文件失败，本轮已修该 smoke。
- 固定真实 LuaLS/broker/安装包语义、外部 API 文档/CASC oracle/图像样本及显式游戏安装等可选测试未在此 Linux 环境执行；跳过不计作通过。七产品旧实机矩阵见 [todo.md](todo.md)，不扩展成本轮验收。

## 待实机验证

以下五项均为“代码已修复、Lua/状态机离线验证通过、待实机验证”。只供后续明确授权的本机任务执行，实机结果留空；各项详细步骤见对应审计条目。

共同前置：匹配版本的干净受管 CLI/addon，明确产品/构建及 installation/PID/创建时间/HWND，使用原 project/CON/request 和期限；不重放 submitted/uncertain 输入。

| ID | 最小步骤与预期 | 实机结果 |
| --- | --- | --- |
| R3-01 | 非战斗时 Trace `GetTime.foo`/标量路径，再试正常方法；错误显示在页面，正常目标可追踪 | |
| R3-06 | 开拾取后进入战斗，主窗口保持隐藏；dock/键盘脚本立即关闭，F/Enter/Escape 不被吞 | |
| R4-01 | 无副作用请求跨 prepare/commit/confirm/release、原 CON resume；同 runtime 采样至少晚于上次输入 100ms，无重放；reload 后新 runtime 独立 | |
| R4-04 | 隔离新配置检查 status、off/on、保存 off 后 reload；status 与 INPUT 生命周期一致 | |
| R4-07 | Objects 检查嵌套失败 tostring 及普通错误，再检查健康对象；点击不抛二次错误、失败状态明确，部分流/limited 保留 | |

R3-04、R3-05、R4-02、R4-03、R4-06 的 Windows 宿主验证与上述游戏验证分别记录；R4-03 的正式 fixture 在真实磁盘升级后、PID=0 激活边界停止，不需要游戏。

## 仍待核实的候选

- **R3-C01**：当前 SpellMiscDetail/legacy golden 是单值，仓库没有固定真实多行/DifficultyID fixture，未证明领域合同违例；保留候选。
- **R3-C03**：ForeignOwner 仅被动展示，instances 无 `--project`；Connect/doctor/activation 使用真实项目 ID，没有找到阻断消费者；保留候选。
- **R4-C01**：doctor archive 跨 Windows 卷的 Rel/MoveFileEx 路径仍需合法双卷部署证据和支持合同；Linux 不能替代验证，保留候选，不添加 unsupported 门禁。

## 下一步与交付边界

1. 本地验证已完成并如实记录；提交推送同分支，让既有 Windows CI 执行当前修改，修复本轮引入的实际失败。
2. 确认当前提交的 Windows 检查终态，更新此交接、审计状态与草稿 PR 描述；不合并、不发布、不更改版本或 npm 标签。
3. 后续本机任务仅处理上述五项待实机验收与既有七产品矩阵；三项候选有新合同/真实证据再定性。

临时内容只在忽略的 `.tmp/pr13/`，结束后清理本轮产物；不触碰任何用户项目、未解决恢复记录或保留的 Rust worktree。
