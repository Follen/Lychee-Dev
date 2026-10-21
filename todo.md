# Rust 架构实施清单

## 当前唯一优先主线：完整新架构先完成
- [ ] 统一正式DataFusion执行核：typed发现/批量ID/筛选/汇总/关联/排序/分页/导出同核；旧SQL语法兼容不作必达目标，但业务能力必须typed等价或更优；保精确数字、数组、Hotfix、partial与证据，不长期双执行器。
- [ ] Agent同进程session与owner结果句柄：真实CLI入口、版本/身份、独立取消、请求去重、上限/过期/断连、异常退出和预算guard回收；结果页不是CAP别名。
- [ ] 默认可配置5GB全Data口径、共享物理内容去重、冷淘汰、active/evidence保护与写入峰值；不长期保留三份派生。
- [ ] 新CLI自描述和Skill任务路径一致；固定Go历史oracle不动，新架构扩展独立验证。
- [ ] 保留Source已验证二进制底座与Live现有私有SessionOwner/Mailbox v1；完整集成质量先行，随后性能，最终五循环。

以下旧优化/诊断清单保留为历史与待验证事实，不再挤占上述主线，也不表示统一新架构已经实现。

当前批：ea7原生585通过/1原ignored，完整七调用通过但性能未达标。四文件共享数字Indexed内层已独审及隔离差分验证；新Windows产品质量/收益待测。

以 [handover](handover.md) 为当前事实、[DESIGN](DESIGN.md#authorized-architecture-implementation-plan--2026-10-19) 为完整计划。质量基线f811；用户指定后续性能参考固定Go324，并保留现有Rust优化。先业务质量/完整实现，再≤30%耗时门禁，之后正式五项循环。历史已完成项与原失败留在[308efa旧清单](https://github.com/Follen/Lychee-Dev/blob/308efa2766d00273b13c9c7e24a0b956bb60e9e9/todo.md)，不滚动复制到当前待办。

## 立即闭合固定基线
- [x] ea7原生585通过/0失败/1原ignored、完整七调用正确性通过；hit Rust301.2569/Go112.4970ms，跨单轮下降非因果证明且未达标。
- [ ] 共享内层最终SHA：先名单确认不重叠Core public_json、Data完整local_query/relational/generation/phase_profile及CLI data_phase_profile集合，7新增各一次；原size/alignment、Arc spare allowance、default-stack/深度/预算失败禁止改限额或计费。locked/offline/-j1、test-threads=1、原600s，禁止one-shot。
- [ ] 精确匹配产物完整七调用/数值/CAP/IPC/manifest/认证/复用/最终完整性及原计数；隔离构造12.648→9.101ms、drop2.284→.257ms不代表SQL/RSS收益，性能验收仍独立待办。
- [x] d952首批335+补61 Rust通过，Node307通过/1 symlink权限skip；七调用完整正确性通过，hit Rust339.3229/Go113.6338ms，inline无明确实际收益，未达性能门禁。
- [ ] ASCII快路最终SHA：先列libtest名单并排除交叉重复，按完整relational、hotfix::remote、local_query、generation、unicode_checks调用方集合验收；四新增各一次、原ignored保留。统一locked/offline/-j1、test-threads=1、600s原默认栈，禁one-shot，不仅测helper。
- [ ] 新匹配优化产物完整七调用及startup归因、原全量SQL/CAP/IPC/manifest/最终完整性；孤立查找20.452→2.558ms不是SQL收益，原性能目标不变。
- [x] fca完整generation一对诊断通过所有结果/CAP/IPC/复用/finalIntegrity；fresh/first/hit Rust仍慢于Go，不是正式性能验收，历史收据不补造startup归因。
- [ ] 本批最终SHA：all-targets、完整relational（原execution/default-stack/depth/真实future-size门禁不放宽）、generation与local_query输出/投影回归，六新增逐名一次；保原ignored。Node890/0/11及语法通过不是native证明，当前8e Source诊断不抢占。
- [ ] 匹配新优化产物及固定Go/fixture运行新profile-observation，七份实测receipt全部startup归因、完整16,384×5有序结果/CAP/manifest/IPC/复用及finalIntegrity，helper关闭诊断，保全部完整时钟。98,304分配减少仅源码推导，收益尚未测；20对正式验收单独无profile执行。
- [x] 7bd Source六场景完整Facts/Index/summary/custody及finalIntegrity通过；四增量单pair低于0.30，cold/warm未达，20对未验收。实际warm typed decode 121642µs属于cached_index 168301µs父桶。
- [x] 05660组合default/DF两check和定向native948通过/6原ignored，Source4与容量15新增逐名各一次；不借用于本批新插桩。
- [x] 05660匹配Source六场景完整Facts/Index/summary/custody及finalIntegrity通过；仅insert/delete/rename单pair低于0.30，cold0.41458/warm0.40232/change0.31859未达，非20对验收。typed decode 106632µs与7bd121642µs跨run差异不可作因果结论。
- [ ] 本批test-only decode/control诊断已独审，三active回归及一个默认ignored优化诊断NOT_RUN（owned.windows_required）；最终组合九套Node422/422不是native证明。保持当前fca9质量队列；之后唯一父队列固定新SHA完成Source all-targets/lib，逐名确认三新增和原ignored，再用现有认证单Index fixture及封存优化test artifact独立exact运行三轮诊断。禁用计时内计数但保真实checks，不做相减归因/收益声明。
- [ ] Source低重复/唯一路径语料及独立20对正式性能验收仍待；不能用测试诊断替代生产端到端门禁。
- [x] 7bd Data新runner精确timeoutSeconds=300后完整七CLI及全部完整性通过；原7786五样本后失败保留。单pair约慢Go3–4倍，不是20对验收。
- [x] 7bd uppercase source BlobRef修复：default/DF all-targets及generation42实际通过，原失败保留。
- [ ] 本批固定最终发布SHA跑all-targets与Data phase_profile、local_query generation/regression、generation及CLI profile回归，6新增逐名一次，resource v5断言与原ignored库存独立保留。当前Rust NOT_RUN，Node/语法不是native证明。
- [ ] 新v2 --profile-observation绑定本批最终产品commit/clean sourceTreeSHA256/optimized binarySHA256，automaticEvictionEnabled=true，仅一对诊断，七CLI/四双profile/完整rows/CAP/IPC/finalIntegrity及owner退役全部验证。旧7bd false合同保持，原预算与timeout300不变；不把嵌套计时相加或诊断当性能收益/20对验收。

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
- [ ] [Data缓存测试映射](tests/rust-parity/data-cache-quality-test-map.json)保留历史静态范围；已执行各组以当前逐名记录为准，剩余缺证单列，不将整个旧映射重列待跑或冒全过。

## A. Home、配置与全Data容量
- [x] 实现decimal5GB可配置基础、派生目录与至少真实cache prepare消费者；初批运行验证未闭。
- [x] 整合共享blob、decoded spool和CAP独立引用保护28代码路径，静态独审通过；保留发布后错误receipt与278测试修正。
- [x] 56新增中uppercase历史CAP失败已在19e精确复验通过，070f原55/1不重标。
- [x] 已实现并独审精确缺体认证重建与repair峰值计量，保留原pair/CAP/keep及旧reader fail-closed；27新增保原分批实测记录；b47本地decoded9已通过，当前两个失败待修后复验，完整产品与全容量路径仍未验收。
- [ ] 完整接入Data归属：下载、共享blob去重、Arrow/索引、staging/导出峰值、CAP和SQLite/WAL/SHM/历史记录；明确共享跨域归属和去重，不把局部准入称总硬上限。
- [ ] 可重建冷缓存淘汰必须保护活跃lease与证据引用；旧超额/缩配置拒新增并报告，不删证据。验证导入峰值，避免raw/IPC/Parquet长期三副本。

- [ ] b47库存目录13新增中12通过、1个目录rename测试前提失败；最小fixture修正保真实open/close与fresh计量，修后待实际复验。没有此Core片匹配wall收益。
- [x] b47本地decoded合法缺体精确重建9项实际通过：Root/selected公开入口、alias/pair/CAP/keep保持，坏体硬拒及EOF身份/取消/预算。

## B. Data统一typed执行与按需缓存
- [x] f811 check及71项针对性回归实际通过；不等于全仓/性能。
- [ ] Arrow cache prepare真实生命周期、fixed generation、schema/partial/ID/capture与晚期close故障完整验证。
- [ ] DataFusion资格通过后核真实provider、Hotfix、预算/取消、精确数值、任务质量及cold/warm/内存/磁盘/包体；适合才收敛单执行内核，不长期维护重复自研引擎，不装daemon。
- [ ] Lookup/投影/过滤/batch与复杂SQL共享执行/证据，Agent调用和token收益以真实任务证明。

## C. Source认证、按需消费与binary cache
- [x] 子树scratch与移除旧JSON缓存计数遍历两片在0bca实际check通过；Source730+inspect15+flow4+真实luac1=750通过/0失败/6忽略，13新增/调整名称及1细化各一次；原binary/owned/输出额度保留。86f匹配产品及六场景完整性全部实际通过；该轮86f六项耗时比仍>0.30，继续定位剩余串行及增量聚合。
- [ ] 完成当前Facts Postcard正确性和匹配性能；旧cache失效重建，新损坏不得fallback成健康。
- [x] 整合已独审Index r2与正式v13-postcard-index18诊断；固定Go323来源已核；070f七Node为357通过/0失败/4权限skip。
- [x] 旧validate fixture在19e闭合；0bca完整Source库730通过，6个lib ignored仍单列，真实luac精确用例实际通过。
- [ ] 86f六场景完整Facts/Index/summary/custody及finalIntegrity全部通过；该轮86f六项耗时比仍>0.30，未达正式70%门禁。2d原最终失败、666新增只读复核保留；后续改动保持完整认证与公共输出，首次导入/query成本不移到未测命令。

- [x] 借用 Symbol 后继：44a实际check通过，index155通过/1原exporter忽略、inspect15与flow4通过，合174通过/0失败/1忽略，10新增逐项一次；共享四来源映射、完整wire字节与取消/损坏回归已执行。精确完整组合树九套Node382/382通过，0失败/skip。44a匹配产物六场景总wall已测，四增量单pair达线、cold/warm和20对仍待，Symbols工作转入IndexSerialize不得当独占收益；warm无此片收益。

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
- [ ] 同机同数据/结果完整等价，cold/warm和增量分开正式性能验收，Rust≤Go30%。44a四增量单pair达线，cold/warm未达0.30、20对正式验收未完成；Data CSV旧新Rust诊断不是Go验收。
- [ ] 内存采样gap、资源计量与RSS区分；受限原始本机证据不夹公开docs。
- [ ] 两门禁通过后依次循环：架构/业务/Skill→覆盖有效性与冗余→死代码→联合全仓→Rust原生度。此前预审不追认为最终完成。
- [ ] Coverage companion因raw总预算超限仍未完成；不增限、不继续扩工具抢实现/性能主线。真正OS压力/CLI真实使用/游戏缺证分别保留。


## 当前窄后继（不是门禁完成）
- [ ] 复验b47两原失败：Core保持held目录拒替换并核unknown真实open/close和同名leaf新身份；SQL引用合法表名First后实际到达双表关闭失败、资源释放与已提交CAP断言。
- [ ] 显式cache prune --data-derived：23新增Rust实际编译执行；多manifest/active pin/CAP、canonical orphan、容量缩小、取消和部分完成真实报告。默认普通prune保持。
- [ ] 自动准入退役与全5GB尚未闭；先复用同quota内退役能力、完整峰值重盘点，不递归取quota、不把receipt当已释放空间；下载/sharedblob/CAP等归属缺口仍保留。
- [ ] Source profile v2四子段：历史v1兼容已Node验证，Rust及真实warm计时待；不将嵌套桶相加、不把计时增强当性能收益。
