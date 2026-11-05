# 当前交接

更新：2026-11-05，Asia/Shanghai。架构与合同见 [Design.md](Design.md)、[Live协议](docs/live-protocol.md)；整改背景见 [整改计划](docs/rectification-plan.md)，真实环境验收见 [todo.md](todo.md)。

## 分支与任务

- 用户已授权提交、推送、发布并更新本机全局CLI与Skill；追加排查[Fuploader PR #29](https://github.com/Follen/Fuploader/pull/29)已确认Go Publish同源分页bug并修复。当前准备3.2.8/latest，next标签不变；先完成本地验证再提交、推送并跟进发行CI。
- 正式无限服在线适配验证完成：`wowf/1.60.1.70790`、Interface 16001，当前源码CLI受管首次安装主插件及500槽。原PID 55328无运行时，activation连接`CON-12ca628ff5e242f5663712ce8d05bf73`已取消；用户新开PID 56184就绪，独立绑定`CON-019ad0cda62574d909beae9831304ab9`完成6份verified/released成功报告、八页面功能激活、执行/结果树/自有函数追踪/事件注册/异步回调、自动reload及原CON重绑。最终disconnect、Live doctor全绿、0预约，无新增产品代码。具体范围见todo的FOREVER-FORMAL-LIVE；全局npm和其他客户端未更新，未发布。
- 用户追加的一轮全仓边界复查完成，无新增可复现bug或待复现候选；覆盖CLI/Source/Data/Live/交付/Publish/npm及存储与LuaLS边界。SQL参数大小写候选经16组原生执行排除。当前无新增产品代码；`.tmp/forever-live/project`完整保留已关闭连接与实机报告，其余部署和审计临时材料已按脚本清理。
- 当前main，HEAD `9bc61af2b0`，领先origin/main三个既有提交；本次发布提交与tag尚未创建。远端npm latest仍为3.2.7，工作区版本源、渠道清单及生成文件已同步3.2.8。
- 用户要求全仓深度审计并强制循环到完整一轮无新增bug。本轮修复与复查完成，最后一轮无新增可复现问题、无待复现候选；所有改动保留未提交，未推送或发布。
- 主Agent独立负责，已有worker停写，无交叉修改。保留无关`.zcodeignore`删除、`.worktrees/Rust-refactor`、`.tmp/toolchains`与活动项目。

## 当前未提交实现

- BLACKBOX-CONTINUATION-TOTAL：133/173/205条历史的get/versions共6组在旧代码均报`version count changed`。只允许已有正总数的续页独立`total: 0`占位；11组无效续页继续拒绝，删除回读检查续页仍存在目标。先红后绿，build/vet与Publish整包388通过事件/5skip/0失败/exit0。仅fixture HTTP回放，未上传或删除平台内容。
- DEEP-LOOP共9处产品边界修复：DD/CF/NewBee回执完整JSON检查、NewBee状态指纹检查、DD备份清单完整JSON检查、缓存普通对象与暂存文件共享MaxObjects额度、小黑盒备份清单实际大小检查、Source索引及prune清单大小检查。公开入口或实际生成的清单/工作树回归均先红后绿；正常空白及恰好字节上限的对照继续通过。
- doctor关闭重试测试原50ms整次调用假设在完整套件中失败；注入75ms观察延迟稳定复现“尚无输入、reload仍为intent”。测试改为在输入持久化后的可控观察超时点停止，保留原请求、原期限和不重放断言；20轮通过。产品输入/恢复逻辑未改。
- 新增SQL索引与全扫描10组独立对照、嵌套CTE绑定回归；前向CTE候选经固定DuckDB原生执行排除。
- 之前未提交修复继续保留：CLI必填/条件schema、live bugs计数、Raidbots字节预算、source list与doctor显式项目选择、JSONL超时分类、目标选择输入分类、Addon输出上限与错误保留、npm布尔参数分流。具体行为见Changelog。

## 最终验证

- 分页修复及3.2.8版本同步后，本地build/vet、Publish与六客户端Lua共606通过事件/5skip/0失败/exit0，Node99通过/1个缺少固定LuaLS archive的skip，Skill/版本合同及39个文档链接检查通过。完整Lua必需Go套件由本次发行CI验证；下列35包及14包结果属于分页修复之前证据。
- 2026-11-05追加复查：当前源码CLI构建、全仓vet与852文件导入合同通过；14个相关Go包全新运行，2023条通过事件、28项外部材料/权限或helper skip、0失败、两批exit0且stderr为空，含六客户端Lua。Node共100项，97项首轮通过、1项缺少固定LuaLS archive跳过；两项构建集成因调用时遗漏固定CC而失败，改用GCC14.2后各自通过，合计99通过/1skip。不是本次重跑完整35包套件；下列冻结完整套件属于前轮证据。
- 每批代码build/vet通过。最终完整Go/Lua套件固定GCC14.2/CGO1、Lua5.1必需，`go test -json -p=2 -parallel=4 -count=1 -timeout=0 ./...`，35测试包、3495条通过事件、48项skip、0失败、exit0，另有3个无测试包，stderr为空。
- 最终运行前冻结1074个仓库文件；退出后文件清单与全部SHA256一致。十项关键回归均有pass终态，六客户端Lua全部通过。此前中断、测试名交叉编译和时序失败均由本次最终冻结套件替代，不计作通过证据。
- 48项skip为38项环境材料/权限限制与10个helper入口，包括真实LuaLS/DBCD/CascLib/安装样本、客户端与跨卷/链接环境；实机未扩展。
- 新增4组fuzz（encoding物理页、稀疏行、字符串引用、DBD元数据）各20秒/2 worker通过；852个Go文件与交付脚本导入合同无违规；版本检查3.2.7。CLI/Skill目录本轮未改，原351命令合同与702次原生help/schema证据归前轮。
- 前轮Node全套98通过/1平台skip及后续npm入口/隔离更新/恢复/锁11项通过，本轮未重复未改动的Node检查。基线真实Data Retail 12.1.5.70688/zhCN与固定Source调查的范围继续保留在整改计划和todo中。

## 收尾与后续

- 上轮`.tmp/cli-loop`、`.tmp/repo-loop`已清理。本轮`.tmp/deep-loop`已在最终验证后按绝对路径、链接与文件白名单脚本清理；可复用回归留在原测试目录。
- 当前无待修确认项；新PR回放补充确认了原追加审计未覆盖的分页占位场景，现已修复。无关`.zcodeignore`删除已单路径stash保全，身份记录在`.tmp/release-328/retained-stash.txt`；下一步提交推送固定源码、以tag发行3.2.8，等待发行CI与registry读回，然后显式更新本机CLI/Skill，最后恢复该删除。`.tmp/release-328`及`.tmp/blackbox-pagination`成功后脚本清理，完整Live项目保留。正式Forever实机已完成，完整产品矩阵、冷CDN和规模/性能仍列todo。
- ModUs图片字节无读回端点，图片修改保留待核验；未知storage登记缺少查询端点时保持pending、不重放。NewBee Creator缺少稳定账号ID，原会话绑定有效token摘要，token刷新暂停原pending续写。
