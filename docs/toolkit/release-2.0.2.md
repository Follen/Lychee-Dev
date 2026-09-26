# Lychee Dev Toolkit 2.0.2：Windows CI 与 npm 发布合同

状态：2.0.2 候选，尚未发布。日期：2026-09-23。2.0.1 已发布，其标签、
npm 包和发行字节不可改写；历史发布与恢复记录见
[2.0.1 合同](release-2.0.1.md)。本文件只约束 2.0.2 候选及其最终发布。

## 产品范围

- 唯一发行平台是 Windows amd64；不构建、不发布、不验收其他平台。
- 插件验收客户端是 Retail `120100`、Classic `50504`、Titan `38002`。
  Forever `16001` 保留源码和 TOC，但不声明为已验收支持端。
- 产品由一个 Go CLI、一个 Lua addon、一个 agent skill 和一个零运行时依赖的
  npm 分发包组成。源码、数据、资源和 live 调查共享 `~/.lycheedev`、固定引用
  和证据链；旧工作空间、旧 SavedVariables 与旧 Python 入口不导入。
- 游戏内人工验收由项目所有者单独记录。托管 CI 不伪装为交互桌面或真实游戏，
  也不把离线 Lua fixture 写成真机结果。本轮静态收口不执行真机操作。

## 2.0.2 增量合同

- live 公共流程按原子操作拆分：`live probe put` 注册不可变 revision，
  `live probe load` 只加载，`live run <operation-id>` 只推进到 verified report，
  `live ack <operation-id>` 精确回收报告、队列项和窗口所有权。
- ACK 不触发 cleanup reload。`live reload` 是独立操作；reload 导致 readiness
  变化时，CLI 自动重新连接。用户不手输 `/dev connect`。
- `live bugs` 读取已有 BugGrabber 错误，不制造探针或错误补足数量。
- 同一游戏窗口只有一个 writer；源码与数据查询可并行。多实例或多安装候选必须
  以产品、Build、角色、服务器和窗口身份消除歧义，不能按目录名或 ordinal 猜测。
- 后台输入只向已绑定 HWND 使用有界 `PostMessageW`；不激活前台、不使用
  剪贴板或 `SendInput`。真实窗口证据使用 WGC，不用屏幕截图替代 D3D 捕获。

## 版本与产物

`release/version.json` 是唯一版本源，目标值为 `2.0.2`。
`node tools/version.mjs --write` 同步 npm package/lock、单一平名清单
`addon/Lychee Dev.toc`、Lua Runtime、协议 fixture 和 Go buildinfo；
`--check` 在 CI 拒绝漂移。

标签必须是 `v2.0.2` 并指向最终验收提交。发行物包括：

- 公开的 `lycheedev@2.0.2` tgz，零 runtime dependencies；
- Windows amd64 原生 ZIP；
- addon ZIP 与 skill payload；
- 对应源码归档、许可文件、`release.json`、sealed manifest 和 SHA256SUMS。

npm launcher 只选择包内 Windows amd64 二进制并转发 argv、stdio 和退出码；
不下载 fallback，不在安装期部署 addon/skill。`npm install --ignore-scripts`
必须可用。正式 2.0.2 使用 `latest`；若先发行 `2.0.2-rc.N`，只能进入
`next`，不得用正式版本号试装。

## 必需自动门禁

1. 工作树在候选提交上干净，`node tools/version.mjs --check`、命令目录、skill
   合同和第三方许可清单一致。
2. 托管 Windows required jobs 全绿：`windows-contract`、`windows-process`、
   `windows-addon`、`windows-package`、`ci-required`。Linux 交叉编译不能替代。
3. `go build ./...`、`go vet ./...` 与
   `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` 通过；Lua 5.1、协议、
   进程、安装、wowdata/wowdoc 迁移回归均包含在内。
4. `tools/release.mjs assemble` 完成许可门禁、Windows amd64 构建、对应源码
   重建比较、addon ZIP 和实际 npm tgz 审计。只组装和 pack 一次。
5. 封存摘要后，使用同一 tgz 做隔离 `--ignore-scripts` 安装、版本/describe
   启动、payload 安装与移除 smoke；不得重新 pack 一个“等价”包发布。
6. release-gate 重算全部摘要，核对 Windows run evidence 和 required jobs。
   不设置 self-hosted interactive desktop、desktop-evidence 或真机 CI 门禁。
7. npm 只通过 Trusted Publishing OIDC 发布，不保留 token fallback。发布前读取
   精确 name/version/integrity；未知、超时或冲突都停止，不能当作不存在。
8. 发布后有界重试 registry 可见性，核对 version、integrity、provenance 和隔离
   安装；确认后再创建 GitHub Release 并上传与 sealed manifest 一致的资产。

## 失败恢复与不可变性

- CI、组装、许可或 sealed gate 失败：不发布。
- publish 返回不确定：先读 registry；相同 integrity 则继续后验，冲突则停止，
  绝不重复发布相同版本。
- npm 已成功但后验失败：记录恢复义务，补齐可恢复步骤；代码或字节变化使用新
  版本，不能覆盖 2.0.2，也不能移动已有标签。
- GitHub Release 缺失时只能在重新验证 sealed artifact 后补齐，不重新 publish。
- 发布完成后 tgz、tag、release assets 与摘要不可替换。

## 发布前清单

- [ ] 当前提交已完成全部非真机回归，结果写入 implementation-status。
- [ ] 需要的人工真机验收已由所有者记录；未运行项目保持 `not_run`。
- [ ] Windows required jobs 与实际 tgz 隔离安装全部通过。
- [ ] 许可、对应源码、版本、命令目录、skill 和发行 manifest 无漂移。
- [ ] registry 中不存在冲突的 `lycheedev@2.0.2`。
- [ ] OIDC publish、registry 回读和 GitHub Release 使用同一 sealed tgz 完成。

在以上项目完成前，2.0.2 只能称为候选，不得称为已上线或可发布。
