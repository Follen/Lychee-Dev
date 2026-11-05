# 当前交接

更新：2026-11-05，Asia/Shanghai。架构与合同见 [Design.md](Design.md)、[Live协议](docs/live-protocol.md)，当前验收与待办见 [todo.md](todo.md)。

## 分支与交付

- 当前main；发布代码为`1f7810b482a315a5db2e528f6312f0d61705ff78`，`v3.2.9`固定同一提交，均已推送。[3.2.9 Release](https://github.com/Follen/Lychee-Dev/releases/tag/v3.2.9)六项资产齐全；npm latest=3.2.9，next=3.1.2保持，integrity与provenance元数据读回通过。
- [main CI 41965813010](https://github.com/Follen/Lychee-Dev/actions/runs/41965813010)及[发行41967085049](https://github.com/Follen/Lychee-Dev/actions/runs/41967085049)全绿。3.2.8候选标签保留原提交，因SDK选择故障未发布npm；未移动旧标签或覆盖已发布产物。
- 本机全局CLI及`C:/Users/follen/.agents/skills/lycheedev`已受管更新3.2.9，版本命令、8文件Skill回执及Skill启动脚本均指向发布提交。Skill新会话加载新版；本次更新未选择游戏addon或其他安装。新版全局CLI已被动识别当前wowf/1.60.1.70790。
- 主Agent独立维护；原有无关`.zcodeignore`删除已恢复，临时单路径stash已应用并仅删除本次stash，其他stash保留。`.worktrees/Rust-refactor`、工具链与原项目保持。

## 关键改动与证据

- [Fuploader PR #29](https://github.com/Follen/Fuploader/pull/29)同源问题已在Go Publish复现并修复：133/173/205条历史的get/versions共6组原报`version count changed`。续页独立`total: 0`沿用首屏正总数；11组无效续页仍拒绝，删除回读检查续页仍存在目标。先红后绿，fixture HTTP回放，未执行平台上传或删除。
- 之前审计的CLI/schema、SQL/Source清单、缓存额度、发布回执、Addon输出和npm布尔参数修复均随3.2.9交付，具体行为见Changelog。新增PR场景不沿用此前“无新增问题”的结论。
- 发布CI实际复现`DBCD SDK drift: 10.0.403`；根global.json改为精确10.0.100、禁止roll-forward，保留实际SDK身份检查。新main与tag均完成DBCD/CASC/LuaLS固定输入准备，确认修复。
- 正式无限服实机为发行前开发版3.2.7源码：wowf/1.60.1.70790、500/500槽、八页面功能激活、执行/结果树/自有追踪/事件注册/异步、自动reload及原CON重绑。6份成功报告verified/released，两个进程分别处理，连接均关闭；最终Live doctor全绿、0预约。此范围不扩展为版本9游戏内重载、战斗、视觉、重登或完整产品矩阵。

## 验证与保留

- 发布代码的main完整Lua5.1必需Go套件：3566条通过事件、22项外部环境或helper skip、0失败/0未完成；身份与tree固定发布提交。race、Addon、多进程、包安装及发行组装/来源重建/原生运行/封存/registry/GitHub回读均通过，完整证据在上述Actions与Release。
- 本地build/vet、分页修复后Publish整包、六客户端Lua、Node99通过/1个固定LuaLS archive skip及Skill/版本合同通过。云端固定材料验证独立补齐，不把本地skip记为通过。
- 完整已关闭Live连接与实机报告保留在`.tmp/forever-live/project`；部署临时文件此前已脚本清理。本次`.tmp/release-328`及`.tmp/blackbox-pagination`已在发行/安装成功、原编辑恢复后按绝对路径、链接和文件白名单脚本清理。

## 后续

- 产品修复、提交推送、发布、本机更新和临时清理均已完成；本交接随独立文档提交更新。当前无待恢复发行或本机更新，后续按用户新范围执行。
- 完整六产品实机矩阵、冷CDN、规模/性能及真实平台写入仍按todo单列。ModUs图片无字节读回端点，未知storage登记保持原pending、不重放；NewBee Creator token变化暂停原pending续写。
