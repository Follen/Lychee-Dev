# Live 健壮性实施记录

2026-09-29；工作树 `codex/live-channel-recovery`。本记录覆盖架构复审后新增的主机改动，
不以此前 Retail 原会话恢复证据代替本批验收。未发布，未修改真实受管安装或输入游戏。

## 已实现

- 关闭目标在业务推进中生效。尚未发布 prepare 的操作标为 cancelled，不执行、不触发
  容量 reload；prepare 已接受但 commit 未发布时返回 `prepared_operation_requires_reload`。
  已有 prepare/commit/bind 未知交换仅取回原回执，不能因 close 补发业务。
- 原报告继续独立核验/持久化/释放。关闭不能将未知业务改写为成功，CLI 取消仅表示
  停止本次等待（exit 7），不表示 Lua 已取消；等待到期仍为 pending（exit 6）。
- 首次 bind 与 reload intent 的 pending 可进入同一 runtime 发现/恢复路径，不再受 Bound
  与 reload phase 的旧门控阻断。已有准确 bootstrap_changed 回执可释放对应 bind 预约。
  已知输入阻挡不触发全量发现；无进展恢复发现从扫描完成后冷却 1–30 秒，按实际扫描耗时
  让普通协议先推进。严格匹配但返回后过期的输入样本先重新观察，500ms 安全门槛不变。
  删除 reload 阶段独立扫描/选择候选的路径，统一由 Continue 发现后交给 RecoverRuntime；
  保留旧日志已处于 binding 阶段的完成兼容。自动容量 reload 继续原业务，显式 reload
  则按自身目标返回，两者不混用完成判断。
- reload 的输入资格继续核验固定 runtime、角色/build、owner/fence 和采样新鲜度，但
  不再要求旧 nextSlot 一致；槽位业务的精确槽号验证保持不变。
- 业务、显式 reload 各 600 秒，首次普通 bind 与首次 close 各 120 秒；首次安装/显式
  activation 为 600 秒并将原期限传给后续 bind。预算为绝对期限，包含调用间空闲时间。
  自动容量/清理 reload 继承原业务预算；重复 close/resume 不续期，close 有独立额度。
- 到期、检测到时钟回退后停止自动动作并持久记录；同次调用用单调 context 限制剩余
  时间。无法检测停机期间发生后又恢复的时钟变化，不宣称具备外部可信时钟。
- 结构化 continuation 区分继续、外部等待、活跃 driver、预算耗尽和需要决定。
  共享槽位预约返回准确 installation/slot/consumer/runtime/nonce 与解除条件；短发布锁
  超时是另一类 blocker。只报告依赖，不授予接管他人预约的权限。
- `progressVersion` 只随动作/结果等事实写入增长；预算观察和重复 blocker 不伪造进展。
  终态证据优先于历史 blocker，覆盖完成落盘后、清除等待信息前的崩溃窗口。
- Skill 沿原 CON/request 串行继续，解释具体依赖和剩余预算；删除性能参考中过时的 ACK
  编排。source/data 查询继续使用原固定引用链，不进入 Live 恢复逻辑。

## 日志与部署兼容

新写入使用 `lycheedev.channel.v2`；新增预算、进展、阻塞证据及 cancelled 终态。
v1 可只读；缺少预算的既有目标在首次驱动时获得一次带 Legacy 标记的有限窗口，先落盘
再输入。此前耗时无法还原，不假装从旧执行时间精确扣除。旧 CLI 的 schema 校验拒绝 v2，
不能降级它继续驱动新日志。未决请求仍沿原 CON 与 request，去重范围没有扩大到跨 CON。

activation 对应 `lycheedev.channel-activation.v2`，同样可读 v1、首次驱动迁移。
若旧 activation 早已交接成普通 channel 日志，则沿普通 channel 的 legacy 规则处理，
不回读激活文件伪造历史预算。迁移未改变游戏侧 slot.v1 格式，不要求覆写受管 addon。

预算耗尽后，状态/既有证据读取仍可用；有 OS 进程退出证据的精确资源退役不需游戏输入。
本批没有增加预算扩展命令、同步 Lua 强制抢占或跨 CON 接管。

## 仍然存在的明确边界

| 场景 | 当前行为/状态 |
| --- | --- |
| 同安装未知 slot 1 预约挡住新 bind | 明确返回预约依赖；正式 v1 协议仍可能阻塞 |
| 新 runtime 尚未绑定，游戏又换代 | `runtime_changed_during_recovery`，保留原交换与中间 bind，不覆盖它们追下一代 |
| prepare 已接受但无 commit，要求关闭 | v1 无 abort；等待已授权的显式 reload/准确 runtime 销毁证据，不以 commit 换退出 |
| 第三方界面/未登记副作用 | 不把 resourcesReleased 或 runtime 销毁说成外部状态已恢复 |
| 活跃 driver | 等原调用结束；没有抢占式取消通道 |
| runtime token 顺序 | 保留原候选筛选及 fresh bind，不将随机 token 或最大值当授权；异常 SV 回滚未新验收 |

共享槽位的有界集合目前仅在 `tests/slotset` 中，**没有接入产品**。
模型验证了按稳定实例分配额度、恢复预留、同槽共存、成员摘要及有限崩溃切点；
但四实例最大负载编码达 4,199,960 字节，超过现有 2 MiB 读取上限，Lua loader、冷启动
身份、生产 Lua/manifest 提交和实机性能仍未验证。按复审准入规则停止正式迁移，未放宽
现有读取上限、未知预约保护或宣称这些门槛已通过。详见 `tests/slotset/README.md`。

## 验证

已增加公开 Continue/日志恢复测试：关闭的八个 prepare/commit 持久化边界、首次绑定和
reload 前换代、槽位已消费而回执缺失、两次换代、重复请求预算、时钟回退、legacy 迁移、
完成事件落盘后的崩溃、具体发布 blocker。预算/activation 状态测试使用临时目录和注入时钟。

静态/离线验证：

- `go build ./...`、`go vet ./...` 通过。
- `LYCHEEDEV_REQUIRE_LUA51=1`，指定真实 Lua 5.1 解释器，`go test -count=1 -p 2 ./...`
  通过。首轮默认包并行运行曾在 slot-cycle 控制测试触发 10 秒夹具期限；修正为先完成
  安装夹具再开始 30 秒文件操作期限，并限制测试包并行度。没有放宽生产预算或删掉断言。
- 此后统一 reload 发现路径的变更通过对应恢复/关闭/自动容量测试；最后
  `go test ./internal/live/channel ./internal/command -count=1 -p 2` 通过
  （channel 47.414s，command 27.507s）。
- `node tools/skill-contract.mjs`：12 文件、87 合同、204 引用，0 violations。
- skill-creator `quick_validate.py`、`node tools/version.mjs --check` 与 `git diff --check` 通过。

Retail `12.1.0.69933` 同安装双实例已完成分阶段功能验收，版本、证据、延迟波动和
最终冻结 baseline 入口见[本轮验收](live-retail-dual-acceptance-2026-09-29.md)。
Classic/Titan 本批代码、first-install/upgrade/relogin、同条件性能对照与独立 Agent 行为验收仍为 `not_run`。
原型通过或离线全绿不代表整份架构方案已实施验收完成。
