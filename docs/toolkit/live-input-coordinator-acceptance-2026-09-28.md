# 统一输入与 Live 编排验收

日期：2026-09-28。工作区候选，未发布；本页与旧传输、上一轮输入局部修复的结果分开。
后续 Classic / Forever 双在线实测、退出占用修复及对应边界见[补测记录](live-input-multiclient-2026-09-28.md)。
下文 not_run 描述的是本页 Retail 验收当时的范围。

## 范围与入口

目标契约为 [输入架构](live-input-architecture-2026-09-28.md)。保留原生内存和 64 槽位，
本轮统一输入意图/事实、连接推进、reload、结果投影与内容引用，不替换 source/data。

离线入口：强制 Lua 5.1 的 `go test -count=1 ./...`、`go build ./...`、`go vet ./...`；
`node tools/skill-contract.mjs` 与 skill-creator 的 `quick_validate.py` 检查 skill。
新增 coordinator/input/artifact/scan 用例验证以下性质：

- 每次真实输入前存在持久意图；取消后已知发送结果仍落盘。
- 零发送继续原槽位；缺失发送结果、已提交 reload 不重复输入。
- reload 与 invoke 共用就绪检查，模拟三次 Esc 后恰好停止；只有新 runtime 绑定可完成。
- 关闭中的连接不能借用 Bound/上次任务状态报告完成。
- 大脚本不在每条日志重复保存，旧内联快照可读，内容损坏拒绝恢复。
- 提前命中的扫描摘要保持不完整标记，不生成海量虚假的未读错误。

上述离线检查通过。最终强制 Lua 5.1 全量输出保存在
`.tmp/channel-live/coordinator-accepted-go.txt`；build/vet、版本一致性、skill-creator 校验均通过。
Skill 命令校验覆盖 87 项命令契约、203 个文档引用，0 项违规。
本轮使用的受管插件来自 `.tmp/channel-live/package-coordinator-reload/dev-npm-stage` 开发包。

## 真机

固定 Retail PID 31208、12.1.0.69933、灵止光—死亡之翼。候选从开发包受管安装，
通过公开 CLI 激活、恢复和断开；没有手改日志、特殊重发或切换实例。

| 检查 | 状态 | 证据 |
| --- | --- | --- |
| 核心重构首轮受管升级、reload、绑定、关闭 | pass | `.tmp/channel-live/retail-coordinator-activation/` |
| 单/多行自建编辑框，三次 Esc、原文与光标不变 | pass，6 步 | `.tmp/channel-live/retail-coordinator-focus/focus.json` |
| 新增 reload 观测能力后的受管升级 | pass | `.tmp/channel-live/retail-coordinator-reload-activation/` |
| 就绪 reload 零 Esc、禁用缓存、同请求只读恢复 | pass | `.tmp/channel-live/retail-coordinator-reload-verified/reload-readiness-resumed.json` |
| 显式 fallback 自动选择内存就绪路径并关闭连接 | pass | 同一报告，`CON-b81c478c1e2998fb16ede075d1871428` |
| 公开完整生命周期、槽位容量 reload、失败与清理恢复 | pass，20/20 | `.tmp/channel-live/retail-coordinator-public/baseline.json` |

原始失败记录保持原样，不以新的通过结果覆盖。

reload 基准初次解析错误使用了日志键名的大写拼法；修正后又发现“输入意图数”不能等同于
“实际发送数”。第二轮禁用缓存出现两次采样过期，均记录 `not_sent/messagesQueued:0`，
第三次才发送；这是预期的安全重试。最终沿原 CON 只读恢复并完成剩余验证，未重发 reload。
两条路径的实际 reload 都是 11 条消息、0 次 Esc。只读重复请求约 45–48 ms，
显式 fallback 的完整 reload/新绑定约 28.9 秒。它们不是脱离机器负载的性能承诺。

普通 Esc、Enter、`/reload` 输入不发布 LoD 槽位。reload 后重新绑定消耗新池的一个可用槽位（正常是第 1 个），
此绑定单独落盘并验证准确 nonce。

完整基准连接 `CON-8296f589cb8d5c21a5a0821d61183a3b` 已关闭，重复 disconnect 为只读成功。
13 个正常请求、容量自动 reload、跨日志段的旧请求读取、编译失败报告、清理失败触发 reload
并保留原报告、bugs 查询及收尾均通过。重读首个请求耗时约 108 ms，原日志字节不变。
全部本轮连接均已正常关闭；此前退出的 Classic 未决记录保持原样。

## 边界

本轮不将 Classic、Titan、Forever 或同一安装下多进程的历史通过结果升级为此次新编排验收。
这些新回归尚为 `not_run`；Forever 仍不在正式矩阵。第三方任意键盘拦截器、真实战斗中
输入竞争、极低后台帧率、首装新目录必须重启的客户端也不因模拟用例通过而标记实测通过。
采样新鲜度的 GetTime/Windows uptime 关系目前只在 Retail 实测。
