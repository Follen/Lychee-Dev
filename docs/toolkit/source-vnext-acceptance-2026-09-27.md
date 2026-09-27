# Source vNext 工作区验收记录（2026-09-27）

状态：Source vNext 工作区实现与本地验收完成，尚未发布。此页只记录已实际运行的检查；生产 CI、发布、互动游戏与 Forever 客户端验收均为 `not_run`。

## 固定输入与已通过证据

- Retail 生成 API 原文：Gethe/wow-ui-source commit `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`，`UnitDocumentation.lua` SHA-256 `a33d4c585bc3458ab1179070778a2ebef5c324bc5c56bee9511a9fa9eee04a60`。Forever 规则存在性对照：commit `4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069`，同名文件 SHA-256 `3cefaa815d7e207465a9086b60ee92242d5bdef076af13a227591d23f71662ce`；这不是 Forever 客户端验收。
- `go test -count=1 ./internal/codebase/flow`（附上述固定文件）通过：本地绑定的文档表、真实 `UnitHealth` 返回规则、`UnitPower` 有条件参数规则和原始位置均保留。`UnitHealth('player')` 到 `UnitPower('player', amount)` 得到 `possible` 候选路径，不声称运行态违反。跨文件参数/返回、返回槽位、同名作用域、重赋值、正确/错误 guard、动态别名、条件调用副作用、秘密值算术/比较/索引、输出预算和同一行歧义分别有聚焦断言。
- CLI 离线夹具 `tests/fixtures/codebase/sources/valid-retail`：`source query` 返回固定 commit 和可继续的结果游标；选择真实 `symbolId` 后 `source refs/context --static-only` 保存 capture 和结构覆盖；缺随包 LuaLS 时默认尝试保留结构结果并显示语义缺口；`source context --flow` 显式执行；无效 context 游标退出 2；`source prune --target-bytes 0` 后同一固定 pin 仍可查询。测试见 `internal/command/source_contract_test.go`。
- `semanticDefinitions` 与 `researchEnvironment` 的声明字节一致，第三方源码在未传固定客户端环境时不假定 Retail。LuaLS 输入、声明、配置与原始报告各登记 capture ID；普通 `cache prune` 后逐项 `VerifyCapture` 通过。对应测试见 `internal/codebase/semantic_vnext_test.go`。
- `addoncheck` 已从 source 依赖链抽出；`delivery` 无 codebase/LuaLS 依赖。原 addon release 验证与相关包测试通过。重型 source 同步、索引、检查、语义运行在公共 metadata 写锁外，锁内只保存固定 pin 或 capture。
- `go build ./...`、`go vet ./...`、`go test -count=1 ./internal/codebase ./internal/command ./internal/addoncheck ./internal/delivery` 已通过本轮批次。`node tools/skill-contract.mjs --check` 与 skill-creator 的 `quick_validate.py` 通过：83 条命令合同、208 处引用、0 违规；命令参考由 `describe` 重新生成。

## 固定大仓库读取

下表是全新隔离 home 的首次查询结果：四条客户端源查询 `UnitHealth`，WeakAuras 查询 `WeakAuras.Add`（`.tmp/source-vnext-acceptance/final-query-summary.json`）。耗时为单机观测，不是性能保证。每个查询只取有限结果，`truncated=true`；“已分析文档”是索引覆盖，不代表查询结果完整。

| 源与固定 commit | Pin | 已分析文档 | 首次查询 |
| --- | --- | ---: | ---: |
| Retail `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b` | `PIN-f5ca7507c049580ad68831744678771cc49525719fdc901a91c739db2871b38b` | 4,036 | 7,712 ms |
| Classic `1028c1e687f721ba9d3af14d1b12a5745e4227c7` | `PIN-d3072fa23874a7faf838cd4bb9c6fae473fbfb7756e522ab7e85648055e9c869` | 2,969 | 4,672 ms |
| Titan `825d29d3662b372f0bead725ee6abd339e4a77b5` | `PIN-5919a64afc7b75e3489be9d2cc1394495c8218ed7938e55b67ecf57b94097552` | 2,963 | 5,456 ms |
| Forever `4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069` | `PIN-774581c069f41f749a9a81ed9a9e81470b4c7d80b52386b148f57e7dc480b748` | 4,393 | 5,926 ms |
| WeakAuras `9158131f10fb0126580cfa11b9478bdcfe6c11ef` | `PIN-f1bd40277fdc8d350ae4c46dcf0c8280f2f0b27eb744732315c697df59d9e7a7` | 234，另跳过 1 | 1,205 ms |

WeakAuras 的 `WeakAurasModelPaths/ModelPaths.lua` 为 18,748,435 字节，超过单文件 16,777,216 字节限制，索引明确报告跳过，覆盖为 partial。后续热查询 Retail 643 ms、WeakAuras 117 ms（`final-warm-query-summary.json`）。

四轨道 `UnitHealth` context 均从固定 `symbolId` 取得一段完整原文和固定 API 事实：Retail 1449–1465 行，Classic/Titan 877–891 行，Forever 1525–1541 行（`final-context-matrix-summary.json`）。Retail 和 Forever 的固定元数据含 `SecretReturns=true`、`SecretArguments=AllowedWhenUntainted`；Classic/Titan 对应字段缺失，不能据此推断整个客户端没有秘密值限制。四轨道环境提取覆盖仍为 partial。Retail 使用 `--max-lines 5` 连续翻页 1449–1453、1454–1458、1459–1463、1464–1465，末页 `truncated=false`（`final-context-pagination-summary.json`）。

WeakAuras 中 `UNIT_AURA` 正文搜索定位 `WeakAuras/BuffTrigger2.lua:2370` 的 `RegisterEvent`；`source inspect` 将 144,090 字节完整原文件归档为 `CAP-40233944a9258ba7b9652df2017e7724ad832a40f0fcbfd6ec313d618a2066e2`。Classic 与 Titan 的固定源差异得到 2,447 个文档和 1,996 个声明变化；`--limit 3` 输出显式截断。

最终本地冻结包位于 `.tmp/源研究 离线包 孤儿隔离验收/dev-npm-stage`，对应隔离安装位于 `.tmp/源研究 离线安装 孤儿隔离验收/node_modules/lycheedev`。本地 tgz `lycheedev-local-2.0.6.tgz` 为 18,231,171 字节，SHA-256 `d022f6d234e3ba64bd7cb0d60f589d12fd2101b6d952959ab57d5a8044f8ef68`；它是本地验收包，未发布。`TestPackagedSourceSemanticCLIWhenAvailable` 已通过：包内 LuaLS 3.19.1 对固定小型 addon 产生报告和独立可核验的输入、声明、配置、原始报告 capture，安装后的 CLI 完成 query→symbolId→refs/context。真实大仓库结果另列如下。

孤儿隔离修复前的冻结验收包（同版 LuaLS 与语义代码）在另一独立 home 对完整 Classic/Titan 固定源实际运行 query→声明 `symbolId`→incoming refs。Classic `SYM-7376ea002988d52cbfbdbf2d5b7fb08c` 与 Titan `SYM-0f1c6aed04c2f373db9fe960e8f44a82` 各得 27 条不截断关系：13 条 LuaLS `resolved` 非声明引用、13 条静态候选和 1 条结构事件。两者 `semantic=complete`，LuaLS 版本 3.19.1；总体覆盖仍为 partial，因为 API 环境提取有缺口。冷关系查询分别约 12–13 秒，热查询约 1.6 秒；原始结果在 `.tmp/source-vnext-agent-acceptance/{classic,titan}-refs-200.json`。

独立干净 home 中完整 Retail `UnitHealth` incoming 返回 33 条不截断关系，其中 16 条 `resolved`；`semantic=complete`、总体 partial 仅由环境提取缺口造成。该次观测 15,851 ms、输出 13,812 字节（同时运行了全量 Go 测试）。WeakAuras `WeakAuras.Add` outgoing 返回 4 条不截断关系，其中 `TimeMachine:DestroyTheUniverse` 与 `Private.Add` 两条有 LuaLS `resolved` 位置；其余解析和大文件边界使 `semantic=partial`，13,603 ms。README 的 `C_Spell.GetSpellInfo`→context 示例取得对应固定 API 事实和 22 段原文，`semantic=complete`，结果仍按预算显示分页。原始摘要分别在 `.tmp/source-vnext-acceptance/final-retail-semantic-summary.json`、`final-weakauras-semantic-summary.json`、`final-readme-context.json`。

干净隔离 home 的 `source prune --target-bytes 0` 从 88,181,463 字节回收到 0，移除两个已核验的 WeakAuras/Retail worktree，`complete=true`；归档原文 `CAP-f05eb87885f9cdd25e2171b9657df1249cce287c87c33fda58ac86c5ac33ad02` 的 144,090 字节仍完整可核验，原 WeakAuras PIN 之后可查询。在另一个保留脏 WeakAuras orphan 的旧 home，Retail 再次取得 16 条 resolved、`semantic=complete`；prune 从 89,687,264 字节到 57,021,980 字节，只移除 verified Retail，`complete=false`，脏 orphan 的 `.editorconfig` 哈希不变，原 `CAP-40233944a9258ba7b9652df2017e7724ad832a40f0fcbfd6ec313d618a2066e2` 仍可核验。证据见 `.tmp/source-vnext-acceptance/final-clean-*` 和 `final-orphan-*`。

## 本地门禁与边界

WeakAuras 严格 worktree 原文字节校验曾发现 Windows `text=auto` 换行问题；固定 `core.eol=lf` 后新隔离 home 的真实 LuaLS 查询通过，旧 home 脏 orphan 已证明只限制自身预算而不阻塞 Retail。

最终孤儿隔离实现冻结后，`LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` 全通过，包含 Lua 5.1、真实 LuaLS/固定 API 环境/完整 Retail workspace 和最终隔离安装包测试；日志 `.tmp/source-vnext-acceptance/full-go-lua-final.log`。`go build ./...`、`go vet ./...`、diff 检查、版本与 LuaLS 身份检查、skill-creator quick_validate 均通过。Node 测试 54/54；skill 合同 83 条命令、208 处引用、0 违规。验收结束时残留 LuaLS 进程为 0；delivery 的 Go 依赖闭包无 codebase/LuaLS。共享工作区原有的 217 个受保护 data 文件按字节审计未改变。

生产 CI、发行组装/发布和 source→live 真机调查未运行，分别保留 `not_run`；本地包与源码验收不等于已发布能力。真实 source→live 游戏输入未执行，不能用静态候选代替已验证报告或清理完成。

Flow 只分析所选有界 Lua 闭包与固定规则。`AllowedWhenTainted` / `AllowedWhenUntainted` 保留原始条件，只给出 `possible`；缺规则的内建操作、事件 payload 到 handler 的映射、动态调用或别名、预算截断均显式报告边界。无发现不等于安全；LuaLS 声明和静态关系不证明加载、战斗、秘密值或 secure execution taint 的运行态结论。
