# Lychee Dev Toolkit 2.0 实施路线

状态：设计阶段工作分解，未实施。日期：2026-09-21。

架构与命名以 [design.md](design.md) 为准，验收以 [regression.md](regression.md) 为准，Windows CI 与 npm 发布以 [release-2.0.0.md](release-2.0.0.md) 为准。本次正式版本固定为 `2.0.0`、tag 为 `v2.0.0`。本文只规划实现与交付，不授权当前执行发布、删除旧数据或操作正在运行的游戏。

## 1. 能力迁入清单

迁入的是经过核对的能力、算法和样本，不继承旧产品的公共或内部接口。下表中的旧名称仅用于追踪来源。

| 当前来源 | 目标职责 | 处理方式 | 验收关联 |
| --- | --- | --- | --- |
| wowdoc catalog、版本/分支解析 | selection.Pinner、统一 catalog | 重写选择与错误语义；映射有来源；不静默 latest fallback | SEL-01–09 |
| wowdoc Git、对象 pack、SQLite/FTS | codebase 内部与 vault | 保留去重/索引算法，替换 home 和锁接口；新命名后重接 | SRC-01–07、CON-05–07 |
| wowdoc query/explore/inspect/diff | codebase.Browser、Comparer | 用新类型返回结果，CLI 统一渲染 | SRC-01、02、07 |
| wowdoc TOC/XML validate、matrix | codebase.Checker | 保留有序闭包和未决语义；统一 source validate --matrix | SRC-03、04、08 |
| wowdata CASC、BLTE、DB2、DBD、TACT | records 的格式实现 | 保留已验证格式知识及算法；去除旧 root/config/runtime 装配 | DAT-01–03、09 |
| wowdata SQL | records.Reader | 维持只读、有界与流式能力；新请求和错误类型 | DAT-04、05 |
| wowdata Hotfix | changes.Feed | 独立提供方和快照，保留原始与解码证据 | DAT-06、07 |
| wowdata spell/item/creature/encounter/decor | records.Navigator | 新领域请求及关系输出，去除命令层业务拼装 | DAT-08、AST-04 |
| wowdata file/icon/BLP/video | assets.Exporter | 统一文件、图像和视频资产语义；保留解码能力 | AST-01–04 |
| 三套 profile/home/cache/doctor | selection、vault、delivery 和统一 CLI | 重写，仅一个生命周期；旧配置不转换 | STO、CLI、PKG |
| JS 与 Python 客户端/实例解析 | selection、bridge.Binder、desktop | Go 单一实现，进程启动身份和角色复核 | SEL-01–03、10、WIN-04 |
| Python 后台消息输入 | desktop 原生实现 | Go 重写，保留 Win32 语义和有界发送，新函数名 | WIN-01–04、07 |
| Python WGC/NumPy/QR 链 | desktop 捕获与 bridge 信号接收 | Go 重写；纯 Go 解码候选先验证，正式链路无 Python | WIN-05、06、08、09 |
| Python SV、registry、reload、session | bridge、evidence、jobs、vault | 重写受限解析、所有权、持久化状态机和完整收尾 | STO-07、08、RUN、CON |
| Lua 执行、对象、事件、追踪、诊断、导出 | 新私有 Lua 模块及 CaptureWriter | 能力覆盖、新 namespace/协议/命名；维持 UI 规范 | LUA-01–08 |
| 三个独立 skill | skills/lycheedev | 重新编写一个入口及按需工作流，机械编排下沉到 Go | SKL-01–11 |
| 旧 npm 安装/启动/更新 | delivery 与薄 npm 平台包装 | 去除业务和 Python 安装逻辑，统一发行清单 | PKG-01–08 |
| 开发 golden、benchmark、临时 Python 校验 | tests、Go tools/check | 固定样本与语义断言；开发工具不暴露为产品命令 | ARC、PERF、全部 suite |

实现前将每一条实际旧命令和旗标展开成带状态的清单，归类为“新能力已覆盖”“被统一生命周期替代”“有意改变且已定义新行为”。发布时不得有“未归类”。与旧数据兼容、前台输入兜底、历史遗留传输等不自动成为新功能需求。

## 2. S0：固定行为与架构合同

交付：

- 三仓库基线 Commit、现有测试执行结果、已知失败和差异清单。
- 旧能力逐项覆盖表和原始样本来源清单。
- 新命令目录、结果/信号/capture schema、产品目录及命名清单。
- 工作空间存储所有权、并发锁顺序和资源预算策略。
- 原始性能基准和冻结后的回归门槛。
- 许可证及代码来源核对结果，确定可发行方案。

门槛：不把未验证旧行为当契约；命令和数据语义没有多套重复定义。旧测试失败单独记录、处理，不通过改 expected output 掩盖。

## 3. S1：Go 原生通路与新协议验证

优先验证最可能影响技术选型的部分，限制在一个可回收的技术试验路径。

- 原生窗口识别、PostMessage UTF-16 输入、WGC 生命周期和 ROI 获取。
- 一组真实 QR 样本及纯 Go 解码对比。
- Go 受限 SV 解析器与相同字节的跨语言校验。
- 最小配套 Lua 新协议，完成探针提交、报告写盘、ACK 和清理。
- 验证宿主退出后的同 operation 恢复不重复执行。

门槛：WIN-01–09、RUN-01/04/05/07/08 的最小验证路径可复现；没有 Python helper；构建方式和系统依赖已明确。只通过模拟器不足以通过本阶段。实机操作在实施任务的授权范围内执行，此设计阶段不执行。

该阶段的试验代码不会成为永久的第二套执行内核；通过后转入正式 bridge 接口并替换试验入口。

## 4. S2：统一基础设施

- 建立新 module、command/actions 装配、selection.Pinner 和统一 catalog。
- 实现 workspace 标记、短事务、不可变对象、原子发布、跨进程锁和引用保护。
- 实现 jobs.Book、资源准入、结果渲染与 evidence.Archive。
- 实现干净初始化、显式旧根归档计划与新格式拒绝降级写入。
- 建立 CLI 进程测试、伪 CDN/Hotfix、固定 Git 仓库和协议 adapter。

门槛：ARC、CLI、SEL 的离线部分、STO 和 CON 的持久化部分通过。示例命令走真实新内核，不代理旧程序。

## 5. S3：源码、记录与资产模块

codebase、records/changes、assets 可以在 S2 的契约稳定后并行实施，但不得各自再引入工作空间、profile、网络预算或输出协议。

- 将源码解析、索引和 TOC 检查接到新接口。
- 将数据格式解析、只读 SQL、领域查询和独立 Hotfix 接到新接口。
- 将文件、图标、图片和视频处理统一到资产模块。
- 以固定原始数据对照新接口结果，建立独立小数据集 oracle。
- 完成新查询与 evidence 的来源关联、流式结果和取消处理。

门槛：SRC、DAT、AST 通过；无旧 CLI 依赖；跨模块使用同一 PinnedSet；冷/热性能达到冻结门槛。

## 6. S4：正式游戏执行端与并发

- S1 已验证的原生实现接入 bridge.Executor。
- 新 Lua 私有模块、机器入口、队列与 LycheeToolkitDB 落地。
- 角色/Build/代码摘要校验、输入资格、加载、落盘、ACK 和清理进入状态机。
- 多窗口共享安装队列、同窗口排他、恢复 CAS 和进程死亡处理完成。
- 将原 UI 能力与新数据协议接通，保持懒加载和客户端差异集中处理。

门槛：RUN、WIN、CON 全部对应测试通过；四端离线矩阵通过；至少完成一个受支持客户端全链路实测，再逐一完成其余客户端。实机缺失单列，不用单客户端替代四端。

## 7. S5：统一 skill、安装和产品闭环

- 重写根 skill 和六类工作流，删除脚本依赖与重复产品目录。
- describe、帮助、参考文档从命令定义生成并交叉验证。
- delivery 安装器、单一零运行依赖 npm 包、平台原生压缩包、发行资源 manifest 和版本匹配落地。
- Windows required jobs、实际 tgz 隔离安装和新 workflow 的 OIDC 绑定准备完成；npm 包体积达到冻结门槛。
- 用户从空工作空间开始，完成源码/数据/实时联合调查并导出完整证据包。
- 模拟 Agent 上下文中断、两个 Agent 并行以及错误结果的准确报告。

门槛：SKL、PKG 通过；原生包无需 Python/Node；npm 包只有分发包装。旧工具不安装也能完成端到端流程。

## 8. S6：切换与发布验收

- 删除已经被替代的旧实现、旧内部结构测试、旧脚本和旧协议入口。
- 更新 AGENTS、双语 README、构建/发行脚本与支持矩阵，确保它们描述新产品。
- 完成命名/依赖/产物审计，不保留 shim、旧函数 wrapper 或另一套配置。
- 对最终发行 Commit 运行完整回归，执行四客户端实机验证并记录实际 Build。
- fresh 安装、旧根隔离、新数据持久化和未来 schema 拒绝降级检查完成。
- 正式版本源、npm/lock、四 TOC、CLI、skill/resource manifest 统一为 2.0.0，准确 tag 为 v2.0.0；RC 使用 2.0.0-rc.N 与 next，不提前占用正式版本。
- 完成 Windows CI、四端实机和实际 tgz 验收，封存同一 Commit 的产物及摘要。发布时消费同一份 tgz，通过 OIDC 发布并回读 registry 验证；发布本身另按授权执行。

门槛：regression.md 的发布条件全部满足。旧二进制仅可保留在隔离测试基线或归档中，正式产品不依赖它们。

## 9. 工作拆分与协作规则

| 工作包 | 主范围 | 依赖 | 可并行范围 |
| --- | --- | --- | --- |
| 基础契约与命名 | protocol、catalog、selection、command | S0 | 原生能力试验 |
| 存储与调度 | vault、jobs、evidence | 基础合同 | 源码和数据模块的纯解析部分 |
| 源码研究 | codebase | 固定引用与存储接口 | records/changes、assets |
| 游戏数据 | records、changes | 固定引用与资源预算 | codebase |
| 资产输出 | assets | records 文件读取接口 | source、bridge |
| 游戏桥接 | bridge、desktop、Lua | 原生试验、jobs/evidence | 静态能力与安装 |
| 交付与编排 | delivery、npm、skills | command/protocol 稳定 | 各模块后期验证 |
| 回归与实机 | tests、tools | 随各阶段递增 | 所有工作包，但同一游戏窗口排他 |

公共契约变更需要同步依赖方和测试样本。同一文件或同一游戏窗口由一个明确所有者修改/操作；Git 工作可以使用独立 worktree，运行时隔离仍由 Toolkit 的并发模型保证。

## 10. 风险与决策点

| 决策点 | 当前方案 | 关闭条件 |
| --- | --- | --- |
| WGC/WinRT 的 Go 绑定与资源释放 | 原生调用，纯 Go 发布优先 | S1 实机、泄漏与打包证据 |
| QR 识别效果 | 纯 Go 候选，以真实样本决定 | 同样本识别与性能门槛通过 |
| 编辑框/输入资格 | 未确认就不提交，不能把 Escape 入队视为已复位 | 新协议与实机编辑状态用例通过 |
| 源码与游戏版本对应 | 分开固定，显式匹配等级 | 统一目录与不匹配用例通过 |
| 资源预算的跨进程实现 | 工作空间准入与短事务 | 四进程压力及取消/退出测试通过 |
| 探针不可抢占与发送结果不确定 | 保留 unresolved，按证据恢复 | 故障注入不重复执行 |
| 旧数据隔离 | 新格式/新 namespace，显式 fresh 归档 | 哨兵与崩溃恢复用例通过 |
| 许可与来源 | 发布前统一核对，保留上游署名 | 发行 manifest 和许可清单完成 |

这些决策点有明确验证顺序，不以“完美整合”代替验收。阶段 S1 或持久化并发测试未通过时，后续完成的静态能力也不能被描述为整套 Toolkit 已完成。

## 11. 完成交付物

最终交付包括新 Go 源码与配套 Lua、新命令/schema/产品目录、全新工作空间实现、统一 skill、新安装包与发布链路、旧能力覆盖报告、命名审计、回归报告及实际实机证据。

提交设计文档、形成目录结构、重命名函数、单独通过 go test 或能启动一个命令，都只是阶段产物。整套交付的判定以一个干净环境中的完整开发工作流及其故障恢复、并发和四端结果为准。
