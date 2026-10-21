# Lychee Dev Toolkit 设计

本文描述 3.2.5 开发基线的当前实现。发行版本以 [release/version.json](release/version.json) 为准；验证范围见 [todo.md](todo.md)。

## 组成与职责

产品交付 Go CLI、Lua addon、Agent Skill 和 npm 包，平台为 Windows amd64。Source、Data 和资源调查独立于游戏进程；Live 负责实际客户端执行和观察。

| 模块 | 职责 |
| --- | --- |
| `cmd/lycheedev`、`internal/command` | 命令合同、参数解析、选择与编排、结果和退出码 |
| `internal/selection` | 产品目录、安装身份、Source/Data/Changes pin |
| `internal/codebase`、`internal/addoncheck` | Git 快照、索引、搜索、关系、上下文、LuaLS，以及独立的 addon 静态解析与 API 审查 |
| `internal/records` | CASC/CDN、DB2/SQL、Hotfix、关联记录和资源导出 |
| `internal/live/channel`、`internal/live/memory` | Native 连接、恢复日志、运行时定位和 Mailbox 读取 |
| `internal/bridge` | Go/Lua 共享信封、记录和协议类型 |
| `internal/delivery` | 受管安装、升级、槽发布与迁移 |
| `internal/desktop` | 固定窗口身份、进程核验与输入 |
| `internal/evidence`、`internal/vault` | 证据、缓存、预算和持久化 |
| `addon/` | 启动、兼容层、Live bridge、工作台与共享 UI |
| `skills/lycheedev/`、`packages/npm/lycheedev/` | 调查编排、启动器和发行资源 |

业务状态由对应模块维护，命令层调用现有合同。客户端差异集中在产品目录、Compat、ClientGate、生成目录和运行时定位配方。

## 产品与来源

`internal/selection/products.go` 定义交付产品。Source 家族和数据交付轨道分开建模。

| CLI product | Launcher/CDN 键 | 客户端目录 | Source ref | Interface |
| --- | --- | --- | --- | --- |
| retail | wow | _retail_ | live | 120105、120100 |
| classic | wow_classic | _classic_ | classic | 50504 |
| classic-era | wow_classic_era | _classic_era_ | classic_era | 11509 |
| titan | wow_classic_titan | _classic_titan_ | classic_titan | 38002 |
| forever-asia-beta | wow_classic_beta | _classic_beta_ | forever | 16001 |
| forever-cn-beta | wow_cn_beta | _cn_beta_ | forever | 16001 |
| anniversary | wow_anniversary | _anniversary_ | classic_anniversary | 20506 |

安装身份从 `.flavor.info`、`version.txt`、`.build.info` 等元数据解析；明确选中的客户端路径保持原选择。两个 Forever beta 共享源码家族，但独立固定地区、产品和构建配置。

Data pin 固定 product、build、BuildConfig、CDNConfig、region、locale 和 DBD 来源。同版本号不代表相同内容。主 addon 与 500 个 LoD 槽采用一致的多 Interface 声明，声明更新通过受管安装迁移。

Retail 12.1.5.70391 对应 `live` 固定提交 `18c6d677b605b312f3d813d498404631fb941977`、Interface 120105；保留12.1.0的120100基线。同一产品的安装选择按实际构建系列匹配，不取该产品的第一条基线。

周年服 API 基线为 `classic_anniversary` 提交 `2cfec7c00f0f24762d5ca35a4beb1e93c0cde328`（2.5.6.70006），具有独立的 1483 项事件目录。其他客户端基线保留在对应生成文件和运行时目录中。

## Source

`source sync` 固定仓库与提交，index 生成结构事实；query、refs、context、flow 和 diff 在固定快照上运行。第三方 addon 的 API 环境同样固定，避免读取时隐式换成另一个客户端。

精确查询按 exact、prefix、索引文本、正文的顺序查找。已命中时停止向低层扫描；声明索引按需生成，请求内复用查找表、Git 文档与 API 环境。

文档按原始字节解析并保留哈希。非UTF-8字节作为独立告警记录所在文件、行号与零起始字节偏移；编码告警不减少分析完整性或AST文件数，也不使addon加载检查失败。query和inspect共用U+FFFD显示规则，归档仍保留原字节，不把非UTF-8归档标成UTF-8。实际语法错误和跳过的超限文件仍影响coverage；文档超限单独报告实际字节数及上限。索引与文件facts使用v3缓存格式，旧缓存不复用，固定PIN及parserRevision不变。

`source batch` 接收至多 64 KiB 的文件，包含 1–32 项 query、refs 或 context。整批共享 90 秒期限和请求级 LuaLS worker，保留输入顺序；每项各自返回 coverage、complete、truncated 和 cursor。部分条目失败不能被当作整批成功。显式 `session-reuse` 才复用跨请求 worker。

语义结果注明引擎及覆盖范围。`static-only` 保留静态证据；未知调用和条件传播不能证明 secret 值使用安全。

## Data、Hotfix 与资源

远端读取从指定产品和地区的 versions/cdns 固定配置；`--build` 精确匹配，CN 使用国服版本服务。来源缺失时返回缺口，不切到其他地区或产品。

本地 CASC 按 pin 选择配置、索引和内容，优先读取 EKey 对应桶并验证完整键、代次和校验和。远端归档每查询默认最多并发八项，显式 download.workers 限制同工作区获准查询的总下载并发；按清单顺序确定结果，共享网络与内存预算，大索引使用稀疏读取。

Windows Data查询在获得工作区slot后读取可用物理内存与进程提交/虚拟地址余量，取最小值的一半按并发容量分配本次上限。retained/scratch与metadata预算按实际计费扩容，每次扩容重读可用内存；同一查询的天花板保持不变，网络/工作量、单文件和SQL执行器限制独立。预算是保守逻辑计费，不是进程RSS限制。SQL和导航结果的memoryBudget报告模式、当前额度、天花板、扩容次数和scratch峰值。

DB2、SQL、关联记录和资源导出共用固定数据身份。Hotfix 记录 provider、build、locale、过滤及分页范围。缺密钥、缺定义、截断或部分解码显式返回；原始内容和派生资源分别记录完整性。

Wago 的 coverage.complete 表示当前窗口耗尽，本地 snapshot 保留完整快照语义；统一的 fullQueryComplete 表示从第一页无 cursor 开始并完整到末尾。完整本地缓存查询不代表完整服务端覆盖。DB2 JSONL 先完成来源复核与归档，再发唯一 end；后置失败发 error，不发成功 end。

## Native Live

完整合同为 [Lycheedev mailbox protocol v1](docs/live-protocol.md)。

**读取路径：** 运行时指令特征 → RIP 相对目标 → Lua state → globals → 命名 Mailbox。CLI 只读游戏内存，每次核验路径和发布对象。Native Live 不使用私有堆扫描、光学输入或聊天命令。

Forever 扫描主模块 `.text` 的可读区间，保留短读前缀，对剩余部分按页有界补读。所有命中先解析 RIP，再按目标根去重并验证 Lua/Mailbox；多个有效根报告歧义。扫描缺口与读取失败写入覆盖证据，部分覆盖下成功定位不代表全范围唯一。

**输入路径：** 500 个不可重用的 LoD 槽承载信封；正式执行键触发 prepare → challenge → commit，独立重载键负责切换运行时。普通编辑框透传两个有效绑定，CLI 使用新鲜 INPUT 授权输入，不发送 Esc 或聊天文字。玩家按键优先：所有键松开且连续500ms没有键盘事件后重新核验INPUT；已开始的组合键先完成并抬键，不在修饰键按下时等待。

主插件启动时检查当前角色的受管槽启用设置，启用禁用槽并保存；有修改时通过 `Mailbox.input.slotReloadRequired` 请求 CLI 重载。CLI 在发布 bind 前处理请求，沿原 CON 和期限重载后重绑；已有重载和待恢复交易继续保留。启动检查不加载槽，不修改其他插件，也不增加轮询。

**身份与竞争：** 先固定 installation、PID、进程创建时间、HWND、build 和 product，再绑定 GUID、runtime、owner、fence。每个进程独立连接；共享安装使用短发布锁和持久槽预约，等待执行或回执时不占发布锁。

**结果与恢复：** HEAD/BODY 验证报告，confirm/release 完成确认与清理。reportState、report.ok 和 cleanup 分别表示字节已核实、业务成功和资源释放。输入意图先落盘，恢复沿原 project/CON/request 与期限继续，submitted/uncertain 输入不重放。

容量不足时重载并在新 runtime 继续原请求。报告已核实且 release 回执缺失超过 5 秒时，在原期限内允许一次独立重载清理；保留原报告，验证旧 runtime 退役后才记为清理完成。短暂读取变化在原调用内重读，不延长持久预算。

`live doctor` 只读检查，`--fix` 复用受管安装和原连接。首次没有兼容且新鲜的 INPUT 时，保留目标等待一次手动重载。

旧窗口队列、光学/QR 和 SavedVariables 执行驱动已退役；OP/BTP 历史记录保留查看、取消及必要处置，不恢复执行。当前执行、重载和错误调查要求 Native CON。

## Addon 与工作台

未启用的业务功能不创建 frame、事件、hook、timer 或 OnUpdate。最小启动核拥有正式绑定和独立启动诊断，注册完成后不保留空闲采样；bridge 启用后才运行 INPUT 采样与编辑框透传。

INPUT 采样框架独立于 UIParent，使挂机展示隐藏主 UI 时仍可更新状态。登录、加载、战斗以及 protected、forbidden、密码、secure text、secret 值边界仍须检查。

编辑框透传在修改键盘传播设置前检查战斗状态，战斗中保留原按键处理且不接管新焦点。战斗中停用立即撤销 INPUT 和采样；若旧编辑框尚未恢复，仅保留既有框架的脱战清理事件及原处理函数委托，恢复后全部撤销，不另建轮询。

共享样式位于 `addon/UI/Theme.lua`，字体使用客户端字体；工作台不改写 Blizzard 对象来实现自身样式。中英文 locale 同步维护。

## CLI 与持久化

`internal/command/command_contract.go` 同时驱动 parser、help、describe 和 Skill 命令参考。结果使用 `lycheedev.result.v1`；JSONL 以合法 end 帧和 exit 0 表示输出完成。

| 退出码 | 含义 |
| --- | --- |
| 0 | 完成 |
| 2 | 参数或选择错误 |
| 3 | 能力或环境限制 |
| 4 | 输入或协议无效 |
| 5 | 外部或业务失败 |
| 6 | 等待继续 |
| 7 | 取消 |
| 8 | 内部错误 |

项目引用与恢复日志位于 `.lycheedev`，缓存与未完成请求分别管理。缺失来源、覆盖缺口和待清理状态必须保留。fixture 结果与真实客户端结果分别记录；历史归档中的图像仅保留读取与校验，不启动捕获。

受管升级先在独立准备目录生成完整 next 与 journal，再发布恢复目录。更新库存记录发现位置；默认更新现存安装和中断 replacement 的原目标，显式路径仍可重新安装已移除的目标。npm 更新使用 Windows 系统命名管道互斥，退出或崩溃释放锁。
