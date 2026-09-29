# 三块混合输入实施记录

2026-09-29，`codex/live-channel-recovery`，未提交、未发布。
对应[架构方案](live-hybrid-architecture-2026-09-29.md)。本记录区分源码实现、离线验证和实机结果。

## 已实现

- 游戏客户区左上三块，每块目标 2×2 物理像素，总 6×2；前两个块各使用红、绿、蓝、黑、白颜色集合，第三块为白/黑心跳。
  四个合法码为 ready=绿白、普通键盘焦点=红蓝、战斗=蓝红、unknown=白黑。
  任意单个数据块换色不会得到另一合法状态；这不是认证，也不能检测所有双块误读。
- 一个 frame、三个纹理，InputState 统一控制周期、世界事件、显示和清理。
  保留常驻 1s 采样及即时 Refresh，不移动周期计时。缺角色/时钟异常、离开世界、加载画面隐藏信号。
  bridge off 移除回调/事件，同时 Stop StartupBeacon；迟到 Refresh 不复活。
- 新 Identity 声明 `lycheedev.input.hybrid.v1`，内存记录仍为 `lycheedev.input.v1`。
  Native 先读光学门禁，准备实际动作时仍读取完整内存身份及样本；两者矛盾不发键。
  发键前保留原地址重读、runtime/owner/fence/slot/GUID/build 和 500ms 样本门槛。
  内存的 `after+100ms` 不由画面时间代替。临近输入只检查现成帧，不在输入锁内等待 capture 或全扫。
- WGC 绑定固定窗口，只复制核验后的客户区左上 ROI，使用实际物理边界，不能猜标题栏高度。
  同一个 Native 只有一个事件驱动的 capture，容量为最新一帧，Close 停止并释放；runtime 变化重建观测资格。
- 光学帧有效期 500ms、心跳边沿窗口 1500ms；首次看到 ready 不能发键。
  缺块/非法色/过期/乱序均撤销资格。帧高水位不随普通失效清零，坏色帧也推进有效的时间高水位。
  动作后还要观察到新的心跳边沿；重启 CLI 不复用日志中的光学资格。
- 心跳缺失或 unknown 仍保留 Driver 的有界 runtime 发现出口，不把观测失败解释为旧 runtime 仍在。
  新能力不能因缺块转为盲输入；未知非空能力明确拒绝，activation 同步识别新能力。
- 前三项通用内存优化保留；nearby / 2s 等待 / 1500ms 线索仍服务动作前身份读取，没有误删。

## 清理与编排

已撤销未接线的 64B / 512 格实现、InputSignalProtocol.lua、旧 golden 和 TopCenterCapture。
现有 input_color.go、InputSignal.lua 被三块实现替换。未删除仍有公开命令/UI/历史依赖的 OP/BTP/QR 模块。
捕获时钟助手有真实输入与只读验收消费者，不是无调用的新抽象。

Skill 的四个入口文档已同步为混合观测：CLI 负责输入和恢复，Agent 保留固定目标、原 CON/request、
持久预算，读取 continuation；不手动看色发键、不删预约、不重放 unknown，分别判断 report.ok 和 cleanup。
使用 skill-creator 做基础验证，并以四组原始实机 JSON 做独立行为评估。
评估未执行游戏命令，结果保存在 `.tmp/hybrid-skill-evaluation-20260929/evaluator-response.md`：
准确识别持久预约阻塞、断开后业务未知、534ms 探针断言失败和两个实例独立编排；没有编造缺失 PID 或任务。

## 已验证与新发现

- Lua addon 完整 fixture：24 suites × 4 profiles = 96 子测试，另 2 个 Go 契约检查通过。
  四 profiles 的模拟通过不是四种真实客户端验收。
- bridge/desktop 全包、channel/protocol/memory 定向与受影响全包检查通过。
  覆盖 25 个颜色组合、单块误码、缺块/灰色/透明度、心跳定格、失效后回放、动作后屏障、
  阻塞时不扫描输入内存、扫描后再查色块、内存身份/时效失败不被覆盖，以及关闭 capture。
- `.tmp/hybrid-input-20260929/offline/report.json` 第一批完整基准 passed：build、vet、
  强制 Lua 5.1 全量 Go、Node、版本、Skill contract 和生成命令检查均通过。
  随后的实机帧时间修订已由 `offline-final/report.json` 再验：8 项检查全过，
  Go 42 包 / 2402 测试事件，33 项依赖环境或辅助进程检查跳过（以报告列表为准）。
  此后新增的快捷键冲突修复与 capability 错误分类修复不继承该批通过，另行记录最终验证。
- 两个正式服进程已重新发现：PID 9480 / 34876，build 12.1.0.69933，共用 `_retail_` 安装。
  固定进程创建时间及窗口证据、修改前 WGC 截图保存在 `.tmp/hybrid-input-20260929/before-*`。
  旧 PID/角色记录不能当作这次的新进程身份。
- 实机只读 ROI 预检触发 `desktop.invalid_frame_time`。原始 clock-probe 记录显示部分 WGC 时间标签
  超前当前 QPC 约 3–6ms；不能据此推断微软内部原因，也不能直接放宽输入门槛。
  修订仅在 InputSignalCapture 交付帧前执行一次有界等待：超过 50ms 拒绝，取消退出，醒后重查时钟，
  不修改原帧时间、不发布仍在未来的帧。after 记录 5/5 帧年龄非负；门禁仍严格拒绝未来时间。

## 共享槽位仍有独立门槛

本次增加真实 Lua 5.1 loader 原型，复用生产 SlotProtocol/SlotRuntime，详见
[原型说明](../../tests/slotset/README.md)。6 个顶层测试及 6 个持久化故障子测试通过。
已验证模型内双 runtime 唯一选择、歧义/无匹配/解析失败耗槽、旧协议拒绝集合及零业务执行。

原型同时暴露生产缺口：如果解析在 Receive 前抛错，当前 SlotRuntime 不记录该槽已消费，第二次 Wake 会再次尝试。
真实 WoW 是否已把这种 LoD 标为 loaded、生产 payload/manifest 双文件发布、未发现 runtime 碰撞和升级隔离仍未验。
4 实例原型最大成员约 4.2MB，超过生产 2MiB 读取上限；未直接扩大上限或切换生产协议。
因此不得宣称共享同槽未知预约已完全隔离。三块输入不改变该限制。

## 实机组合验收

候选 `package-final` 主机二进制 SHA256 为
`d57a3dd612aec3327a321e908df0fbfc0098680dc338ff5c848fff06e69f7868`。
两个正式服实例均完成由旧 input.v1 到 hybrid.v1 的 runtime 切换，尚未完成新绑定。
`after-signal-9480` / `after-signal-34876` 各 40 帧均成功解码，分别观察到 8 / 7 次心跳变化；
画面年龄分别为 0–7ms / 0–1ms，状态分别为 keyboard_focus / ready。
这证明本次 2560×1440 客户区左上 6×2 ROI 能读取，不证明全部显示配置。

实机暴露快捷键冲突：次年雪账号已有 `ALT-CTRL-F12 TOGGLELYCHEE`，ReceiverBindings 会拒绝安装，
但此前 InputState 仍能报告 ready。首次 bind 唤醒后，WGC 记录到 Lychee 搜索框，bind 回执未返回。
原连接 `CON-c5709ace7cc7f2e9b2f09977735ea7c7` 的 slot 1 已发布、输入已提交，不能推断未执行而重放。
人帅网卡连接 `CON-2da6f56586e7d960508cabc78d23312e` 因该精确预约返回 `wait_external/slot_reservation`，
没有覆盖 A 的槽位。原始 JSON、journal、WGC 与信号采样均在 `.tmp/hybrid-input-20260929/`。
此前只支持通过进程退出证据收尾；本轮已补入[同进程换代退役](live-runtime-retirement-2026-09-29.md)，
原双实例连接均已不发键关闭，无需退出客户端，也未手改预约。

该缺口已在源码修复：采样前核验接收快捷键仍由本通道持有，冲突/失效映射到现有 unknown，
内存记录 `input_binding_unavailable`，不新增颜色状态，也不替换玩家绑定。
Skill 同步保留原 CON、禁止重复 wake 和未知输入重放。
真实 ReceiverBindings / SlotRuntime / InputState 链路的 RED/GREEN 测试覆盖初始冲突、绑定抢占、
恢复及非原生路径隔离；addon 全包为 25 suites × 4 profiles = 100 Lua 子测试及 2 个 Go 契约检查通过。
未知 capability 的命令错误映射已修为 exit 3，裸码及附详情分类测试通过。
两项最后修订的完整组合基准输出为 `.tmp/hybrid-input-20260929/offline-binding-final/report.json`，
开发候选包为 `.tmp/hybrid-input-20260929/package-binding-final/`，不把旧实机二进制的证据转记给它们。
该最终离线基准已 passed：build、vet、Go 全包及强制 Lua 5.1、Node、版本、Skill、生成引用及
SOURCE 共 8 项检查通过；Go 42 包 / 2406 测试事件（33 跳过），Node 88（1 跳过）。
基准源摘要为 `8788274c3ef16d1c2f7c552c625c848dab64ffcbb8e89212c5be4dfe05ce339b`；
此段验收结果在测试后补记，之后只修改文档。
新开发包 CLI SHA256 为 `423f49e42ab526c077e8a2db9cc6f75de5408eae33ea8e2e0c344fbc4065c44e`，
尚未安装或做实机验收。独立绑定冲突 Skill 评估保存在 `skill-binding-evaluation.md`：
正确保留 A 未知输入和预约、B wait_external，不重发、不删槽、不声称关闭成功。

状态：原进程占用已通过独立退役解除，快捷键冲突修复的部署与复验另行记录。尚未以最终候选完成双实例公共基准、焦点恢复、
reload、禁用/启用、低 FPS 与像素几何矩阵。共享槽位故障隔离本次明确未通过。
当前不据离线通过声称 HDR、全部 DPI、最小化可用性或延迟提速；Classic/Titan 新混合路径仍为 not_run。
后续结果必须绑定同一候选源码与二进制，并保留失败和原 CON 恢复证据。

后续部署与验证见 [runtime 退役记录](live-runtime-retirement-2026-09-29.md#部署后的追加验收)：
绑定门禁已受管安装；A 20 步公开基准通过，B 前 18 步通过而关闭失败保留，
随后修复关闭与恢复调度；B 旧进程退出后旧 CON 已关闭，新 PID 上最终候选正常流程通过。
最终完整离线基准通过。以上更新不覆盖 DPI/HDR、低 FPS 或共享同槽故障隔离的未验结论。

用户要求重试后的检查：PID 9480 的创建时间未变化，只读发现已换为灵止光 / 死亡之翼，
runtime `00000054000017af9ff5f68499386a67`，GUID 与原次年雪连接不同。
原 CON resume 返回 `budget_exhausted`，没有新输入；证据 `retry-a.json` 及 `retry-discover-a/`。
换角色不是原进程退出，也不是把原连接改绑到新 GUID 的授权；原预约仍须经公开恢复精确退役。
