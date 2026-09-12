# Lychee Dev 内存往返能力调查（2026-09-12）

结论：wowdump 0.3.11 已能从当前正式服进程读取原始字节，但本轮没有验证到活的 LycheeDevDB、Ticket 或任务结果。工具没有内存写入、Lua 调用或游戏输入接口，因此不能凭现有能力实现“一个外部脚本投递任务、触发执行、读回结果”的完整往返。

用户随后优先启动独立 Lychee Performance Test。本调查的唯一扫描已正常退出；没有创建 watch、定时诊断或后台循环，收到暂停指令后未再读取游戏内存。共享 broker 保留空闲。后续内容来自已保存证据及磁盘源码。

## 目标与证据范围

`wowdump targets` 本次只发现一个目标，无须沿用旧 PID：

| 字段 | 本轮观测 |
| --- | --- |
| PID | 54936（仅本轮身份，后续必须重新发现） |
| 路径 | `D:/Game/World of Warcraft/_retail_/Wow.exe` |
| buildKey | `retail@12.1.0.69587` |
| moduleBase / moduleSize | `0x7ff74e2a0000` / 138768384 |
| executable SHA-256 | `bf514622471b799656eb860bca0a5fc449469ce3f494e89eefba3eeba35e0f09` |
| 扫描完成时间 | 2026-09-12 07:16:27.487 UTC（北京时间 15:16:27） |

原始证据目录：`C:/Users/follen/.wowdump/retail@12.1.0.69587/runtime/lychee-dev-roundtrip-20260912/`。其中 `target.json`、`broker-status.json`、`regions.json`、`scan-0.json` 和 `scan.mjs` 分别保存身份、实际引擎能力、页面范围、命中/读取记录和临时只读扫描器。该脚本使用现有 broker 的 `modules/read` 请求，并非新内存引擎。未读取或修改用户 SavedVariables。

当前正式 profile 目录只有 `character-stats.json`，不适用于 Lua/插件结果。静态数据库状态为 `creating`，不是现成的 Lua 根证明。本轮没有创建、提升或保存任何新正式 profile，也没有使用旧实时调试材料。

## 实际命令与读取结果

```powershell
wowdump targets
wowdump --version
wowdump memory --help
wowdump memory read --help
wowdump database status --build 'retail@12.1.0.69587'
wowdump targets --pid 54936
wowdump target
wowdump memory regions --pid 54936 --start 0x10000 --end 0x7fffffffffff --max-regions 20000
node 'C:/Users/follen/.wowdump/retail@12.1.0.69587/runtime/lychee-dev-roundtrip-20260912/scan.mjs'
```

CLI 版本为 0.3.11。broker 实际返回 `read, regions, modules, dump, watch`，权限仅 `PROCESS_QUERY_INFORMATION`、`PROCESS_VM_READ`。安装包 `dist/memory/windows.js` 和 `broker.js` 与本地 TypeScript 的对应能力一致：没有 `WriteProcessMemory`、`PROCESS_VM_WRITE`、`VirtualAllocEx`、`CreateRemoteThread` 或 invoke 分发。

页面枚举完整返回 6383 个区间；其中 `MEM_PRIVATE / PAGE_READWRITE` 合计 6768529408 字节。扫描仅顺序处理这一类别的一部分，搜索 `LycheeDevDB`、`LYCHEE-2026`、`LycheeDevInput` 和两种分隔符的插件路径。它不扫描保护页，不导出整个进程，保存命中附近约 212 字节上下文及每次请求状态。

扫描进程 exit 0，耗时 6301.54 ms，共 820 次请求：788 次完整读取，32 次失败或短读。完整块合计 536870912 字节（512 MiB），全部请求合计 559415296 字节，含短读实际返回 544096256 字节（约 518.89 MiB）。失败包括 Win32 299。

临时脚本的 512 MiB 预算计数只累计完整块，因此不能将它描述成总读取的硬上限；原始脚本及结果保持原样作为证据。恢复调查前应改为按请求字节计预算，并保留短读统计、时间上限及每块游标。第一次尝试因旧案例的 `reader-broker.json` 不存在而在连接前失败；随后按当前源码改为 `memory-broker.json`，才执行了上述唯一扫描。

最小有效读取证据：

| 地址 | 从内存读到的文本 | 能证明什么 |
| --- | --- | --- |
| `0x165a7760fd1` | `Lychee Dev\\addon_version.txt` | 该路径文本存在于目标进程 |
| `0x165a7761a21` | `Lychee Dev\\Core\\Compatibility.lua` | 同上 |
| `0x165a7762cb1` | `Lychee Dev\\Lychee Dev_Wrath.toc` | 连非正式服 TOC 也存在，路径命中不能代表已执行 |

59 个命中均为 `Lychee Dev\\`，集中在 `0x165a73f0000` 区间，形态与路径列表/缓存一致；尚未证明具体缓存结构。没有在已扫描完整块内找到另外三个专属标记。搜索范围不完整且有短读，不能据此断言插件未加载、DB 不存在或内存读取路线不可行。

没有恢复 Lua 全局根、没有读取 `LycheeDevDB.exports.records`，没有验证当前结果、同会话变化或跨重启复用。绝对命中地址只保留在本轮证据中，不是可复用 profile。

## 实际安装与执行链

磁盘安装目录为 `D:/Game/World of Warcraft/_retail_/Interface/AddOns/Lychee Dev`，是普通目录，无链接。Mainline TOC 标记 Interface 120100、版本 0.7.2，仍加载 `Modules/Performance.lua` 等旧模块，区别于已删除性能模块的隔离分支。因此版本字符串相同不代表代码相同；磁盘 TOC 和路径命中均不足以认证当前已加载代码版本。

以下仅是实际安装目录的静态代码证据：

- `Core/Bootstrap.lua:114` 的 `/dev` handler 不使用参数，只检查状态、初始化并切换窗口，不执行任务。
- `Core/Bootstrap.lua:69` 的 `ns.Execute(code)` 编译并 `pcall` Lua，返回状态与结果；环境能访问和写入 `_G`，不是强隔离沙箱。它是插件局部 namespace 接口，不是外部进程 RPC。
- `UI/MainWindow.lua:824` 的 `RunInput` 从 EditBox 取文本，调用 Execute，显示结果并调用 AddHistory；`:949` 将 Run 按钮点击绑定到它。输入到达和代码执行是两件事。
- `Core/Database.lua:117` 的记录结构将正文放在 `payload.content`，另存 `payload.byteCount`；`:314` 的 AddExport 生成 Ticket 并写入 `db.exports.records[ticket]`。Ticket 与正文在不同 Lua 字段中，不能假定内存连续。
- `UI/Export.lua:106` 的 Save 才调用 AddExport。Run 的普通历史不是自动产生的 Ticket 导出，完整链路需要明确结果发布步骤。
- 当前 `$lychee-dev` Skill 提供持久化证据读取及诊断操作指导，不含进程输入或内存传输实现。技能改名和安装入口保持原状。

## 能力表

| 环节 | 本轮结果 | 尚缺证据/实现 |
| --- | --- | --- |
| 发现当前 PID/build/hash | 已验证 | 每轮及重启后重新发现 |
| 外部脚本读原始内存 | 已验证，包括插件路径文本 | 读取不代表语义正确 |
| 定位活的 Lua/DB 根 | 未完成 | 根来源、布局、表项解析和身份验证 |
| 读完整新鲜任务结果 | 未完成 | 活根或自描述封包、长度/完整性、会话/任务对应 |
| 写入任务到游戏内存 | wowdump 不支持 | 不能补一个虚构 write 参数；需独立评审传输机制 |
| 触发游戏线程执行 | wowdump 不支持，未尝试游戏输入 | 接收入口、游戏线程上的执行事件、明确执行确认 |
| 自动收发完整往返 | 未实现、未验证 | 接收确认、执行确认、结果发布与读回全部闭合 |

即便未来某工具可以写内存，也不能将改 Lua 字符串/表的字节等同于合法赋值或执行：对象布局、分配器、GC、内部一致性、线程时机都需要单独解决。写一个任务号也不会自动产生 Lua 事件。本轮不另造写入器、注入器或函数调用器。

## 候选：插件保留自描述结果字符串

此方案仍是设计候选，未编码、未部署、未实机验证。可以让明确启用的开发功能在结果完成时构建一个完整、受字节预算约束的字符串，并由插件持有当前引用。字符串内同时包含固定协议标记、协议版本、客户端信息、会话 nonce、外部指定 taskId/request nonce、结果序号、执行状态、正文 UTF-8 字节长度、完整性校验与结束标记。正文应有确定编码，避免正文内容冒充头尾。

外部首先做有界发现，再仅对候选小范围重读；验证完整封包、请求 nonce、长度与校验后接受。读取前后头尾一致只能提高一致性，不能把非原子内存读取变成原子快照。校验用于发现截断/混读，不是来源认证；旧副本也能校验通过。

Lua GC 后旧字符串可能仍残留在未覆写内存；分配、复制、UI 文本和历史记录也可能产生多个合法副本。保留当前引用能防止当前字符串被回收，不能自动标记外部扫描找到的那个副本就是当前对象。必须用本轮外部 request nonce 与会话身份排除旧结果，并明确重启和超时失效；没有传入通路就无法完成这种请求关联。缓存地址需要重新验证，读失败、标记不符或会话变化后有限重新发现，不可无限全堆扫描。

优势是减少对 Lua Table/TString 固定字段偏移的依赖；仍有进程访问、页面变化、编码、版本协议、分配和 GC 行为、扫描范围及客户端更新的维护成本。不是“零 build 维护”。本次约 6.3 秒扫描只覆盖部分私有页，不能将其当作低成本日常轮询或精确性能基准。

## 推荐的下一步边界

目前继续让独立性能测试独占观测窗口。不要为这份报告恢复扫描。性能测试结束且恢复调查获授权后，可沿原生内存读取路线继续补证据，不把已搁置的 SV/reload 方案改成定案。

1. 优先定位现有 DB/结果；修正扫描预算后只扩大有依据的范围，或者静态定位 Lua 根。记录根语义与字段语义，不能把路径命中提升为插件已加载证明。
2. 若现有数据不足，再准备独立、明确启用的最小探针供审阅：仅发布带唯一 nonce 的短结果，无轮询、无 SV 修改，给出短手动入口及准确加载动作。需要游戏实际加载探针才能验证；本轮未准备安装或要求用户执行。
3. 传入必须独立立项验证。候选是经过实际能力核查的原生 UI 输入适配器加插件内显式接收/执行入口；它可以由同一个外部协调脚本调用，但不是 wowdump 内存写入，也尚未证明 PID 定向、后台输入或焦点可靠。当前工具无此现成能力，未向游戏发送任何输入。
4. 最小协议明确区分 `received`、`running`、`succeeded/failed`；接收确认只表示输入校验通过。任务必须有 nonce、大小界限、过期/重复处理策略，不能无确认地自动重试可能有副作用的执行。异步任务以实际完成回调发布终态。
5. 用无副作用的唯一 nonce 任务验证接收、执行、结果逐环节对应，再测试同会话第二次任务、旧结果残留、对象变化与用户主导的重启。跨 build 必须另行认证。之后才更新 Skill 为真实可用的协调流程。

## 变更与验证状态

本轮只新增这份隔离工作树报告，并在 wowdump runtime 目录保存临时调查证据。未修改插件运行代码、Skill、游戏安装或 SV；未覆盖剪贴板、输入命令、重载、终止或重启客户端。未部署、合并或推送。没有执行任何诊断 Lua，也没有成功任务执行确认。

静态证据文件 SHA-256：

| 文件（上述实际安装路径） | SHA-256 |
| --- | --- |
| wowdump `dist/memory/windows.js` | `aa36b9fd32e99cace23abb4a59cb3c00c978f29ae0ffa4d996ba21eb46822444` |
| wowdump `dist/memory/broker.js` | `c3aa2fdb666d088680a3a187a7183174290c559d78836bfd8fa5098e8489e28f` |
| Lychee Dev `Core/Bootstrap.lua` | `b8bc341a54755ab01acd1f760133a284ded50e06efefc98780c470d61f071e39` |
| Lychee Dev `Core/Database.lua` | `3929e0f5fded6fc39c20d7f4444e082b42199770e537aa85537b3eebc33615ba` |
