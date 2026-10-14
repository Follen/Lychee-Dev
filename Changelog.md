# Changelog

从当前开发基线开始记录功能变化；历史版本查阅 Git 标签与提交。

## 3.2.0 — Unreleased

发行版本源仍为 3.1.2，3.2.0 尚未发布。

### 产品、Source 与 Data

- 统一 Retail、Classic、Classic Era、Titan、两个 Forever beta 和 Anniversary 七产品的发现与路由。
- 两个 Forever beta 共享上游 `forever` 源码分支，保留各自数据地区与构建身份。
- 支持固定构建的本地 CASC 和远端 CDN 读取；修正兄弟目录误选、同版本号 beta 混用及缓存身份问题。
- 优化本地索引桶选择、远端归档索引读取，以及 Source 精确查询、关系定位和文档缓存。
- `source diff` 对两端固定版本自动准备缺失索引，与已有查询按需准备合同一致。
- Skill 的 WoW 源码/API 调查默认复用固定 Source；空结果先核查询与覆盖，不猜 GitHub raw 路径。外部检索限真实缺口或明确请求并区分证据来源，已有充分证据不重复准备。
- 新增 `source batch`，在同一快照与预算内复用 Git、API 和 LuaLS 环境。
- 分离 addon 静态解析与 Source 浏览职责；修复 tracked ref、正文搜索、Context 分页、TOC 覆盖缺口、UNION 重复列和 AVI 嵌套记录问题，显式区分 Hotfix 窗口结束与完整查询覆盖。
- DB2 JSONL 在来源复核与归档后发送 end；错误退出保留 error 帧。工作区 download.workers 约束实际下载并发。

- 修复 Hotfix pending 来源删除、精确 SymbolID flow、Source inspect 覆盖和跨进程 SQL CSV/manifest 一致性。
- 视频 demux 按目标目录串行发布帧与 manifest，复用 capture 的已核验 blob，消除重复帧写入。
- 修复 Git-backed Source 对中文等合法文件名的正文搜索；Search/Inspect/Context 摘要复用现有行索引，避免每个结果重复切分整文件。
- Hotfix CSV 与 SQL CSV 共用目标级发布互斥和 staged 文件边界，保留原覆盖与 manifest 合同，并阻止覆盖当前工作区身份/数据。
- 修复 SQL CSV 发布前 query capture 被删除的间隙；冷 Definitions HTTP 缓冲使用临时预算，持久 metadata 与热缓存一致按实际字节计费。
- Hotfix 普通页/scan checkpoint 与资产导出/视频 manifest 持久保护其 capture 依赖，兼容历史记录，缺失页面不再恢复为完整成功。

### Live 与 addon

- 统一使用 **Lycheedev mailbox protocol v1**，扩展到 500 个 LoD 槽，支持受管扩容、容量重载、释放和重连。
- 新增周年服 Interface 20506 profile 和独立的 1483 项事件目录。
- 通过运行时指令特征与 RIP 相对位移定位 Lua 根。Forever 扫描保留短读、合并相同根并验证 Lua/Mailbox，显式报告扫描缺口。
- 注册正式执行与重载快捷键；普通编辑框透传对应动作。Native 路径移除光学输入、Esc 恢复和聊天命令。
- 以独立启动诊断和 `live doctor` 统一首次安装、升级与连接恢复。
- 修复 INPUT 指针变化导致读取提前退出，以及 UIParent 隐藏导致 INPUT 采样停止的问题。
- 发送前等待实体修饰键释放；报告已核实但 release 回执缺失时，在原期限内重载清理并保留报告。
- 修复旧三键/二键运行时的升级能力判断，以及未确认 bind 的原连接关闭恢复。
- 退役旧窗口队列、光学/QR、SavedVariables 执行驱动及其命令入口；历史 OP/BTP 仅保留查看、取消和必要处置，当前执行要求 Native CON。

- 修复跨交易 INPUT 采样间隔、Trace 目标错误、战斗拾取关闭、默认 bridge 状态与对象流式错误处理。
- 缺失输入 outcome 的恢复先持久化保守时间上界，保持原请求且不重放；报告编码保留有限 binary64 数值精度。

### 验证与维护

- 七产品完成当前协议的容量轮换与释放后续跑；Retail 完成同安装双实例及读取恢复测试。
- Forever Asia 更新至 1.60.1.70374 后，原 CLI 无需新增固定 RVA 或 hash 适配即可完成测试。
- 以当前实现重建项目文档，移除旧实验、测试流水和临时发行副本。
- 修复受管升级中断准备、旧槽卸载、默认更新库存、npm 更新锁及混合 PATH 启动器选择；addon ZIP 随包携带项目许可与第三方声明。
- Source broker 关闭等待已核验的原进程退出，避免关闭回包早于 listener 和租约清理导致紧接请求失败。
- 发行组装接入九项真实 LuaLS/Source/broker/安装包语义测试，并分别检查 registry integrity 与 provenance 元数据。
- 修复 game-root 更新恢复身份、doctor 默认归档父目录、SQL stdin 查询期限和 Windows Skill 自定义 npm prefix 查找。
- evidence bundle 在 ZIP 阶段取消后不再发布最终文件，保留正常无覆盖的原子发布合同。
- doctor 的跨卷默认归档移至目标侧；同卷项目归档和显式 output 保持，旧恢复档案不被替换。
- 校验与剩余场景集中记录在 [todo.md](todo.md)，开发和清理规则见 [开发指南](docs/development.md)。
