# Native exact COMMIT：授权发布点原型（2026-10-08）

状态：独立增量候选，未接产品。v0/v1/P0 文档及证据保持原样；B1 用户决定仍待答复。本文不把受控原型证明扩大成 Windows 队列、冷启动或游戏验收。

2026-10-12 用户选择：以本文的四槽 exact COMMIT 文件方式作为当前实施路线。每个正常独立任务的 RUN 和 RECEIVED 各用 STAGE/COMMIT 一对 LoD，合计四槽、四次正常触发；Mailbox 读取次数另计。两槽 token 保留历史研究，不再承担路线选择门槛。该决定只选择提交载体，不等于 B1 语义、完整协议审计或产品验收已经通过。500 槽容量换代在未决任务完成收尾后的安全点自动 reload 目标实例并重连；先证明收尾容量、额外耗槽和多实例分配，不能用强制 reload 消除未知执行或丢弃未保存结果。

2026-10-12 最新定位修订：Mailbox 参考 Go `v3.1.0`，运行时指令特征匹配/RIP 解析动态 Lua 根，再沿 state → globals → Mailbox 读取；停止私有堆扫描。身份、认证、本次观察、exact COMMIT、持久结果及统一预算继续适用。见 [总方案 §7](rust-refactor-plan-2026-10-08.md#7-mailbox-运行时指令定位与-lua-路径读取)；旧原型证据只证明原受测范围，不代表新入口通过。

## 具体替代

删除无 payload 的 APPLY。只读 STAGE 仍携带完整请求及本次随机 nonce；主机验证本次认证观察后，才允许发布新的、不可变的 COMMIT 文件。COMMIT 含 exact binding、command number、nonce、完整请求 digest 及分配的 stage/commit slot。游戏的 COMMIT 按键入口实际调用 once-only LoD loader，只有该文件与私有 pending 完全相符且当前游戏 gate 合格才执行业务。

由此，**文件发布是业务授权的暴露点，按键只是促使已授权文件被读取。** 旧键可以提前消费已授权的 exact COMMIT；不能替主机生成、选择或授权尚未通过 fresh gate 的另一个请求。主机必须先持久化完整授权 intent，再允许可执行 COMMIT 出现；不能只在本次按键前写 journal。accepted 不推导哪个物理键已经消费。

父审查者的 FIFO 反例保持为输入：外部 COMMIT 先完成 A → 旧 STAGE 读取刚发布 B → host 为 A 投递的旧键到达。候选必须分别证明：未发布 B COMMIT 时最多损耗槽/失败，B 不执行；只有 B 的 fresh gate 已通过并已持久授权、发布 exact COMMIT 后，任何键才可能执行 B。这不依赖按键来源、队列排空或等待时间。

## 最小真实入口与有限失败

本次原型先使用固定已分配的 stage/commit 对，两个真实 Lua button callback：STAGE 只读取下一 stage slot；COMMIT 只读取私有 pending 指定的 commit slot。每 LoD 一次加载；空文件也算消耗。主机不能覆写已发布文件，也不能复用已消费 slot。过早旧 COMMIT 因文件不存在而损耗该对，不能让稍后新文件重新执行；要继续观察，必须用新 nonce、新 slot 对，原 attempt 保留未执行/未知事实，不能声称这次输入没发生。

一个 command 需要两 LoD、一项任务 RUN+RECEIVED 需要四 LoD、正常四次按键。这是相对于 v1 额外文件的真实代价。若盲用 8×62 固定 lane，扣掉 BIND/CLOSE 后正常容量约 14 项/换代，明显应重新评估；不沿用 v0 的“28项”数字。候选后续比较固定 lane 与全安装池按命令预留/私有 next-slot 链接；本次不把不存在的动态路由写成已实现。

对比物理输入携带 exact identity：可让 COMMIT 直接携带 nonce/digest，不另用 COMMIT LoD，但需要可验证的有界输入编码、帧边界、重放/部分消息恢复、焦点处理。仅用一个泛化热键不携带身份；用有限键色轮转也会遇到重复与迟到。按 16 符号编码 256-bit nonce 就需要至少 64 个数据符号（未算 framing/校验），因此不能先声称它比一文件一唤醒更简单或更快。当前优先验证不可变 COMMIT 文件；物理携带身份保留为明确成本的备选，不伪造可用原语。

## 本轮可执行目标

真实 Lua 的入口必须先经过模拟的 `LoadAddOn` once-only 行为再验证消息，不能直接调用 Receive。Rust host 原型必须把 `prepared → fresh observation → authorization recorded → immutable commit published` 的事实推进落在实际代码，并在每个切点停止/恢复；不能给 Lua 测试预设 `authorized=true`。

关键轨迹：父 FIFO 反例、旧 COMMIT 提前消耗空槽、exact COMMIT 的 binding/number/nonce/digest/slot 任一错配、重复键、同编号异内容、私有 combat 切换、主机在 fresh gate 前/后及 publication 前/后中断、重复发布相同/不同字节、跨请求旧 STATE 重放。每条消息、文件数和读取数设上限。当前不增加没有用途的 reconcile、drain receipt 或第二本 recovery journal。

完整 runtime 冷启动、旧 BIND、MAC 私有边界、跨暂停过期和真实进程输入仍未由此增量证明。P3 后续收妥清理必须沿同一 exact COMMIT 发布规则，不能为清理另开无身份快捷入口。

## 证据

父反例为 `.tmp/rust-supervision/astra-v1-apply-accepted-not-drained.mjs/json`，脚本 SHA-256 `067653ee65982932c21ae76b5f562016930daa89110f9aba33d8b83e159fcef9`。v0/v1/P0 原文件及封存证据未改动。

2026-10-08 已执行 `node protocol/native-exact-commit-prototype/run-lua.mjs`，exit 0，耗时约 0.91 s。主机当前是 **Node fixture**，真实 Lua 5.1 从 button callback → once-loader → 实际磁盘 LoD 文件的 `loadfile` 返回字节验证 exact 请求；不是直接调用 Receive。Rust host 源码已单独准备，但 **尚未编译/运行**，不把 Node 结果记为 Rust 结果。

成功证据：`.tmp/rust-supervision/native-exact-commit-lua-0eda4bc8-f327-4fec-8f65-25cbf41eb915/`；输入 before/after 清单一致，各场景保存实际 slot 文件、封存发布文件、intent/authorization 记录和 Lua stdout/stderr。结果 SHA-256 `1f3594ccd4329184be94b6aaf40df77e3c50ae3ecce7d111169180914bf50de4`；receiver 源码 `7e5bd90c31a8da54f64fda569be10497fe7886aa15de523897eee27da1b995f2`；runner `79c67a8612bcb9188d4692a27d430d06dab11655b4a6902fb893e4a9dab7c063`。固定 Lua 解释器身份沿用 P0 清单 `a45f0f8376d3059a8bc79a4d6d07536cc1d9dec429852cfbc1f899ee96d6cd88`。

| 轨迹 | 实际限定结论 |
| --- | --- |
| FIFO E(A)→旧STAGE(B)→H(A)，B未授权发布 | A接受是真实的，B仅staged；旧H只消耗B空COMMIT槽，B执行0次。host没有推导H已排空 |
| 同一FIFO，B本次观察及授权记录完成后才发布exact COMMIT | 旧H可促使已授权B执行1次；这是文件授权发布合同，不是物理键因果证明 |
| 旧COMMIT早于文件发布 | 空槽损耗后稍晚发布同槽文件不能重新执行；新number、nonce及slot对可继续，原尝试不被描述为“未发送” |
| binding/number/nonce/stage slot/commit slot/op/payload 七类错配 | COMMIT实际字节重新计算digest后仍被私有exact匹配拒绝；格式不合法的slot关系先被格式校验拒绝 |
| 私有combat在fresh观察之后变阻塞 | COMMIT入口在load前拒绝，commit槽未消耗；解除阻塞再加载仅执行一次。未测试secret/focus等真实客户端条件 |
| 重复键、重复同字节发布、冲突字节发布、同编号异内容STAGE | 不重复业务；同字节发布只报告已存在；不同字节文件不可覆盖；receiver序号拒绝异内容旧编号 |
| prepared/observed/authorized/published四切点 | 前三个切点按键不能执行；仅第四切点可执行exact请求。这是停止在阶段边界的fixture，不是完整OS进程崩溃及磁盘故障矩阵 |

fixture文件通过完整写入、`fsync`文件、hard-link no-replace发布；没有证明 Windows 目录/卷崩溃持久性，也没有证明真实受管安装的500空wrapper加载语义。模型假定每个空wrapper once-load也消耗；这个假定只保守地展示损耗，后续必须按真实客户端原语核对。固定可信binding、测试私钥及短消息沿用P0边界，未关闭冷启动、真实Lua私钥隔离、跨暂停过期、最大消息成本、B1或游戏验收。

受测receiver/runner/crypto/observation原文件已在上述证据的 `source-snapshot/` 复制封存，并逐项先核对before清单身份；后续修改候选源码不会使历史证据只能读到新版本。系统Lua解释器保留精确hash及原路径，不将其复制充当产品组件。

首次失败均保留：`native-exact-commit-lua-f4f91d79-a096-4dcc-9cd2-cedb47c074bf` 为 sandbox `spawn UNKNOWN`（未执行Lua）；`native-exact-commit-lua-22e5518d-58af-4801-a554-1c37a47b7f14` 为故意构造同编号异内容时fixture自身的create_new intent路径碰撞（EEXIST）。修正仅把那条恶意文件注入绕过host journal，以实际测试receiver的序号拒绝；没有放宽不可变journal或授权规则。

### 认证回执与过期增量（仍保留反例）

2026-10-08 已执行 `node protocol/native-exact-commit-prototype/run-auth-expiry.mjs`，exit 0，耗时约1.27s；证据 `.tmp/rust-supervision/native-exact-auth-expiry-2bf96155-085e-45ec-85db-692c7c6ec388/`。这是不同受测版本：`receiver-auth-expiry.lua` SHA-256 `038a2ac95e3aab232cb8de298bf96f1fd1935e2308c720298fc6054ca17cf8dc`，runner `237cc49275616555b5b23b175ec3a39d5d2f0cf7182273b929a4a06018ddb15b`，结果 `7ba4b7b78e18526859b6c46ee6ba916cb877ff78fed659d49a8cd3cd64528132`。输入before/after一致，受测源码已复制封存，先前receiver及证据保持原样。

本增量不以 stdout `ERR/ACCEPTED` 作为认证事实：实际Lua私有closure签名 `LDEC1` Mailbox字节，含binding/runtime/session、exact command identity、slot、outcome、这次是否消费slot、模型clock/deadline及有界计数。Node独立验证HMAC及完整identity后断言：空COMMIT是`commit_empty_slot/consumed=true/executions=0/loads=2`；错配是绑定原expected identity的`commit_exact_mismatch`；私有combat是`private_blocked/consumed=false/loads=1`。stdout仅是fixture读取这些已签名字节的管道，**尚无真实游戏进程内Mailbox地址/内存读取**。

过期分开记录两条轨迹，不能把其中一条替换为笼统“expiry通过”：

- 私有模型clock从10000前进到10751时，先前staged deadline=10750；即使exact COMMIT已经发布，旧callback得到认证`private_expired`、未消耗COMMIT、业务0次。这只证明**若**能取得跨暂停前进的可信clock，receiver的拒绝分支有效。
- exact COMMIT发布后真实主机墙钟等待800ms，而模型game clock冻结于10000；新input guard以当前主机时间拒绝新发送/新publication。故意投递此前排队的旧callback仍得到认证`accepted`并执行1次。**这是真正保留的冻结clock反例，跨暂停过期尚未解决**；不能把新发送guard拒绝误称为旧输入不会执行。

候选Rusthost也须在完整发布操作前/后以及每次实际新输入前取当前时间。检查与fsync/link不原子：若发布后已过期，文件已暴露的事实不能回滚为“未发布”，必须保留不确定授权；即使新输入guard随后拒绝，游戏侧跨暂停expiry仍不可缺。Lua控件`CLOCK`只属于fixture，不是声称WoW存在的clock原语。

Node runner没有outer Job，正常退出子进程已回收不证明父进程异常终止能收全后代。Rust 原型使用受控outer树和Lua JobGuard，并含满16项输出channel时panic的回收断言：先drop receiver解除阻塞sender，再join reader及核对child终态；已执行范围见下一节，不扩大为所有父中断cut。

### 受控Rust/真实Lua51对照已执行（有限通过，包含预期反例）

2026-10-08 Root在Foundation owner测试终态后冻结全部writer并授权唯一 integration wrapper。执行 `node protocol/native-exact-commit-prototype/run-rust.mjs`，内部正常ordinary通道命令 `cargo test -p lycheedev-live --test native_exact_commit_prototype --locked --offline -j 1 -- --nocapture`，原exec直接terminal0，无悬而未决或重启的handle。编译4.82s，**一个联合test**通过，0失败，实际运行1.48s；不是9个独立测试。

| 证据 | 绑定身份 |
| --- | --- |
| 构建/封存索引 | `.tmp/rust-supervision/native-exact-commit-build-ac0b9a3c-7fc2-447c-8b1a-d7e246d02adb/result.json`，SHA-256 `6955b5f82cacba44e236a4e54dad3ea1a5e9e078d88d44b8134ba63f88876899` |
| 实际Rusthost/Lua逐条轨迹 | `.tmp/rust-supervision/native-exact-commit-rust-94a840a6fc96faba3c0c1d34c8449a4d/`，result SHA-256 `f1ce3140e9885591ea041c39acec463fdcdec37b3dd181a9d514c661e13c2071` |
| standard受控receipt | `.tmp/rust-disk/evidence/7605e356-6845-472d-a9fa-60d9e595389d/receipt.json`，retired、handlesReaped=true |
| 本次源摘要 | `2e353585013cf2870b9b6ceeea5909b5b1927ca05dca4df9079739d0603aa760`（包含本次唯一wrapper） |
| 封存实际EXE | `2b9256cfbf8320a374533a189084e3ad3b5f76d7fce7b5ce07cb410a968ef387` |
| 匹配PDB | `6a39e8bec932b32eb629bdcfb5323acceaeeb8f453930d8d69cd7158f9e47d6b` |

wrapper只导入独立`protocol/native-exact-commit-prototype/host.rs`，没有产品入口。protocol额外编译输入、Lua/crypto/gate、wrapper和runner前后hash一致，完整受测源码已在build证据`source-snapshot/`封存；Rust runtime清单还绑定实际执行EXE与固定Lua解释器。实际EXE/PDB复制后核验，不以运行后源码hash冒充编译产物。

联合test实际覆盖FIFO两支、prepared/observed/authorized/published四切点、签名空槽消费/accepted回执、模型clock前进拒绝expired，以及真实墙钟800ms后`send_new_commit`当前时间guard拒绝新送入路径。在冻结game clock分支，刻意旁路新送入路径交付旧queued callback仍accepted，**这个反例仍成立，expiry未关闭**。`fsync`/hard-link发布前后取时间不能消除已暴露文件的竞态，本test没有把它描述为原子授权时限。

第18条reader记录使16项channel已满后，故意触发host panic并由`catch_unwind`接住；析构实际先drop receiver使阻塞sender醒来，再join reader、终止/wait Lua并drop JobGuard，断言readerJoined、childTerminal、handlesDropped全部满足。stderr中的“expected fixture abort with full bounded output channel”是这个预期轨迹；测试最终exit0。outer owned证明root/tree reaped、handles/tool locks closed、output readers complete、activeProcesses=0，无deadline命中。其`forced=true`与`knownAuxiliaryCleanup=true`同时成立：Cargo/test正常exit0后，Job内已核验Microsoft VCTIP PID33376仍运行，按既有admission结束该辅助进程；不能把它写成`forced=false`或业务被强杀。Lua JobGuard attach仍发生在spawn之后，没有声称无pre-attachment race。

编译有两个只来自保留P0 gate的unused字段/function `dead_code`警告，没有修改旧P0来消除它们；不将本次测试当全crate clippy或产品回归结果。没有实际首编失败，本次不能虚构首失败；前述Node/Lua首次失败保持原样。

fixture的RUN/RECEIVED仍只记录模型业务分派，未实现/验证BODY、durable结果、cleanup重试或取消。Windows/WoW真实输入、Layout/Mailbox内存读取、冷启动认证、私钥隔离、cross-pause clock/B1、500槽完整分配/换代及候选实机仍未证明。四槽对照有限通过不使四槽成为技术下限；两槽独立比较见 `rust-native-two-slot-candidates-2026-10-08.md`。

## 500槽重新核算的明确假设

不能从两个逻辑消息推导两个槽：每个STAGE/COMMIT占一个LoD，正常RUN+RECEIVED占四槽。对一条RUN，必须在暴露任何可执行文件前同时为其RECEIVED预留完整两槽；close/control预留另算，不能以耗尽后再争取收尾空间作为方案。

以下数字是**有条件算术模型，尚非产品支持上限**。假设每实例BIND占两槽、CLOSE占两槽；无并发第二任务；正常任务四槽；共500槽，任务预留包含收妥；失败损耗和另外控制消息都减少余量。BIND本身的冷启动真实性仍待P1，因此不把四槽控制假设写成已实现。

| 分配方式 | 条件性正常容量 | 简化与代价 |
| --- | --- | --- |
| 原8条固定62槽lane | 每lane floor((62−4)/4)=14项，共112项，安装剩余4槽 | 相比v0“两槽任务28项”减半；固定lane的入口简单，但单实例最多只用约1/8安装容量 |
| 全安装500槽单一不可变分配账本 | I个已绑定实例时，总 floor((500−4I)/4) 项；I=1为124，I=8为117 | 不预先固定八实例；每实例私有next-slot链和host账本协调引入真实复杂度，需要按实例/请求原子预留RUN与收妥、拒绝重用、处理入口链切点 |

全安装池不是“免费更简单”：host应只有一份带安装身份的分配账本和一个互斥所有权；各实例只持其不可变私有链。游戏不能读账本，next-slot必须由已经验证的LoD消息携带；不能让Mailbox篡改决定加载哪个实例的文件。私有next跳转何时提交、空槽损耗如何推进、换代时仍在线实例及旧按键如何失效，均待可执行模型，不用增添第二本recovery journal掩盖。

任何槽位复用都必须重新验证旧runtime不可误读新文件，而不是仅“新generation写入成功”。全池需要协作换代还是允许实例独立epoch、最大支持实例数、关闭失败及额外控制预算，仍待原型/审计决定。数学可容纳的实例数不等于可支持或已经压力验收的实例数。
