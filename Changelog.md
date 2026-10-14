# Changelog

从当前开发基线开始记录功能变化；历史版本查阅 Git 标签与提交。

## 3.2.0 — Unreleased

发行版本源仍为 3.1.2，3.2.0 尚未发布。

### 产品、Source 与 Data

- 统一 Retail、Classic、Classic Era、Titan、两个 Forever beta 和 Anniversary 七产品的发现与路由。
- 两个 Forever beta 共享上游 `forever` 源码分支，保留各自数据地区与构建身份。
- 支持固定构建的本地 CASC 和远端 CDN 读取；修正兄弟目录误选、同版本号 beta 混用及缓存身份问题。
- 优化本地索引桶选择、远端归档索引读取，以及 Source 精确查询、关系定位和文档缓存。
- 新增 `source batch`，在同一快照与预算内复用 Git、API 和 LuaLS 环境。

### Live 与 addon

- 统一使用 **Lycheedev mailbox protocol v1**，扩展到 500 个 LoD 槽，支持受管扩容、容量重载、释放和重连。
- 新增周年服 Interface 20506 profile 和独立的 1483 项事件目录。
- 通过运行时指令特征与 RIP 相对位移定位 Lua 根。Forever 扫描保留短读、合并相同根并验证 Lua/Mailbox，显式报告扫描缺口。
- 注册正式执行与重载快捷键；普通编辑框透传对应动作。Native 路径移除光学输入、Esc 恢复和聊天命令。
- 以独立启动诊断和 `live doctor` 统一首次安装、升级与连接恢复。
- 修复 INPUT 指针变化导致读取提前退出，以及 UIParent 隐藏导致 INPUT 采样停止的问题。
- 发送前等待实体修饰键释放；报告已核实但 release 回执缺失时，在原期限内重载清理并保留报告。
- 修复旧三键/二键运行时的升级能力判断，以及未确认 bind 的原连接关闭恢复。

### 验证与维护

- 七产品完成当前协议的容量轮换与释放后续跑；Retail 完成同安装双实例及读取恢复测试。
- Forever Asia 更新至 1.60.1.70374 后，原 CLI 无需新增固定 RVA 或 hash 适配即可完成测试。
- 以当前实现重建项目文档，移除旧实验、测试流水和临时发行副本。
- 校验与剩余场景集中记录在 [todo.md](todo.md)，开发和清理规则见 [开发指南](docs/development.md)。
