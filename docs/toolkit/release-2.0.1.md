# Lychee Dev Toolkit 2.0.1：Windows CI 与 npm 发布合同

状态：2.0.1 候选；以发布标签对应的提交和 CI 结果为最终证据。2.0.0 已发布，
其发行字节和标签不修改。历史 2.0.0 方案见 [旧版合同](release-2.0.0.md)，
其中五平台、交互桌面 CI 和四客户端实机门禁不适用于 2.0.1。

## 产品与验收范围

- 唯一发行平台为 Windows amd64；Go 代码的可移植性不构成其他平台的产品承诺。
- 插件支持 Retail `120100`、Classic `50504`、Titan `38002`。这三端的
  TOC、Lua 5.1、locale、事件目录和协议 fixture 属于托管 Windows CI 验收。
  Forever `16001` 的源码及 TOC 保留，但未经真机验证，不列为 2.0.1 支持端。
- 游戏内工作台的人工真机手测由项目所有者完成并在
  [实施状态](implementation-status.md)记录。此项不经 CI 伪装为自动测试；
  没有交互式桌面 runner 时，发行工作流不等待桌面 artifact。
- CLI 自动完成候选识别、`/dev connect` opt-in 和新鲜 ready 回执核验；
  用户不手输连接命令。清理重载使旧 session 失效，下一次由
  `lycheedev live connect` 自动重建。仅真实歧义交由用户选择。

## 版本与产物

`release/version.json` 是唯一版本源，目标为 `2.0.1`。
`node tools/version.mjs --write` 同步 npm package/lock、四个 TOC、
Lua Runtime 和 Go buildinfo；`--check` 在 CI 拒绝漂移。
通信及工作空间 schema 独立版本化，不因补丁发行重置用户数据。

标签 `v2.0.1` 必须指向完成验收的准确提交。发行产物是一个公开、
零运行时 npm 依赖的 `lycheedev@2.0.1` tgz、一个 Windows amd64 原生 ZIP、
一个插件 ZIP、对应源码包、许可说明、`release.json`、封存 manifest
和 SHA256SUMS。npm 启动器在非 Windows amd64 平台拒绝运行；
`npm install --ignore-scripts` 仍可安装包本身。
发行清单固定提交、Go/Node/npm 工具链、二进制和资源摘要。

## 自动发行门禁

1. `.github/workflows/toolkit-ci.yml` 在托管 Windows 上执行 Go build/vet/test、
   Lua 5.1 协议和三个支持客户端的离线矩阵、跨进程与安装测试、版本/命令/
   skill 合同、插件 ZIP 与 npm tgz 内容审计。平台 smoke 在
   `.github/workflows/toolkit-release.yml` 的 Windows amd64 job 执行。
2. tag push 触发发行；手动 `workflow_dispatch` 只允许 dry-run。
   发行工作流冻结版本和提交，验证许可、`CGO_ENABLED=0` 所需的可运行
   解码测试、对应源码重建比较，然后只组装和 `npm pack` 一次。
3. 封存摘要并以实际 tgz 做隔离安装 smoke。release-gate 重算摘要、
   验证 Windows amd64 的运行报告以及必需 CI job。没有 self-hosted
   interactive desktop job、desktop-evidence artifact 或桌面发布门禁。
4. 发布 job 只消费封存的 tgz，以 npm Trusted Publisher OIDC 发布到
   `latest`；不使用长期 npm token 作为失败回退。发布前查准确
   `name/version/integrity`：超时或未知不得视作不存在，已有不同字节时停止。
5. 发布后核对 registry 版本、integrity、provenance 和隔离安装；
   npm 接受发布后可能暂不可见，只对回读做有界重试，绝不重复 publish。
   再用 `GH_TOKEN` 和 `contents: write` 创建或补全 GitHub Release
   `v2.0.1`。已发布的 tgz 和标签不可替换。

失败恢复：CI 或封存失败时不发布；publish 返回不确定结果时先读 registry，
相同完整性则续做回读，冲突则停止。npm 已成功而后验失败时记录未完成，
修复改用新版本，不覆盖 2.0.1。GitHub Release 缺失时可在确认摘要后补齐，
不重新 publish。

## 人工验收记录

项目所有者确认已完成游戏内真机手测。该结论是人工验收，不是 CI 产物；
实际客户端 Build、逐项截图和 WKB 记录如果尚未归档，应在
[实施状态](implementation-status.md)保持“用户确认／细节未归档”的区分。
发行 CI 以可在托管 runner 稳定执行的自动检查为门禁。

## 发布前核对

- [x] `node tools/version.mjs --check`、完整 Go/Lua 和工具测试通过。
- [x] `v2.0.1` 指向已合并主分支的验收提交，工作树干净。
- [x] Windows amd64 必需 CI、发行组装、运行 smoke、封存摘要全部通过。
- [x] npm Trusted Publisher 绑定本仓库和 `toolkit-release.yml`。
- [x] registry 状态确认 2.0.1 可发布，且不存在不同字节冲突。
- [x] 同一 tgz 完成 OIDC 发布、registry 回读和 GitHub Release；最后一步按下述记录人工恢复，标签 workflow 未全绿。

## 发布后验与恢复记录（2026-09-23）

`v2.0.1` 的提交为 `baa83e9d7ec80091dce5de68c4295bb233139613`。
标签发行 run `35843639061` 的 Windows CI、组装、隔离安装 smoke、封存
和 release-gate 均通过；npm OIDC publish 成功，npm 明确提示新版本仍在
处理。首次即时 `npm view` 返回 404，等待可见后重跑失败任务，registry
摘要、provenance 与隔离安装均通过，且重跑跳过了 publish。

重跑在创建 GitHub Release 时因 job 未设置 `GH_TOKEN` 再次失败。
使用该 run 的 `sealed-bundle` artifact，先通过 `verify-sealed`，再核对
registry integrity 与 tgz 字节一致，最后手动创建
[`v2.0.1` GitHub Release](https://github.com/Follen/Lychee-Dev/releases/tag/v2.0.1)。
六个 Release 资产已由 GitHub Releases assets API 确认处于 `uploaded`
状态，摘要与封存 manifest 对应。`latest` 指向 `2.0.1`。

原标签 run 因后验自动化失败保持红色，不能记作全绿。主分支后续提交
补上有界 registry 可见性重试、Release 所需的 `GH_TOKEN` 与
`contents: write`；不移动标签、不覆盖 npm 版本，也不将后续修复
倒填为标签提交内容。
