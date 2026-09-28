# Classic / Forever 输入编排实机补测

日期：2026-09-28。分支 `codex/nonce-memory-transport`，2.5.1 本地候选，未发布。
本轮目标是同机两个独立安装的实例；不等同于同一安装目录共享槽位池。

| 实例 | 本轮固定身份 | 安装 |
| --- | --- | --- |
| Classic | PID 67816，5.5.4.69934 / 50504，次年雪—祈福，Player-4778-073BD91C | `_classic_`，wow_classic |
| Forever | PID 14056，1.60.1.70009 / 16001，Auto—Forever，Player-4613-00EE6DAE | `_classic_beta_`，wow_classic_beta |

角色均经新 nonce 绑定确认；目录名不作为身份。两端 version.txt 缺失，使用 .flavor.info
和可执行文件/build catalog 识别。Classic 不是 Titan。Forever 仍属该 build 的实验验证。
安装采用受管升级，随后公共 `live reload fallback` 激活，并验证新的 runtime、角色与输入状态能力。

## 本轮修复

旧 Classic PID 28276 已退出，旧 CON 的 window claim 仍使受管升级报 resource_busy。
新增 `live disconnect` 的进程退出收尾：固定 PID/创建时间经 OS 确认结束后，持有原连接锁，
先记证据，再在发布锁内退役精确槽位，最后释放原 claim，不发送游戏输入。
权限错误和找不到窗口不会被当作进程退出。中断的日志尾也可由同一 disconnect 恢复。

旧 `CON-8c57f45bd295970b55aaf7582b85da83` 原操作停在 confirm_ready。
公共 disconnect 返回 process_absent / closed=true，但保留 execution_unknown、
reportState=unavailable、complete=false；未把候选报告升级为确认结果。
原 `.tmp/channel-live/workbench-before-final/.lycheedev/live` 保持完整证据链。

第二个问题属于实机测试脚本：stdout 的 Buffer 分块被分别转为字符串，中文 UTF-8
跨块时产生替代字符。Forever 大结果的原始 CAS 报告（90,211 字节）与预期完全一致；
失败的是测试采集层。所有 channel-live runner 已启用流式 UTF-8 解码。
原失败报告保留；恢复沿原 CON 和 request 只读回取完成结果，未重跑探针。
恢复脚本第一次错误给 status 传 wait-seconds，参数校验拒绝且未输入；该失败也保留。

## 验证与证据

证据根目录 `.tmp/channel-live/`。实机结果与离线测试分开记录。

| 验证 | Classic | Forever | 证据目录/文件 |
| --- | --- | --- | --- |
| 受管升级、reload、新身份绑定 | pass | pass | `classic-coordinator-activation`、`forever-coordinator-activation` |
| 并发异步请求与结果身份隔离 | pass | pass | `classic-forever-coordinator/*/concurrent-async.json` |
| 第二项目争用同一进程被拒绝 | pass | pass | `classic-forever-exclusivity/*-conflict.json` |
| 禁用缓存的大结果、超时收尾 | pass | pass（原 CAS 与恢复采集均校验） | `classic-forever-coordinator` |
| 执行中 reload：observation 续跑、opaque 不重跑 | pass | pass | `classic-forever-coordinator` |
| 单/多行编辑框：恰好三次 Esc，文本/光标不变 | pass，6/6 | pass，6/6 | `{classic,forever}-coordinator-focus/focus.json` |
| 内存就绪 reload 与 fallback，幂等重试 | pass，6/6 | pass，6/6 | `{classic,forever}-coordinator-reload/reload-readiness.json` |
| 公共完整生命周期与容量换代 | pass，20/20 | pass，20/20 | `{classic,forever}-coordinator-public/baseline.json` |

两端的两种就绪 reload 均只提交一次，11 条输入消息、0 次 Esc、0 次零发送重试。
原请求只读重试两端均约 38 ms；性能数字只代表本次机器负载。

WGC：Classic 并发运行截图位于 `classic-forever-coordinator/classic/evidence`。
Forever 首张按固定时间采集的截图未覆盖运行阶段，不能作为指示器通过证据；补拍
`forever-coordinator-resumed-running/evidence`，并保存同刻 running / attempt=2 的日志状态，
确认左上角荔枝和“Agent 运行中”，无接收面板或二维码。

离线回归涵盖进程仍存活/PID 复用/退出、无效身份、取消观察，未确认结果不提升，
精确 owner 退役、重复关闭不写新事件、截断日志尾恢复、发布前后崩溃的槽位退役、
拒绝外部 consumer、保留 consumed=false、新预约不继承退役标记。

最终 `go build ./...`、`go vet ./...`、强制 Lua 5.1 的 `go test -count=1 ./...` 均通过。
完整输出为 `classic-forever-final-go-tests.txt`。Skill 合同检查 87 个命令、204 处引用，
零违规，skill-creator quick_validate 通过，版本检查保持 2.5.1。
双端组合回归的完成清单为 `classic-forever-coordinator/report-resumed-1790573793413.json`，
它引用原 report.json 中已经完成的 Classic 步骤；原失败结果未覆盖。

本轮连接全部关闭。最后分别核对两端受管 addon、64 个受管槽位、pending=0、
window owner claim 数为 0；最终 WGC 留在 `{classic,forever}-coordinator-final/evidence`。
最后的公共基准连接分别为 `CON-6c2f5a97740c01a89f648d17d79e36ba` 和
`CON-429211dfc286bb56bc3bf67362b8cdd8`，disconnect 重试均只读成功。
最终两张 WGC 已检查：荔枝、接收面板与输入保护均未遗留在场景上。
诊断完整读取：Classic 0 条；Forever 6 条均来自旧 session 1/2/8，当前 session 37，
未在本轮伪造清空旧错误，也未把故意制造的探针失败当作通道故障。

## 边界

未覆盖同一安装目录双进程、真实战斗输入竞争、所有第三方键盘拦截器或低后台帧率。
未重新验收 Titan，不修改正式矩阵，不据此声明零错误率或可发布。

后续正式服同目录双开已另行补测，具体竞态、修复和仍未覆盖范围见
[共享安装记录](live-input-shared-installation-2026-09-28.md)；不倒改本次独立安装测试的范围。
