# 双向 Mailbox：独立架构审查与推荐方案

日期：2026-10-04。状态：**下文保留设计阶段审查；当前实现和原生写入限制以 [实施合同](live-duplex-implementation-2026-10-04.md) 为准，不是生产安全验收**。

本审查按用户明确请求编写，纳入 inbox/sendbox、明确 ready、轮询、取消、断开、GC 容错、移除色块和 200 LoD、单一“Agent执行中”提示等要求。本轮只新增方案文档，不修改生产代码、不操作游戏。

审查基线为 main 952a4fcd276b222aa27fa801ed36e525a24445a2。实机研究保存在本地研究提交 fcd0f7374b5b020951d8dc95452c4ee4522e6d3e 的 Analyze/memory-inbox，尤其是 README.md 和 acceptance.json；本方案工作树不复制其二进制、现场日志和内存地址。

## 1. 推荐结论

设计一代全新的 **CLI→addon inbox、addon→CLI sendbox** 协议，保留进程身份、runtime、owner/fence、prepare challenge、准确执行结果及持久化确认，取消普通通信对按键、色块和 LoD 槽的依赖。用户已明确要求本分支不保留 LoD 方案：新版本删除全部 LoD 输入及槽位运行依赖，不打包备用 LoD，不做 LoD 降级或旧协议兼容。

推荐第一版采用：

1. 一个当前业务请求；一个预分配、固定结构、完整容纳 1048576 字节（1 MiB）源码的数值请求区。数据用 uint32 打包，每个数字保存 4 字节。按 256 个 4 KiB 逻辑分片组织，分片只占本请求自己的固定位置；它们不是业务槽，也不规定必须各自分配一个 Lua 表。
2. 独立的、定长的控制区，至少分开 bind/resume、commit、cancel、close、result_ack、reload、lease 七条控制 lane。close 不等待 cancel lane 腾空，cancel 不等待业务请求结束。
3. sendbox 发布不可变状态记录、独立控制回执、prepare challenge、业务终态清单及结果页。明确 ready，并同时公开 transportReady、businessReady、controlReady。
4. addon 在明确启用后使用有界轮询；私有状态每次接收和真正执行前重新验证。禁用时没有轮询、事件、计时器或动画工作。
5. 只有**准确业务终态 + 宿主结果持久化 + result_ack + addon RELEASED**，才允许正常复用业务请求区。分片 ACK 不能替代业务终态。
6. 同步 Lua 卡住、重载竞态、结果丢失不能被协议包装成“已经取消”。给出有限等待、原 ID 恢复、明确放弃及准确宿主占用退役路径，但不承诺所有情况自动解锁。

**研究写入器尚不能直接成为生产传输；这不改变删除 LoD 的目标。** 研究已证明数值消息可以到达 Lua，尚未证明所有 GC/reload 时刻的地址生命周期安全。普通协议可以设计完整；外部 WriteProcessMemory 与 Lua 对象释放之间没有现成生命周期锁。若“任何正常 GC/reload 时序下都不能写到失效对象”是硬性投产门槛，仅靠本方案的纯 Lua 协议和外部进程写入不能证明满足，应继续研究具备受支持生命周期保证的传输原语；门槛未满足时该 build 明确不可用或阻止发布，不能回退 LoD。

生产切换一次换代。已发布旧版本的历史事实不构成本分支保留旧传输代码、兼容层或备用安装载荷的理由。本轮是方案和 Skill 文档改动，尚未执行生产代码删除；后续实现必须完成第 19 节的全链路删除。

## 2. 实际证据与不能外推的部分

### 已观察事实

| 证据 | 支持的结论 |
| --- | --- |
| Retail 12.1.0.69933、同一 PID/creation、CLI/addon 3.1.1 的实机研究 | 在这个准确 build 和进程中，已存在的 Lua number payload 可以由独立 CLI 写入 |
| 0、1、-13.5、0.125、4294967295 的全部位校准 | 该现场样本匹配 number tag 3、clear secret、little-endian binary64 |
| 1234567→7654321，经游戏 Lua 独立读取及一次完整 GC 后读取 | 这次分配在这次 GC 前后仍被保留并可读取 |
| 128 字节 00..7f、sum=8128、两次 Consume 的 executions 都为 1 | 本次有界消息成功消费一次；重复读取结果未重复执行 |
| challenge/checksum/nonce 错误均拒绝，executions=0 | 原型的对应拒绝分支在这些实例上生效 |
| 正例消费区间 consumed slots 为 15→15 | 数据传输及一次定时消费不需要追加 LoD；初始化和结果生命周期仍使用现有 LoD |
| 原 reload 恢复后旧 namespace 不存在，旧身份 inspect 拒绝且 writes=0 | 这次 reload 后旧实验身份未被重新当作当前写入资格 |

128 字节的宿主发布耗时为单次样本 1502 ms，含根定位、反复 guards 和 journal；142 次写入、110954 次具名读取、1873824 字节具名读取。它不是消费者延迟、吞吐量基准，也不能线性外推新目标 1 MiB 的生产耗时。消费者刻意等待了 45 秒。

### 源码事实

- 当前 [Mailbox 合同](live-mailbox-protocol-2026-10-01.md) 是公开只读结果发布 + 200 LoD 输入，不是双向内存通道。
- [memory/lua_mailbox.go](../../internal/live/memory/lua_mailbox.go) 和 [lua_mailbox_recipe.go](../../internal/live/memory/lua_mailbox_recipe.go) 提供有界读取及 root recipe，不提供外部写入资格。
- [SlotProtocol.lua](../../addon/Bridge/SlotProtocol.lua) 的 prepare 接受最多 262144 字节源码；现有私有 prepare/commit/结果身份检查应作为语义要求保留，不直接沿用槽位字段。
- [CaptureWriter.lua](../../addon/Bridge/CaptureWriter.lua) 的结果上限为 512 KiB、深度 32、条目 32768；当前 checksum 为 Adler32，不是 Lua 端 SHA256。
- [ProbeExecution.lua](../../addon/Bridge/ProbeExecution.lua) 明确不是同步 CPU sandbox。计时器、Callback 和 OnCleanup 有界，但不能抢占同线程同步 Lua。
- 当前 native CON 的取消、activation 退役和断开路径不完整。业务预算先失败、普通 Driver journal 不存在、业务事务不静止，均可能挡住清理。本次研究通过只针对一个 CON 的临时恢复工具证明了准确宿主占用可独立退役；那不是通用产品实现。

### 仍未证明

重复自然 GC、增量 GC 与每个外部写入交错；collector 是否对所有目标 build 保证对象不移动；数组型 TValue 布局；其他 build；真实多 writer；未进角色/切角色全过程；纯 Lua SHA256 成本；大请求与大结果的轮询帧成本；同步无限循环可取消性；任意 reload 时刻的无失效地址写入。

**一次 collectgarbage 成功不能证明“以后每次 GC 都不会回收”，也不能证明对象永不移动。**

## 3. 必须长期成立的不变量

1. 一个进程创建实例只允许一个当前执行 owner；一个 session 只允许一个未退役业务请求。控制请求不算第二个业务请求。
2. 所有业务和控制消息同时绑定 runtime、session、owner、fence，以及对应 request/control ID；PID/creation/exe hash 由宿主 OS 观察独立绑定。
3. 私有状态负责授权。公共 ready、哈希、状态表、回执镜像的存在不授予执行权限。
4. 接收、校验、prepare、commit accepted、实际开始、业务终态、结果落盘、释放是不同事实，不能互相替代。
5. 时间戳是审计字段；跨端新鲜度不建立在两端时钟相等的假设上。
6. SHA256 是内容完整性和身份关联字段，不是密码、签名或恶意同机进程/同 Lua 环境插件的身份认证。
7. 没有准确终态就不正常复用当前业务内容。unknown 不是 success、failed 或 cancelled。
8. cancel_requested、cancel_accepted、cancelled、cancel_too_late、execution_unknown 分开呈现。
9. 关闭和结果取回独立于业务预算；关闭预算耗尽不会续长原业务、重新发代码或删除证据。
10. 断开或宿主退出不代表游戏执行被取消；退役宿主 claim 不代表 addon 私有 owner 已释放。
11. 外部 writer 只改已创建、已验证、非 secret 的 number payload；不写 tag、pointer、string、table header、代码页、对象引用或函数。
12. 所有 buffer、页数、队列、历史、工作片段和等待都有上限。达到上限时明确阻塞/拒绝，不能偷偷丢弃当前未确认消息。

## 4. 模块及接口：把复杂度放在一处

~~~mermaid
flowchart LR
    CLI[CLI / Skill] --> Coordinator[Session Coordinator]
    Coordinator <--> Journal[Durable Journal / Result Store]
    Coordinator --> Transport[Duplex Transport]
    Transport --> Inbox[Mailbox.inbox]
    Sendbox[Mailbox.sendbox] --> Transport
    Inbox --> Receiver[Addon Receiver]
    Receiver --> Protocol[Private Session Protocol]
    Protocol --> Executor[Probe Executor]
    Protocol --> Sendbox
    Executor --> Activity[ActivityView: Agent执行中]
~~~

推荐四个深模块，而不是让命令入口、内存 adapter 和 Lua UI 分别推进状态：

| 模块 | 小接口负责什么 | 隐藏的实现 |
| --- | --- | --- |
| Session Coordinator | Connect / Execute / Cancel / Disconnect / Status / Resume / Abandon | 原 ID、预算、日志、控制优先级、结果持久化和准确退役 |
| Duplex Transport | Observe / PublishFrame / PublishControl / ReadResultPages / CloseHandles | profile、具名路径、数值编码、guards、页面检查、8 字节写入事实 |
| Private Session Protocol | 接受已复制的 frame/control，产生状态和回执 | challenge、owner/fence、单请求、幂等、高水位、准入和资源释放 |
| Probe Executor | Start / RequestCancel / Observe | 已登记异步资源、有限 cleanup、真实执行终态；不假装拥有同步抢占 |

Transport 必须返回事实，如未调用 WPM、写入字节数、读回一致、部分/未知、当前回执；不能自行创建新业务、换角色、续预算或重放 opaque 操作。虚拟内存 fixture adapter 与 Windows adapter 通过相同接口测试；真实不同 build 的布局在 profile/Compat 内集中处理。

所有下文命令语义都是拟议的新代接口要求，不能先写入已实现命令目录。尤其当前 live cancel/abandon 不应被描述为已经支持这里的 CON 语义。

## 5. 身份分层及未进角色

| 层 | 标识及验证方 | 作用 |
| --- | --- | --- |
| 安装 | canonical installation path、receipt digest、release；宿主检查 | 判断匹配且干净的受管产品，不替代运行时证据 |
| OS 进程 | PID + processCreationFILETIME + exact image path + executable SHA256；OS 查询 | 拒绝 PID 重用、同 build 另一个实例、不同可执行文件 |
| 窗口 | 当前 HWND→同一进程创建实例；宿主查询 | 诊断及必要的物理激活路由；不是纯内存消息的授权字段 |
| VM/runtime | addon runtime token + private generation + 观察到的 root path | reload、logout、VM 重建失效分界；裸地址不是身份 |
| Arena | arenaGeneration + transportAttempt + 当前 writePermit | 同 runtime 内重建后的物理区域边界；禁止沿用旧地址/旧许可 |
| Actor | 当前普通值 GUID + build/product；addon 每次业务准入/执行重新检查 | 防止切角色、未进世界、secret/不可用 actor 被误用 |
| Session | session token、owner token、fence、协商后的固定 capabilities | 一个合作宿主 owner；拒绝旧 driver |
| Request | request ID、request sequence、request SHA256、prepared challenge | 精确内容、prepare/commit 及结果关联 |
| Control | lane、control ID/sequence、target request ID、control digest | 独立且幂等的取消/关闭/确认 |

addon 不能直接证明 Windows PID 或 executable hash；宿主不能用安装目录名推断当前角色或产品。两侧验证的证据要分别记录，不能伪称由对方独立认证。

未进角色、登录界面、插件未加载、bridge 关闭、布局不支持、读取失败，都可能表现为没有当前 sendbox。**没有发布不等于“肯定未登录”或“肯定没安装”。** 只有已验证的 login-state reader 或用户提供的明确现场事实，才能进一步标记未登录。

默认 connect 在没有可绑定 runtime 时返回 awaiting_runtime；不在未知阶段持续发送 /reload，不抢占其他实例。等待安装/登录的本地意图不能长时间占用游戏执行 writer；确有已提交物理激活的历史意图则保留准确 claim 和输入结果，按第 13 节退役。

## 6. 内存拓扑：推荐完整请求区，先不做流式覆盖

以下为逻辑结构；sendbox 的权威记录实际编码为有版本、长度和完整性检查的不可变字节串。CLI 不依赖公开 Lua table 的可变业务对象。

~~~text
LycheeDevInternal.Mailbox
  schema = lycheedev.duplex.v1             新代名称草案
  release / runtime / arenaGeneration / layoutId
  inbox                                  仅宿主写 number payload
    request
      manifest                           定长数字字段，当前业务元信息
      frames[1..256]                     每逻辑帧定长 header + 1024 uint32 words
    control
      bindResume                         bind/resume/repair，小型、独立发布
      commit
      cancel
      close
      resultAck
      reload
      lease
  sendbox                                仅 addon 发布，宿主只读
    status                               ready / reasons / heartbeat / identity
    controlReceipts                      每条 lane 当前回执 + 固定终态摘要
    transfer                             当前请求 frame 接收高水位/bitmap
    prepared                             私有数据已验证后的 challenge
    terminal                             业务终态及准确结果 manifest
    resultPages[1..32]                    每页最多 16 KiB，不可变
    released                             最后准确退役请求的 tombstone
~~~

### 业务请求区

原型每个字节一个 number；新方案每个 number 存一个 uint32，按 little-endian 展开成 4 字节。保持数值范围 0..4294967295，binary64 可精确表示，不使用 52/53 位打包，不把 64 位整数塞进一个 double。

最多 262144 个 payload number，分为 256 个固定 4 KiB 逻辑片。若经独立验证的数组布局仍为每个 TValue 24 字节，单份 payload 存储为 262144×24=6291456 字节，即 6 MiB；这只是按布局计算的 payload 存储量，不包含 table、header、私有源码、编译对象、结果及 allocator 开销。故障同时保留 active+retiring 两份时，仅 payload 就为 12 MiB。禁止把这些估算当成实测内存峰值。

逻辑分片是传输/校验/CPU 工作粒度，不是独立业务生命周期或 GC 单位。底层可使用一份固定数组，按 offset 分片，也可使用经 profile 验证的固定分块数组；实际布局须统一进 layoutId 并单独验证，不能临时混用。两种布局都必须由同一 arena 的强引用图保留全部 cells。单片结构异常按整代 arena 故障处理，不在旧地址单独补造它。

每个固定 frame 有自己的 publication stamp、request ID/sequence、index、offset、byteCount 和 SHA256。分片 ACK 说明该片已复制校验，可继续写**同一请求的下一个未使用片**；它不允许覆盖这个已接收片，也不允许开始第二个请求。

源码全部占自己的固定位置，直到准确业务终态和 RELEASED 才复用。这样严格满足“未收到明确业务结束，不写另一份新业务内容”，而不需要给“覆盖已接收分片”引入隐含例外。

header、frames、控制 cells 在首次启用时完整创建并校验，再发布写入能力。数字数组的读取/定位是新的 layout 能力：旧原型只验证了 string-key hash 节点，**不能把数组 offsets 当作已验证**。如目标 build 暂未证明数组布局，必须拒绝生产写入；不要悄悄退回大规模 hash 表或猜测偏移。

### 为什么暂不选双 bank 流式复用

2×4 KiB bank 可以少占常驻内存，但同一请求的大源码需要多次覆盖 bank。它依赖“frame_copied 可以释放物理页，而业务终态才释放逻辑请求”的额外语义。该方案可行的前提是用户明确接受这种解释，且私有重组、重传、ACK 丢失和 ABA 测试完备。

v1 正常运行只使用一套完整 arena；不增加第二份可接收业务的完整请求 bank，也不增加业务队列。第 14 节故障修复可额外保留一套已撤销接收资格的 retiring arena，内存峰值须包含它。后续性能/内存证据表明有必要时，再评估流式 bank。两者不能在同一协议版本中由 sender 临时猜测选择。

### Sendbox 与控制区的边界

sendbox 的页和状态可以由 Lua 替换为新不可变记录；宿主只读，读取不一致就丢弃该次观察。外部可写 inbox 的 backing 对象则不得因 ACK、取消、disconnect、lease 到期或暂停轮询被释放/换表。

addon 在创建完成后**不清空或重写 inbox 的数值 cells**；接收高水位和准入状态放在私有状态/sendbox。这样每个方向只有一个实际 writer。宿主在获得精确 reusePermit 后才写下一代内容；无须清零全部旧 payload，未使用片和尾部按新的长度、request ID 与 frame generation 排除。

## 7. 字段、编码、时间和 SHA256

### 固定公共头

每个 request/control frame 都包含：

| 字段 | 建议编码/语义 |
| --- | --- |
| protocolVersion、layoutId、kind、lane、headerBytes | 固定整数枚举；未知版本、枚举、保留位或长度拒绝 |
| runtimeToken、sessionToken、ownerToken | 每项 128 位，4 个 uint32；绑定准确私有状态 |
| arenaGeneration | 128 位，4 个 uint32；绑定当前 backing 对象，新分配必换代 |
| transportAttempt | 64 位 hi/lo；同一逻辑请求获准重新传输时递增，不重置原执行预算 |
| fence | 64 位拆为 hi/lo 两个 uint32 |
| requestId / controlId | 128 位；业务与控制 ID 分开 |
| requestSeq / controlSeq / publicationSeq | 每项 64 位 hi/lo；Lua 按字典序比较，不转换成 double 大整数 |
| actorBindingId | 私有 session 绑定的 actor token；sendbox 同时公开准确 GUID，私有执行门禁仍核对当前 GUID |
| createdUtcMillis | 宿主 UTC 毫秒拆 hi/lo；只作审计，结果原样回显；JSON 使用十进制字符串 |
| senderElapsedMillis / budgetMillis | 有界相对值；不拿宿主时间减 Lua GetTime |
| payloadBytes、frameCount、frameIndex、offset、frameBytes | 有界 uint32；连续、无重叠、无越界；尾 word 未使用字节为零 |
| requestSHA256、frameSHA256、headerSHA256 | 标准 SHA256 原始 32 字节，各用 8 个 uint32 打包；展示用 64 位小写 hex |
| publicationBegin、publicationEnd | 相同的稳定偶数 64 位 stamp；构建期为下一奇数/未发布状态 |

SHA 字节按标准摘要顺序存储；将原始摘要字节打包成 little-endian words 只是传输，不颠倒 SHA 摘要显示顺序。源码按原始 byte sequence 校验，不自动转换换行、Unicode、BOM 或 JSON escaping。

必须有一份 Go/Lua 共用的 wire 字节定义：字段顺序、宽度、domain separator、长度、reserved zero、digest 字段的排除规则全部固定。建议分别定义：

- requestSHA256 = SHA256(domain_request || immutable request manifest || exact source bytes)；
- frameSHA256 = SHA256(domain_frame || identity + request ID/sequence + index/offset/length || exact frame bytes)；
- controlSHA256 = SHA256(domain_control || lane + identity + target IDs + control fields)；
- resultSHA256 = SHA256(domain_result || request identity/digest + outcome metadata || exact result bytes)。

hash 自身字段和 publication stamps 不进入自身摘要；除这些明确排除项外，所有影响解释/执行的字段都进入摘要。不能只哈希 body，却让 opcode、budget、target 或长度可独立改变。

逻辑 request digest 与物理 transport digest 分开：requestSHA256 包含原 request ID、业务种类、actor 绑定、原预算/创建信息和原始源码，不包含 arenaGeneration、transportAttempt、当前 publication 或修复许可。后者连同 runtime/session/owner/fence 必须进入每帧 headerSHA256/frameSHA256 和控制 digest，并由当前私有状态校验。因此第 14 节只重传同一业务时可以保持 requestSHA256 不变，同时拒绝旧物理世代；这不是把传输身份排除出完整性检查。实际 wire 规范须枚举这两组字段，不能以模糊的 identity 一词代替。

**addon 必须独立计算 SHA256 才能声称“游戏端 SHA256 校验”。** 现有 Adler32 不能以字段改名替代。拟新增经过 golden vectors 验证的纯 Lua 5.1 实现，使用明确的 32 位运算语义；bit/bit32 可用性和符号行为必须逐 client 能力验证，不能凭经验假定。没有可靠实现或帧预算不合格就返回 capability unavailable，而不是降级为“宿主自己比对过”。

建议测试 empty、abc、55/56/63/64/65 字节、128 字节二进制、含零字节、最大源码、最大结果，以及完整 header+body。算法定义参考 [NIST FIPS 180-4](https://csrc.nist.gov/pubs/fips/180-4/upd1/final)；SHA256 不提供发送者认证。

### 时钟及新鲜度

timestamp 用于解释日志，不用于决定权限。host durable deadline 用宿主自己的时间规则；addon challenge/lease/执行预算用自身已验证的单调时基。二者各自只比较本地相对时间。

prepare challenge 绑定私有 request digest、owner/fence、runtime 和新鲜计数；addon 保存生成时的本地时间及有限 TTL。commit 过期则拒绝；resume 不刷新它。重新 prepare 是新 transport attempt，是否可做必须由旧请求未开始执行的证据决定，不能暗中延长 opaque 业务。

所有 64 位序号不回绕；临近上限进入 controlled_close，不能归零后让旧 frame 重新有效。Lua 的 64 位字段采用两段 uint32，JSON 中可能超过安全整数的值一律使用字符串。

## 8. 外部发布算法及其真实保证

对每个固定 frame：

1. 获取准确 data-writer lease；验证 OS 身份、唯一 profile、当前 runtime/session/owner/fence、reusePermit、current request。
2. 重新沿完整具名路径定位 manifest/frame/cells；验证 tag、secret、对象结构、keys/array shape、calibration、目标旧值或已知前代值及完整 8 字节页面范围。
3. 落盘准确写入意图：请求和分片 ID、旧/新 stamp、期望字节、目标字段、内容 hash、权限/profile 摘要。Sync 后再次验证 Lua 路径和进程。
4. 发布构建中 stamp；只写 allowlist 内已有 number 的 8 字节 payload。固定批次有硬上限；检查短写、错误和 readback。
5. 填完整 header/body/digests，再写稳定 publicationEnd 与 publicationBegin。任意一个稳定标志写入都不能被当作原子提交事实。
6. 重新定位、校验、读回；保存 outcome。缺 outcome、短写或路径变化记为 partial/unknown，不自动回滚或重放。
7. addon 先读稳定 stamp/header，分帧复制到私有数据，增量计算摘要，最后重读 stamp/header。只有版本一致、长度合法、完整内容 hash 匹配、身份有效才接受。私有副本及其摘要进入下一状态，执行不能再引用公共 inbox。

这是一种**带版本标记和内容校验的快照协议**，不能宣传为已证明的 lock-free seqlock。传统 seqlock 依赖写入/读出原子的前提；这里不能先假定 WPM 对 publish word 提供该保证。

Microsoft 的 [WriteProcessMemory](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-writeprocessmemory) 文档描述可写区检查、字节复制及返回值，没有承诺多次调用事务、Lua GC pin 或对象生命周期同步。[ReadProcessMemory](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-readprocessmemory) 也不是快照屏障。因此 hash 和双读用于拒绝不一致，不建立不会访问失效对象的证明。

数据 frame 的准确重复只返回原 frame receipt；同 ID/sequence 不同 digest 是冲突，冻结本请求并报告。恢复时先观察当前私有接收高水位和宿主 journal；只有能证明尚未发布/已拒绝且未执行的准确阶段才允许继续同一传输。未知 commit 不得通过重写源码、换 request ID 或新 bank 来“继续”。

## 9. 正常业务流程及各层 ACK

~~~mermaid
sequenceDiagram
    participant H as Host Coordinator
    participant I as Inbox
    participant A as Addon private protocol
    participant S as Sendbox
    H->>I: Bind/Resume control + fresh host nonce
    A->>S: Bound(session, owner, fence, nonce echo)
    H->>I: Request manifest + frame 1..N
    A->>S: frame_copied(N) / transfer progress
    A->>A: Verify full SHA, compile private copy
    A->>S: PREPARED(request digest, challenge)
    H->>I: COMMIT exact request + challenge
    A->>A: Recheck private identity / close / actor / deadline
    A->>S: commit accepted
    A->>A: Start probe once
    A->>S: TERMINAL + result manifest/pages
    H->>H: Read all pages, verify, durable persist
    H->>I: RESULT_ACK exact result digest
    A->>S: RELEASED + reusePermit
    H->>H: Persist release; next request now permitted
~~~

| 信号 | 证明什么 | 不能证明什么 |
| --- | --- | --- |
| frame_copied | 某个准确片已进入 addon 私有缓冲并校验 | 完整请求存在、源码已编译、业务已执行/结束 |
| PREPARED | 完整私有请求通过校验并准备执行，附 challenge | 业务已经执行 |
| commit_accepted | 私有执行状态已经接管准确请求 | 函数成功、没有副作用、结果必定可取回 |
| TERMINAL | addon 明确给出 success/failed/确认取消，或明确的执行前拒绝 | 宿主已保存、cleanup 必定完成 |
| RESULT_ACK | 宿主声明并以准确 digest 确认已可靠保存所需结果 | 原业务尚未运行、所有游戏副作用被回滚 |
| RELEASED | 此请求可正常退役，reusePermit 对应准确前代 | 可以使用任意旧地址，或下一请求已获得执行授权 |

编译失败应产生明确 failed/not_started 终态和小型结果，而不是留下不可回收的 prepared。ACK/release 丢失时继续读取原 tombstone 并补齐原确认，不重跑业务。

### 业务结果最小 schema

~~~json
{
  "schema": "lycheedev.duplex.result.v1",
  "runtime": "exact-runtime",
  "session": "exact-session",
  "requestId": "exact-request",
  "requestSeq": "42",
  "requestSHA256": "64-lowercase-hex",
  "outcome": "success | failed | cancelled",
  "executionStarted": true,
  "failureCode": "optional-fixed-code",
  "cancelControlId": "optional-exact-id",
  "effects": "none_started | may_have_occurred",
  "resourcesReleased": true,
  "resultBytes": 1234,
  "resultSHA256": "64-lowercase-hex",
  "pageCount": 1,
  "truncated": false
}
~~~

cancelled 表示 addon 已确认停止后续受管理执行，并完成规定的取消收尾；不表示此前副作用回滚。execution_unknown 属于宿主观察结论，不能由宿主伪造为上述业务终态。缺资源释放证据时即使业务已 failed，也不能给出正常 reusePermit。

## 10. 双方状态机和内容复用

### Addon 私有业务状态

~~~text
EMPTY
  -> RECEIVING
  -> VERIFYING
  -> PREPARED
  -> EXECUTING_SYNC / EXECUTING_ASYNC
  -> TERMINAL
  -> RESULT_RETAINED
  -> RELEASED
  -> EMPTY(next exact generation)
~~~

RECEIVING/VERIFYING/PREPARED 可被取消而不执行业务，产生 cancelled + executionStarted=false。执行状态仅能由实际 Executor 回调、同步返回或已确认取消推进。transport hash 错误可拒绝某帧；一旦宣告 request 终止，必须保留准确拒绝结果供宿主确认。

PREPARED 和 COMMIT 间再次核对：private owner/fence、challenge、request digest、deadline、actor、没有 sticky close、没有已受理 cancel、允许的操作能力。公开 ready 为 true 也不能跳过这一步。

### Host 业务状态

~~~text
INTENT_DURABLE -> TRANSFERRING -> PREPARED_OBSERVED
  -> COMMIT_INTENT_DURABLE -> COMMIT_SENT_OR_UNKNOWN
  -> TERMINAL_OBSERVED -> RESULT_DURABLE -> ACK_SENT
  -> RELEASED_OBSERVED -> RETIRED
~~~

独立分支是 NOT_STARTED_CONFIRMED、CANCEL_PENDING、CANCELLED_CONFIRMED、EXECUTION_UNKNOWN、ABANDONED_UNKNOWN。所有 unknown 保留原 request/attempt/commit 内容和原预算。

结果已可靠保存但 ACK 丢失时，业务报告仍 verified，cleanup=pending；不得把已验证报告降成执行失败。结果页读取失败时保留 terminal manifest 和已验证页，后续只补读。

### 内容复用条件

正常复用要求：同一 runtime/session、上一个 request 的 terminal 已验证、所需原始结果字节已落盘、addon 接受 exact RESULT_ACK、resourcesReleased=true、RELEASED 的 reusePermit 与本地前代日志一致，且没有 close/cancel 控制阻断、没有旧 data writer 句柄仍活动。

租约过期、ready=false、没有响应、CLI 崩溃、结果 digest 不匹配、页面读取失败，都不能替代这些条件。

## 11. 大源码、大结果和有界保留

候选初始上限如下；它们是待性能验收的配置常量，不是当前已验证的吞吐能力。

| 项目 | 候选上限 |
| --- | --- |
| 当前业务 | 1；业务排队数 0 |
| 源码 | 1048576 字节（1 MiB），256×4096 字节逻辑片；控制区/header 另计 |
| 每 lane 控制内容 | 1024 字节以内；独立当前记录 + 固定回执 |
| 结果 | 沿用 512 KiB、深度 32、条目 32768 |
| 结果页 | 32×16 KiB |
| 当前未确认结果 | 1；不能被下一结果覆盖 |
| 已释放业务 tombstone | 最后 1 个准确摘要 + session/request sequence 高水位 |
| 控制历史 | 每 lane 当前/最后终态记录，有固定总条数；旧序号只拒绝，不重执行 |
| host transfer budget | 请求创建前声明并固定的有限预算；1 MiB 的默认值和硬上限待全尺寸性能验收定稿，不沿用未验证的 120 秒吞吐假设；resume 不刷新 |
| 执行业务预算 | 1..120 秒；仍是合作式限制，不是同步抢占 |
| cancel / close | 各自独立有限预算，建议 15/30 秒；不继承已耗尽业务预算 |

结果生成必须改成可分片的有界 serializer/hash 流程；把现有一次性 Encode(512 KiB) 接在轮询 callback 里，不能称为已经做了帧预算。序列化失败返回小型 failed/result_encoding_error 元信息；不能对截断内容声称是原完整结果。确需截断时必须显式 truncated=true 并定义其业务含义，默认建议失败而不假装成功。

sendbox 页在 RESULT_ACK 前保持可读取的不可变版本。宿主按 manifest 中固定 page index、长度、page SHA256 逐页验证，最后验证完整 result SHA256；写临时文件、Sync、原子登记归档和 journal 完成后才 ACK。ACK 必须引用准确 request/result digest，不能是一个无身份的 ack=true。

主机故障后 addon 最多保留一个完整结果。保留超时可以停止轮询、撤销新执行资格、进入 orphaned/quarantined，但不能为了空出队列自动丢结果并开始新业务。内存压力时明确拒绝新工作；只能在已确认 release、runtime 消亡或用户明确处置未知工作的路径上退役。

### 连续命令的内存保留合同

140 条顺序命令必须复用同一份固定 arena，不能创建 140 份邮箱、不能按 request ID 在 Lua 中永久累积源码/结果/执行对象。固定数值 payload 原位覆盖不创建新的 Lua 对象引用；新的源码字符串、哈希工作区、编译 closure、执行环境和结果是当前请求的临时对象，必须有明确释放引用的时点。

| 对象 | 最多强保留 | 正常退役动作 |
| --- | --- | --- |
| 外部可写 arena | 正常 1 份；故障最多 active+retiring 共 2 份 | 正常请求之间原位复用；旧故障代只在全部 writer 停稳后解除根引用 |
| 当前业务源码/片副本/hash 状态/编译 closure 与环境 | 只属于 1 个未退役业务；不建历史列表 | 不再用于校验、准备或执行后解除引用；最迟在执行静止和终态 cleanup 时释放 |
| Timer/event/受管回调/cleanup 闭包 | 单个当前执行的有界资源集合 | 正常或取消收尾时注销/取消并断开 payload 引用；未能清理则 resourcesReleased=false，阻止下一业务 |
| 当前完整结果及分页 | 1 份，仍按 512 KiB 上限；共享内容不无谓复制 | 已验证 RESULT_ACK 后去掉结果对象/页引用，再发布仅含摘要的 RELEASED |
| 私有 operation ledger | 当前请求 1 条 + 最后 released 摘要 1 条 + 单调高水位 | 退役后去掉源码、closure、结果和资源引用；旧请求只按高水位拒绝，不保留所有历史 |
| 控制回执与 sendbox 快照 | 按 lane 固定当前/最后记录 | 新快照替换旧快照，不保留 heartbeat/status 历史链 |

CLI 磁盘保存完整历史、源码和结果；addon 只保存恢复当前请求和拒绝旧消息所需的有界状态。先形成独立、有界的结果/诊断 wire 快照，再断开原始返回树、错误对象/stack、源码、编译 closure、环境和日志闭包的引用；快照不能通过函数/对象引用暗中反向保留整个执行环境。编码失败产生独立的小型失败快照，也清理无用原对象。终态后仍未 ACK 时仅保留完整快照及必要身份元信息，不为了方便 resume 继续强留已经无用的执行对象。故障中的私有执行/结果仍独立保活，不能为压低内存破坏第 14 节的恢复证据。

去重只保留固定摘要与高水位，不创建随命令/session 数量增长的 ever-seen ID 集合、控制历史数组或已关闭 session 映射。对已退役旧序号/旧 session 拒绝，不需要保存它们的所有明细。

去掉强引用不等于内存立刻归零：自然/增量 GC 尚未运行、分配器保留页、Lua 临时拼接都会造成波动；不能每条命令强制 full GC 来掩盖引用泄漏。生产路径按有界工作片段自然运行，离线/研究验收可在测量检查点显式 GC 以区分临时垃圾和长期保留。构建完整源码或结果不能每片反复拼接累计前缀；需要测试 concat/序列化时的临时峰值，不能用结果大小代替峰值内存。

1 MiB 是源码字节数上限，不是执行内存预算。这里约束的是通信与受管执行资源，不是任意探针对游戏全局的所有副作用。探针主动把大表/函数写到全局、注册未受管回调或修改第三方插件状态，可以造成另外的内存增长；CLI/Lua 代码没有因此获得隔离 sandbox。需要把这类增长与 transport 开销分开记录，不能承诺任意 140 条代码都占相同内存或全游戏总内存不会增长。

验收至少比较连续 1、10、140、1000 条后的空闲保留量、自然 GC 周期、检查点 GC 后的可达对象与同基线增量、最大请求期间峰值、最大结果和故障双 arena 峰值。使用相同输入分布/固定业务，检查源码、closure、timer/callback、result page 和 ledger 条数：退役后不能随命令数增长。GC 后保留量应在由 layout/allocator 实测确定的范围内达到平台，而不是要求操作系统进程工作集每次相等。不要把一次总内存读数当作归属证明。

## 12. Cancel、disconnect 和跨 CLI 控制

### 独立控制 lane 解决的两个阻塞

仅有业务 inbox StopWait 会使 cancel 排在被取消的请求后面；仅有一条控制 StopWait 又会使 close 排在无回执的 cancel 后面。因此使用独立 cancel、close、resultAck lane，close 一旦受理即为 sticky，当前 session 不能再接收新业务。

第二个 CLI 不能被第一个 execute 长期持有的 driver lease 挡住：

1. 所有控制意图先进入准确 CON 的独立 durable control journal，由短 control-intent lease 串行落盘。
2. 若 active driver 正常运行，它在每个 I/O/work slice 之间优先读取控制意图，主动停止新 data/commit 发布。
3. 若 active driver 正等待回执，第二 CLI 可取得独立 control-writer lease，只写该 CON 已绑定的控制 allowlist。它不能接管 data lane、改变 owner、重建 arena 或借此恢复业务。
4. data writer 和 control writer 的写地址集合不重叠；它们都验证同一 process/runtime/session。close/control ACK 不能释放外写 backing。
5. 新业务发布和 COMMIT 前检查持久 sticky close/cancel 意图。检查后控制仍可能并发到达，因此最终顺序由 addon 私有状态判定；先开始的执行不能事后伪称未执行。
6. 在准许新 session/业务复用或退役 host claim 前，确认原 data writer 已关闭写句柄并释放其 OS lease。控制 writer 也必须先关闭自己的写句柄。不能只看到 close receipt 就把仍在 WPM 的进程遗留在后面。

控制处理优先级为 close → cancel → resultAck/已知结果收尾 → bind/resume/lease → reload → business frame/prepare/commit。每 tick 有固定控制处理数，维护类重复消息合并为固定最新意图；不允许靠无限 lease 刷新淹没 close/cancel。

### Cancel 的精确定义

| 当时状态 | addon 动作与回执 | 宿主允许的下一步 |
| --- | --- | --- |
| host 还没发布业务 | 本地撤销已知未发送意图；无需 WPM | cancelled/not_started；原 ID 归档 |
| receiving / copying / hashing | 受理 cancel，停止继续收片/计算，保留 inbox backing | 等 cancelled + not_started 终态，保存并确认 |
| prepared、commit 尚未被私有状态接管 | 撤销 challenge，销毁私有可执行 closure，返回 cancelled/not_started | 不再发送 commit；按原 ID 收尾 |
| commit 与 cancel 竞争 | addon 同一私有状态机裁决；先受理 cancel 则禁止执行，先开始则按 running 处理 | 以准确回执为准，不能凭宿主发送顺序下结论 |
| running async | cancel_accepted；撤销后续受管理 Callback、停止已登记 timer/event，执行有界 cleanup | 仅实际停止和 cleanup 结果后给 cancelled；effects 可能已发生 |
| running sync | Lua 没有机会检查 lane 时无法受理 | cancel_requested/pending；等函数返回后裁决，或 unresponsive |
| terminal/result_ready | 返回 cancel_too_late，并关联原终态 | 读取原 success/failed，不能改成 cancelled |
| runtime/process 丢失 | 没有确认取消的回执 | execution_unknown；保留原日志；不能重跑 opaque |

取消接口必须能准确定位 CON 和 request；默认“当前业务”必须在意图落盘时解析成不可变 ID。不要把后续新业务误取消。取消 retry 复用同 control ID/digest，原 cancel budget 不自动更新。

### Disconnect 的精确定义

Disconnect 是“停止接受新业务并关闭当前 session”的控制目标，不是“先等原业务成功并读完结果”的别名。

1. 宿主持久化 close intent 和独立 close budget，立刻禁止自身后续新业务/commit。close 控制单独发布，不等业务 lane。
2. addon 一旦能处理 close，即置私有 closing=true；拒绝后来的新 prepare/commit，按上表停止尚未开始/异步可取消工作。
3. 成功或失败结果已经存在时保留它；取消产生的新结果也保留。控制回执可独立给出 session 已不再接受新执行、执行是否 quiescent、资源是否释放、结果是否保留。
4. 结果正常取回走原 manifest/pages/ACK。即使结果正文暂时读不到，也能读取独立 close receipt；不能将“下载结果失败”变成无限阻塞关闭请求。
5. 仅有 closed_to_new_work 不足以销毁 arena 或确认取消。若业务仍在运行，返回 closing/running；若已静止但结果未知，返回 closed/quarantined + execution_unknown，businessReady=false。
6. 具备准确 addon unbound/执行静止证据且所有宿主 writer 已结束时，可以退役当前宿主 claim。未取回的结果仍是恢复义务；是否可开始新 session 由 addon 私有 quarantine 决定，不能通过删除 claim 绕过。
7. close budget 到期返回原 CON、closeControlId、remaining=0、当前 blocker、可读状态和下一恢复条件。不续业务预算、不重新执行旧代码。

正常断开可自动完成。无法确认结果的强约束意味着不能承诺“断开后无条件立刻开始下一项”。这应是诚实的显式处置流程，而不是永久缺失取消/退役入口。

### Lease 失效与孤儿 owner

private lease 只管接受未来的 prepare/commit，不是已提交业务的回滚机制。心跳超时后 addon 可以撤销未来执行资格、进入 closing/orphaned，并保留当前 request、result、arena 和高水位；不能把 timeout 解释为旧 host 死亡或业务已取消。

同 owner 恢复必须带准确 session、原 request/digest、journal 对应的高水位及新的 fresh nonce；fence 只能按明确 handoff 推进。新的 fence 不允许重执行已经 accepted 的 opaque 请求。不同 owner 不自动接管活跃/未知 session。

## 13. Status、resume、abandon 和 activation：清理不能再被预算挡住

建议新代 CLI 明确区分：

| 意图 | 允许动作 | 绝不隐含的动作 |
| --- | --- | --- |
| status | 只读宿主 journal、准确 runtime/sendbox、控制和清理状态 | 新 claim、bind、续预算、发消息、修复 |
| resume | 同 CON/请求/控制 ID 继续观察、补齐已授权且有明确前置证据的阶段 | 重发未知 opaque、换角色/进程、新 request 覆盖、自动续期 |
| cancel | 写准确取消意图，必要时发独立控制消息，等待有限确认 | 把 host context cancellation 当作游戏取消 |
| disconnect | 独立 close intent/budget，禁止新业务，控制收尾 | 等原业务成功才允许发 close |
| abandon | 用户明确接受未知后，保全证据并按准确对象退出宿主恢复责任 | 假造 cancelled/result/release、释放仍在运行的对象、自动开始替代业务 |

**业务预算校验之前，先识别 terminal/closing/activation-only/host-only retirement。** 退役是独立控制目标，有自己的持久预算。预算已耗尽也必须允许只读 status、精确 evidence 归档、已经确认的幂等退役；不能因业务 budget=0 连关闭入口都进不去。

### 无 Driver 的 activation

activation 是明确的一等持久对象，至少包括 prepared、input_not_sent、input_submitted_or_unknown、runtime_observed、bound、abandoned_unknown、closed_not_started。新代 Status/Cancel/Disconnect/Abandon 都识别它，不要求普通 Driver journal 必须存在。

纯等待用户进入角色且从未发输入/写入时，cancel/disconnect 可以完成 host-only cancelled/not_started。已经提交或输入结果未知时，准确退出是 abandoned_unknown：保留原输入事实、预算、目标、原始证据，不再发旧键，不声称已取消 reload。

宿主退役步骤：

1. 精确校验 project/CON/activation/target/owner 和原操作摘要。
2. 获取准确执行 lease，确认无正在进行的 writer/input，并在 lease 内重新读取。
3. 用文件存在性区分“普通 journal 确实不存在”和“存在但为空/损坏”；后者不能走 activation-only 简化路径。
4. 持久化不可逆的终态/明确 abandon 决定；保存原记录及输入未知。
5. 先释放执行 lease，再调用自身会加锁的 claim 退役函数。
6. 只退役完全相等的 own claim；被其他 owner 替代时确认自己的 claim 已不在，绝不删除替代 claim。
7. terminal resume 永远不进入 budget Observe、Native 创建或输入发送，重复调用不改写终态。

### Unknown 的退出条件

| 未知场景 | 保留什么 | 能继续的条件 |
| --- | --- | --- |
| commit 可能送达，结果丢失，runtime 仍存在 | 原 ID、digest、commit intent/outcome、owner/fence | 同 runtime 精确回执/结果恢复；或用户明确 abandon，但不能自动新业务复用 |
| 业务已经开始，Lua 无响应 | 控制意图、旧 writer 句柄关闭证据、最后 heartbeat | Lua 恢复并给准确结果/停止确认；或用户决定真正终止/重启进程，再保存 OS/runtime 终止证据 |
| addon 控制确认 quiescent/unbound，但业务结果未取回 | 所有已读结果和 terminal/control 元数据 | 补读并 ACK；或用户明确放弃结果，准确归档为 unknown/abandoned，按新协议定义的 quarantine 处置 |
| process 退出/PID 已重用 | 原身份、未决事务、最后输入/写入事实 | OS 证明准确旧实例消亡后退役宿主资源；未决业务结果仍 unknown |
| reload 已证明新 runtime | 原 request 与新旧 runtime 证据 | 只做旧对象退役；是否发新的业务是新的明确意图，不自动重跑旧 opaque |

如果 addon 仍持有旧 owner 或处于 quarantine，host claim 退役后新 connect 仍可能被拒绝。CLI 应返回准确理由和所需条件，不把“文件锁已清”包装成系统已经可重新执行。

不设计隐藏的 force-clear、改 budget 文件、换 request 绕过、删除 claim 或重写旧 pending 内容。显式 abandon 是退出恢复责任的决定，不是把 unknown 改成业务终态的工具。

## 14. GC、强引用、失效地址及重建

### A. 防止仍需要的对象被 GC 当作不可达

必须给出可以检查的强引用图，而不是“这次全局 GC 后值还在”：

~~~text
当前 VM 的真实强根
  -> 持续存活的私有 RuntimeOwner
      -> TransportEngine closure / state
          -> lifetime arena
              -> fixed manifest / frame / control tables
      -> Lifecycle Supervisor / 独立 operation ledger
      -> 私有当前请求、挑战、结果
  -> 公开 Mailbox publication
      -> 同一 lifetime arena 的只写数值区域
~~~

RuntimeOwner 必须由 addon 生命周期的稳定强根持有，不能只由“当前 timer callback”“当前活动请求 closure”或可被替换的公开 Mailbox 持有。实现前要证明实际 root 链：正常取消 timer、解绑 UI、结束探针、禁用 bridge 都不会断掉它。若使用独立内部命名空间作为根，它必须和公开 publication 分离；同 Lua 环境其他插件仍能破坏它，这不是安全 sandbox。

arena 和各子表是普通强表，无 weak metatable；不提供业务代码直接修改其表结构的公开引用。创建时一次性分配全部 keys/cells，开始接收后不插入、删除、nil 化、扩容、换 backing、修改 metatable 或重排。

disconnect、lease 到期、cancel、ACK、轮询停止都**不释放、不替换外部可能写入的 backing**。同 runtime 正常多轮只复用一套 arena，改数值及消息 generation，不不断分配旧 bank 造成泄漏；arenaGeneration 只在第 D 节重新分配 backing 时更换，故障保留另有硬上限。初始禁用不分配 arena；启用后再禁用可以保留既有 arena，但取消所有持续工作。这是“禁用零持续开销”，不是声称禁用后历史分配的内存立刻为零。

number payload 不新增 GC 对象引用，且外部不修改 tag/pointer；因此设计不需要用外部写入制造新的 Lua 引用关系。其具体 TValue/collector 语义仍必须由目标 WoW build 的证据确认，不能直接搬标准 Lua 的实现当作事实。

### B. 发现地址、布局和 publication 已变化

每次写入前、意图 Sync 后及写后重新检查 process identity、runtime/private generation、root 路径、arena/table identity、固定 shape、tag/secret 和值约束。持久地址缓存只能是有界观察的优化提示，不能成为写权限。

公开 namespace 被替换、keys 被删/重排、数组布局变化、secret/type 变化、路径不一致时立即停止。保存最后意图、确切 WPM 字节数、失败阶段和所有已验证结果，标记 partial/unknown；不对旧地址尝试“恢复原值”，不扫描 heap 猜另一个可写对象。

如果 collector 是移动式的，强引用也可能只保证对象存活而不保证地址不变。**本设计不预设 WoW collector 在所有 build 都不移动。** 写 profile 必须包括有依据的 collector/对象布局能力结论；未知时只读/拒绝写入。压力测试是必要证据，但不是任意时序的形式证明。

### C. 最后一次 guard 与 WPM 之间的生命周期竞态

即使 A、B 全部实现，reload、VM 销毁或失去全部强根仍可能发生在最后一次核验之后、内核真正复制字节之前。页面仍是 RW/MEM_PRIVATE 也可能已被分配给另一对象。随后再次核验只能发现风险发生，不能撤销已经写到错误对象的字节。

因此：

- ready、challenge、seqlock stamp、SHA256、双 bank、强引用都不能单独提供跨进程对象 lifetime pin；
- 自定义 addon 的 Reload 通路可以协商停止 writers，但不能拦截并证明所有用户 reload、logout、崩溃、客户端动作和 VM 终止时序；
- 不以暂停进程、注入 native 代码、改页保护或劫持游戏函数作为本方案默认补丁；
- 若严格零失效地址写入是硬门槛，当前外写 Lua heap 的技术路径没有完成证明，不能投产；本分支不以备用 LoD 绕过门槛；
- 若将其限制为显式 opt-in 实验性能力，必须把残余风险作为产品边界，并由后续实施决策明确接受，不能在本文中用“测试通过”偷换成生产零崩溃保证。

### GC 验收不能只做一次 full collect

需要自然 GC、可验证的增量 step、full collect、分配压力；在 header 构建、每个 WPM 前后、每个 frame 复制/哈希片段、prepare、commit、结果保留、取消和断开交错。

负例包括弱表、丢失独立私有 root、公开 namespace 替换、table rehash/array shape 改变、禁用再启用、runtime 替换及进程结束。离线 fixture 用确定性调度覆盖每个切点；真实客户端只在批准的研究范围内做压力验证，并记录未观察到的风险，不把故意旧地址 WPM 当作常规验收。

还需长期多轮运行，检查 arena 数量固定、内存峰值、timer/event/callback 引用清理和 disabled 后无持续工作。游戏主线程停在同步 GC 或执行中时，host 只能得到 stale/unknown，不能把停顿当作取消回执。

### D. Arena 确实已丢失或被回收：重建后复写

强引用是正常路径的预防措施，不能代替对象实际丢失后的恢复协议。必须将 runtimeId、arenaGeneration、session/owner/fence、operation ledger 分开。arenaGeneration 是独立的 128 位代号；所有 frame/control/writePermit 都绑定它，不能只绑定 runtime。

私有 RuntimeOwner 持有独立的 Lifecycle Supervisor 和有界 operation ledger。ledger 保存 request ID/digest、是否接受 commit、是否真正开始、执行终态/结果引用及最后 ACK/release；它不挂在公开 arena 上，也不依赖当前 poll closure 唯一保活。Supervisor 在启用期间属于已声明的有界 scheduler 工作，检测 public arena、control area 和发布路径完整性；禁用后不为了自修复继续轮询。

恢复只能由 addon 自己创建新的 Lua 对象。宿主不能按旧地址复写、扫描 heap 猜对象、补造 table pointer 或恢复已释放页：

1. Supervisor 发现 arena 丢失/结构被破坏，先在私有状态中撤销旧 arenaGeneration 的接收与 commit 资格，发布 repair_pending 和 businessReady=false；control area 丢失时由独立 sendbox 发布 controlUnavailable/repair 状态，不依赖已丢失的 cancel lane 请求修复。
2. 私有协议停止将旧 frame/control 转成新动作；迟到的旧 commit 必须先按已撤销 generation 拒绝，再处理其他字段。旧异步 callback 仍只属于其原 operation，绝不能完成一个新 request。
3. addon 检查独立 ledger，确定下表中的恢复类别。声明 not_started 必须在旧 generation 撤权之后生成；不能先宣告没执行，再接受迟到旧 commit。
4. 新分配完整 arena，初始化普通强表、数字 cells、calibration 和新 generation；生成新的 repair challenge。先完成结构，再发布同 runtime 下的新 descriptor。repair notice 不是业务 writePermit；descriptor 可只授予准确 bindResume lane 的一次 repair-only 控制许可，该许可不授权源码、commit 或其他操作。
5. 宿主以原 CON/request 关闭所有旧 data/control 写句柄并证明其 writer leases 已结束；保存旧代 partial/unknown 事实。重新从 root 具名定位新 descriptor，核对准确 generation、profile、数字校准、session、ledger 和新 challenge。完成所有写前保护和旧 writer drain 后，宿主用 repair-only 许可在新 bindResume lane 写修复握手；addon 校验后才发业务/常规控制 writePermit。若不能证明旧 writers 停止，只能只读观察，不能先写新区域再补 drain。
6. 原 request 内容只在准确 not_started 证明下重新传输，request ID/digest 不变，transportAttempt 和 arenaGeneration 更新。若已有私有执行或结果，只恢复控制/读取，不能重传代码并再次 commit。下一业务仍遵守终态、落盘和 release 规则。

| Arena 丢失时的准确私有阶段 | 可恢复动作 | 禁止动作 |
| --- | --- | --- |
| header/body 正在接收或 copying，尚未接受 commit | fence 旧代后返回 not_started；丢弃不完整私有片；新 arena 按原 ID/digest 重传 | 将分片接收误当已执行，或放任旧 commit 后到 |
| validating/hash/compile 中 | 保留完整已验证私有源码，或在 not_started 证明后重传；新 prepare challenge | 沿用旧 prepared challenge |
| prepared，commit 尚未接受 | 撤销旧 challenge；新代 reprepare 同内容，原业务预算不刷新 | 仅因新 arena 出现就自动开始执行 |
| commit accepted / running sync / running async | ledger 保持同一 operation；等待其原回调或终态；新区仅承载控制与观察 | 重跑 opaque、把旧 callback 关联到新请求 |
| terminal/result_ready | 从独立保留的原结果重新发布 manifest/pages；结果 digest 不变 | 通过再执行补回结果 |
| host 已持久化，RESULT_ACK 可能送达 | 用 host durable result + ledger 的 ack/released 高水位恢复 | 丢失 ACK 就重新执行业务 |
| released | 恢复 tombstone/reusePermit，核对 host 前代日志后允许下一业务 | 仅看到空 arena 就认为没有历史请求 |
| ledger、controller 或整个 runtime 也丢失 | 无法证明的旧操作 execution_unknown；已验证持久结果不降级；停止写入，等待新的已验证 runtime/用户处置 | 新 runtime 自称能够证明旧业务未执行 |

如果仅公开 link 被替换，私有 arena 仍完整可达，也按完整性故障处理；只有验证一致才可重发布，不能让一条未经核验的缓存地址继续写。若确实失去全部 arena 强引用并已 GC，只有重建新对象这一条复写路径。

Supervisor 自身也丢失时没有可靠自修复执行者。host 只能观察 unavailable，按已授权的初始化/恢复路径等待新运行时；不能把“设计了 supervisor”当成它永远存活的证明。

有能力仍持有旧 arena 时，可在 retirement quarantine 保留它直到准确 writer drain，降低迟到写风险；固定最多一个 active 和一个 retiring arena，第三次故障不继续分配，进入 repair_blocked。旧 arena 已被回收时无法事后保活；固定等待若干毫秒也不构成 lifetime pin。正常无故障多轮只复用一套 arena。

该协议解决“已经丢失后如何安全重新定位和重新传输”，**仍无法撤销已经进入内核的旧 WPM，也不消除最后 guard 后对象被释放的竞态**。重建许可必须与这一物理限制分别陈述。

新增验收：在已关闭 writer 的受控故障注入中断开 arena 的全部强引用、执行可验证 GC，保留 controller/ledger；证明新 generation、新分配和重新校准、旧 writer 零写入拒绝、同 request 至多执行一次。再覆盖 ledger 丢失、control area 丢失、每个上表阶段、迟到旧 commit/回调及 repair 上限。并发 WPM/释放切点优先用离线确定性模型，实机风险注入须另行明确研究范围，不能为了证明恢复故意向旧地址写。

## 15. 轮询、ready 和活性证明

用户明确允许启用轮询，覆盖此前本路径的纯事件驱动限制；它不授权禁用状态持续工作。

推荐先复用已有能力证据中的 C_Timer.NewTimer，单个自重排 scheduler；不假定尚未验证的 NewTicker。在 callback 内总是先处理固定数量控制，再做最多一个业务工作片段，最后根据状态排下一次；不并排创建无界 timer。

研究起点节奏：启用未绑定 500 ms；绑定空闲 200 ms；有传输/prepare 工作 50 ms；heartbeat 最多每秒一个完整记录，状态变化可立即发布。暂停/低帧率不能补跑累计 N 次 callback。研究起点工作片段上限为 512 个 uint32 复制或 4 个 SHA block，并另设 0.5 ms 软 CPU 目标；具体阈值以 real-client 测量收敛，软目标不是任何硬件上的硬实时保证。

这些起点值不是 1 MiB 正式吞吐合同：每 50 ms 只计算 4×64 字节 SHA，理论处理速率仅 5 KiB/s，单遍 1 MiB 就约 204.8 秒，尚未计双层 hash、复制、WPM 和 journal，不能与 120 秒总传输预算同时承诺。全尺寸验收必须共同定稿有硬上限的每 callback 批次数/调度间隔和有限传输预算，记录控制延迟与 CPU 成本；不可只扩大总超时掩盖不可接受延迟，也不能提高批次却取消 CPU/控制预算。执行预算独立保持 1..120 秒，传输扩容不延长执行预算。

只查少量 publish stamps 时不哈希整块 arena；只在新准确 generation 出现后处理对应固定片。源码/结果 hash 和序列化均可分段；compile 和用户同步代码无法靠这个 scheduler 分段，单独列为阻塞风险。

宿主性能必须单独设计和测量。不能为 262144 个数值每次遍历整个大表并完整检查全部 cells，那会产生平方级工作。采用已经验证的固定数组或分块数组 profile，按 frame 解析有界路径、每次具体 WPM 核对目标 tag/secret/页面和当前 generation；frame 前后做完整身份/内容验证。数组 offset/stride 本轮没有实机证据，不能猜测后投入使用。

可用“一份完整 frame 写入意图先 Sync，描述所有允许字段/值/顺序”的方式替代每 word 一次意图 Sync；每个实际写入仍记录准确 outcome，frame 完成后 Sync。进程崩溃导致尚未持久化的个别 outcome 丢失时，整个未确认 frame 保守归为 partial/unknown；不借批量日志减少不确定性记录。不得把连续 TValue 连同 tag/header 一起 WPM 以节省系统调用。这个优化必须通过 crash 切点测试，尚未实现。

1048576B（1 MiB）是用户更新后的目标源码合同，取代本方案先前的 256 KiB 上限，不是未经测量就承诺现有写预算能完成的默认值。先在 16/64/256 KiB 等明确实验能力上测量 guard calls、WPM 次数、journal Sync、CPU、内存和完整 wall time，再验证 1 MiB 全尺寸。研究阶段更低能力必须 capability 明示并对超限返回错误；不能无声截断。未满足正式源码合同则阻止发布，不以保留或回退 LoD 补齐能力。

### 明确 ready 的定义

sendbox.status 的一个完整不可变状态记录包含：

| 字段 | 含义 |
| --- | --- |
| ready | 精确定义为 businessReady 的便利镜像；同一记录内必须相等 |
| transportReady | 当前 runtime 的协议端点/布局已初始化；不是“下一纳秒一定可以 WPM” |
| businessReady | 当前可以接受新业务：已绑定、actor 可用、无 pending/result/quarantine/closing、private 能力门禁通过 |
| controlReady | 在该次 addon callback 时，能处理当前 session 的安全控制消息；业务 busy 不把它清零 |
| reasonCodes | 固定原因集合，分别说明业务/控制不可用 |
| statusSeq、heartbeatSeq | runtime 内单调序号 |
| addonMonotonicMillis | addon 自己的采样时刻；不是 UTC |
| runtime/session/owner/fence、active request、phase | 准确身份和内部状态 |
| bridgeEnabled、actorReady、capabilities | 声明与私有状态对应的能力 |

这是逻辑 schema；物理发布采用一份不可变 status wire，避免多个独立 ready/owner/heartbeat 格子被读成拼接状态。若为调试增加裸 ready 镜像，CLI 也必须同时验证完整 status wire，不能仅凭镜像 bool 输入或判定健康。

新鲜度由宿主在本地有限观察窗口读取同一身份的至少两个严格递增 heartbeat/status，记录本地观测间隔；有已绑定控制权限时可额外用 fresh nonce echo。doctor 只读，不为了取 nonce echo 偷发控制。不能直接比较 host UTC 与 Lua GetTime，也不能把静止的 ready=true 当作 fresh。

游戏随后仍可马上挂起；新鲜观察不是未来调度保证。因此 ready 是可用性证据，**不是执行授权或 WPM lifetime 保证**。

### loading、combat、focus、后台

- loading/leaving/actor unavailable：停止新业务接收与 commit；安全控制在 Lua 可调度且身份仍有效时继续，否则显示 control freshness unknown。停在加载画面可能根本无 callback。
- combat：内存接收本身不是受保护 UI 操作。v1 可保守暂停新 probe 执行；cancel/close/结果读取不能被 combat 通用门禁堵死。具体 probe 仍逐操作做 protected/secret 检查。
- keyboard focus：不再是内存通信门禁，不把聊天焦点为 true 解释为 transport blocked。
- background：不自动抢前台。实际低帧率会增加延迟；若需要暂停新 probe，由显式策略和已验证能力决定。control 保持 best effort，deadline 不因后台自动延长。
- 同步 Lua 卡住：heartbeat 不再前进；doctor 显示 stale/unresponsive/unknown。旧 controlReady=true 不能证明取消命令现在可被处理。

## 16. Doctor 作为只读分层诊断

未来定向 doctor 的观察对象是准确安装/进程/runtime，输出证据、观测时间、capabilities、blockers 与原 continuation；它不 bind、claim、续租、发键、写 inbox、删除文件或替用户修复。当前 3.1.1 doctor 尚无这些定向字段/参数，Skill 不能提前宣称已实现。

| 层 | 状态例子 | 不能混淆 |
| --- | --- | --- |
| installed | clean / modified / missing / unknown | 磁盘正确 ≠ runtime 已加载 |
| processIdentity | verified / exited / reused / inaccessible | 窗口没了 ≠ 进程退出 |
| rootRecipe | verified / unsupported / ambiguous / unknown | 能定位 root ≠ number layout 可写 |
| numericLayout | calibrated + approved_profile / unsupported / not_run | 一次校准 ≠ 任意 build 可写 |
| runtimePublished | yes / unavailable / malformed | unavailable ≠ 一定未登录 |
| runtimeFresh | advancing / stale / unobservable | ready=true ≠ fresh |
| actorReady | verified / unavailable / changed | 目录名字 ≠ 当前角色 |
| bridgeEnabled | observed_true / observed_false / unknown | 缺发布 ≠ observed_false |
| protocolCompatible | exact_match / mismatch / unknown | 安装版本 ≠ 协议匹配 |
| ownerAvailable | free / own / foreign / orphaned / unknown | own busy ≠ 可开始第二业务 |
| businessReady / controlReady | 准确值 + reason + freshness | business busy 不阻 close/cancel |

聚合状态建议：

- healthy：请求所需能力和新鲜身份均满足；可以同时说明 business busy、control usable，不能一刀切视为坏。
- degraded：仍有安全控制/只读能力，但业务暂停、结果待取或输入能力不完整。
- action_required：已知安装/协议不匹配、明确 foreign owner、需用户进入角色或决定 unknown 处置。
- unknown：缺证据、读取不一致、heartbeat stale、无受支持 login reader。unknown 不是 false。

普通业务 preflight 不通过可阻止新业务；诊断 warning 或泛化 health error 不能成为已授权 cancel/disconnect/host-only retirement 的通用阻断。相关动作仍必须执行各自准确身份/owner/日志门禁。

sendbox ready 完整切换后移除原普通通信的色块、屏幕采样与按键焦点门禁。若保留首装物理激活或其他物理操作，它们是独立能力和独立 journal 路径；不能拿 sendbox.ready=true 给盲发 /reload 授权。

## 17. UI：只有“Agent执行中”

跳动荔枝只表达一件事：**addon 已真正开始执行当前 probe，且该执行生命周期还没有完成。** 唯一显示文本精确为“Agent执行中”。

显示触发点是 Executor 私有 Start 已接管执行之后，不是 connect、写入、hash、prepare、commit 正在等待或公开状态被改成 running。同步调用同帧开始/结束时可能没有可见中间帧，不为了动画先帧延迟或伪造显示成功。

异步 probe 从真正进入 Run 到 Finish/Fail，或确认取消且 cleanup 完成，视为同一 active probe 生命周期；等待它自己的异步回调期间仍显示。cancel_requested/accepted 本身不隐藏；明确 terminal 但资源清理仍未完成时保持 active/cleanup 事实，不能假称已停止。

probe 已结束且规定清理完成后立即隐藏；仅等待结果下载、宿主 ACK、release、disconnect 握手不显示。connect、轮询、ready 探测、收消息、摘要校验和编译准备也不显示。

内部保留完整状态机用于日志/doctor；UI 不增加“传输中”“等待确认”“控制中”等第二种荔枝状态。无 active probe 时不播放动画、不保留动画 OnUpdate；第一次从未启用时不创建其 frame。指示器永远不是 readiness、执行回执或 GC root 的替代物。

## 18. 故障矩阵：下一步必须有具体前置条件

| 事件 | 准确结论 | 有限下一步 | 不允许做什么 |
| --- | --- | --- | --- |
| frame 构建中 host 崩溃 | 此 frame 未获得接收证明；业务可能未完整 prepare | 原 ID 观察私有 transfer 状态，关闭旧 writer；准确取消或按证据继续未发布部分 | 新 request 覆盖旧 arena |
| frame receipt 丢失 | frame 接收未知 | 只读同 request 的 bitmap/digest；准确重复查询 | 当成业务失败或开始第二业务 |
| PREPARED 后 commit 未写 | 可由 journal + addon state 证明 not_started | 取消 prepared；challenge 到期返回准确拒绝 | 静默续 TTL 后执行 |
| commit WPM/回执中断 | execution_unknown，直到私有回执澄清 | 同 request 查询 accepted/terminal；cancel/close 用独立 lane | 新 commit nonce 或 opaque 自动重跑 |
| 结果页坏/暂不可读 | 终态可已知，报告未完整验证 | 原 manifest 补读，控制 close 不依赖正文 | ACK 未落盘结果、丢页腾空间 |
| RESULT_ACK 后 host 崩溃 | 可能已释放 | 从持久 result 与 RELEASED tombstone 收尾 | 再执行业务取结果 |
| close 已发布、cancel 没回执 | close 独立进行，cancel 仍原状态 | 读取 close lane；addon 在实际调度时按优先级处理 | 覆写 unknown cancel |
| 数据 writer 持 lease 阻塞 | control 仍可发；backing 不能退役/重建 | bounded drain，证明句柄结束后再 retire/reuse | 强制删 lease 文件、误判 writer 死亡 |
| lease 过期 | 禁止后续新执行；原业务结果未自动改变 | same owner resume/handoff 或明确 close/abandon | 以 lease 超时证明进程已退出 |
| bridge off | 停止新业务/持续工作；既有 arena 强引用保留 | 关闭 host 句柄、只读静态终态/日志；明确再次 opt-in | 因 off 释放仍可能被写 backing |
| reload / logout / actor 改变 | 原 capability 失效；既有业务可能 unknown | 证明新 runtime/旧进程结束，准确退役旧资源 | 对缓存地址回滚或继续写 |
| Lua 同步无限循环 | 没有轮询、控制、timer 或正常显示调度 | unresponsive；有限等待后请用户决定进程操作 | 声称 cancel 保证抢占/预算硬停止 |
| 未进入角色 | 无可执行 endpoint，具体原因可能 unknown | 只读等待或 host-only cancel/activation abandon | 持续盲发激活键、占用全部实例 |

## 19. 是否还需要 200 个 LoD

**本分支新版本必须删除 200 个 LoD，不保留 LoD 方案。** 它们目前承担 CLI 输入字节、唤醒和协议交换；这些职责在新版本全部由 inbox、启用后的有界轮询、sendbox 与独立控制接替。下列条件是新版本的实现/发布验收要求，不是保留 LoD 的理由。

移除 LoD 后，完整新版本不能只凭“128 字节写成功”通过验收，必须满足：

1. 目标 build 的写 layout/GC/lifetime 风险已达到明确决定的产品门槛；
2. bind、源码最大边界、prepare/commit、结果分页、ACK/release、cancel/close、reload、disabled/on、未登录和多实例均有端到端证据；
3. 首装/升级有独立激活方案：已运行新协议可以用 inbox.reload 协商；根本没运行 addon 时只能报告待激活或使用独立受控物理路径，不能假装 inbox 自己能启动不存在的接收器；
4. 安装、修复、校验、卸载、归档、doctor、skill、命令目录和日志恢复已经不再依赖槽池；
5. 旧未决 claim 有明确处置和证据，不能为安装新协议自动删除它们。

### 切换范围

不能只替换 Native.Publish。当前槽位概念贯穿 bridge/slot.go、channel 的 Identity/Receipt/Transaction 校验、Driver begin/Run、wait 容量 reload、delivery/slots.go、SlotProtocol/SlotRuntime、安装生成器、200 个 TOC/loader、skills 与回归矩阵。新代应删除 index/nextSlot/consumed/slot capacity 驱动的调度和容量 reload，改为 request generation、reusePermit 和有界 transport budgets。

无需为了每 200 次消息自动 reload；序号接近上界、真正版本迁移、用户明确请求或已知运行时故障才进入自己的 reload 控制流程。

### 无旧协议兼容的安全迁移

在新版本发行前，旧 owner/claim 使用其原版本完成或明确放弃；过程未完成时安装拒绝写入，并提供具体原 ID。已有 host-only activation 特殊记录保留原工具/证据，不能拿新协议解释旧字段。

安装器在独立安装维护锁内验证准确旧受管文件、无活跃 writer/未决预约、所有需要保全的记录；记录迁移意图，部署一份新代 addon/CLI，并只删除确认为本产品且符合 receipt 的旧槽文件。未知目录、用户修改或未决 claim 一律保留并明确阻塞。

新旧运行协议不在同一进程内猜测兼容，不对 unknown hash/old journal 自动降级，不把旧 mailbox 表当作可写新 inbox。旧证据可以归档和只读显示为历史，不能成为新执行权限。迁移中断恢复依据迁移日志和具体文件 digest，不收养未知内容。

原有色块可以随普通通信一起移除；若保留独立物理首装工具，它的目标与验证合同单独存在，不再作为每次业务执行的视觉门禁。

## 20. 方案比较及不能承诺的地方

| 方案 | 优点 | 代价/边界 | 推荐 |
| --- | --- | --- | --- |
| 完整固定请求 arena + 独立控制 lanes | 最直接满足 pending 不覆盖；易解释最大源码和恢复；只一个请求 | 若 TValue 为 24B，固定 payload 6 MiB 起，故障双份 12 MiB；仍需大量 8 字节写入与新数组布局验证 | v1 推荐研究方向 |
| 两个小 bank 流式传输 | 常驻内存较小，可传大对象 | frame ACK 后覆盖的额外语义、重组/重传/ABA复杂，不能叫业务ACK | 性能证据需要时再评估 |
| 一个无限长字符串，由宿主改 pointer/length | 看似少量写入 | 改 GC 引用、布局、barrier 和分配，越出已有验证 | 拒绝 |
| 只改 commit 标志、假定 8B 原子 | 实现少 | 未证明 WPM/观察原子与对象 lifetime；无法抵抗 reload | 拒绝 |
| 具备受支持 lifetime pin 的 native/共享内存适配 | 原理上可提供更强对象生存期与原子原语 | 当前 addon 能力没有提供；引入新原生执行/集成范围，不能自行假定可用 | 另立研究，不能偷偷注入实现 |

不能承诺：所有 GC/reload 都安全、任何同步探针都能即时取消、heartbeat 新鲜就未来可调度、SHA256 认证本机攻击者、业务 exact-once 跨 runtime 崩溃、超时后一定可无损继续。LoD 删除是本分支的明确实施要求，但本文不声称代码删除已完成。

## 21. 分阶段实施及验收门槛

| 阶段 | 可交付成果 | 进入下一阶段条件 |
| --- | --- | --- |
| P0 协议定稿 | canonical wire、身份/状态机、控制优先级、结果与未知语义、风险选择 | 用户认可单请求和不能消除的 lifetime/同步取消边界；不写游戏 |
| P1 纯内存 fixture + Lua 模型 | Go/Lua codec/SHA、所有状态转换、journal crash、双 CLI 竞争模型 | 每个 commit/cancel/close/ACK 切点可确定性验证，无伪成功 |
| P2 单 build 受控研究 | 完整 arena、数组校准、强 root、poll budget、GC 压力、最大请求/结果 | 地址变化拒绝、内存有界、disabled 零持续工作、真实性能证据 |
| P3 生命周期与多实例研究 | 同 build 多实例、其他 build profile、登录/角色/加载/后台/异常取消 | 明确每个 build 的受支持能力；没有跨实例写入；控制不被业务等待阻塞 |
| P4 产品可行性裁决 | 说明剩余 WPM lifetime 风险与受支持能力，必要时继续 mailbox 技术研究 | 不能以“暂时没崩溃”替代严格保证；硬门槛不满足则停止投产，不回退 LoD |
| P5 新代集成 | Coordinator/Transport/Executor/doctor/UI/安装替换，完整命令合同，删除全部 LoD 槽依赖 | 新链路端到端成立；载荷和运行路径均无 LoD；无旧协议兼容 |
| P6 发布与迁移 | 精确版本包、受管迁移、归档与操作手册 | 项目全部必要检查和真实客户端验收；发布需要另行授权，本轮不发布 |

P2/P3 不把研究 profile 自动加入发行 allowlist。完整接受一个新 build 必须同时验证 root recipe、Lua/TValue/string/table/array layout、number calibration、collector/强 root 约束和实际场景；仅 root pattern 唯一匹配不够。

## 22. 验收矩阵

下表全部是新方案待执行项，不能由旧 128B 原型直接标记通过。

| 类别 | 必测场景 | 通过标准 |
| --- | --- | --- |
| 数值 codec | uint32 0/max、64位 hi/lo、尾 word、错误 tag/secret/NaN | 精确字节一致；任何非预期类型在写前拒绝 |
| SHA256 | golden vectors、完整 header+source/result、错误字段/长度 | Go/Lua 相同；影响业务的 header 变化都被发现 |
| 分片 | 0/1B、4095/4096/4097B、1048575/1048576/1048577B、重复/缺失/乱序/冲突 | 0B 有明确准入语义、超上限拒绝；合法完整私有请求才 prepared；分片ACK不准许新业务覆盖 |
| 大结果 | 0/1/16KiB边界/512KiB、页丢失/乱序/坏hash/编码失败 | 只有完整验证落盘才ACK；失败结果保持诚实 |
| StopWait | 当前 receiving/prepared/running/result_pending 时提交新请求 | 全部拒绝，不改旧内容，不生成隐式队列 |
| Commit | prepared challenge错误/过期、duplicate commit、nonce/digest变更 | 准确执行一次或拒绝；unknown不自动换attempt |
| Cancel | 每个状态、commit竞争、同步/异步、cleanup失败 | requested/accepted/cancelled/too_late/unknown区分；副作用不伪回滚 |
| Disconnect | cancel pending、结果读取失败、业务超时、活跃writer | 独立close可推进；backing不释放；claim仅按证据/明确决定退役 |
| 双CLI | execute持driver lease；另一CLI cancel/close；同时cancel和close | 控制不排在业务后；lanes不互盖；无两个data writers |
| Journal | intent前后崩溃、Sync失败、WPM短写、outcome丢失、ACK后崩溃 | 准确未知；不重放；已保存结果继续可用 |
| GC正常 | 多轮自然/增量/full GC、每个读写/复制/hash切点 | 强root可说明；不因常规GC丢对象；任何变化fail-closed |
| GC负例 | weak表、断private root、public替换、rehash、shape变化 | 写前拒绝或诚实unknown；不对旧地址回滚/猜测修复 |
| GC重建 | writer停稳后实际丢arena、控制区丢失、各业务阶段、ledger丢失 | 新代重定位；仅not_started可重传；已执行不重跑；旧回调不完成新请求 |
| Lifetime | 禁用/重启bridge、logout/reload/进程退出与WPM交错 | 明确已能拒绝的切点和未能消除的最后窗口；不写“零风险”结论 |
| 保留上限 | 顺序1/10/140/1000条、最大请求/结果、无人ACK、lease失效、反复enabled/off | arena/result/ledger/回调数量有界；退役后源码/closure不强留；GC后无按命令数增长；无隐式结果丢弃 |
| 多实例 | 同exe/hash两个PID、PID复用、不同角色、多安装 | 准确进程创建实例隔离，错误目标零写入 |
| 跨build | Retail/Classic/Titan分别，未知hash/profile | 独立证据；未知拒绝；Forever仍非默认验收对象 |
| 登录 | 未进角色、角色选择、进入/离开世界、actorsecret/缺失 | 不伪造GUID/ready，不盲激活；host-only退出始终可用 |
| Ready | 静止ready=true、status撕裂、heartbeat回退/停止、nonce陈旧 | stale/unknown不授权，公开flag不能改变私有执行 |
| 轮询 | idle/active/后台/低FPS/加载/combat/同步长任务 | 有界CPU/次数，无catch-up洪峰，禁用无timer/event/OnUpdate |
| Doctor | 无runtime、foreignowner、ownbusy、布局不支持、原unknown | 只读、分层、有具体continuation；控制清理不被通用诊断挡住 |
| UI | connect/hash/prepare/执行/async/cancel/结果ACK/close | 只有实际active probe显示“Agent执行中”；空闲无动画 |
| Activation | 无Driver、空/坏Driver文件、预算0、17条未知输入类场景 | 精确退役不依赖业务预算；原始证据和未知保留 |
| 迁移 | 旧未决claim、未知/修改槽文件、过程崩溃、旧runtime仍活跃 | 拒绝不安全替换，不清他人claim，不猜兼容或自动回滚已发布版本 |

性能验收至少记录 enabled idle/active 的 callback 次数、CPU 分位数、帧时间影响、峰值内存、各消息大小的完整传输时间和控制响应时间。不能只报告总体成功率或某次 wall time；同步 probe 本身的耗时与协议开销分开记录。

## 23. 最终设计判定

该设计可以清晰消除旧系统中“取消排在业务后面”“断开必须先拿到业务结果”“activation 没有 Driver 就没有退役入口”“业务 budget=0 阻塞所有清理”等协议层死结，并给出 ready、doctor、UI 的单一含义。

它不能消除 Lua 同线程不可抢占，也不能凭软件协议给外部 WPM 增加 Lua 对象生命周期锁。产品实施必须删除 LoD，并证明新链路可达到的边界；不能以删除 LoD 为理由，把未证明的地址安全当作已解决。能力不足明确不可用，发布条件不足则阻止发布。

建议采纳本文件的协议/控制/结果模型进入 P0/P1；将外写生产可行性列为独立门槛，明确保留研究结果与生产承诺之间的区别。主代理负责把此独立审查与用户最终决策、[Skill/doctor 方案](live-duplex-skill-plan-2026-10-04.md)整合为最终实施合同。
