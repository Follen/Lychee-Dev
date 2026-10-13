# Rust 全量重构：执行目标与验收入口

日期：2026-10-12。状态：进行中，未完成。工作树：`.worktrees/rust-refactor`。终点是同一份封存的 Windows amd64 候选通过全部必需验收，可替换冻结 Go v3.1.2；分项原型、命令注册和测试数量不代表完成。

## 权威与最新变更

完整要求见 [当前需求](rust-refactor-requirements-2026-10-08.md)，实施结构见 [总方案](rust-refactor-plan-2026-10-08.md)。原始附件 `C:/Users/follen/.codex/attachments/b9a17cce-74f2-4b37-ad7a-15a753205b0f/goal-objective.md` 保留来源原文；其第四节“独立静态定位与跨 build 复用”已由 2026-10-12 用户要求替换，不再构成 Mailbox 门槛。

2026-10-12 最新定位修订：Mailbox 参考 Go `v3.1.0`，运行时指令特征匹配/RIP 解析动态 Lua 根，再沿 state → globals → Mailbox 读取；停止私有堆扫描。身份、认证、本次观察、exact COMMIT、持久结果及统一预算继续适用。见 [总方案 §7](rust-refactor-plan-2026-10-08.md#7-mailbox-运行时指令定位与-lua-路径读取)；旧原型证据只证明原受测范围，不代表新入口通过。

2026-10-12 用户已选定四槽 exact COMMIT 文件路线：正常任务 RUN/RECEIVED 各占一对 STAGE/COMMIT，合计四槽；每安装 500 槽，正常容量换代在未决任务安全收尾后自动 reload 目标实例并重连。两槽 token 比较不再是实施选择门槛；完整协议审计、资源界限、多实例隔离和所有 A04/A08 验收仍须完成。合同见 [当前需求第二节](rust-refactor-requirements-2026-10-08.md) 与 [exact COMMIT 方案](rust-native-protocol-exact-commit-2026-10-08.md)。

## 验收门槛

客户端身份补充：用户指定正式别名 **Forever Asia Beta → `wow_classic_beta`**、**Forever CN Beta → `wow_cn_beta`**；分别涉及 `_classic_beta_` 与 `_cn_beta_`，不能将相同 build 当作相同客户端。2026-10-12 只读安装元数据确认根 `.build.info` 两 product 均为 `1.60.1.70338`，metadata Branch 分别 `us`/`cn`；这不证明实际玩家地区、运行时 build 或实机功能。两 `.flavor.info` 分别为对应 product；无 `version.txt`，不得靠目录名补身份。当前 CN 进程路径只作为 OS 目标证据，尚无业务读取/绑定验收。

安装元数据来源哈希：根 `.build.info` SHA-256 `432e224f8d8e971f97addd9411d858fc9faf7b13d2c61d7bd837ce09a677891a`；CN `.flavor.info` `7a46dacb327bf8990bf8ecdca0044e2002122bc258a0448bf42b550801ec0426`；Asia `.flavor.info` `89a497119c160dc1aff3ab41cfc806797d64a64193087fb2c2a96a0db17e6584`。`_beta_` 存在但缺 flavor/version 元数据，保持未识别。

wowdump 的相同别名支持是其 owning chat“Clarify task request”的独立工作；本项目仅只读参考并记录依赖，不修改 wowdump 仓库或全局 Skill。Lychee Dev 候选运行不依赖 wowdump。

| ID | 必需结果与证据 | 当前状态及入口 |
| --- | --- | --- |
| A01 全量命令 | 冻结 Go v3.1.2 的 89 条公开命令及参数逐项映射旧行为、Rust 实现、正常/异常回归、产物与输入；覆盖 JSON/JSONL、分页/截断、退出码、取消、超时及中断恢复。 | 未闭环；[命令快照](rust-command-coverage-2026-10-08.json) 仅为原产物注册事实；[parser 回归](rust-public-parser-regression-2026-10-08.md) 仅为分项证据。 |
| A02 Rust 产品 | 主机业务全部 Rust，保留 Lua addon/npm 包名及入口；最终产品不调用 Go、Python、wowdump。明确取消色块、光学心跳及 WGC，其余业务不得削减。 | 未闭环；[兼容决定](rust-compatibility-decisions-2026-10-08.md)。研究/fixture 依赖与发行运行依赖分别核验。 |
| A03 Source | 先验证固定 EmmyLua Rust 直接库、许可证及完整业务；search/definitions/references/calls/context/flow/validate 和同步/索引/差异/会话/清理完整。固定输入与可追溯事实；luac 5.1.5、XML 位置映射；完整 Retail 连续真实分析、缓存/失效、跨 CLI、故障后的正常请求和资源释放；历史 AV/EOF/OOM/生命周期失败逐项处理，不靠 partial/缩小输入/放宽预算通过。 | 未闭环；[Source 设计](rust-source-design-2026-10-09.md)、[control 原型](rust-source-control-prototype-2026-10-09.md)。当前 host 准入与 flow 仍需验证/整合，默认路径尚未迁移。 |
| A04 Live | 完整统一 Native 协议、独立审计及 Rust/Lua/状态模型/故障证据后方可大范围替换。生产 Rust host/transport、Lua/addon 闭包；Mailbox 运行时指令/RIP 根定位与 Lua 路径读取、500 LoD、多 build/实例与共享安装隔离、私有授权、本次观察、未知输入不重放、结果先持久再收妥/释放、取消/关闭/reload/恢复/换代与全部资源界限。 | 未闭环；[协议审计](rust-native-protocol-audit-2026-10-08.md)、[Live 进度](rust-refactor-progress-live-2026-10-08.md)。新 runtime 根/路径入口尚未接入；堆扫描原型已停止作为产品路线，磁盘静态定稿不作为门槛。 |
| A05 Data 与其余业务 | Data/Assets、项目、证据、安装/升级/卸载/恢复全部等价；精确输入/product/build、质量、分页预算、错误、中断和用户改动保护有独立证据。 | 未闭环；[Data 剩余项](rust-data-remaining-2026-10-08.md)、[Source/Data 进度](rust-refactor-progress-source-data-2026-10-08.md)、[基础进度](rust-refactor-progress-foundation-2026-10-08.md)。 |
| A06 性能 | Source/Live/Data 每必需场景在同机器/输入/输出/校验/缓存下，Go/Rust 交替各至少 20 样本；Rust 中位 ≤ Go 50%，报告 p95、CPU、峰值内存、I/O 与释放；网络、游戏等待和输入节奏单列。 | 未闭环；不得以局部快路径、减少业务或增大预算替代完整矩阵。 |
| A07 候选与 Skill | 同一 Windows amd64 包含 Rust/addon/版本化 Skill/固定分析依赖/luac/许可证/对应源码/哈希；离线隔离安装、两独立环境重建、连续升级及失败恢复通过；skill-creator 优化后由新上下文独立调查验证。 | 未闭环；历史包仅证明其受测身份。npm latest=2.5.1、GitHub latest 保持，候选验证不是发布。 |
| A08 最终实机 | 前述实现/审计/自动回归/候选验证后，同一候选验收 Retail、Classic、Titan、Forever Asia Beta / Forever CN Beta，各单实例、同安装双实例、跨 build 并行、混合多实例与声明上限压力。覆盖安装激活、执行、大结果、换代、reload/relogin、恢复和输入释放。 | 未验收；sidecar、合成窗口、离线 replay 和历史 Go 不替代真实候选证据。需人工参与/缺客户端时明确剩余项，不缩减范围。保留用户十分钟空格任务，不将其输入作为工具证据。 |
| A09 完成交付 | 同一封存候选绑定源码/工具链/输入/产物身份、最终协议/审计、完整业务表、回归、性能、人工验收、兼容与限制。修改后补跑受影响检查，保留首次失败及修复证据。 | 仅所有必需项均通过才标记 complete。任何 failed/partial/not_run/证据不足均保持未完成。 |

## 执行与下一批

沿用当前工作树和证据，不另建分支/worktree，不擅自 commit/push/publish，不覆盖他人改动。主代理整合与独立核验；architect 仅用户明确要求时调用。构建经既有受控工具，共享缓存最多两通道，遵守现有配额/保留空间/所有权，先保全产物、符号、输入、日志和失败证据再回收可重建缓存。不终止其他任务的进程。

当前分项已实际通过 Source entry 201 加边界 36 断言、runtime 根定位 33 新 tests，以及运行时 Lua 路径/Core/完整 Data 自动联合回归 258 passed / 13 ignored。它们只证明受测原型和 fixture 范围。下一批复验当前 Source 原 control/query/native 基线及严格检查，继续完整 flow；Live 按 Go v3.1.0 路线关闭生产 ABI profile/默认接线及完整协议审计，停止私有堆扫描；Data 继续公开 CLI 差分、其余导航/Assets/provider 业务。当前产物身份和未完成门槛见 [最新进度](rust-host-and-runtime-progress-2026-10-12.md)，不借分项结果声明完成。

本页引用现有设计与证据，不重新生成历史通过。执行登记与最新检查须继续记录实际产物身份和未完成范围。
