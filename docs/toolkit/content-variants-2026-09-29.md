# CASC 内容变体选择与验收

日期：2026-09-29。`codex/model-content-variants` 实现，纳入 3.0.2 候选；
npm 3.0.1 不包含新参数。发布门禁见 [3.0.2 合同](release-3.0.2.md)。

## 问题与合同

同一 FileDataID、同一 locale 可以有不同 CKey 的多个 Root 记录。原实现严格
拒绝歧义是正确的，但缺少调用方明确选择内容变体的入口。

`asset inspect` 和 `asset export` 新增
`--content-variant standard|low-violence`。只筛选 `ContentFlags & 0x80`：
standard 要求为零，low-violence 要求非零。省略参数保持原有严格行为。
指定变体不存在时报缺失，不回退到另一变体或其他 locale；筛选后仍有多个记录
时继续报 `records.file_ambiguous`，即使 CKey 相同。不解释、排序其他 flag。
此参数不是完整客户端 Root 选择算法，不承诺模拟运行中的游戏配置。

保持固定 PIN、安装身份、Root/Encoding/BLTE/CKey 验证和输出完整性门禁。
导出证据 `source.contentVariant`（inspect 为 `contentVariant`）记录明确选择，
Root entry 保留完整 flags、group、CKey。失败不覆盖已有输出。

`0x80` 的依据是固定提交的 [CascLib 定义与 Root 实现](https://github.com/ladislav-zezula/CascLib/blob/38a34665624b8775bb875274b36191b21c38d97b/src/CascRootFile_WoW.cpp)，
其中 LOW_VIOLENCE 与 overrideArchive 相关；
[WoW-Tools Root 实现](https://github.com/WoW-Tools/CascLib/blob/3f8be478177802de4ae7ebae24fb25ab860ef104/CascLib/RootHandlers/WowRootHandler.cs)
也将 locale 与内容选择分开处理。历史实现不能证明 Forever 客户端的当前默认值。

## 固定目标与真实数据验证

- PIN：`PIN-7426aa539f5e923c3c6a4131bfaecb083a9534067b024be2dd28a414017ff777`
- Forever US/zhCN，`1.60.1.70009`。
- BuildConfig：`05215079e3905ef5922ae0b03ffefb73`。
- CDNConfig：`7b0257bf0fe52d0f11cd8e7b71f7f832`。
- Root SHA256：`62a568c6272dae7e798cd3080d3748e1e7789e247f31e632ae9528cf77f4aa91`。
- 只读输入：`D:/Code/wowclassic/foveverup/analyze/navigation-1-20/tool-blockers-all.json`。
  208 次失败事件对应 153 个唯一 ID。

153 个 ID 各导出两种变体，306 份全部通过真实 CLI、本地 CASC 和原有完整性校验，
再独立复算原始输出 MD5 对照 CKey、SHA256 对照导出清单。剩余导出错误 0。
standard 为 `0x120c0000`，low-violence 为 `0x120c0080`；153 对 CKey 均不同。
未由此推断碰撞几何或外观的具体差异。导航任务另外解析并比对碰撞，回报
102 对一致、51 对不同，结果为导航项目的 `variant-collision-comparison.json`；
工具任务只读检查了该报告，没有重复解析或将两变体声明为相同几何。

测试输出只写本 worktree 的 `.tmp/model-variants-20260929/`，没有改动导航项目
的任何地形、场景、模型或错误报告。该目录保留：

- `models-report-all.json`：306 项结果、flags、CKey、SHA256。
- `handoff-manifest.json`：逐项绝对文件路径及上述结果。
- `verified-models/<ID>-<standard|low-violence>.raw`：原始模型。
- 对应 `.json`：完整 CLI 参数、原始响应和退出状态。
- `lycheedev.exe`：本轮实测候选 CLI；没有覆盖全局安装。

客户端配置仅明确 `textLocale=zhCN`、`audioLocale=zhCN`、`portal=test`，
没有显式 overrideArchive。没有读取运行时 CVar，因此实际客户端变体仍为 unknown；
不能从中文语言或地区推定。

## 回归入口

真实离线 CLI fixture 覆盖双变体、相反 Root 顺序、默认歧义、非法参数及 CKey
损坏时保留已有输出；读取层覆盖缺失不回退、相同 CKey 重复、其他 flags 歧义
及 locale 隔离。完整离线基线输出位于上述临时目录的 `offline/`，
其实际报告是基线通过与否的依据，不把模型导出成功视为全套测试成功。

本轮没有操作游戏输入，不将静态文件导出验证扩展为 Forever 实机功能验收。
