# Rust 迁移交接：唯一当前事实来源

## 1. 当前目标和执行顺序

当前执行已批准的 [完整架构计划](DESIGN.md#authorized-architecture-implementation-plan--2026-10-19)，不是开始新一轮正式审计。顺序是：**Source/Data 业务质量不低于固定 f811 → 全部实现门禁 → 同任务 Rust 可控耗时 ≤ Go 的 30% → 五项循环审计**。必要开发独审、差分和回归不是最终五循环完成证据。

- 质量基线 `f811fc7f75fc206158cecb01dd0eb5d39e88b35f`：实际 check 与71项针对性测试通过，16新增逐名一次；不是全仓通过。准确性、完整性、排序/可读性、来源和已提交证据不能退步，已认可的 Rust 语义优势保留。
- Go 业务/性能参考固定 v3.2.3 peeled commit `3766f9cacb52e93a803a626187e2a1d54a445468`。既有增强单独有任务级 oracle，不机械回退正确结果以求原始字节全等。
- 产品仍为3.5.0 beta配置；无发布、合并或游戏操作授权。Skill 七产品定义和双 Forever 分离、Mailbox v1、未知结果不重放、精确数字/partial/证据合同不变。Emmy使用固定未修改上游；自有业务Rust不新增unsafe，必要Windows边界独立审查。
- 当前仓库只在 `rust-refactor` / [草稿PR #14](https://github.com/Follen/Lychee-Dev/pull/14) 分批保存。用户本机只负责固定SHA测试及必要协调，由唯一父队列派发；运行中的身份不会随新head改变。

## 2. 当前实现批次与验证状态

| 范围 | 已保存/已实现 | 当前证据和剩余门禁 |
| --- | --- | --- |
| Core/Data/Source基础 | 8c9：配置、Data派生目录/准入、真实Arrow cache prepare、Source Postcard Facts | 官方依赖fetch成功；8bfe分组实际434 matched：430通过/3失败/1 exporter ignore；disk两失败待278复验，generation取消预期码一失败待修，不能声称完整正确性通过 |
| Live资源归属 | 5c60：私有 SessionOwner，native先于journal关闭与claim退役 | 8bfe connection61/durable36通过，六新增逐名一次；无新线程/daemon，真实客户端未验 |
| 编译修复 | 6ea恢复原SYM SHA writer；2b400显式Data结果类型；66d空slice断言类型 | 8c9、6ea、2b400、66d原编译失败保留。8bfe实际check通过，config14通过；disk26为24通过2失败（copy注入阶段不确定、Windows路径文本）。其余11组续跑392通过/1失败/1 ignore；新增失败为generation取消测试期待operation.cancelled、实际command.cancelled，后续断言未执行，单独修正和复验 |
| Skill精简 | 308efa：22指南文件、工具与身份/历史fixture，43路径逐字节/ Git blob核验 | 六Node套件101/101通过；25文件/57链接独审通过。两类MODEL_ONLY独立前后任务均通过修正模型：恢复旧/新调用8/11次、身份核查旧/新6/7次，不能推断新版更优；原宽松模型接受非法wait=0的记录保留。四类真实CLI任务未跑，文档缩短不是质量通过 |
| Source正式二进制证据 | 740a：已独审10文件工具片，完整Facts exporter与六场景适配 | 七Node套件382/382通过；新Rust exporter/Windows/性能待验证。历史v11读取与新v13显式分开 |
| DataFusion适配资格 | 独立可选资格片已保存并静态独审，固定54.0.0/Arrow58.3.0 | 14数值/计划回归未编译未运行；默认依赖图不引入DF，生产执行器尚未切换；不能宣称适合或提速 |
| Data磁盘后继 | 共享blob准入、精确decoded spool与CAP独立引用保护已整合28代码路径 | 56新增正式回归均未跑；只覆盖已接入归属，历史blob/CAP/SQLite等全口径和冷缓存淘汰仍未闭，不宣称5GB已全闭合 |
| Source主Index二进制 | 已整合24文件r2与8文件正式Node适配，采用共享typed模型/Postcard，公共JSON不变 | Rust静态复审通过，25 active/1 ignored未跑；Node七套独立361/361通过。固定Go323字段/资产顺序来源已核，新格式真实Windows、导出容量与性能仍待 |

已完成的DBCD四项修复保留：WDC5 palette物理顺序、多词metadata、官方BUILD预设、非SQL实际读取限制传播。c571与9004分批合计113 distinct通过、3原ignore；旧红例及四个admission测试失败保留，不能将小fixture称作真实>256MiB表验证。产品定义未改变，完整产品矩阵仍待。

## 3. Home、容量和所有权边界

Data默认容量为**5,000,000,000字节（十进制5GB）**，配置支持明确单位；与测试scratch 5/6GiB不同。最终统一核算Data下载/共享blob归属、物化/索引/目录、staging/spill/导出峰值和证据占用，按内容去重防双计。活跃lease和证据引用保护；旧已超额或配置缩小先拒新增并说明，不删除证据。

CAP删除会识别同body的独立Data保留权：仅删除CAP引用并返回仍共享，不承诺物理释放。判定在原blob lease内认证固定Data记录和body类型/长度，不倒序获取quota或重复全读hash；这是保留权判断，同长度坏内容仍留存，真实消费继续完整认证。损坏记录、孤立完成记录或已登记body缺失/类型长度不符均硬拒，不静默减账。Data引用的冷缓存释放与完整淘汰生命周期仍须后继闭合。

当前基础只覆盖已接入的Data派生写入。共享blob、decoded spool后继未全部运行，历史未登记blob、CAP、SQLite/WAL/SHM等尚未闭合归属。不能把派生对象helper当全Data硬上限。只缓存实际查询过的表，临时查询可不落盘；不默认长期保留raw、IPC、Parquet三份。尚未实现并验收的淘汰策略不称已生效。

Source主Index采用明确v18身份与LYCIDX01帧，query metadata同步版本化。完整文件SHA认证后，共享typed字段校验供完整与摘要消费；摘要不构造全量owned Symbol。128MiB限制新缓存实际文件字节，构造分配另行有界准入，不再为旧JSON内部表示额外全量计数。因新版本及时取消可先于尚未读取的损坏尾返回；已经确认的语义/预算错误保持，不把坏缓存作为健康结果。正式诊断保完整公共Index与Facts，两个exporter输出共用原128MiB证据额度，超过明确失败，不能截断比较。资产比较保f811已认可的大资产前置稳定分区和组内完整顺序/重复。

Source新格式旧cache失效后重建，不维护生产旧格式迁移。新格式完整认证、边界/预算/取消和损坏拒绝仍必须保留；坏新缓存不能通过旧格式fallback隐藏。公共JSON和query/inspect业务输出保留，私有wire变化单独版本化。

## 4. 性能与已知证据范围

最新完整Source诊断为d427，同run Go/Rust毫秒：cold7730.4355/4036.1422；warm559.6647/345.8190；change2740.9258/1127.5592；insert2725.3417/1156.8068；delete2726.4203/1149.3939；rename2842.2565/1168.1344。六场景完整Facts/summary/custody通过；均未达到0.30，只一轮诊断，内存采样缺口不能证明RSS峰值改善。parse并行累计、visit/Facts/workerwait嵌套不可相加伪造独占。

Data CSV9693工具对旧63d与a860的单pair：cold401.4790/389.9062ms，warm349.0380/324.7937ms；完整CSV/sidecar与8CAP核验。该对比是旧Rust/新Rust，不是Go性能门禁；retainedBytes减少46,988,672B与decodeWork减少81,920是逻辑计量，不是RSS。其余Data、Live正式性能与全产品矩阵仍未验收，不把历史结果归新产物。

历史coverage十二integration实际130通过/1权限ignore，报告是独立对象分母，不能平均成全项目覆盖率。f86 companion真实四CLI产生profile后超原raw128MiB总预算，merge/export/witness未执行；不加预算，覆盖工具扩展暂停，先完成业务与性能门禁。真正OS压力、游戏、当前Skill实际使用仍有缺证。

## 5. 下一批及证据规则

1. 27827已保存disk两项测试修正及新增发布后失败事实回归，固定check/disk27/DF资格队列待结果。8bfe其余11组已结束，generation取消测试另修command.cancelled公开期待，并补exit7与无可见object断言；未复验不标绿。保持原600秒/default stack，各SHA和历史unknown辅助进程forced事实分开。
2. 按已保存的固定SHA提供Source诊断工具和DF资格片的命令/依赖前置并执行验证；DF用显式非默认feature，先官方锁定依赖获取再资格测试，不能替代生产正确性/内存/性能门禁。
3. Skill用固定前后指南、同一真实CLI/fixture做独立任务盲评；模型输入与真实CLI/客户端分别记证据。所有本机任务仍唯一串行队列。
4. Data容量后继与Source Index候选完成独审后集中回归，再匹配唯一产品构建及同工作量性能。尚未完成的共享所有权/预算不能用文档承诺代替接线。

当前摘要只列决策与开放项。完整历史测试、失败、旧SHA和各次报告留在[308efa历史handover](https://github.com/Follen/Lychee-Dev/blob/308efa2766d00273b13c9c7e24a0b956bb60e9e9/handover.md)及对应PR评论；旧证据不改标新候选。完整功能/协议和累计版本要求见[验收矩阵](docs/toolkit/rust-migration-acceptance.md)、[Go323增量](docs/toolkit/rust-go-3.2.3-increment.md)。不向公开摘要夹带受限本机原始路径、日志或身份数据。

## 11. 最后执行：需要实际配合的测试清单

**本节必须始终是handover的最后一节。** 只有前述可独立完成的实现、离线差分、自动Windows测试、封存候选和可自动性能检查就绪后再约实际配合。当前全部 `not_run`；以下“结果”故意留空待真实执行填写。每次先固定candidate commit/exe/package/addon hash、产品/build、所选安装/PID/创建时间/HWND及授权操作；缺客户端或用户未就绪留缺口，不缩范围。普通Windows API单元/自有子进程/合成窗口CI不在本节。

| ID | 配合/前提 | 步骤 | 预期证据 | 结果（实测后填） |
| --- | --- | --- | --- | --- |
| H01 | 用户指定真实安装和角色；七产品逐一可用 | Retail、Classic、Classic Era、Titan、Forever Asia Beta、Forever CN Beta、Anniversary独立identify/connect/execute/report/cleanup/disconnect | 同candidate各产品身份、完整精确receipt和清理；两beta不按同build合并；不能以Source/Data成功代替Live | |
| H02 | 选定未安装或旧200槽真实客户端，用户可手动reload/重启 | 首装/升级后doctor；无可验证INPUT时手动reload；未发现500槽时重启；原CON resume | 未盲发输入、原目标/CON不变，旧200仅升级识别，500发现/启用/加载事实分别记录 | |
| H03 | 用户允许安全编辑框/聊天焦点和键位冲突场景 | 测两实际绑定、已有绑定保留、聊天/普通EditBox透传、secure/protected拒绝、UIParent隐藏、最小化和全部键盘VK按住/释放、500ms静默中再次活动、活动与已开始chord重叠、仅释放本attempt按键 | 不改文字/光标/焦点，不抢前台，不Esc/Enter；恢复handler，500ms后新鲜INPUT、原deadline不延长、submitted/uncertain不重放及精确receipt各自证据 | |
| H04 | 同安装双实例、跨build并行及混合多实例就绪 | 分别连接/执行/关闭，交错共享槽发布和选定实例reload/doctor --fix；至少Retail双实例，其他产品依声明范围补齐 | 独立身份/owner/fence/CON，短锁预约不串线，其他实例不被授权/升级结果冒认 | |
| H05 | 用户允许真实容量与reload/relogin验证 | 连续运行触发500槽完整操作容量边界，先日志后reload/重绑同请求；reload/relogin前后验证 | 旧槽不复用、预算不延长、目标固定、exact报告与清理，运行时退役明确 | |
| H06 | 用户同意可控故障与恢复窗口 | bind无回执关闭、submitted/uncertain中断、发布替换、release无回执5秒cleanup reload、退出/重开CLI、opaque与observation区别 | 无未知业务重发；原request/ticket/期限/attempt保留；报告可用和cleanup分离，退役后才runtime_destroyed | |
| H07 | 用户同意在选定真实角色设置受管槽禁用并恢复 | 按角色GUID启用、复查并保存；doctor只读核slot_activation；有新鲜INPUT时按原CON reload，模拟多次replacement与可控保存失败 | 不LoadAddOn、不消费槽、不动foreign元数据；slotReloadRequired未清不发槽动作，reload自身可执行；原deadline/交易/unknown不重放，失败不报ready；记录恢复步骤 | |
| H08 | 同机同数据Go/Rust正式候选与用户允许的只读/有界Live采样 | 按B06独立cold/warm交替重复，真实游戏等待分段；保持同业务/输入/校验 | 原始计时/输出等价、excluded阶段与总wall时间，未达0.30照实失败，不影响前面可自动工作 | |
| H09 | 若既有“十分钟空格”人工活动仍由用户确认需要 | 先确认当时任务目的和安全场景，再由用户实际配合；不把历史计划当持续输入授权 | 人工行为与工具输入分开标记，明确时长/目标/结果，不冒充CLI输入或协议通过 | |
