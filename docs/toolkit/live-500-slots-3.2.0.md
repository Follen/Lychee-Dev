# 3.2.0 开发线：500 槽

当前输入与命名由 [Lycheedev mailbox protocol v1](lycheedev-mailbox-protocol-v1.md) 统一约束。
下方 2026-10-13 的容量记录属于历史协议；当前协议的本轮结果如下。

## 2026-10-14：同一 Retail 安装双实例

北京时间 00:29–01:08，两个 `12.1.0.69933` / Interface `120100` 进程共享
`D:/Game/World of Warcraft/_retail_` 的受管插件与 500 槽池，各用独立 project、PID、
进程创建标识和 CON。使用当前协议开发 CLI，未修改产品代码、重新安装或发布。

| 实例 | 并行容量循环 | 容量自动重载 | 隔离检查后续跑 | 总成功调用 |
| --- | --- | --- | --- | --- |
| 灵止光—死亡之翼，PID 30552 | 122 次 | 第 122 次，原 LMO/期限继续 | 释放原 CON、重新连接后 25 次 | 147 |
| 荔枝小月亮—燃烧之刃，PID 58940 | 122 次 | 第 122 次，原 LMO/期限继续 | 保持原 CON 再执行 25 次 | 149，含两次异步隔离检查 |

合计 296 次业务报告 `verified`、`ok=true`、`cleanup=complete`。每次核对请求标记、
角色 GUID、runtime、连接 owner、协议名和 500 槽身份；未发现串实例执行或结果混用。
每边容量循环累计消耗 488 个业务槽；续跑各消耗 100 个业务槽。这里验证 500 槽池的
正常容量轮换，不声称逐个执行所有 500 个物理槽。

- 两个 project 分别尝试连接已由对方持有的 PID，均返回预期的 `journal.foreign_owner`。
- B 的 35 秒异步任务处于 running 时，仅重载 A：A 换 runtime，B 保持原 runtime 并正常完成。
- B 的另一次异步任务处于 running 时，仅断开 A：B 正常完成；A 用新 CON 重连后两边并行续跑。
- 最终三个 CON 全部 closed；主插件及 500 槽仍为 managed，pending=0，安装目录内 owner 文件为 0。
- 核对当前日志及所有 `.jsonl.history`，共 44 段；A/B 分别有 598/602 次提交输入，均为
  invoke/reload、每次 6 条窗口消息、零 Esc。同一 runtime/exchange/action 没有重复提交。
  442/672 次 `not_sent` 记录均为 0 条窗口消息；容量切换未更换原 LMO 或恢复起止期限。

本轮有 **6 次 Mailbox 读取中断**：A 初始对齐重载的 `string_layout`，A 第 60/63 次与
B 第 44/105 次的 `path_changed`，B 第 122 次容量重载的 `lua_state_null`。全部返回
原 CON 的 `continuation=continue` 并在原期限内通过 `live resume` 完成。
其中三次中断验证脚本，由主代理续行；另三次由验证脚本续行。原始 exit 3/5 与恢复结果均保留，
产品 CLI 没有因本轮测试而修改。**双实例功能与恢复检查通过，不记为全程无错误的稳定性通过。**
随后 [读取中断原因调查](mailbox-read-interruptions-2026-10-14.md)在两进程中分别只读采样，
确认 INPUT 字符串指针更新会触发 path_changed；Go 调度器直接退出，没有在当前调用内继续观察。
其中重载 string_layout 的具体对象缺少历史字节证据，保留该限制。随后 Go 修复与双实例复测
已完成，见调查记录的“修复与复测完成”；原六次错误不被改写为无错误通过。

连接：A `CON-ee6163d75063b92f9a756f6ab207d2d1`，释放后
`CON-7ead09b32d98b9504efa1bad89c691ac`；B `CON-1ddce0bb8012605459bee0c835e5ed8e`。
证据在 `.tmp/live-protocol-v1/retail-dual/`：`report.json`、`journal-audit.json`、
`final-installation.json`、逐次 CLI 结果和 `a/`、`b/` 下完整 `.lycheedev/live/`。
运行脚本为 `.tmp/live-protocol-v1/retail-dual-run.mjs`，审计脚本为同目录 `retail-dual-audit.mjs`。
本轮未触发对外返回的 shared publication blocker；不继承战斗、受限焦点、进程崩溃或重新登录验收。

## 2026-10-14：国服无限与正式服并行容量及释放后续跑

北京时间 00:00–00:17，使用同一开发 CLI、各自独立 PID/CON 和安装目录并行执行。
正式服主插件与 500 槽已受管升级；其原 hybrid 运行时由用户手动重载一次激活新协议。
之后两服的容量重载均由 CLI 根据新鲜 INPUT 自动发送独立 F11 完成。

| 目标 | 500 业务槽 | 容量自动重载 | 释放后再跑 100 业务槽 | 最终状态 |
| --- | --- | --- | --- | --- |
| 国服无限测试服 1.60.1.70338，晴—岚，PID 61388 | 125 次完整调用全部成功 | 第 122 次，nextSlot=488，新 runtime 后继续成功 | 断开、确认无占用，再用新 CON 完成 25 次调用 | closed、managed、pending=0 |
| 正式服 12.1.0.69933，荔枝小月亮—燃烧之刃，PID 58940 | 125 次完整调用全部成功 | 第 122 次，nextSlot=486，新 runtime 后继续成功 | 断开、确认无占用，再用新 CON 完成 25 次调用 | closed、managed、pending=0 |

合计 300 次调用，每次实际消耗 4 个业务槽，报告 `verified`、`ok=true`、
`cleanup=complete`；每次同时核实全部 500 槽元数据、协议名、角色和运行时。
500/100 是跨合法容量轮换累计的业务槽消耗，预留物理控制槽按协议保留。
两边续跑阶段均从 nextSlot=20 推进至 120，结束后再次正常断开。

容量切换保留原 LMO 与恢复期限，完整日志及 `.jsonl.history` 各记录 606 次提交输入，
每次 6 条窗口消息；每服恰有一次自动 reload，零 Esc、零重复提交 input ID。
连接分别为：

- 国服：`CON-6141c3d925803f5bd999fe341de61f1f` → 释放后 `CON-96195dc329ebb9e1338a782064eca659`。
- 正式服：`CON-1430d30142140e8839f11d91dad6185a` → 释放后 `CON-40cbfb350d959c87aba2969d31500fa2`。

汇总在 `.tmp/live-protocol-v1/capacity-500-release-100-summary.json`；原始逐次结果与
释放检查分别在同目录的 `cn-capacity-release/`、`retail-capacity-release/`。
完整连接日志位于 `.tmp/live-shortcut-focus/.lycheedev/live/connections/` 及各自 history。
本轮覆盖两个不同产品、不同安装目录的并行运行；后续 Retail 共享安装双实例结果见上节。
未修改产品代码、提交或发布。

## 初始容量合同与历史协议验收

2026-10-13。本阶段先完成单个 Retail 实例的槽容量扩展。此合同覆盖旧
200 槽合同中的容量、留存和安装迁移数字；七产品实机、Retail 双实例及新的
Mailbox 输入就绪设计分别验收，不借用本阶段结果。

## 运行时与 CLI

- Go `bridge.SlotCount` 和 Lua `SlotProtocol.Count` 为 500；Compat 从后者读取加载边界。
- 物理槽为 1–500，耗尽游标为 501。Lua 每次 wake 最多遍历 500 个空槽或外部 runtime 槽，
  遇到本 runtime 封套即停止。禁止重用已加载槽或重发未知输入。
- 最多留存 500 条 nonce 回执和 121 个操作身份；报告 release 后释放 BODY 和代码。
  `floor((500-16)/4)=121`；CLI 在新任务 `nextSlot > 485` 时执行现有容量 reload，
  保留原请求和期限，尾部控制容量用于确认、释放与断开。
- wire06、slot.v3、identity.v2、input.v3、hybrid.v2 和 Mailbox 定点读取路径继续使用。
  本次不更换输入协议，不增加帧、事件、计时器或常驻扫描。

## 受管安装升级

安装器创建 500 个 LoadOnDemand addon。64 槽旧 schema、200 槽旧 schema 和当前
slot.v3 的 200 槽安装只能用于受管文件迁移，不能被当作可运行的 500 槽安装。
即使版本号相同，容量不同也必须扩容。

扩容前核实原文件和预约，拒绝未解决预约和扩展范围内的未知目录；保存迁移意图后，
可从部分静态文件或部分扩展目录继续。迁移标记升级为 `lycheedev.slots.migration.v4`，
避免旧 200 槽安装器把未完成的扩容提交成 200 槽。旧 v3 迁移意图先转为新标记再续行。
全部文件完成后原子提交 500 槽账本。每个 PID 仍须独立证明 `slots=500`、`inventory=500`；
客户端未发现新增目录时需要重启，磁盘安装不能代替运行时证明。

## 验证记录

已通过构建、vet、Lua 插件全套、Go/Lua 协议套件、命令包及槽位/迁移定向回归。
覆盖 500/501 边界、121 个完整操作、最高槽发布与退役、空槽遍历、禁用状态、旧 64/200
槽扩容、同版本升级、中断续行及外部预约保留。完整通道包中唯一失败为旧测试把 202 槽
视为非法；非法样例修正为 502 后，输入诊断组通过，其余通道用例已通过。本轮没有重跑
与容量改动无关的整个仓库，也没有提交或发布。

单 Retail 实机已通过，范围如下：

- 客户端 `_retail_`，build `12.1.0.69933`，PID `6196`。
- 通过受管安装器从干净的 200 槽安装升级；一次激活 reload 后，真实身份回执为
  `slots=500`、`inventory=500`，不需要重启客户端。
- CON：`CON-05d54f07574180b16b6b8c68d0fc4a14`；runtime：
  `0000000500028446d135df1e58b1d5b7`。
- 同一 runtime 连续完成 51 次只读探针，每次均为 `reportState=verified`、
  `report.ok=true`、`cleanup=complete`；未通过中途 reload 重置槽位。
  第 51 次操作在第 202 槽开始，完成后 `nextSlot=206`，实际越过旧容量。
- 探针核实第 1、200、201、499、500 槽的实际客户端元数据、transport 和 release。
  最后使用第 206 槽断开，回执为 `closed=true`、`bound=false`、`nextSlot=207`。

本地证据在 `.tmp/live-500-retail/`：`install.json`、`connect.json`、`execute.json`、
`cross-200.json`、`cross-final.json`、`disconnect.json`；对应日志在本工作树
`.lycheedev/live/`。旧 Lychee 项目中已请求关闭的连接由匹配的 200 槽 CLI 按
`process_absent` 完成原断开，没有删除 claim 或向旧进程发送输入。

最终开发包为 `.tmp/live-500-retail-final/dev-npm-stage`，CLI 位于其
`native/windows-amd64/lycheedev.exe`；addon 字节与本机安装一致，最终 CLI 已用于断开验证。
这是 `3.2.0` 分支的本地开发产物，版本文件仍为 `3.1.2`，全局 npm 包未替换。
后续操作这个已升级 Retail 时使用该开发 CLI。

实机未执行到第 500 个物理槽或容量自动 reload；这两项本轮由离线边界/协调器回归覆盖。
上述 Retail 记录不包含其他产品、双实例竞争或重新登录；国服无限首次安装补测见下节。

## 国服无限测试服补测

2026-10-13，`wow_cn_beta` / `_cn_beta_`，build `1.60.1.70338`、Interface `16001`，
PID `42704`、进程创建标识 `134363471982352441`，角色 `晴—岚`。
运行时产品族为 `forever`；安装目录和发行产品身份仍与亚服 beta 分开。

首次连接复现 `memory.lua_mailbox_root_unsupported: anchor_missing`，当时尚未创建
CON 或发送游戏输入。使用 **wowdump 3.0.1** 只读取得当前模块运行时字节：
142,495,744 字节均可读，`.text` 的 69,805,516 字节中，新增 49 字节特征只有一个命中。
当前代码用 `rbp` 保存 Lua context，旧 70124 特征固定的是 `r15`，因此旧模板无法匹配。

- 模块文件 SHA-256：`20d62993de642e17055bdd2b2575f3ca571f86763b301ebc77765da5547aa1f2`。
- 运行时锚点 RVA `0x63441b`，RIP 位移解析得到 Lua 根 RVA `0x7fa8878`；
  锚点 SHA-256：`ff532e0aafd0440198f65b4ab4dfdcc40152d636b91e914d25100228d79b5c76`。
- 三次相同 getter 调用均指向 `0x4080d90`，读取 state 的 `+0xe0` context；
  后续沿 state `+0x90` globals 的具名查找读到 `_VERSION="Lua 5.1"` 和 `RunScript`。
- Go 新增 `forever-lua-state-context-rip-v1`，只通配 RIP 位移，固定寄存器关系和重复
  call 目标约束。当前 build 仍由运行时代码匹配定位，没有新增固定根地址或私有堆扫描。
  wowdump 对整个运行时 PE 的认证曾报 `PE_INVALID`；调查保留该限制。Go 原有的同一哈希
  文件节布局与运行时节表校验通过，没有修改或补写运行时 PE 头。

新回归用例先复现 `anchor_missing`，修复后整个 `internal/live/memory` 包通过；
另覆盖位移迁移、跨读取块命中、新旧模板竞争、重复调用不一致和寄存器不一致。
`go build ./...`、`go vet ./...` 通过。未为本次定位修复重跑无关的全仓库测试。

真实 CLI 验证已完成：

| 步骤 | 结果 |
| --- | --- |
| 首次受管安装与自动激活 | 原安装为空；安装主插件和 500 槽，一次激活 reload 后连接成功，无需重启 |
| 运行时容量 | `slots=500`、`inventory=500`；探针逐一检查 1–500 槽的编号、transport 与 release |
| 第一次执行 | `LMO-2206083447d285059eafb080cbd666e4`，报告 verified、ok=true、cleanup=complete |
| 显式 reload | 同一 CON、PID 和角色重新绑定新 runtime，容量仍为 500 |
| 旧请求重读 | 返回原 LMO 与原 runtime 的报告，新 runtime 的 nextSlot 仍为 2，没有重复执行 |
| reload 后新执行 | `LMO-a7a5064e723e4523abaa3abac24324c9`，再次检查全部 500 槽，报告与清理均通过 |
| 正常断开 | `closed=true`、`bound=false`、nextSlot=7；安装保持 managed，slots pending=0 |

CON 为 `CON-711dc6f86a20c5f0886938bdd270348d`；reload 前后 runtime 分别为
`00000001000292ec04f2e9023288080c`、`000000020002936e9efab2c071d620be`。
证据在 `.tmp/live-cn-beta/`：`connect.json` 保留原失败；`cn-anchor-proof.json`、
`lua-root-probe-v3.json`、`runtime/cn-beta-lua-root.json` 保留定位证据；
`install.json`、`connect-fixed.json`、`execute.json`、`reload.json`、
`retained-after-reload.json`、`execute-after-reload.json`、`disconnect.json`、
`addon-final.json` 保留实测结果，完整连接日志在 `.lycheedev/live/`。

后续使用含修复的开发 CLI：
`.tmp/live-cn-beta/dev-release/dev-npm-stage/native/windows-amd64/lycheedev.exe`。
版本文件仍为 3.1.2；未提交、发布或替换全局 npm。
上述基础测试未覆盖容量自动 reload；随后完成的容量循环见下节。
重新登录、双实例竞争及其他尚未测试的产品均不标记通过。

### 国服容量循环实测

同日按用户要求补做一次真实容量循环。继续使用同一 PID、创建标识、build 和角色，
新连接 `CON-f08be9e92662a5e1c429c17119bce18e` 从现有 runtime 的 `nextSlot=8` 开始。
通过公开 `live execute` 串行调用，**122 次请求全部完成**，每次均为
`reportState=verified`、`report.ok=true`、`cleanup=complete`，没有外部 resume 或重发。
执行循环约 943.9 秒；每次仍逐项检查全部 500 槽的实际元数据。

| 阶段 | 实际结果 |
| --- | --- |
| 同一 runtime 连续消耗 | 前 120 次操作从第 8 槽推进至 `nextSlot=488`，实际业务槽用到 487；中间没有 reload |
| 容量触发 | 第 121 次请求 `cn-beta-capacity-121` 在 `488 > 500-15` 时产生 `automatic_reload_intent`，`automatic=true`，reload request 为 `capacity-90583ec2016ad65d61f66332d3f074f0` |
| 原请求继续 | 同一 LMO `LMO-a6e94af47e9fa39454c59ffdcf6a1cf6` 在新 runtime 完成；自动 reload 继承原恢复期限，未换请求或延长该期限；这一调用耗时 16.729 秒 |
| 切换后继续执行 | 第 122 次 `cn-beta-capacity-122` 完成，LMO 为 `LMO-e2d35d8c0f654e9348713447527b4ca0`，新 runtime 的 `nextSlot=10` |
| 断开与安装检查 | 第 10 槽正常 unbind，`closed=true`、`bound=false`、`nextSlot=11`；主插件与 500 槽均 managed，pending=0 |

旧 runtime 为 `000000020002936e9efab2c071d620be`，新 runtime 为
`0000000300029cb66acec21a4969ac26`。整个循环由 CLI 自然触发一次容量 reload，
没有人为修改槽游标、写入 slot payload 或调用手动 reload 来替代容量行为。

本项通过的是 **500 槽协议的实际容量边界、自动 reload 与后续执行**。
协议预留尾部控制容量，因此本轮没有强行消耗物理槽 488–500；不能表述为
“每一个物理槽都执行过”。最高槽的逐项元数据检查和离线边界测试仍分别保留其范围。

证据目录 `.tmp/live-cn-beta/capacity-cycle/` 保存 `summary.json`、122 份原始 CLI 结果、
`automatic-reload.json`、`disconnect.json` 和 `addon-final.json`；
起始连接为 `.tmp/live-cn-beta/capacity-connect.json`，运行脚本为
`.tmp/live-cn-beta/capacity-cycle.mjs`，完整协议日志仍在 `.lycheedev/live/`。
此次只补实测和记录，没有修改生产代码，也未提交或发布。

## 泰坦重铸容量循环实测

2026-10-13，`wow_classic_titan` / `_classic_titan_`，build `3.80.2.70315`、
Interface `38002`，PID `57064`、创建标识 `134363530770986967`，角色
`Qingtianjiuz—时光I`。原干净受管 200 槽通过 `addon install --output <archive>`
升级为 500 槽；自动激活 reload 后成功连接，无需重启客户端。

CON 为 `CON-85ad7aa947f5dcd67dd4d2981861eed8`，初始 `nextSlot=2`。
通过公开 `live execute` 串行完成 **123 次请求**，每次均逐项检查 500 槽元数据，
报告 verified、ok=true、cleanup=complete；执行循环合计约 1,139.1 秒。

| 阶段 | 实际结果 |
| --- | --- |
| 连续消耗 | 同一 runtime 完成前 121 次请求，实际业务槽用到 485，`nextSlot=486` |
| 容量 reload | 第 122 次 `titan-capacity-122` 自然触发，日志为 `automatic_reload_intent`、automatic=true；原 LMO `LMO-92f0e11c96d8912d18339682c439c9d8` 与恢复期限保留，调用耗时 28.859 秒 |
| 切换后继续执行 | 第 123 次完成，新 runtime 的 `nextSlot=10` |
| 正常断开 | 第 10 槽 unbind，closed=true、bound=false、nextSlot=11；主插件与 500 槽均 managed、pending=0 |

runtime 从 `000000120002a4aaace9adee90837c9e` 切换为
`000000130002a96c6c61589d29b8389c`；reload request 为
`capacity-304ac58763454ee92ac2a28e7620f560`。没有手动 reload、外部 resume、重发或
游标修改。物理槽 486–500 属于预留控制容量，本轮未强行逐槽执行。

证据在 `.tmp/live-titan-asia/titan/`：`upgrade.json`、`upgrade-archive/`、
`connect.json`，以及 `capacity-cycle/` 的 `summary.json`、123 份原始结果、
`automatic-reload.json`、`disconnect.json`、`addon-final.json`。
测试使用此前国服补测的开发 CLI；泰坦本项没有新增生产代码。

## 亚服无限运行时定位与首次连接

同日，`wow_classic_beta` / `_classic_beta_`，build `1.60.1.70338`、Interface `16001`，
PID `45596`、创建标识 `134363530642424051`。原安装为空，已受管安装主插件和 500 槽。
初次连接在发出游戏输入前失败：通用 `.text` 扫描首先遇到 `PAGE_NOACCESS`。

使用 **wowdump 3.0.1** 保存当前模块运行时镜像和覆盖账本；实际读取
89,624,576 / 139,165,696 字节，保留 2,310 个盲区，不将缺口补成已读字节。
可读区域中的 context 特征在 `0x6fcb98`、`0x764b4b` 两处出现，两处 RIP 位移均指向
Lua 根 `0x79f46b8`，重复 getter 均为 `0x4595eb0`，读取 state `+0xe0`。
沿根指针和 globals 的具名读取确认 state tag=8、globals tag=5、secret=0、
`_VERSION="Lua 5.1"`、`RunScript` 为函数，根指针重读稳定。

该区域构建 SHA-256 为
`cfe26cc200fda36407736c7e6b5575a1f2ca2b774c8176529814e32d51312185`。
CLI 沿用已存在的精确构建校验路径：匹配该哈希后读取 `0x764b4b` 的全部 49 字节，
与实际采样逐字节比较，再解析 RIP 位移，并继续验证根、运行时节表及完整 Lua 路径。
不跳过缺页做“唯一匹配”，不接受裸 RVA，也不扫描私有堆。未知哈希仍须通过原有完整扫描。

新的真实锚点回归先复现 `module_read / NOACCESS`，修复后内存包全套、build、vet 通过；
用例同时确认指令变化和未知哈希不会继承本构建的定位结果。新开发包位于
`.tmp/live-titan-asia/dev-release/dev-npm-stage`。其 CLI 首次连接成功并自动激活插件，
确认角色 `Ailuoli—Free`、`slots=500`、`inventory=500`。

证据在 `.tmp/live-titan-asia/asia/`：`connect-unsupported.json` 保留原失败；
`runtime/asia-beta-lua-root.json`、`runtime-patterns.json`、`lua-root-probe.json`
记录运行时字节、覆盖限制和 Lua 读值；`install.json`、`connect.json` 保存首次安装与连接。
全局 npm 未替换，版本文件仍为 3.1.2；没有提交或发布。

### 亚服容量循环实测

同一 PID 与角色，CON `CON-b175845a41ac183f0d8f430e7e0fefd8` 从 `nextSlot=2`
开始，公开 CLI 串行完成 **123 次请求**，每次均核实全部 500 槽元数据。
报告 verified、ok=true、cleanup=complete 全部通过，执行循环约 1,174.8 秒。

| 阶段 | 实际结果 |
| --- | --- |
| 连续消耗 | 同一 runtime 完成前 121 次请求，实际业务槽用到 485，`nextSlot=486` |
| 容量 reload | 第 122 次 `asia-capacity-122` 自然触发 automatic_reload_intent；原 LMO `LMO-36926e8046fbf6dfe55425ae4b5b82ac` 与恢复期限保留，调用耗时 24.642 秒 |
| 切换后继续执行 | 第 123 次完成，新 runtime 的 `nextSlot=10`；重载后的 Lua 根定位和 Mailbox 路径通过真实调用 |
| 正常断开 | 第 10 槽 unbind，closed=true、bound=false、nextSlot=11；主插件与 500 槽均 managed、pending=0 |

runtime 从 `000000010002a9d9d9774e90079f1d76` 切换为
`000000020002ae65f9fa10361722932e`；reload request 为
`capacity-ef8282635616459f790aee71525e8782`。已核对完成结果与自动 reload 意图中的
LMO、恢复期限一致，没有手动 reload、外部 resume、重发或游标修改。

证据在 `.tmp/live-titan-asia/asia/capacity-cycle/`：`summary.json`、123 份原始 CLI
结果、`automatic-reload.json`、`disconnect.json`、`addon-final.json`。
两服复用 `.tmp/live-titan-asia/capacity-cycle.mjs`，完整日志分别留在本工作树
`.lycheedev/live/connections/<CON>.jsonl` 及对应 `.history/`。

两服本次均通过 **500 槽容量阈值、自动 reload、后续执行和正常断开**；
预留控制槽 486–500 没有强行逐槽执行。重新登录、Retail 双实例和其他产品仍须独立验收。
后续统一使用包含亚服定位修复的本地开发 CLI：
`.tmp/live-titan-asia/dev-release/dev-npm-stage/native/windows-amd64/lycheedev.exe`。

## Classic Era 支持与运行时定位

2026-10-13，`wow_classic_era` / `_classic_era_`，build `1.15.9.70003`、Interface `11509`，
PID `29448`、创建标识 `134363597656112063`，角色 `看么就看了你—匕首岭`。
原安装为空；接入时发现客户端 profile 和 TOC 缺少 Era，已补齐 Go 基线、Lua ClientGate、
主插件及 500 槽的 Interface 声明，并生成独立 Era 事件目录。

源码基线为 `classic_era` 提交 `8165d4cd6e48d606369336cc3a7977902310e81e`
（版本 `1.15.9 (70003)`），从其 APIDocumentationGenerated 提取 1,483 个事件及参数。
该提交已经通过此前 Source sync 固定；本轮没有借用其他客户端目录。
新 Era profile 回归先复现缺失，修复后通过；五种 profile 的全部 Lua 套件通过。

使用 **wowdump 3.0.1** 只读保存运行时模块，125,571,072 字节全部可读，
无盲区或部分读取。现有 `LuaMailboxRootPattern` 在完整模块中唯一命中：

- 模块文件 SHA-256：`a417628399fc4dabf990e9fe015aa8f44f700107e35049b5ce06766e754299f2`。
- 运行时锚点 RVA `0x62fcc4`，解析 RIP 位移得到 Lua 根 RVA `0x70273a8`。
- 实际 state tag=8，state `+0x90` globals tag=5、secret=0；具名读取确认
  `_VERSION="Lua 5.1"`、`RunScript` 为函数，根指针重读稳定。
- 运行时 `GetBuildInfo` 的第四返回值调用 `0x33b1020`，其指令返回十进制 11509；
  插件激活后的真实 Lua 调用再次确认版本、build 和 Interface。

本轮不修改 Go 内存定位器；沿用现有特征、RIP 位移和 Lua 路径。上述 RVA 为采样证据，
没有硬编码为新根。完整运行时镜像摘要为
`dffd7065ad0263e6f1101e6796db41919bade83447642aec0fff754e00cc8a86`。

Era 开发包通过 `addon install` 受管部署并自动激活，CON 为
`CON-ea8edcd71b42b0f4a332998f2164682a`。初始 runtime 为
`000000010002c2ee73c455c4393605d0`，连接确认 slots=500、inventory=500。
现有四 Interface TOC 仅按精确历史字节识别；安装迁移沿原维护锁和升级日志进行，
未决预约会阻止迁移。64/200 槽旧协议的历史声明保持原样。

首次探针错误地断言 `GetAddOnMetadata(name, "Interface")` 可返回 TOC Interface；
该 API 在本客户端返回 nil，因此报告 verified、ok=false，资源仍正常释放。
独立诊断请求核实实际 API 行为，随后从磁盘逐项确认全部 500 个 TOC 均声明 11509；
容量探针改为检查运行时 Interface 与全部槽的编号、transport 和 release。
原失败和诊断结果保留，未重发原请求或修改槽游标；正式容量循环从 nextSlot=10 开始。

证据目录 `.tmp/live-era/` 保存 `runtime/era-lua-root.json`、`runtime-patterns.json`、
`lua-root-probe.json`、`interface-getter.txt`、`install.json`、`upgrade.json`、
`upgrade-archive/`、`connect.json`、`metadata-inspection.json`、`toc-verification.json`
及 `first-probe-summary.json`。开发 CLI 为
`.tmp/live-era/dev-release/dev-npm-stage/native/windows-amd64/lycheedev.exe`。

离线验证：`go build ./...`、`go vet ./...`、selection 全包和强制 Lua 5.1 的五 profile
addon 全套通过，Node 包装/隔离更新 14 项通过。delivery 全包运行中，新的 Era TOC 迁移
及其他非历史样本用例通过，但旧 v1 fixture 从当前 TOC 生成，错误带入了 11509。
已将其 TOC 固定为真实历史字节；所有使用该 fixture 的迁移、预约及跨代拒绝用例
定向重跑通过（118.164 秒）。没有为此重复全包或声明新的全仓库 baseline。
同版本 TOC 迁移另覆盖既有四 Interface 安装可读、未决预约阻止迁移及完成后的声明更新。
本轮版本文件仍为 3.1.2；未提交、发布或替换全局 npm。

### Era 容量循环实测

同一 PID、角色和 CON，正式循环从诊断后的 nextSlot=10 开始。公开 CLI 串行完成
**121 次请求**，每次核实全部 500 槽元数据，报告 verified、ok=true、cleanup=complete；
执行循环约 1,107.7 秒。前置两次诊断占用 8 槽，因此本循环次数与泰坦/亚服的 123 次不同。

| 阶段 | 实际结果 |
| --- | --- |
| 连续消耗 | 同一 runtime 完成前 119 次正式请求，实际业务槽用到 485，nextSlot=486 |
| 容量 reload | 第 120 次 `era-cycle-120` 自然触发 automatic_reload_intent，原 LMO `LMO-94bbb88f8fad6b1c007cf8c856882fdd` 与恢复期限保留，调用耗时 20.978 秒 |
| 切换后继续执行 | 第 121 次 `era-cycle-121` 完成，新 runtime 的 nextSlot=10 |
| 正常断开 | 第 10 槽 unbind，closed=true、bound=false、nextSlot=11；主插件与 500 槽均 managed、pending=0 |

runtime 从 `000000010002c2ee73c455c4393605d0` 切换为
`000000020002c7e6952ee8ae13682d09`，reload request 为
`capacity-fa0cd8ec3c6a655906c72d946a8b2ddf`。自动 reload 日志与完成结果中的
LMO、恢复期限一致；没有手动 reload、外部 resume、重发或游标修改。

本项通过 **500 槽容量阈值、自动 reload、后续执行和正常断开**；
预留物理槽 486–500 没有强行逐槽执行。重新登录、双实例及其他产品不继承本次结果。
`.tmp/live-era/capacity-cycle/` 保存 `summary.json`、121 份 `era-cycle-*.json`、
`automatic-reload.json`、`disconnect.json` 和 `addon-final.json`；原失败的
`era-capacity-001.json` 继续保留。完整日志在 `.lycheedev/live/connections/<CON>.jsonl`
及其 `.history/` 中。
