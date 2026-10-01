# 非 live 查询优化验收

日期：2026-09-30。分支 `codex/data-source-optimization`，基线 `8e4dab0455c8bee512c73912c26b31bc9605f6c2`。
本轮完成 data R1–R8、source S1–S5 及显式启用的跨 CLI LuaLS 会话。
Addon、live 生产代码及发行版本/渠道未修改。

## 冻结分支验证

Windows amd64，Go 1.27.1，Lua 5.1.5，LuaLS 3.19.1；LuaLS archive 与固定 SHA256 相同。
`node tools/baseline.mjs` 于北京时间 14:13:46–14:19:35 完成，状态 **passed**。
使用独立 Go build cache、真实已验证的 LuaLS runtime、当前分支 CLI 与固定 CascLib oracle。

| 检查 | 结果 |
| --- | --- |
| `go build ./...`、`go vet ./...` | passed |
| `LYCHEEDEV_REQUIRE_LUA51=1 go test -json -parallel=4 -count=1 -timeout=10m ./...` | passed，43 packages；记录 2,697 个测试状态，34 个可选环境/helper 跳过，不计作通过 |
| 发行工具、npm launcher、runner Node 测试 | 98 passed，0 failed，0 skipped |
| 版本一致性、Skill command contract、生成命令目录 | passed |
| SOURCE：验证期间的源码 tree SHA256 一致 | passed |
| 新增预算/恢复、Root/index 等价、provider 绑定与缓存篡改 | passed，包含在全量 Go 测试 |
| 真实 LuaLS、跨 CLI broker、配置变化及取消退役 | passed，包含在全量 Go 测试 |
| 固定 CascLib N/Z 内容与 checksum 拒绝对照 | passed，包含在全量 Go 测试 |

证据：[baseline report](../../.tmp/baseline-offline-nonlive-final-20260930/report.json)、
[summary](../../.tmp/baseline-offline-nonlive-final-20260930/summary.md)、同目录各项 stdout/stderr。
验证时源树为 dirty 的开发分支，tree SHA256
`a6e61108b1053ca6356d4c55397f7cf0c0908f94dd83e9b19b6be202021221aa`。
此后只追加验收文档与入口链接，不改生产代码或测试。

二十个 local/offline CDN 冷暖准备场景、固定 decoder intersection 与 tagged storage harness 合同测试
另见 [Data R8](data-regression-implementation-2026-09-30.md)。缺少指标、测试或 required case 的 runner 结果不能成为 verified。

## 保留的失败与环境修复

解码预算初始批次曾改变错误 decoded size 的分类；修正后原有完整性回归与新增预算测试通过。
固定 raw CascLib 缺钥 API 曾发生 access violation，该形状排除在共同对照之外；Go 的缺钥拒绝单独通过。
早期失败输出保留，不被后续成功覆盖。

本机共享 Go cache 中 records、delivery、live、channel、command 的部分归档具有零字节前缀，
`go tool pack` 拒绝其格式，并导致 export-data `bufio: buffer full`。
损坏归档保存在 `.tmp/go-cache-investigation/`；仅删除已确认的这五项缓存文件并重建，默认 cache 构建恢复通过。
冻结 baseline 使用独立 cache，未通过修改生产代码规避此环境错误。

## 验收边界

真实网络冷缓存、实际安装的 FDID/locale/variant storage 对照、完整源码 p50/p95、heap/RSS、
其他 logon 的真实 IPC 攻击与完整 kill/upgrade 矩阵、三客户端互动验收仍为 `not_run`。
逻辑资源计费、fixture 分配统计和 Job commit 上限不能解读成测得的 RSS 下降或加速倍率。
跳过列表在 baseline report 中逐项保存；离线 addon/live fixture 通过不代表本轮操作过真实游戏。
本轮没有部署、游戏输入、合并或发布。
