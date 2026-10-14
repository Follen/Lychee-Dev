更新日期：2026-10-15，Asia/Shanghai。详细历史问题与证据见[审计报告](docs/Clean-repo-2026-10-14.md)；本文件只记录当前交接。

## 分支与交付

- [PR #13](https://github.com/Follen/Lychee-Dev/pull/13)：`codex/fix-3.2.0` → `main`，保持草稿。当前远端head `568a5865`，产品基线 `71d645ec`；本轮R29修复尚未提交。用户要求处理7项实机检查及交接的连接故障，使用新PR代码。
- `71d645ec`的[Windows CI](https://github.com/Follen/Lychee-Dev/actions/runs/38593192586)五项必需作业与汇总全部通过；该结果不代表本轮工作区。原生覆盖24349/31245=77.9293%，0fail、17skip的口径与明细见审计报告。
- [PR #12](https://github.com/Follen/Lychee-Dev/pull/12)已关闭、未合并，只移植必要Definitions冷读预算修复。R23无消费者WGC删除、R24–R26用户要求的三轮清理及R27整合已完成，不扩展新一轮全仓审计。
- 发行源3.1.2、渠道next、latest=2.5.1保持；3.2.0为开发基线。不合并、不发布。没有修改Lychee Talent或全局npm安装。

## 当前实现与分工

- 主代理独占实机、受管安装、所有CON、整合和文档。live worker已完成旧200槽迁移分类、首个读取失败诊断及公开Mailbox map初始化；delivery worker已完成禁用槽的INPUT/doctor诊断；data worker提供Trace/Objects临时probe。所有worker当前已结束写入。
- **R29-01**：结构有效的已知旧200槽身份只进入手动激活等待，不授权旧协议输入。正式回归及原CON实机红绿验证通过。
- **R29-02**：`LoadAddOn`返回DISABLED时保留`slot_addon_disabled`，doctor显示最近回调失败；500份元数据不再表述成500个已加载槽。已通过离线回归，旧运行时捕获的真实DISABLED是故障依据，不声称修复后的错误文案已在游戏重新触发。
- **R29-03**：公开receipts/bodies map保留固定schema元数据，避免没有业务记录时使用不可读的空哈希节点；私有协议map不变。正式Lua/Go回归通过，修复后实机对照待部署。
- 首发读取诊断沿既有scan记录逻辑节点、单次长度、已有区域准入结果及guard摘要，不增加读取、不放宽准入、不记录原始地址/内存/角色信息。正式fixture与Windows自有子进程回归通过。

## 当前实机状态

- 固定同一Retail 12.1.0.69933进程、创建身份和窗口，使用本地CLI及受管500槽安装。用户按当前客户端的角色GUID启用了500个槽并保存设置；当前bridge已恢复on。
- 当前版本失败已区分两件事：Slot 01禁用导致INPUT回调load_failed且未Receive；空回执查询另在`record.receipts.entry.node`被MEM_MAPPED区域准入拒绝，56字节请求、0字节实际读取、guard稳定。
- 用户重载后的全新、无owner、NextSlot=1运行时，不发布槽、不发键，仅查询未执行nonce的命名回执，同样复现2161B/116calls的稳定拒绝。因此空表问题独立于禁用槽和提交输入；修复后同条件对照尚未执行。
- 失败CON凭新鲜替代runtime证据关闭；随后成功CON已完成全部请求、verified报告、released清理及正常disconnect。当前两条CON均closed，安装无owner、无待处理槽预约。用户完成bridge off后的reload，已销毁本轮Trace hook和Objects临时快照。

## 7项实机结果

| ID | 已完成的实际检查 | 剩余范围或限制 |
| --- | --- | --- |
| R3-01 Trace | 真实已加载页面OnClick回调：GetTime.foo及3类标量owner报页内错误；自有方法两次调用、参数与停止正常 | 非硬件点击；测试hook已由reload清除 |
| R3-06 拾取器 | 用户实际进入/退出战斗；报告确认dock、键盘脚本和输入保护关闭；用户确认F、Enter、Esc正常 | 本轮特定角色/客户端，不扩展全部战斗场景 |
| R4-01 输入与恢复 | 完整prepare/commit/confirm/release；数值请求采样比恢复上界晚382–460ms；异步请求running后原CON/request恢复至verified/released | 修复版整合后核对最终journal及独立runtime |
| R4-04 bridge状态 | 已加载运行时默认option、off/on与INPUT一致；用户保存off并reload后status为false、无RuntimePublication；on恢复成功 | 新加载运行时的默认option待最后一次部署重载；不清空整个账户配置 |
| R4-07 Objects | 真实页面回调处理失败tostring与普通错误；保留5000字partial/limited；健康对象42恢复 | 非硬件点击；临时快照已由reload清除 |
| R5-01 数值 | 4个高精度/大数/微小数JSON数值与%.17g文本double回读相等；报告verified、released，正常断开 | 修复版完整交易将再次覆盖受影响Mailbox map |
| R20 连接故障 | 匹配500槽协议新CON bind成功；旧失败CON在reload后无充分证据时保持pending，取得新鲜替代证据后无新输入关闭 | 当前禁用槽及空表问题已定位；旧3.1.2原包仍不能唯一归因 |

本轮共7项，不包括已删除的WGC；七产品完整安装/登录/升级矩阵仍见[todo.md](todo.md)。

## 验证与下一步

- 本轮Go build/vet及完整Lua必需Go套件通过：本地Windows/amd64、Go1.27.1、Lua5.1.5、包并发2、测试并发4、包时限20分钟，2886事件/2620叶节点pass、0fail、42skip、无未完成测试。skip包含helper及本地缺少的公开/原始资料、LuaLS集成和符号链接权限，不借用旧CI结果补算；此前中止的运行不计通过。
- 版本检查通过，版本源仍为3.1.2。没有修改命令目录或Skill输入，不扩展无关验收。
- 下一步：提交本轮代码，受管安装该提交，用户手动reload；同条件只读空回执对照应为完整读取、0记录、无首发失败，再做bind/数值报告/release/disconnect，更新同一PR。
- 私人现场日志只留在本地，不上传公共PR。按用户既有要求保留全部剩余忽略材料、原ZIP和恢复记录；不得清理其他任务或保留的`.worktrees/rust-refactor`。原R20/R22部分临时负对照日志已删除的历史限制仍见审计报告，不能补造。
