# Lychee Dev Toolkit

魔兽世界插件开发工具：查源码、读取 DB2/SQL 和 Hotfix、导出资源，以及通过配套 addon 执行实机 Lua 探针。

[项目文档](https://github.com/Follen/Lychee-Dev#readme) · [设计](https://github.com/Follen/Lychee-Dev/blob/main/Design.md) · [更新记录](https://github.com/Follen/Lychee-Dev/blob/main/Changelog.md)

## 安装

需要 Windows amd64 和 Node.js 22.14.0 或以上。

```powershell
npm install --global lycheedev@latest --ignore-scripts
lycheedev version --format json
lycheedev describe --format json
```

启动器没有运行时 npm 依赖。包内包含原生 CLI、addon、Agent Skill、LuaLS 工具资源、发行清单和许可文件。

## 部署组件

安装 npm 包后，按需要部署游戏插件和 Skill：

```powershell
$release = Join-Path (npm root -g) 'lycheedev'
lycheedev addon install --release $release --installation 'D:/Game/World of Warcraft/_retail_'
lycheedev skill install --release $release --path (Join-Path $env:USERPROFILE '.agents/skills/lycheedev')
```

Source、Data 和 Assets 无需启动游戏。Live 需要运行中的客户端和配套受管插件；恢复时保留原 project、CON、request 与预算。

本包版本为 3.2.1，通过 `latest` 渠道发布。以已安装版本的 `version`、`describe` 和 `--help` 确认能力。

项目代码采用 MIT，第三方组件声明见包内 LICENSE 与 THIRD_PARTY_NOTICES。
