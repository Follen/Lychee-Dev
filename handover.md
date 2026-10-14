# 当前交接：fix-3.2.0

更新日期：2026-10-14，Asia/Shanghai。只保留当前状态；协作规则见 [AGENTS.md](AGENTS.md#multi-agent-handover)，完整问题证据见 [审计报告](docs/Clean-repo-2026-10-14.md)。

## 分支、授权与状态

- [PR #13](https://github.com/Follen/Lychee-Dev/pull/13)，`codex/fix-3.2.0` → `main`，保持草稿。已推送首批产品修复 `81ccdd5` 和文档 `f7f2239`；R5 `2660a4ef` Windows CI 全部通过；R6 已推送 `9a02522f`，[该 head Windows CI](https://github.com/Follen/Lychee-Dev/actions/runs/38536137573) 六作业全部通过。
- R3/R4 的 13 项及 R5 的 5 项已交付；R6 又确认并实施 4 项新事项。R4-01 crash 分支和 D-06 清理残留沿用原 ID，不重复计数。前一批 42 项保留为基线，候选不当作确认问题。
- 追加任务：至少 R5/R6/R7 三轮，各由不同的新审计者检查架构、bug、死代码、Skill 编排及有明确收益、可验证的架构优化；持续审计→修复→验证→推送，直到独立复审不再发现可离线修复项。仅实机的新问题记录复验，不代替用户操作。
- 本任务允许提交、推送同一分支及更新 PR，阶段性进展同时汇报。未连接用户电脑/游戏、未发键或部署游戏目录；不合并、不发布。只使用仓库 handover.md，不使用 handoff Skill。
- 发行源仍 `3.1.2`、渠道 `next`、`latest=2.5.1`；`3.2.0` 仅为开发基线。

## 当前实施与文件边界

| 项目 | 当前结果 | 写入负责人 / 边界 |
| --- | --- | --- |
| R7-01 | Hotfix CSV 阻止覆盖当前 workspace 身份；公开 CLI 定向回归通过 | 协调者：command dispatch、records export boundary/SQL 共享检查与正式测试 |
| R3-03 残留 | Source/页面依赖同租约保护、历史依赖及缺页恢复已修复；12 包及 race ×3 通过 | Data/evidence 组：capture 依赖发布/保护、scan 恢复及正式测试；video 测试仅调整 Ref 比较 |
| R7-02 | 两个零消费者 UI 方法及不可达 callback 已清理，Lua 全包通过 | 协调者：TreeView 与 Events 页面 |
| R4-C01 候选 | 新合同证据支持跨卷；原生双卷磁盘 fixture 已加入，交叉编译/vet 通过，仅待原生证据 | Windows fixture 组：仅 doctor Windows 测试；尚无原生结论 |
| 独立审计 / 文档 | R7 已完成，仍有可离线修复项；之后需要新 R8 | 审计者只读；协调者独占审计、handover、todo、Changelog |

R6 `9a02522f` 六个 Windows CI 作业全部通过。R7 工作区尚未提交，不能借 R6 绿色结果宣称 R7 验证完成。正式测试不依赖临时审计 fixture。

## 实际验证与限制

- 工具链：官方 SHA 校验 Go 1.27.1、Node 24.18.1、npm 11.18.0、固定源码构建 Lua 5.1.5；`LYCHEEDEV_REQUIRE_LUA51=1` 与明确解释器路径。LuaLS 3.19.1 Windows archive bytes/SHA 已校验，不等于原生进程运行。
- R7 build/vet、records/evidence/vault 12 包、依赖/scan 与 Hotfix CLI race 各 ×3、addon 34 suite × 六 profile = 204 组通过。旧实现的 workspace 覆盖和来源/页面删除负对照失败，新正式回归通过。
- R7 完整 Lua 必需命令 `go test -p=2 -parallel=4 -count=1 -timeout=20m ./...` 实际执行：30 测试包通过、command 1 包仅 3 个既有 Linux 平台失败、4 包无测试；2456 test/subtest pass、3 fail、28 skip。三个失败是 Native Live 参数检查两项和非 Windows LuaLS broker 的 Source prune，基线已复现；不称 Linux 全套通过。
- 已发布 head [`9a02522f` 的 Windows CI](https://github.com/Follen/Lychee-Dev/actions/runs/38536137573) 六作业全部 success：Lua 必需完整 Go、race、process、addon、真实 tgz/隔离安装、汇总。R7 未提交修改需要自己的原生 CI，R4-C01 双卷 fixture 仅交叉编译/目标 vet 通过。
- R7 受影响 workflow Node 检查 8/8、import 合同、文档本地引用、diff 检查通过。版本/Skill/命令目录输入未变。R5/R6 的计量、旧代码负对照与额外容量 reload 计时 fixture 修正详见审计，不复制运行历史。
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
| R4-C01 候选 | Windows 双真实卷、隔离 release/client/project；同卷对照后跨卷跑默认archive磁盘升级，停在PID=0，不需游戏；按实际Rel/MoveFileEx/恢复轨迹和支持合同定性 | |
| LuaLS/broker/打包 Source 可选覆盖缺口 | Windows 固定可运行LuaLS、匹配release/CLI和docs；运行正式real integration，预期引用/关闭/取消/隔离及安装语义断言通过 | |
| 固定 API/DB2/CASC/图像及原 runtime replay 缺材料 | 取得匹配hash/build的固定资料，运行各正式oracle/replay测试；预期身份、原始字节、解码和根证据吻合，不把合成fixture当原始输入 | |
| Live read-only 可选覆盖缺口 | 后续授权的Windows/WoW固定目标；只读测试核对身份/窗口，不扩大输入许可 | |

## 下一步

1. 完成 R7 整合验证并提交；R4-C01 以双卷磁盘 fixture 在同一 draft PR 获取真实 Windows 结果。
2. 提交推送 R7 后用新的独立审计者复审 R8；如仍有可离线项继续循环。协调者独占更新审计、todo、handover。
3. 新独立专项先审计/删除无独立价值的冗余测试并正式回归，之后才对固定提交计量 Go 覆盖率；当前未计量，不给估算比例。
4. 暂存材料只在忽略的 `.tmp/pr13/`，任务结束后按已核实绝对路径清理本轮产物；不触碰用户项目、未解决恢复记录或保留的 Rust worktree。
