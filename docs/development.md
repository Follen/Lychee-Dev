# 开发、验证与发布

当前代码基线为 3.2.0；实际发行版本读取 release/version.json。架构见 [Design.md](../Design.md)，剩余验收见 [todo.md](../todo.md)。

## 开发环境

Windows amd64、Git、Go 1.27.1、Node.js 22.14.0 及以上。Go toolchain 声明位于 go.mod；CI 的 Node/npm 精确版本以 workflow 为准。

Lua addon 和跨语言测试需要 Lua 5.1：

    node tests/tools/build-lua.mjs

也可以通过 LYCHEEDEV_LUA51 指定已有解释器。设置 LYCHEEDEV_REQUIRE_LUA51=1，使缺少解释器时测试失败而非跳过。

## 必要检查

PowerShell：

    $env:LYCHEEDEV_REQUIRE_LUA51 = '1'
    $env:LYCHEEDEV_LUA51 = 'C:/path/to/lua.exe'
    go build ./...
    go vet ./...
    go test -p=2 -parallel=4 -count=1 -timeout=20m ./...
    node tools/version.mjs --check
    node tools/skill-contract.mjs
    node tools/skill-commands.mjs --check

500 槽安装测试包含大量文件操作。限制并发避免与其他包互相拖慢；20 分钟是现有 Windows 安装/进程测试采用的时限，不改变产品运行时预算。

Node 工具和启动器检查：

    node --test tools/*.test.mjs
    node --test packages/npm/lycheedev/test/launcher.test.mjs

完整功能基线入口为 tools/baseline.mjs，额外需要 release/tools/luals.json 固定的 LuaLS archive/runtime。参数与环境见 [baseline 说明](../tests/baseline/README.md)。正式离线套件包括：

| 路径 | 内容 |
| --- | --- |
| internal/* 的 Go 测试 | 命令、选择、Source、Data、安装、状态机、持久化和资源预算 |
| tests/addon、tests/protocol | Lua 5.1、profile、禁用成本、协议编码和业务生命周期 |
| tests/process | 原生进程、文件发布与竞争 |
| tests/parity、tests/fixtures | 语义等价、固定来源和最小回归数据 |
| tests/dbcd、tests/data-regression | 数据读取对照与明确的外部来源 |
| tools/*.test.mjs、npm test/ | 构建、包装、启动器、升级和发布合同 |

实机测试使用公共 CLI，在独立 project 中固定客户端和 PID。先 doctor，再 connect/execute/reload/disconnect；异常沿原 CON 恢复。不得为了测试通过而重置占用、修改日志或重放未知输入。视觉证据使用 WGC。

## 仓库卫生

- 当前文档只有根目录的 README、Design、Changelog、todo，以及 docs 下的协议、开发和许可说明。Skill 参考与测试目录的使用说明各自贴近代码。
- .tmp、dist、发行 stage、截图、运行日志和内存快照不提交。
- 正式回归用最小 fixture，保留来源与授权说明；一次性实验完成后删除。
- .lycheedev 是项目数据，包含未完成连接与证据，不能当作临时目录盲删。
- 不覆盖其他任务的未提交改动，不清理仍被其他任务使用的 worktree。
- 历史版本通过 Git 标签和提交查阅，不再追加逐次工作流水文档。

## 发布

release/version.json 是版本唯一来源，使用 tools/version.mjs --write 同步相关文件。文档中的开发基线名称不会自动更新 TOC 或 npm 版本。

release/npm-channel.json 固定当前版本的渠道。现有政策是 next 发布，并保留 latest 在 2.5.1；只有用户明确要求才能改变。合并 main 不等于 npm 发布。

发布通过 .github/workflows/toolkit-release.yml 执行：

1. 固定干净 Git 提交和版本，确认要求的 CI job 通过。
2. 校验第三方许可和源输入，构建 Windows amd64 可执行文件、配套 addon/Skill 与 corresponding source。
3. 完成重建比较、摘要封存和 --ignore-scripts 隔离安装测试。
4. 通过 OIDC 发布，回读 registry 版本与标签，再创建 GitHub Release。

已发布 tgz 不修改。不得移动已发布标签或用本地测试代替远端发布成功。自动 CI 不运行游戏实机验收。