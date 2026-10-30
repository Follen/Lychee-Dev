# 当前待办

以 [DESIGN.md](DESIGN.md) 为业务与架构基点。工作规则见 [AGENTS.md](AGENTS.md)，测试用途与缺口见 [test.md](test.md)，当前实施状态见 [handover.md](handover.md)。

这里仅保留未完成事项。`[ ]` 表示尚未确认在当前正式路径上闭合，不能据此断言代码完全没有实现。先复用已成立的能力，再补缺口；不把历史通过记录搬来勾选当前任务。已完成并验证的本批项（Go 产品源码删除、旧测试/旧文档账本清理、CI workflow 重写、core/data 编译）已移出，验证事实见 handover。

## 1. 旧代码、旧测试、Go 与旧 CI 的剩余清理

- [ ] **CLEAN-07｜清理后全仓引用核对。** 最新 Data/Source 修复后的 workspace all-targets 已通过；此前文档、Skill 与相关 Node 合同检查通过。用户指定的 Cargo target/缓存清理已完成并保留 Emmy 上游与外部 Go 参考。最终文档/引用检查 47 份、0 violation；整体差异检查仍有接手前 `generation.rs` 末尾空行提示。13 个已解决实验 scratch 的清理被自动审批拒绝，尚未删除。ignored 和早期中断恢复记录单独保留，不扩大清理范围。

## 2. CI 构建一次，后续复用（剩余验证）

`ci.yml`/`release.yml` 与 `tools/ci-*.mjs` 已实现四阶段产物流并在本机通过结构/静态/纯函数验证；以下未闭合：

- [ ] **CI-04｜一次真实 CI 运行核实。** 需要 GitHub Actions 日志核实：build每种配置只编译一次、validate只消费本轮artifact、无隐式重建、失败定位准确。运行39834671890的static已全部通过，随后两个配置的构建被取消，下游未验证；固定Rust后helper身份与原许可匹配。共享脚本独占读取冲突已用独立同字节fixture修复并红/绿验证；嵌套产物父目录缺失已修复、12项工具测试通过。两项修复未提交。**本机已端到端模拟通过**：ci-test-programs build → ci-artifacts seal → verify → run。真实跨job产物复用与下游验证仍未完成。
- [ ] **CI 产物的跨 job 路径。** build artifact 记录含构建时绝对 workspace 路径；当前 workflow 依赖 hosted Windows 默认 checkout 路径一致，须在真实 CI 核实。validate-package 已改为显式复用 build 封存的完整许可证 closure，移除下游 fetch/metadata；纯 Node 回归及本批实际新包复用均通过。只读 `cargo -V` 工具链探针保留，不声称完全没有 Cargo 进程。

## 3. 当前路径测试与交付

- [ ] **TEST-02｜补齐必要业务回归。** 当前可执行的本地业务已通过，包括最终语义、XML/四产品矩阵及当前新包的Retail/Forever、CLI、refs/context和luac；无关已通过结果继续有效。剩余两项symlink测试因Windows权限未执行，真实Actions和客户端验证分别见下列待办。Delivery原错误5未复现，完整库156项后续通过，原失败记录保留但原因未定。
- [ ] **DELIVERY-01｜验证实际安装。** 两轮审计修复版Release已完成隔离npm安装、Skill launcher、受管luac、显式许可证closure复用与语法检查；包内receiver/发现信息、完整Retail及Forever Source业务也已通过，未发布。七产品Map及Retail图标完整值业务通过；两个Forever beta各自73行可读、8行不可用的partial保持。此前Retail的3.5.0受管安装、500槽及有效INPUT已确认，实际输入受LIVE-01阻断；两个CON现均关闭，业务尚未通过。正式Go随后更新至3.2.6，恢复实机前重新核对安装与实例。
- [ ] **LIVE-CLIENT｜最后完成真实客户端验证。** 见 handover 最后一节 H01—H08，七产品与两 Forever beta 分别记录；缺客户端/环境时留下具体缺口。
- [ ] **LIVE-01｜发送前INPUT修复的实机复验。** 已在Retail复现采样后的500槽acquire与verify分别耗时519/517ms，导致发送前复核拒绝、零输入入队。Invoke准备后重新采样与独立Reload已通过真实DiskPublications回归及Live完整库512项，保留one-use、同profile、最终publication与原deadline。两个实机CON均已正式关闭，本轮按用户要求不占用Live；再次授权实机时使用当前候选产物验证实际发送与清理。

本待办不包含未经确认的新功能，也不授权删除旧数据、操作游戏或发布产品。
