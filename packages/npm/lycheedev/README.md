# Lychee Dev Toolkit

用于魔兽世界插件开发的原生 Go CLI：Source 调查、DB2/SQL、Hotfix、资源导出和游戏内 Live 验证。

[项目文档](https://github.com/Follen/Lychee-Dev#readme) · [开发基线](https://github.com/Follen/Lychee-Dev/blob/main/Design.md) · [更新记录](https://github.com/Follen/Lychee-Dev/blob/main/Changelog.md)

需要 Windows amd64 和 Node.js 22.14.0 及以上。启动器没有运行时 npm 依赖，不下载替代可执行文件。

    npm install --global lycheedev@next --ignore-scripts
    lycheedev version --format json
    lycheedev describe --format json

包内包含原生 CLI、addon、Agent Skill、LuaLS 工具资源、发行清单与许可文件。npm 安装不会自动部署游戏插件或 Skill；组件安装与升级通过 CLI 显式执行。

    $release = Join-Path (npm root -g) 'lycheedev'
    lycheedev addon install --release $release --installation 'D:/Game/World of Warcraft/_retail_'
    lycheedev skill install --release $release --path (Join-Path $env:USERPROFILE '.agents/skills/lycheedev')

仓库当前文档以 3.2.0 开发基线为准，可能领先于本包。以 version 和 describe 确认已安装版本的能力；Live 需要配套的受管插件与运行时。

Source、Data 和 Assets 无需启动游戏。Live 使用固定客户端和持久连接，恢复时保留原 project、CON、request 及预算。当前协议与已验证产品范围以对应版本文档为准。

项目采用 MIT；第三方许可见包内 LICENSE 和 THIRD_PARTY_NOTICES。