# 当前待办

以 [DESIGN.md](DESIGN.md) 为业务与架构基点。工作规则见 [AGENTS.md](AGENTS.md)，测试用途与缺口见 [test.md](test.md)，当前实施状态见 [handover.md](handover.md)。

这里仅保留未完成事项。`[ ]` 表示尚未确认在当前正式路径上闭合，不能据此断言代码完全没有实现。先复用已成立的能力，再补缺口；不把历史通过记录搬来勾选当前任务。已完成并验证的本批项（Go 产品源码删除、旧测试/旧文档账本清理、CI workflow 重写、core/data 编译）已移出，验证事实见 handover。

## 1. 旧代码、旧测试、Go 与旧 CI 的剩余清理

- [ ] **CLEAN-07｜剩余临时材料清理。** 用户指定的Cargo target/缓存清理已完成并保留Emmy上游与外部Go参考；13个已解决实验scratch的删除曾被自动审批拒绝。当前`.tmp/rust327-audit`的465文件/4,950,653,420字节已完成用途、路径/reparse/项目及回收检查，但删除同样返回`blocked by policy`，仍保留，不重试绕过。本批8份退役且回收的旧编译副本（约13.05GB）与PowerShell临时ModuleAnalysisCache清理也返回blocked by policy，已保留；不更换工具重试。活动输入、CAP、最终CI包/当前构建与早期中断恢复记录不在删除范围。

## 2. 当前路径测试与交付

- [ ] **LIVE-CLIENT｜最后完成真实客户端验证。** 见 handover 最后一节 H01—H08，七产品与两 Forever beta 分别记录；缺客户端/环境时留下具体缺口。
- [ ] **LIVE-01｜发送前INPUT修复的实机复验。** 已在Retail复现采样后的500槽acquire与verify分别耗时519/517ms，导致发送前复核拒绝、零输入入队。Invoke准备后重新采样、独立Reload及激活身份检查已通过真实DiskPublications回归和Live完整库516项，保留one-use、同profile、最终publication与原deadline。两个实机CON均已正式关闭，本轮按用户要求不占用Live；再次授权实机时使用当前候选产物验证实际发送与清理。

本待办不包含未经确认的新功能，也不授权删除旧数据、操作游戏或发布产品。
