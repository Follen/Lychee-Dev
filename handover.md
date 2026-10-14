更新日期：2026-10-14，Asia/Shanghai。当前状态见下；详细问题与逐轮证据见 [审计报告](docs/Clean-repo-2026-10-14.md)，协作规则见 [AGENTS.md](AGENTS.md#multi-agent-handover)。

## 分支与交付

- [PR #13](https://github.com/Follen/Lychee-Dev/pull/13)：`codex/fix-3.2.0` → `main`，保持草稿。前批已发布 `3ed74d85`；其[原生CI38563819418](https://github.com/Follen/Lychee-Dev/actions/runs/38563819418)五项必需作业和汇总通过，不能代替本次R23–R27清理树。本批代码/离线记录正交付同分支，新head原生CI待完成。
- 已交付R3/R4的13项、R5–R9修复、R16-02/R19-02/R20/R21恢复边界及R17 Source Skill规则；R18/R22等独立复核与撤回R16-01的依据见审计。现场因果未确认，不将其他代码复现当现场根因。
- [PR #12](https://github.com/Follen/Lychee-Dev/pull/12)已关闭、未合并。只移植必要Definitions冷读预算修复 `ef9b0113`；旧地区槽位算法与现合同冲突，未移植。
- 发行源3.1.2、渠道next、latest=2.5.1保持；3.2.0为开发基线。不合并、不发布。只用本仓库handover，未连接用户电脑/游戏、发键、部署游戏目录或修改Lychee Talent。

## 当前批次

- **R23完成**：删除无生产消费者的WGC实现、专属测试/SDK/观察CI与失效截图文案。保留窗口身份、输入、进程及历史图像读取。此前把残留当产品能力/用户实测义务属于漏审，现撤回；删除不是修好截图。
- **用户要求的三轮已完成**：R24、R25、R26各由不同全新审计者按顺序审查修正后的冻结树，分别确认并最小清理10、5、5组；有效测试迁到现行入口。R25四份仍有消费者的baseline probe已改用当前模块并加入正式真实TOC/ProbeExecution回归。R26删除109对旧语言键，263活键及动态选择保持。
- **R27整合验收**未发现R23–R26运行回退，另确认末端忽略的Native私有cache参数链。窄清理已完成并冻结；公开--no-cache保留兼容、当前带或不带走同一新鲜路径，旧3.1.2另核。最终build/vet及完整套件已完成，有限独立复核无可操作缺陷，不扩大全仓审计。
- 写入边界：所有产品实现者已完成并冻结；协调者独占共享文档、提交与验证；原ZIP新调查者只读，禁止公布原始私人日志。

## 实际验证与下一步

- 工具链Go1.27.1、Node24.18.1、npm11.18.0、Lua5.1.5已核，设置LYCHEEDEV_REQUIRE_LUA51=1及确切解释器路径。
- R23–R26受影响检查已完成：全局build/vet、Windows交叉构建、各包/协议/Lua回归通过；R26最终预检Node81pass/6 Windows-only skip，版本/Skill/命令目录/import通过。两处既有channel fixture完成条件已修且定向通过，生产预算不变；此前失败日志保留。Linux npm launcher4项受既有Windows平台准入限制，不称通过。
- R27冻结913文件起终一致（aggregate `0c7c62a0384fad93a6fd67d21dc9b4eeb57d68e91bcbd7252f2797581b6abf31`）。最终build/vet通过；完整Lua必需Linux套件2578事件pass/2346叶节点、28skip、3既有平台fail，仅TestLiveConnectArgumentAdmission、TestExecuteContractRejectsIncompleteRequestsBeforeWorkspaceAccess、TestSourceResearchThroughCLI；30包pass，不称Linux全套通过。定向64事件0fail/skip、四Windows包测试交叉编译及有限独立终审通过。下一步推同分支并完成新head原生CI/coverage，旧结果不替代。
- 上批精确3ed74d85原生计量24752/32099=77.1114%，2893事件/2628叶节点pass、0fail、20skip（9helper入口、11实际可选）；原始artifact与口径见审计。当前删死代码会改变分母，必须重新计量，不把比例上升当新增业务覆盖。

## 待实机验证

代码修复、离线通过和实机验证分别记账。下列实际结果留空，只供后续明确授权的本机任务；各项详细复验见审计条目。共同前置为匹配版本的干净受管 CLI/addon、固定 installation/PID/创建时间/HWND、产品/构建与原 project/CON/request/期限，不重放 submitted/uncertain 输入。

| ID | 最小复验与预期 | 实机结果 |
| --- | --- | --- |
| R3-01 | Trace `GetTime.foo`/标量路径及正常方法；前者页内错误，后者正常追踪 | |
| R3-06 | 主窗口隐藏时开拾取后进入战斗；dock/键盘脚本立即关闭，不吞 F/Enter/Escape | |
| R4-01 | 无副作用请求跨 prepare/commit/confirm/release、原 CON resume；已知输入及恢复上界后采样间隔至少 100ms，无重放，reload 新 runtime 独立 | |
| R4-04 | 新配置 status、off/on、保存 off 后 reload；状态与 INPUT 生命周期一致 | |
| R4-07 | Objects 检查失败 tostring/普通错误及健康对象；无二次错误、失败明确、部分流/limited 保留 | |
| R5-01 | 只返回高精度有限数值和 `%.17g` 对照文本；报告数值保持一致，正常清理并断开 | |
| R20 现场因果未确认 | 两原CON已关闭，原3.1.2同runtime bind读取根因仍未确认；后续匹配当前协议CLI/addon、隔离新CON，复验bind回执及同进程切角色/reload后原bind无回执；有新鲜替代证据才无输入退役，暂不可读无证明时保留未知，中断后续清理且正常关闭不退化。旧3.1.2日志仅用匹配CLI诊断，当前driver不得接管 | |

本轮原6项之外新增R20现场复验1项，共7项客户端整合；已确认无生产入口的WGC不再列为待验证产品能力。七产品旧矩阵见 [todo.md](todo.md)，未扩展为本轮通过。

## 候选与特定环境复验

| 项目 / 当前状态 | 必要环境、最小步骤与预期 | 复验结果 |
| --- | --- | --- |
| R3-C01 候选 | 固定真实SpellMisc DB2/DBD/DifficultyID多匹配资料；比较全表、导航及领域合同，证据冲突才升级 | |
| R28-C01 现场静态空节点候选 | 后续获准Windows固定目标与匹配版本诊断；新连接首个Kind2失败记录逻辑节点/单次长度/region拒绝及guard变化，预期区分静态节点/发布变化/坏指针。现原包不能唯一定位，不重放旧输入 | |
| R3-C03 候选 | 先明确instances归属语义；隔离Windows owner fixture，对照展示与连接消费者，不把被动字段当阻断 | |
| R4-C01 已修复/原生通过 | Windows双真实卷、隔离release/client/project、PID=0；比较同卷及跨卷默认/显式archive，旧树应保全并在安全激活边界停止 | R8 `7566493c` 双真实卷及同卷对照通过 |
| 原安装/index/图像与runtime材料缺口 | 取得获准、匹配hash/build的原始资料后跑正式replay；身份/字节/解码/根证据应吻合，合成fixture不代替原材料 | |
| Live read-only可选缺口 | 后续授权Windows/WoW固定目标；只读核对身份和窗口，不扩展输入许可 | |

## 证据与限制

- R20最新401行诊断报告确认两次CON均已通过runtime_replacement_closed关闭，旧“仍卡closing”现状已更正；核心是同runtime的bind回执Kind2持续short_read/ineligible_read。identity/INPUT完整不证明回执可读，1574字节/42调用仅汇总，不能定位根因。旧3.1.2/新Skill合同差异已确认；69文件/50scans原ZIP现已取得且SHA256吻合，69文件路径/CRC、68项内部hash、两journal校验链均通过。精确旧reader的三条不同路径可产生相同汇总，不能据此定位。静态空节点仅R28-C01候选，WoW布局/节点证据不足，无可直接修复的现场根因结论。已推R20读取变化分类和R19/R21关闭边界不等于此现场根因已修，不要求现在开游戏。
- 全部剩余忽略材料保留，不再清理。此前部分R20/R22临时负对照原日志已删除；正式回归、版本和GitHub正式CI artifacts仍在，不能声称全证据完好或重造历史日志。
