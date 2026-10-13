# Native 协议 v1 决策与最小原型（2026-10-08）

状态：**原型候选，未定稿；无产品入口生效。** v0 原文件及 hash 保留；本稿响应独立审计 V01–V10。当前首先执行 P0，不把未测试的完整设计写成已实现。

2026-10-12 最新定位修订：Mailbox 参考 Go `v3.1.0`，运行时指令特征匹配/RIP 解析动态 Lua 根，再沿 state → globals → Mailbox 读取；停止私有堆扫描。身份、认证、本次观察、exact COMMIT、持久结果及统一预算继续适用。见 [总方案 §7](rust-refactor-plan-2026-10-08.md#7-mailbox-运行时指令定位与-lua-路径读取)；旧原型证据只证明原受测范围，不代表新入口通过。

## 本轮具体决定

V01 的核心缺口是本次观察没有新信息。MAC 和任意多条尚未被主机见过的递增历史，均不能证明当前就绪。采用以下最小候选原型：把一个现有 COMMAND 的加载拆成只准备的 `STAGE` 按键和执行私有 pending command 的 `APPLY` 按键。COMMAND 自带本次 OS 随机 256-bit attempt nonce；不再增加独立 permit 或周期 freshness revision。

1. 主机先验证固定目标和**有效认证被动 Mailbox**（格式、受管安装、选定进程/窗口、已建立 binding；其静态 ready 不授予业务输入），原子保存完整 command intent，记录本次 nonce 生成前的主机单调时刻，才发布 LoD 文件。
2. `STAGE` 的实现只能读取/验证完整 COMMAND、保留至多一份 pending command、发布绑定 nonce +完整 command digest 的当前输入事实；不能执行业务、取消、确认或释放结果。这个限制由真实入口测试，而不是名称保证。它是明确的非业务观察例外；无有效 Mailbox 时不能发送。首次完全无 Mailbox 激活另行处理。
3. 主机只接受 nonce/digest/binding 全匹配，且从本次生成 nonce 前至读完记录的总耗时不超过 Δ 的认证回答。记录不晚于读完、也不可能早于未知的新 nonce，因此其年龄上界是这次总耗时；不从“第一次看到旧记录”推导年龄。
4. 主机 gate 通过才允许尝试 `APPLY`；游戏检查当前输入事实、exact 私有 pending 和单调 command number 后才执行一次。V02/B1 用户语义决定未到，不连接产品 OS 发送入口。

每任务仍是 RUN 与 RECEIVED 两份可执行文件，**总计四次按键和两次本次观察往返**；这不是“两消息所以同样快”。先按这个真实成本验证，不能假设热路径可省 STAGE。省略仅可在已有本次不可重放观察关联且 deadline/未决输入证明相同强度时另审。

为何没有独立 OBSERVE COMMAND：STAGE 已持有完整业务 intent，它所返回的观察直接绑定同一消息，避免再写一份只问状态的 LoD。为何没有 permit：私有 command number 是唯一命令消费权威；nonce 只防观察历史重放，不能描述成事先授权任意未来消息。

## 先验证机制，保留真正失败

P0 原型放在 `protocol/native-v1-prototype/`；Rust 候选 gate 仅由独立 integration test 引入，生产 crate 不导出它。真实 Lua 5.1 用私有 signing closure 生成两条旧 ready，公开篡改器仅得到公开字节，在私有 combat=true 时重放。Rust 先运行 v0 的实际认证+双样本谓词，断言反例仍成立，再运行绑定新 nonce 的候选 gate，断言拒绝。不得把 `MAC valid` 或 `fresh=true` 作为测试传入公理。

第二组用本次 nonce 的真实 Lua blocked/ready 回答：blocked 拒绝、ready 在 Δ 内接受，超过 Δ/clock rollback/错误binding/digest/nonce/篡改/MAC错误/超长输入/同一attempt二次消费全部拒绝。Crypto 使用独立 RFC 4231 HMAC-SHA256 向量与 Rust sha2 对照；Lua 原型纯 5.1，不依赖系统 bit 库。

第三组主动寻找 STAGE/APPLY 的限制：旧无 nonce APPLY 在新 STAGE 后到达、主机尚未看到 staged 回执、主机崩溃、old STAGE 消费 future file。**nonce 回答不解决无 payload 按键的绑定。** 若原型证实旧 APPLY 能执行新 pending，则候选不得授予生产使用；需要增加一条能证明的入队/消费不变量，或换成能携带 exact attempt 的物理输入。不能将该反例改名成“用户按了键所以允许”。

在同一已绑定 runtime 中，最小可审约束是：只有认证证据证明之前所有 host APPLY 已消费，且不存在 partial/unknown 输入，才能发布下一份 pending command；任何未知都禁止新的 STAGE。此约束仍不能自动覆盖 reload 丢失回执或外部重复按键；这些路径由 P1/P2 实际入口模型继续推翻或证明。新 nonce 不是 runtime 身份来源。

P0 不预装 BIND/runtime 真实性前提为产品事实：fixture 使用显式固定 binding，测试覆盖跨 binding 误用；真实冷启动、Lua 反射隔离、时钟跨暂停过期留给 P1 的独立可执行工作。P0 通过只能关闭“在同一可信 binding 内，公开历史重放不能冒充本次观察”这一子性质，不能关闭完整 A01/A02/A09。

## 相比 v0 直接删改的结构

- 删除双递增 publication 作为新鲜度证明；保留 revision 仅用于诊断，不延长任何期限。
- 删除独立 permitRevision/permit 消费状态，完整 command number/digest 为唯一去重依据。
- 当前原型不需要 250 ms HMAC 采样与 ObservePulse lease；只在 STAGE/私有事实变化时发布。是否完整生命周期还需周期工作由 P1 决定，不能先隐藏保留。
- 输入事实 combat/focus/loading/unknown 用 bitset 作权威；ready 与主 reason 从固定规则派生，wire 不接受互相矛盾的冗余字段。
- 暂不实现 v0 未定义的 reconcile；P2 从实际 button→slot 路由决定是否完全删除。不会给模型直接调用 Receive(slot) 的捷径。

## 可执行证据

P0 已执行，**只在固定可信 binding 范围内通过**。命令为 `node protocol/native-v1-prototype/run-rust.mjs`，内部使用唯一 ordinary 通道执行 `cargo test -p lycheedev-live --test native_v1_prototype --locked --offline -j 1 -- --nocapture`。受控编译 0.77 s，测试 1.10 s，exit 0；1 个联合测试实际覆盖 11 类断言，不把它写成 11 个独立测试。

封存索引：`.tmp/rust-supervision/native-v1-rust-7cdbc26a-c4ca-401b-bc12-b23c5ed8cc7f/result.json`；Rust/Lua 逐条输入输出及行为报告：`.tmp/rust-supervision/native-v1-p0-4dc9d3330d6762d30b65948831cf83c0/`。标准构建 receipt：`.tmp/rust-disk/evidence/b6a5a95a-a7ea-4c18-84b8-20e3c9d5d369/receipt.json`。额外 `protocol/` 编译输入和 integration wrapper 在编译/运行前后 SHA-256 一致；实际 EXE/PDB 已复制封存，避免仅用运行时源文件 hash 冒充编译输入。

| 项目 | 实际结果 |
| --- | --- |
| v0 的认证双递增谓词 | Rust 对真实 Lua 历史 ready 字节仍准入，反例复现 |
| 候选本次观察关联 | OLD1/OLD2 均 `wrong_attempt`；当前私有 combat 回答 `input_blocked` |
| 本次 nonce +完整 digest +binding 正确、ready | 准入；本次实际 nonce→ready 11.3913 ms，单样本，非性能验收 |
| 过期/主机时钟回退/错误 binding/错误 digest/损坏 MAC | 全部拒绝 |
| 状态超 8 KiB/读次数超过 32/同一 qualification 二次消费 | 全部拒绝 |
| 实际子进程回收 | root/tree reaped、handlesClosed、outputReadersComplete=true、activeProcesses=0；knownAuxiliaryCleanup=true，对已核验 VCTIP 的终止与业务退出分开记录 |

实际 EXE SHA-256 为 `e69eb3085dd6efe4f7ee4ad71f7ca56ba92511d83fae5ecff6f201051a728198`；匹配 PDB 为 `7b7c90d8dd82ae09542f26219e9a51345fd5c37c33afc3b246091fb0d43bc608`。Lua 解释器沿用固定 5.1 `a45f0f8376d3059a8bc79a4d6d07536cc1d9dec429852cfbc1f899ee96d6cd88`；实际输入清单另有逐文件 hash。v0 独立审查 hash 为 `c73c52b0ac00e91abbcc97768e5ff8dd8beb5f06c38f84726c14c428a0f789fa`。

另一个独立 Node 交叉校验运行在 `.tmp/rust-supervision/native-v1-lua-398360af-cbb3-4b86-a6e5-28d584c736da/`。它用 Node crypto 校验真实 Lua 发布的全部 MAC，并从实际 Lua button handler 与 once-only loader 复现旧 APPLY 反例。禁止新 STAGE 的局部 driver 条件只证明该小函数，没有贯穿真实 durable journal，也没有 OS 队列证据；**不得将它写成 APPLY 问题已经解决。** 当前处理决定是保留本次 nonce 观察机制，拒绝把无额外证明的通用 APPLY 接入产品。

纯 Lua 8 KiB HMAC ×10 的 CPU 为约 1.0 s（两次分别 0.996 / 1.016 s），仅诊断本机 fallback 实现。它进一步说明不应无条件增加每 250 ms 的 HMAC 后台工作。下一 P1 性能用例必须含实际短 STATE、8 KiB STATE、典型 BODY 和完整 512 KiB BODY 摘要；不能仅据短消息测量批准强摘要性能，也不能降低预算或校验强度换取通过。native bit 路径若使用，须从固定客户端原语证实并与同样向量对照。

首次失败按原样保留：

- Lua runner `ecb25a16...` 为 sandbox spawn UNKNOWN，未执行；`a0ef29af...` 与 `60115785...` 发现 RFC case 2 期望字符串抄写漏一位，修正为 RFC 原向量后通过，算法未为此改动。
- Rust build `74613554...` 编译成功但 canonicalize 被 sandbox 拒绝，未进入 Lua；随后 host 复验 `7dac6d1f...` 暴露 Lua 5.1 不支持 Rust Windows verbatim 路径，wrapper 只在传入旧解释器时规范化该前缀。
- `ec9921b0...` 已真实完成 Lua 轨迹，但 Windows Instant 将 1 ns 回退注入量化为零，回退断言失败；将注入改成可表示的 1 ms 后通过，750 ms 准入界限和 gate 未放宽。

RFC 向量来源：[RFC 4231 §4](https://www.rfc-editor.org/rfc/rfc4231)。本稿不宣称私有密钥在真实 WoW 中已隔离，也不宣称输入队列、runtime 重放、冷启动、500 槽完整生命周期或 A03 新接收端已通过。P0 可以交独立审查复核其限定结论；V01/V03 的完整产品关闭仍需要后续入口/身份/恢复证明。
