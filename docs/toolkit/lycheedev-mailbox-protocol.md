# Historical Native Live input experiments

当前唯一协议为 [Lycheedev mailbox protocol v1](lycheedev-mailbox-protocol-v1.md)。
本页仅保留开发过程中的实测记录；旧 Esc、光学、三键和盲发重载方案均不再授权当前输入。

## 8. 实施与验收记录

本节记录同日早期 Esc 方案的历史验收，不代表当前两键方案的实机通过范围。
当前行为以新的 Lycheedev mailbox protocol v1 和实施状态为准。

当时的 Go/Lua 实现：统一协议标识，移除 Native 光学模块与捕获路径，Mailbox 发布 Esc 额度，
统一输入入口持久化计数。旧能力仅能进入受管升级 reload；已存在但不兼容的身份返回协议错误，
不能伪装成“尚未安装”并触发盲发 bootstrap。

Era `1.15.9.70003` / Interface `11509`，PID `29448`，角色“看么就看了你—匕首岭”，
实机验证完成。开发包版本仍为 `3.1.2`，这是 `3.2.0` 分支验证，不是 npm 发布。
连接 `CON-484fd6ca1a30f47ea83c731b9c933d3a` 已正常关闭。

| 实际行为 | 结果 |
| --- | --- |
| 从旧 hybrid.v2 经 Mailbox 升级 | 读取旧 INPUT，单次 reload（11 条消息），新协议 bind 成功 |
| 新协议与 500 槽 | 身份、输入、诊断统一标识；逐项验证 500 个槽的元数据；无 InputSignal/StartupBeacon |
| 单行编辑框，一次 Esc 释放 | 恰好一次，原文字及光标位置保留 |
| 多行编辑框，三次 Esc 释放 | 逐次读取，计数 1→2→3，原文字及光标位置保留 |
| 持续占焦点，三次仍不释放 | exit 6，`input_focus_recovery_exhausted`，保留原 confirm nonce |
| 新 CLI 进程立即 resume | 仍为 exit 6；输入 ID、nonce、计数不变，没有第四次 Esc |
| fixture 自行释放焦点后 resume | 原操作完成，业务报告通过，清理完成 |
| 无焦点阻塞的 no-cache reload | 零 Esc、一次 reload（11 条消息），新 runtime 重绑及随后执行成功 |
| disconnect / 安装检查 | closed=true、bound=false、managed、500 槽、pending=0 |

初始新协议 runtime 为 `000000040002d0ff11b117c77caa8483`，正常 reload 后为
`000000050002d16ef78334a7e28f3c04`。完整命令结果、输入日志及历史分段位于
`.tmp/live-mailbox-input/era/`；入口报告为 `mailbox-protocol.json`。
实机 runner 为 `tests/channel-live/mailbox-protocol-baseline.mjs`。
升级前后 WGC 图片保存在 `.tmp/live-mailbox-input/before/`、`after/`，只用于视觉检查。

离线验证覆盖新鲜度、身份与记录类型隔离、七类输入动作的 Esc 上限、零发送退款、
outcome 丢失与重启恢复、禁用/启用及加载生命周期、500 槽消费、Go/Lua 互通。
build/vet、memory、bridge、command、五 profile Lua、protocol、slotset、版本/Skill 检查及
14 项 addon 包装/隔离更新测试通过。channel 整包初次发现两个仍按旧合同编写的 fixture，
修正为发布 Esc 额度和按 wire kind 区分记录后，相关用例定向通过；其他整包用例无失败。
delivery 整包在默认 10 分钟期限内因大量落盘操作超时，栈停在 `FlushFileBuffers`，
不记为全量通过；相关 TOC 迁移及 bootstrap 修复定向验证通过（77.5 秒）。

本次没有重复整轮 500 槽容量压力、重新登录、首次无 Mailbox 安装、真实战斗/secret 场景，
也没有验证其他六产品或双 Retail 的新协议实机行为；这些项目保持 `not_run`。
此前旧协议的容量循环、双实例和其他客户端结果不计入本协议验收。

## 9. 正式双键与编辑框传播验证（2026-10-13）

国服无限测试服 `1.60.1.70338` / Interface `16001`，PID `61388`，角色“晴—岚”。
开发版本仍为 `3.1.2`，工作分支 `3.2.0`；未发布 npm。

依据固定 Forever 源码提交 `943764493e6b16d63ded3ab304150d1f05e58b57`：
[SimpleFrame 的 SetPropagateKeyboardInput](https://github.com/Gethe/wow-ui-source/blob/943764493e6b16d63ded3ab304150d1f05e58b57/Interface/AddOns/Blizzard_APIDocumentationGenerated/SimpleFrameAPIDocumentation.lua)
允许选择性传播键盘输入；[聊天编辑框焦点事件](https://github.com/Gethe/wow-ui-source/blob/943764493e6b16d63ded3ab304150d1f05e58b57/Interface/AddOns/Blizzard_ChatFrameBase/Shared/ChatFrameEditBox.lua)
用于即时接通当前编辑框。源码只提供机制，以下结果来自生产实现的实机调用。

| 实际行为 | 结果 |
| --- | --- |
| 普通 EditBox 保持焦点，CLI 完成一次操作 | 四次 F12 送达；文本、光标保持；Esc=0；清理后原 OnKeyDown 恢复 |
| 真实 ChatFrame1EditBox 保持焦点 | CLI 执行成功，INPUT 就绪且 forwarding=true |
| 真实聊天框有焦点时直接 F11 reload | 一次六条按键消息，新 runtime 重新绑定成功；不进入聊天命令 |
| reload 后读取正式键位 | 两个动作在 Lychee Dev 分类内；账号方案 1 保留 F12/F11，实际动作匹配 |
| 实机最终 runtime | `0000000f0002f8ce19ed275fc6611dd3`，500 槽 inventory |

结果在 `.tmp/live-shortcut-focus/verify-production-focus.json`、
`production-chat-reload.json`、`production-focused-reload.json`、`final-formal-keys.json`。
测试连接 `CON-a440f7592615b735c5e63afd35dbab27` 已正常断开，`closed=true`、`bound=false`，
结果保存在 `final-disconnect.json`。
早期 Shift+F12 重载实验实际触发 wake，属于失败；修改修饰键发送顺序未解决并已撤销。
改用独立 F11 后通过，上述失败记录没有改写为成功。

离线验证覆盖两键注册/保存、冲突保留、登录/战斗延后、焦点传播和恢复、受限编辑框拒绝，
以及 Go 发键和旧日志读取。build/vet、Lua 5.1 addon 与 protocol 整包、channel 与 desktop
相关测试、addon 包装和 Skill 命令合同检查通过；本轮未重跑全仓库测试。
其他产品、双 Retail、重新登录、真实战斗/secret 场景保持 `not_run`。
上述“尚未实现”状态已由新协议及当前实施状态替代。
