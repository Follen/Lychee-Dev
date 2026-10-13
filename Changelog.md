# Changelog

本文件从 3.2.0 开发基线开始。历史发行内容保留在 Git 标签和提交历史中。仓库合并与 npm 发布分别记录，不把开发完成写成已经发布。

## 3.2.0 — Unreleased

发行版本源仍为 3.1.2，未创建 3.2.0 发布。

### Data 与产品

- 统一 Retail、Classic、Classic Era、Titan、Forever Asia beta、Forever CN beta、Anniversary 七产品目录。
- 支持明确的本地安装读取和远端 CDN 路由，按产品、地区及构建配置隔离。
- 修正同版本号两个 beta 的选择、兄弟目录误选和缓存身份。
- 优化本地 CASC 索引桶选择及 CDN 归档索引读取。
- 七产品均完成 ChrRaces 小范围本地/CDN 读取；固定对应来源的内容一致，不表示所有表都已验证。

### Source

- 两个 Forever beta 都映射到上游 forever 分支，保留区域产品身份。
- 七产品完成实际源码同步。
- 增加 source batch，复用固定快照、Git 文档和 LuaLS 环境。
- 精确查询和关系定位使用请求内查找表，声明索引按需建立，增加有界行位置缓存。

### Live 与 addon

- 统一协议名称为 Lycheedev mailbox protocol v1。
- LoD 槽池扩展到 500，支持受管迁移、容量重载、释放和重连。
- 使用运行时指令特征及 RIP 相对位移定位 Lua 根，沿 Lua state → globals → Mailbox 读取。
- 注册正式执行/重载绑定；默认 Ctrl+Alt+F12、Ctrl+Alt+F11，公布实际有效键位。
- 普通 EditBox 选择性透传两个动作，保留文字、光标与焦点；移除 Native 光学输入和 Esc 恢复。
- 增加独立启动诊断及 live doctor，顶层 doctor 复用相同检查。
- 修复读取期间 INPUT 指针变化导致当前 CLI 提前退出的问题；重载空窗沿原期限等待，发键前复核采用统一错误分类和 trace。
- Retail 与国服无限测试服完成 500 槽容量轮换和释放后续跑；Retail 完成同产品双实例验证。
- Mailbox 修复后双 Retail 复测完成 16 次业务调用、2 次重载；一次真实 path_changed 在同一 CLI 调用内恢复。

### 仓库

- 重建 README、Design、Changelog、todo 和当前开发/协议文档。
- 清理旧设计、审计流水、运行时研究快照、临时实验及过期实机脚本。
- 保留生产代码的 Go/Lua 回归、来源固定信息、许可证和发布工具。

当前协议尚未完成的产品、首次安装及实机场景见 [todo.md](todo.md)。