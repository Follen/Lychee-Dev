# Rust 架构实施清单

以 [handover](handover.md) 为当前事实、[DESIGN](DESIGN.md#authorized-architecture-implementation-plan--2026-10-19) 为完整计划。质量基线f811；性能参考固定Go323。先业务质量/完整实现，再≤30%耗时门禁，之后正式五项循环。历史已完成项与原失败留在[308efa旧清单](https://github.com/Follen/Lychee-Dev/blob/308efa2766d00273b13c9c7e24a0b956bb60e9e9/todo.md)，不滚动复制到当前待办。

## 立即恢复编译与有限回归
- [x] 保存8c9基础、5c60 Live SessionOwner和三次最小编译修；Skill308efa全部43路径读回一致。
- [x] 固定Core data_resources旧config调用修复8bfe，独审和单文件读回通过；check与config14已验证，其他组见下。
- [x] 8bfe实际all-targets通过，config14通过；原8c9/6ea/2b400/66d编译失败保留。
- [ ] 闭合8bfe disk24通过/2失败：实际copy注入点及发布后错误事实、Windows完整路径语义；其余11组已终态392通过/1失败/1 ignore；generation取消预期码失败单独修正并复验。
- [x] 740a已保存Source Postcard正式证据工具并10文件读回；七Node套件382/382已过。
- [ ] Rust exporter与完整六场景实际验证。
- [x] 保存并静态独审DataFusion54可选资格片14项；默认依赖图隔离已核。
- [ ] 执行DataFusion资格片14项；获取官方锁定依赖，保持默认构建不引入DF。资格尚未过，不切生产执行器。

## A. Home、配置与全Data容量
- [x] 实现decimal5GB可配置基础、派生目录与至少真实cache prepare消费者；初批运行验证未闭。
- [ ] 验证共享blob准入及decoded spool精确预留后继，故障/取消/跨进程/cleanup与实际成本均须实测。
- [ ] 完整接入Data归属：下载、共享blob去重、Arrow/索引、staging/导出峰值、CAP和SQLite/WAL/SHM/历史记录；明确共享跨域归属和去重，不把局部准入称总硬上限。
- [ ] 可重建冷缓存淘汰必须保护活跃lease与证据引用；旧超额/缩配置拒新增并报告，不删证据。验证导入峰值，避免raw/IPC/Parquet长期三副本。

## B. Data统一typed执行与按需缓存
- [x] f811 check及71项针对性回归实际通过；不等于全仓/性能。
- [ ] Arrow cache prepare真实生命周期、fixed generation、schema/partial/ID/capture与晚期close故障完整验证。
- [ ] DataFusion资格通过后核真实provider、Hotfix、预算/取消、精确数值、任务质量及cold/warm/内存/磁盘/包体；适合才收敛单执行内核，不长期维护重复自研引擎，不装daemon。
- [ ] Lookup/投影/过滤/batch与复杂SQL共享执行/证据，Agent调用和token收益以真实任务证明。

## C. Source认证、按需消费与binary cache
- [ ] 完成当前Facts Postcard正确性和匹配性能；旧cache失效重建，新损坏不得fallback成健康。
- [x] 整合已独审Index r2与正式v13-postcard-index18诊断；固定Go323来源已核，Node七套361/361通过。
- [ ] 编译并实际运行25 active/1 ignored对应入口及既有完整消费者回归；独立旧Symbol/Document/Locator字节、取消首错/构造前预算均保。
- [ ] 新Index格式正式诊断reader/exporter接线完整后，再同输入cold/warm/单改/插删/rename；认证和query成本不移到未测后续命令。

## D. Live内部资源归属
- [x] 8bfe SessionOwner六新增逐名一次、connection61/durable36实际通过；真实客户端仍未验。
- [ ] 后续仅有证据的会话/调度/重复工作优化，保Mailbox v1、切角色身份、durable intent和Unknown不重放；不合并fsync弱化恢复。
- [ ] 实际客户端/游戏验证仍按handover末节，模型与离线结果不能冒充。

## E. CLI自描述与Skill
- [x] 308efa Skill/CLI发现接线静态独审，六Node套件101/101通过；产品定义不变。
- [ ] 同一封存CLI和fixture对固定前后Skill进行独立盲评：未知参数、版本/产品差异、空/partial、超能力任务；记录真实动作、结果、调用与误导。
- [x] 断连和Source/Live身份矛盾已做独立MODEL_ONLY前后评分；修正模型两版均过，原wait=0替身缺陷保留，不能证明新版收益。
- [ ] 实际CLI/客户端仍另列验证；不以模型、字数或单次调用数代替质量。

## F. 实现与性能门禁，之后正式五循环
- [ ] Source/Data结果质量≥f811及已认可增强；DBCD四修分批113 distinct pass/3旧ignore保持，完整产品矩阵仍需验。
- [ ] 同机同数据/结果完整等价，cold/warm和增量分开正式性能验收，Rust≤Go30%。当前全部最新Source六场景未达；Data CSV旧新Rust诊断不是Go验收。
- [ ] 内存采样gap、资源计量与RSS区分；受限原始本机证据不夹公开docs。
- [ ] 两门禁通过后依次循环：架构/业务/Skill→覆盖有效性与冗余→死代码→联合全仓→Rust原生度。此前预审不追认为最终完成。
- [ ] Coverage companion因raw总预算超限仍未完成；不增限、不继续扩工具抢实现/性能主线。真正OS压力/CLI真实使用/游戏缺证分别保留。
