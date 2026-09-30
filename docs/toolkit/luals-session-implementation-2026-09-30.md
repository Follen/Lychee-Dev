# LuaLS 批处理与受控会话实施记录

日期：2026-09-30。基线：`8e4dab0455c8bee512c73912c26b31bc9605f6c2`，分支：`codex/data-source-optimization`。
本页记录 [Source 审计](source-live-optimization-audit-2026-09-30.md) S3 和
[Phase D 会话合同](phase-d-publication-luals-session-2026-09-30.md) 的实现与本机证据。
没有执行 live/game 输入、修改 addon 版本或发布产品。

## 请求内批处理

`source refs` / `source context` 的 incoming、outgoing 和 hover 共用一次初始化与工作树租约。
incoming 使用 references + hover 两个查询；outgoing 保留原来的最多 128 个调用点。
合计最多 130 个逻辑查询，按每批最多 128 个拆成最多两批，共用原请求的 90 秒 deadline。
没有用 incoming 的两个查询扣减 outgoing 预算。投影分别保留方向覆盖和 truncation，
只解码实际 definition 目标的 path + line 声明记录。空查询仍直接返回覆盖信息，不启动 LuaLS。

默认模式在请求结束时关闭 worker。`--session-reuse` 是 refs/context 的显式选择；
静态查询、关闭该选项和语义缓存命中均不启动或唤醒 broker。
`source session status` 和 `source session close` 只访问已存在的会话，不创建后台服务。
`source prune` 先通过认证连接关闭有关会话、释放工作树租约，再裁剪工作树。
操作说明已同步至版本化 `references/source-research.md`；命令目录仍由 command contract 生成。

## 跨 CLI 所有权和身份

Windows named pipe 使用当前 logon SID 的显式 protected DACL，拒绝远程客户端。
连接验证操作系统返回的 server PID、创建时间和完整当前 CLI executable 摘要。
隐藏入口仅接受固定结构参数、一次启动 secret，以及仍存活且创建时间和 executable 匹配的父进程。
只启动当前 executable，LuaLS 路径来自验证过的 release；没有客户端可指定的 executable/shell 字段。

composition owner 保留精确工作树租约，绑定 runtime/release、source pin、index records、
API environment、generated definitions/config、parser schema 和实际工作树内容。
上下文变化先关闭旧 worker 并释放租约，再取得新租约，避免嵌套 lease 死锁。
release、工作树和生成 config/library 的文件变化由事件监测使会话退役；恢复原字节不能恢复旧内存状态。
每批仍验证密封 config/library 字节。目录 watcher 使用 overlapped I/O，关闭取消并 join。

LuaLS 在 suspended 状态先加入 Job 再恢复运行；broker 也有独立 Job 所有权。
连续 RPC pump 消费 idle 通知、回答允许的 server requests，并拒绝未知 response ID、坏 framing 和预算超限。
active cancellation、deadline、worker EOF 或协议错误关闭 owned Job 并回收进程；不等待可能永不返回的取消响应。
broker worker 和连接 handlers 完成 join 后才释放 owner 与 aggregate lease。
回退必须发生在确认退役并释放之后；无法证明所有权时失败关闭，不盲杀未知 PID。
尚未 admission 的坏客户端 IPC 只关闭该连接，不影响无关 worker；worker RPC 坏帧/EOF 则退役 worker。

## 有界资源

| 项目 | 当前实现上限 |
| --- | --- |
| pending queue | 16 项，另有 1 个 active caller；取消只移除自己的 pending 项 |
| IPC request / response | 64 KiB / 8 MiB |
| RPC 查询 | 每批 128 个；一次 Source 合成请求最多两批 |
| RPC 字节 | 每批 32 MiB，worker lifetime 256 MiB，包含双向流量 |
| owner lifetime | idle 60 秒、绝对 10 分钟、最多 256 个 RPC batches |
| session 临时产物 | 单独 `home/tmp/luals-session`，64 MiB、10,000 节点；拒绝 links/special files |
| Windows Job | broker 及其后代 aggregate commit 上限 2 GiB；LuaLS worker 另有 Job |

同一 logon 只允许一个 aggregate broker owner，跨 home 竞争也保守拒绝，避免多 home 叠加资源上限。
session 临时产物不写入 `source/v1`；source 的 4 GiB 容量核算由 S5 的容量锁治理。
启动清理仅处理已识别的 private session 标记目录，未知内容不当作可删缓存。
2 GiB 是安全上限，不是测得的 RSS，也不是完整 Blizzard 源码性能验收结论。

## 已执行的本机验证

使用已安装官方 Windows runtime，LuaLS 固定 `3.19.1`。fixture 都位于隔离临时 Git/文件目录。

- 真实 worker：相同 PID 的 warm batches、结果一致、active/readiness cancellation 后退役并重建。
- environment/config：number/string 生成 API 分别解析；source `.luarc.json` 无法插入 plugin/library；编辑配置使旧 worker 退役。
- 真实三个 CLI 语义 cache misses：保留同一 worker PID，batch 计数 1 → 2 → 3；cache hit 不增加 batch。
- 真实 `source prune`：先关闭 worker，随后可再次取得该工作树租约；初始 status 不创建服务。
- 单元/集成：空查询、128 + 2 拆批、方向投影、独立 wire budgets、长无换行 header、未知 response ID、pending queue 边界、取消自己任务、watcher 无事件关闭和 source 编辑后恢复。
- skill command contract 与 release license tests 通过；新增 `go-winio v0.6.2` MIT 许可记录，npm runtime dependencies 未改变。

最终重跑日志保存在工作树 `.tmp/luals-session-20260930/runtime-tests.log`、
`broker-cli-tests.log` 和 `contract-license-tests.log`；这些是本地忽略的验收产物，不随源码发布。
真实测试需要显式环境变量，缺少环境的 `SKIP` 不计作真实验收通过：

```powershell
$env:LYCHEEDEV_LUALS_TEST_RUNTIME = 'C:/Users/follen/AppData/Roaming/npm/node_modules/lycheedev/payload/tool/luals'
go test -count=1 -v ./internal/luals
go build -o .tmp/luals-session-20260930/lycheedev.exe ./cmd/lycheedev
$env:LYCHEEDEV_BROKER_TEST_BINARY = "$PWD/.tmp/luals-session-20260930/lycheedev.exe"
$env:LYCHEEDEV_BROKER_TEST_RELEASE = 'C:/Users/follen/AppData/Roaming/npm/node_modules/lycheedev'
go test -count=1 -v ./internal/codebase -run 'TestRealBrokerAcrossCLIProcesses|TestRelationBatchPreserves128OutgoingSites|TestProjectedSemanticAnalysisIsIndependent'
node tools/skill-contract.mjs
node --test tools/release.test.mjs
```

## 仍为 not_run

完整 Retail/Classic/Titan 源码上的 cold 5 次、warm 20 次 p50/p95、进程 RSS 和游戏帧耗时未执行。
其他 logon SID/远程端的攻击连接、等待完整 60 秒 idle / 10 分钟 lifetime、kill/crash/upgrade 全矩阵未完成实机验收。
代码合同与小 fixture 的通过不能替代这些结果。没有真实游戏或互动桌面证据。
