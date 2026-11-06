# 当前交接

更新：2026-11-06，Asia/Shanghai。架构与合同见 [Design.md](Design.md)、[Live协议](docs/live-protocol.md)，当前验收与待办见 [todo.md](todo.md)。

## 分支与交付

- 当前main；发布代码为`1f7810b482a315a5db2e528f6312f0d61705ff78`，`v3.2.9`固定同一提交，均已推送。[3.2.9 Release](https://github.com/Follen/Lychee-Dev/releases/tag/v3.2.9)六项资产齐全；npm latest=3.2.9，next=3.1.2保持，integrity与provenance元数据读回通过。
- [main CI 41965813010](https://github.com/Follen/Lychee-Dev/actions/runs/41965813010)及[发行41967085049](https://github.com/Follen/Lychee-Dev/actions/runs/41967085049)全绿。3.2.8候选标签保留原提交，因SDK选择故障未发布npm；未移动旧标签或覆盖已发布产物。
- 本机全局CLI及`C:/Users/follen/.agents/skills/lycheedev`已受管更新3.2.9，版本命令、8文件Skill回执及Skill启动脚本均指向发布提交。Skill新会话加载新版；本次更新未选择游戏addon或其他安装。新版全局CLI已被动识别当前wowf/1.60.1.70790。
- 主Agent独立维护；工作区原有的 `.zcodeignore` 删除保持原状。`.worktrees/Rust-refactor`、工具链和原项目保留。

## 关键改动与证据

- SOURCE-CAPACITY：本机 `source/v1` 原占 4,183,464,552 字节，`source prune` 只统计 98,315,546 字节的工作树。索引构建失败曾留下新写入的逐文件事实。修复在容量不足时回收可重建的偏移表，索引提交后才发布新增逐文件事实。保留 4 GiB 上限、索引记录、固定 PIN 和证据。
- StardropFish 的 Forever 原 PIN（提交 `eac30af597e5aaa43a60f6675ef5fdcea100375f`）用开发版建索引成功，回收两个 v2 偏移表共 207,111,143 字节。已安装 3.2.9 重跑原命令也成功，结果字节一致。4,489 文档完整覆盖，11 项中 2 项结果截断。已验证结果证据 `CAP-5c8ac008462cef5c6ee833da9b24b0457172fd07c5a8c73f103f788a3d86b4f0`，原请求和失败文件哈希保持。两项新回归先红后绿，build/vet、Lua 必需的 Source 与 CLI 四包测试通过，临时验证文件已清理。
- [Fuploader PR #29](https://github.com/Follen/Fuploader/pull/29)同源问题已在Go Publish复现并修复：133/173/205条历史的get/versions共6组原报`version count changed`。续页独立`total: 0`沿用首屏正总数；11组无效续页仍拒绝，删除回读检查续页仍存在目标。先红后绿，fixture HTTP回放，未执行平台上传或删除。
- 之前审计的CLI/schema、SQL/Source清单、缓存额度、发布回执、Addon输出和npm布尔参数修复均随3.2.9交付，具体行为见Changelog。新增PR场景不沿用此前“无新增问题”的结论。
- 发布CI实际复现`DBCD SDK drift: 10.0.403`；根global.json改为精确10.0.100、禁止roll-forward，保留实际SDK身份检查。新main与tag均完成DBCD/CASC/LuaLS固定输入准备，确认修复。
- 正式无限服实机为发行前开发版3.2.7源码：wowf/1.60.1.70790、500/500槽、八页面功能激活、执行/结果树/自有追踪/事件注册/异步、自动reload及原CON重绑。6份成功报告verified/released，两个进程分别处理，连接均关闭；最终Live doctor全绿、0预约。此范围不扩展为版本9游戏内重载、战斗、视觉、重登或完整产品矩阵。

## 验证与保留

- 发布代码的main完整Lua5.1必需Go套件：3566条通过事件、22项外部环境或helper skip、0失败/0未完成；身份与tree固定发布提交。race、Addon、多进程、包安装及发行组装/来源重建/原生运行/封存/registry/GitHub回读均通过，完整证据在上述Actions与Release。
- 本地build/vet、分页修复后Publish整包、六客户端Lua、Node99通过/1个固定LuaLS archive skip及Skill/版本合同通过。云端固定材料验证独立补齐，不把本地skip记为通过。
- 完整已关闭Live连接与实机报告保留在`.tmp/forever-live/project`；部署临时文件此前已脚本清理。本次`.tmp/release-328`及`.tmp/blackbox-pagination`已在发行/安装成功、原编辑恢复后按绝对路径、链接和文件白名单脚本清理。

## 后续

- SOURCE-CAPACITY 纳入 3.2.10 发布候选；下一步由固定标签运行发行流程，随后更新本机 CLI 和 Skill，并重跑 StardropFish 原命令。
- 完整六产品实机矩阵、冷CDN、规模/性能及真实平台写入仍按todo单列。ModUs图片无字节读回端点，未知storage登记保持原pending、不重放；NewBee Creator token变化暂停原pending续写。
