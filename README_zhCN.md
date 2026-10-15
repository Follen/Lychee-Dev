# Lychee Dev Toolkit · Rust 迁移

[English](README.md)

`rust-refactor` 是**尚未完成的 Rust 实现**，目标行为与固定 Go 3.2.2 一致，不是新发行版，也不表示功能或性能已经通过验收。

未发布 Rust 候选版本为 **3.5.0**，发行配置渠道为 **beta**。包版本与 Go 3.2.2 行为基线分别固定；本次没有执行发布或 npm dist-tag 操作。

## 唯一目标与固定基线

基线为 **v3.2.2**，发布 tag 解析后的精确提交 **`0bec60e4406f5d5dc155ee25cef44f9a710e37e8`**（[发行源码](https://github.com/Follen/Lychee-Dev/blob/0bec60e4406f5d5dc155ee25cef44f9a710e37e8/README.md)）。不得用滚动 main、Go 3.1.2、任意开发二进制或之后的修复替代。

此前 v3.2.1（`10b561db5706702cd42e047eaeed4ccb5e825351`）和 v3.2.0（`3057a9666ab078270b63c7f06de748069b0d1f4b`）的夹具、测试与失败证据保持原身份。[3.2.1 累积增量](docs/toolkit/rust-go-3.2.1-increment.md)仍须完成；新增[3.2.2 增量](docs/toolkit/rust-go-3.2.2-increment.md)按当前角色 GUID 启用受管槽但不加载，贯通 `slotReloadRequired`，在原连接、期限和事务内恢复 reload，并增加只读槽启用诊断和启动指引。新增范围待实现及精确候选验收。已核 tag/main 源码身份；检查时 Release API 未提供产物，未声称发行产物已经验证。

以 Go 3.2.2 定行为，复用已有 Rust 成果；逐模块差分决定保留、替换、补齐，不一律从零重写，也不因旧 Rust 协议已存在而继续沿用。

仅改变宿主实现语言与 Source 引擎：**Rust + EmmyLua 直接分析库/native worker/session，替代 LuaLS**。其余全部业务、同一份 Lua addon、CLI、版本化 Skill、交付及完整 **Lycheedev mailbox protocol v1** 都要与固定正式版本等价。

性能目标是每条必需路径的**可控耗时减少至少 70%，Rust ≤ Go 的 30%**。同机器、同数据、等价结果与校验，cold/warm 分开独立重复；下载及不可控外部等待单列。尚无本次测量证明达标，禁止删功能、少校验或缩小输入取巧。

## 文档入口

- [handover.md](handover.md)：当前决策、状态、证据与缺口的唯一事实来源；需要用户或实机配合的测试全部在最后一节。
- [DESIGN.md](DESIGN.md)：从上述事实来源派生的目标架构和兼容规则。
- [todo.md](todo.md)：有顺序的执行及验收清单，不另写竞争的状态叙述。
- [固定 Go 协议](https://github.com/Follen/Lychee-Dev/blob/0bec60e4406f5d5dc155ee25cef44f9a710e37e8/docs/live-protocol.md)：完整线协议、恢复、输入和诊断合同。

旧 `docs/toolkit/` 计划与测试报告保留为历史证据；旧 Go 3.1.2 目标、50%性能门槛、四槽 exact-COMMIT 重设计及旧输入假设均不能覆盖本次目标。

## 当前代码与构建边界

`crates/` 已有 cli、core、source、data、live、delivery、windows、emmy-adapter、binding-contracts 九个 crate。自有 adapter 集中引擎接入；低层 binding 合同由自有 crate 提供，通过兼容重导出供固定引擎共享。Source 默认 `emmy-prototype` 并直接链接 EmmyLua。原先缺失的修改版依赖已由公开上游固定 commit `08b0873962e83bb8307e9a81052e3f41578fbc78` 加可审补丁重新构建，对应源码、补丁和许可证清单在 [third_party/emmy](third_party/emmy/README.md)。这是独立标识的新重建版本，不冒称恢复旧私有字节。准备脚本物化被忽略的依赖目录，Windows CI 已能构建并测试封存候选。

组件实际通过不等于完整 Source、默认 Live、最终安装包或性能已验收。完整 Retail 原生分析已暴露真实 API 生成错误，新修复须在精确候选上复跑；正式 runtime/input/backend 已接默认公共生命周期，精确候选验收仍未完成。精确通过/失败/未运行证据见 [handover.md](handover.md)，剩余模块见 [todo.md](todo.md)。独立可复建和最终打包门槛仍未完成。

交付平台仍为 Windows amd64；Source/Data/资源调查不依赖运行游戏，真实客户端只用于最后的 Live 验收。

## 本次范围

首批提交仅文档，用户随后已授权全量实施本计划；仍不发送游戏输入、不发布、不合入 main。每批更新同一独立草稿 PR，实现后按 handover 执行四阶段串行循环审计。先做离线与可自动执行的 Windows 检查，需要用户、真实游戏或特定交互机器配合的测试放最后。历史失败和产物必须保留；死代码须完成引用、feature、打包与回归核查后再删。

正式安装说明见[固定 Go README](https://github.com/Follen/Lychee-Dev/blob/0bec60e4406f5d5dc155ee25cef44f9a710e37e8/README.md)，不得把 npm 安装说成安装本分支未完成的 Rust 候选。旧 README 和工作台 UI 设计保留在 [docs/history](docs/history/)。
