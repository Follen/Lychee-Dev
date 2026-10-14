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
- 新增 Anniversary 2.5.6 / Interface 20506 插件配置和独立事件目录，主插件及 500 槽均支持周年服；旧槽声明可受管迁移。
- 从游戏根目录发现客户端时复用统一产品目录，补齐 Anniversary 和 Classic Era。
- 使用运行时指令特征及 RIP 相对位移定位 Lua 根，沿 Lua state → globals → Mailbox 读取。
- Forever 扫描按可读代码区间进行，保留短读前缀；多个命中先解析 RIP、按根去重，再验证 Lua/Mailbox，移除固定 Forever hash 的捷径并保留扫描缺口。
- 注册正式执行/重载绑定；默认 Ctrl+Alt+F12、Ctrl+Alt+F11，公布实际有效键位。
- 普通 EditBox 选择性透传两个动作，保留文字、光标与焦点；移除 Native 光学输入和 Esc 恢复。
- 增加独立启动诊断及 live doctor，顶层 doctor 复用相同检查。
- 修复旧三键与二键运行时共用旧 schema 名称导致的自动升级误判；doctor 与激活读取实际 INPUT，三键版本直接返回手动重载要求。
- 修复读取期间 INPUT 指针变化导致当前 CLI 提前退出的问题；重载空窗沿原期限等待，发键前复核采用统一错误分类和 trace。
- 修复已核实报告的 release 回执丢失后清理一直等待的问题：等待 5 秒后在原期限内自动重载清理，保留报告，不重放业务或 release。
- 发键前检查全部修饰键和本次组合键，玩家按住键时等待松开；不强制更改实体键状态。
- INPUT 采样不再依附 UIParent，修复第三方挂机界面隐藏主 UI 后遥测停更的问题。
- 关闭中的初始 bind 即使没有收到回执，也可通过原 CON 的显式重载验证退役并完成关闭，不重新绑定或执行。
- Retail 与国服无限测试服完成 500 槽容量轮换和释放后续跑；Retail 完成同产品双实例验证。
- Classic 与 Titan 各完成 500 槽容量轮换、自动重载、释放重连后的 100 槽续跑，共 300 次业务执行全部通过；最终连接及预约均释放。Classic 一次真实 path_changed 在原 CLI 调用内恢复。
- Era 与 Forever Asia 1.60.1.70374 各完成 500 槽容量轮换、自动重载和释放重连后的 100 槽续跑，共 300 次业务执行全部通过；最终连接和预约释放。Era 掉线前已核实的旧报告原样保留，并通过旧进程退出证据完成清理。
- Anniversary 完成 500 槽容量轮换和释放重连后的 100 槽续跑，另完成修复后 25 次复验及重登探针；确认 20506 profile 与 1483 项事件目录，最终连接和预约释放。
- Forever Asia 从 1.60.1.70338 更新到 1.60.1.70374 后，现有 CLI 直接定位并完成实机测试，无需新增固定 RVA 或 hash 适配。多个特征命中解析到同一根，wowdump 独立对照一致，扫描缺口单独记录。
- Mailbox 修复后双 Retail 复测完成 16 次业务调用、2 次重载；一次真实 path_changed 在同一 CLI 调用内恢复。

### 仓库

- 重建 README、Design、Changelog、todo 和当前开发/协议文档。
- 清理旧设计、审计流水、运行时研究快照、临时实验及过期实机脚本。
- 保留生产代码的 Go/Lua 回归、来源固定信息、许可证和发布工具。

当前协议尚未完成的产品、首次安装及实机场景见 [todo.md](todo.md)。
