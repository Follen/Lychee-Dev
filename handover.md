# 当前交接

更新：2026-11-06，Asia/Shanghai。架构与合同见 [Design.md](Design.md)、[Live协议](docs/live-protocol.md)，当前验收与待办见 [todo.md](todo.md)。

## 分支与交付

- 当前 main，发布代码与 `v3.2.10` 固定到 `ab938c14238a4c9932836dd1ed19c26e800e57e2`。[3.2.10 Release](https://github.com/Follen/Lychee-Dev/releases/tag/v3.2.10)六项资产齐全，npm latest=3.2.10、next=3.1.2，integrity 和 provenance 元数据读回通过。
- [main CI 42166696738](https://github.com/Follen/Lychee-Dev/actions/runs/42166696738)首轮因 lua.org 连接超时失败，原提交重试通过。[发行 42166702680](https://github.com/Follen/Lychee-Dev/actions/runs/42166702680)全绿。
- 本机全局 CLI 及 `C:/Users/follen/.agents/skills/lycheedev` 已受管更新至 3.2.10。全局命令、Skill 启动脚本和 8 个受管文件均已核验，Skill 新会话加载新版。更新目标限 CLI 与该 Skill。
- 主Agent独立维护；工作区原有的 `.zcodeignore` 删除保持原状。`.worktrees/Rust-refactor`、工具链和原项目保留。

## 关键改动与证据

- SOURCE-CAPACITY：本机 `source/v1` 原占 4,183,464,552 字节，`source prune` 只统计 98,315,546 字节的工作树。索引构建失败曾留下新写入的逐文件事实。修复在容量不足时回收可重建的偏移表，索引提交后才发布新增逐文件事实。保留 4 GiB 上限、索引记录、固定 PIN 和证据。
- StardropFish 的 Forever 原 PIN（提交 `eac30af597e5aaa43a60f6675ef5fdcea100375f`）用开发版建索引成功，回收两个 v2 偏移表共 207,111,143 字节。全局 3.2.10 重跑原命令退出 0，结果与开发版字节一致。4,489 文档完整覆盖，11 项中 2 项结果截断。已验证结果证据 `CAP-894e5cdccb44be6003885f1a844cf1ed8e91fbf593e123edb3d71d7a6b26a88a`，原请求和失败文件哈希保持。两项新回归先红后绿，临时验证文件已清理。
- [Fuploader PR #29](https://github.com/Follen/Fuploader/pull/29)同源问题已在Go Publish复现并修复：133/173/205条历史的get/versions共6组原报`version count changed`。续页独立`total: 0`沿用首屏正总数；11组无效续页仍拒绝，删除回读检查续页仍存在目标。先红后绿，fixture HTTP回放，未执行平台上传或删除。
- 之前审计的CLI/schema、SQL/Source清单、缓存额度、发布回执、Addon输出和npm布尔参数修复均随3.2.9交付，具体行为见Changelog。新增PR场景不沿用此前“无新增问题”的结论。
- 发布CI实际复现`DBCD SDK drift: 10.0.403`；根global.json改为精确10.0.100、禁止roll-forward，保留实际SDK身份检查。新main与tag均完成DBCD/CASC/LuaLS固定输入准备，确认修复。
- 正式无限服实机为发行前开发版3.2.7源码：wowf/1.60.1.70790、500/500槽、八页面功能激活、执行/结果树/自有追踪/事件注册/异步、自动reload及原CON重绑。6份成功报告verified/released，两个进程分别处理，连接均关闭；最终Live doctor全绿、0预约。此范围不扩展为版本9游戏内重载、战斗、视觉、重登或完整产品矩阵。

## 验证与保留

- 3.2.10 发布流程的 Lua 5.1 必需完整 Go 套件、race、Addon、多进程、包安装、发行组装、来源重建及注册表回读均通过。可在上述 Actions 查看完整证据。
- 本地 build/vet、Source 与 CLI 四包测试、版本和 Skill 合同通过。
- 完整已关闭Live连接与实机报告保留在`.tmp/forever-live/project`；部署临时文件此前已脚本清理。本次`.tmp/release-328`及`.tmp/blackbox-pagination`已在发行/安装成功、原编辑恢复后按绝对路径、链接和文件白名单脚本清理。

## 后续

- SOURCE-CAPACITY 已发布，本机更新与原命令复验完成。
- 完整六产品实机矩阵、冷CDN、规模/性能及真实平台写入仍按todo单列。ModUs图片无字节读回端点，未知storage登记保持原pending、不重放；NewBee Creator token变化暂停原pending续写。
