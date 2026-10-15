更新日期：2026-10-15，Asia/Shanghai。历史审计见[审计报告](docs/Clean-repo-2026-10-14.md)；本文件只记录当前交接。

## 当前交付：3.2.2 发行

- 用户已授权提交3.2.2、合并main、发布npm，并在发布后直接安装到本机。沿用`latest`渠道，`next`仍为3.1.2；不改动已有发行产物。
- 当前在`main`；[PR #16](https://github.com/Follen/Lychee-Dev/pull/16)已合并，实现提交`969fe48`，合并提交`0bec60e`。`v3.2.2`已固定该合并提交并推送。主代理负责最终发布及registry核验；此前功能实测和限定范围复核已完成。
- 版本源与发行渠道清单已同步为3.2.2，CLI/addon/npm派生文件已生成。本地build/vet、Lua 5.1必需完整Go套件（2922项pass事件、42项既有可选skip、0fail）、93项Node工具/启动器检查、版本/Skill/发行输入检查通过；PR必需[CI](https://github.com/Follen/Lychee-Dev/actions/runs/38723851897)及分支CI全部通过，远端已准备固定公开输入并验收。
- [正式发布流水线](https://github.com/Follen/Lychee-Dev/actions/runs/38725056854)全部成功，包含必需CI、CGO预检、正式包构建、源码重建比较、真实LuaLS/Source检查、Windows隔离安装、摘要门禁、OIDC发布和registry重新安装验收。独立registry回读确认`latest=3.2.2`、`next=3.1.2`，integrity和provenance元数据存在，不声称完成provenance密码学验证。[GitHub Release](https://github.com/Follen/Lychee-Dev/releases/tag/v3.2.2)六项产物齐全。
- 本机受管`update`返回`complete=true`，CLI、1份Skill及7个客户端addon均为`current`。PATH实际运行3.2.2、commit=`0bec60e`、workspaceDirty=false。Skill需新Agent上下文加载，游戏内addon需后续reload激活；本轮只同步文件，未新增Live连接，不计入新的七产品实机覆盖。已按限定绝对路径清理`.tmp/release-3.2.2`；本次发行、本机更新及临时清理均已完成。

## 当前任务：槽位自动启用与 Mailbox 重载

- 用户要求主插件加载时检查当前角色的500个槽位，启用被禁用的槽位，并通过Mailbox通知CLI重载。保留现有角色、原连接、输入不重放和重载验证合同；不修改其他插件的启用状态。
- 实现基线`6df4390`；实现和Retail实测已完成，已受管部署测试版，发行状态以上方3.2.2交付为准。主代理负责Go连接/重载、文档和最终验证；pr13_review_live已完成Lua启用检查、Mailbox字段及对应回归，pr13_review_delivery已完成Go限定范围只读复核，无可操作发现。
- Lua在现有启动元数据遍历内按角色GUID启用禁用槽，复查并统一保存；成功修改通过`Mailbox.input.slotReloadRequired`请求重载，失败走启动诊断。CLI在bind发布前处理请求，保留原CON、重载请求、业务交易和期限；已有重载的新runtime再次要求启用时可继续重载，submitted/uncertain仍不重发。没有新增轮询或批量加载槽。
- 已确认此次故障来自另一个角色沿用的旧槽位禁用配置；此前成功角色500槽均启用。本次实测前doctor确认所选PID无占用、0预约；本任务没有操作原LycheeGroup项目或其恢复记录。
- 已通过build/vet、正常Windows环境下的channel/addon/protocol整包（Lua 5.1必需，包并发2、测试并发4、包时限20分钟）、submitted/uncertain/missing outcome分别跨重启不重放的定向回归，以及Skill合同和文档本地链接检查。旧测试已按新增绑定前读取调整，保留原输入不重放和两份未决预约的恢复约束。
- 本次实现、离线验证及Retail 12.1.5.70391实测已完成：灵止光／死亡之翼原有17个禁用槽，部署后启动自动启用并保存；新鲜INPUT记录slotReloadRequired=true，CLI在同一CON及原期限内追加一次槽启用重载，新runtime绑定成功。此前另有一次测试版激活重载；两者分别只提交一次。游戏内探针确认500/500启用，角色AddOns.txt禁用数为0、其他插件设置无变化，报告verified/released并返回42。共8次已提交输入交换无重复，7次等待均0消息。CON-f68a01a481d7cd9100d0694ef9644988已正常关闭；最终doctor全绿、无owner、0预约。已清理本次实现和实测临时目录。实测结束时游戏为测试版addon，全局CLI/Skill仍为正式3.2.1；后续发行及本机更新状态以上方3.2.2交付为准。

## 已发布基线：3.2.1

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
- 七产品干净首次安装、重新登录及完整升级矩阵仍见[todo.md](todo.md)。上述发行验收结束时没有未关闭的测试连接；后续任务状态以上方当前任务为准。
- 3.2.1实现、审查、合并、发布及本机正式包更新均已完成。发布后的游戏内addon等待用户下次reload，七产品剩余实机范围按todo.md保留。
- 本轮临时目录`.tmp/keyboard-priority/`已按用户要求通过限定绝对路径的脚本删除；清理前已确认实验连接关闭、报告verified/released，无待恢复操作。删除1368个文件，共90338196字节，清理脚本随后移除。用户此前要求留下的其他任务材料、原ZIP、恢复记录及`.worktrees/rust-refactor`继续保留。
