# D 阶段研究合同：addon 发布与跨请求 LuaLS 会话

日期：2026-09-30。Lychee Dev 基线：`8e4dab0455c8bee512c73912c26b31bc9605f6c2`，发布版本 3.0.2。
对应 [source/live 审计](source-live-optimization-audit-2026-09-30.md) 的 L6、S3 和阶段 D。

状态：**研究与候选合同已记录；生产实现、部署、完整性能验收均未完成。**
本页约束后续这两项优化的实现，不把研究原型提升为当前产品能力；现行 native 200 槽、输入节奏、零成本和 release 合同继续有效。
本轮没有向游戏发键、连接、reload、修改安装或发布版本。原型仅在独立 Lua 5.1 和临时 LuaLS 工作区运行。

## 决策

| 方案 | 结论 | 理由 |
|---|---|---|
| L6-A：复用编码中的稳定数据，严格等价的 Describe 去重 | 可进入小步实施 | 不新增发布通道；但应先确认真实热路径收益，不能只凭重复调用探针估计 |
| L6-B：保持新鲜采样，改成自包含的紧凑 input 载荷 | 保留为下一层候选 | 可减少序列化/候选校验成本；需要新 schema、能力协商和跨语言 fixture |
| L6-C：静态发现 beacon + 动态状态分离 | 本轮不选择 | 新增记录与关联读取；现有 descriptor 已是事件驱动，收益证据不足 |
| L6-D：固定地址、原地写 Lua 字符串、共享原生缓冲 | 不采用 | 缺少受支持的客户端接口；普通 Lua 字符串不能作为可原地写的进程通信缓冲 |
| LS-A：单请求内合并 incoming/outgoing/hover | 优先实施路径 | 生命周期与租约仍由当前 CLI 拥有，收益与成本较易验证 |
| LS-B：按需、有界、可关闭的跨 CLI LuaLS broker | 有条件可实施，初期 opt-in | 真实进程小样本确认复用有效，但新增租约、IPC、预算与取消生命周期 |
| LS-C：一个 LuaLS 进程动态切换多个 pin/environment | 初期不采用 | workspaceFolders 支持不等于环境与旧缓存完全隔离，验证代价高 |
| LS-D：默认永久 daemon / 从退出 CLI 脱离未托管子进程 | 不采用 | 不满足清晰的所有权与资源回收要求 |

本轮已经把“必要时研究”推进为上述具体决策；尚不能把 D 标记为产品实施或实机验收完成。

## 固定依据

- LuaLS 以 [release/tools/luals.json](../../release/tools/luals.json) 为权威：3.19.1、Windows amd64，archive SHA256 `fdb9a59108cf62517813c97fa5549b0e16d1ef0688306bac728b08434db7e4cd`。
- 上游 3.19.1 tag 解引用到 `d11e79dc2745b5bfe654490eff234c6be2f6606f`。本轮克隆该提交并查阅 provider、workspace、config 和 proto；没有使用浮动 HEAD 判断随包 runtime。
- 安装包中的 proto/proto.lua、provider/provider.lua、workspace/workspace.lua、config/config.lua 与该 tag 文件逐字节一致，包含取消队列分支；对照保存在 [本机校验记录](../../.tmp/phase-d-research-20260930/upstream-shipped-compare.json)。
- LSP 依据 Microsoft 仓库提交 `f1b0378161a72018a2abeb5794532d5f6e2aebac` 中的 3.17 文档。3.17 是协议兼容依据，不声称它是当前最新规范。
- addon API 和行为依据当前仓库与 [实施状态](implementation-status.md) 的固定客户端基线；独立 Lua 5.1 不证明 WoW 的 allocator、GC 时序或内存地址行为。
- 当前约束：[设计](design.md)、[200 槽合同](live-slot-routing-2026-09-29.md)、[输入节奏实现](input-cadence-implementation-2026-09-29.md)、[回归矩阵](regression.md)。

## 一、L6：不可变记录、分配与发现信息发布

### 1. 当前发布分为三类，不能一起减少

| 类别 | 现有拥有者 | 更新原因 | 可优化内容 | 不能省掉的内容 |
|---|---|---|---|---|
| Identity descriptor | SlotProtocol + SlotRuntime | startup、bind/unbind、wake 结束、槽耗尽及部分失败 | 稳定字段预处理、严格相同记录复用 | actor、owner/fence、nextSlot、能力、header sequence 的变化 |
| InputState | 一个 sampler/display | Start 立即、1s 周期、世界/加载/战斗事件、wake/close 后 Refresh | 编码实现、稳定 token/字段复用、之后的紧凑格式 | 每次真实观察、新 sampleMillis/sequence、门禁和 heartbeat |
| Receipt/HEAD/BODY | SlotProtocol 的操作表 | prepare/commit/finish/confirm/release 等转换 | 已验证的编码优化及现有有界释放 | 新 nonce 相关回执、HEAD 授权、BODY/digest、墓碑和未知操作证据 |

代码：[SlotProtocol](../../addon/Bridge/SlotProtocol.lua#L38)、[SlotRuntime](../../addon/Bridge/SlotRuntime.lua#L25)、[InputState](../../addon/Bridge/InputState.lua#L19)、[MemoryProtocol](../../addon/Bridge/MemoryProtocol.lua#L21)。
Describe 并非每秒发布：SlotRuntime 在一次 wake 结束后调用；InputState 才是持续刷新项。
InputState 的 provider 返回当前身份，sample 再读取输入条件和时钟，最后发布内存与光学状态。
旧字符串仍可能被 GC 留在 heap；替换 Lua 变量不等于立即擦除其进程字节。

### 2. 本轮发布探针：两个容易误判的点

独立 Lua 5.1.5，加载当前 CaptureWriter/MemoryProtocol/SlotProtocol，没有改变生产文件：

| 场景 | 观察 |
|---|---|
| 初始 Describe + 100 次状态未变的 Describe | JSON 与 wire 各编码 101 次，最终记录字节相同；fixture descriptor 长 428 字节 |
| bind 内部 Describe 与 Receive 返回后的 Describe | identity JSON 载荷相同，完整 record 不同：中间 publish 增加了 header sequence |
| 消耗下一空槽 | 描述符必须变化，即使没有新业务结果 |
| 连续 ready 的 120 次新输入样本 | 120 条不同记录，fixture 平均 360 字节；sampleMillis 和 sequence 每次变化 |

这证明存在可复用的稳定编码工作，但没有证明正常操作经常触发完全重复的 Describe。
不能把人工 100 次重复调用算成 99% 的真实收益，也不能假定每个 bind 的两次 Describe 都可去重。
360/428 字节只描述该 fixture，不是实际角色名字、客户端或完整运行的平均值。

### 3. L6-A 候选合同：保持格式和所有观察事实

后续实现必须满足：

1. **先观察和检查，再命中缓存。** 每次 Describe 仍调用 actor adapter，先拒绝 secret、失效角色和非法值；不能因为上一条 descriptor 有缓存就跳过当前身份检查。
2. **等价键覆盖全部输出。** 包含 runtime、actor、build/product/release、inventory、inputState 能力、owner/fence、nextSlot、slots 与 header sequence。只用 JSON 载荷、owner 或 nextSlot 作为键不够。
3. **只保留当前有界缓存。** 每个 engine 至多当前 descriptor 和其稳定编码部件；不增加所有历史 descriptor 的 ring，不新增 payload 历史。
4. **发布事实保持同步。** 空/外来/畸形槽消耗、拒绝、bind/unbind、序号变化、actor 不可用和错误返回都维持当前合同。旧 heap 字节存在不能被解释为当前 runtime 仍有效。
5. **InputState 不做整条不变去重。** ready 没变化也继续采样、增加 sequence、更新 sampleMillis；保持 after+100ms、500ms、发键前地址复核及光学门禁。
6. **稳定部件可预处理。** runtime/zero 的二进制 token、固定字段的验证和引用，可在所属 engine/sampler 中有界保存；不能把 secret 检查、UTF-8、字节限制或 checksum 整体去掉。
7. **checksum 始终对应最终字节。** 如果复用固定字段编码，组合顺序、数字格式、转义和头尾必须与当前 encoder 等价；不要引入无法证明等价的 checksum 拼接捷径。

建议先在现有 MemoryProtocol/SlotProtocol 内部实现，调用者继续使用原来的 Describe/Encode seam。
只有固定部件缓存确有多个调用点时才增加私有 helper；不要为相同功能再加一层公开 transport。
维持 API 单一性；客户端差异进入 Compat 或生成目录，不散布在 encoder。

### 4. L6-B：紧凑载荷可以减少字节，不能消除新字符串

紧凑格式的候选优先选择**自包含输入记录**，而非靠外部 static beacon 补齐授权事实。
保留 runtime（可由已验证 envelope 提供）、owner、fence、nextSlot、GUID、build、sampleMillis、inputBlocked 与 reason 的完整意义；只有已证明可从 envelope 无歧义恢复的重复字段才可不在载荷重发。
actor 及槽位等动态授权事实不能只从上一次发现缓存补齐。

候选需要独立 schema/capability；旧 `lycheedev.input.v1`/`hybrid.v1` 不变。宿主先按明确能力选择 decoder，未知版本拒绝；不能把新二进制载荷交给旧 JSON decoder 后猜测回退。
协议迁移通过版本一致的 clean managed addon 安装与现有 reload/reconnect 流程，不能 overlay-copy；初期不同时常驻发布新旧两套采样记录，否则增加分配和歧义。

长度/整数/字符串合同应明确：sampleMillis 与 fence 的数值范围、大小端、nextSlot 1..201、缺失布尔值、reason 代码与未知原因的 fail-closed 规则、UTF-8 和总载荷边界。
Lua number 在本合同范围内保持精确整数；不能借二进制 uint64 的存储能力放宽超过 Lua 可精确表示的值。
reason 可以有受限代码加有界文字，不能把未认识的受阻原因解成 ready。

此路线仍每次创建不可变字符串；减少的是 JSON 组装、重复字段和 checksum 工作量。
即使记录缩短一半，游戏进程可读 private heap 总量也未必显著下降，不能据此推导全扫描耗时减半。
只有 L0 指标证明编码/候选校验或 retained 副本占重要成本，才进入该 schema 变更。

### 5. 为什么本轮不选择 static beacon 或固定地址

Identity 现在已按事件发布，频率远低于 InputState。再加静态记录会增加类型、关联读取与 GC 副本；静态字符串也不提供宿主可直接获得的固定 VA。
Lua 官方的 [字符串说明](https://www.lua.org/pil/2.4.html) 和 [Lua 5.1 GC 说明](https://www.lua.org/manual/5.1/manual.html#2.10) 支持不可变值与自动回收的语言语义；并不证明 WoW 使用同一 allocator、驻留策略或不搬迁实现。
不要通过保存字符串引用、`tostring`、普通 Lua table 或 checksum 推断宿主地址，也不能要求强制 collectgarbage 来服务 transport。

如果将来重新评估拆分，合同至少须包含：

- static identity 的摘要/版本与 dynamic state generation；每条新鲜输入样本精确关联所属 generation。
- 关联读取的完整身份和 checksum，缺少任何一侧时拒绝，不按“各取最大 sequence”组合。
- 新发现只能提供候选；旧 GC 留存的完整 static/dynamic 组合仍不证明当前 runtime。必须保留真实字节变化或 nonce 回执及现有 runtime replacement 证明。
- actor 变化、reload、bridge off、partial scan、多 PID、旧新 generation 共存和清理路径。

当前没有支持原地写字符串/共享原生缓冲的 pinned WoW API 证据。不引入 FFI、进程注入、写内存、原始指针/offset 依赖或新的 OS 通道。

### 6. 生命周期和零成本合同

L6-A 不创建 frame/event/hook/timer，不增加 OnUpdate；L6-B/C 的后续设计也不能新增常驻发布器。
基础 loader/binding 及 owner 已批准的输入 sampler 是现有预算例外，不能把它们算成“禁用完全没有 frame”的新承诺，也不能借例外给任意功能常驻预算。
显式 bridge off 继续 Stop：撤掉事件/OnUpdate/显示回调，清除采样记录并停止 StartupBeacon；Refresh、wake 失败或 session 查询不能偷偷复活 sampler。
已有 bind/receipt/tombstone 仍由其所属生命周期拥有，不能为了少占字节抹掉未收尾操作。
reload/relogin/新 runtime 不继承缓存；Start 重入复用同一 sampler，Stop 重入不产生新工作；加载/离开世界、时钟失败/回退、secret 或观察异常保持 fail-closed。

## 二、LuaLS：跨请求复用的可行性和必要合同

### 1. 现有 seam 为什么只能用于一次调用

[AnalyzeWorkspace](../../internal/luals/lsp.go#L85) 每次创建 definitions/config/log/meta，启动 LuaLS，initialize + initialized，经过 worktree scope 的 definition barrier，执行 query，然后 shutdown/exit。
[Windows Job](../../internal/luals/process_windows.go#L15) 采用 kill-on-close；当前进程/取消拥有子树，不允许无人管理的 worker。
[AcquireWorktree](../../internal/codebase/worktree.go#L53) 持有确切 checkout 的独占 OS 租约，完成全部 blob 校验，结束请求后释放。

现有 LSP client 也是一次调用设计：绑定一个 context，顺序 request ID，单一 event channel，累积 32 MiB 接收预算；通知主要在 request 等待期间消费。
把它直接挂进全局 map 会产生跨请求取消、迟到 ID、空闲通知排空和无限累计预算问题。
当前完整语义结果缓存已是跨请求复用的一种形式，必须先查它；命中缓存的请求不应为了“热会话”启动 LuaLS。

### 2. 真实 LuaLS 小工作区原型

用已安装 3.0.2 的完整 inventory 验证 runtime，再通过公开 stdio LSP 执行临时 a.lua/b.lua 工作区：

| 场景 | 最近一次原型结果 |
|---|---|
| 一次 initialize + worktree barrier | 827.9 ms |
| 第一次 definition | 1.23 ms |
| didClose 后重新 didOpen，同一 server 第二次 definition | 1.03 ms，与第一次相同 |
| 现有一次性 AnalyzeWorkspace 的启动/barrier/query/退出 | 830.5 ms，结果与暖查询相同 |
| 同 source、不同 generated environment | 分别独立 server，hover 为 number/string，各自正确 |
| 发请求后立即发 cancel，等待 2s | 没有收到该 request ID 的响应；退役进程，重新初始化后下一请求正确 |

这是一次短样本的小工作区真实进程实验，不是完整 Retail/Classic/Titan 源码性能回归，也不是两个独立 CLI 连接 broker 的测试。
冷启动和就绪等待合计远大于本 fixture 的查询，支持复用有价值；不能声称完整产品从 830 ms 降到 1 ms，worktree 重新验证、IPC、结果投影和缓存仍有成本。

### 3. 取消实验发现了一个具体上游边界

最初按“cancel 后等待其响应再复用”的原型，在 45s 会话 deadline 后失败。该失败证据保留，没有删除或当作通过。
上游 [proto.applyMethodQueue](https://github.com/LuaLS/lua-language-server/blob/d11e79dc2745b5bfe654490eff234c6be2f6606f/script/proto/proto.lua#L207) 先收集同批 cancel ID，再跳过相应排队请求；该分支不执行正常请求 handler 的响应收尾。
[cancel handler](https://github.com/LuaLS/lua-language-server/blob/d11e79dc2745b5bfe654490eff234c6be2f6606f/script/provider/provider.lua#L1374) 只关闭已 holdon 的请求。
这与 [LSP 取消仍应返回响应](https://github.com/microsoft/language-server-protocol/blob/f1b0378161a72018a2abeb5794532d5f6e2aebac/_specifications/lsp/3.17/specification.md#L353) 的预期存在差别。

随后将取消恢复改为 2s 有界等待，未收到响应就结束该进程；重建后 definition 正确，实验通过。
2s 是这次原型的回收观察窗口，不是新的生产查询 deadline。
现有产品在取消时结束一次性 Job，不依赖复用，所以本发现不能直接宣称当前 3.0.2 的取消功能坏了。

候选 broker 初版选择更简单的合同：**活动语义请求被取消或超时，退役其 worker session；不复用等待状态不明的进程。**
可尝试发送 cancel/shutdown，但回收不能无限等待其响应。排队且尚未发送的请求取消仅移除自己的队列项。
之后其他请求重新 cold initialize，沿各自原始 deadline 和预算执行；不能因重建替取消请求重跑或延长期限。

### 4. 候选 module 与所有权

外部调用只需要“使用某一确切语义上下文执行有界 queries”，以及 composition 持有的 Close/回收能力。
broker 的 IPC、初始化、worker、lease、排队、统计、失效、退出都藏在这一 module 的 implementation，不能要求每个 Search/Relate/Context 各自管理状态机。
`internal/luals` 继续只处理已验证 runtime/workspace 的 LSP，不反向导入 codebase、delivery 或游戏逻辑；上层 composition 负责身份/租约与 release 验证。

建议层次：

```text
source 调用：结果缓存 → 固定语义上下文/预算 → 会话拥有者
会话拥有者：单次 CLI adapter 或可选 broker adapter
worker：一个确切上下文 → verified runtime + worktree lease → stdio LuaLS
```

初版 broker **每个 toolkit home 只保留一个 worker**；所有请求有界串行，不先做多 pin server 池。
同 key 的连续 cache miss 可复用；key 变化先停止旧 worker、释放租约，再打开新上下文，不能动态改同一进程的 definitions。
这牺牲多 pin 并发性能，换取可验证的配置隔离、容量与取消行为。多 worker 必须另有跨 home 的资源准入证据。
broker 只处理 source 语义任务，不复用 live 的 owner、connection、input 或游戏资源。

### 5. 会话身份必须比路径或版本号更完整

至少绑定以下指纹：toolkit release/manifest commit、broker wire revision、LuaLS version/archive/inventory、toolkit home 规范路径、source repository/product/确切 commit/tree、parser/recordsHash、environment 来源/输入/definitions/mappings 摘要、完整有效配置摘要、workspace 覆盖政策、query projection 与语义缓存政策版本。
同路径不同文件内容、相同 commit 不同 product/environment、相同 LuaLS version 不同文件摘要，都不能复用。
文档 URI 只允许该会话验证的 workspace 与 definitions；不能把调用方任意绝对目录直接交给 broker 执行。

发布安装在原目录被升级、库存校验失败、配置变化、工作区改变或进程身份失效时，旧 session 退役。
旧、新 CLI 的 broker wire/version 不一致时拒绝复用，按已确认的冷路径处理，不能接管未证明所属的进程。
broker endpoint/元数据只是发现线索，PID 创建身份、manifest/协议握手和合法 OS 所有权才决定可连接实例；禁止按 stale PID 盲杀。

### 6. Worktree 租约迁移是实施前置条件

worker 使用期间由**会话拥有者**持有 worktree lease；请求结束不能释放但让 server 继续读，idle 回收顺序是停 worker/确认退出 → 释放 lease → 删除其暂存 definitions/log/meta。
当前 CLI 的 AcquireWorktree 与 broker 再次获取同一个独占锁会互相等待；不能保留旧调用链再在里面追加 broker。
后续实现应把取得/验证 worktree 的职责移动到会话拥有者，或采用能明确转交所有权的内部 seam；不能传一个目录名就假装转交了 OS lease。

共享目录仍受 source/v1 的真实容量核算保护。prune 遇到活跃 lease 可跳过并诚实报告不能完全回收；用户显式回收此上下文时需通知 broker 退役 idle worker，再按现有规则 prune。
cold fallback 前先停自己的热 worker 并释放其 lease，否则同一 home 的一次性 adapter 会被自己的锁挡住。
broker 不可达但 lease 尚被未知存活者占用时，保持 fail-closed，不覆盖锁、不换 pin、不盲杀；按现有 source 能力/预算/partial 错误合同报告。

lease 只保护受协议管理的删除/写入，不阻止同用户外部编辑。每次 admission 仍重新验证不可变源码、definitions/config/inventory；外部变化通知只可触发失效，不能代替内容证明。
发现改写时停止 worker，而非让恢复到相同磁盘字节的旧内存缓存被视为已重新加载；需要明确验证、重新 initialize 和 readiness。
初版宁可保留此验证成本，也不能仅靠 mtime/size 宣称输入不变。

### 7. LSP 生命周期、文档和 IPC

- 每个 worker initialize 一次；`processId` 对应真正拥有 server 的 broker，不能保留已经退出的 CLI PID。标准依据：[initialize](https://github.com/microsoft/language-server-protocol/blob/f1b0378161a72018a2abeb5794532d5f6e2aebac/_specifications/lsp/3.17/general/initialize.md#L10)。
- 维持 worktree scope 的 readiness barrier，不能只对 generated API library 查询就宣称整个 workspace ready；上游 [awaitReady](https://github.com/LuaLS/lua-language-server/blob/d11e79dc2745b5bfe654490eff234c6be2f6606f/script/workspace/workspace.lua#L571) 依赖所属 scope。
- didOpen 用验证过的确切文本和明确 version；同 URI 不重复打开未关闭文档。每批结束 didClose，后续重新打开。didClose 是内容所有权变化，不是立即释放全部语义缓存；见 [LSP didClose](https://github.com/microsoft/language-server-protocol/blob/f1b0378161a72018a2abeb5794532d5f6e2aebac/_specifications/lsp/3.17/textDocument/didClose.md#L3) 及 [LuaLS handler](https://github.com/LuaLS/lua-language-server/blob/d11e79dc2745b5bfe654490eff234c6be2f6606f/script/provider/provider.lua#L287)。
- 始终有读取/分发循环消费 server 通知及允许的 server requests，空闲也不能让 channel/pipe 堵塞；通知按类别截断或统计，合法 request 必须得到响应，未知协议请求导致有界失效。
- 使用会话级唯一 ID 与 request→caller 关联；迟到消息不能归入下一请求。队列取消和发送后取消分开，request deadline 与 worker lifetime 分开。
- 单帧 8 MiB、单批 queries≤128 等现有界限继续生效；每批 I/O 与整个 session 累计预算分开，不能把现有 32 MiB 计数简单清零造成 lifetime 无界，也不能永久累积导致热会话必然失效。
- 初版采用当前登录身份范围的本机 named pipe。设明确 DACL/登录 SID、拒绝远程和限制创建权限；默认 ACL 不是足够的产品隔离。依据：[Microsoft pipe 权限](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)。
- broker 协议只接受结构化身份/queries/deadline/budget；不是任意命令执行接口，不接收客户端提供的 runtime executable 或任意 shell 字符串。
- shutdown/exit 正常收尾有界，失败时关闭自身 Job 回收 worker 子树；不允许任意 breakaway。依据：[Microsoft Job 生命周期](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)。

### 8. 配置隔离不能因热会话放宽

继续固定 `--configpath` 与 generated definitions，禁止 runtime.plugin、third-party/外部 library、telemetry、addon manager 等未授权行为；repo 的 `.luarc` 不能覆盖安全键。
上游 [updateConfig](https://github.com/LuaLS/lua-language-server/blob/d11e79dc2745b5bfe654490eff234c6be2f6606f/script/provider/provider.lua#L32) 同时处理 override 和 folder/fallback，
[workspaceFolders 变化](https://github.com/LuaLS/lua-language-server/blob/d11e79dc2745b5bfe654490eff234c6be2f6606f/script/provider/provider.lua#L246) 会创建/删除 scope 并 reload；支持这些消息不证明所有 linked library、fallback 和缓存已消除。
初版因此一个 worker 一个 source/environment/config，切换则退役，不用 didChangeWorkspaceFolders 做 pin 切换。

不能为了加速重写 pinned checkout 的 `.luarc`；若创建过滤后的 sealed copy，它也必须是完整固定身份、预算、容量与回收链的一部分，不能隐藏排除范围。
已有 [真实配置隔离用例](../../internal/luals/runtime_test.go#L111) 要扩展到热会话、多批、配置 watcher、两个环境和重建路径。

### 9. 激活、预算与回收候选

跨 CLI 模式初期明确 opt-in；不在 doctor、普通 source search、非 semantic 查询或 cache hit 时偷偷启动 broker，不装 Windows 服务/计划任务。
会话关闭后无 worker、watcher、Job 或持久运行 broker；活跃与空闲有可查的所有权/资源状态。这里的零空闲成本指未启用或完全回收，不能声称已启用的热 LuaLS 不消耗内存。

供原型的参数候选为：每 home 1 worker、最多 16 个排队批次、idle 60s、absolute lifetime 10min 或 256 批次先到者回收。
这些是**待测参数**，不是当前默认或实测容量。工作空间数量、机器 RAM、常用查询间隔和完整源码 RSS 未测，暂不给生产 RSS 阈值。
后续必须在 workspace 层完成 aggregate 准入，同时计 LuaLS/worker 子树、temporary definitions/log/meta、checkout/缓存磁盘和排队载荷；不能每个 Agent/home 独立申请整机额度。

查询排队、验证、冷启动、readiness、LSP、重建和结果投影共享原 deadline；不能因为热会话不存在就重置 90s 或更上层任务期限。
worker 资源超预算、malformed frame、EOF、退出、升级或 active request 取消时退役；结果维持 partial/unsupported/truncated，不把能力错误变成 complete 空结果。
只允许尚未取消的只读请求在原期限内有界 cold 重试；重试次数和消耗不重置。正常结果缓存仍只封存完整、固定身份的输出。

sessionReuse 的配置/命令若进入产品，要先进入 command contract，再由 describe/help/skill 生成；本页不广告尚未实现的 broker 命令或开关。

### 10. 收益门槛与何时停止投入

先完成 S1 查询快照复用、S3 单请求合并以及 L0 阶段计量，再测多个不同符号的真实语义 cache miss。
比较 one-shot、单次合并和 warm broker：结果/capture、验证时间、启动/readiness、查询/投影、命中率、上下文切换、RAM/CPU/磁盘、退出和取消响应。
完整源码用 ≥5 次 cold 和 ≥20 个不同 query 的 warm 样本，报告 p50/p95 和所有失败/回退；重复同一已缓存符号不能用来估计 broker 收益。
如果 queries 很稀疏、pin/environment 频繁切换、现有结果缓存已覆盖大多数调用，或验证/投影仍占主导，保留 one-shot 作为默认，停止扩展常驻模式。
上线门槛是同任务总耗时与资源收益，不是本实验的 1 ms LSP 响应。

## 三、验收与需要 owner 配合的范围

### 1. 无需 owner 操作即可完成的部分

- 代码/合同、pinned LuaLS 与 LSP 源码调查，独立 Lua probe，真实 LuaLS 隔离实验与失败复现。
- L6-A 的 byte-for-byte、secret、长度/转义/序号、actor/slot/error、disabled/enabled、Stop/Start/reload fixture。
- LuaLS 结果等价、环境/config 隔离、排队/取消、EOF/错误 ID/notification flood、lease/prune/升级、Job 退出、跨 CLI 竞争与预算测试。
- 固定源码 pin 的冷/暖 source 基准和文档维护，不需要登录游戏。

这不意味着上述所有未来实现测试本轮已经执行；本轮只完成下一节列出的两个原型。

### 2. 需要 owner 的实机验收，待候选准备好后安排

本轮研究不要求现在切客户端、开启桥或改游戏设置。研究文档更新可以完成，实机收益仍保持 `not_run`。
若后续实施 L6，按以下两轮安排，时间是准备估计，不是已完成验收：

| 轮次 | owner 配合 | 工具/agent 工作 | 预计窗口 |
|---|---|---|---|
| 首轮 Retail 对照 | 登录指定角色，在安全场景正常站立/移动；手工切桥 on/off、打开带草稿聊天框、观察输入和卡顿；按明确步骤 reload/relogin | 提供候选与回滚包，走 clean managed 安装；采集编码/GC/扫描/时延/WGC 证据；跑有界任务并收尾 | 约 20–30min，视恢复和完整操作数调整 |
| 后续矩阵 | Classic/Titan 指定 build；同安装双实例；按需低帧率/战斗/加载场景，人工确认草稿/焦点与玩法无变化 | 固定每个 PID/创建身份/角色，复现任务集合，比较冷/暖与 cache off，记录失败并恢复原 CON | 分场次安排，不用一次坐等全部客户端 |

战斗和受保护交互由 owner 手工进行；不靠自动键猜测或改 protected UI 来制造场景。
配置和安装操作只有进入实际实施/测试范围后才执行；使用当前授权的目标，不自动切角色或把未知输入重发。
互动桌面验收由 owner 标记，agent 的 WGC/结构化证据作支持；fixture 不能代签真实客户端通过。

若只实施 LuaLS 复用，不需要游戏配合。可以先用已固定的源码工作区基准；只有真实工作习惯中的 query 间隔、常用 pin 切换和资源偏好需要补充时再收集，不作为本轮文档工作的阻塞条件。

### 3. 验收矩阵

| 范围 | 必须验证 | 本轮状态 |
|---|---|---|
| L6 原始行为 | 同状态重编码、bind 头序号、空槽与 fresh sample | standalone probe 通过 |
| L6-A/B 实现 | 等价缓存/字段、停启/异常/容量、跨语言新格式 | not_run，未实施 |
| L6 真实客户端 | CPU/GC/RPM/heap 噪声、WGC、输入/帧时间、reload/relogin/升级、多 PID/三客户端 | not_run |
| LuaLS 基本复用 | initialize once、多 didOpen/query/didClose、fresh 对照 | 真实 3.19.1 小 fixture 通过 |
| LuaLS 取消 | 无界 drain 失败；2s 后退役与新建恢复 | 失败及修订后通过均保留 |
| LuaLS 环境 | 两个独立 server 的 number/string definitions | 小 fixture 通过，不证明一 server 多环境 |
| 跨 CLI broker | 身份/IPC/DACL、队列/租约/idle/崩溃/升级/aggregate 预算 | not_run，未实施 |
| 完整源码收益 | 冷/暖、结果缓存、pin 切换、p95/RSS | not_run |

## 四、本轮证据与复现

本机原始证据：[发布探针](../../.tmp/phase-d-research-20260930/publication-probe.log)、[LuaLS 修订后](../../.tmp/phase-d-research-20260930/session-probe.log)、[取消无界等待失败](../../.tmp/phase-d-research-20260930/session-cancel-unbounded-failure.log)。
`.tmp` 是本机忽略目录。下方保留完整源码，避免调查依赖临时文件存活。

发布 probe 传 addon 根目录；使用 Lua 5.1.5。LuaLS probe 通过 Go overlay 映射到虚拟 `internal/luals/audit_session_overlay_test.go`，生产文件不改写；`LYCHEEDEV_AUDIT_RELEASE` 指向完整安装的 3.0.2 release root。
它先调用 release inventory 验证，不使用任意下载路径直接启动 executable。

```text
lua publication_probe.lua <addon-directory>
go test -count=1 -overlay <session-overlay.json> ./internal/luals -run TestAuditPersistentLuaLSSession -v
```

取消失败复现可将原型的 2s timer 分支去掉，等待 session context：本轮在 45s 后超时。不要把这一失败探针加入产品通过门禁。
当前修订版证明的是有界退役/重建策略，不能代替跨 CLI broker、完整源码或真实客户端验收。

本轮文档与原型收尾验证：`go build ./...`、`go vet ./...`、真实 runtime 的 `go test -count=1 ./internal/luals`，
以及强制 Lua 5.1 的 `go test -count=1 ./tests/addon ./tests/protocol ./protocol` 均通过。
文档本地引用和行号边界检查通过；生产代码未改动，没有 commit 或发布。此前同生产 HEAD 的强制 Lua 全量测试记录继续适用，未以本轮局部测试冒充新增生产实现的全量验收。

### 发布探针完整源码

```lua
local root=assert(arg[1])
local ns={}
for _,path in ipairs({"Bridge/CaptureWriter.lua","Bridge/MemoryProtocol.lua","Bridge/SlotProtocol.lua"}) do
    assert(loadfile(root.."/"..path))("Lychee Dev",ns)
end
local runtime=string.rep("1",32)
local actor={character="Research",realm="Fixture",guid="Player-1-1"}
local jsonCalls,wireCalls=0,0
local wireEncode=ns.MemoryProtocol.Encode
ns.MemoryProtocol.Encode=function(...)wireCalls=wireCalls+1;return wireEncode(...)end
local engine=ns.SlotProtocol.Create({runtime=runtime,build="120100.69933",product="retail",release="3.0.2",inventory=200,
 inputState="lycheedev.input.hybrid.v1",actor=function()return actor end,
 encode=function(...)jsonCalls=jsonCalls+1;return ns.CaptureWriter.Encode(...)end,
 compile=loadstring,execute=function()error("business execution outside research")end})
local initial=assert(engine.Describe())
for _=1,100 do assert(engine.Describe()==initial)end
print(string.format("unchanged Describe: json=%d wire=%d recordBytes=%d sameBytes=true",jsonCalls,wireCalls,#initial))
local bind={schema="lycheedev.slot.v2",index=1,runtime=runtime,owner=string.rep("2",32),fence=1,
 nonce=string.rep("3",32),ticket=string.rep("4",32),action="bind",guid=actor.guid,build="120100.69933"}
assert(engine.Receive(1,bind))
local inside=engine.Snapshot().descriptor
local after=assert(engine.Describe())
assert(inside~=after,"bind internal/outer Describe unexpectedly equal")
assert(inside:sub(85,#inside-40)==after:sub(85,#after-40),"identity payload changed without new identity state")
print("bind internal/outer Describe: payloadSame=true recordSame=false (header sequence changed)")
engine.Receive(2,nil)
assert(engine.Describe()~=after,"slot cursor change must invalidate descriptor reuse")
local inputBase={schema="lycheedev.input.v1",runtime=runtime,owner=bind.owner,fence=1,nextSlot=3,
 guid=actor.guid,build="120100.69933",inputBlocked=false,reason=""}
local unique,last,total={},nil,0
for sequence=1,120 do
 inputBase.sampleMillis=100000+sequence*1000
 local text=assert(ns.CaptureWriter.Encode(inputBase,2048))
 local record=assert(wireEncode(runtime,runtime,string.rep("0",32),5,1,sequence,text))
 assert(record~=last);unique[record]=true;last=record;total=total+#record
end
local count=0;for _ in pairs(unique)do count=count+1 end
print(string.format("unchanged ready state, 120 fresh samples: uniqueRecords=%d averageRecordBytes=%.1f",count,total/120))
print("probe is standalone Lua 5.1; no game heap, no RPM, no addon input")
```

### LuaLS 会话原型完整源码

```go
package luals

import (
    "context"
    "encoding/json"
    "os"
    "os/exec"
    "path/filepath"
    "reflect"
    "strings"
    "testing"
    "time"

    "github.com/follenfang/lycheedev/internal/delivery"
)

// Lab only: persistent stdio under one test owner, not a cross-CLI broker.
type auditSession struct {
    client *lspClient
    workspace, library string
    shutdown func()
    retire func()
}

func auditOpenSession(t *testing.T, r *Runtime, files map[string][]byte, defs []byte) *auditSession {
    t.Helper()
    dir, workspace, library, _, clean, err := frozenWorkspace(files,defs)
    if err!=nil {t.Fatal(err)}
    ctx,cancel:=context.WithTimeout(context.Background(),45*time.Second)
    cfg,err:=configuration(library);if err!=nil {cancel();clean();t.Fatal(err)}
    cfgPath:=filepath.Join(dir,"config.json")
    if err=os.WriteFile(cfgPath,cfg,0600);err!=nil {cancel();clean();t.Fatal(err)}
    command:=exec.CommandContext(ctx,filepath.Join(r.directory,"bin","lua-language-server.exe"),
        "--configpath="+cfgPath,"--logpath="+filepath.Join(dir,"log"),"--metapath="+filepath.Join(dir,"meta"),"--quiet")
    command.Dir=dir;command.WaitDelay=2*time.Second;hideProcess(command)
    in,err:=command.StdinPipe();if err!=nil {cancel();clean();t.Fatal(err)}
    out,err:=command.StdoutPipe();if err!=nil {cancel();clean();t.Fatal(err)}
    var stderr cappedWriter;command.Stderr=&stderr
    closeJob,err:=startGuarded(command);if err!=nil {cancel();clean();t.Fatal(err)}
    client:=newLSPClient(ctx,in,out)
    s:=&auditSession{client:client,workspace:workspace,library:library}
    s.retire=func(){cancel();closeJob();_=in.Close();_=command.Wait()}
    s.shutdown=func(){
        if ctx.Err()==nil {_=client.request("shutdown",nil,nil);_=client.notify("exit",nil)}
        s.retire();clean()
    }
    t.Cleanup(s.shutdown)
    rootURI:=fileURI(workspace)
    var init json.RawMessage
    if err=client.request("initialize",map[string]any{
        "processId":os.Getpid(),"rootUri":rootURI,
        "workspaceFolders":[]map[string]string{{"uri":rootURI,"name":"phase-d-fixture"}},
        "capabilities":map[string]any{"general":map[string]any{"positionEncodings":[]string{"utf-16"}},
            "workspace":map[string]any{"configuration":false,"workspaceFolders":true},
            "textDocument":map[string]any{"definition":map[string]any{"linkSupport":false},"references":map[string]any{}}},
    },&init);err!=nil {t.Fatal(err)}
    if err=client.notify("initialized",map[string]any{});err!=nil {t.Fatal(err)}
    // Match the production worktree-scope preload barrier.
    s.open(t,"a.lua",files["a.lua"])
    var barrier json.RawMessage
    if err=client.request("textDocument/definition",map[string]any{"textDocument":map[string]string{"uri":fileURI(filepath.Join(workspace,"a.lua"))},"position":Position{}},&barrier);err!=nil {t.Fatal(err)}
    _=client.notify("textDocument/didClose",map[string]any{"textDocument":map[string]string{"uri":fileURI(filepath.Join(workspace,"a.lua"))}})
    return s
}

func (s *auditSession) open(t *testing.T,name string,data []byte) string {
    t.Helper();uri:=fileURI(filepath.Join(s.workspace,name))
    if err:=s.client.notify("textDocument/didOpen",map[string]any{"textDocument":map[string]any{"uri":uri,"languageId":"lua","version":1,"text":string(data)}});err!=nil {t.Fatal(err)}
    return uri
}

func (s *auditSession) query(t *testing.T,name string,data []byte,pos Position) []Location {
    t.Helper();uri:=s.open(t,name,data)
    defer s.client.notify("textDocument/didClose",map[string]any{"textDocument":map[string]string{"uri":uri}})
    var raw json.RawMessage
    if err:=s.client.request("textDocument/definition",map[string]any{"textDocument":map[string]string{"uri":uri},"position":pos},&raw);err!=nil {t.Fatal(err)}
    locations,cut,err:=decodeLocations(raw,s.workspace,s.library)
    if err!=nil||cut {t.Fatalf("decode=%v cut=%t",err,cut)}
    return locations
}

func TestAuditPersistentLuaLSSession(t *testing.T) {
    root:=os.Getenv("LYCHEEDEV_AUDIT_RELEASE")
    if root=="" {t.Skip("verified installed release required")}
    r,err:=Open(context.Background(),root,func(ctx context.Context,root string)error{
        _,err:=delivery.InspectRelease(ctx,root,"3.0.2");return err
    });if err!=nil {t.Fatal(err)}
    files:=map[string][]byte{"a.lua":[]byte("local M = {}\nfunction M.answer() return 42 end\nreturn M\n"),
        "b.lua":[]byte("local M = require('a')\nlocal x = M.answer()\n")}
    start:=time.Now();s:=auditOpenSession(t,r,files,nil);cold:=time.Since(start)
    start=time.Now();one:=s.query(t,"b.lua",files["b.lua"],Position{Line:1,Character:13});first:=time.Since(start)
    if len(one)!=1||one[0].Path!="a.lua" {t.Fatalf("unexpected definition: %+v",one)}
    start=time.Now();two:=s.query(t,"b.lua",files["b.lua"],Position{Line:1,Character:13});warm:=time.Since(start)
    if !reflect.DeepEqual(one,two) {t.Fatal("warm output changed")}
    t.Logf("one initialize, two didOpen/query/didClose batches: cold+barrier=%s first=%s warm=%s definition=%+v",cold,first,warm,one)
    start=time.Now()
    fresh,err:=r.AnalyzeWorkspace(context.Background(),s.workspace,nil,[]Query{{Kind:Definition,Path:"b.lua",Position:Position{Line:1,Character:13}}})
    if err!=nil||len(fresh.Results)!=1||!reflect.DeepEqual(one,fresh.Results[0].Locations) {t.Fatalf("fresh equivalence: %+v err=%v",fresh.Results,err)}
    t.Logf("fresh one-shot startup/barrier/query/shutdown=%s, output equals warm",time.Since(start))

    // Cancellation still produces a response. Drain its ID before reuse.
    uri:=s.open(t,"b.lua",files["b.lua"])
    s.client.nextID++;cancelID:=s.client.nextID
    if err=s.client.send(map[string]any{"jsonrpc":"2.0","id":cancelID,"method":"textDocument/definition","params":map[string]any{"textDocument":map[string]string{"uri":uri},"position":Position{Line:1,Character:13}}});err!=nil {t.Fatal(err)}
    if err=s.client.notify("$/cancelRequest",map[string]int{"id":cancelID});err!=nil {t.Fatal(err)}
    cancelCode:=0
    receivedCancelReply:=false
    timer:=time.NewTimer(2*time.Second);defer timer.Stop()
drain:
    for {
        select {
        case <-s.client.ctx.Done():t.Fatal(s.client.ctx.Err())
        case <-timer.C:
            s.retire()
            t.Log("cancel response absent after 2s: retire process instead of unbounded wait")
            break drain
        case event:=<-s.client.events:
            if event.err!=nil {t.Fatal(event.err)}
            msg:=event.message
            if msg.Method!="" {
                if len(msg.ID)>0 {if err=s.client.send(map[string]any{"jsonrpc":"2.0","id":json.RawMessage(msg.ID),"result":nil});err!=nil {t.Fatal(err)}}
                continue
            }
            var id int;if json.Unmarshal(msg.ID,&id)!=nil||id!=cancelID {t.Fatalf("unexpected reply %s",msg.ID)}
            if msg.Error!=nil {cancelCode=msg.Error.Code;if cancelCode!=-32800 {t.Fatalf("unexpected cancellation code %d",cancelCode)}}
            receivedCancelReply=true
            break drain
        }
    }
    reuse:=s
    if !receivedCancelReply {reuse=auditOpenSession(t,r,files,nil)} else {
        _=s.client.notify("textDocument/didClose",map[string]any{"textDocument":map[string]string{"uri":uri}})
        t.Logf("cancel response drained (code=%d; 0 permits already-completed request)",cancelCode)
    }
    if three:=reuse.query(t,"b.lua",files["b.lua"],Position{Line:1,Character:13});!reflect.DeepEqual(one,three) {t.Fatal("request after cancellation recovery changed")}
    t.Logf("request after bounded cancellation handling correct; reusedOriginal=%t",receivedCancelReply)

    // Same fixed source, distinct generated environments need distinct servers.
    envFiles:=map[string][]byte{"a.lua":files["a.lua"],"use.lua":[]byte("Widget.value\n")}
    for _,typ:=range []string{"number","string"} {
        defs:=[]byte("---@class Widget\n---@field value "+typ+"\nWidget = {}\n")
        env:=auditOpenSession(t,r,envFiles,defs)
        uri:=env.open(t,"use.lua",envFiles["use.lua"])
        var raw json.RawMessage
        if err:=env.client.request("textDocument/hover",map[string]any{"textDocument":map[string]string{"uri":uri},"position":Position{Line:0,Character:8}},&raw);err!=nil {t.Fatal(err)}
        hover,cut,err:=decodeHover(raw);if err!=nil||cut {t.Fatal(err)}
        if !strings.Contains(hover,"Widget.value: "+typ) {t.Fatalf("environment leaked or missing: %s",hover)}
        t.Logf("isolated definitions type=%s hover=%s",typ,hover)
    }
}
```
