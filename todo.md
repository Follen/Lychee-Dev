# 当前待办

以 [DESIGN.md](DESIGN.md) 为业务与架构基点。工作规则见 [AGENTS.md](AGENTS.md)，测试用途与缺口见 [test.md](test.md)，当前实施状态见 [handover.md](handover.md)。

这里只保留未完成事项；完成项移出，当前验证事实见handover。新增缺口先核当前正式路径和既有能力，再确定实现与验证范围。

## 1. 工作区恢复与材料保留

- [ ] **CLEAN-07｜未完成恢复与日志差异。** 已累计清理51份闲置工作树，仅保留main和当前Rust-refactor；另删除58个已结束的可重建scratch、11份退役程序副本及ModuleAnalysisCache。当前目录仍保留10个active/interrupted且回收未确认的owner与1个封存日志校验异常目录，待按原身份和证据核实。三份已移除旧树中的6个未收尾owner及原prepared fence/lease等资料集中保留在`.tmp/worktree-rename/recovery/`，状态未改写。15个evidence输入、2个被当前验收引用的目录、当前CI包、build-16、Lua、活动项目/CAP及固定输入继续保留。

## 2. 当前路径测试与交付

- [ ] **LIVE-CLIENT｜最后完成真实客户端验证。** 见 handover 最后一节 H01—H08，六个当前产品分别记录，Forever仅正式版；两个Beta历史结果不能计为正式Forever通过。用户安排前不操作游戏；缺客户端/环境时留下具体缺口。
- [ ] **LIVE-01｜发送前INPUT修复的实机复验。** 已在Retail复现采样后的500槽acquire与verify分别耗时519/517ms，导致发送前复核拒绝、零输入入队。Invoke准备后重新采样、独立Reload及激活身份检查已通过真实DiskPublications回归和Live完整库516项，保留one-use、同profile、最终publication与原deadline。两个实机CON均已正式关闭，本轮按用户要求不占用Live；再次授权实机时使用当前候选产物验证实际发送与清理。

本待办不包含未经确认的新功能，也不授权删除旧数据、操作游戏或发布产品。
