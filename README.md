# Lychee Dev Toolkit

<img src="addon/Media/Logo.png" width="80" alt="Lychee Dev" />

用于魔兽世界插件开发的 Go CLI、游戏内 Lua 工作台和 Agent Skill。把源码调查、游戏数据、资源导出和实机验证放在同一套工具里。

[设计](Design.md) · [更新记录](Changelog.md) · [待办与验证范围](todo.md) · [Live 协议](docs/live-protocol.md) · [开发与发布](docs/development.md)

**当前文档以 3.2.0 开发基线为准。** 版本源仍为 3.1.2，3.2.0 尚未发布；从 npm 安装的版本不自动等于仓库当前代码。

## 能做什么

| 入口 | 能力 |
| --- | --- |
| Source | 固定 Blizzard UI 或第三方插件的 Git 提交，检索 Lua/XML/TOC，查询定义、引用、上下文和秘密值流；支持批量请求 |
| Data | 固定产品、地区和构建，从本地 CASC 或远端 CDN 读取 DB2，执行只读 SQL，查询法术、物品和其他关联记录 |
| Assets / Hotfix | 搜索和导出资源，转换支持的图片格式，按明确来源读取 Hotfix |
| Live | 通过 500 个 LoD 槽和 Mailbox 执行有预算的 Lua 探针，核实报告、清理资源并恢复中断 |
| 工作台 | 游戏内输入 /dev，使用 Run、Diagnostics、Inspector、Events、Function Trace 等工具 |
| Agent Skill | 按问题选择调查路径，保留固定引用、原连接和请求，按真实验证范围报告结果 |

Source、Data 和 Assets 不需要启动游戏。Live 需要 Windows amd64、运行中的客户端，以及已加载的配套受管插件。

## 产品

| CLI product | 客户端目录 | Source 分支 |
| --- | --- | --- |
| retail | _retail_ | live |
| classic | _classic_ | classic |
| classic-era | _classic_era_ | classic_era |
| titan | _classic_titan_ | classic_titan |
| forever-asia-beta | _classic_beta_ | forever |
| forever-cn-beta | _cn_beta_ | forever |
| anniversary | _anniversary_ | classic_anniversary |

两个无限测试服共享 forever 源码分支，但 Data 的产品、地区和构建配置独立。目录用于发现候选，实际身份取自安装元数据。

七产品已完成源码同步及本地/CDN 数据抽样读取。当前 Live 协议已在 Retail、国服无限测试服完成容量验证，Retail 完成双实例验证；其他产品的当前协议实测及周年服运行时适配见 [todo.md](todo.md)。

## 从仓库运行当前基线

需要 Git、Go 1.27.1；Node.js 22.14.0 及以上用于 npm 启动器和维护脚本。

    go build -o .tmp/lycheedev.exe ./cmd/lycheedev
    ./.tmp/lycheedev.exe version --format json
    ./.tmp/lycheedev.exe describe --format json

describe 是命令和参数的唯一目录。需要具体用法时，使用相应命令的 --help；不要照搬旧版本命令。

## 使用已发布 npm 包

    npm install --global lycheedev@next --ignore-scripts
    lycheedev version --format json
    lycheedev describe --format json

npm 包包含原生 Windows 可执行文件、插件、Skill 和发行清单。安装 npm 包不会自动修改游戏目录或部署 Skill。

PowerShell 中可从已安装发行包部署：

    $release = Join-Path (npm root -g) 'lycheedev'
    lycheedev addon install --release $release --installation 'D:/Game/World of Warcraft/_retail_'
    lycheedev skill install --release $release --path (Join-Path $env:USERPROFILE '.agents/skills/lycheedev')

已有安装的升级与恢复按 [安装指南](skills/lycheedev/references/installation.md) 执行。开发版 Live 需要与 CLI 配套的受管发行包，不能通过覆盖复制 addon 目录绕过安装清单。

## Live 的工作方式

当前协议名称统一为 **Lycheedev mailbox protocol v1**。

- CLI 通过运行时指令特征和 RIP 相对位移找到 Lua 根，再沿 Lua state → globals → Mailbox 读取。
- 500 个 LoD 槽承载输入信封，独立快捷键负责执行和重载。
- 默认执行键为 Ctrl+Alt+F12，重载键为 Ctrl+Alt+F11；CLI 使用 Mailbox 公布的实际有效绑定。
- 普通编辑框透传这两个动作，保留文字、光标和焦点。Native Live 不发送 Esc 或聊天文字。
- 每个进程有独立连接；共享安装的多实例通过槽预约协调。
- 读取期间快照变化会在原期限内重读。已提交或结果未知的输入不会因恢复而重复发送。

首次安装或连接异常先用 live doctor。doctor 默认只读；--fix 复用受管安装和原连接恢复。插件尚未加载、没有可验证 INPUT 时，需要在指定客户端手动重载一次。完整规则见 [协议](docs/live-protocol.md)。

## 给 Agent

使用仓库内的 [lycheedev Skill](skills/lycheedev/SKILL.md)。先读取实际命令合同，再选择 Source、Data、Assets 或 Live；跨调用保留 pin、project、CON 和 request。

缺失密钥、部分数据、语义分析边界和未运行的实机项目应如实报告。静态检查、fixture 通过和实机通过是不同事实。

## 参与开发

代码结构、接口合同和 UI 原则见 [Design.md](Design.md)；构建、测试、临时产物和发布流程见 [开发文档](docs/development.md)。

项目代码采用 [MIT](LICENSE)。第三方许可及来源见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。