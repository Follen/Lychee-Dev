# 原生 live 观察会话与读取预算

日期：2026-09-30。状态：本任务 worktree 的候选实现；未部署、未发布。
对应前一轮 `source-live-optimization-audit-2026-09-30.md` 的 L0/L3/L4，
并接入 L1/L2 内存搜索改动。本页不替代 [200 槽合同](live-slot-routing-2026-09-29.md)
或 [输入与恢复合同](live-input-architecture-2026-09-28.md)。

## 生命周期与准入

一个 `Native` 持有一个有界 `memory.Session`，生命周期是一次 CLI invocation。
同一 Native 的 discovery、input、receipt、HEAD/BODY、动作前点读、runtime 换代快照
共用读取和候选预算，重复 `Driver.Continue` 不补充额度。PID、进程创建时间与映像固定；
创建时间在结果中编码为字符串，避免 FILETIME 经 JavaScript 数字丢失精度。

Continue 先取得既有持久目标预算，再把截止时间传给观察会话。观察会话只收紧调用
边界，不延长目标期限。邻域的 250ms、换代观察的 15s 等局部期限只限制该阶段；
成功结束的局部阶段不能令整个 invocation 提前过期。后续 CLI resume 会创建新的
物理读取预算，**物理计数尚未跨 invocation 持久累计**；原有持久业务/控制期限仍继续
消耗，不因 resume、缓存或 runtime 换代重置。

已明确选择的新 runtime 会清除输入等待、高水位及旧光学流；精确查询继续限定
runtime/nonce/ticket。清理调度缓存不触碰预约、未知输入、业务进度或 owner。
没有复用旧区域图来授权读取，每次实际读取仍由 Source 检查当前映射，最终验证仍
检查进程身份。地址、sequence 和 runtime 排序都不能代替 fresh receipt 或绑定证明。

## HEAD/BODY 与搜索路径

已知 runtime 的首个小记录查找先尝试有界邻域，未命中或局部额度耗尽后按原调用
语义回退完整扫描。未知 runtime、`first=false` 和 `--no-cache` 保持完整发现路径。
输入观察保留原有新鲜度、1500ms 调度线索、固定本地等待与恢复出口。

`ObserveRecords` 首先验证 reported HEAD 的全部操作、角色、owner/fence、版本与
协议字段，再从 HEAD 的 report 长度及校验和创建精确 BODY selector。
仅在此之后，HEAD 地址可作为 BODY 邻域的调度种子；没有固定相邻偏移假设。
邻域未命中会回退完整 BODY 搜索，局部覆盖不能证明全进程不存在。
一般 hints 不学习、保存或点读 BODY，BODY 负载也不进入扫描诊断。

BODY 找到后再次点读原 HEAD，要求完整 header 和 payload 字节保持相同，再验证
进程身份。HEAD 改变、当前映射不可读、短读、取消或身份失败均不能输出混合报告。
该结果仍是候选报告；后续精确 confirmation、持久化及 release 流程完整保留。
`report.ok=false` 的业务失败与传输、清理完成继续独立呈现。

## 限额与指标

Session 默认物理上界为 1,048,576 次 Source.Read、256 GiB 请求字节、
1,048,576 个业务候选；独立学习上界为 4096 个候选及 64 MiB 学习字节。
这些是资源上界，尚不是实机调优结果。短读、失败请求、重叠、salvage、hint 点读和
完整重读均计入相同预算，学习耗尽不会停止合法业务查找。
每 Native 最多 256 个查询证据；有界邻域仍保留自身 8 MiB/4096 reads/250ms 上界。

既有结果新增可选 `observation`，既有扫描证据在延后 flush 时附同一聚合快照：

```text
observation: {
  scope: "invocation", pid, processCreated: "精确创建时间",
  runtime?, deadlineMS?, lookups, stageTruncated,
  total: memory.Stats,
  stages: [{kind, path, timed, calls, elapsedMillis, stats}]
}
```

Stats 包括 Source.Read 次数、请求/实际字节、短读、Verify/Regions 次数、
真实 Windows 映射查询与 RPM 次数、业务/学习候选和学习字节、成功校验、拒绝及
头部/完整性/谓词拒绝分类。逻辑 PlannedBytes/ScannedBytes 保持原 coverage 含义，
不能解释成实际跨进程复制字节。fake Source 的 read 计量验证适配器实际调用，
不把其中零次 Windows syscall 当作实机测量。

按 kind/path 累计最多 31 组查询阶段，再留一组 residual，列表总上界 32。
完整统计由 `Stats.Add` 聚合，避免新增计数字段丢失。正常 residual 是
`direct_validation`，涵盖动作前精确点读、最终 HEAD 和换代快照；其 `timed=false`，
没有声称已测该组持续时间。组数溢出时 `stageTruncated=true` 且 residual 改为
`unattributed`，保留全部计数但不伪称遗漏查询是直接验证。
WGC、输入等待、journal、JSON 的独立 CPU/耗时，本轮没有完整计量。
查询 elapsedMillis 和既有扫描文件可供后续样本统计，不构成实机 p50/p95 结论。

`localStopped` 表示局部附近路径停止，可正常回退；`cancelled`/`deadlineExceeded`
以及预算标记是会话中的诊断事实，不能单独替代命令、业务或 cleanup 的完成状态。
纯只读历史结果不继承前一次命令的计数，没有新 Native 观察时该字段可缺省。

物理预算耗尽保留当前 CON、原日志和未决交换，命令层显式映射资源错误
`live.channel_memory_budget`（exit 3）；256 查询证据限额同样是资源错误。
动作前点读及换代快照的预算错误不能被折叠成普通缺失或正常关闭回退。
下一次恢复仍从原持久事实继续，未确认或未知输入不会重放。

## 验证与未运行范围

本批次在独立 `.tmp/gocache-observation` 中通过 `go build ./...`、`go vet ./...`。
整个 channel 包一次通过；最终新增覆盖同时通过定向 `go test -race -count=1`：

- 实际 Source：近处授权 BODY、远处完整回退、关闭缓存完整扫描、业务失败报告保留。
- HEAD 修改、映射变化、最终 HEAD 短读、进程身份失败和取消拒绝混合结果。
- 外来 HEAD 不产生 BODY 点读、HEAD/BODY 共用物理预算、换代快照预算显式报错。
- 同 Native 重复推进、runtime 变化不续预算、局部阶段期限不污染外层 deadline。
- 完整计数字段跨重复阶段累计、阶段列表溢出可见，所有计数仍与总计一致。
- 原输入 cadence、诊断缓冲、flush 取消、资源释放顺序等回归继续通过。

完整最终离线基线由主任务统一运行并另行记录。真实客户端首连、warm/cache-off
延迟、HEAD/BODY 分布、reload、重登录、首装/升级、双实例和取消响应为 **not_run**。
本页没有将旧版本实测或离线 fixture 成功迁移成当前候选的实机结论。
