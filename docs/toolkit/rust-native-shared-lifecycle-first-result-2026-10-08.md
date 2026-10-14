> Historical reference for the Rust migration (2026-10-14): current decisions, baseline and acceptance status are in [handover.md](../../handover.md). This document preserves its original scope and evidence; conflicting requirements, protocol designs, performance targets and release instructions are superseded. Historical test results are not current-candidate passes.

# Native P3 首次 Rust/Lua 共享矩阵结果

Root 执行的首次 governed Cargo 窗口实际终态 0：`native_shared_lifecycle_prototype` 一个联合测试，1 passed / 0 failed / 0 ignored，31.50s。受测代码为两/四槽共用的 Rust `Driver::advance`、真实 Lua51 私有 business receiver，以及 once-loader、clock、owned-resource 模型。**这是原矩阵范围通过，不是完整协议通过**。

原封存目录：`.tmp/rust-supervision/native-shared-lifecycle-build-8be646e8-d8f1-43ee-987a-2bc8dd581d53/`。其 `result.json`、原源码快照、实际输入快照、stdout/stderr、process receipt 和 EXE/PDB 保持不可变。本页是额外结果说明，没有修改这些记录或原受测协议源码。实际 Lua 逐场景轨迹在 `.tmp/rust-supervision/native-shared-lifecycle-98c7d727e3fb98e22ef9b6a49817d2d2/`；governed build receipt 为 `.tmp/rust-disk/evidence/5bd4ef29-0f75-41c1-a5de-375e9361551a/receipt.json`。

| 身份/事实 | 原记录 |
| --- | --- |
| 编译 sourceTree 前后相等 | `ed9acd35a084e682cad8a42f20b4480d66cb1295b991a33c66f4d0185bef6df7` |
| 实际 test EXE SHA256 | `4b59a36548f2f96fb37b2c164e9d1595ce25704e1e7093ed06d2810040e0880a` |
| 同次 test PDB SHA256 | `238d85ced58e5ea7b0ae58b671c9a58edad7248f4ff0e9e259eb87d7a6c6433e` |
| 原 driver.rs SHA256 | `f3b491931433ea4bcdb0385ebbf06971e28fd436cc98e4f14faaf87759db2184` |
| 原 host.rs SHA256 | `af28d6bf7573420e2290ff034a5b772e17958d010673d5f90030682cd9e69679` |
| 原 receiver.lua SHA256 | `09443328cfb94eb10618097ae53a8f987e1f249e5502e09a4435a69669ba07c9` |
| 全部 additionalInputs 前后相等 | result.json 的 before/after 相等；原 sourceSnapshot/actualInputSnapshot 已封存 |
| 外层自有 process/tree 回收 | root/tree reaped、handles/identity locks closed、readers complete、activeProcesses=0、forced=false、deadlineHit=false |

## 实际两槽/四槽同业务成本

从两边 `normal-restore/cost.json` 的 **RUN1 和 RECEIVED2 两条记录**分别读取，而非把额外 CLOSE3、重复 CLOSE3 算进正常独立任务。二者 logical command digest 完全相同，RUN `f6f8f274…d32ee6a`、RECEIVED `1758bde4…accfcd4`，都有 `accepted|published` 和 `cleanup_complete|published`。

| 同一个 RUN→保存完整 BODY→RECEIVED | 两槽 token | 四槽 exact file |
| --- | --- | --- |
| 实际发布 LoD 槽文件 | 2（各命令1） | 4（各命令2） |
| 认证 token ASCII bytes | 508（各254） | 0 |
| final callback | 2 Enter 模型回调 | 2 COMMIT 模型回调 |
| STAGE | 每命令各一个实际 Lua callback | 每命令各一个实际 Lua callback |

额外 CLOSE 与重复 CLOSE 在原 cost.json 中保留并分别计数；总数分别是两槽4文件/1016 token bytes/4 final callbacks、四槽8文件/0 token bytes/4 final callbacks。本次并没有实际 OS INPUT、字符 down/up、客户端 UI 事件或真实游戏节奏，所以这些计数不能变成“少两个按键”、CPU/吞吐或 Live 50% 性能验收结论。

## 新上下文独立审查留下的问题

未参与本 P3 设计的 fresh reviewer 审阅同一冻结源码后提出下列高风险反例。收到时这些新反例尚未执行，不能把原矩阵通过解释成覆盖它们，也不能在修复后再伪造原失败。

1. **合法 MAX_CODE 不能进入载体。** 262144-byte code 的 canonical wire 长 524322；P3S1 再 hex 后为1048820 bytes，Lua `return` 文件1048830 bytes。原 host 的1048800上限与 receiver 的1048704上限均更小。本次通过的512KiB用例是结果 BODY，不是最大 RUN code，不能覆盖这一缺口。
2. **接受过的 RECEIVED 被后续 lastCommand 遮蔽。** RUN1 terminal→保存真实 BODY→RECEIVED2 得 cleanup_unknown→迟到 exact duplicate RUN1 将 lastCommand 降为1、highWater仍2；原 advance 可能一直返回 received_input_unknown_no_replay。后续 CANCEL3 又可能造成 exact_received_outcome_unavailable。需要执行原例，并区分历史结果可观察性与未知输入不得重放。
3. **同一个存活 Driver 或 create-existing 绕过恢复防重放。** Exposure 后丢 input-pending 文件，只有 restore 构造设置 restored=true；同一个 alive Driver、或在已有 intent 目录上 `create` 得 restored=false，可能重新 PrepareTransport。保守 restore 隔离并未覆盖它们。

下一批只准备 isolated negative tests / bounded handshakes，保持原 driver/business/receiver/host 权威函数、wrapper、Cargo 和原封存证据不变。先在 Root 放行的后继窗口执行并记录原反例，再决定修复。Source 的关键批先占用下一 Cargo/重负载窗口，不并行新编译或 Lua 子进程。

准备态补充：fresh reviewer 给出精确 S01–S03 轨迹后，已新增 `negative-cases.inc.rs`（SHA256 `916a6befd08c4fa9c3bdc1ecd10fa8678928c01d3a4a810453d4d14b161ae5fd`）、`negative-original-host.rs`（`d43c3721eab811b1fd087fbd5b10ad1b7636dbb01bfcc59460c6573c6f25c582`）和 `run-negative-original.mjs`（`ba49f4febcaafa88b9daec133fb4325b3f18878c1d1796540564e4d9089c374f`）。原 host 的测试辅助函数在新 test-only copy 内保持原字节，仅取消旧 positive test 的注册并 include 新断言；原 host/driver/business/receiver 和既有 crate wrapper 的 hash 复核仍与首次受测记录相同。新的薄 negative crate 入口尚未创建；runner 尚未执行，全部新断言未编译/未运行。若后继运行返回 `original_review_counterexamples_reproduced`，其含义是原缺陷被实际重现，不能解释为修复或协议通过。

这批八个独立小场景（two/four各4）保留实际 original host 最大 code 发布 panic、original Lua once-loader `file_limit`；签名 highWater/lastCommand 与两次有限 blocker观察；same-alive/create-existing 的重放准备候选。后者仅观察 PrepareTransport，故意不发送 suspect action，也不声称 business 执行两次。reviewer另提出 S04：取消动作可能先耗掉5秒 cleanup deadline，长任务后实际终态 BODY再也不能释放；本批未扩入 S04，仍列为未闭环活性问题。

此前的 restore unwitnessed 活性缺口继续明确保留。私钥/reflection/cold binding、跨暂停 clock/B1、500共享安装 allocator/最大实例、真实 OS carrier、真实游戏与完整业务/性能均未验证；首次 scoped pass 没有关闭协议审计或总 Goal。

## 2026-10-09 原反例实际证据与接续

总 Goal 恢复后，Root 核对了原 driver/business/receiver/host、测试辅助副本及 runner 的封存身份，并在当前 `060d81fa…8ae6` 源树上运行原反例。实际原始案例证据还包括此前 `native-p3-original-negative-build-3eeae5dd-6ac9-4857-b573-e6011fed3ce1` 的终态记录（receipt `c2d27c16-2a0b-4396-a1d1-b22298689b02`，源树 `e15fe5c0…a3c`）；两份证据各自绑定其产物，不覆盖或合并身份。

本次 governed receipt `1ed694f0-e33f-4235-9a97-70f9e2819ea3` 终态0，实际联合测试1 passed/0 failed/0 ignored，1.14s。含义是 **eight counterexample observations reproduced**，不是原协议正确：two/four 各自重现 S01、S02迟到RUN、S02后续CANCEL、S03。原函数和输入前后哈希相同。

- S01：262144-byte 合法代码实际生成1048830-byte槽文件；原 host 载体上限触发 panic，原真实 Lua once-loader 另行返回 `ERR|file_limit`；没有业务执行。
- S02：已保存真实正文且 RECEIVED 已得到 cleanup_unknown；迟到RUN或后续CANCEL使签名 lastCommand 改变。连续两次实际观察分别保持 `received_input_unknown_no_replay` / `exact_received_outcome_unavailable`，正文与原清理期限保持。有限观察证明收尾推进缺口，不夸大为数学上的永久等待。
- S03：原 final key 故意未发，exposure后删除 input-pending。same-alive 与 create-existing 实际返回相同 RUN 的 PrepareTransport；restore 返回 quarantine。未发送危险候选，不宣称实际重复执行。

本次封存目录 `.tmp/rust-supervision/native-p3-original-negative-build-68c9d59e-da60-4f84-b7e3-e5486933c876/`，实际逐场景证据 `.tmp/rust-supervision/native-p3-original-negatives-6a7727dd44c2798d795bfb068b639f2b/`。实际EXE SHA256 `7bdea22221a3728860b09c8a61630f66cf2de02579b6b1efa44e8edc48d23580`，同次PDB `5a1b60a1593fc2279a6934660447ded9e90cd8500292c1f719471e7e4931b36e`。外层原进程/树及全部句柄回收，activeProcesses=0、forced=false、deadlineHit=false。

下一步先新增并实际执行 S04（取消提前启动同一个清理期限，长任务随后产出终态正文而无法收妥）原反例，再制定修正与复核。S04此刻仍未运行；原实现尚未修复，协议审计及总 Goal继续未完成。Data后续多表导航只读准备，与本次编译输入分开协调；没有游戏输入、提交、推送或发布。

S04后继实证：仅追加 test-only 原反例断言，原driver/business/receiver/host及runner哈希保持。governed receipt `4406eb4a-14c8-462e-a355-d4b48b7ffb45` 实际终态0，联合测试1 passed/0 failed/0 ignored、1.54s，two/four共10条反例均重现。封存目录 `.tmp/rust-supervision/native-p3-original-negative-build-87820d14-170c-4bd0-8fa3-845953af08d0/`；实际逐场景证据 `.tmp/rust-supervision/native-p3-original-negatives-c3bdd0f243682c43ad3bf2cc9322a93b/`。EXE `f5607386c12b2d2d61c754e62741ec67407d1a2ad21bdf89e840002480eae74e`、同次PDB `3f56b23acfb1f3fdb6437c0235cc5d0252c175c0cc5c472ee2237acfd04f5fae`，输入前后一致，原进程/树/句柄完整回收、active0、无强制清理。

实际S04轨迹：RUN执行期限20000；第一次取消返回cancel_unconfirmed，提前固定cleanupDeadline15000；模型时钟15001时真实DONE产出verified正文且原执行期限未到。真实BODY独立读取、保存、SHA256核验后，RECEIVED返回cleanup_budget_exhausted；alive与restore均CleanupBlocked。codeCalls=1、cancelCalls=1/cancelEffects=0、closeCalls=0、bodyClears=0，实际私有正文仍与持久字节相同，资源未关闭。此证据只证明模型clock/资源adapter上的原活性缺口，不等同实机clock或资源证明。

修正准备现在位于新的隔离原型目录（原P3和原反例保留），将在真实Rust/Lua矩阵与独立审计后再判断范围。拟分别提供独立签名的exact RECEIVED回执、排他的新journal创建与精确输入准备许可、逐层一致载体上限，以及独立有界取消/收尾阶段；**尚未完成或验证，不是最终协议**。Data三个导航入口已获独立编辑范围，后继编译仅在两边freeze后协调运行。
