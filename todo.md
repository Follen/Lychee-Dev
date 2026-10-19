# Rust 架构实施清单

以 [handover](handover.md) 为当前事实、[DESIGN](DESIGN.md#authorized-architecture-implementation-plan--2026-10-19) 为完整计划。质量基线f811；性能参考固定Go323。先业务质量/完整实现，再≤30%耗时门禁，之后正式五项循环。历史已完成项与原失败留在[308efa旧清单](https://github.com/Follen/Lychee-Dev/blob/308efa2766d00273b13c9c7e24a0b956bb60e9e9/todo.md)，不滚动复制到当前待办。

## 立即闭合固定基线
- [x] 070f实际all-targets通过；2808四处编译错误和Node版本断言闭合，历史失败保留。
- [x] 原copy/发布后receipt路径/generation取消四个修复目标均通过；278 DataFusion可选资格14项通过保持独立，不代表生产执行器或性能。
- [x] 070f七Node为357通过/0失败/4权限skip；Rust为957通过/23失败/6 ignored，不称全仓通过。
- [x] 19e validate32/32通过，070f共享fixture的21失败逐项闭合，真实typed schema及全部业务断言保留。
- [x] 19e data_disk13/13、remove_commands22/22通过，两个Core失败及两新增均通过；一次消费、重复close事件和CAP签名/认证保留。
- [x] Source 25新增Index测试逐名一次通过；source_inspect15、flow4与正式认证luac真实ignored用例1通过。
- [x] 19e all-targets及67项定向回归全通过，原23失败闭合；不是全仓/性能通过。
- [x] 2d补官方flatbuffers许可证后新匹配release/assembly/exporter认证通过；19e旧装包失败保留。
- [x] 666对2d原六场景封存结果只读复核exit0，sealed_complete_scenarios_reverified，52,494文件认证；产品/build/exporter新增0。原runner失败/finalIntegrity=false及六组wall保留；Node498通过/0失败/7权限skip，非全性能门禁。
- [x] 10ed显式6GiB runner接线保存并完整readback；12套Node作者/独审各452/452；Windows实际447通过/0失败/5权限skip，原默认5GiB及其它限额保持。
- [x] ea Core/Windows131项及Node121项通过；e486两check通过但Data155为152通过3失败。86f两check、client23/prepared7/generation33/engine33共96项实际全过，原Root缺体重用、cold导入计量、DF Wildcard三失败闭合，8新增逐名一次。历史失败保留，不称全仓绿。
- [x] 最终组合Node检查记录sourceTree并区分git archive导出与export-ignore范围；原隔离361/361不能代替最终组合树结果。

- [x] CLI旧Go320元数据oracle已按固定Go323及明确新增更新，独立10项与10个物理坏例通过；最终组合11套Node121/121，无skip，非实际CLI任务验收。
- [ ] 按[Data缓存测试映射](tests/rust-parity/data-cache-quality-test-map.json)核真实286项名单并串行执行，94新增逐名一次，不将静态数当通过。

## A. Home、配置与全Data容量
- [x] 实现decimal5GB可配置基础、派生目录与至少真实cache prepare消费者；初批运行验证未闭。
- [x] 整合共享blob、decoded spool和CAP独立引用保护28代码路径，静态独审通过；保留发布后错误receipt与278测试修正。
- [x] 56新增中uppercase历史CAP失败已在19e精确复验通过，070f原55/1不重标。
- [x] 已实现并独审精确缺体认证重建与repair峰值计量，保留原pair/CAP/keep及旧reader fail-closed；27新增按实际Core/Data分组核验；未执行Data及原session/asset回归仍待实际运行，完整路径尚未验收。
- [ ] 完整接入Data归属：下载、共享blob去重、Arrow/索引、staging/导出峰值、CAP和SQLite/WAL/SHM/历史记录；明确共享跨域归属和去重，不把局部准入称总硬上限。
- [ ] 可重建冷缓存淘汰必须保护活跃lease与证据引用；旧超额/缩配置拒新增并报告，不删证据。验证导入峰值，避免raw/IPC/Parquet长期三副本。

## B. Data统一typed执行与按需缓存
- [x] f811 check及71项针对性回归实际通过；不等于全仓/性能。
- [ ] Arrow cache prepare真实生命周期、fixed generation、schema/partial/ID/capture与晚期close故障完整验证。
- [ ] DataFusion资格通过后核真实provider、Hotfix、预算/取消、精确数值、任务质量及cold/warm/内存/磁盘/包体；适合才收敛单执行内核，不长期维护重复自研引擎，不装daemon。
- [ ] Lookup/投影/过滤/batch与复杂SQL共享执行/证据，Agent调用和token收益以真实任务证明。

## C. Source认证、按需消费与binary cache
- [x] 子树scratch与移除旧JSON缓存计数遍历两片在0bca实际check通过；Source730+inspect15+flow4+真实luac1=750通过/0失败/6忽略，13新增/调整名称及1细化各一次；原binary/owned/输出额度保留。86f匹配产品及六场景完整性全部实际通过；六项耗时比仍>0.30，继续定位剩余串行及增量聚合。
- [ ] 完成当前Facts Postcard正确性和匹配性能；旧cache失效重建，新损坏不得fallback成健康。
- [x] 整合已独审Index r2与正式v13-postcard-index18诊断；固定Go323来源已核；070f七Node为357通过/0失败/4权限skip。
- [x] 旧validate fixture在19e闭合；0bca完整Source库730通过，6个lib ignored仍单列，真实luac精确用例实际通过。
- [ ] 86f六场景完整Facts/Index/summary/custody及finalIntegrity全部通过；六项耗时比仍>0.30，未达正式70%门禁。2d原最终失败、666新增只读复核保留；后续改动保持完整认证与公共输出，首次导入/query成本不移到未测命令。

- [ ] 借用 Symbol 后继：共享四来源映射、完整旧wire字节oracle、取消无主manifest/CAP与坏record修复回归已备10项；Rust尚未运行。精确完整组合树九套Node382/382通过，0失败/skip。固定产物总wall待测，Symbols工作转入IndexSerialize不得当独占收益；warm无此片收益。

## D. Live内部资源归属
- [x] 8bfe SessionOwner六新增逐名一次、connection61/durable36实际通过；真实客户端仍未验。
- [ ] 后续仅有证据的会话/调度/重复工作优化，保Mailbox v1、切角色身份、durable intent和Unknown不重放；不合并fsync弱化恢复。
- [ ] 实际客户端/游戏验证仍按handover末节，模型与离线结果不能冒充。

## E. CLI自描述与Skill
- [x] 308efa Skill/CLI发现接线静态独审，六Node套件101/101通过；产品定义不变。
- [x] 新增runner-only逐调用试用入口，源码独审及最终组合三套Node108/108通过；固定产物身份独立于runner，无产品重编；Windows入口Node107通过/1权限skip，受控真实help/describe、未知参数纠错与version首验完成。
- [x] 固定86f CLI与独立fresh home的旧/新8臂真实盲评：6通过、2 partial，旧新同评分；原点查缺key未覆盖SQL空partial，原评分保留。另2个全新SQL臂均通过，明确空结果不能证明不存在，CAP保留、fixture不变。不宣称新版整体更优，也不把脚本首验或MODEL_ONLY合计为盲评。
- [x] 断连和Source/Live身份矛盾已做独立MODEL_ONLY前后评分；修正模型两版均过，原wait=0替身缺陷保留，不能证明新版收益。
- [ ] 扩展到尚未覆盖的实际任务及Live/客户端仍需证据；本轮离线范围通过不等于全部Skill验收，不以模型、字数或单次调用数代替质量。

## F. 实现与性能门禁，之后正式五循环
- [ ] Source/Data结果质量≥f811及已认可增强；DBCD四修分批113 distinct pass/3旧ignore保持，完整产品矩阵仍需验。
- [ ] 同机同数据/结果完整等价，cold/warm和增量分开正式性能验收，Rust≤Go30%。当前全部最新Source六场景未达；Data CSV旧新Rust诊断不是Go验收。
- [ ] 内存采样gap、资源计量与RSS区分；受限原始本机证据不夹公开docs。
- [ ] 两门禁通过后依次循环：架构/业务/Skill→覆盖有效性与冗余→死代码→联合全仓→Rust原生度。此前预审不追认为最终完成。
- [ ] Coverage companion因raw总预算超限仍未完成；不增限、不继续扩工具抢实现/性能主线。真正OS压力/CLI真实使用/游戏缺证分别保留。
