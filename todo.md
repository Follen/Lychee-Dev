# 当前待办

以 [DESIGN.md](DESIGN.md) 为业务与架构基点。工作规则见 [AGENTS.md](AGENTS.md)，测试用途与缺口见 [test.md](test.md)，当前实施状态见 [handover.md](handover.md)。

这里仅保留未完成事项。`[ ]` 表示尚未确认在当前正式路径上闭合，不能据此断言代码完全没有实现。先复用已成立的能力，再补缺口；不把历史通过记录搬来勾选当前任务。已完成并验证的本批项（Go 产品源码删除、旧测试/旧文档账本清理、CI workflow 重写、core/data 编译）已移出，验证事实见 handover。

## 当前死代码与冗余测试专项

- [ ] **AUDIT-DEAD｜生产路径、有效覆盖与文档复审。** 确认无消费者路径/测试包装/旧工具已清，独立业务预期迁正式入口；Skill已精简，版本/渠道与请求/响应/错误由CLI自解释。当前全目标编译及受影响回归通过，最终交叉审查完成；桌面钩子fixture的本机失败保留并交CI干净runner验证。尚待独立Agent实际使用、完整CI/Release和唯一正式包验收，前轮结果只作基线，Live留后。

## 1. 旧代码、旧测试、Go 与旧 CI 的剩余清理

- [ ] **CLEAN-07｜未完成恢复与被拒绝材料。** 本批闲置工作树已按新授权清理48份；当前/主checkout/明确保留树保留，e85与fc001另保原prepared fence及未确认回收owner。此前13个scratch、rust327-audit、8份旧编译副本与ModuleAnalysisCache删除被自动审批拒绝，未绕过。活动输入、CAP、最终CI包/当前构建及未解决恢复不在删除范围。

## 2. 当前路径测试与交付

- [ ] **LIVE-CLIENT｜最后完成真实客户端验证。** 见 handover 最后一节 H01—H08，七产品与两 Forever beta 分别记录；缺客户端/环境时留下具体缺口。
- [ ] **LIVE-01｜发送前INPUT修复的实机复验。** 已在Retail复现采样后的500槽acquire与verify分别耗时519/517ms，导致发送前复核拒绝、零输入入队。Invoke准备后重新采样、独立Reload及激活身份检查已通过真实DiskPublications回归和Live完整库516项，保留one-use、同profile、最终publication与原deadline。两个实机CON均已正式关闭，本轮按用户要求不占用Live；再次授权实机时使用当前候选产物验证实际发送与清理。

本待办不包含未经确认的新功能，也不授权删除旧数据、操作游戏或发布产品。
