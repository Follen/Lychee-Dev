> Historical reference for the Rust migration (2026-10-14): current decisions, baseline and acceptance status are in [handover.md](../../handover.md). This document preserves its original scope and evidence; conflicting requirements, protocol designs, performance targets and release instructions are superseded. Historical test results are not current-candidate passes.

# 两槽候选：认证状态年龄的独立反例（2026-10-08）

状态：Root 已执行独立谓词模型；仅检验“认证 Mailbox + 有界年龄”能证明什么。**没有否定全部两槽方案，没有批准四槽为下限，没有关闭完整协议审计。** 两槽、四槽均继续受完整总目标和未回答的 B1 语义问题约束。

## 被检验的具体性质

两槽第一候选将本次观察从 STAGE/COMMIT LoD 移出：主机验证实例、输入事实和认证记录年龄后，先保存授权，再发布不可变的确切 RUN 或收妥文件；游戏入口仍复核私有条件、身份、期限和去重。

这个方案不能直接继承 P0“本次 nonce 拒绝历史观察”的性质。年龄有上界意味着记录较新，不能直接推导发布后输入条件从未改变。这个区别需要具体合同和实际机制处理；不能通过重命名字段消除。

## 最小反例与实际执行

脚本：`.tmp/rust-supervision/two-slot-age-replay-counterexample-01.mjs`；结果同前缀 `.json`。Node 实际 exit 0，表示反例断言成立。脚本 SHA-256：`7ffac6c604033ecff3204ade649b5114d15fed62214f38011ce165c62a6ab179`。

模型刻意给予年龄方案最有利的时钟前提：游戏时钟跨暂停前进、单调不回退、主机与游戏的映射误差为零；固定可信 binding，真实 Node HMAC 校验；攻击者只能复制已公开的认证记录，不能改私有事实或伪造 MAC。目标保持运行，模型没有在暂停游戏里偷偷给 addon 写入能力。

1. 游戏在时间 1000、1010 发布序号 21、22 的 ready 记录。
2. 其中一个历史在时间 1050 已进入私有 combat 并发布认证 blocked 序号 23；另一个历史继续 ready。
3. 重放者让主机在 1100、1110 读到相同的旧 ready 21、22。两条都递增、MAC 有效，年龄均为 100 ms，满足 750 ms 上限。
4. 主机的“认证 + 递增 + 年龄合格 + ready”谓词在两个历史都准入。在 blocked 历史中，游戏私有执行入口仍会拒绝；**主机输入资格与游戏接受资格是两个不同性质。**

模型还实际断言：当前 blocked 23 会被拒绝；真正过期与未来时间记录被拒绝。上述反例不是时钟误差、忘记 MAC 校验或忘记年龄检查造成的。

## 对比较方案的影响

模型支持的精确结论：有界认证年龄能限制旧记录的年龄，不能证明输入条件在记录发布后仍然成立。给定本次不可预测 nonce 的观察关联，可以拒绝这些旧 ready；对本次私有 combat 的签名回答也会拒绝。这只是信息差别的验证，**没有证明承载新 nonce 的物理入口、真实客户端时钟或其成本**。

下一步应比较以下机制，而非宣布必须用四槽：

- 不消费 LoD 的、只观察不执行的本次关联入口；完整记录其按键、往返和资源成本。
- 有实际契约依据的可信时间、状态发布和时钟映射，并说明合同如何处理记录期间条件变化及公开重放。
- 通过物理输入携带经鉴权的 exact COMMIT 令牌，每逻辑命令只消耗一个 LoD；不能仅回显公开 nonce。

若某方案只承诺“发送前读取过年龄合格的事实，游戏入口决定实际接受”，须明确与更强“主机从未被重放错误授予输入资格”的差别，结合总目标审计；本模型不替用户批准较弱语义。观察完成后到 OS 投递之间的暂停仍属于 B1，不能由本次模型宣布解决。

正常及恢复比较必须计入每个独立任务的最终收妥、所有按键/令牌字节、观察往返、CPU、峰值资源、故障损耗和换代。此模型没有 Rust/Lua/Windows/WoW 执行、500 槽分配、进程崩溃、结果释放或人工验收证据。
