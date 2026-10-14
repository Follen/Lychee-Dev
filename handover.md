更新日期：2026-10-14，Asia/Shanghai。当前状态见下；详细问题与逐轮证据见 [审计报告](docs/Clean-repo-2026-10-14.md)，协作规则见 [AGENTS.md](AGENTS.md#multi-agent-handover)。

## 分支与交付

- [PR #13](https://github.com/Follen/Lychee-Dev/pull/13)：`codex/fix-3.2.0` → `main`，保持草稿。当前已推送 SDK 对照诊断 `310a5d7d`（tree `75770dbc`）；[Windows CI](https://github.com/Follen/Lychee-Dev/actions/runs/38555316327)尚在运行，不能填写原生通过。
- 本次 Skill 批次 R17-01：主 Skill、source-research、source-preparation 已修订；全新 R18 独立复核17个场景，无可操作冲突，正式合同检查通过。规则默认用固定 Source，空结果先核模式/关键词/版本/覆盖，禁止猜 raw 路径；真实能力缺口或明确外搜请求保留例外，已有充分证据直接回答。
- 已交付 R3/R4 的13项、R5–R9确认修复与清理，前42项作为基线，不反复计数。R10–R15在各自冻结范围的独立复核结论见审计；不能替代后续产品变化的复核。
- [PR #12](https://github.com/Follen/Lychee-Dev/pull/12)已关闭、未合并。必要 Definitions 冷读预算修复随 `ef9b0113` 移植；旧地区槽位算法与现行 legacy 合同冲突，未移植，不是整批 cherry-pick。
- 发行源仍 `3.1.2`，渠道 `next`、`latest=2.5.1`；`3.2.0`仅开发基线。不合并、不发布。本任务只用仓库 handover.md，未连接用户电脑/游戏、发键或部署游戏目录。

## 当前工作与写入边界

| 范围 | 当前状态 | 负责人 |
| --- | --- | --- |
| R16 高风险补测与修复 | 六个正式测试覆盖五个独立边界；确认 R16-01 原 bootstrap 拒绝回收失败恢复、R16-02 回收完成前逻辑状态过早推进，两项最小修复已在本地，尚未推送 | 实现者仅 evidence/vault/channel 六文件，已冻结；全新 R19 只读复核 |
| R17-01 Skill | 三文件已冻结，合同与 R18 独立场景复核通过；本次提交不混入 R16 产品改动 | Skill 实现者只写 Skill 目录，已完成 |
| R12-C01 WGC | 可见交互桌面、IsSupported=true，两个自建窗口均失败；Go/SDK同进程同HWND对照已推送，等待原生诊断，不提前定性宿主或产品 | 协调者及 desktop 专组 |
| 整合与文档 | R16 完整 Lua 必需套件运行中；后续精确提交的 Windows 原生、覆盖率及 skip 重测待执行 | 协调者独占 workflow/tools/审计/todo/handover/Changelog |

持续审计→修复→验证→推送，直到独立复核不再发现可离线修复项。新确认问题继续处理；实机相关只保留复验步骤与空白结果，不等待用户开游戏。

## 已实际完成的验证

- 工具链：官方 SHA 校验 Go 1.27.1、Node 24.18.1、npm 11.18.0、Lua 5.1.5；`LYCHEEDEV_REQUIRE_LUA51=1`及明确解释器路径。
- 最近完整有效 Windows 主套件 `787eff3b`：[CI38551956825](https://github.com/Follen/Lychee-Dev/actions/runs/38551956825)，2800事件pass/0fail/20skip，2545叶节点pass。24737/32122 Go产品语句=77.0095%；实际merge `729e5813` 的tree与head一致。全部16项公开固定LuaLS/API/DBD/CascLib集成实际通过；race/process/addon/package通过，只有另跑的WGC probe及汇总失败，整CI并非全绿。
- 20skip中9个子进程helper入口均有父回归执行，11个真正可选项为3 desktop、1客户端read-only、4安装/index/图像原材料、2原runtime replay、1宿主8.3能力。WGC主套件skip与独立probe失败分别记账。Go语句比例不代表Lua/Node、业务场景或游戏覆盖，CLI子进程不会自动计入instrumentation。
- R16 三受影响包364事件pass/0fail/0skip，六个新测试race×3共90pass；build/vet与Windows交叉编译通过。旧实现11个叶节点负对照失败、9个回收后保存失败对照保持通过；四个故障mutation均在预期断言失败。正式文件无overlay定向重跑通过；完整套件与Windows原生待实际结果。
- R17 三Skill文件：Skill合同21文件/84命令/225引用、0违规；生成命令目录、版本3.1.2检查及既有Skill合同测试2项通过。R18是17个规范/场景决策复核，不冒称真实模型重放或用户环境部署。
- 上次完整Linux Lua必需套件2506pass/3既有平台fail/28skip，30包通过、command1包失败、4包无测试；失败为两个Native Live参数路径及非Windows Source broker prune。当前R16完整重跑尚未结束。交叉编译、跳过和旧head结果均不冒称当前原生通过。
- 冗余测试已删除两处无新增边界的重复输入及仅服务退役解析器的测试；保留独立失败、并发、跨进程和平台合同。R8清理后73.9475%、R9 74.2544%均为各自提交阶段值，不替代最终重测。

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

七产品旧矩阵见 [todo.md](todo.md)，未扩展为本轮通过。

## 候选与特定环境复验

| 项目 / 当前状态 | 必要环境、最小步骤与预期 | 复验结果 |
| --- | --- | --- |
| R3-C01 候选 | 固定真实SpellMisc DB2/DBD/DifficultyID多匹配资料；比较全表、导航及领域合同，证据冲突才升级 | |
| R3-C03 候选 | 先明确instances归属语义；隔离Windows owner fixture，对照展示与连接消费者，不把被动字段当阻断 | |
| R4-C01 已修复/原生通过 | Windows双真实卷、隔离release/client/project、PID=0；比较同卷及跨卷默认/显式archive，旧树应保全并在安全激活边界停止 | R8 `7566493c` 双真实卷及同卷对照通过 |
| 原安装/index/图像与runtime材料缺口 | 取得获准、匹配hash/build的原始资料后跑正式replay；身份/字节/解码/根证据应吻合，合成fixture不代替原材料 | |
| R12-C01 WGC候选 | Windows runner自建窗口；同进程Go/SDK调用同HWND并记录立即restricted error，随后在支持环境完成10次采帧/释放、delegate归零。已知可见交互桌面两窗口CreateForWindow均0x80070057；原因未定 | |
| Live read-only可选缺口 | 后续授权Windows/WoW固定目标；只读核对身份和窗口，不扩展输入许可 | |

## 下一步

1. 发布已复核的 R17 Skill 规则；取得 `310a5d7d` SDK/WGC 原生证据，按具体原因处理，不添加无依据的平台限制或静默skip。
2. 完成 R19 对 R16 六文件独立复核及完整套件，修复确认项；推送后运行精确新head的Windows全部检查、覆盖及skip统计。
3. 协调者同步审计/todo/PR当前结果。暂存材料仅在忽略的 `.tmp/pr13/`，任务结束按核实绝对路径清理；保留活动恢复记录及指定Rust worktree。
