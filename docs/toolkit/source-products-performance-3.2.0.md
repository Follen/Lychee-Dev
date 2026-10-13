# Source 产品映射与查询优化（3.2.0 开发线）

2026-10-13。Go 实现，位于 `.worktrees/3.2.0`；版本文件仍为 3.1.2，未提交、未发布。
本页补充设计、状态和回归合同，之前的 Source 优化记录保留其历史验证范围。

## GitHub 实际分支

本次读取 [Gethe 分支列表](https://api.github.com/repos/Gethe/wow-ui-source/branches?per_page=100)，
并使用 CLI 对七产品执行真实 `source sync`，固定得到以下来源。

| 产品 | Git 分支 | 本次固定 commit |
| --- | --- | --- |
| retail | live | `09b9db7948abc9b9648dedaab51eb0cf3ee67b31` |
| classic | classic | `ea620b2f7a2ec8e9c446db102cc74641931e674e` |
| classic-era | classic_era | `8165d4cd6e48d606369336cc3a7977902310e81e` |
| titan | classic_titan | `25ab1ea3631ac33b0c900b495bd61c8c1f3fbd57` |
| forever-asia-beta | forever | `943764493e6b16d63ded3ab304150d1f05e58b57` |
| forever-cn-beta | forever | `943764493e6b16d63ded3ab304150d1f05e58b57` |
| anniversary | classic_anniversary | `2cfec7c00f0f24762d5ca35a4beb1e93c0cde328` |

[`forever` 的本次提交](https://github.com/Gethe/wow-ui-source/commit/943764493e6b16d63ded3ab304150d1f05e58b57)
为 `1.60.1 (70338)`；镜像没有单独的国服/亚服源码分支。
其 [导出配置](https://github.com/Gethe/wow-ui-source/blob/943764493e6b16d63ded3ab304150d1f05e58b57/.github/workflows/export.yml)
使用 `wow_classic_beta`。
旧 [`classic_beta` 分支](https://github.com/Gethe/wow-ui-source/commit/4d9399a664eb65f2c8d5e96d7a6bf5e234414239)
最后提交为 2025-11-03 的 `Disable cron-based export`。本机 `_classic_beta_` 是交付目录，
不能据此选择同名旧 Git 分支。

两个 Beta 的 Source 轨道均指向 `forever`。使用区域名称 sync 时，pin 保留选定的区域产品名；
已有通用 `forever` Source pin 也可分别与两种区域 Data pin 组合。
亚服 Source pin 不能隐式变成国服 Data pin，区域 Data/Changes 之间仍要求精确一致。
两个 Data build configuration 继续分开保存；共享镜像 commit 不证明国服、亚服客户端源码逐字节相同。
精确旧基线的 Interface 可通过 Forever 家族映射复用；未知 commit 仍不冒充已验证运行时基线。
本次没有扩展 addon/Live 客户端验收范围。

## 已实现

### 名称、路径与关系查找

沿用认证的 `offsets.json` 和 records 格式，请求内构建指向原记录序号的查找表。
名称、关系目标、限定名后缀、文档路径和规范化加载目标可直接定位；
符号 ID 与声明路径在首次需要时建立，前缀名称在首次前缀查询时排序。
并集去重后按原记录顺序访问，保持消歧、排序和分页。

precise 查询依次尝试 exact、prefix、索引文本、正文；命中高层后不扫描低层。
低层未访问候选的截断不再影响已命中的 exact 结果。exploratory 和任意文本搜索仍扫描相关元数据，
不声明所有文本查询均为常数时间。Context 的 callee、load、深度 frontier 与语义 definition 投影
只访问相应名称、路径和相邻关系；原来超出 sidecar 预算的完整扫描回退仍可使用。

快照初次打开仍验证完整 records 和 sidecar；本次减少的是查询阶段反复遍历。
offset 解码直接写入预分配 slice，避免逐条临时对象分配。
查找表从同一认证 metadata 构造，不新增磁盘索引或迁移旧缓存。

### 多问题批量请求

```text
lycheedev source batch --snapshot <pin> --file requests.json --format json
```

请求文件至多 64 KiB，包含 1–32 项，每项恰好一种查询：

```json
{"requests":[
  {"id":"find","query":{"text":"C_Spell.GetSpellInfo","topic":"api","limit":10}},
  {"id":"refs","refs":{"symbol":"C_Spell.GetSpellInfo","direction":"incoming","limit":10}},
  {"id":"context","context":{"symbol":"C_Spell.GetSpellInfo","depth":2,"limit":10,"maxBytes":16384,"maxLines":120}}
]}
```

固定一个 Source pin，所有项共用其快照、查找表、Git batch、文档、API 环境和 LuaLS worker。
API 环境 memo 最多四个；正常 worker 随请求结束回收，显式 `--session-reuse` 继续使用既有 broker。
`--environment` 为第三方源码选择固定客户端 API 环境；`--static-only` 返回结构证据。
整个 CLI 请求共享既有 90 秒截止时间，每项保留自己的 coverage、truncated、complete 与 nextCursor。
结果按输入顺序返回，`id` 原样回传；一份 capture 保存整批结果。
任一项出错时命令失败并指出项序号/ID，不把已执行的前半批宣称为成功交付。

各查询沿用模块默认值：query limit 10；refs/context limit 25；context depth 0、maxBytes 16384、
maxLines 80。JSON 中可以明确给出这些参数；跨独立 CLI 续页时必须使用相同参数和 cursor。
命令参数错误包含非法批格式、查询 topic/mode/limit、关系方向和上下文范围，映射退出码 2。
固定来源损坏、外部失败和取消继续保留各自错误分类。

### 行位置缓存

同一文档缓存字节行起点，只把所需行转成字符串，之后计算 LuaLS 的 UTF-16 列。
行缓存最多 256 文件、8 MiB；保留 CRLF、末尾空行、越界与同名位置歧义语义。
一次 128 个同文件 outgoing 位置查询只有一次 document 读取和一次行索引构建。

## 验证与耗时

fixture 验证已通过：

- query + refs + context 共用一次完整认证、一次查找表构建和一个 Git batch 进程。
- 2,007 条事实的 fixture 中，三次精确/前缀查找加深度 4 上下文只访问 28 条候选元数据；
  查找表首次构建遍历全体 2,007 条，单独计数，不隐藏准备开销。
- 批量结果、分页、部分覆盖与逐项执行等价；旧扫描回退、损坏 sidecar、同句柄改写和取消检查通过。
- 真实 LuaLS fixture 的两个不同符号查询共用一个 worker/API 环境，并在请求结束后关闭。
- 两个区域身份、家族 Source 兼容与区域错配反例通过；七产品真实 Git sync 通过。

完整 Retail Source 使用上表的固定 commit：4,036 个文档、65,330 个声明、227,816 条关系，
索引覆盖 complete。使用改动前的开发 CLI 与当前 CLI 读取同一个已准备快照，
两种 binary 均先预热，再逐项测量一次；这是本机暖查询样本，不是 p50/p95 或冷启动统计。
关系/上下文耗时采用 `--static-only`，不包含完整 Retail LuaLS 分析。

| 相同问题与参数 | 改动前独立 CLI | 当前独立 CLI |
| --- | ---: | ---: |
| C_Spell.GetSpellInfo / api | 733 ms | 694 ms |
| C_Spell.GetSpellCooldown / api | 720 ms | 723 ms |
| C_UnitAuras.GetAuraDataByIndex / api | 742 ms | 739 ms |
| UnitHealth / api | 676 ms | 686 ms |
| GetSpellInfo refs / both | 723 ms | 740 ms |
| UnitHealth refs / incoming | 785 ms | 758 ms |
| GetSpellInfo context / depth 2 | 1,022 ms | 856 ms |
| 七项合计 | 5,401 ms | 5,196 ms |

同样七项由新 `source batch` 执行 **870 ms**，比原独立调用合计减少约 84% 的时间。
逐项结果、coverage 与 cursor 均相等。单条短查询基本持平；主要收益来自多问题共享准备和较深关系查询。
早期查找表为不需要的关系 ID/路径建立索引，曾增加单条查询成本；最终版本按需建立声明索引，
同时消除 offset 解码临时分配。新 binary 首次运行开销不混入上述暖查询样本。

本地日志位于 `.tmp/source-3.2.0/`：`performance.json`、`measure-source.mjs`、`requests.json`、
各产品 `*-sync.json` 和 `retail-index.json`。这些是开发验证产物，不进入发行包。

最终 `go build ./...`、`go vet ./...`、强制 Lua 5.1 且启用真实 LuaLS 的
`go test -count=1 ./...`、Source 定向 `-race` 均通过。Skill 生成参考与命令合同通过
（90 个命令、219 处引用，无违反项），版本一致性与 `git diff --check` 通过。
全量 Go 日志为 `.tmp/source-3.2.0/full-go-test.log`。
这里不声称完成 Live、游戏实机、发布或 npm tag 变更。
