更新日期：2026-10-15，Asia/Shanghai。详细历史问题与证据见[审计报告](docs/Clean-repo-2026-10-14.md)；本文件只记录当前交接。

## 分支与交付

- [PR #13](https://github.com/Follen/Lychee-Dev/pull/13)已按用户授权合入`main`，合并提交`22043f9faa081549338990b38474db4172704d0d`。本地`3.2.0`、`codex/fix-3.2.0`和远端修复分支已删除，全部提交由main保留。R29产品代码`e17f0759f9145383fdbb67b2ef6ecd021aa4802f`；7项实机、当前协议连接故障及追加容量轮换均已完成。
- `71d645ec`的[Windows CI](https://github.com/Follen/Lychee-Dev/actions/runs/38593192586)五项必需作业与汇总全部通过；该结果不代表本轮工作区。原生覆盖24349/31245=77.9293%，0fail、17skip的口径与明细见审计报告。
- [PR #12](https://github.com/Follen/Lychee-Dev/pull/12)已关闭、未合并，只移植必要Definitions冷读预算修复。R23无消费者WGC删除、R24–R26用户要求的三轮清理及R27整合已完成，不扩展新一轮全仓审计。
- 用户已明确授权发布3.2.0，并将npm `latest`直接指向3.2.0，发布后升级本机全局npm安装。发行源和渠道已更新；首轮发布在npm上传前停止以同步包内安装说明的`@latest`，待固定新提交重新发布。没有修改Lychee Talent。

## 当前实现与分工

- 主代理独占实机、受管安装、所有CON、整合和文档。live worker已完成旧200槽迁移分类、首个读取失败诊断及公开Mailbox map初始化；delivery worker已完成禁用槽的INPUT/doctor诊断；data worker提供Trace/Objects临时probe。所有worker当前已结束写入。
- **R29-01**：结构有效的已知旧200槽身份只进入手动激活等待，不授权旧协议输入。正式回归及原CON实机红绿验证通过。
- **R29-02**：`LoadAddOn`返回DISABLED时保留`slot_addon_disabled`，doctor显示最近回调失败；500份元数据不再表述成500个已加载槽。已通过离线回归，旧运行时捕获的真实DISABLED是故障依据，不声称修复后的错误文案已在游戏重新触发。
- **R29-03**：公开receipts/bodies map保留固定schema元数据，避免没有业务记录时使用不可读的空哈希节点；私有协议map不变。正式Lua/Go回归、同条件实机红绿对照和完整交易均通过。
- 首发读取诊断沿既有scan记录逻辑节点、单次长度、已有区域准入结果及guard摘要，不增加读取、不放宽准入、不记录原始地址/内存/角色信息。正式fixture与Windows自有子进程回归通过。

## 当前实机状态

- 固定同一Retail 12.1.0.69933进程、创建身份和窗口，使用本地CLI及受管500槽安装。用户按当前客户端的角色GUID启用了500个槽并保存设置；当前bridge已恢复on。
- 当前版本失败已区分两件事：Slot 01禁用导致INPUT回调load_failed且未Receive；空回执查询另在`record.receipts.entry.node`被MEM_MAPPED区域准入拒绝，56字节请求、0字节实际读取、guard稳定。
- 用户重载后的全新、无owner、NextSlot=1运行时，不发布槽、不发键，仅查询未执行nonce的命名回执，同样复现2161B/116calls的稳定拒绝。修复版在同一进程的新runtime、相同条件和同一只读helper下，结果为2236B/123calls、complete=true、0记录、无firstReadFailure。空表问题独立于禁用槽和提交输入；未放宽MEM_PRIVATE准入。
- 失败CON凭新鲜替代runtime证据关闭；两条成功CON均完成verified报告、released清理及正常disconnect。本轮三条CON均closed，最终doctor全绿、安装无owner、无槽预约。用户完成bridge off后的reload，已销毁Trace hook和Objects临时快照；最后默认option重载后又完成修复版完整交易。

## 7项实机结果

| ID | 已完成的实际检查 | 剩余范围或限制 |
| --- | --- | --- |
| R3-01 Trace | 真实已加载页面OnClick回调：GetTime.foo及3类标量owner报页内错误；自有方法两次调用、参数与停止正常 | 非硬件点击；测试hook已由reload清除 |
| R3-06 拾取器 | 用户实际进入/退出战斗；报告确认dock、键盘脚本和输入保护关闭；用户确认F、Enter、Esc正常 | 本轮特定角色/客户端，不扩展全部战斗场景 |
| R4-01 输入与恢复 | 完整prepare/commit/confirm/release；异步请求running后原CON/request恢复至verified/released；两成功连接28次交换各提交一次，后续采样间隔最小分别257/332ms | 修复版在reload后的独立runtime通过；未制造真实崩溃或缺失outcome，相关边界仍为正式离线覆盖 |
| R4-04 bridge状态 | 默认option、off/on与INPUT一致；保存off并reload后status=false、无RuntimePublication；清除显式option并再次reload后status=true，新runtime可正常连接 | 验证默认开关启动，未清空整个账户配置，不称全新账户矩阵 |
| R4-07 Objects | 真实页面回调处理失败tostring与普通错误；保留5000字partial/limited；健康对象42恢复 | 非硬件点击；临时快照已由reload清除 |
| R5-01 数值 | 4个高精度/大数/微小数JSON数值与%.17g文本double回读相等；修复版完整交易再次通过，报告verified、released，正常断开 | 同时确认游戏内receipts/bodies保留schema元数据 |
| R20 连接故障 | 匹配500槽协议新CON bind成功；旧失败CON在reload后无充分证据时保持pending，取得新鲜替代证据后无新输入关闭 | 当前禁用槽及空表问题已定位；旧3.1.2原包仍不能唯一归因 |

本轮共7项，按表中范围完成，不包括已删除的WGC；七产品完整安装/登录/升级矩阵仍见[todo.md](todo.md)。

## 验证与下一步

- 本轮Go build/vet及完整Lua必需Go套件通过：本地Windows/amd64、Go1.27.1、Lua5.1.5、包并发2、测试并发4、包时限20分钟，2886事件/2620叶节点pass、0fail、42skip、无未完成测试。skip包含helper及本地缺少的公开/原始资料、LuaLS集成和符号链接权限，不借用旧CI结果补算；此前中止的运行不计通过。
- R29验证时版本源为3.1.2；本次发行同步将CLI、addon、npm和测试版本统一为3.2.0，并扩展渠道校验以支持授权的latest。命令目录和Skill输入不变。
- 3.2.0准备检查：build/vet、buildinfo与addon测试通过；Node工具及启动器92pass、0fail、1skip（本地未提供固定LuaLS archive）；版本、Skill合同/生成命令与发行源码输入检查通过。14a8cf2的发布流水线38606807522已通过完整Lua必需Go、race、process、addon、package及正式组装和包内真实LuaLS测试；其后为修正包内安装文案主动取消，未发布npm。仅文档变化不重复本地Go/实机检查，新固定标签仍经过既有发布流程。
- 本地CLI确认为`e17f0759`、workspaceDirty=false；同提交78项开发资源受管update成功，用户完成默认option重载。空回执对照、bind/数值报告/release/disconnect及最终doctor均通过；当前无需用户继续操作。
- 当前文档head `ab037808` 已推同一PR；[Windows CI38603173292](https://github.com/Follen/Lychee-Dev/actions/runs/38603173292)的contract、race、process、addon、package及ci-required全部成功，PR说明已同步。本地42skip与CI环境结果仍分开记，不推测新覆盖率或skip计数。
- 私人现场日志只留在本地，不上传公共PR。按用户既有要求保留全部剩余忽略材料、原ZIP和恢复记录；不得清理其他任务或保留的`.worktrees/rust-refactor`。原R20/R22部分临时负对照日志已删除的历史限制仍见审计报告，不能补造。

## 用户追加：500槽容量一轮与发布

- 同一Retail进程、e17f0759受管安装完成125次完整请求：prepare/commit/confirm/release各125次，共500次业务槽交换；另有2次bind、1次unbind和1次自动reload。504次输入交换各提交一次。容量轮换在两个runtime间完成，不称同一runtime逐一执行500个物理槽。
- 125份报告全部verified、资源released、清理complete；500个数值/十进制文本比较通过。16份当前/历史journal包含4120条业务事件（另15条checkpoint），连接已正常关闭，最终doctor全绿、无owner、0预约。
- 下一步：完成3.2.0版本和渠道检查，推送main及v3.2.0标签，通过既有发布流水线发布到latest，回读registry和GitHub Release。保留用户要求留下的材料及rust-refactor工作树。
