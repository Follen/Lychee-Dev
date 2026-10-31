# 当前待办

以 [DESIGN.md](DESIGN.md) 为业务与架构基点。工作规则见 [AGENTS.md](AGENTS.md)，测试用途与缺口见 [test.md](test.md)，当前实施状态见 [handover.md](handover.md)。

这里仅保留未完成事项。`[ ]` 表示尚未确认在当前正式路径上闭合，不能据此断言代码完全没有实现。先复用已成立的能力，再补缺口；不把历史通过记录搬来勾选当前任务。已完成并验证的本批项（Go 产品源码删除、旧测试/旧文档账本清理、CI workflow 重写、core/data 编译）已移出，验证事实见 handover。

## 当前死代码与冗余测试专项

- [ ] **AUDIT-DEAD｜生产路径、有效覆盖与文档复审。** 9个Source examples已退役，35个迁移默认测试、当前全目标编译与受影响回归通过；原始失败保留，实际结果见handover。仍须最终独立Agent、最终快照完整CI/Release及唯一TGZ验收和闭包复审，Live实机留后。
- [ ] **FOREVER-FORMAL｜正式Forever最终数据与包验收。** `forever` / `wowf` / `_forever_`全链实现及当前产品/退休Beta离线正负例已验证，旧PIN/CAP/raw输入与恢复身份保留；只读本地70658与远端70707证据不推导最低build。仍须正式wowf固定数据实际读取、最终CI和同包业务验收；Live实机由用户后续安排。

## 1. 旧代码、旧测试、Go 与旧 CI 的剩余清理

- [ ] **CLEAN-07｜未完成恢复与被拒绝材料。** 本批闲置工作树已按新授权清理48份；当前/主checkout/明确保留树保留，e85与fc001另保原prepared fence及未确认回收owner。此前13个scratch、rust327-audit、8份旧编译副本与ModuleAnalysisCache删除被自动审批拒绝，未绕过。活动输入、CAP、最终CI包/当前构建及未解决恢复不在删除范围。

## 2. 当前路径测试与交付

- [ ] **LIVE-CLIENT｜最后完成真实客户端验证。** 见 handover 最后一节 H01—H08，六个当前产品分别记录，Forever仅正式版；两个Beta历史结果不能计为正式Forever通过。用户安排前不操作游戏；缺客户端/环境时留下具体缺口。
- [ ] **LIVE-01｜发送前INPUT修复的实机复验。** 已在Retail复现采样后的500槽acquire与verify分别耗时519/517ms，导致发送前复核拒绝、零输入入队。Invoke准备后重新采样、独立Reload及激活身份检查已通过真实DiskPublications回归和Live完整库516项，保留one-use、同profile、最终publication与原deadline。两个实机CON均已正式关闭，本轮按用户要求不占用Live；再次授权实机时使用当前候选产物验证实际发送与清理。

本待办不包含未经确认的新功能，也不授权删除旧数据、操作游戏或发布产品。
