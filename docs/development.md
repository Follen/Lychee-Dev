# 开发指南

架构见 [Design.md](../Design.md)，Native Live 合同见 [协议](live-protocol.md)，当前验收范围见 [todo.md](../todo.md)。

## 环境与运行

当前开发环境为 Windows amd64、Git、Go 1.27.1、Node.js 22.14.0 或以上。Go toolchain 以 `go.mod` 为准，CI 的 Node/npm 版本以 workflow 为准。

```powershell
go build -o .tmp/lycheedev.exe ./cmd/lycheedev
./.tmp/lycheedev.exe describe --format json
```

Lua 测试需要 5.1 解释器，可运行 `node tests/tools/build-lua.mjs` 构建，或通过 `LYCHEEDEV_LUA51` 指定已有文件。

## 验证

代码改动后执行 build、vet 和受影响测试；代码提交完成前运行 Lua 必需的完整套件：

```powershell
$env:LYCHEEDEV_REQUIRE_LUA51 = '1'
$env:LYCHEEDEV_LUA51 = 'C:/path/to/lua.exe'
go build ./...
go vet ./...
go test -p=2 -parallel=4 -count=1 -timeout=20m ./...
```

限制并发是为了避免 500 槽安装测试的大量文件操作互相拖慢。20 分钟为测试包时限，不改变产品预算。

版本、Skill、构建工具或启动器有相关变化时执行对应检查：

```powershell
node tools/version.mjs --check
node tools/skill-contract.mjs
node tools/skill-commands.mjs --check
node --test tools/*.test.mjs
node --test packages/npm/lycheedev/test/launcher.test.mjs
```

完整功能入口 `tools/baseline.mjs` 还需要固定的真实 LuaLS archive/runtime，配置见 [baseline 说明](../tests/baseline/README.md)。缺少输入或跳过检查必须保留为未验证。

| 测试位置 | 负责范围 |
| --- | --- |
| `internal/` | 命令、Source、Data、Live 状态机、安装与持久化 |
| `tests/addon/`、`tests/protocol/` | Lua 5.1、禁用成本、客户端 profile 和跨语言协议 |
| `tests/process/` | 原生进程、发布与竞争 |
| `tests/parity/`、`tests/fixtures/` | 语义对照、最小 fixture 和来源 |
| `tests/dbcd/`、`tests/data-regression/` | 外部数据对照 |
| `tools/*.test.mjs`、npm `test/` | 构建、包装、启动器、升级与发布合同 |

文档和本地产物清理检查路径、链接及涉及的合同，不因此重跑全部实机或完整 Go 套件。已通过的检查仅在输入变化、实际失败或有未解决问题时重跑。

## 开发版实机部署

从当前源码生成配套的开发 stage，再通过受管安装部署：

```powershell
$commit = git rev-parse HEAD
node tools/live-stage.mjs --out .tmp/live-stage --commit $commit
./.tmp/lycheedev.exe addon install --release .tmp/live-stage --installation 'D:/Game/World of Warcraft/_retail_'
```

stage 用于本机验证，不是可发布包。安装后通过公共 CLI 的 `live doctor`、connect、execute、resume、reload、disconnect 操作固定客户端。具体参数读当前 `describe` 和 `--help`。

每次实机任务使用明确的 project 和 PID。中断沿原 CON 恢复，保留原请求和期限；完成报告确认及连接释放后再清理项目产物。截图采用 WGC。首次无可验证 INPUT 时手动重载一次，新增槽位未被客户端发现时重启该客户端。

## 仓库卫生

- 根目录保留 README、Design、Changelog、todo 和 AGENTS；`docs/` 保留当前协议、开发和许可说明。
- Skill 参考、测试用法、fixture 来源及第三方许可随对应代码保留。历史设计通过 Git 查阅，不再复制工作流水或交接日志。
- 临时二进制、stage、截图和实验放在 `.tmp/<task>/`，任务结束后清理。Git 忽略不等于已经清理。
- `.lycheedev` 包含来源引用和恢复记录。先区分活动项目与已结束实验，确认连接已关闭再按用户授权清理。
- 清理 worktree 前核对分支、改动和忽略文件；使用 `git worktree remove` 同步移除登记。明确保留的工作树不纳入清理。
- Windows 递归清理先核实绝对路径和目录链接，范围限定在选定目标；不要使用仓库级 `git clean -fdx`。
- 完成卫生任务后检查 `git status --short --ignored` 与 `git worktree list`，不要为清理再创建封存包或永久临时记录。

## 发布

`release/version.json` 是版本来源，`node tools/version.mjs --write` 同步相关文件。当前开发基线名不会自动修改 TOC、CLI 或 npm 版本。

`release/npm-channel.json` 固定发行渠道；当前政策是发布到 `next`，保留 `latest` 为 2.5.1，改变渠道须有用户明确授权。

发布使用 `.github/workflows/toolkit-release.yml`：固定干净提交，取得所需 CI 结果，校验许可和源码，构建 Windows 包及 corresponding source，完成重建比较、摘要封存和隔离安装；随后 OIDC 发布、回读 registry，再创建 GitHub Release。

已发布包不可修改。提交、推送 main 与发布 npm 分别确认，自动 CI 不代表真实游戏验收。
