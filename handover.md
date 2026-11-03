# 当前交接

更新：2026-11-03，Asia/Shanghai。架构与合同见 [Design.md](Design.md)，本次范围和DuckDB迁移结果见 [整改计划](docs/rectification-plan.md)，后续验收见 [todo.md](todo.md)。

## 当前状态与责任

- 复用main `a8fdb7fc8464f06c1cffbd05e6648e124edcb0e2`，修改全部留在当前工作区。未新建分支/PR、提交、推送、发布或变更npm渠道；保留既有`.zcodeignore`删除和`.worktrees/Rust-refactor`。
- 本轮循环审计、代码/测试清理、文档修正、CI优化和临时清理已完成；既有worker已停写。Cobra/自解释、Skill精简、正式Forever身份、DBCD/Data、Source、五平台Go Publish及CI去重前轮已完成。Data/Source/CLI前轮全新审计，以及DD第11轮、公共层/CF/NewBee第9轮、ModUs/Blackbox第11轮与Publish CLI窄复审均无新增可复核问题。
- 追加DuckDB迁移已完成：官方驱动v2.20000.0、DuckDB2.0.0、bindings/原生库v0.20000.0；流式Data表源与安全单表ID索引接入。旧relational的50个源码/测试文件已删除，无旧方言转换、引擎开关或回退。此次迁移由主Agent实施和验证，未将此前独立审计结论扩展为新引擎独立复审。
- Windows固定GCC14.2/CGO静态构建、完整原生许可和流式源码归档已接入。CLI约140.5MB，含LuaLS的npm包约46.3MB；LuaLS peer核验上限随实际包体调整至256MiB，真实跨进程回归通过。
- SQL-AUD-01已先红后绿：表列别名将普通字段命名为ID时，原索引提示导致等值/IN漏行；现对列重命名交给DuckDB完整绑定。按调用图删去不可达函数，测试便捷入口移入测试或改用生产API；Source重复摘要/包装已删除，历史恢复fixture与解码入口按实际消费者保留。CI移除5个Linux作业的Windows TMP步骤、修正对应过度约束测试，固定download-artifact SHA，原生准备先选择固定Node。最新手工复审与Windows测试调用图检查无新增，最终候选完整套件已通过。

## 实际验证

- 最终Lua5.1必需Go套件：`-p=2 -parallel=4 -count=1 -timeout=0 ./...`，35包、3455条测试通过事件、30项环境/helper skip、0失败、exit0。固定DBCD Hotfix差分、真实LuaLS/打包Source/跨CLI broker及跨盘备份均实际执行。
- 每批代码build/vet和受影响SQL/Data/CLI测试通过；sqlengine race通过。前轮DD race及ModUs/Blackbox定向验证保留，未因文档变化重复运行。
- 98项Node检查通过；后续发行参数变化的44项检查43通过、1项未配置archive skip（该archive用例在前述全套已通过）。最终Skill为6份Markdown、351命令、0违规；导入合同843个Go文件、0违规；actionlint通过，版本仍为3.2.7。
- 新原生CLI在仅系统PATH下启动，并通过SQL/CSV、Hotfix及资源导出；最终npm包隔离安装通过。原生PE依赖为Windows系统DLL，固定工具链prepare实际执行通过，未安装到用户游戏或全局CLI。
- 工作区快照的vendored离线源码重建字节一致，归档约202.4MB。移除含本地库搜索路径的链接build ID，继续核验VCS与产物SHA。范围为工作区快照和固定预编译原生库，非tag发行、非DuckDB C++源码重编译。
- 单ID/35-ID在本地与离线CDN fixture准备链上只供给1/35行。24项合成数据准备链基准通过；100ms采样的整基准进程峰值working set约51MiB/private约75MiB。无真实网络或迁移前后加速比例结论。
- 全仓40份Markdown的182处本地链接及diff空白检查通过；最后文档收尾仅更新当前验收和清理状态。

## 待验、清理与下一步

- 真实账号/平台写入、短信、UAC、游戏操作与云端CI未执行。正式Forever实机、六产品安装矩阵、真实PIN/联网冷CDN、旧基线同输入性能及大输入/多进程资源测量继续列todo。
- ModUs图片字节无读回端点，图片修改保留待核验；未知storage登记无查询端点时保留pending、不重放。NewBee Creator缺少稳定账号ID，原会话绑定有效token摘要，token刷新暂停原pending续写。
- 用户追加要求用脚本清理后，固定清单的PowerShell脚本完成路径、项目与目录链接检查，预览和实际执行均exit0：删除51个已结束任务目录、1个临时状态文件和6个本轮npm隔离目录，清理脚本随后移除。此前清理阻塞已解除。`.tmp/toolchains`为当前开发所需固定编译器缓存，继续保留；活动项目、无关旧材料和Rust worktree未动。
- 最终验证已结束，无运行中的验收进程或未修复的确认项；本轮临时清理后核对工作区和worktree登记，未改动产品代码，未重复完整套件。下一步为用户安排todo中的真实平台、游戏与云端验收；提交、推送和发布继续等待对应授权。
