# Lychee Dev Toolkit 2.0.6：Windows CI 与 npm 发布合同

日期：2026-09-27。2.0.5 已发布；本文件约束 2.0.6 候选。历史标签与 npm
产物不可改写。用户授权修复桥接审计问题、发布 npm 并安装本机组件。

## 范围

- B01：受管旧运行态升级到当前 CLI 版本，保留 actor/window/nonce/epoch，
  每次输入前复核目标 manifest；旧 session 的升级前后连接与公开请求幂等。
- B02/B03：原子 probe/bugs 的同报告 ACK 幂等重取；零/部分/未知输入和
  永久丢失 ACK 的明确 host-only abandon；保留原观察，不重执行探针。
- B04：战斗、loading、聊天交错后重新验证并恢复回执；明确隐藏、失效 actor
  或旧 callback 不复活画面。B05：小二维码分数像素采样，不改变显示尺寸。
- B06：自动有界 ROI、实际区域存储；显式 window 保持全窗及解码上限。
- B07/D2：Skill 不把未观察到等同未执行，恢复有边界的性能调查方法。
- D1：`live finish` 机械完成 ACK→hide→连续有效帧确认→归档清屏证据。
  verified 报告不因清屏 pending 丢失，成功后同 operation 重试不再输入游戏。

## 产品和兼容边界

唯一发行平台为 Windows amd64；产品仍为 Go CLI、Lua addon、Agent skill 和
无 runtime dependencies 的 npm 包。Retail 120100、Classic 50504、Titan 38002
为验收矩阵；Forever 16001 保留但不升级为已验收支持声明。

普通业务仍要求当前 release；升级例外只属于受管 reload。运行态 ACK 缓存
不跨任意 reload/login 保证存活；不可取得新鲜确认时必须保留 pending，显式
放弃也不证明 ACK 未发生或运行态已经清空。旧 pre-atomic 记录保留观察恢复。

## 必需门禁与发布顺序

1. `release/version.json` 唯一版本源为 2.0.6；`tools/version.mjs --check`、
   generated commands、skill、许可、源码输入和 clean tree 均通过。
2. 本地 `go build ./...`、`go vet ./...`、Lua 5.1 必需的
   `go test -count=1 -parallel=4 ./...`，以及 Node tools/npm launcher 测试。
3. `v2.0.6` 指向最终验证提交；Windows required jobs 全绿（contract、process、
   addon、package、ci-required），保留 race 与真实原生窗口 fixture。
4. Release workflow 只 assemble/pack 一次；对应源码重建比较、许可审计、
   Windows run smoke、封存摘要、隔离 `--ignore-scripts` 安装验收。
5. OIDC 发布同一 sealed tgz，无 token fallback；npm registry read-back、
   隔离安装和 GitHub Release 成功后才称为已发布。稳定版使用 latest。
6. 从 registry 安装本机 CLI，再从该包 release root 用官方 installer 更新
   受管 skill 和 addon，逐个核验 receipt；不以覆盖拷贝制造 managed 状态。

## 验证记录

定向 RED/GREEN 及审计边界见 [审计记录](bridge-regression-audit-2026-09-27.md)。
完整自动回归日志保存在 `.tmp/release-2.0.6-go.log` 和
`.tmp/release-2.0.6-node.log`。Go/Lua 全仓库回归已通过（live 138.403s、
protocol 14.523s，Lua 5.1 required）；build/vet 通过。Node 工具与 launcher 共 51 项通过；skill
合同为 80 commands / 192 references，无违规。

本候选真机游戏验收在发布前尚未执行。合成缩放、Lua 事件和 Windows 测试窗口
不能替代真实游戏；发布后安装及运行态激活结果单独记录，不预先写为 passed。
Hosted CI 没有自托管交互桌面门禁，完整工作流参照
[不可变发布合同](release-2.0.5.md) 和 `.github/workflows/toolkit-release.yml`。
