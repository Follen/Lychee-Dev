# 跨语言协议 fixtures

本目录保存仍被 Go/Lua 解析器测试使用的 schema、样本和 golden 数据，运行时不依赖此目录。

当前 Native Live 合同是 [Lycheedev mailbox protocol v1](../docs/live-protocol.md)。其信封和内存记录实现分别位于 internal/bridge、internal/live/channel 与 addon/Bridge，交叉验证位于 tests/protocol 和 tests/addon。

现存 signal schema 与样本用于历史报告解析验证。旧 QR receipt 和 receiver wire golden 已随执行路径退役。新增协议样本必须由生产编解码路径验证，不能把一次实机运行的完整日志当作 fixture。