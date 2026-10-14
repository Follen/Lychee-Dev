# 功能回归基线

正式离线入口为 tools/baseline.mjs。它检查当前工作树，不发布、不安装游戏插件，也不执行游戏输入。

## 环境与运行

需要 Lua 5.1，以及 release/tools/luals.json 固定的真实 LuaLS archive/runtime：

    node tools/baseline.mjs --lua51 "C:/path/to/lua.exe" --luals-archive "C:/path/to/pinned-luals.zip" --luals-runtime "C:/path/to/payload/tool/luals"

也可以设置 LYCHEEDEV_LUA51、LYCHEEDEV_LUALS_ARCHIVE 和 LYCHEEDEV_LUALS_TEST_RUNTIME 后直接运行入口。

检查包括 build、vet、Lua 必需的完整 Go 套件、Node 工具和 npm 测试、版本一致性、Skill 合同，以及运行前后源码身份。Go 测试限制包并发为 2、测试并发为 4，并使用 20 分钟包时限。

## 目录与结果

[catalog.mjs](catalog.mjs) 为必要的行为指定实际 Go 测试名称；测试缺失、被跳过或包没有正常结束，不能作为通过。当前 Mailbox 的编辑框输入、独立重载、启动诊断和读取恢复位于 BASE-08–11。

历史持久化覆盖仅验证只读查看和取消处置；旧 Live 执行驱动与 envelope 基线已移除。这些结果不能证明当前协议的游戏实机场景。

当前实机验收直接使用公共 live doctor/connect/execute/resume/reload/disconnect 命令，遵循 [协议](../../docs/live-protocol.md)，并对照 [待办矩阵](../../todo.md)。保留本次失败所需的原连接；不要为了重跑创建竞争请求或重放未知输入。

工作台探针保留在 probes/，用于明确授权的特定场景。临时日志和捕获放入 .tmp，完成后清理。
