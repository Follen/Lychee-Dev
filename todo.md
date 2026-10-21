# 待办与验证范围

基线：3.2.4；更新日期：2026-10-21。发行版本源为 3.2.4，授权渠道为 `latest`。

## 待办

- [ ] FB-COMBAT-01实机复验：聊天框战斗按键、战斗中切换焦点及停用后的脱战恢复。代码已修复，受保护API的离线复现先失败、修复后通过；build/vet、Lua5.1必需完整Go套件及93项Node测试通过。本轮尚未部署或运行真实客户端，不计为实机通过。

- [x] SpellName查询修复：紧凑索引、SQL数值IN优化及Windows动态内存预算均已实现并验证。上限按可用内存和工作区并发容量确定，保留/元数据/临时缓冲按需扩容。原35-ID SQL返回35/35，8组克隆名一致；spell info实际元数据预算从128 MiB经3次扩容到320 MiB，本地单条和JSONL通过。完整Lua必需Go套件和93项Node测试通过；缺密钥仍标partial。已随3.2.3提交`3766f9c`推送main并发布至latest，本机CLI、Skill及7个客户端addon受管更新完成；已安装正式CLI复验35-ID SQL和spell info通过，详见[当前交付](handover.md#当前交付323-发行)。

- [x] 3.2.2发行：[PR #16](https://github.com/Follen/Lychee-Dev/pull/16)及必需CI通过，已合并main；`v3.2.2`固定`0bec60e`，[正式发布流水线](https://github.com/Follen/Lychee-Dev/actions/runs/38725056854)全部成功，registry回读确认`latest=3.2.2`、`next=3.1.2`，[GitHub Release](https://github.com/Follen/Lychee-Dev/releases/tag/v3.2.2)六项产物齐全。
- [x] 按用户授权，在3.2.2正式发布后完成本机受管更新：PATH CLI为3.2.2、commit=`0bec60e`、workspaceDirty=false，Skill及7个客户端addon均为current、complete=true；Skill需新Agent上下文，游戏内addon需后续reload，本轮不计作新的实机验收。

- [x] Retail 12.1.5当前角色禁用槽自动启用与Mailbox重载实测通过：17个禁用槽自动启用并保存，500/500游戏内启用，其他插件设置不变；Mailbox标志触发同CON/原期限内一次槽启用重载，报告verified/released，连接正常关闭。结果见 [handover.md](handover.md#当前任务槽位自动启用与-mailbox-重载)。仅本次测试版及所选角色，不计入已发布3.2.1或其他产品实测。

- [x] 3.2.1发布：[PR #15](https://github.com/Follen/Lychee-Dev/pull/15)审核及必需CI通过后合并，`v3.2.1`固定`10b561db`；[正式发布流水线](https://github.com/Follen/Lychee-Dev/actions/runs/38676733974)全部成功，registry回读确认latest=3.2.1、next=3.1.2，[GitHub Release](https://github.com/Follen/Lychee-Dev/releases/tag/v3.2.1)六项产物齐全。本机CLI/Skill/所选Retail文件已受管更新为正式3.2.1；CLI为干净发行提交，游戏内addon需下次reload激活。

- [x] R23无消费者WGC及顺序三轮R24/R25/R26清理完成；R27窄清理/有限独立终审完成。代码71d645ec已推，Windows CI38593192586五项必需作业及汇总全部通过。Linux完整结果2578pass/3既有平台fail/28skip单列；本次原生24349/31245=77.9293%，2894事件/2628叶节点pass，0fail、17skip（9helper+8实际可选）。分母删除不计新增业务覆盖。

- [x] 完成追加审计 R3/R4 的 13 项代码修复及正式回归；R3-C01/R3-C03仍为候选，R4-C01已修并通过双真实卷原生回归；条目与证据见 [审计报告](docs/Clean-repo-2026-10-14.md)。
- [x] 修复提交 `81ccdd5` 的 Windows 原生整套 CI（含 Lua 必需 Go、race、process、addon、package）通过；Linux 实际结果与可选环境限制见 [handover.md](handover.md)。
- [x] 本轮持续循环独立复审与修复；R5–R9确认项已交付，R10–R15在各自冻结范围复核完成。当前R16-02已修；R19撤回合成的R16-01确认分类，退役分支已清理、成功unbind终态预算阻断已修；R20遍历/预检分类修复并入924文件冻结树，R21整套完成并发现quiescent关闭wrapper组合缺口，最小修正已完成，R22完整Linux整套已结束（2576pass/3既有平台fail/28skip），新鲜独立复核完成无新增可操作离线缺陷；a60e294f已推送，原生CI38562152610五项必需作业及汇总全部通过；详情见[审计报告](docs/Clean-repo-2026-10-14.md)。
- [x] 冗余测试已完成两处最小删除及退役解析器测试清理；高风险补强后a60e294f实测24753/32099语句=77.1146%，2893事件/2628叶节点pass、0fail、20skip；分开记录业务场景与未跑环境。
- [x] 16项公开LuaLS/API/DBD/CascLib输入已在a60e294f原生全部执行通过；当前20skip为9helper入口与11实际可选环境/材料项。
- [x] 更正WGC验证义务：现行产品无捕获入口，旧SDK失败不再列用户待测；历史证据保留，删除无消费者代码不等于修好截图。
- [x] R17-01主Skill与Source规则已统一：固定Source优先、空结果自查、禁止猜raw路径、真实缺口/明确外搜例外；合同检查和R18独立17场景复核通过。规范复核不等于实际模型重放。
- [x] R20当前协议现场专项：已定位禁用槽导致未Receive，以及空回执表命名节点的稳定区域拒绝。e17f0759完成首发诊断、公开map固定元数据键及doctor诊断；同进程新runtime的无输入空回执对照由失败转为完整0记录，bind/报告/release/disconnect通过。旧3.1.2原包的唯一根因仍未知，不将当前结果倒填旧证据。
- [x] 用户追加高风险覆盖补强已交付：数据保全、非幂等恢复、取消/事务和并发边界均有独立触发与负对照，当前Windows正式回归通过，未为覆盖数字改产品合同。
- [x] 完成本轮7项实机整合：Retail12.1.0.69933通过Trace/Objects真实页面回调、用户实际战斗与按键、数值往返、running原请求resume、bridge off/on及保存off重载、默认option重新加载、bind回执与新鲜替代runtime退役。e17f0759修复后空map和完整交易复验通过；报告verified/released、本轮三CON均closed、doctor全绿、0预约。范围与限制见 [handover.md](handover.md#7项实机结果)。
- [ ] 补齐七产品的干净首次安装、重新登录和完整升级实机矩阵。
- [x] Retail 12.1.5.70391/120105完成受管升级、启动识别及单实例容量验证：125份verified/released报告、500次业务槽交换、1次自动reload；504次输入交换各提交一次，500个数值比较通过，最终disconnect且doctor全绿、0预约。仅本轮升级和容量路径，不扩展为七产品完整安装矩阵。
- [x] R1215-INPUT-01键盘优先等待已实现，并按用户要求更新本机CLI/Skill完成12.1.5实测：按住W时两次发送尝试均0消息，松开469ms时仍等待静默，随后完成原请求；四个业务交换各提交一次，报告verified/released，disconnect后doctor全绿、0预约。desktop、channel整包与定向race通过；仍不能保证消除与已开始组合键恰好重叠的误触。修改已随3.2.1公开发布，本机CLI已从实测开发包更新为正式包。
- [x] e17f0759追加一轮500次业务槽交换：125份verified/released报告、1次自动容量reload及重连、504次输入交换无重复；500个数值比较通过，最终disconnect与doctor确认无owner、0预约。
- [ ] 补齐战斗、受限编辑框和最小化窗口等真实阻塞场景；现有离线测试不能替代这些验收。
- [x] 3.2.0已发布到 `latest`：固定提交3057a966的[发布流水线](https://github.com/Follen/Lychee-Dev/actions/runs/38608441597)完成CI、打包、Windows隔离安装、registry回读与[GitHub Release](https://github.com/Follen/Lychee-Dev/releases/tag/v3.2.0)。本机全局CLI及受管Skill均已升级并核验版本。

## 已验证范围

七产品均已完成 Source 同步，以及固定构建的本地 CASC / 远端 CDN 数据抽样。抽样为 ChrRaces ID 1、2，不代表全部表和资源。

| 产品 | 当前协议实机结果 |
| --- | --- |
| Retail | 500 槽容量轮换、释放后 100 槽续跑、同安装双实例、读取中断恢复 |
| Classic 5.5.4.70268 | 200 → 500 槽受管迁移、125 次容量调用、释放重连后 25 次调用 |
| Classic Era 1.15.9.70003 | 125 次容量调用、释放重连后 25 次调用；掉线后保留原报告并清理原连接 |
| Titan 3.80.2.70315 | 125 次容量调用、释放重连后 25 次调用 |
| Forever Asia 1.60.1.70374 | 更新构建后自动定位、125 次容量调用、释放重连后 25 次调用；FunctionTrace 重入复现、受管修复及同客户端复验 |
| Forever CN beta | 500 槽容量轮换、释放后 100 槽续跑、聊天框聚焦时重载 |
| Anniversary 2.5.6.70006 | 125 次容量调用、释放重连后 25 次调用；修复后复验及重登探针 |

上述容量轮换包含自动重载。已完成测试的报告与清理均已核实，最终连接关闭、槽预约释放。容量测试证明正常轮换和续跑，不表示单个 runtime 的 500 个物理槽全部逐一执行。

Forever Asia 在 1.60.1.70338 和 1.60.1.70374 均验证过根扫描；构建更新后没有修改 CLI。70374 的三个特征命中解析到同一根，wowdump 独立对照一致。一次 CLI 观察读取 31,916,032 / 75,236,476 字节，记录 2,324 个缺口和 4 次读取失败：成功定位不等于完整 `.text` 覆盖。

2026-10-14 在非战斗的 Forever Asia 1.60.1.70374 验证 FunctionTrace：GetTime/InCombatLockdown 的 hook 会重入，修复后每次外部调用只记录一次，追踪可继续。相同 PID 完成受管部署前后对比；最终 reload 清除测试 hooks，两条连接关闭，doctor 全绿且无槽预约。此客户端禁止 hook math.min/max，该结果不扩展为其他客户端 API 的实机证明。

## 离线基线

2026-10-14 审计修复后，Go build、vet 及 Lua 5.1 必需的完整 Go 套件通过，包并发 2、测试并发 4、包时限 20 分钟。21 项离线基线和八项真实 LuaLS、broker、安装包 Source 语义集成均执行通过；受影响的 75 项 Node/启动器测试、隔离 npm 升级、版本和 Skill 合同检查通过。

符号链接权限、固定外部资料、真实样本和明确客户端环境所需的可选测试仍有跳过，未计作通过；七产品实机矩阵未因本次修复重跑。后续 C-01 日志容量、C-05 broker 关闭交接均经确定性回归确认并修复，C-06 的实机范围见上文。新增 broker 关闭回归已接入发行组装，必跑真实集成增至九项。逐项结果见 [清理与修复报告](docs/Clean-repo-2026-10-14.md)。

追加修复后的 build/vet、受影响 Go/Lua 包与重新组装开发包的 Source 语义测试通过；已验证 C-01 的原期限和未知输入约束，以及 C-05 关闭后重新连接和查询。未重复未受影响的安装长套件与 Node 全套。

检查入口见 [开发指南](docs/development.md)。
