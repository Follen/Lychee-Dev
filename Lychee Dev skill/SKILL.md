---
name: lychee-dev
description: 编排 Lychee Dev 的 WoW 后台自动化任务、错误快照、二维码回执和 SavedVariables Ticket 读取；也用于有界 Run 性能调查、对象检查与事件监控。不用于无关插件实现或通用 WoW API 查询。
---

# Lychee Dev

把用户的调查问题变成完整、可追溯的游戏运行证据。已获授权的自动化应连续完成发送、读取、确认和收尾，不能停在“已发送”、二维码出现或脚本生成。

## 按任务读取资料

- 要操作正在运行的游戏：先读 [clients.md](references/clients.md)，确定版本、实例和安装位置。
- 要执行自动任务、获取最近 n 条错误、ACK 或恢复中断：读 [automation.md](references/automation.md)，按其闭环继续到完成。
- 设计内存/CPU 调查：另读 [runtime-investigations.md](references/runtime-investigations.md)。只读已有 Ticket 不需要准备新探针。

## 默认后台通路

使用随 Skill 附带的 `scripts/automation.py`。发送显式传 `--mode messages`，通过 PostMessage 投递到绑定的 HWND；WGC 直接捕获窗口，zxing-cpp 解码。两者都不要求窗口获得前台焦点，不使用 GDI 截屏，也不需要剪贴板。不要自行换成 SetForegroundWindow、SendInput、点击聊天框或要求用户粘贴。

工具仍保留显式 `foreground` 模式；它不是后台通路的自动降级方案。后台异常时检查状态和证据，不把失败直接转成人工提交。用户明确选择人工 Run 时，才按该方式组织。

无焦点要求不等于所有窗口状态都可用：最小化、客户端切换、战斗、尚未载入世界、其他游戏编辑框和自定义聊天键位可能影响操作。只报告本次实际验证的客户端/版本/状态。消息成功入队不证明游戏执行；二维码出现不证明 SV 已落盘。

支持重载握手的插件与脚本应配套更新。自动化重载使用 `reload --mode messages`：脚本本地识别本次 nonce 的就绪二维码并清理，只向 Agent 返回简短 JSON。正常流程不要采集整图交给 Agent 判断。`run`/`bugs` 的输出重载同样等待就绪；超时查看日志中的阶段和本地截图，使用 `reload --resume <nonce>` 继续等待/清理，禁止重新发送重载。该信号证明命令通路已在重载后进入世界，不代表任意第三方异步任务都已完成。

## 完成标准

一次自动调查需要取得匹配 task/requestId/Ticket 的完整报告，核对身份与校验信息、保存证据、确认 ACK，并完成该回执的界面清理。未完成的阶段必须明确说明。不要为了清理画面重跑任务。

- 接收已有 Ticket：先找精确记录，禁止重新执行原任务来“获取同样结果”。
- 新任务：只在目标安装目录 `Modules/Automation/auto/auto.lua` 更新自己拥有的 task-id 区块；同一次逻辑执行保持 requestId 不变。
- 错误调查：`bugs --count n` 获取最多 n 条已有错误，不制造新错误填满数量。
- 完整内容始终走 SV；超过 10 KB 不切换传输方式，不把正文塞进二维码。
- 测试也是完整工作：每个用例结束或中断时处理自己留下的草稿、回执与任务块，再报告状态；不要在没有解释的情况下留在下一用例的中间步骤。

保留当前会话已给出的授权，不重复请求常规步骤确认。调查授权不意味着可覆盖其他任务、重置数据库或发布插件；共享游戏实例存在实际占用时先协调。

## 已有证据与人工工具

SV 是数据：用受限解析器读取，不能在宿主执行 Lua。主记录位于 `LycheeDevDB.exports.records[TICKET]`，完整内容是 `payload.content`；`history` 与 metadata 都不能代替完整报告。未知/较新字段应保留，已有旧性能记录仍可作为历史证据。

用户选择手动操作时：Run 支持 `/run` 或 `/script` 和 Lua 5.1；异步调查使用验证过的完成写入机制。对象检查走 `/dev > Objects`，事件监控走 `/dev > Events`。没有独立 `/object`、`/event` 命令；Performance、自动性能采集、health scan 和 benchmark 页面已移除。脚本必须有界，异步资源用清理回调释放，不对可能为 secret 的值直接比较或格式化。

结果说明以用户问题的结论开头，附 Ticket、客户端实际版本、完整 payload 路径和验证边界。区分“提交”“游戏完成”“报告读取”“收尾完成”，不要用一个“成功”掩盖中间阶段。
