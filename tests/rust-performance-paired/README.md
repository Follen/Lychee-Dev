# 当前固定输入与有界测量工具

此目录保留仍有消费者的 Agent Data、generation 和 Live 控制器及独立输入校验。
性能目标已经由用户接受，不因审计重新执行比较。当前任务与实际结果见
[todo.md](../../todo.md) 和 [handover.md](../../handover.md)。

| 入口/组件 | 当前用途 |
| --- | --- |
| [agent-prepare-fixtures.mjs](agent-prepare-fixtures.mjs)、agent-fixture-inputs、agent-importer | 准备和认证独立原始 WDC/DBD、CASC、数值及缺密钥输入；正式包业务也复用这些输入。 |
| [agent-paired-owned.mjs](agent-paired-owned.mjs)、agent-paired-run、agent-query-run、agent-replay-run | 有界 Agent Query/Page/Capture/Release/Close 全生命周期、固定输入与完整结果复核。 |
| [generation-paired.README.md](generation-paired.README.md) | 显式 prepare、fresh 与复用工作量；编译与业务期限分别控制。 |
| [live.README.md](live.README.md) | 当前addon与固定Lua peer的离线Mailbox诊断；无游戏访问或原生输入。 |
| inputs、contract、data-contract、agent-custody/audit | 认证显式外部参考、固定业务输入、结果、报告和原进程回收；模块导入不执行参考产品。 |
| data-phase-profile、generation-profile | 解析显式启用的诊断记录；分段可能嵌套，不相加或从原始总时扣除。 |

旧 Source pack 格式诊断、导出器和 B01/B02 比较入口已退役。当前 Source 正确性使用
[full corpus](../rust-source-full-corpus/README.md)、
[native acceptance](../rust-source-native-acceptance/README.md) 和当前 Rust 测试。
独立原始输入与已接受结果不随执行器清理而改写。

需要显式外部参考的入口要求其固定 artifact、包和二进制身份，仓库不构建 Go。
机器条件包含非空 `machine`、`powerAndThermal`、`competingLoad`、`disk`、`operator`；
未知条件如实写明。测量不得并行占用原生队列。

复用匹配的已构建 Rust 产物与当前 target/cache。Cargo 编译默认60分钟；业务控制器
保各自期限。消费者不得隐式编译、重打包或替换候选。应用冷缓存不等于系统冷缓存。
原始计时包含完整业务进程、输出排空和树回收；失败、缺样本或未回收均不能记通过。
输入、结果与回收证据分别认证，工具单测通过不代表产品业务或实机通过。
