# 当前待办

以 [DESIGN.md](DESIGN.md) 为业务与架构基点。工作规则见 [AGENTS.md](AGENTS.md)，测试用途与缺口见 [test.md](test.md)，当前实施状态见 [handover.md](handover.md)。

这里只保留未完成事项；完成项移出，当前验证事实见handover。新增缺口先核当前正式路径和既有能力，再确定实现与验证范围。

## 原生作者发布的剩余业务

当前基线、实施顺序、删除和保留范围统一见 [FUPLOADER.md](FUPLOADER.md)。固定原业务 204 项、官方 NewBee 38 项及恢复入口 2 项均已接入；当前 312 条命令及 93 份真实输入定义。DD、小黑盒、ModUs 完整业务的离线回归已通过。

- [ ] **PUBLISH-PLATFORM｜真实环境验收。** 实际厂商 SDK、账号登录和各平台真实发布/读回未执行；离线合成 SDK、回环协议及发布包验证分别记录。按明确账号、目标和发布内容开展实际业务验收；本轮不读取用户账号、不提交真实平台写入，Live 仍留待用户安排。
- [ ] **PUBLISH-DELIVERY｜完整交付验证。** 新 Publish 尚未完成整批 CI、Release 和正式包验收。当前离线回归与真实 SDK/账号/平台验收分别记录，复用原产物；Live 由用户安排。

## 1. 工作区恢复与材料保留

- [ ] **CLEAN-07｜未完成恢复与证据缺失。** 当前仅保留 main 和 Rust-refactor 工作树；10 个 active/interrupted 且回收未确认的 owner 仍待核实。三份旧树中的 6 个未收尾 owner 及原 prepared fence/lease 保留在 `.tmp/worktree-rename/recovery/`，状态未改写。此次清理误删了旧清单中两处包验收临时安装目录及一处封存日志校验异常目录，当前工作区未找到原副本，具体 ID 见 handover；日志异常未解决。15 个 evidence 输入、当前 CI 包及直接引用的验收报告、build-16、Lua、活动项目/CAP 和固定输入仍保留。

## 2. 当前路径测试与交付

- [ ] **LIVE-CLIENT｜最后完成真实客户端验证。** 见 handover 最后一节 H01—H08，六个当前产品分别记录，Forever仅正式版；两个Beta历史结果不能计为正式Forever通过。用户安排前不操作游戏；缺客户端/环境时留下具体缺口。
- [ ] **LIVE-01｜发送前INPUT修复的实机复验。** 已在Retail复现采样后的500槽acquire与verify分别耗时519/517ms，导致发送前复核拒绝、零输入入队。Invoke准备后重新采样、独立Reload及激活身份检查已通过真实DiskPublications回归和Live完整库516项，保留one-use、同profile、最终publication与原deadline。两个实机CON均已正式关闭，本轮按用户要求不占用Live；再次授权实机时使用当前候选产物验证实际发送与清理。

本待办不包含未经确认的新功能，也不授权删除旧数据、操作游戏或发布产品。
