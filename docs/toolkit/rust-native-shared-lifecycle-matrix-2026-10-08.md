> Historical reference for the Rust migration (2026-10-14): current decisions, baseline and acceptance status are in [handover.md](../../handover.md). This document preserves its original scope and evidence; conflicting requirements, protocol designs, performance targets and release instructions are superseded. Historical test results are not current-candidate passes.

# Native P3 共享生命周期原型与待运行矩阵

状态：**prepared / not_run**。本批只修改 `protocol/native-lifecycle-prototype/` 与本页；Root 的 Source 原件、符号封存与正常 Retail 查询仍持有编译/重负载窗口。没有新增 crate 测试入口、修改 Cargo/release/vendor、运行 Cargo/Lua/OS fixture、发送游戏输入、提交、推送或发布。既有四槽与两槽的封存结果保持其原受测范围。

## 一份业务接收端、一份推进器

`business.lua` 保留 RUN / RECEIVED / CANCEL / CLOSE 的同一私有状态机。`commands.lua` 把任务身份、正文、预算与结果引用编码为不可变逻辑命令；传输 nonce 与物理槽不属于业务身份。`receiver.lua` 只提供两种外壳：四槽的 STAGE→exact file COMMIT，或两槽的 STAGE→认证 token→final Enter。两者都由真实 Lua 回调读 once-loader 文件进入同一个业务接收端。主机测试没有直接调用业务 `dispatch`。

`driver.rs::Driver::advance` 在正常与 restore 路径复用同一函数。签名状态的 binding/runtime/session、逻辑命令与结果身份均检查，BODY 的实际字节必须独立 SHA256。实际顺序为 `result.bin` write/fsync → immutable result metadata → RECEIVED intent write/fsync → input uncertainty witness → transport exposure → final callback。PrepareTransport 是准备工作，不能单独授予发送许可；`host.rs` 还用实际 Lua 发布字节、本次 OS nonce 和固定 750ms host 窗口进行资格检查。

所有返回值采用明确 `true` 才确认副作用；false、nil、throw 或先副作用再 false/nil/throw 都保持未知。关闭资源前、失败后新编号重试先 inspect 同一实际 fixture owner。资源确认关闭加公开 BODY clear 确认为 true 后，才释放 private BODY/task。completion 绑定 exact task/result/RECEIVED，独立于 lastCommand；后续 CLOSE 和重复 CLOSE 不修改它。

本次只读代码核对发现并修正的准备态问题：

| 问题 | 本批隔离源码处理 | 当前验证 |
| --- | --- | --- |
| Rust `saved_result` 使用无效的逗号连写 `let` | 改成两个声明 | 未编译 |
| completion 文件包含整个可变 state wire，后续命令会产生错误 immutable 冲突 | 只封存稳定 completion identity；每次认证当前 state 后核同一 completion | host 正常/restore/CLOSE 场景已写，未运行 |
| restore 丢失 input-pending witness 后可能把旧 intent 当未发送 | restored 且未观察旧输入结果、无 witness 时 quarantine，绝不猜测未发送 | lost-input-witness 场景已写，未运行 |
| closed gate 先于去重，使 exact duplicate CLOSE 不能只读观察 | history 身份检查先于 closed | 重复 CLOSE 场景已写，未运行 |
| 四槽 COMMIT 私有 expiry 失败保留 pending，妨碍后续准备 | 失败清除 pending；STAGE 也可淘汰已过期 pending | 未运行；不构成跨暂停 clock 修复 |
| 临时 sealed hardlinks 随正常写入持续保留 | 发布成功后删除本次临时 link；异常与目录条目仍有独立上限 | 未运行；目录 crash durability 未证明 |
| 意图已达 32 时自动 RECEIVED 仍可能写第 33 条 | 先返回 CleanupBlocked | 未运行 |

restore 的上述处理是保守隔离策略，**没有证明所有中断点可自动继续**。如果 intent 已持久而 input uncertainty witness 尚未持久便中断，当前 restore 也可能隔离实际未发送的命令。这是明确的活性缺口，不把“没有重放”宣传为恢复全闭环。需要后续持久状态合同与独立审计决定安全而可继续的处理；不能凭 B1 没有回答降低要求。

## 已编写的真实 Rust/Lua 故障矩阵

`host.rs::shared_lifecycle_fault_matrix_with_real_lua51` 已准备对 two/four 各运行相同场景。没有 crate wrapper 时本文件不进入任何 Cargo 构建。单个总测试中的每个场景使用独立 Lua 子进程、独立槽目录和 journal，记录实际源码/Lua/test EXE 哈希、逐回调 transcript、文件/token/最终回调计数。每个子进程 attach Job，异常 Drop 关闭 stdin、结束自己子进程、释放 bounded receiver 并 join reader；正常出口读回 EXIT 与实际子进程终态。spawn→Job attach 的短边界仍存在，不把它说成完整宿主异常树保证。

| 场景与实际源码名 | 故障边界/必须断言 | 状态 |
| --- | --- | --- |
| normal-restore | RUN→实际 BODY→durable bytes/meta→restore→RECEIVED→CLOSE；completion 文件字节在后续 CLOSE 与重复 CLOSE 后相等；code 执行一次 | not_run |
| cleanup_{false,nil,throw}_{before,after}（6） | 返回未知时 private BODY 保留；同编号只是历史观察；新编号 inspect 后收尾；原 deadline 不重置；副作用后异常不重复 close | not_run |
| clear_{false,nil,throw}_{before,after}（6） | private BODY 保留，不能提前完成；新编号重试；资源 close effect 恰一次 | not_run |
| body_{false,nil,throw}_{before,after}（6） | 私有正文仍在；REPUBLISH 修复发布且不执行代码第二次；之后真实保存与收妥 | not_run |
| state_{false,nil,throw}_{before,after}（6） | BODY 在 state 之前；发布未确认不重执行；重新发布后恢复同结果 | not_run |
| unknown-final-key | uncertainty witness 在 exposure 前；restore 仅观察、不发第二份 RUN；fixture 释放原 queued key 一次后观察结果 | not_run |
| lost-input-witness | 故意删除已 exposure 的 witness；restore quarantine，codeCalls=0 | not_run |
| execute_{false,nil,throw}_before 与 execute_false_after / execute_nil_after / execute_launch_throw（6） | before 无资源不猜 business failure；after 保留 running_unknown；实际 producer 后续完成才得到真实结果，code 至多一次 | not_run |
| timeout-confirmed-stop | 原 execution deadline 到期；fixture owner stop 可确认时结果 unavailable，不能当业务成功 | not_run |
| timeout-unconfirmed-stop | stop false 时 running_unknown；第二 tick 不重复 stop 或重置 deadline；实际晚 producer completion 才得到 verified BODY | not_run |
| cancel_{false,nil,throw}_{before,after}（6） | cancel_unconfirmed 保留执行未知；新编号先 inspect 已关闭 owner，stop effect 恰一次；确认后仍需保存 unavailable BODY 与 RECEIVED | not_run |
| producer-wins-cancel | 真实 DONE 先于 CANCEL；terminal 结果保留，取消不发 stop effect | not_run |
| tamper-and-lost-body | 公开 BODY 篡改被 host 实际 hash 拒绝；公开 state 篡改 quarantine；durable bytes 丢失后不产生 RECEIVED，private BODY 与资源仍保留 | not_run |
| cleanup-budget | 4 次 false 后 CleanupBlocked，private BODY 保留，无第 5 次 close 副作用 | not_run |
| clear-budget | 4 次 false 后 CleanupBlocked，private BODY 保留，owned resource 仅 close 一次 | not_run |
| original-cleanup-deadline | 第一次 RECEIVED 固定 15000 模型 deadline；15001 新编号不得再 close，BODY 不释放 | not_run |
| history-eviction | 18 条后续 exact CANCEL 使原 RUN 超出 16 项 history；旧 RUN 的资格拒绝且无 final-key、无重执行 | not_run |
| body-limit-exact / body-limit-over | 524288 bytes 正文实际保存与收妥；524289 bytes 被编码为 unavailable；不缩减上限 | not_run |

这些是**已写的断言**，不是已发生的结果。测试采用 fixture 私有 owner 资源与模型 clock；不会因此证明真实 WoW frame/hook/timer/focus 或 producer 引擎有相同 inspect/stop 契约。最大正文用例可能需要独立受控重负载窗口，不能在 Source 独占期间运行。

## 还没有写完或必须补做的覆盖

- host 持久化各 cut：BODY fsync 后无 meta、meta 后无 RECEIVED、RECEIVED intent 后无 exposure witness、exposure 后回执丢失、完成记录后父进程中断；实际断进程与 readonly 后继恢复，而不只是同进程构造 restore。
- 同编号不同内容、乱序/缺号、迟到旧 token/file、清理后旧 producer completion、完整 MAC/字段 mutation、signed malformed state、状态/BODY torn sample组合。
- owner/runtime/actor/build/PIDcreated/HWND/安装绑定改变、reload 与冷 binding、公钥/私钥 reflection 与 hook 隔离。
- 500 槽共享安装协调、收尾预留、最大实例数、换代失败与 full product business map；本 fixture 的固定 1..500 once-loader 不是 allocator 证明。
- 冻结 clock 反例、跨暂停 clock 依据与 B1；两/四槽均仍受这些前提影响。
- 真实 OS carrier、焦点取得/释放、partial 字符、硬件权限、host 中断自动释放；token 的 254 字节模型文本不是真实按键记录。
- Go/Rust Live 主要业务完整矩阵 ≥20 交替样本、p95/CPU/内存/IO 与资源释放；fixture cost.json 只用于机制对照，不给性能完成结论。

## 下一受控步骤

Root 明确放行新 build/重负载窗口后，再添加最薄 crate wrapper，先执行小正文矩阵、修正真实失败并封存首失败/修复/最终 EXE/PDB/输入身份。512KiB 场景另按共享资源窗口安排。准备与通过都不能代替新上下文、未参与设计者的独立审计；协议未闭环前不大范围替换 addon 或 host。

后续协调补充：Root 的正常完整 Retail Source 两批实际结果已结束后，明确给本 lane **一个首次 P3 受控编译/真实 Lua 矩阵窗口**。本次已新增薄 `crates/lycheedev-live/tests/native_shared_lifecycle_prototype.rs` 与隔离 `run-rust.mjs`；截至本补充仍未运行。Source/Foundation 的源码保持冻结。runner 的确切内层命令为 `node tools/rust-cargo.mjs --owner native-shared-lifecycle-first -- test --locked --offline -j1 -p lycheedev-live --test native_shared_lifecycle_prototype -- --nocapture`，使用既有 ordinary cache/admission/process owner，无私设 Cargo target。外层最多 300000ms，保存 P3 全部文件及相关入口/工具/矩阵前后 hash/source snapshot；实际 test 早发 evidence pointer，失败也保存 exact EXE/PDB 和 partial transcripts。首编译错误、首真实场景失败都必须保留，后继修复另有身份；没有把 Source 的成功推广为游戏验收。

四槽不是已证技术下限，两槽不是已证完整协议。此前 exact COMMIT、token 与 age-only 反例分别保持原范围；本页没有覆盖或重新提升它们。私钥隔离、冷绑定、expiry/B1、allocator/最大实例、实际载体和真实客户端仍未验证，总 Goal 保持未完成。
