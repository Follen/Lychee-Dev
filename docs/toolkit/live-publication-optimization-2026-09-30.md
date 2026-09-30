# L6-A：保留原始协议的发布编码复用

日期：2026-09-30；实现基线 `8e4dab0455c8bee512c73912c26b31bc9605f6c2`。
状态：工作树实现与离线定向验收通过，未部署、未发布；组合候选全量验收由主任务另行记录。
Retail / Classic / Titan 实机矩阵、首次安装和升级的实际游戏激活、游戏 CPU/GC 与端到端时延均为 **not_run**。
Forever 仅保留模拟 profile，不计客户端验收。

## 合同与实现范围

本次只实现 L6-A。研究依据为同基线的 D 阶段发布/LuaLS 合同与 source/live L6 审计；
当前运行合同仍是[200 槽分配](live-slot-routing-2026-09-29.md)、
[统一输入架构](live-input-architecture-2026-09-28.md)和[输入节奏](input-cadence-implementation-2026-09-29.md)。
没有新增 schema、能力、beacon、固定地址、FFI 或进程内存写入。

- [SlotProtocol](../../addon/Bridge/SlotProtocol.lua) 每次 Describe 仍观察 actor，检查当前输出字段；
  比较覆盖 runtime、owner/fence、nextSlot、slots、character/realm/guid、build/product/release、inventory、inputState 与 schema。
  只有已成功编码的 primitive 值可复用；secret、table、非法 UTF-8、过长字段等继续经过原拒绝路径。
  每个 engine 只保留当前 identity 表、JSON 和 descriptor/sequence，没有历史列表。
- 相同字段可以复用 JSON；只有 header sequence 也相同才复用整条 descriptor。
  bind 内部 Describe 后 publish 会增加 sequence，因此返回后的 Describe 仍构造新的完整记录。
  空槽、畸形内容、foreign 消耗、拒绝、bind/unbind、fence 与 actor 变化均使相关字段或 sequence 失配。
- [MemoryProtocol](../../addon/Bridge/MemoryProtocol.lua) 私有地保存一个当前 runtime 的 16 字节 token 编码以及固定 zero token，
  在同一记录内复用 nonce/runtime 字节。每次调用先验证当前 token 与字段，命中缓存不跳过 secret 检查。
  kind/state/sequence 也显式检查类型、整数与界限，畸形输入返回 `memory_record_invalid`。
  完整 payload 与最终 header 的 checksum 继续逐字节计算；格式、字段顺序、长度、头尾和数字编码不变。
  这份有界 token 缓存不是当前记录或授权事实，不能启动采样、恢复旧连接或证明运行时有效。
- [InputState](../../addon/Bridge/InputState.lua) 生产代码未改。
  每次观察仍读取当前 identity、时钟与输入门禁，再编码并增加 sequence。
  即使 JSON 完全相同，仍发布新序号记录并翻转光学 heartbeat；不做整条记录去重。
  Start 即时观察、1s 周期、已知 wake/close 后 Refresh、不重置周期、Stop 移除回调/事件均保持现状。
  主机 500ms 新鲜性与 after+100ms 门槛也不变。本次没有引入固定字段 JSON 拼装器。

## 离线验证

使用 Lua **5.1.5**，`LYCHEEDEV_REQUIRE_LUA51=1`，解释器固定为
`D:/Code/wow/addons/Lychee Dev/.tmp/memory-channel-lab2/lua51/lua.exe`。
构建缓存独立放在任务 `.tmp/go-cache-publication`。

新增[发布等价性套件](../../tests/addon/t_publication.lua)：原始 framing 独立 oracle，
5 种 kind、空/二进制/Unicode 载荷、0/最大序号、最大长度、token 切换/复用、secret 与畸形字段；
Describe 当前角色读取次数、完全相同记录复用、bind 内外 JSON 等价且 header 新序号，
owner/fence、unbind、空/foreign/畸形槽、拒绝、全部 adapter/actor 字段变化与字段消失，
长度/转义/UTF-8、可变 table，以及新 engine 的 reload/relogin/upgrade 缓存隔离。

[InputState 套件](../../tests/addon/t_input_state.lua) 追加不变 ready 的 sampleMillis、相同 JSON 的新 sequence、
leaving/loading 隐藏 authority 与重新进入世界恢复；保留首装禁用零 frame、显式 Stop/Start、
停止后 Refresh 不复活、reload 新运行时、时钟回退/secret/观察错误等原断言。
既有 slot/runtime 和跨语言 driver 生命周期套件继续覆盖 receipt、HEAD/BODY、业务恢复与输入释放。

[跨语言 fixture](../../tests/protocol/publication_wire.lua) 产生 23 条记录；
[Go 对照](../../tests/protocol/publication_wire_test.go) 解码后以独立 Go encoder 重新编码，逐字节比较完整记录，
另核验载荷、sequence、actor、owner/fence 和 slots/nextSlot。
覆盖最大 512KiB 载荷、三种序号、五种 kind，以及 bind 内部/外部 sequence 差异。

本批通过：`go build ./...`、`go vet ./...`，以及强制 Lua 的
`go test -count=1 ./tests/addon ./tests/protocol ./protocol ./internal/bridge`。
最后一次定向执行耗时分别为 4.557s、13.930s、0.651s、0.344s。
Lua 四个 profile 是模拟测试，不把 fixture 中的停启/世界切换/版本替换算作真实 reload、relogin 或安装升级通过。
本页不以定向测试代替最终 `go test -count=1 ./...` 与离线 baseline。

## 独立解释器收益与复现

[对照入口](../../tests/addon/publication_benchmark.lua) 从两个 addon 根分别加载发布实现。
baseline 三文件从 `git show 8e4dab0:addon/Bridge/<file>.lua` 保存到临时目录，candidate 使用本任务 addon；
入口不读取游戏或发键：

```text
lua publication_benchmark.lua <baseline-addon-root> <candidate-addon-root>
```

7 次重复交替先后顺序，使用 `os.clock`，每次先 collect 再停止 GC，执行有界批次，记录
`collectgarbage("count")` 增量后重新开启 GC。停止 GC 的分配量只说明此 fixture 的 Lua 堆增长，
不是正常 GC 开销、峰值 RSS 或 WoW allocator 行为。
每次两侧最后记录逐字节一致；另外独立统计 bind 的实际调用形状，避免计数 wrapper 影响计时。

| 场景 | 批次 | baseline 中位 CPU 秒 | candidate 中位 CPU 秒 | baseline 中位运行堆增长 KiB | candidate 中位运行堆增长 KiB |
|---|---:|---:|---:|---:|---:|
| input JSON + 完整 wire；每条新 sampleMillis/sequence | 5,000 | 0.197 | 0.168 | 19,725.672 | 18,749.421 |
| 新 engine 初始 Describe、bind 内部 Describe/publish、外部 Describe | 1,000 | 0.189 | 0.149 | 18,545.511 | 15,958.998 |

实际 bind 调用形状：baseline JSON=4、wire=4；candidate JSON=3、wire=4。
wire 次数不降低，体现了 header sequence 的保留。完全相同 Describe 的 100 次人工重复仅用作
正确性测试，没有纳入上述收益估计，也不用于声称生产命中率。

本机原始输出位于任务忽略目录 `.tmp/publication-benchmark.log`；入口与表格随源码保留，
不依赖该临时文件存在。基准只隔离 addon 发布工作，不能据此宣称游戏总 CPU 或 CLI 操作稳定提速。
真实双实例、低 FPS/战斗/加载、禁用/启用、clean managed 首装/升级、reload/relogin、WGC、
扫描噪声与端到端验收仍为 **not_run**，沿用主任务实机矩阵。
