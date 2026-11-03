# CLI 命令迁移

本轮复用现有业务入口，Cobra 命令树、help、describe、schema 与发现信息均来自 `internal/command/command_contract.go`。每次 Execute 创建独立命令树，参数状态不跨调用保留。

旧命令保持可达；`data table` 是 `data db2` 的同处理器别名。`describe` 保留用于完整目录，`discover <任务>` 返回最多八个候选，`schema <command...>` 返回输入类型、范围、必填条件、示例、影响和结果信封。它们离线运行，不初始化工作区。

| 原命令 | 当前入口 | 处理 |
| --- | --- | --- |
| `update` | `update` | 保留原业务处理器 |
| `project init` | `project init` | 保留原业务处理器 |
| `data hotfix` | `data hotfix` | 保留原业务处理器 |
| `project lock` | `project lock` | 保留原业务处理器 |
| `project status` | `project status` | 保留原业务处理器 |
| `live probe put` | `live probe put` | 保留原业务处理器 |
| `live execute` | `live execute` | 保留原业务处理器 |
| `live probe list` | `live probe list` | 保留原业务处理器 |
| `live probe show` | `live probe show` | 保留原业务处理器 |
| `live probe remove` | `live probe remove` | 保留原业务处理器 |
| `skill install` | `skill install` | 保留原业务处理器 |
| `skill status` | `skill status` | 保留原业务处理器 |
| `skill remove` | `skill remove` | 保留原业务处理器 |
| `addon status` | `addon status` | 保留原业务处理器 |
| `addon install` | `addon install` | 保留原业务处理器 |
| `addon remove` | `addon remove` | 保留原业务处理器 |
| `data sql` | `data sql` | 保留固定 Data/Hotfix 来源，改用 DuckDB 原生方言、`$name` 参数和 EXPLAIN |
| `data db2` | `data db2`；别名 `data table` | 保留原业务处理器 |
| `data db2 schema` | `data db2 schema`；别名 `data table schema` | 保留原业务处理器 |
| `data db2 search` | `data db2 search`；别名 `data table search` | 保留原业务处理器 |
| `data db2 foreign-key` | `data db2 foreign-key`；别名 `data table foreign-key` | 保留原业务处理器 |
| `data db2 stream` | `data db2 stream`；别名 `data table stream` | 保留原业务处理器 |
| `data spell info` | `data spell info` | 保留原业务处理器 |
| `data spell auras` | `data spell auras` | 保留原业务处理器 |
| `data spell summons` | `data spell summons` | 保留原业务处理器 |
| `data item get` | `data item get` | 保留原业务处理器 |
| `data item models` | `data item models` | 保留原业务处理器 |
| `data item geosets` | `data item geosets` | 保留原业务处理器 |
| `data item textures` | `data item textures` | 保留原业务处理器 |
| `data creature display` | `data creature display` | 保留原业务处理器 |
| `data creature model` | `data creature model` | 保留原业务处理器 |
| `data encounter get` | `data encounter get` | 保留原业务处理器 |
| `data decor list` | `data decor list` | 保留原业务处理器 |
| `data decor get` | `data decor get` | 保留原业务处理器 |
| `asset inspect` | `asset inspect` | 保留原业务处理器 |
| `asset search` | `asset search` | 保留原业务处理器 |
| `asset export` | `asset export` | 保留原业务处理器 |
| `asset demux` | `asset demux` | 保留原业务处理器 |
| `version` | `version` | 保留原业务处理器 |
| `describe` | `describe` | 保留原业务处理器 |
| `init` | `init` | 保留原业务处理器 |
| `doctor` | `doctor` | 保留原业务处理器 |
| `config show` | `config show` | 保留原业务处理器 |
| `config set` | `config set` | 保留原业务处理器 |
| `cache status` | `cache status` | 保留原业务处理器 |
| `cache verify` | `cache verify` | 保留原业务处理器 |
| `cache prune` | `cache prune` | 保留原业务处理器 |
| `live instances` | `live instances` | 保留原业务处理器 |
| `live connect` | `live connect` | 保留原业务处理器 |
| `live reload` | `live reload` | 保留原业务处理器 |
| `live reload fallback` | `live reload fallback` | 保留原业务处理器 |
| `live bugs` | `live bugs` | 保留原业务处理器 |
| `target resolve` | `target resolve` | 保留原业务处理器 |
| `target show` | `target show` | 保留原业务处理器 |
| `target list` | `target list` | 保留原业务处理器 |
| `target add` | `target add` | 保留原业务处理器 |
| `target remove` | `target remove` | 保留原业务处理器 |
| `target available` | `target available` | 保留原业务处理器 |
| `live status` | `live status` | 保留原业务处理器 |
| `live resume` | `live resume` | 保留原业务处理器 |
| `live cancel` | `live cancel` | 保留原业务处理器 |
| `live abandon` | `live abandon` | 保留原业务处理器 |
| `live session` | `live session` | 保留原业务处理器 |
| `live disconnect` | `live disconnect` | 保留原业务处理器 |
| `evidence show` | `evidence show` | 保留原业务处理器 |
| `evidence verify` | `evidence verify` | 保留原业务处理器 |
| `evidence list` | `evidence list` | 保留原业务处理器 |
| `evidence bundle` | `evidence bundle` | 保留原业务处理器 |
| `evidence keep` | `evidence keep` | 保留原业务处理器 |
| `evidence remove` | `evidence remove` | 保留原业务处理器 |
| `source list` | `source list` | 保留原业务处理器 |
| `source batch` | `source batch` | 保留原业务处理器 |
| `source sync` | `source sync` | 保留原业务处理器 |
| `source inspect` | `source inspect` | 保留原业务处理器 |
| `source index` | `source index` | 保留原业务处理器 |
| `source query` | `source query` | 保留原业务处理器 |
| `source refs` | `source refs` | 保留原业务处理器 |
| `source context` | `source context` | 保留原业务处理器 |
| `source session status` | `source session status` | 保留原业务处理器 |
| `source session close` | `source session close` | 保留原业务处理器 |
| `source diff` | `source diff` | 保留原业务处理器 |
| `source prune` | `source prune` | 保留原业务处理器 |
| `source validate` | `source validate` | 保留原业务处理器 |

新增入口：`discover <question...>`、`schema <command...>`；CurseForge 新增 `publish curseforge session doctor`、`project list`、`plugin game-versions/upload/status/verify` 六项完整路径。Publish 的字段、动态选项和示例直接引用业务 InputSpec；`--plan` 在本地验证，提交状态和读回状态分别返回。产品目录与当前选择验证共用 selection 目录；中文“无限服／国服无限服”发现到正式 Forever，不产生新的 Beta 选择。

退役两个不改变行为的参数：`--passive`（instances 一直只读）和 `--no-cache`（Native 一直读取新鲜身份）。旧接受形式的测试改为拒绝测试；实际零输入、身份新鲜度和恢复不重放回归保留。

语法支持 `--flag=value`、布尔 `=false`、`--` 后的字面位置参数、带空格路径；普通标量不允许重复，`--param` 允许不同参数名，update 的路径和安装选择允许重复。解析错误仍使用请求的 text/JSON/JSONL；Cobra 的 usage 不混入机器正文。原业务结果与退出码不变。

NewBee 官方/Creator、网易 DD、小黑盒和 ModUs 主客户端/Creator 同样通过 `publish <platform> ...` 注册；各动作的嵌套字段与动态选项由平台 InputSpec 提供。`discover` 按业务词定位，`schema` 返回具体动作合同，无需在 Skill 再维护完整命令表。

示例中的 PIN/CON/CAP、路径和 PID 是待替换输入；CLI 的 schema 不查询动态环境或凭据。业务运行仍核固定来源、环境准备和恢复状态。
