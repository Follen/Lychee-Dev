# Lychee Dev Toolkit — 3.2.0 设计基线

本文件描述当前实现。3.2.0 是开发基线名称，发行版本仍由 release/version.json 决定。变更摘要见 [Changelog.md](Changelog.md)，尚未完成的工作见 [todo.md](todo.md)。

## 1. 产品与边界

一个 Go CLI、一套 Lua addon、一个 Agent Skill 和一个 npm 分发包共同交付。Windows amd64 是当前发行平台。Rust 重写不在当前计划中。

源码与数据调查可以独立运行；只有游戏内观察和执行需要 Live。所有跨调用查询都使用固定来源，显式区分来源身份、结果完整性、业务成功与资源清理。

## 2. 模块职责

| 路径 | 职责 |
| --- | --- |
| cmd/lycheedev、internal/command | 入口、命令合同、参数解析、模块错误到结果和退出码的映射 |
| internal/selection | 产品目录、安装识别、固定 Source/Data/Changes pin、项目引用 |
| internal/codebase | Git 源码、索引、搜索、关系、上下文、LuaLS 及有界数据流 |
| internal/records | CASC/CDN、DB2、SQL、Hotfix、关联记录和资源解码 |
| internal/live/channel、internal/live/memory | Native 连接状态机、日志恢复、运行时定位和只读 Mailbox |
| internal/bridge | Go/Lua 共享记录、信封和协议类型 |
| internal/delivery | 受管插件/Skill 安装、升级恢复、500 槽发布与迁移 |
| internal/desktop | 固定窗口输入与 WGC 捕获 |
| internal/evidence、internal/vault | 证据、完整性、缓存、预算与持久化 |
| addon/Core、addon/Bridge、addon/Modules、addon/UI | 启动、兼容、协议、工作台模块与共享控件 |
| skills/lycheedev、packages/npm/lycheedev | 调查编排和发行资源 |

业务状态属于负责该功能的模块。CLI 负责选择和编排，不在命令层复制协议。客户端差异集中在 Compat、ClientGate 和产品/运行时配方目录。

## 3. 产品、安装与来源

internal/selection/products.go 定义七个交付产品；源码家族和交付轨道分别建模。

| product | Launcher/CDN 产品键 | 目录 | Source ref |
| --- | --- | --- | --- |
| retail | wow | _retail_ | live |
| classic | wow_classic | _classic_ | classic |
| classic-era | wow_classic_era | _classic_era_ | classic_era |
| titan | wow_classic_titan | _classic_titan_ | classic_titan |
| forever-asia-beta | wow_classic_beta | _classic_beta_ | forever |
| forever-cn-beta | wow_cn_beta | _cn_beta_ | forever |
| anniversary | wow_anniversary | _anniversary_ | classic_anniversary |

目录不是身份。读取可用的 .flavor.info、version.txt、.build.info 与配置内容；明确选中的客户端不能跳到兄弟目录。

两个 Forever beta 可以共享 Source 家族 pin，但不共用区域 Data pin。相同 build 字符串不等于相同 BuildConfig。DataPin 固定产品、build、BuildConfig、CDNConfig、地区、locale 和 DBD 来源。

Addon 当前具有 Retail 120100、Classic 50504、Era 11509、Titan 38002、Forever 16001 五个运行时 profile。周年服已有 Source/Data 产品路由，尚无对应 addon profile。产品目录、API 基线和实机验收分别维护。

## 4. Source

source sync 固定仓库和 commit；index 生成可校验的结构事实。query、refs、context、flow 和 diff 在固定快照上工作。第三方源码的 API 环境也必须固定，不由当前安装目录隐式决定。

精确查询依次使用 exact、prefix、索引文本和正文；已命中高层时不再扫描低层。名称、路径和关系查找共用认证元数据，声明索引按需创建。初次读取仍验证完整快照。

source batch 在一个请求中共享快照、查找表、Git batch、文档缓存、API 环境和 LuaLS worker：

- 请求文件最多 64 KiB，1–32 个条目，每项是 query、refs 或 context。
- 整批共享既有 90 秒截止时间，每项独立返回 coverage、truncated、complete 和 cursor。
- 结果保留输入顺序与 ID；某项失败时不把前半批报告为成功交付。
- 默认 worker 在请求结束时回收；显式 session-reuse 才复用持久 worker。

语义结果说明所用引擎和覆盖范围。static-only 保留静态证据；秘密值传播的未知边和条件不能被解释为污染安全证明。

## 5. Data、Hotfix 与资源

远端解析使用指定产品的 versions/cdns 清单并固定配置；--build 是精确条件。CN 使用国服版本服务，其余地区使用对应地区服务。地区缺失不能自动改用其他地区或产品。

本地读取匹配已固定 pin 的 CASC 元数据，根目录按产品选择配置行；明确客户端路径保持原选择。缺文件、缺密钥、定义不匹配均返回具体缺口，不隐式切换 CDN。

CDN 归档查找最多并发八个，按清单顺序判定结果，并共享网络/内存预算。小索引可合并读取，大索引保持稀疏访问。本地索引优先读取 EKey 对应桶，仍核验完整 EKey、索引代次与校验和。

DB2、SQL、关联记录和资源共用固定数据身份。Hotfix 保留 provider、build、locale、分页和过滤范围。缺密钥不等于空表；一页结果不等于完整表；原始内容与派生资源分别记录完整性。

## 6. Native Live

详细合同见 [Lycheedev mailbox protocol v1](docs/live-protocol.md)。

CLI 只读进程内存。运行时指令特征匹配和 RIP 相对解析得到 Lua 根，沿命名对象读取 Mailbox。每次读取核验指令、指针路径和发布对象；不扫描私有堆，不把旧地址当成当前输入许可。

输入通过 500 个不可重用的 LoD 槽和两个正式快捷键实现。prepare → challenge → commit 后执行业务，HEAD/BODY 确认报告，confirm/release 完成结果与资源清理。容量不足时独立重载并继续原请求。

目标固定 installation、PID、进程创建时间、HWND、build 和产品；绑定后增加 GUID、runtime、owner、fence。共享安装采用短发布锁和持久槽预约，业务等待不占发布锁。

输入先持久化 intent，后记录 not_sent、submitted 或 uncertain。恢复沿原 project/CON/request 继续，期限不重置；已提交或不确定输入不重放。瞬时 path_changed 在原调用中重读；仅已提交重载阶段等待暂时为空的 Lua 根或字符串布局。

reportState、report.ok、cleanup 表示不同事实。报告已核实但清理未完成时，报告仍可用，同时保留清理义务。

## 7. Addon 与工作台

未启用的业务功能不创建 frame、事件、hook、计时器或 OnUpdate。独立启动核注册正式绑定和启动诊断；注册完成后不保留空闲事件和计时器。启用 bridge 后的 INPUT 采样与普通 EditBox 透传是明确例外，停用时撤销。

受保护、forbidden、密码、secure text 和 secret 值属于边界。战斗和不可安全观察的输入状态不能授权机器输入。所有客户端分支集中在兼容层或生成目录。

UI 使用 addon/UI/Theme.lua 的共享变量：深灰背景、浅色正文、荔枝红强调色；控件圆角 8、面板圆角 12、间距 6/10/16/24、图标点击区 32。字体使用客户端 STANDARD_TEXT_FONT。工作台不改动 Blizzard 对象来实现自身样式。中英文 locale 同步维护。

## 8. CLI、持久化和证据

internal/command/command_contract.go 同时生成 parser、help、describe 和 Skill 命令参考，不维护另一份手写命令目录。

结果使用 lycheedev.result.v1。JSONL 只有合法 end 帧和 exit 0 才表示输出完成。截断、部分覆盖、缓存来源和未完成清理均显式返回。

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

项目引用与运行记录保存在 .lycheedev，跨进程占用使用明确的锁和租约。缓存可以清理，未解决请求的日志不能当作缓存删除。既有旧版本用户数据不自动导入。

源码 pin、命令参数和结果只记录复现需要的信息。常规开发不提交逐次运行的日志、内存快照、截图或整套发行副本；维护正式测试和当前状态即可。