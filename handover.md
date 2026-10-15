更新日期：2026-10-15，Asia/Shanghai。历史审计见[审计报告](docs/Clean-repo-2026-10-14.md)；本文件只记录当前交接。

## 当前任务：3.2.1

- [PR #15](https://github.com/Follen/Lychee-Dev/pull/15)已按用户授权合并，合并提交为`10b561db`；实现提交`83463a03`，最终PR提交`842823df`的[Windows CI](https://github.com/Follen/Lychee-Dev/actions/runs/38675723476)五项必需作业及汇总全部通过。`v3.2.1`固定合并提交，[正式发布流水线](https://github.com/Follen/Lychee-Dev/actions/runs/38676733974)全部成功；npm `latest=3.2.1`，`next=3.1.2`，已发布3.2.0产物不变。
- 本次范围为Retail 12.1.5/120105兼容与R1215-INPUT-01键盘优先等待，以及3.2.1版本和发布说明。主代理负责整合、文档、全部实机及发行；pr13_review_delivery已完成客户端/安装/包装审查，pr13_review_live已完成输入/恢复审查，两者均无可操作发现，未写文件或运行游戏。
- Retail基线固定Gethe live提交`18c6d677b605b312f3d813d498404631fb941977`（12.1.5.70391）；TOC、ClientGate、客户端基线与500槽声明新增120105并保留120100。安装按产品和构建系列匹配，旧槽通过既有受管路径迁移。
- R1215-INPUT-01已实现：Native持有被动键盘监听，所有键松开且连续500ms无事件后重新核验INPUT；忙时零消息返回原Driver等待，已开始组合键完成抬键，submitted/uncertain只等待原回执。不能保证消除与已开始组合键恰好重叠的误触。

## 已完成验证

- Retail兼容改动的build/vet、受影响Node 16项、版本检查及完整Lua必需Go套件通过（2891测试事件、2625叶节点pass、42skip、0fail）。这次完整运行早于键盘功能加入，不能代表最终3.2.1。
- 3.2.1最终build/vet、Lua 5.1必需完整Go套件通过：2902测试事件、2634叶节点pass、42skip、0fail、无未完成测试，包并发2、测试并发4、包时限20分钟。skip保留helper及本机缺少的固定输入/原始材料/权限；固定公开集成输入另由本次PR及发布CI验证，不倒填本地结果。
- Node工具和启动器92项通过，唯一缺archive的跳过项使用固定LuaLS archive单独补跑通过，共93项实际通过。版本、Skill合同/生成命令、发行源码输入检查通过。键盘改动此前desktop/channel整包及定向race通过，Windows被动监听实测收到按键/松键并满足500ms静默。
- 12.1.5实机受管升级后，125份报告全部verified/released、500个数值往返比较通过；prepare/commit/confirm/release各125次，另2次bind、1次unbind、1次自动容量reload，共504次输入交换各提交一次。跨两个runtime，不称单runtime逐个执行500个物理槽。
- 按用户要求，本机全局CLI通过本地开发包升级并同步受管Skill和Retail。用户在游戏按住W约4秒，两次held检查均0消息；松开后469ms仍等待静默期，随后完成原请求。四个业务交换各提交一次，报告verified且返回42，原期限不变。
- 两轮实机连接均正常disconnect；当时最终doctor全绿、无owner、0预约。本机已从3.2.0开发包切换到npm正式3.2.1；PATH CLI确认commit=`10b561db`、workspaceDirty=false，受管Skill与Retail addon更新complete=true。Skill供新Agent上下文使用，游戏内addon需下一次reload激活；本次发布后未再操作游戏或新增CON。

## 7项实机结果

以下为此前3.2.0修复在Retail 12.1.0的已完成范围，不代替本轮版本检查或七产品完整安装矩阵：

| ID | 实际检查及限制 |
| --- | --- |
| R3-01 Trace | 真实页面回调错误、自有方法调用与停止；非硬件点击，临时hook已reload清理 |
| R3-06 拾取器 | 用户真实进入/退出战斗；dock、键盘脚本及输入保护关闭，F/Enter/Esc正常 |
| R4-01 输入与恢复 | 完整交易、running原请求resume、28次交换及采样间隔；真实崩溃/缺outcome仍仅离线覆盖 |
| R4-04 bridge状态 | 默认option、off/on、保存off后reload及恢复新runtime连接 |
| R4-07 Objects | 真实页面错误处理、5000字partial/limited、健康对象恢复；临时快照已reload清理 |
| R5-01 数值 | 四种double与%.17g文本往返相等，报告verified/released |
| R20 连接故障 | 公开map固定schema后新runtime空回执及完整交易通过；旧3.1.2原包唯一根因仍未知 |

## 发行与剩余范围

- 3.2.0已由[PR #13](https://github.com/Follen/Lychee-Dev/pull/13)合并后发布，发行标签`v3.2.0`固定`3057a966`；[发布流水线](https://github.com/Follen/Lychee-Dev/actions/runs/38608441597)及[GitHub Release](https://github.com/Follen/Lychee-Dev/releases/tag/v3.2.0)完整成功，`next`仍为3.1.2。
- 3.2.1发布流水线完成合并提交CI、真实LuaLS/Source、源码重建、Windows正式包安装运行、摘要校验、OIDC发布、registry回读和重新安装验证；[GitHub Release](https://github.com/Follen/Lychee-Dev/releases/tag/v3.2.1)六项产物齐全。独立registry回读确认版本、latest渠道、integrity与provenance元数据存在，不声称完成provenance密码学验证。
- 七产品干净首次安装、重新登录及完整升级矩阵仍见[todo.md](todo.md)。当前没有新确认缺陷或活动游戏连接。
- 本次实现、审查、合并、发布及本机正式包更新均已完成；当前无需继续代码修改。游戏内addon等待用户下次reload，七产品剩余实机范围按todo.md保留。
- 本轮结果与本地重装包保留于忽略的`.tmp/keyboard-priority/`；观察程序已删除。构建stage、缓存与LuaLS下载包的清理被自动审批策略拒绝，暂留。保留用户此前要求留下的材料、原ZIP、恢复记录及`.worktrees/rust-refactor`，不清理其他任务资源。
