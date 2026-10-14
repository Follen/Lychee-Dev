更新日期：2026-10-14，Asia/Shanghai。当前状态见下；详细问题与逐轮证据见 [审计报告](docs/Clean-repo-2026-10-14.md)，协作规则见 [AGENTS.md](AGENTS.md#multi-agent-handover)。

## 分支与交付

- [PR #13](https://github.com/Follen/Lychee-Dev/pull/13)：`codex/fix-3.2.0` → `main`，保持草稿。最新已推送Skill修订 `0f26b9a7`；此前SDK对照 `310a5d7d` 的[精确head CI](https://github.com/Follen/Lychee-Dev/actions/runs/38555308695)完整Go与16集成及四专项通过，仅独立WGC/原汇总失败。
- 本次 Skill 批次 R17-01：主 Skill、source-research、source-preparation 已修订；全新 R18 独立复核17个场景，无可操作冲突，正式合同检查通过。规则默认用固定 Source，空结果先核模式/关键词/版本/覆盖，禁止猜 raw 路径；真实能力缺口或明确外搜请求保留例外，已有充分证据直接回答。
- 已交付 R3/R4 的13项、R5–R9确认修复与清理，前42项作为基线，不反复计数。R10–R15在各自冻结范围的独立复核结论见审计；不能替代后续产品变化的复核。
- [PR #12](https://github.com/Follen/Lychee-Dev/pull/12)已关闭、未合并。必要 Definitions 冷读预算修复随 `ef9b0113` 移植；旧地区槽位算法与现行 legacy 合同冲突，未移植，不是整批 cherry-pick。
- 发行源仍 `3.1.2`，渠道 `next`、`latest=2.5.1`；`3.2.0`仅开发基线。不合并、不发布。本任务只用仓库 handover.md，未连接用户电脑/游戏、发键或部署游戏目录。

## 当前工作与写入边界

| 范围 | 当前状态 | 负责人 |
| --- | --- | --- |
| R16 高风险补测与修复 | R16-02回收完成前逻辑状态过早推进已修，R19-02成功unbind后的纯终态关闭被过期预算阻断已修；R19撤回无受支持生产路径的R16-01，改清理退役bootstrap死代码R19-01。修正树尚未推送 | 实现者仅约定 evidence/vault/channel/protocol 文件，已冻结；R21-01 wrapper组合缺口已修；R22新鲜只读复核完成，无新增可操作离线缺陷 |
| R17-01 / R20-02 Skill | R17三文件已随0f26b9a7推送；R20另补startup/recovery两reference已冻结、合同228引用0违规，纳入R22最终冻结树 | Skill 实现者只写 Skill 目录，已完成 |
| R12-C01 WGC | 原生SDK编译与对照完成，Go/SDK两个自建窗口均0x80070057及相同即时错误。无Go特有缺陷证据、具体宿主根因未定；单列非门禁观察，保留失败和后续Windows非游戏复验 | 协调者及 desktop 专组 |
| 整合与文档 | R22最终924文件树完整Linux套件2576pass/3既有平台fail/28skip，build/vet通过；独立复核完成无新增确认问题，随后推送并测精确head Windows/覆盖；R20仅已读取230行现场输出摘录 | 协调者独占 workflow/tools/审计/todo/handover/Changelog |

持续审计→修复→验证→推送，直到独立复核不再发现可离线修复项。新确认问题继续处理；实机相关只保留复验步骤与空白结果，不等待用户开游戏。

## 已实际完成的验证

- 工具链：官方 SHA 校验 Go 1.27.1、Node 24.18.1、npm 11.18.0、Lua 5.1.5；`LYCHEEDEV_REQUIRE_LUA51=1`及明确解释器路径。
- 最近完整有效 Windows 主套件为 `310a5d7d`：[CI38555308695](https://github.com/Follen/Lychee-Dev/actions/runs/38555308695)。完整Go、16项公开固定LuaLS/API/DBD/CascLib集成与race/process/addon/package通过，Go产品语句24734/32122=77.0002%；另跑WGC probe/原汇总失败，整CI并非全绿。这是旧提交阶段值，本批原生结果待重测。
- 当前R22冻结924文件（aggregate `dcae36da3ad5bc001f5c4ac91bd50b5e098b9784ce91a5247787aa921f30aaeb`）：build/vet通过，完整Lua必需Linux套件2576 test/subtest pass、3既有平台fail、28skip；30包pass、command1包fail、4无测试。失败只为 `TestLiveConnectArgumentAdmission`、`TestExecuteContractRejectsIncompleteRequestsBeforeWorkspaceAccess`、`TestSourceResearchThroughCLI`，不称Linux全套通过。起终924文件SHA一致。
- R16/R19数据保全与恢复正式边界、旧实现负对照、四项故障mutation及定向race通过；R20新增12个memory、8个Windows分类叶节点，R21新增11个Windows关闭wrapper叶节点。Windows函数逐字抽取测试与交叉编译仅是离线证据，本批原生待CI。撤回的R16-01合成bootstrap用例不计当前修复或覆盖。
- R22独立复核114个consumer事件、69个race事件、31个相邻Go/Lua事件及13个Node测试通过；精确旧实现负对照仍失败，8个安全控制通过，无新增可操作离线缺陷。
- 完整Node工具85pass/6 Windows-only skip/0fail；当前Skill21文件/84命令/228引用0违规，生成目录、版本3.1.2、import合同及文档路径检查通过。R17/R18的17场景是独立规范决策复核，不冒称真实模型重放或用户安装环境更新。
- 上次Windows 20skip中9个子进程helper入口均有父回归执行，11个真正可选项为3 desktop、1客户端read-only、4安装/index/图像原材料、2原runtime replay、1宿主8.3能力。WGC主套件skip与独立probe失败分别记账。Go语句比例不代表Lua/Node、业务场景或游戏覆盖，CLI子进程不会自动计入instrumentation。
- 冗余专项已删除两处无新增边界的重复输入及仅服务退役解析器的测试；保留独立失败、并发、跨进程和平台合同。当前高风险补测不追任意比例；修正后按精确提交重测。

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
| R20 现场因果未确认 | 后续匹配当前协议CLI/addon、隔离新CON，同进程切角色/reload后原bind无回执；有新鲜替代证据才无输入退役，暂不可读无证明时保留未知，中断后续清理且正常关闭不退化。旧3.1.2日志仅用匹配CLI诊断，当前driver不得接管 | |

本轮原6项之外新增R20现场复验1项，共7项客户端整合；WGC另列Windows非游戏验证。七产品旧矩阵见 [todo.md](todo.md)，未扩展为本轮通过。

## 候选与特定环境复验

| 项目 / 当前状态 | 必要环境、最小步骤与预期 | 复验结果 |
| --- | --- | --- |
| R3-C01 候选 | 固定真实SpellMisc DB2/DBD/DifficultyID多匹配资料；比较全表、导航及领域合同，证据冲突才升级 | |
| R3-C03 候选 | 先明确instances归属语义；隔离Windows owner fixture，对照展示与连接消费者，不把被动字段当阻断 | |
| R4-C01 已修复/原生通过 | Windows双真实卷、隔离release/client/project、PID=0；比较同卷及跨卷默认/显式archive，旧树应保全并在安全激活边界停止 | R8 `7566493c` 双真实卷及同卷对照通过 |
| 原安装/index/图像与runtime材料缺口 | 取得获准、匹配hash/build的原始资料后跑正式replay；身份/字节/解码/根证据应吻合，合成fixture不代替原材料 | |
| R12-C01 WGC候选 | 交互图形Windows/Go1.27.1，无需WoW；LYCHEEDEV_TEST_DESKTOP=1运行desktop包TestWGCLifetimeBetweenSessions（-count=1 -timeout=1m），仅自建窗口。预期10次采帧/释放、delegate归零。已知Go/SDK两窗口均0x80070057；原因未定，留存SDK/原日志，不判unsupported | |
| Live read-only可选缺口 | 后续授权Windows/WoW固定目标；只读核对身份和窗口，不扩展输入许可 | |

## 下一步

1. R17规则已推送0f26b9a7；WGC SDK证据已取得，收口为单列未完成观察，真实失败及Windows非游戏复验保留。
2. R19-01清理/R19-02终态修复与R20两窄读取/分类修复已实施；R21补充的wrapper组合缺口已修，R22独立复核与整套已结束，提交推送，再测原生/覆盖/skip。R20现场因果未确定，已装合同已核实，原CON/target及相关scans待用户提供，不把其他复现当现场根因。
3. 协调者同步审计/todo/PR当前结果。暂存材料仅在忽略的 `.tmp/pr13/`，任务结束按核实绝对路径清理；保留活动恢复记录及指定Rust worktree。
