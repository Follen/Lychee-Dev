# 当前交接：fix-3.2.0

更新日期：2026-10-14，Asia/Shanghai。由接手主代理维护本文件中的当前状态，不追加逐轮日志。用法见 [AGENTS.md](AGENTS.md#multi-agent-handover)。

## 接手入口

- 仓库：[Follen/Lychee-Dev](https://github.com/Follen/Lychee-Dev)。交接分支为 `codex/fix-3.2.0`，PR 标题为 `fix-3.2.0`，目标分支为 `main`；以该 PR 的最新 head 为准。
- 起点为 `1f7b5977bd509422128d7c14bc8c4aeabb1ca07f`。本分支收纳其后的当前代码基线、清理、修复、审计报告和交接约定。
- 本轮交付是可继续开发的基线。前一批 42 项确认问题已修复；追加两轮审计新发现的 **13 项确认问题尚未修复**，另有 **3 项候选**。下一项实现工作从这些待办开始。
- `3.2.0` 是本次开发基线及 PR 名称；[release/version.json](release/version.json) 仍为 `3.1.2`。[npm 渠道](release/npm-channel.json) 仍为 `next`，保留 `latest=2.5.1`。没有合并、发行或修改已发布包。
- 先读 [Design.md](Design.md)、[Live 协议](docs/live-protocol.md)、[todo.md](todo.md)、[开发指南](docs/development.md) 和 [Changelog.md](Changelog.md)。详细问题、触发、证据及修复方向见 [审计报告](docs/Clean-repo-2026-10-14.md)，最新待办为文末 R3/R4 条目。

## 已完成的基线

- 退役旧主动 Live、Receiver、QR、聊天输入和私有堆扫描路径；迁出仍需要的历史读取、记录与释放代码，保留当前 Native Mailbox/500 槽协议。
- 修复证据归档与删除协调、原事务恢复、Live 等待及日志容量、Source 索引和查询、Data 结果边界、安装升级、npm 回收锁、发布来源证明和 Skill 启动等已确认问题。共享 Source 解析器与有效测试已迁移。
- 原报告 38 项确认问题及后续确认的 C-01/03/05/06 共 42 项已修复；C-02/04/07/08 已明确合同并落实。精确状态与验证范围以报告为准，不重开已修复项来重复计数。
- 三名 GPT-6.1-sol xhigh 已连续完成两轮追加全仓审计，第二轮交换分区。新增问题合计高 1、中 11、低 1；审计阶段只更新报告，没有实施这 13 项修复。

## 待办与建议分工

下列修复任务尚未分配，各组均待接手。开始实现时由协调者填入实际负责人和状态；这张表记录协作边界，不增加审批步骤。

| 任务组 | 待修问题 | 主要文件范围 | 当前负责人 / 状态 |
| --- | --- | --- | --- |
| Source、Data、存储 | **R3-03（高）**：删除仍被 pending Hotfix scan 引用的来源；R3-02：精确 SymbolID 的 flow 丢位置；R4-02：并发 CSV 覆盖的数据/manifest 错配；R4-05：Source inspect 丢弃索引覆盖 | `internal/codebase/`、`internal/records/`、`internal/evidence/` 及对应测试 | 未分配 / 待修复 |
| Live、addon | R3-01：Trace 非表 owner 抛错；R3-06：战斗后拾取器仍截键；R4-01：跨交易未保留输入后 100ms 采样条件；R4-04（低）：默认 bridge 状态错报；R4-07：Object 流式错误再次格式化失败 | `internal/live/channel/`、`addon/`、`tests/addon/`、`tests/protocol/` | 未分配 / 待修复 |
| CLI、交付、Skill | R3-04：Windows npm 全局 prefix 回退失败；R3-05：game-root 更新恢复改变事务路径；R4-03：doctor 默认归档父目录未创建；R4-06：SQL stdin 不受整个查询期限控制 | `internal/command/`、`internal/delivery/`、`skills/lycheedev/scripts/` 及对应测试 | 未分配 / 待修复 |
| 集成与文档 | 去重核实、协调共享文件、受影响验证、更新审计修复状态及本文件 | `AGENTS.md`、`handover.md`、`docs/`、`todo.md`、跨组调用边界 | 接手主代理 / 待认领 |

优先处理 R3-03 的持久化恢复来源删除，再推进输入采样与事务/导出一致性。各组可并行，但 `internal/command/entry.go` 等共享文件先确定一个写入负责人。复用当前分支；未发生实际隔离需求时不另建 worktree 或 PR。

三项候选继续保留，先查合同与实际证据，再决定是否修复：

- **R3-C01**：SpellMisc 多行的单值选择。已有合成双行证据，缺少固定真实构建的变体语义及明确领域合同。
- **R3-C03**：foreignOwner 的项目 ID 与 home WorkspaceID 不同。尚未发现阻断连接或恢复的消费者；实例列表也没有 `--project` 语义。
- **R4-C01**：doctor 默认归档与游戏跨卷。已定位路径检查与目录发布边界，尚无完整跨卷验证。

CON claim 发布前的取消窗口缺少已成功创建后的明确恢复合同及用户后果，已排除为本轮确认问题。不要据此扩建恢复框架。

## 已有验证及其边界

以下检查在本次交接前实际执行，详细结果见审计报告“验证与交付边界”。接手时按新改动选择复验，不把旧结果当作未来代码的通过证明。

- 主修复批次：`go build ./...`、`go vet ./...` 和 Lua 必需的完整 `go test -p=2 -parallel=4 -count=1 -timeout=20m ./...` 通过；31 个测试包通过。Node 工具及 npm launcher 共 75 项通过，无跳过。
- 后续 C-01/05/06 改动：重新通过 build/vet、command/luals 整包及定向回归；最终开发包上的 channel、journal、codebase、addon/protocol 五个包及安装包 Source 语义测试通过。没有在这些追加改动后再执行一遍整个仓库；未改变的 delivery 长套件和 Node 检查沿用主批次结果。
- 固定 LuaLS 3.19.1 的真实语义、broker、跨 CLI 与安装包入口已实际执行，九项必跑集成都有通过结果。版本一致性、Skill 合同、生成命令目录和 Skill 格式检查通过；Skill 合同为 84 条命令、220 个引用、0 违规。
- C-06 在亚服 Forever `1.60.1.70374` 做过真实前后对比：GetTime/InCombatLockdown 的 Trace 重入已修复；最终报告 verified、cleanup complete，临时 hook 经独立 reload 移除，连接已关闭。其他六产品的矩阵未因本次工作重跑，见 todo.md。
- 可选的符号链接权限、固定外部样本、CASC/oracle 数据与交互环境覆盖仍有限；没有把跳过计作通过。追加 R3/R4 的定向离线 fixture 证明缺陷存在，不是修复通过或实机验收。
- 当前交接只增加/调整文档，按项目约定检查文档链接、路径、暂存内容与格式；没有因此重跑未改变的整套测试。

后续代码批次遵循开发指南：build、vet、受影响测试，提交完成前满足 Lua 必需的完整套件要求。Lua 可用已有 5.1 解释器，或使用仓库构建入口；真实 LuaLS 与发行组装输入需要按固定清单准备。测试包并发及 20 分钟安装测试时限不要省略。

## 工作区与交付边界

- 本轮用户已授权提交当前基线、推送 GitHub 并创建交接 PR。没有合并或发行授权；保留发行版本和 npm 标签。
- `.worktrees/rust-refactor` 明确保留，不纳入当前分支提交或清理。用户原有 `.lycheedev` 项目和未解决恢复记录继续保留；不要通过删除记录消除待办。
- 前一批修复和本次追加审计的临时包、日志、探针与合成 fixture 按任务范围清理；报告保留关键触发和结果。修复新问题时把必要回归加入正式测试，不恢复旧临时目录作为运行依赖。
- 实机操作必须使用明确的安装、PID/创建时间/HWND 和原 project/CON/request。此次 PR 交接本身不授权操作另一客户端；已提交或结果不确定的输入不能重放。
- 结束一个实现批次时，协调者更新本文件的负责人、状态、实际测试结果和下一项动作，并同步对应审计条目；提交、推送、合并、发布分别记录真实状态。
