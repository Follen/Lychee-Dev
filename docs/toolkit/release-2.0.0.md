# Lychee Dev 2.0.0：Windows CI 与 npm 发布规范

状态：实施中，基础 Go CI 与 npm 开发启动包装已落地，正式发布工作流和门槛尚未完成。日期：2026-09-21。

当前本地 npm smoke 仅验证 Windows 单平台开发包，含空格/中文路径、实际 tgz、
隔离 prefix、禁用安装脚本及原生启动，并已打入当前 addon/skill 源码，在模拟客户端
完成安装、升级、恢复、归档移除及用户编辑冲突检查；不是全部游戏运行验收。
`terminalRecovery` 另记录实际安装的启动器对受控完成任务执行 `live resume` 的
成功释放与重复调用测试；它不代表未完成任务续跑或真实游戏执行通过。
开发 manifest 暂时 private=true，最终许可未核定。实际证据见
[实施状态](implementation-status.md)，下方正式检查表不得据此勾选完成。

在有交互桌面的 Windows amd64 主机上，可显式设置
`$env:LYCHEEDEV_TEST_DESKTOP='1'` 后运行
`node packages/npm/lycheedev/test/install-smoke.mjs <npm-cli.js绝对路径>`。
此模式还通过实际安装的 Node 启动器执行 `init → target resolve → live bind →
evidence verify → live session`，以独立进程的自建 QR 窗口验证原生捕获和会话存储。
报告的 `nativeBinding` 记录启动器路径、结果和测试输出；未开启时明确为 `not-run`，
不得计入原生桌面验收。测试不向 WoW 发键、不替换游戏插件，亦不证明游戏运行闭环。
普通托管 CI 不假定交互桌面可用，继续保留此显式开关。

本次正式发行版本固定为 **2.0.0**，Git tag 固定为 **v2.0.0**，正式 npm 包固定为 **lycheedev@2.0.0**。本文补充 [架构设计](design.md)、[回归方案](regression.md) 和 [实施路线](roadmap.md)。当前文档修改不创建 tag、不发布 npm、不修改生产安装。

## 1. 版本的单一来源

`release/version.json` 已作为版本源落地，当前为 `2.0.0-dev`，正式目标为 `2.0.0`。
`node tools/version.mjs --write` 同步 npm manifest、锁文件根与包条目、四个 TOC、
addon Runtime 和 Go buildinfo 常量；先验证所有目标再执行生成。
`node tools/version.mjs --check` 只检查，不静默修复；CI 与开发打包入口执行此门槛。
skill/addon 资源发行清单从校验后的包版本生成。生成器不改变 private、许可或
发布授权，也不创建 tag。CLI `version` 已从 Go 内嵌 VCS 信息读取 `commit` 与
`workspaceDirty`；未知状态保留 null，不回退读取调用者目录。开发打包强制
`-buildvcs=true` 并核对安装后二进制与清单的提交/工作树状态；正式发行仍必须拒绝
dirty 或未知身份，并完成候选 Commit、跨平台产物、tag 与发布门槛绑定。

| 对象 | 本次正式值 / 规则 |
| --- | --- |
| 版本源、npm package.json、package-lock 根版本 | `2.0.0` |
| 四 TOC 的 `## Version:` | `2.0.0` |
| CLI `version` | `2.0.0`，同时报告构建 Commit、Go 和平台 |
| skill/addon/resource 发行 manifest | release 为 `2.0.0` |
| Git tag / GitHub Release | `v2.0.0`，指向验收过的准确 Commit |
| npm 正式 dist-tag | `latest` 指向 `2.0.0` |
| 可选候选发行 | `2.0.0-rc.N`，tag 为 `v2.0.0-rc.N`，npm dist-tag 为 `next` |
| 工作空间和通信 schema | 独立版本，不强行改成 `2.0.0` |

候选版必须使用自己的完整版本，不能先将 `2.0.0` 发布用于试装。npm 同名同版本不能覆盖或重复使用；若正式包内容需要修复，应使用新的版本，不移动 `v2.0.0`。[npm publish 规则](https://docs.npmjs.com/cli/v11/commands/npm-publish/)

发布日志、回归报告和资源 manifest 必须包含同一个 Commit。带未提交改动的本地产物不进入正式发行。

## 2. 工作流分工

目标工作流如下；实施时替换当前流程，同时更新 npm Trusted Publisher 的工作流绑定。

| 文件 | 触发 | 职责 |
| --- | --- | --- |
| `.github/workflows/toolkit-ci.yml` | pull_request、默认分支 push、merge_group（启用合并队列时）、手动验证 | Windows 必过检查、跨平台离线测试、构建与打包验证 |
| `.github/workflows/toolkit-desktop.yml` | 受信任发行候选的手动验证 | 有交互桌面/GPU 的 Windows 实机、WGC、四客户端证据 |
| `.github/workflows/toolkit-release.yml` | 版本 tag push；手动仅允许 dry-run | 固定 Commit 验证、构建、封存产物、npm 发布与发布后验证 |

普通 CI 只有读取代码所需权限。PR 不使用发布权限，不把外部 PR 代码交给带游戏账号的 runner。实机 runner 不承担 npm 发布；npm 发布运行在 GitHub-hosted runner。

PR CI 可以取消被更新 Commit 替代的旧运行。发布 workflow 使用版本级 concurrency，`cancel-in-progress: false`，避免发布到一半被同版本重跑中断。

必过状态使用固定 job 名称，不能因平台矩阵名称变化而丢失保护规则。聚合 job `ci-required` 即使上游失败也执行，显式检查所有必需 job 成功；skipped/cancelled 不能被解释为 passed。

## 3. Windows 是必须通过的原生环境

基础 runner 明确使用 `windows-2025`，不只使用随时间变动的 windows-latest。记录实际 runner image 版本，因为固定 OS 标签仍会更新镜像。Windows x64 原生构建和运行必须通过，Linux 上交叉构建一个 exe 不算 Windows 验证。[GitHub runner 镜像目录](https://github.com/actions/runner-images)

PowerShell 使用 `pwsh`，设置失败即停，并在调用 go/npm/lua 等外部程序后检查退出码；不能依赖 PowerShell 错误偏好自动捕获所有非零外部退出。路径包含空格、中文及长路径场景进入 Windows 测试。

| Required job | 必须实际执行的检查 |
| --- | --- |
| `windows-contract` | 格式、`go vet ./...`、`go test ./...`、新命名/import 约束、schema 与命令参考一致性 |
| `windows-process` | 真实 CLI 多进程、OS 锁、同窗口排他、双 Agent 并行、进程死亡及未决恢复 |
| `windows-addon` | Lua 5.1 语法、四客户端离线矩阵、locale、TOC 顺序、事件目录和 ZIP 结构 |
| `windows-package` | Windows 原生发行 exe、npm 实际 tgz 内容、隔离全局安装、禁用安装脚本、离线业务 smoke |
| `race` | 共享状态竞态检测（非必需，Windows 原生） |
| `ci-required` | 核对必需结果、归档报告，不接受缺失或跳过作为通过 |

Windows 多进程测试采用临时工作空间和 barrier，不对真实桌面发送输入。消息投递模拟和自建接收窗口可用于基础 CI，但有交互桌面要求的用例应进入 desktop workflow。

托管 Windows CI 不假定可用真实 WoW、WGC/GPU 或已登录交互桌面。发行门槛额外要求 WIN 系列原生验证和 LUA 四客户端实机报告，与候选 Commit、二进制摘要、插件摘要绑定。缺证据时 release-gate 失败，不用基础 CI 的绿色状态替代。

## 4. 工具链与构建

- Go 发行工具链固定为 `1.27.1`；`go.mod` 的语言基线为 `1.27.0`，CI 核对实际 go version。
- Windows Lua 固定 `5.1.5`。下载或构建输入必须有校验和，不能只信任一个可变下载链接。
  当前 `tests/tools/build-lua.mjs` 使用[Lua 官方源码及校验清单](https://www.lua.org/ftp/)
  的 `lua-5.1.5.tar.gz`，SHA256 为 `2640fc56a795f29d28ef15e13c34a47e223960b0240e8cb0a82d9b0738695333`。
  Windows contract 设置必需解释器环境，缺失时测试失败，不接受跳过；构建报告保留
  C 编译器版本及解释器摘要。此离线协议门槛不等同于四端完整 UI 或实机矩阵。
- CI Node 选择受支持的 24.x 补丁，并在 S0 核实后把精确补丁写入工具链清单。npm 可采用已核实流程使用的 `11.18.0`，后续调整需提交清单变更，不使用 `npm@latest`。
- npm Trusted Publishing 的最低要求为 Node `22.14.0`、npm `11.5.1`；启动包装的用户引擎要求定为 Node `>=22.14.0`，并在最低版本和发行 Node 上测试。原生发行包不依赖 Node。[npm Trusted Publishing](https://docs.npmjs.com/trusted-publishers/)
- GitHub Actions 固定到审核过的完整 commit SHA，并保留动作版本注释；实现时填真实 SHA，不在设计文档里编造。
- release 构建使用 `-trimpath`，明确 GOOS/GOARCH、版本和 Commit。CGO_ENABLED=0 的发行目标与需要 CGO/本地编译器的 race 测试 job 分开配置。
- 模块与工具缓存键包含 OS、架构、工具链和 go.sum/lockfile；构建产物不能只依据缓存命中即被接受。

目标原生发行矩阵为 **Windows amd64**（2026-09-23 owner 裁决收缩：清理 macOS 与
Linux 支持，产品只发布并验收 Windows amd64）。每个声明平台必须执行匹配架构的
安装/运行 smoke；纯 Go 包保持可移植，但非 Windows 平台不构建、不发行、不验收。

## 5. npm 包形态：一个公开包，零运行依赖

Go 安装层读取的 `release.json` 使用 `lycheedev.release.v1`：`version`、
`commit`（完整小写 Git SHA-1）、`binaries`（平台键到文件记录）和 `resources`
（资源记录数组）。每条文件记录为 `path`、`bytes`、`sha256`；资源路径相对
`payload/`，仅允许 `addon/` 和 `skill/`。部署载荷必须含 skill 入口与四 TOC，
并与资源清单完整匹配。读取拒绝重复/未知 JSON 字段、超限输入和版本不匹配。
原生压缩包声明其对应平台；npm 组装门槛要求 windows-amd64 平台。当前安装层核验
二进制记录结构，不代替启动器的二进制内容校验、发行来源认证或 TOC 语义测试。
只有二进制、缺少资源的包不能用于该安装接口。开发 smoke 现已包含当前 addon/skill
资源，但只有宿主平台二进制；报告同时记录 HEAD 和 workspaceDirty，不能将未提交
工作树的载荷称为该 Commit 可复现的正式发行。

为遵守项目现有的零运行时 npm 依赖约束，2.0.0 只发布一个 `lycheedev` npm 包。主包内携带 windows-amd64 二进制与共用插件/skill 资源；不引入 optionalDependencies 平台包链。之前方案中的“平台包”在 2.0.0 指独立原生发行压缩包，而不是多个互相依赖的 npm 包。

```text
packages/npm/lycheedev/
  package.json
  bin/lycheedev.mjs            仅选择平台、转发 argv/stdio/退出码
  native/
    windows-amd64/lycheedev.exe
  payload/addon/
  payload/skill/
  release.json
  README.md
  LICENSE
  THIRD_PARTY_NOTICES
```

规范：

- `name: lycheedev`、`version: 2.0.0`、`type: module`、`bin.lycheedev: bin/lycheedev.mjs`。
- `dependencies`、`optionalDependencies`、`peerDependencies` 为空或省略；开发工具与实际交付分离。
- `publishConfig.registry` 指向官方 npm registry，`access: public`，包不能为 private。
- `repository` 指向实际 `Follen/Lychee-Dev` 仓库，并准确填写新 package 所在 directory，确保来源声明正确。
- `files` 为白名单；不含 Python、旧业务脚本、测试、调查记录、任务块、工作空间或本地账号路径。
- 安装脚本不是可执行文件可用的前提。`npm install --ignore-scripts` 后可直接运行 version、doctor 和离线业务；插件和 skill 部署由显式 CLI 命令完成。
- 启动器不联网下载 fallback 二进制，不读取旧 home，不通过 shell 拼接参数；不支持的平台返回准确错误。
- Unix 二进制可执行位和 Windows .exe 路径必须从真实安装后的目录测试。使用 node 的绝对 argv 转发，不把用户文本拼成 PowerShell/cmd 命令。
- npm 卸载不顺带删除用户工作空间、游戏插件或 skill，单独删除动作由显式命令和所有权清单处理。

代价是 npm 下载体积包含其他平台二进制；S0/S5 必须测量压缩体积、解包体积及安装耗时，并为 2.0.0 冻结门槛。原生用户可下载仅含本平台的压缩包。若包体积不满足门槛，先调整设计，不静默改用平台依赖或下载脚本。

## 6. 打包一次，验证并发布同一份字节

`npm pack --dry-run` 只用于初步检查，正式流程必须生成 `lycheedev-2.0.0.tgz`，并对该文件执行内容审计、全新安装和 smoke。发布 job 消费通过验证的 tgz，不从另一个目录重新构建或重新 pack。

发行 manifest 包含版本、Commit、工具链、五个二进制摘要、共用资源摘要、原生压缩包摘要、npm tgz 的 SHA-256/SHA-512 和测试报告摘要。tgz 自身摘要记录在外部封存清单中，避免自引用。

测试完成后重新计算文件摘要，与封存清单一致才允许发布。预发布脚本不能在 publish 阶段再次改变 payload。源码仓库不提交生成的二进制、压缩包、tgz 或临时打包目录。

原生压缩包和 npm 包中的对应二进制、addon、skill 必须内容一致。ZIP/TAR 元数据造成的包摘要差异与实际文件内容差异分别处理，不能只比较文件名。

## 7. OIDC 与发布条件

发布使用 GitHub-hosted runner 的 npm Trusted Publishing，发布 job 具有 `contents: read` 和 `id-token: write`，npm 包绑定准确的用户/仓库/workflow filename/environment。重命名 workflow 后必须完成远端绑定更新；每次发布前核对，不假设旧绑定自动生效。

不设置 NPM_TOKEN/NODE_AUTH_TOKEN 作为常规发布途径，不在 OIDC 失败时偷偷回退到长效 token。`.npmrc` 不得残留要求空 NODE_AUTH_TOKEN 的配置。公共仓库向公开包通过 OIDC 发布时自动生成 provenance，发布后核实关联的仓库与 Commit。[npm 来源证明](https://docs.npmjs.com/generating-provenance-statements/)

正式发布前同时满足：

1. ref 是严格校验过的 `v2.0.0` tag；不能只依赖触发器的通配符。
2. tag Commit 与固定版本源、通过的 Windows CI、四端实机证据、全部发行产物匹配。
3. `ci-required` 和 release-gate 完成，所有要求的报告可访问且摘要正确。
4. 工作流来源属于受保护的仓库和允许的 ref；检查通过的 SHA 必须是实际发布 SHA。
5. registry 尚未包含不同字节的 `lycheedev@2.0.0`；超时、403 和未知响应不能当作包不存在。
6. npm 权限和 Trusted Publisher 绑定已配置，许可及第三方说明通过审阅。

release-gate 验证既有实机报告时必须核对来源、Commit 和二进制/插件摘要，不接受任意提交一个 `passed: true` 文件作为证据。

手动 workflow_dispatch 只允许 dry-run，不通过 `dry-run=false` 在任意分支发布。失败恢复使用原 tag 运行的重跑机制，继续消费同一份已封存产物；产物过期或找不到时须重建并重新完整验收，不能假称原验证仍适用。

## 8. 2.0.0 发布顺序

```text
冻结版本与来源
  -> Windows / 跨平台 CI
  -> 构建 Windows amd64 原生产物及共用资源
  -> pack 实际 tgz
  -> 从实际 tgz/原生包安装验证 + 四端实机证据核对
  -> release-gate 封存 SHA / checksums / 报告
  -> npm publish 同一 tgz（2.0.0, public, latest）
  -> registry 元数据、provenance、实际安装回读
  -> 发布 GitHub Release v2.0.0 与验证清单
```

候选版使用同样检查路径，版本为 `2.0.0-rc.N`，publish tag 为 next。正式 2.0.0 的版本字节不同，必须对正式产物重新验证，不能直接引用不同摘要的 RC 实机报告。

正式命令形态为 `npm publish ./lycheedev-2.0.0.tgz --access public --tag latest`，该命令仅记录为未来工作流规范，不在当前任务执行。

发布后从 registry 读取准确的 `lycheedev@2.0.0` 元数据和 tarball integrity，在空目录/隔离 prefix 实际安装。至少 Windows x64 与各声明平台的 smoke 成功，确认 latest、version、Commit 和资源版本正确，再将 GitHub Release 标为正式完成。

不把 OIDC 可用于 publish 理解为可用于所有 npm 写命令。需要 dist-tag 回退、deprecate 或其他维护时，单独使用经授权且适用的维护身份；这些操作不作为 2.0.0 正常发布的隐含步骤。

## 9. 失败恢复

| 失败点 | 恢复规则 |
| --- | --- |
| 测试、实机或打包失败 | 不 publish，不创建正式 Release；修复后使用新候选 Commit 完整验证 |
| publish 返回不确定结果 | 先查询准确 name/version/integrity；相同字节已存在则进入后验证，不盲目再次发布 |
| 同名版本已存在且内容不同 | 停止；不得覆盖、删除重发或移动 tag；按新修复版本处理 |
| npm 已成功、GitHub Release 失败 | 校验 registry 与封存清单后只补 Release，不重复 publish |
| npm 成功但安装回读失败 | 标明发行未完成；保留证据，修复使用新版本；如需渠道回退另执行明确维护动作 |
| 产物不全、缺某平台或摘要变化 | 停止；不能以 warning 跳过后把整套发行标为成功 |

版本 tag 不移动，已经发布的 tgz 不替换。数据重置不属于发布后的自动修复动作。

## 10. 2.0.0 交付检查表

- [ ] 版本源、package/lock、四 TOC、CLI、skill/resource manifest 全部为 2.0.0。
- [ ] v2.0.0 指向最终验收 Commit，协议和工作空间 schema 单独记录。
- [ ] Windows required jobs、跨平台 smoke、回归矩阵和四客户端实机门槛通过。
- [ ] 实际 tgz 安装通过，禁用脚本也可用，且运行时 npm 依赖为零。
- [ ] Windows amd64 二进制、原生包、addon ZIP、skill 和 checksums 完整且相互匹配。
- [ ] 新 workflow 对应的 npm Trusted Publisher 绑定正确，OIDC 发布来源证明可验证。
- [ ] 实际 publish 消费已验收的 tgz；没有重新构建、重新 pack 或版本漂移。
- [ ] registry 回读和安装 smoke 通过，latest 指向 2.0.0。
- [ ] GitHub Release v2.0.0 附正式产物、变更说明、零旧数据继承说明和验证报告。

以上均是未来实施与发行的检查项。本文写入版本目标不表示当前 1.2.0 代码已经达到 2.0.0，也不修改现有 package.json 或触发 workflow。
