# 3.2.0 Data 产品与来源

状态：开发实现；真实读取结果记录于 implementation-status.md。用户于 2026-10-13
要求先完成 Data，再继续 Mailbox 输入协议、500 槽和全客户端实机验收。

## 产品目录

`internal/selection/products.go` 的 `DataProducts()` 是交付产品目录。
目录名只用于发现候选；`.flavor.info`、有效 `.build.info` 行和配置内容确定身份。
`VerifiedClientBaselines()` 保留已有精确源码/API 验证事实，不能用 Data 可读推断 addon 或 Live 已通过验收。

| CLI product | 客户端目录 | launcher / versions / cdns 产品键 | 本机完整 build（2026-10-13） |
| --- | --- | --- | --- |
| retail | `_retail_` | `wow` | 12.1.0.69933 |
| classic | `_classic_` | `wow_classic` | 5.5.4.70268 |
| classic-era | `_classic_era_` | `wow_classic_era` | 1.15.9.70003 |
| titan | `_classic_titan_` | `wow_classic_titan` | 3.80.2.70315 |
| forever-asia-beta | `_classic_beta_` | `wow_classic_beta` | 1.60.1.70338 |
| forever-cn-beta | `_cn_beta_` | `wow_cn_beta` | 1.60.1.70338 |
| anniversary | `_anniversary_` | `wow_anniversary` | 2.5.6.70006 |

这些本机观测来自 `D:/Game/World of Warcraft/.build.info` 及各客户端的 `.flavor.info`。
初次盘点时周年服本地为 2.5.6.69795；正式解析、读取时本地 `.build.info` 已更新为 2.5.6.70006，
本轮本地和远端内容验收均为 70006。每次本地解析仍以安装元数据为准，不跟随远端当前 build。
两个 Forever beta 的版本号相同，但当前 BuildConfig 分别为
`f6e309c700cea095978aeb5d85210df4`（亚服）和 `53cd6d7e54e80a8c38d3eab6e7a6cfbe`（国服）。
产品、BuildConfig、CDNConfig、region、locale、DBD commit 一起确定固定 DataPin。

## 远端

`target available --region <region>` 列出七个产品各自在该地区的版本清单。
`target resolve --product <product> --region <region> --locale <locale>` 从该产品的
`versions` 和 `cdns` 解析固定身份；`--build` 是精确约束，不搜索邻近版本。
`data db2`、`db2` 子命令、SQL、文件和资源查询复用同一产品路由和固定配置。

CN 版本服务为 `https://cn.version.battlenet.com.cn/<product-key>/`；其余地区为
`https://<region>.version.battle.net/<product-key>/`。归档主机和路径取自对应 `cdns` 清单。
地区是调用者选择，不能从中文对话、目录或 product 名自动替换。缺少该地区版本行时明确不可用，
不改查另一产品。2026-10-13 亚服 beta 清单有 us/eu/kr/tw，国服 beta 有 us/cn，Titan 有 cn。
七种产品的 versions 和 cdns 均已在线读取；这只证明路由和元数据可用，内容读取另验。

若没有可用的 loose 文件或聚合索引，归档索引按清单顺序判定结果，网络查询最多并发八个。
不超过 256 KiB 的单个索引用一次完整 range 同时获取 footer、TOC 与目标页；大型索引保持稀疏读取。
并发共享同一次查询的网络和内存预算，完整性失败保持原顺序语义，默认额度不变。
已保存的完整索引及旧版分片缓存均可离线使用。

## 本地

`target resolve --installation <game-root-or-client> --product <product>` 按产品筛选真实安装；
显式客户端路径不能跳到兄弟目录。省略 product 时，读取已选择客户端的元数据，
Forever 的 `catalogProduct` 转为明确的亚服或国服 DataPin。共同的 Lua API 家族仍可叫 `forever`。

查询既接受游戏根目录，也接受客户端目录。客户端身份及精确 CASC 行必须与 pin 相符；
两个 beta 即使 build 相同也不能互借配置或读兄弟客户端。根目录按 pin 的产品键选择 `.build.info` 行。
本地缺文件、缺密钥或定义不匹配返回实际缺口，不自动切换 CDN。

本地 CASC 读取优先访问文件 EKey 前九字节折叠 XOR 对应的索引桶，避免冷查询先加载无关桶。
若目标不在首选桶，继续既有索引查找顺序，兼容已观察到的其他布局。索引代次、校验和、完整 EKey
认证和查询完成前的文件复核保持；默认资源预算不提高。Retail 空缓存回归已确认修复原 `metadata_bytes` 失败。

原有 `forever` 输入保留兼容：远端仍指向旧有 `wow_classic_beta`；本地它是家族筛选，
两个 beta 同时存在时返回歧义。已保存旧 pin 不改写，新本地解析尽可能保存明确的 beta 产品。
新任务优先使用明确的七个产品名。旧元数据仅有 `wow_forever` 时保留其旧身份，不按目录猜地区。

## 验证

定向回归覆盖七产品本地解析、真实 flavor 格式且没有 version.txt 的安装、同 build 的两个 beta、
错误兄弟目录拒绝、不同在线/离线 catalog 及 CDN route 缓存隔离。真实内容验收分别保存本地和远端 pin，
每个产品读取一张有明确 DBD 定义的 DB2 表。查询成功、分页截断和来源覆盖分别报告。
Data 验证不发送游戏按键，也不代替后续 addon/Live 验收。

本轮全部查询固定 DBD commit `d6f50d4f351193e84226aa62a36faa0f58528afa`，读取 `ChrRaces`
的 ID 1、2，`contentVerified=true`。分页输出明确 `truncated=true`，不代表完整表验收。
公共密钥快照为 `71b75360752840dd62a412013a38b5a972524a50`。
七种本地来源及七种 CDN 来源均已完成真实读取；每一对对应快照的 `ChrRaces` 内容 SHA-256 相同。

| product | 本地 / CDN 验证 build | 本地读取 | CDN 读取 |
| --- | --- | --- | --- |
| retail | 12.1.0.69933 | passed；修复后空缓存 1.4 秒 | passed |
| classic | 5.5.4.70268 | passed | passed |
| classic-era | 1.15.9.70003 | passed | passed |
| titan | 3.80.2.70315 | passed | passed |
| forever-asia-beta | 1.60.1.70338 | passed | passed；修复后独立空缓存 192.5 秒 |
| forever-cn-beta | 1.60.1.70338 | passed | passed |
| anniversary | 2.5.6.70006 | passed | passed |

亚服 beta 的原串行路径先后在 300 秒和 600 秒超时，成功缓存重试不算首次网络读取通过。
确认目标表位于 CDN 清单第 1226 个归档后，改进索引读取与调度；修复后的独立空缓存工作区
使用默认 300 秒限制成功，Root 和 DB2 的 decoded cache 均为零命中。各产品首轮可复用同来源的
不可变 CDN range；仅两项标注的空缓存回归使用独立的新工作区，不能将全部首轮耗时解释为独立冷启动。
原始命令和响应保留在本工作树 `.tmp/data-products-3.2.0/`：`local/`、`remote/`，
以及 `retail-final-cold/`、`remote-asia-final/`。
最终 `go build ./...`、`go vet ./...`、强制 Lua 5.1 的 `go test -count=1 ./...`、
`go test -race ./internal/records -run TestCDN -count=1`、版本和 Skill 命令合同检查均通过。
开发二进制为 `.tmp/data-products-3.2.0/lycheedev.exe`；版本源仍为发布基线 3.1.2，尚未发布 3.2.0。

常用操作（使用本工作树构建的 CLI，已发布的 npm 包尚未更新；先 `init`，查询时替换 `<PIN>`）：

```powershell
lycheedev target resolve --product forever-cn-beta --region cn --locale zhCN --installation "D:/Game/World of Warcraft/_cn_beta_" --format json
lycheedev data db2 --snapshot <PIN> --table ChrRaces --limit 2 --installation "D:/Game/World of Warcraft/_cn_beta_" --format json
lycheedev target resolve --product forever-asia-beta --region tw --locale zhCN --format json
lycheedev data db2 --snapshot <PIN> --table ChrRaces --limit 2 --cdn --timeout-seconds 600 --format json
```
