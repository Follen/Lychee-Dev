# 后台自动化闭环

检索：目标绑定、任务区块、状态、执行、错误快照、ACK、恢复、清理、证据。

## 工具与协议

发送使用 Python `scripts/automation.py` 的 `--mode messages`（当前默认）；读取使用 WGC + zxing-cpp。不要重写 ctypes 投递、拼接临时按键脚本或改成 GDI。Node CLI 负责安装/发现，游戏与 SV 逻辑由 Python 持有。

运行时 RGB 色块表示状态；完成 QR 只包含版本、Ticket、task、requestId、时间戳等短元数据：

```json
{"v":1,"ticket":"LYCHEE-...","task":"probe","run":"req-...","ts":1234567890}
```

无论正文大小，一律在输出 reload 后读取 SavedVariables。二维码不承担正文传输。成功/失败/取消都可能生成报告，必须读取报告的 status/complete/incompleteReasons，不能看见二维码就说调查成功。

固定一个稳定的 `--installation` 标识（对应安装目录与会话绑定），并在所有命令中沿用同一个 `--data-dir`。宿主默认目录是 `%LOCALAPPDATA%/LycheeDev/automation`，保存 `session.log.jsonl`、配置和 `received/<Ticket>/` 下的 evidence/report/content 文件。插件内存中的数据库只有在 reload、退出或登出后才落盘。

## 职责和完成边界

Agent 负责目标选择、设计有界 Lua、连续推进各阶段、解释异常。Python 负责注册表原子更新、输入、QR 校验、reload 去重、SV 校验和 ACK。当前不存在一个涵盖所有步骤的 `workflow` 命令；用下面的真实命令连续编排，不让用户逐条接力。

| 阶段 | 通过证据 | 下一步 |
| --- | --- | --- |
| 目标绑定 | 本次进程与产品/角色/安装匹配 | 写自己的任务区块 |
| 源码已准备 | upsert 的 hash/requestId/revision | 输入侧 reload 使新源码加载 |
| 输入已提交 | command_submitted，仅表示投递完成 | 等待匹配 task/run 的完成 QR |
| 游戏已完成 | 匹配 QR | 输出 reload 一次 |
| 已落盘读取 | 精确 Ticket、环境、完整性和校验通过 | ACK received |
| ACK 已确认 | Ticket/nonce/status 匹配 | 按该 nonce 清理并观察码消失 |
| 已收尾 | ticket_ack_cleared；自有测试资源处理完成 | 向用户报告结果 |

`run`/`bugs` 当前负责到 SV 读取，不自动执行 `ack`。返回 0 后必须继续 ACK。`send` 返回 0 不能当作游戏成功。`recover` 没有未决 reload 也不证明 ACK/界面清理完成，需要检查对应阶段事件。

## 准备任务

目标目录是 `<addon>/Modules/Automation/auto/auto.lua`，按 task-id 分区。只能通过注册表工具改自己的区块，保留别人的块和标记外文本：

```text
python <skill>/scripts/automation.py --installation <binding> task upsert --install-dir <addon> --id <task> --source-file <probe.lua> --request-id <request> --revision <revision> --expected-interface <interface>
```

task-id 是 `[A-Za-z0-9_-]{1,64}`；requestId 建议只用字母、数字、点、连字符（兼容插件与 Python）。一项新执行分配新 requestId，源码变化同时换 revision；超时恢复保持原 requestId。更新已有块时用 `--expect-revision` 防覆盖。

源码是 UTF-8 Lua 5.1，最多 256 KiB；注册表最多 16 块/1 MiB。标准输出限制默认 384 KiB、最大 512 KiB，以实际安装工具限制为准。不要用不受支持的 schema/kind 扩展协议。区块只注册数据，不会加载即执行。

同步调查返回紧凑结构。异步调查调用 `SetAsync()`，通过 `Finish(...)` 或 `Fail(message, code)` 完成，用 `OnCleanup(fn)` 释放自己创建的资源，检查 `IsCancelled()`；`Log` 与 `print` 输出有界。不得返回尚未完成的表并指望后来修改自动保存。

输入侧 reload：发送 `/reload` 后等待客户端恢复可交互状态再发执行命令。SV mtime 变化只证明开始写盘，不证明加载已结束。用 WGC 观察世界画面恢复或验证过的就绪信号；固定等待数秒不能单独作为就绪证明。不得因暂时加载中而连续发命令试探。

## 执行已绑定账户的任务

下面示例的全局 `--data-dir`（如有）放在子命令前。全部窗口操作沿用本次绑定：

```text
python <skill>/scripts/automation.py --installation <binding> send --mode messages --hwnd <hwnd> --pid <pid> --exe-path <exe> --text "/reload"
python <skill>/scripts/automation.py --installation <binding> run --mode messages --hwnd <hwnd> --pid <pid> --exe-path <exe> --task <task> --install-dir <addon> --sv <verified-sv> --timeout 120
```

不要把两条命令无条件贴在同一个连续执行脚本里：输入 reload 后需先确认就绪。`run` 读取安装区块的 requestId/revision，提交 `/dev auto run <task>`，过滤旧完成码，记录输出 reload 去重键，再校验 SV 并保存完整报告。

账户未绑定时按 [clients.md](clients.md) 分段执行：`send` 运行、`capture` 取码、Agent 验证 task/run、带 requestId/Ticket 发送一次 `/reload`、定位精确记录，再 `sv read`。`capture` 本身只验证 QR 格式，不知道本次预期 task/run，不能拿任何码直接触发 reload。

```text
python <skill>/scripts/automation.py --installation <binding> send --mode messages --hwnd <hwnd> --pid <pid> --exe-path <exe> --text "/dev auto run <task>"
python <skill>/scripts/automation.py capture --hwnd <hwnd> --pid <pid> --exe-path <exe> --timeout 120
python <skill>/scripts/automation.py --installation <binding> send --mode messages --hwnd <hwnd> --pid <pid> --exe-path <exe> --text "/reload" --request-id <request> --ticket <ticket>
python <skill>/scripts/automation.py --installation <binding> sv read --sv <matched-sv> --ticket <ticket> --task <task> --request-id <request> --revision <revision> --request-type task
```

同一个 `(installation, requestId, Ticket)` 的输出 reload 只允许自动发一次。写盘暂时未完成时进行有界读取等待，不重跑 Lua。

## 最近 n 条错误

```text
python <skill>/scripts/automation.py --installation <binding> bugs --mode messages --hwnd <hwnd> --pid <pid> --exe-path <exe> --count <n> --request-id <request> --sv <verified-sv> --timeout 120
```

n 在 1–100 之间。该流程发送 `/dev auto bug <n> <request>`，完成码 task 为 `bug`，内层报告 requestType 为 `bug`。数量是上限；按 provider storage order 读取可能跨会话的现有日志，不能未经核对就声称是本次测试产生的 n 个错误。报告保留 scope/排序信息，不用 metadata 代替错误正文。账户未知时同样分段定位 Ticket。

## ACK 与正常收尾

SV 内容已完整验证后发送 received；读失败但已明确决定结束本次接收才用 failed，不把一次暂时读盘失败直接 ACK 为失败。

```text
python <skill>/scripts/automation.py --installation <binding> ack --mode messages --hwnd <hwnd> --pid <pid> --exe-path <exe> --ticket <ticket> --status received --timeout 30
```

输出 reload 后先确认世界已恢复。ACK 分配新 nonce，等待含该 nonce、Ticket 和 outcome 的 QR；随后发送 `/dev auto unidentify <nonce>`，只隐藏对应 ACK 标记，再观察该码消失。`ticket_ack_confirmed` 和 `ticket_ack_cleared` 分开记录；确认成功但清理超时不等于任务失败。

旧版插件只支持两参数 ACK，不支持 nonce 清理。使用前核对工具与插件协议版本；授权的测试/更新应备份后同步相应源码并 reload。不能在旧插件上持续重试不支持的命令。`/dev auto unidentify` 无参数会隐藏身份标记，仅在明确属于自己的标记且没有新回执时用；不能用通用 stop 掩盖未处理证据。

读取 ACK 不需要又一次输出 reload；receivedAt/receivedStatus 留在内存，下一次正常 reload 才持久化。不要为了把确认再确认一遍制造无限 ACK/reload 环。

自有临时任务结束后用 `task remove --install-dir <addon> --id <task>` 清理磁盘区块，保留证据和他人区块；是否额外 reload 清除内存定义按实际需要决定，不为界面整洁无条件多做 reload。

## 完整报告校验

读取当前 `Lychee Dev.lua`，用受限 Lua 数据解析器定位 `LycheeDevDB.exports.records[Ticket]`。校验 envelope schema/ticket/createdAt、payload 的 UTF-8 字节数与编码、Adler-32、metadata taskId/executionId/revision、内层 schema/status/complete/requestType。Agent 另将 environment 的角色/服务器/产品/build 与当前绑定比较；当前 reader 验证环境结构，不替 Agent 证明它就是选定窗口。

`received/<Ticket>/content.json` 是字节精确的完整 payload，report.json 是解析报告，evidence.json 是外层证据。报告给用户时引用实际文件与 Ticket，不能只给 QR 截图或日志摘要。

## 中断、超时与恢复

先读本 installation 的 `status`/`recover`、本次保存的 task/request/Ticket 和当前画面；不要从第一条命令重新开始。

| 停留状态 | 恢复动作 |
| --- | --- |
| 已 upsert、未确认加载 | 核对源码与客户端状态，完成输入侧 reload，不运行旧区块 |
| 已提交、无完成码 | 有界等待/只读观察；超时记 unresolved，不自动生成新 request 重跑 |
| 完成码在屏幕 | 校验 task/run；查是否已请求 reload、Ticket 是否已在 SV，再决定推进 |
| 已请求 reload、读不到 Ticket | 等待写盘并搜索该安装所有候选；拒绝 newest-file 猜测 |
| SV 已验证、ACK 未完成 | 只继续 ACK，不重新执行任务或输出 reload |
| ACK 已确认、码仍显示 | 从日志取原 nonce，仅补对应 unidentify 和读取验证 |
| HWND/PID/角色变了 | 重新绑定，再恢复只读阶段；不沿用旧窗口序号 |
| 留有自有未提交草稿 | 核对仍是原窗口，取消草稿；不按 Enter“继续” |

仅在证据证明上次 reload 未执行且任务仍在内存，并处于现有授权内时，允许一次明确记录的恢复 reload；不循环试错。游戏崩溃前未落盘的内容可能丢失，应直接说明。

后台 PostMessage 不使用前台/剪贴板，但队列接受不证明 Escape 被消费或焦点处于聊天框。不要把 SendInput 当补救。输入错误后检查状态，修复明确原因或报告阻塞；不要留下未说明的测试草稿再开始别的用例。

退出码：0 为该命令完成，1 为参数/绑定错误，2 为注册表错误，3 为 SV 读取或验证失败，4 为窗口/输入/依赖问题，5 为回执、写盘或清理观察超时。已经确认的阶段不能因后续阶段失败而被抹掉。

## 依赖与验证范围

Python 图像依赖放在 `~/.lycheedev/python`，通过 PYTHONPATH 加载，使用随工具 requirements.txt 的版本；不要装进用户 site-packages。后台发送只依赖 ctypes；QR 读取及 ACK 的确认/清理需要 WGC、numpy、zxing-cpp。缺依赖时先在受管目录修复，不切换传输通道。

已有实机证据：Retail 12.1.0.69875 / Interface 120100 后台身份 QR、13,019 字节任务报告、reload 落盘和 nonce ACK 已通过。四客户端离线矩阵通过不等于四客户端实机通过；最小化、全部编辑状态、句柄重用仍未完整验证。同一 Retail 实例的 ACK 自动收尾已实测：确认后按 nonce 隐藏，WGC 再读取无残留码；测试禁止调用前台激活、SendInput 与剪贴板入口，仍成功。不要将该结果扩大为所有客户端/窗口状态都通过。
