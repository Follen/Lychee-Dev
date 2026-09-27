# Source vNext：面向 Agent 的 WoW 源码研究工具

2026-09-27。状态：方案已接受，工作区实现与本地验收完成；尚未发布，生产 CI 和真机调查未运行。

本文替代此前的 source 增量接入、SQLite 索引及以本地插件检查为中心的方案。
新增命令在实现与验证同批进入工作区的 describe 和 skill 命令参考；
本文件中的其他目标在完成前仍只是计划。

本方案覆盖 source 重构、CLI、skill 编排、live 深度调查的衔接和单包分发。
实施时分批同步 design/status/regression；本文的规划不代表已发布能力或已通过验收。

阅读顺序：产品与场景见 1–3；存储/映射/秘密值见 4–6；CLI 与性能见 7–8；
代码目录与隔离见 9–11；npm 与 skill 见 12–13；迁移验收及当前状态见 14–15。

## 1. source 到底做什么

**把暴雪 UI/API 源码和常用第三方插件的 Git 仓库拉到本地，固定版本，建立语义映射，
包装成 Agent 容易使用的 CLI。** Agent 可以查定义、看用法、找引用、追实现、比较版本，
不用自己反复 clone、grep、翻文件、猜符号关系。

暴雪源码通过仓库目录中已配置的源码镜像取得，记录镜像来源，不把镜像作者冒称为
暴雪。第三方范围以现有 catalog 为基线：ElvUI、WeakAuras、NDui、EllesmereUI 等。
每个仓库自己的 branch/tag/commit 与 WoW 客户端版本分开记录。

本地 addon 验证是已有的辅助业务，继续支持；它不决定整个 source 的存储和命令架构。
Source 不执行目标 addon，不接管游戏实时操作，也不作为 data 查询的前置步骤。

确定的技术选择：

- Git 保存源码及版本，detached worktree 提供对应 commit 的普通文件目录。
- LuaLS 提供语言语义能力，Toolkit 补齐 WoW 的 API、事件、TOC/XML 和环境知识。
- 移除 source 自有 SQLite 索引，不再设计 source SQL。
- 用有界 JSON/JSONL 保存可重建的符号映射、关系及工具结果，不另造数据库。
- 现有 data SQL、公共 metadata 数据库、live 和 evidence 合同保留。
- npm 与 native 包携带固定版 LuaLS，用户无需另装语言服务器。

## 2. 从 Agent 的问题决定能力

| Agent 想知道什么 | source 返回什么 |
| --- | --- |
| 这个 API 是什么、怎么用 | 固定版本的声明、参数/返回值、约束、原文和有依据的调用示例 |
| 这个函数在哪里实现 | 准确文件、范围、作用域和定义；同名候选分开列出 |
| 谁用了它、它调用了谁 | 调用点和引用位置，区分语义已确认、静态候选与无法确定 |
| WeakAuras/ElvUI 的某个功能怎么做 | 入口、关键函数、事件、相关代码及加载来源组成的有界上下文 |
| 某个事件/模板/Mixin 与哪些代码相关 | 事件注册及处理、XML 继承和脚本、Mixin 关联及其依据 |
| 两个版本有什么变化 | 文件和符号变化、API 合同变化；单纯移动位置与行为相关变化分开 |
| 还有哪些类似实现 | 在选定仓库分别检索，按仓库和固定版本分组，不混合其全局环境 |
| 秘密值怎样传到报错位置 | 版本规则、可追踪的值传播路径、分支条件和无法继续的位置 |
| 本地插件静态检查 | 保留原 TOC/语法/兼容性和 matrix 业务，使用公共基础能力 |

产品目标是减少 Agent 的调用次数和无关代码阅读，增加可核实的关系与上下文。
不以命令数量、缓存文件数量或生成报告的长度作为完成标准。

## 3. 整体执行流程

```text
仓库目录：暴雪 UI/API 镜像 + 常用插件
                    │
         Git 获取并固定具体 commit
                    │
      detached worktree：按需准备、自动复用
                    │
        文件/TOC/XML/API 结构提取
                    │
        对应仓库 + 客户端的语义环境
                    │
      LuaLS 定义/引用/类型解析与诊断
                    │
       符号映射、关系、上下文及来源
                    │
    query / inspect / refs / context / diff
                    │
       Agent 拿到有界结果与可核实证据
```

不是每个请求都走完整流程。正文检索和原文读取直接使用固定 Git 内容；结构化查询
复用文件事实；只有需要语义关系时才启动 LuaLS。CLI 自动准备缺少的内部产物，
不会要求 Agent 手工调用 Git、导出源码、配置或管理语言服务器。

“做好语义映射”分两部分：先为该版本建立可枚举的文件、符号、API 和加载映射；
再按问题解析并缓存具体符号的语义关系。标准 LSP 不等于可直接导出的完整调用图，
不能声称一次启动 LuaLS 就得到所有动态关系。每类映射都有覆盖范围和来源。

## 4. Git 与 worktree 的管理

每个仓库只有一个受管 bare 对象库。已选择的 commit 用 refs/pins 保留；每个正在
使用的 commit 按需建立 detached worktree，不重复 clone 整仓库，也不随分支移动。
现有受管 bare 镜像可以复用，保留旧 PIN 和已经下载的固定对象。

工作树的身份是 repository + exact commit；语义结果的身份还包括环境和工具版本。
相同文件版本可以共用工作树，但 Retail/Classic 等客户端环境不得共用错误的分析。

生命周期：

1. 解析显式 PIN 或项目锁。仅 sync/明确选择新版本时解析 tag/branch，不偷偷改旧版本。
2. 已有 commit 就离线复用；缺失时按授权和网络选项获取那个版本。
3. 在仓库锁下创建/登记 worktree，完成文件验证后发布 ready manifest。
4. 使用期间持有租约；日志、配置、声明、缓存放在工作树之外。
5. 结束后释放租约，保留热版本；空间预算触发时只回收未使用的可重建工作树。
6. 使用 Git 的生命周期操作维护关联信息，不直接删除或改名已登记目录。

worktree 共享 Git 对象，但每个展开版本仍占磁盘。source 设置独立空间预算，避免
版本越查越多。回收工作树不删除 Git pin、已有 capture 或其他业务的缓存。

**固定 commit 不等于目录不可改。** 复用和分析时检查身份、状态以及实际输入摘要；
发现变化返回明确原因，不把被改动的目录当成原版源码。正文证据优先读取固定 Git
对象。新 checkout 路径需继续隔离 hooks/外部配置/可执行 filters，验证换行与编码
转换，记录 Git 原文和实际分析文件的映射，不能仅靠 git status 宣称字节一致。
Submodule/LFS/链接文件不隐式执行或下载；缺失内容明确影响覆盖。

对本地未提交插件，使用单独的冻结 TOC 闭包；worktree 不能冒充用户尚未提交的内容。
不替用户 commit/stash，不修改其仓库或游戏插件安装目录。

## 5. 用户机器上的目录

继续使用现有 --home。source 的新增目录完全在自己的命名空间下。

```text
<home>/
  mirrors/
    <catalog-key>.git/                 现有受管 bare 仓库，按原位置复用
    ...                               其他业务镜像仍由原业务管理

  source/
    v1/
      worktrees/<repo-id>/<commit>/    detached worktree，源码文件目录
      manifests/<repo-id>/<commit>.json
                                      工作树身份、内容清单、准备状态
      facts/
        files/<parser>/<digest>.json  单文件事实，按内容复用
        snapshots/<mapping-key>/      排序的文件/符号/加载映射 JSONL
      environments/<environment-key>/
        manifest.json                 客户端/依赖/生成器身份与覆盖
        api.d.lua                     给 LuaLS 的客户端声明
        facts.jsonl                   相同来源的结构化 API/事件/XML 事实
      semantics/<analysis-key>/       已解析的语义关系、查询产物和结果清单
      local/<input-digest>/            仅本地验证需要的冻结闭包
      tmp/<run-id>/                   独占配置、日志、临时分析视图

  locks/                              公共 OS 锁，source:v1:* 资源名隔离
  state/toolkit.sqlite                公共 pin/evidence/live metadata，保留
  blobs/                              原有对象与已归档证据，保留
  cache/                              现有 data/公共内容缓存，保持
  indexes/source-*.sqlite             旧 source 索引，新内核不再使用
```

repo-id 绑定规范化仓库身份，避免短名称相同造成串库。mapping-key 绑定输入文件和
提取器；environment-key 绑定客户端声明和依赖；analysis-key 绑定上述身份、LuaLS
版本、配置及请求能力。缓存中断只留下临时产物，完整结果原子发布后才可复用。

源码以 Git 为主存储，不再为普通检索将整个仓库另存一份 blobs/SQLite。派生 JSON
是有界缓存，不做任意 SQL、不把巨大仓库一次性读进内存、不自建通用搜索数据库。
路径移动需要重新绑定源位置，不能仅凭相同内容摘要返回旧文件名。

## 6. 语义映射具体保存什么

| 映射 | 主要来源 | 提供的价值 |
| --- | --- | --- |
| 文件、目录、内容摘要、版本 | Git | 准确原文与版本、变化范围 |
| 符号、作用域、声明范围、类型线索 | 静态提取 + LuaLS | 区分同名函数、局部变量与成员 |
| 定义、引用、调用点 | LuaLS 解析 + 结构化调用位置 | 顺着代码追实现；未确定部分保留候选 |
| API 参数、返回、结构体、枚举、回调、约束 | 固定 Blizzard 元数据 | 查文档、补语言环境、比较 API 合同 |
| TOC/XML 加载、事件、模板、Mixin | Toolkit 的 WoW 提取和规则 | 补齐普通 Lua 语言服务器没有的业务关系 |
| 来源、范围和覆盖 | 输入清单及工具输出 | 每条结论能回到原始文件和版本 |

符号 ID 绑定仓库、版本、文件和作用域，不能只按函数名称关联。跨版本符号对应
关系另做匹配，不凭同名或同一行号断言是同一个实现。

每条关系包含原始位置、目标身份、推导依据和状态：已解析、静态候选、未确定。
类型推断不是运行态证明；字符串提到 API 不等于调用；事件名称相同不证明某个
回调一定被执行。去重保留多个来源，不把冲突结果静默覆盖。

第三方插件默认各自独立分析，依赖库按该仓库真实声明加入。跨仓库查询是分别检索
后聚合结果，不把 ElvUI、WeakAuras、NDui 的同名全局塞进同一个 LuaLS 工作空间。
第三方仓库 main 不等于 Retail 版本；需要客户端声明时使用明确的环境身份，缺失
时照常允许源码检索，并标注语义环境不足。

WoW 元数据只解析一次，同时生成结构化事实和 LuaLS definition files。保留未知
字段、条件和原文，不以 primitive-only stub 或大范围 any/global 白名单冒充覆盖。
XML 内联 Lua 需要虚拟文档和源位置映射。TOC 顺序、Mixin 动态组合、Secret Value
及战斗/taint 限制只能在依据允许的范围内判断，不能被普通类型检查的通过所替代。

### 6.1 秘密值：首版做到有界、可解释的传播分析

推荐目标是**从问题位置追出有证据的值传播链**，服务源码研究和已有验证业务。
基础语义研究先可用，再交付这项增量；不把全仓、全运行态的安全证明作为 source
重构的前置条件。skill 可以立即改善研究行为，自动分析能力仍需要 CLI 实现。

| 层级 | 范围 | 决定 |
| --- | --- | --- |
| 一：版本规则与直接使用 | API 返回/事件参数的秘密值标记、条件、参数约束及直接操作 | 首版必做，供 inspect/context 和验证共用 |
| 二：有界传播路径 | 函数内赋值、分支、静态字段、参数/返回，以及可唯一解析的普通跨文件调用 | 首版目标，逐步验收；预算之外明确未知 |
| 三：运行态深度调查 | 动态回调、实际秘密值状态、事件/战斗条件及安全污染的候选原因 | skill 编排 source 与 live 验证；不承诺还原任意完整动态因果链 |

LuaLS 提供定义、引用及类型线索；Toolkit 自己解释 WoW 规则、控制流和值的传播。
调用图上的相邻函数不等于某个值确实从一个传到另一个。复用现有 Lua AST 解析
能力，先检验语法/位置覆盖，不通过正则拼接传播链，也不依赖 LuaLS 私有 AST。

**版本规则是证据，不是凭客户端名字套模板。** 规则身份包含客户端、API 源码
commit、元数据摘要和规则版本；第三方 addon 还需记录自己的 commit 与加载闭包。
保留 SecretReturns、SecretPayloads、SecretArguments 及条件字段的原文。条件尚不
清楚时标记“可能为秘密值”，不能推导每次调用都返回秘密值或任意操作都违规。

已核对的生成文档中，Retail 31c7f7b9… 与 Forever 4d5d706b… 的 UnitHealth 均含
SecretReturns 和 SecretArguments。两端分别建环境，不套用“旧客户端没有秘密值”
的假设；这只证明这些固定源码含相应声明，不代表 Forever 运行态已经验收。

第二层按错误位置、符号或选定范围按需分析：

- 追踪具体的返回槽位/参数/值：局部赋值、可解析别名、常量表字段及普通函数的
  参数与返回摘要；跨文件边必须有唯一目标、加载依据和实参/形参对应。
- 分支合并保留条件。保护判断只约束相应路径上未被改写的同一个值；检查 x 不
  证明 y 安全，也不把一次 issecretvalue 调用视为清除秘密标记。
- 只在有对应版本规则的操作或参数位置报告限制；仅有一个秘密值来源不足以判错。
  被调用方明确允许的传递与实际违反的限制分开，未知约束保持未知。
- 动态键、未知别名写入、metatable、运行态选出的回调，以及不受支持的递归/循环
  精度均形成显式分析边界。能保守合并时保留可能性，不能分析时不继续伪造路径。
- 每次分析限制文件/函数数量、调用深度、路径、时间和输出；缓存身份包含输入、
  环境、规则和预算。缺依赖、未扫描、超时和输出截断分别报告，零发现不表示安全。

结果逐步给出来源 → 赋值/字段 → 参数/返回 → 使用位置，每步带文件、行号、依据、
条件和置信状态。区分“在已列前提下违反规则”“可能违反”“分析未覆盖”；CLI
成功完成一次有界查询与代码在运行态安全是两回事，不复用 complete 表示安全。

**秘密值传播与 WoW 的 secure taint 分开建模。** 前者讨论值能否被某个操作使用；
后者涉及执行环境、受保护对象与运行时状态。首版对后者只列可核实的写入/hook
位置和候选关系，不根据 Blizzard 栈帧、一次 LuaLS 通过或静态调用链断言污染根因。

深度分析由 skill 把这些线索交给 live：利用已有报告和错误记录，再用有界探针
验证能安全观察的值状态、事件和运行条件，把结果带回源码解释。source 提供路径
与假设，live 提供实际观察，skill 决定下一项验证与何时完成；不在 source 内另建
动态执行引擎，也不改 live 的会话、队列和报告生命周期。静态边界是交接点，不能
成为已授权深度调查的默认终点；live 看不到的内部状态仍明确保留为未知。

验收样本包含：直接使用、两文件普通包装函数、返回槽位区分、同名函数不串线、
条件性来源、有效保护分支、保护另一个值/保护后重赋值、允许秘密值的参数、未知
动态调用、预算耗尽，以及“静态无发现但运行态报安全污染”的证据不足情况。
Retail/Forever 的固定文档分别验证提取；Forever 仍不进入完整客户端验收矩阵。

## 7. CLI：少量任务入口，明确结果

保留原有七个 source 命令，新增两个直接服务语义研究的入口。

| 命令 | 职责 |
| --- | --- |
| source list | 仓库、轨道、固定快照及本地准备状态 |
| source sync | 固定所选版本，取得 Git 对象，后续查询自动补齐必要准备 |
| source index | 显式预热该版本的结构映射缓存；不再生成 SQLite |
| source query | 按名称、正文、路径和 topic 找候选，保留 precise/exploratory |
| source inspect | 看原文或具体符号的定义、声明、类型和出处 |
| source refs（新增） | 查具体符号的引用、调用者/被调用者及明确的关系类别 |
| source context（新增） | 将一个问题对象的定义、关键代码、直接关系和加载依据组合为有界上下文 |
| source diff | 比较两个固定版本的文件、符号与 API 合同，可进一步定位用法影响 |
| source validate | 保留本地插件单 TOC 与 matrix 的静态验证业务 |

不新增 source SQL，也不把本地 analyze/修复任务作为 source 的主入口。此前未发布
的 --semantic 原型重新评估；若保留深度验证能力，作为 validate 的明确检查范围，
复用研究内核，不建立第二套环境和事实模型。

秘密值规则随 inspect/context 提供；有界传播结果复用 context，批量本地检查复用
validate。是否请求深度分析由明确能力范围控制，普通源码查询不强制跑流分析。
暂不另起 taint 命令组；具体参数须随实现进入唯一合同，不能先写入发行 skill。

已有参数和业务语义尽量兼容；新增 refs/context 优先接收 query 返回的稳定符号 ID，
避免 Agent 重新拼名称。多个候选分别返回，不能随意挑选第一个。具体参数在实现时
与合同表、解析器、help/describe、输出 schema 和 skill 同步交付。

结果至少包含：固定来源、命中的位置/原文、关系状态、覆盖、截断/下一页以及 capture。
context 按字节、代码行、关系深度预算输出，不默认倾倒整个仓库。显示原仓库路径，
内部 worktree/虚拟文件位置通过映射还原；UTF-16 与字节位置不能混用。

语义结果还需给出使用的 API 环境/依赖、工具与规则版本、同名候选、未知关系原因
和分析预算。引用结果的状态逐条记录，不能只给一个总置信分数掩盖中间断链。
查询可携带足够身份让 Agent 继续研究，不强迫它从一大段自然语言中重建下一次请求。

失败时按实际能力保留结果：LuaLS 缺失时正文/结构检索仍可用；解析失败的文件保留
原文与诊断；有缓存但离线时只用已固定对象；没有精确版本时明确缺失，不换最新。
语义请求未完成就标出未完成范围，不能把降级成功误报为语义分析完成。

旧 source index/status 暴露的 database/journalMode 字段在新存储下不再适用，要在
新版 source 结果 schema 中明确 storage=file-cache。公共 envelope/退出码/JSONL
完整结束规则保持，不能把文件目录填进 database 字段假装完全同构。

## 8. 检索与性能设计

- 原文、清单和文件差异利用 Git 的成熟能力。正文检索使用固定 commit 的 git grep
  或受控文件匹配，不因结构解析失败而彻底丧失原文查询能力。
- 符号/API/事件/模板等从小型结构缓存过滤和排序，保持已有 topic、排序依据和来源。
- LuaLS 按需启动，同一研究任务复用会话。定义/引用/hover 使用公开 LSP 和实际能力
  协商；不依赖私有 AST、VM 或 worker 参数，不建常驻 daemon。
- 已确认结果按完整环境身份缓存；同一问题再次查询可直接复用。新 commit 复用
  不变文件的提取结果，跨文件语义先以完整环境失效保证正确性。
- 批处理诊断需要可判定的结束条件；没有诊断到达不代表已完成。协议输出、时间、
  文件规模都有界，超时/取消清理整个子进程树。
- 分页绑定固定映射或已保存的结果，不在下一页重新解析移动分支。未扫描部分、
  工具不支持和输出截断分别报告，不能都混成“没找到”。
- 去掉 SQLite 不是速度证明。用相同固定仓库测冷启动、热查询、首次语义解析、
  跨版本复用、内存、磁盘及 Agent 读取量；指标在实现基线测量后设定。

## 9. 仓库代码目录架构

保留 internal/codebase 为 source 唯一业务入口，避免改名牵动其他模块。

```text
internal/
  command/
    command_contract.go               唯一命令目录
    dispatch_source.go                source 参数与结果接线，从 entry.go 收拢
    source_contract_test.go           真实 CLI 合同与兼容测试
    ...                               其他命令保持所属业务

  codebase/
    engine.go                         Prepare / Query / Inspect / Relate / Context / Compare
    request.go / result.go             任务范围、预算、事实、位置、覆盖与输出
    catalog.go                        已支持仓库及轨道目录
    query.go                          候选检索、排序和稳定续页
    inspect.go                        原文与符号详情
    relations.go                      语义解析、关系状态与映射
    flow.go / flow_rules.go            有界值传播、版本规则、条件与未知边界
    context.go                        有界代码上下文组装
    compare.go                        两版本文件/符号/API 合同比较
    environment.go                    WoW 与第三方依赖环境生成
    facts.go                          文件结构与官方元数据提取
    cache.go                          文件缓存身份、原子发布和预算
    validation.go / matrix.go          原有验证业务，复用相同基础能力
    evidence.go                       结果交给公共 evidence 归档
    compatibility.go                  必要输出转换，不含旧分析实现
    gitstore/
      repository.go                   bare 对象库、固定 refs、获取
      worktree.go                     创建、验证、租约、回收
      read.go                         固定 Git 原文、grep、diff
      process.go                      参数/配置隔离、取消与预算
    luals/
      client.go / protocol.go         LSP 生命周期、能力与有界消息
      check.go                        可结束的批量诊断
      process_windows.go              受管进程树和取消

  addoncheck/
    toc.go / xml.go / lua.go           轻量静态读取与语法检查
    closure.go / result.go            加载闭包及基础诊断

  delivery/                           安装与工具载荷验真
  selection/ vault/ evidence/          保持公共身份与存储合同
  records/                            data/SQL/Hotfix/CASC/asset 保持
  live/ bridge/ desktop/               游戏状态机和传输保持

skills/lycheedev/
  SKILL.md                            仍然一个 toolkit skill
  references/source-research.md        查找→定位→关系→上下文→带来源回答
  references/addon-validation.md       保留验证工作流
  references/installation.md           工具随包安装
  references/commands.md               仅从已实现合同生成
  scripts/lycheedev.mjs                仍只定位/转发 CLI

release/tools/luals.json              目标：唯一工具版本/地址/摘要清单
tools/luals.mjs                       发行时获取、校验和封装
packages/npm/lycheedev/               一个包、一个入口
tests/source/                        固定样本、真实进程与 Agent 场景
docs/toolkit/source-vnext.md           本方案
```

以上是职责划分，不要求先创建空目录或每个 helper 一个包。addoncheck 有两个实际
调用者：source 和 delivery；它不能初始化工作空间、下载源码或启动 LuaLS。

依赖方向：

```text
command → codebase → gitstore / luals / addoncheck
command → delivery（取得已验真的工具描述并传入 codebase）
codebase → selection / vault / evidence
delivery → addoncheck
records/live 不依赖 source 的分析流程
下层 gitstore/luals/addoncheck 不反向导入 codebase 或 delivery
```

这避免原型出现过的 codebase → LuaLS → delivery → codebase 循环。

## 10. 与其他功能的隔离合同

| 现有模块/功能 | 这次怎么处理 | 交叉验收 |
| --- | --- | --- |
| data SQL/DB2/Hotfix | 不改查询引擎、数据语义及缓存身份 | 原有数据回归继续通过 |
| asset | 游戏资源仍由 records 寻址/导出；source asset topic 只研究仓库文件 | 来源身份、旧格式与导出结果不变 |
| live/bridge/desktop/addon | 不改会话、二维码、队列、ACK/finish 或窗口所有权 | source 不打开游戏通道，协议夹具不回归 |
| addon 安装 | 将已借用的轻量 TOC/XML 检查提到 addoncheck | 无 Git/LuaLS 时安装仍可验证，收据字节合同保持 |
| selection/project | 沿用 PIN、项目锁及优先级，不篡改旧 pin 内容 | SourcePin 与 DataPin 不混用，旧锁可读 |
| vault | 只移除 source 私有 SQLite 索引，不动 toolkit.sqlite | 公共 metadata/锁/缓存功能保持 |
| evidence | 使用现有 CAP/show/verify/keep/bundle | 工作树回收后证据仍可读/可验证 |
| delivery/npm | 新增私有 LuaLS 载荷，其他安装目标不混入工具文件 | 分组件安装、离线包、中文路径与验真 |
| skill | 更新 source 路由和流程，不重写 data/live 授权与收尾规则 | 对应场景分开验收 |

实际已确认的冲突点：delivery/addon_release.go 调用 codebase 的 AnalyzeDocument
和 InspectLocalLoad；必须先提取轻量检查。vault 的 toolkit.sqlite 保存公共状态，
因此不能删 go.mod 的 SQLite 依赖。records 当前不直接依赖 codebase，无需迁移它。

Git/LuaLS 长任务不占用公共数据库写事务。短暂读取固定身份后结束访问，分析完成
再开短事务归档，避免源码研究拖住 live journal 或 data 的证据提交。
Source 使用独立租约与目录预算；接入 cache 命令时通过显式 source scope 调用自身
回收器，默认公共 cache 语义不变，通用对象清理器不遍历工作树。

## 11. 证据、配置和旧内容兼容

继续使用 --home/--project/--snapshot，明确选择优先级，不新增隐式的上次使用版本。
SourcePin 中旧 parserRevision 不原地改写；派生映射记录实际提取器和语义工具版本。
新配置只加可选 source 区段，不重写用户整个项目文件或改变 data/live 默认值。

查询返回原始文件、范围、摘要及 commit。需要持久证据的内容使用现有归档入口。
复杂研究可封装一个有界、自包含的 source 证据包，将所用原文、环境、映射依据、
配置和结果作为现有 capture 的 payload；不只写未登记的 blob hash 后就声称可重放。
不为一次查询复制整个仓库。报告可阅读、证据可验证、完整任务可离线重跑分别记录
覆盖；超出预算就明确指出，不能把仅有摘要当成完整输入。

旧 source 索引不再读取或更新；新映射从相同 commit 重建。旧 CAP/PIN 继续通过
公共模块读取。升级不自动删除旧数据库；后续清理只针对 source 索引白名单，绝不
删除 toolkit.sqlite、其他索引或共用 blobs。退休工具的数据与 SavedVariables
仍按现有合同不读取、不导入。

## 12. npm 和工具运行目录

```text
安装后的 lycheedev/
  bin/lycheedev.mjs
  native/windows-amd64/lycheedev.exe
  payload/addon/                      游戏插件载荷
  payload/skill/                      skill 载荷
  payload/tool/luals/                  CLI 私用的固定版完整运行目录
  release.json                        所有文件的版本、长度、摘要
  THIRD_PARTY_NOTICES                  对应实际随包组件
```

npm 安装同时获得 LuaLS；首次分析不再下载程序，不需要编辑器或另装 Lua。
Git 沿用现有系统依赖，由 doctor 明确报告。组装发行时获取 LuaLS，固定并校验全部
脚本/DLL/资源及许可；不将上游 MIT 自动解释为所有第三方组件的许可。
工具版本只维护一个权威清单，由生成步骤提供 Go/Node 使用，避免原型双重来源。

addon/skill 安装只 stage 选中的组件，不复制 tool 进目标目录，也避免无谓复制整个
LuaLS。基本命令不启动语言服务器。Windows amd64、CGO_ENABLED=0、版本源、CI、
OIDC、registry read-back 等发行要求保持；不复用已发布 2.0.6 或移动其 tag。

## 13. skill 与 CLI 同批设计、同批验收

按 skill-creator 的原则维护现有 lycheedev skill，不新建一个竞争触发的 source skill，
也不把 CLI 的内部准备流程写成 Agent 需要照做的长操作手册。自动发现继续启用；
agents/openai.yaml 与新版实际能力同步，并保留已有 live 收尾要求。

### 13.1 按问题选业务，而非按名词抢任务

| 用户问题 | skill 选择 | 不需要的准备 |
| --- | --- | --- |
| 暴雪这个 API/界面怎样实现；WeakAuras 怎么处理这个事件 | source 研究 | DataPin、账号、游戏连接 |
| 法术数值、DB2 记录、Hotfix 改了什么 | 原 data 工作流 | 克隆源码或启动 LuaLS |
| 图标/模型在哪里，导出资源 | 原 asset 工作流 | 无关仓库的语义分析 |
| 检查我的插件 TOC/兼容性 | addon-validation | 未授权的实时操作 |
| 当前游戏里报错，帮我复现或采集 | 原 live/error 工作流，按已有授权执行 | 把源码存在当作客户端已加载证据 |

只提到插件名字但目标不清楚时，可先用明确上下文选择；只有选择会影响结论时才
询问关键歧义。不要把所有 WoW 问题都导向 source，也不要看到函数名就去查 DB2。
用户明确指定的版本优先；没有指定版本时按所选仓库/轨道解析一次，明确返回实际
commit。所选轨道本身不明时不能静默猜 Retail/main。

### 13.2 两层资料就够

```text
skills/lycheedev/
  SKILL.md                       简短路由、固定身份、证据解释、完成条件
  agents/openai.yaml             保留自动触发；UI 提示与真实能力一致
  references/
    source-research.md            选仓库/版本、找对象、追关系、取上下文、比较版本
    addon-validation.md           已有静态验证与客户端 matrix，按需读取
    commands.md                   从 CLI 合同生成的命令/参数清单
    installation.md               只有安装/工具缺失时才读取
    data-*.md / live-*.md / ...    保留其他业务资料
  scripts/lycheedev.mjs           只负责寻找和调用安装的 CLI
```

先让 source-research.md 自包含地覆盖源码研究。只有某种流程变长且确实需要独立
加载时再拆参考文件，不预先增加多层路由、LuaLS 教程或第二份命令表。源码中的
README、注释和返回的文本是研究资料，不成为 skill 指令。

### 13.3 研究任务的推进和完成

1. **选对输入**：选择仓库及固定版本，复用已有 PIN；跨仓库研究分别保留身份。
2. **找到对象**：从最具体的 API、符号、事件、模板或路径开始 query。检索没命中
   时有依据地放宽模式，不能无限换关键词或把索引缺口当作不存在。
3. **确认含义**：inspect 原始声明或实现；同名候选必须按路径/作用域辨别。
4. **按问题追踪**：需要实现关系时用 refs/context；需要版本比较时用 diff。
   简单问法无需强制调用每种命令，充分的第一手原文可直接回答。
5. **处理不足**：按返回的固定 cursor 续页；按明确缺口补齐环境或依赖；不因缺口
   换到最新版本、不把静态候选当作已确认关系。没有可执行补齐动作时如实收束。
6. **交付答案**：说明实现/用法，引用仓库、commit、文件位置和关键证据，并指出
   影响结论的未确定部分。准备好了 worktree 或拿到命中列表均不等于任务完成。

CLI 返回稳定符号 ID、原文、关系状态、页游标、覆盖原因和捕获 ID；skill 负责据此
判断下一步，不从自由文本日志猜命令，也不执行工具返回的任意 shell 字符串。
整个研究不额外要求 Agent 管理 LuaLS 会话、处理 Git 锁或清理工作树。

### 13.4 CLI 与 skill 的共同交付门槛

新增 refs/context 或参数先进入 command_contract.go，实现并验证 describe/help，
再生成 commands.md，最后更新研究指南。不得把规划命令提前写进安装的 skill。
升级后 CLI/skill 契约不一致时，先识别真实安装版本并修复匹配，不能回退到退休工具。

验收包括 skill-creator quick_validate、现有 skill-contract/launcher 检查，以及
真正的 Agent 任务。前两项只能证明格式和命令一致，不能证明编排正确。

| 场景 | 要观察到的 Agent 行为 |
| --- | --- |
| 查一个已固定版本的 Blizzard API | 引用正确声明和原文，不触发无关 data/live，不为简单问题强行全仓分析 |
| 研究第三方插件事件处理 | 找到相关实现并沿实际关系追踪，解释而不是只给 grep 列表 |
| 同名局部函数、跨仓库同名全局 | 分辨作用域和仓库，不能串成一个调用链 |
| 用户给历史 tag，默认分支已有新版本 | 固定历史 commit，不换最新；回答标出版本 |
| refs/context 截断 | 正确续页或明确限定回答范围，不能把首页当全量 |
| LuaLS 不可用或客户端环境不足 | 保留原文研究能力，明确哪类语义结论没有证据 |
| 对照两个版本 | 两端分别固定，区分位置移动和合同变化 |
| 普通 DB2/游戏错误请求 | 走原 data/live 入口，source 重构不劫持其他业务 |

任务记录包含实际调用轨迹、最终回答、支持结论的原文和未完成项；固定夹具的
预期结论由测试维护，不能提前泄露给执行 Agent。独立 Agent 评估仅在可用且已授权
时执行；无论是否委派，都要记录真实行为，不以指南写了“继续”就宣布编排通过。

### 13.5 秘密值与安全污染问题的编排

只在用户研究相关 API 限制、秘密值错误或安全污染时进入这一分支。skill 从错误
位置/具体值选研究范围，固定 addon 与客户端 API 的各自来源，然后判断实际限制、
追踪来源和使用位置。用户已有的错误栈作为线索；全是 Blizzard 文件不等于根因
已经定位到 Blizzard，也不等于可以随意指控某个插件。

当前 CLI 尚未提供值传播分析时，使用已有 query/inspect 逐段核实源码，明确结果
是 Agent 基于原文的分析；不能把命中列表叫作工具已验证传播链。未来 CLI 返回
路径后，优先解释其条件与未知边界，只对影响结论的缺口补查，避免重复全仓遍历。

结束时交付：触发限制、支持的路径及源位置、最小修正建议、仍待验证的条件。
若证据不足就明确指出缺哪一段，以及什么证据能区分候选原因；不能以“加了保护”
宣称已修复，也不自动用默认值或强制转换掩盖来源。

用户任务包含运行态深度调查时，skill 主动推进以下闭环，不为每个已授权步骤重复
询问；仅讨论源码或方案不隐式启动游戏调查：

1. **交接线索**：保存源码 commit/位置、疑似传播路径、分支条件与待区分的解释；
   绑定实际会话、客户端 build 和插件版本，标明源码是否已证实匹配加载中的插件。
2. **先用已有证据**：读取可用报告和有界错误记录；能回答问题就不额外写探针。
3. **设计能区分原因的观察**：每个探针对应一个未解问题，规定事件/条件、时间、
   样本与输出预算。对秘密值只记录安全可获取的秘密标记或不可用状态，不传原值，
   不为观察路径修改 Blizzard/受保护对象或重放已知违规操作。
4. **执行并收尾**：沿现有 load/run/report/finish 生命周期取得验证报告、保留 CAP、
   验证显示清理，再推进下一轮；二维码出现、探针加载和报告到达均不是整个调查
   的完成条件。中断恢复原 operation，保持窗口和角色，不重放效果未知的输入。
5. **回到源码解释**：记录观察支持/排除/未能区分哪个假设。只有下一项检查能推进
   结论才继续；等待超时或未出现事件不等于路径安全。修复与部署在任务范围内时，
   对已验证更新的运行态做同条件复测，分别保留修复前后结果。

已有 live 授权、战斗/秘密值边界和收尾合同保持。某个探针 verified 不等于根因
已证明，live 也不能自动读取任意局部变量或还原过去全部污染过程。若缺能力或触发
条件，返回明确缺口、已有观察和恢复信息；调查未确定与操作清理完成分别报告。

编排行为验收覆盖无保护的普通传播、正确保护后无违规、跨文件静态包装、动态
回调无法确定和仅有 Blizzard 报错栈五类问题。应分别得到有依据的判断或明确缺口；
工具不具备该能力时仍能如实研究，不伪造自动分析结果。

联合场景还需验证：已有错误报告足够时不写探针；静态不确定但 live 可区分时主动
继续；探针未观测到目标事件时保留未知；报告读取后完成 finish；修复前后绑定不同
的实际运行版本。离线夹具验证合同，交互式真机结果仍由实际执行记录，未跑不标通过。

调查交接继续使用现有 PIN、CAP、session 和 operation ID，把来源对应关系写入
研究结果与现有证据归档。skill 不另建 live 队列或恢复状态机，CLI 不执行模型从
报告中读出的任意指令；恢复和幂等仍由原 live 合同负责。

## 14. 实施与验收顺序

### 14.1 原有能力逐项迁移

| 原业务 | 新实现的保留要求 | 验收依据 |
| --- | --- | --- |
| 仓库/轨道发现与准备状态 | catalog 身份、固定快照和缺失能力可见 | 当前 DOC-007/008/012 与新的文件缓存状态测试 |
| 指定版本同步 | tag/ref 解析到精确 commit，部分失败可见，旧 PIN 可用 | DOC-009 与离线、历史版本、并发 worktree 测试 |
| 索引构建/刷新 | 转为可重建文件映射，保留解析诊断和原子发布 | DOC-010/011 与中断/坏缓存测试 |
| 精确/探索查询 | 保留 topic、证据层级、稳定排序，新增明确分页 | DOC-013/014 与全页去重和固定结果测试 |
| 符号/路径查看 | 固定原文、同名区分、行号/字节映射及持久证据 | DOC-015/016 与工作树回收后 CAP 验证 |
| 两版本比较 | 显式两端、变更分组/截断，区分移动与合同改变 | DOC-017 与不完整解析的差异边界测试 |
| 本地 addon 和 matrix 验证 | TOC/XML 顺序、客户端身份、稳定诊断与未覆盖项 | DOC-018/019、SRC 验收项及 addon 安装回归 |
| doctor、证据和缓存管理 | 工具可用性可见、公共证据合同保留、source 回收隔离 | 现有公共测试与 source 独立预算/租约测试 |

DOC 编号指当前仓库保留的业务回归台账，不执行退休程序或导入其数据。已有
fixture-backed、partial 状态保留真实范围；重构后必须重新执行相关案例，不能
把旧结果直接继承为新版已通过。新增语义和 live 联合场景另补测试及实际运行记录。

### 14.2 分阶段交付

| 阶段 | 可审查交付 | 退出条件 |
| --- | --- | --- |
| A 原业务基线与隔离 | 固定源代码样本、性能基线、addoncheck 提取 | 原 source/安装合同保持，data 未提交改动完整保留 |
| B Git 工作空间 | bare/worktree 生命周期、文件清单、旧 PIN 复用 | 两 commit 并存、同版本复用、离线、dirty 检测、并发和崩溃恢复 |
| C 结构映射与原命令迁移 | API/符号/加载缓存，query/inspect/diff/validate | SRC-01..08 与现有 source parity 业务逐项通过 |
| D 语义研究 | LuaLS、独立仓库环境、refs/context、语义缓存 | 定义/别名/局部同名/跨文件/第三方库/XML/事件的可核对结果 |
| D2 秘密值研究 | 固定版本规则与有界传播，按 6.1 分批交付 | 规则提取、条件/保护、可解析跨文件路径及未知边界通过人工可核对夹具 |
| E Agent 与分发 | skill、npm、真实 CLI 任务和证据 | 安装后完成真实源码研究；其他业务回归；删除旧 source SQLite 实现 |

验收任务必须来自真实使用：固定版本查 Blizzard API；追踪一个 WeakAuras 或 ElvUI
功能；找某事件的注册和处理；比较两个已固定 commit；同名函数跨仓库不串联；
输出限制后能稳定续页；环境不足时仍能返回正确原文与明确的语义缺口。

真实大仓库用于验证可用性与性能，小型人工可核对夹具用于语义正确性。两者都要有，
不能只看 LuaLS 不报错或把两个工具输出一致当作正确性证明。客户端 API 环境按
仓库固定的 Retail/Classic/Titan 基线验收；Forever 保持未验收状态。

跨功能回归包含：addon 安装无需语义工具；data SQL/Hotfix/CASC 与 source 同时运行；
公共 metadata 不被长任务锁住；source 清理不改其他业务文件；工作树被回收后 CAP
可验证；工具异常和取消没有残留进程；npm 中文/空格路径、离线本地包与验真通过。

每个实现批次执行 build/vet 和受影响测试。交付前完成全量 Go/Lua、命令/skill 合同
及 npm 安装测试。记录 Agent 调用次数、输入/输出字节、耗时和结论依据，证明工具
更好用；不先宣称未经测量的速度倍数。

## 15. 当前状态与资料

工作区实现与本地验收已覆盖：addoncheck 从 source 分离供 delivery 共用；
固定 Git 源、可重建文件映射、查询/续页/查看/差异、refs/context、固定客户端
API 环境、有界 flow、独立 source worktree 回收、随包 LuaLS 和 skill 命令合同。
隔离 npm 安装、包内 LuaLS 3.19.1 场景与真实固定源查询均已运行；Retail、
Classic、Titan 的 `UnitHealth` incoming 分别得到 16、13、13 条 LuaLS
`resolved` 非声明引用，WeakAuras `WeakAuras.Add` 两条外呼得到 resolved，
后者保留 semantic partial 和大文件限制。source prune 在干净隔离 home
回收了两个 verified worktree（88,181,463 字节到 0），归档的 144,090
字节原文件仍可核验，同一 WeakAuras PIN 仍可查询。详细固定 pin、测量、
覆盖和未完成项见 [本轮验收记录](source-vnext-acceptance-2026-09-27.md)。

旧 home 的脏 WeakAuras orphan 保留时，Retail 语义查询仍得到 16 条 resolved；
source prune 只移除 verified Retail worktree，保留脏 orphan 原字节并报告
`complete=false`。最终本地冻结包的完整 Go/Lua、Node、skill 合同、构建与
vet 门禁均通过；生产 CI、发行/发布和 source→live 真机调查为 `not_run`，
Forever 客户端验收仍为 `not_run`。
静态 flow 的 `possible` 和未知操作边界不表示运行态违反或安全。
data 的既有工作区变更需保持原样，不能整体 reset 共享文件；本地工作区
接口和验收记录不等于已发布能力。

依据：当前 codebase/catalog、prepare、git、facts、check、validate、inspect、index
实现；delivery/addon_release.go；selection/pins.go；vault/metadata.go；现有 design、
regression、capability-inventory 和 tests/parity/coverage.json。

- [Git worktree 官方文档](https://git-scm.com/docs/git-worktree)：共享对象库、detached 工作树及生命周期。
- [LuaLS definition files](https://luals.github.io/wiki/definition-files/)：外部环境声明。
- [LuaLS diagnosis report](https://luals.github.io/wiki/diagnosis-report/) 与 [LSP](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)：公开分析接口。
- [Retail 固定 Unit 文档](https://github.com/Gethe/wow-ui-source/blob/31c7f7b9cc79e56c986b365c06a6afbcf3c9177b/Interface/AddOns/Blizzard_APIDocumentationGenerated/UnitDocumentation.lua) 与 [Forever 固定 Unit 文档](https://github.com/Gethe/wow-ui-source/blob/4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069/Interface/AddOns/Blizzard_APIDocumentationGenerated/UnitDocumentation.lua)：秘密值元数据存在性及条件的原始依据。
