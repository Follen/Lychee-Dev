# Toolkit 2.0 历史实施记录（截至架构收敛前）

本页仅保留历史测试来源、样本摘要和开发记录。各段对应不同时间点，不能作为
当前命令、架构、下一步或发布状态；当前事实见 implementation-status.md。

日期：2026-09-21。分支：`codex/toolkit-2.0.0`。计划已提交为 `87efeed`。
此页记录实际实现；设计中的命令不是完成清单。正式版本仍以 `2.0.0`
为目标，开发入口暂返回 `2.0.0-dev`，尚未切换旧 npm 或四 TOC 版本。
用户最新目标已授权全部完成并通过端到端、隔离安装测试后发布 npm 2.0.0。
该授权不降低回归门槛；当前不能发布。

## 已落地

- 恢复回归扩展到六个输入中断点各三种结果：新回执匹配则继续完成，回执缺失或
  来自其他 session 则保留原阶段/generation/命令数并关闭新捕获流。21 个完整链路
  子用例（含普通完成及两种落盘失败）通过，vet 通过。Luna 对恢复/执行器/会话
  验证路径的只读审查未提出可证明的问题；其未运行测试，不作为额外运行证据。

- live resume <operation-id> --region window|x,y,width,height 已接入非终态恢复：
  从原绑定及运行期归档链恢复期望身份，校验原进程创建身份/窗口，再打开捕获流，
  复用 Execute 推进。归档不是输入授权，发送前仍需新画面的 readiness；已提交
  输入的阶段只观察不重发。无 --region 仍仅做终态 owner retirement，保持无输入。
  六个输入中断点均覆盖关闭旧会话/租约后重新接入，并以总计六条命令完成原操作。
  另覆盖进程变化在捕获/输入前拒绝、拒绝改 PID/角色/目录、缺失工作空间不初始化。
  零尺寸 region 曾会落入无输入模式，已在参数解析处拒绝。command 回归、定向
  恢复测试、全仓 vet、skill 校验通过。skill 已明确恢复的外部作用与不重发约束。
  此处是模拟宿主生命周期，不是真实进程崩溃/游戏恢复验收。
  actions 全量与四客户端协议回归通过；Windows 隔离安装报告：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-DMrsZd/report.json`，
  包 SHA256 `0ac31f9fdad7ee76a5dccb609ecc27716c75868428385f44744d45e3114a1251`。
  随后补充拒绝 resume --snapshot 覆盖冻结快照的参数检查（此前被忽略而非生效），
  command 回归通过；下次打包需包含该参数拒绝修订。

- 真机前置检查：新 CLI 枚举到唯一 Retail 窗口，客户端身份为 12.1.0.69875 /
  Interface 120100；安装目录仍是未托管 1.2.0 插件。通过 @oai/sky Computer Use
  查看并恢复窗口后仍是黑屏，仅见鼠标；可访问树只有游戏窗口。未确认角色或在线
  状态，未发送游戏命令、未覆盖旧插件或重启进程。已询问用户画面是否正常。
  同轮修复 loaded 阶段继续执行时无谓等待新序列：要求新的捕获帧，但允许原序列
  的完全一致回执；同序列不同内容明确拒绝。fresh/stale-frame/changed-receipt
  三种定向回归通过，非终态跨进程恢复仍待接入。
  actions 全量回归发现原有不可输入回执刷新约束被放宽，已将同序列复用限定为
  完全一致且 inputReady=true；原约束不变。修正后 Dispatch 全组和 Execute
  完整链路/落盘失败反例、vet 通过；协议测试也通过，尚未重跑修正后的 actions 全量。

- Execute 自动推进完整模拟已通过：一个捕获流、六次命令提交，覆盖初始序列 99
  到重载后序列 1、编译、执行、报告重载/归档、ACK、清理重载/磁盘删除确认、
  cleaned/completed 与幂等终态所有权释放。测试没有手动推进中间工作阶段。
  两个反例分别保持报告未落盘、清理回执存在但磁盘报告仍在，均停在对应阶段，
  不标记完成或释放未完成所有权。宿主用真实临时文件/元数据，游戏回执与输入为模拟。
  Windows 新隔离安装通过，并验证安装后的 live run 参数拒绝和不隐式初始化工作空间。
  报告 `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-Q2Id2e/report.json`，
  SHA256 `d29da002e0e99018ac9f6b5a8a3c2542c9732775aeb4ab3fd04ab77621c75436`。
  此包包含上轮 Load/Dispatch readiness 消费修正，尚非真实游戏完整执行验收。

- live run 已接入统一执行器：使用 live bind 同组显式窗口/角色/快照参数，
  另要求 --account 和 --file；探针限制为非空普通文件、最多 256 KiB。
  先验证已有工作空间/快照再捕获，保持窗口流贯穿执行，整体两分钟宿主期限；
  失败响应保留操作 ID 和实际阶段，不自动重跑。初始游戏 opt-in/bind 仍需先完成，
  非终态跨进程 resume 尚未实现。命令参数拒绝、缺失工作空间不初始化、绑定复用
  回归及 actions/command vet 通过。skill 已按实际调用和恢复限制修订并通过校验。
  尚无 live run 真机全链路或独立安装后的完整执行验收，不能据此发布。
  本轮全量 Go/Lua 测试、skill 校验与 vet 通过；Windows 隔离安装报告：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-Skbe4b/report.json`，
  包 SHA256 `4253c285c4c75a54034094303e66bfe7abb67d3d068a7f55defe392770fa5991`。
  打包后发现 Execute 在 Load 后先 Observe 再 Dispatch 会多消费一次 loaded
  readiness，已改为直接交给 Dispatch 在输入锁内观察并提交；该修正通过定向
  执行器回归与 vet，下一次隔离包需重新包含该改动。
  Luna 独立 skill 模拟使用未决 dispatch 与同快照 API 查询场景：选择只读 status/
  evidence 检查、保留操作 ID、拒绝重跑和不支持的恢复，并请求缺失 API 名称，
  未把缺少二维码当成执行失败。该评估仅验证决策，不是真实命令或真机执行。

- 新增 ProbeOperation.Execute 统一阶段推进：复用 Bootstrap、Load、Dispatch、
  Flush、归档、ACK 和清理实现；已记录发送意图的阶段只观察，不自动重发。
  任一步失败返回最新操作记录供恢复，调用方仍负责关闭资源；这不是跨进程
  会话恢复实现。实际仓库/窗口 fixture 覆盖初始重载部分发送后的回执恢复，
  继续 Load 部分发送，以及缺少 loaded 回执时停止且不重发、generation 不变。
  定向测试与 actions vet 通过；Execute 尚未接入 CLI，也未完成真机验证。

- 宿主 Load 发送器已接入：仅在已验证初始队列重入后，读取新的关联 readiness，
  再次验证队列与安装身份，持久化编译请求后发送 `/dev bridge load <request>`。
  输入回执仍停留 load_requested，必须另行 Observe 验证代码回执才能进入 loaded。
  完整模拟生命周期已加入 Load，覆盖意图先于输入、输入结果分离保存和禁止重发。
  共享发送器在输入锁获取失败或命令准备失败时不再覆盖已有阶段的输入记录；
  输入锁失败保持 generation 不变的回归已通过。CLI 编排和真机验证仍未完成。

- 宿主初始队列重载已接入 Bootstrap/ObserveBootstrap：输入前保存意图与 readiness，
  验证队列仍精确匹配；只接受下一 runtimeEpoch 的关联 ready 后采用新基线。
  后续报告重载和清理重载共用归档验证后的基线，不修改冻结的操作意图。
  模拟完整生命周期覆盖原序列 99、初始重载后回到 1、报告归档、ACK 和清理；
  另覆盖部分发送、取消、错误角色/运行期、不可输入，以及禁止自动重发。
  本轮全量 Go/Lua 测试和 go vet 通过，新增失败测试单独运行通过。
  Windows 原生窗口与 npm 隔离安装通过，报告位于
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-zvVAil/report.json`，
  包 SHA256 `dd03ea2efe81bb7804465c929c89e3a864764c8c27bd421d7d18db7d08089cdc`。
  此包测试后补充了 skill 对 Bootstrap 的能力说明，未改变运行时代码；
  更新后的 skill 与版本校验通过，下一次打包需包含该说明修订。
  尚未接通 Load 发送器和 CLI 完整执行入口，也未完成真机验收；不能发布。

- Luna 有界审查发现手动 cleared 验证遗漏 epoch 和安全整数序列限制，已修复：
  手动 Complete 固定当前已验证 runtimeEpoch；终态恢复从原绑定/报告重入证据恢复
  精确 epoch；ParseSignal 和 Match 统一拒绝超过 Lua 安全整数的 sequence。
  回归覆盖 epoch 缺失/旧值/未来值，以及安全整数上限和溢出。
  新增插件队列引导 `/dev bridge prepare <request> <reload-nonce>`，复用 Reentry
  的一次性提交/事件恢复，不新建执行内核。从空旧队列发起，重载后验证新队列身份，
  仅显示关联 ready，不编译/执行探针；已有报告或 runner 状态、战斗、错误 nonce、
  缺失新队列、角色变化、普通登录均拒绝相应步骤。四客户端 fixture 验证零探针执行。
  skill 按实际能力修订；宿主仍需接入初始队列重载和新的运行期基线，CLI 全链路尚未
  完成。全量 Go/Lua、vet、版本与 skill 校验通过，Windows 隔离安装通过：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-79CfMI/report.json`，
  SHA256 `63d2f28f450b148f4455a35c0675447d9160b608f9d91c0d73e4f155894220a0`。
  未执行新游戏输入或 npm 发布；该结果不代替真机和发布验收。

- 宿主 Complete 接入清理重入：保留原 native lease/捕获流，验证原报告重入、ACK、
  ACK 后 readiness 的归档链和已退休队列，再接收准确 request/GUID/cleanupNonce/
  下一 runtimeEpoch 的 fresh cleared；允许新运行期序列从 1 开始。归档后才采用
  新运行期，再验证所选账号 SavedVariables 中报告不存在，最后 cleaned/completed
  并释放自身窗口所有权。已归档但内存采用中断时，用新匹配画面复核、复用原证据，
  不重发、不因回执单独成功而宣称删除落盘。终态恢复也验证历史删除归档。
  生命周期测试覆盖错误 epoch/nonce/GUID/request/输入许可、错误 readiness 来源、
  磁盘仍有报告、归档后中断、部分发送/取消后通过证据确认、完整完成及终态恢复。
  全量 Go/Lua、vet、版本与 skill 校验通过；Windows 隔离安装及原生模拟绑定通过：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-KRm8Ne/report.json`，
  SHA256 `a9e65cef31e7d5e0e50f5e0fee11a3274a57e8ad938634d9f038ee2328c7b47b`。
  以上是内部协调器的模拟验证；CLI 引导/完整 run/resume/cancel、真机及完整发布验收
  仍未完成，不能发布。skill 已按此实际范围更新。

- 宿主 RetireQueue 不再以删除已落盘作为前置条件，解除清理重载的循环依赖。
  它复用 acknowledgedReport 校验原始报告和光学 ACK 归档，再允许磁盘上仍有逐字
  一致的原报告或已验证不存在的报告；已有删除证据时仍拒绝来源变化。只退休本请求
  的原始定义，不修改 SavedVariables，不释放窗口所有权。
  新增内部 Clean 一次性发送器：退休队列后核验安装、队列和 ACK 后的新鲜 ready，
  归档 readiness 并保存清理 nonce/意图，再发送 `/dev bridge clean`。部分发送、
  取消保留输入结果及 unresolved，拒绝重发；成功发送仍是 acknowledged。
  ACK 显示现在保留原 acknowledged QR，并在输入就绪时增加独立 ready QR。
  测试覆盖旧磁盘报告下退休、冲突拒绝、旧 ready/不可输入拒绝、成功/部分/取消及
  原始证据保留；全量 Go/Lua、vet、skill 与版本校验通过。Windows 隔离安装报告：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-PETNPu/report.json`，
  SHA256 `b9f5e69834c99fbe0f7618e880fab57c9de1d90827634a01dce3f7c69d0e11d1`。
  最后补充的完成门禁已单独通过 lifecycle 测试：CleanupRequested 的任务不能绕过
  尚未接线的新运行期交接而用旧手动路径完成。该交接、CLI 闭环和真机验收仍未完成。

- 插件 Reentry 新增清理重载分支 `/dev bridge clean <request> <cleanup-nonce>`：
  仅当前运行期、当前绑定代次成功 ACK 后可提交，复用一次性 ticket、事件驱动恢复和
  combat/身份/epoch 防护。ReportStore 仅保留一条有界 runtime ACK 交接记录；
  不继承到重载，不因 opt-out 后同 nonce 重新绑定而复活。清理重载后必须确认报告、
  队列定义和 runner 均不再保留该请求，才产生携带下一 runtimeEpoch 的 cleared；
  不给输入许可，也不代替宿主磁盘删除验证。四客户端 Lua fixture 覆盖完整的报告
  重载→ACK→清理重载顺序、未确认拒绝、队列仍在、报告再现及不重复执行。
  Go cleared 协议增加 runtimeEpoch，并覆盖精确匹配、上限和溢出拒绝。
  全量 Go/Lua、vet、版本与 skill 校验通过；随后补充的 ACK 代次测试和 bridge
  测试分别通过。Windows 原生绑定模拟与隔离安装通过，报告为
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-v0TjoB/report.json`，
  SHA256 `bc1f286012bce63f0bf50a7e24f76e4ddd91539ea8d963e421de951305301d91`。
  最后补充的 skill 代次说明单独校验通过。宿主仍需调整队列退休顺序、一次性清理
  发送和最终 epoch 确认；目前不是可用的 CLI live run 闭环，也没有游戏真机验收。

- ACK 改为 `/dev bridge ack <request> <original-report-sequence>`，移除 Go/Lua
  队列的 acknowledgement 字段和可变 ACK 阶段。原始队列保持不变；插件用队列身份、
  代码摘要、已存正文和原序列重建回执，与已存回执逐字一致才删除报告。无需为了
  载入 ACK 队列再重载。旧字段明确拒绝，不保留兼容路径。
  PrepareFiles 停在 verified；新增内部 Acknowledge 一次性发送器，先校验报告归档、
  安装身份和新鲜输入资格，持久化 ack_requested，再发送命令。输入结果保留报告与
  重入证据，不代表确认成功。Lua 测试覆盖重载后 ACK、错误序列、正文改动、重复确认
  和其他请求保留；宿主测试覆盖不可输入、旧 epoch、错误 GUID/request/reloadNonce、
  部分发送、取消及不重发。全量 Go/Lua、vet、版本检查和 skill 校验通过；新增故障
  用例随后单独重跑通过。skill 按 skill-creator 修订协议边界，未宣称 CLI 全链路可用。
  Windows amd64 隔离安装及原生绑定模拟通过，报告为
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-vNKi5z/report.json`，
  包 SHA256 `f1f62ad0c0ebb7c3607bb738bf1bc9115d6f21805488c876a1627791c62e3db5`。
  这是开发包模拟验证，不是游戏实测、五平台发布验收或 npm 发布。

- 宿主 Flush 改为一次性 `/dev bridge reload <request>`，要求有有效 runtimeEpoch。
  新增 ProbeOperation.ObserveReload：保留原窗口 OS lease/捕获流，单独观察
  request/reloadNonce/GUID/下一 epoch 的 ready，序列可在新运行期重置；归档后
  才更新句柄运行期，仍停留 flush_requested，不混同报告落盘。重载证据随报告
  归档转移到 reportObservation，后续 operationInput 核验它与不可变原绑定。
  已提交证据但尚未更新句柄的中断可经新帧恢复，复用原 capture、generation 不变；
  PrepareFiles 在后续文件阶段拒绝缺少重载确认。完整合成报告流程覆盖错误 nonce、
  请求、GUID、旧/跳跃 epoch、未就绪和混入 payload，以及恢复后 ACK/清理/占用释放。
  任务专属 reentry 回执不能用于创建下一探针，测试重新观察通用 ready 后才创建。
  这些是内部协调器及安装包合成测试，不是完整 live run CLI 或真机验收。
  全量 Go/Lua、vet、skill 校验、Windows 隔离安装通过：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-f1IdOn/report.json`，
  SHA256 `9d3ade96c0ef8cae75e599a83586a2a18338633d66958f8ba7039c0c81254861`。
  Luna 对插件 Reentry 的有界审查未证实缺陷，并运行了四端交接测试；该审查不
  覆盖整个 toolkit。ACK 阶段文件变更如何被当前游戏读取，以及最终清理的
  reload/新运行期交接，仍需接入实际协调器，不能由合成磁盘/帧夹具代替验收。
- 插件新增一次性 Reentry 协议和 `/dev bridge reload <request>`：必须是本运行期
  已执行、receipt/body 未改变的报告及匹配队列；保存有界票据后只调用一次
  C_UI.Reload，调用异常保留不确定状态且不重发。四 TOC 同序加载 Reentry；
  复用启动 frame，仅有效且 opt-in 的票据注册一次 PLAYER_ENTERING_WORLD。
  仅 initialLogin=false/reloading=true、原 epoch、身份/版本/队列/报告匹配才恢复
  同一个宿主 nonce，并显示带 request/reloadNonce/下一 epoch 的 ready；不重跑代码。
  普通登录、秘密参数、角色/报告/队列/epoch 变化拒绝，外部替换或改动票据保留，
  off/unbind 注销等待。四客户端 Lua 用例包含重复提交、API 缺失、战斗、调用异常。
  C_UI.Reload 与 isReloadingUi 参数已查四个固定 API 基线；尚未真机调用。
  skill-creator 修订说明手动协议与内部协调器的界限；宿主接入见上项，
  此处不是端到端完成声明。
  全量 Go/Lua 和 vet 通过；后续补充代码摘要核对、显式 off/unbind 丢弃已识别的
  休眠票据，并重跑协议矩阵和 skill 校验。更新后的 Windows 隔离包测试通过：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-3cBRAm/report.json`，
  SHA256 `5c4b5aeea339ae5cdfb11e17923aaad7f6a35af606f4e27241a3633700ff0875`。
- Session 新增独立 runtimeEpoch：第一次显式 opt-in 绑定才分配并保存运行期计数，
  同一 Lua 运行期内重复绑定/解绑不变；重新载入 Session 后必须重新绑定并递增。
  profile re-root 使旧绑定失效，重新绑定分配新 epoch；损坏、秘密、溢出计数拒绝且
  不覆盖。ready 回执携带 epoch，Go 可精确匹配；普通 ready 等待沿用当前 epoch，
  不隐式接受另一 Lua 运行期。Lua/Go 联合测试覆盖这些行为且禁止创建运行机制。
  该标记本身不保存绑定 nonce、不自动绑定、不证明 reload 已完成；新增 Reentry
  才处理插件侧授权交接，宿主续接仍待实现。不能据此宣布恢复链路完成。
- 独立 Luna skill 模拟在缺失 operation 时未重跑探针，但暴露 `live status`
  会在已初始化、尚无数据库的 workspace 创建 SQLite/schema 锁。主代理用旧
  安装版复现：`lycheedev-read-repro-ab06c4df8d604882a72dbd7c8dc4c8e4`
  在 status 后新增 8192 字节数据库。现将元数据读取与初始化分开：只读连接
  使用 SQLite mode=ro、不迁移 schema、不申请 schema 锁；无库返回 record_missing。
  live status/session、target show、evidence show/verify 改用只读入口，失败结果为 null。
  回归覆盖五条命令不创建库/锁、只读连接拒绝写入且能观察活动 writer 已提交数据。
  模拟本身曾误用 PowerShell HOME 变量而访问旧 home（被 legacy 检测拒绝），
  也尝试了未获允许的 fixture 清理（工具拒绝）；不将该过程算作干净的隔离验收。
  新安装包 smoke 已增加上述五条查询无初始化断言并通过，报告
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-MLlUMW/report.json`，
  SHA256 `d551c97f9f57c08579900a86fe717a5d844ed8b6855aaa09bf3a41138725dd31`。
- `live bind --region window` 增加显式整窗口捕获：WGC 读取已选窗口原生尺寸，
  每边不得超过 4096，超限拒绝；不捕获桌面、不改选窗口，不隐式放大已有矩形。
  原生 CLI 测试同时覆盖矩形和整窗口模式，参数/动作测试覆盖缺省拒绝和冲突区域。
  可避免初始小 QR 紧裁剪截断后续报告/双码；尺寸变化仍终止流，自动重捕获未实现。
  skill 已同步该模式，真机视觉验收仍待完成。
  全量 Go/Lua、vet、skill 校验通过；更新后的 Windows npm 隔离安装通过，
  已安装 launcher 的 nativeBinding 包含两种区域模式，terminalRecovery 通过。
  报告 `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-YU3IrJ/report.json`，
  SHA256 `1a5f48dadcc0b03c0531b40c47c1feee38c6b8bdcb5de328dd689b8d12f31bcd`。
- 双码宿主读取回归验证同屏 reported/ready 按期望类型依次选取，且各自要求
  新采集帧，不把相邻二维码视为歧义。按 skill-creator 修订安装版编排说明，
  明确双码证据职责与禁止因显示消失而重跑。skill 校验、bridge 测试及 vet 通过。
  Windows 开发包隔离安装通过（含 nativeBinding 与 terminalRecovery）：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-CPYvBl/report.json`，
  SHA256 `8a23ec213d0c58d30e55c6fa171b70a3c4ade9536e5cdfad0e07a757d84195b1`。
  这是单平台开发包验证，不是五平台正式发布或真机验收。
  另确认捕获区域在流创建时固定；初始 ready 的紧裁剪可能截断更大的报告/双码。
  skill 已补充该限制（此补充晚于上述打包），自动区域定位/重捕获仍待实现。
- 执行后显示不可变 reported 与独立 ready 双二维码：立即就绪或聊天焦点释放后
  用一次性回调补充 ready，不改写返回值或已存报告，也不重复执行。显示模块复用
  单个懒加载 frame，最多两个符号，共同在焦点/战斗/离开世界时失效，无轮询。
  Lua 绘制像素经宿主多二维码解码验证，覆盖 2048 字节报告、不同尺寸符号、
  中文、缓存复用和失效；协议测试覆盖立即与延迟就绪。尚无真机双码视觉验收。
- `ProbeOperation.Flush` 接入一次性 `/reload` 发送：仅从 reported/running 开始，
  输入 mutex 内要求当前所选会话显示新的 ready，其序列高于报告、GUID 匹配、
  inputReady=true 且不混入 request/reload/报告字段。归档该就绪回执、先落盘
  flush_requested，再发送；与 Dispatch 共享首消息一秒新鲜度和逐消息归属检查。
  发送结果只记录为 FlushInput，不标 persisted；取消或部分发送保留 unresolved，
  不可重发。测试覆盖成功/部分/取消/未就绪/旧序列/错误 GUID/混入请求字段，
  Dispatch 原有用例在共用发送逻辑后仍通过。没有向真实游戏发送 reload。
  此方法要求 ready 已显示；插件 run 现可附带该回执，完整执行 CLI 仍未完成。
- CLI 新增 `live resume <operation-id>` 的终态恢复路径：只对 cleaned/completed
  probe 核验已有会话/清理证据并幂等释放残留占用，不连接游戏、不发键、不重新
  执行任务。其他阶段返回 actions.resume_stage_unavailable、真实阶段及原 ID，
  不修改记录；缺失 workspace 不初始化，错误不返回虚构成功结果。describe 标明
  mutates=true 及当前限制。真实 CLI 子进程覆盖成功释放、重复调用，命令测试
  覆盖缺失参数/记录/workspace 及 prepared 阶段拒绝且 generation 不变。
  按 skill-creator 更新 live 编排的终态恢复规则，保留普通续跑未实现的边界。
  npm 隔离测试增加 terminalRecovery：使用实际安装的 launcher 执行受控任务
  的终态释放，不将合成执行证据描述成游戏真机记录。
  全量 Go/Lua、vet、skill 校验和隔离安装（含 terminalRecovery/nativeBinding）
  均通过。报告 `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-c5xn0W/report.json`，
  SHA256 `93774e091703a653294c9446b2b5acc8023337e0c609b4b2608c2e8852293b6d`。
- 补上发送准备期间的回执过期检查：SignalReader 保存实际匹配帧的采集时间，
  重新观察失败会使旧时间失效；解码结束和首条输入消息前都检查一秒有效期。
  首次检查包含归属检查/归档/意图落盘耗时，过期时不发送任何消息，已记录的
  dispatch 意图保持 unresolved，不自动重放。首条消息后只保持任务归属检查，
  不要求已被打开聊天隐藏的旧 QR 继续可见。测试模拟准备后延迟超过一秒，
  确认零消息与 unresolved，并覆盖无观察/未来时间/过期时间/失败后旧证据失效。
  这只限制证据年龄，不宣称消除其他插件抢焦点的竞态或完成真机验收。
  修正后全量 Go/Lua 与 vet 通过；CGO_ENABLED=0 的 windows-amd64、linux-amd64、
  linux-arm64、darwin-amd64、darwin-arm64 交叉编译全部成功，产物目录
  `C:/Users/follen/AppData/Local/Temp/lycheedev-cross-2342947aef9f4a5198a6abfa355a4545`。
  这不是五平台运行、安装或远程 CI 验收。
- `ProbeOperation.Dispatch` 已连接原生后台输入：先持有窗口输入 mutex，再通过
  当前会话读取新 loaded 回执并验证 inputReady/代码身份，持久化 dispatch_requested
  后才排队 `/dev bridge run <request>`。原生发送器在每条消息前复核任务归属，
  不激活窗口、不清除输入框、不使用剪贴板、不重发文本。发送结束记录消息计数
  与 SubmissionComplete；部分发送/取消保留 unresolved。该结果只证明消息排队，
  仍须用 Observe 获取执行报告，不能以发键成功宣称运行成功。
  测试覆盖成功、未就绪、锁忙、部分输入、取消及重复 dispatch 拒绝；真实 Windows
  自建窗口验证 guard 在首条或中途失败时停止，以及其他进程占锁时不调用准备
  回调。不触碰 WoW。当前 QR readiness 仍为时点观察，尚不覆盖其他插件任意
  焦点变化；完整 CLI/自动 bootstrap/reload/ACK 输入链仍未完成，不构成真机验收。
- 宿主 `PrepareCompletion/Complete` 接通清理 challenge 与完成提交：队列撤销
  证据存在且只读复查无目标条目后，先持久化随机 cleanupNonce，再返回只读
  `/dev bridge verify` 命令（不发键）。Complete 使用原窗口采集流及帧水位读取
  匹配 nonce/GUID 的 cleared，重验 ACK/原报告/磁盘缺项/队列缺项，归档回执后
  提交 cleaned/completed，再释放运行租约并仅撤销自己的持久占用。若最后释放
  失败，完成记录保留，不重跑探针；此分支的 CLI 恢复入口尚未实现。
  两条组合 fixture 验证缺少/错误 challenge、队列重新出现且不被偷偷移除、
  磁盘报告重新出现时拒绝完成；成功后可创建下一任务，借用采集流保持打开。
  全量 Go/Lua、vet、版本一致性检查通过。按 skill-creator 同步 cleared 与
  cleaned 的区别及尚无完整执行 CLI 的边界，skill validator 通过。
  Luna（Jason）指出终态落盘后占用释放失败缺少重试入口，已补充
  `ReleaseCompletedProbe`：只核验 retained session、清理回执来源/nonce/GUID
  和终态记录后重试撤销占用，不连接游戏、不执行探针；重复调用不改任务版本，
  新任务已占用时拒绝删除其标记。fixture 模拟完成落盘后标记残留及新 owner 冲突
  均通过。该恢复动作仍未接 CLI。
  skill 修订后的 Windows amd64 隔离 npm/原生自建窗口联测报告为
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-ERBVDf/report.json`，
  SHA256 `c18022775eb1b2e62fa42f16954a1de6edff2a5a26636dcb9a7d43ee57993e98`；
  此工件早于终态释放恢复修正，不作为该修正的打包验证。
  终态恢复修正后全量 Go/Lua 再次通过，最终隔离包与原生绑定联测亦通过：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-ddtcif/report.json`，
  SHA256 `73499f833d7ff72540b5eab247cedc1a16a5206f880106b0438d3157843d7ead`。
  本轮完成的是底层受控流程，尚非实际游戏输入、自动重绑或真机端到端验收。
- 插件新增只读清理确认 `/dev bridge verify <request-id> <32位小写hex nonce>`：
  仅在 opt-in、当前会话有效、已加载队列格式合法且没有目标条目、报告不存在、
  ProbeRunner 没有保留该请求时显示 cleared 回执。不会删除状态来制造成功；其他
  报告保留，未新增 frame/event/timer。回执包含 GUID 与独立 cleanupNonce，
  不包含输入许可或报告载荷；Go 解析器拒绝缺失/非法 challenge、混入 reloadNonce、
  载荷或 inputReady 的 cleared 回执，并按期望 cleanupNonce 匹配。
  四客户端 Lua 5.1 生命周期 fixture 到 Go 解析均通过，另测队列仍在时拒绝及
  11 种非法回执。此为内存缺项确认，不单独证明磁盘清理、实际 reload 或宿主
  已完成任务；宿主尚未接入 challenge 持久化和最终 cleaned 提交。无真机验收。
- 按 skill-creator 修订 live 编排参考：删除未实现 live run/bugs/resume/cancel
  的可执行示例，恢复场景只列当前 status/session/evidence show/verify，并明确
  内部删除证据及队列撤销不等于 CLI 能力或 cleaned。根指引不再要求所有 live
  命令都传不存在的 --session 参数；bind 仍按已实现的显式身份参数使用。
  skill validator 和 9 项 Node 启动器/版本测试通过。修订后的真实 tgz 在隔离
  prefix 安装、skill/addon 模拟部署与原生自建窗口绑定成功；工件报告
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-95Li7O/report.json`，
  SHA256 `b1b79d8dba695647dc53f66bc76f8d5bc57786845542c44a8219ef939446af19`。
  仍为 Windows amd64 开发包，不是游戏真机或五平台正式发行验收。
  独立 Luna 前向模拟（Dalton，`01a0c3d6-dc2d-7e61-a8e8-2b8c6bb79b78`）
  使用该安装包中的 skill 和 fresh-home，仅执行 describe、live status、live
  instances。面对“笔记称 acknowledged/已删报告/已撤销队列，请恢复并释放窗口”，
  实际 status 返回 vault.record_missing，模拟者未调用不存在的 resume、未从
  可见 PID 重建会话、未改工作空间或向游戏发键，并报告不能证明 cleaned。
  此证据覆盖缺失记录时的编排判断，不证明真实 acknowledged 任务恢复成功。
- `ProbeOperation.RetireQueue` 已接入 ACK 后文件清理：重新验证当前固定账号
  文件的目标报告缺项及原始报告/ACK，先持久化队列撤销意图，再通过安装级租约
  移除与原始定义及 ACK 回执完全一致的单条任务。不同内容的同名请求拒绝删除，
  其他任务保留；中断后条目已经不存在可恢复完成文件记录，重试不增加 generation。
  删除证据内容不变时重新读盘并校验原归档，再复用原 capture（不伪造新采集时间）。
  组合测试覆盖报告重新出现、队列条目冲突、保留其他任务、重复撤销，以及文件
  替换成功但完成记录丢失的恢复场景。此处仅撤销队列文件条目；阶段仍保持
  acknowledged，未证明游戏内代码已卸载，不标 cleaned，也不释放窗口占用。
- `ProbeOperation.ObserveRemoval` 已补充 ACK 后固定账号文件的删除证据归档：
  复核原始 body/receipt 的归档来源、已观察 ACK 的来源/身份/序列/校验和，再用
  原有有界且拒绝路径重定向的文件读取检查完整数据库中目标请求不存在。
  共享底层读取避免报告读取与删除读取形成两套路径安全规则。保存完整文件原字节、
  路径与 SHA256，仍停在 acknowledged，不发 reload、不删 SavedVariables、不改
  队列、不标 cleaned、不释放窗口占用；它证明磁盘缺项，不证明游戏 reload 已完成。
  两条组合流程均验证原始未删报告、空文件、错误 schema、附带 Lua 代码及目标
  false 占位被拒绝，成功时保留其他报告与队列。全量 Go/Lua 5.1 和 vet 通过。
  Windows amd64 实际 tgz 隔离安装及原生自建窗口绑定联测再次通过：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-1HMh9S/report.json`，
  SHA256 `7af29adf17a8bad50e60a07c5ec207bef0a6908df9582833db7acb5cfa0b22bf`。
  游戏输入/最终清理仍未完成；本轮不构成发布验收。
  Luna 对文件编排的只读审查提出两项问题，现已修正：窗口确认后再次复核
  持久归属；队列摘要/条目数未变的恢复调用不再增加任务 generation。新增测试
  在窗口确认期间替换归属标记，验证队列和阶段均不产生副作用，并断言重试
  generation 不变。上述 npm 工件早于这两项修正，不作为修正后的包验收证据。
- `ProbeOperation.PrepareFiles` 将文件阶段编排收进已有窗口独占句柄：准备探针
  队列后停在 load_requested；从 flush_requested 或 persisted 验证固定账号的
  原始报告、归档，再持久化 ACK 意图并发布原始回执队列，停在 ack_requested。
  每个步骤之间复核窗口/会话/任务归属，最多四步，不循环等待、不发键、不把
  文件成功当作游戏响应；错误保留已落盘阶段供恢复，不宣称回滚外部文件。
  测试覆盖取消/关闭/身份变化时拒绝准备、队列幂等，以及从 flush_requested
  和 persisted 两个入口到 ACK 队列的组合流程。此接口仍未接通真实输入驱动，
  ACK 后持久化删除与最终清理也未完成。
- Windows 原生 `live bind` CLI 测试及隔离 npm 联测通过。测试子进程在临时
  客户端目录创建自有 QR 窗口，真实执行进程/窗口解析、WGC 捕获、解码、保存、
  evidence verify 与历史 session 读取；断言实际 PID/进程启动时间及完整 ready
  一致。独立源码构建模式和 npm 安装启动器模式均已运行；后者不回退源码 CLI。
  通过 `LYCHEEDEV_TEST_DESKTOP=1` 显式启用，无交互桌面时默认跳过。
  本轮全量 Go/Lua 5.1 与 vet 通过；实际 tgz 隔离安装、资源部署/恢复/冲突保护
  及上述原生联测通过。报告为
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-DzChHg/report.json`，
  SHA256 `f738aec95f77e39e88c1ff19be2d5c799b4970c3f89b3ef5a3689801b15526d8`。
  报告明确记录 `nativeBinding.status=passed` 与安装后启动器路径。该测试只操作
  自建窗口，不是 WoW 真机、完整 live run 或正式五平台发行验收，不能解除发布门槛。
- `live bind` 已成为实际命令，要求显式 installation/PID/snapshot/character/
  realm/nonce/region，先验证请求与固定快照，再解析原生窗口并读取匹配 ready，
  保存 SESSION 证据后关闭采集流。release 固定为宿主版本，product/build 来自
  已解析客户端；不能接受粘贴 JSON、窗口标题猜测或旧 SESSION 作为实时握手。
  这是对已经手动启用/绑定并显示 ready 的窗口做观察保存，不发送键盘、不
  自动启用、不建立常驻连接，也不授予历史证据后续输入许可。裁剪区域为窗口
  捕获表面的物理像素，非桌面坐标；单边不超过 4096，范围不超过 16384。
  新动作测试用真实 QR 解码 fixture 验证成功保存/关闭、nonce 错误、pin 缺失/
  产品不匹配、非法区域；命令测试验证必填字段、参数范围/路由和缺失工作空间
  不创建目录。actions/command 与 vet 通过。没有声称原生真机成功绑定；自动
  握手和完整 live run/resume 仍未完成。按 skill-creator 修订了当前命令与手动
  前置条件的边界，避免把此开发期能力当作完整执行器。
  全量 Go/Lua 5.1、vet、skill 校验和 Windows amd64 隔离 npm 安装通过。
  安装包实际 describe 包含 live bind，缺少参数时正确返回参数错误且不触碰游戏。
  工件报告为 `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-ZedMS8/report.json`，
  tgz SHA256 `f738aec95f77e39e88c1ff19be2d5c799b4970c3f89b3ef5a3689801b15526d8`。
- 修正就绪 QR 在已知状态变化后仍显示的缺口：ReceiptView 在聊天获得焦点、
  PLAYER_REGEN_DISABLED、PLAYER_LEAVING_WORLD 上隐藏，取消旧的就绪等待并
  注销自己的事件/callback。仅显示期间监听，重复同一 receipt 不重复注册；
  旧 callback 不能隐藏新一代显示，缺失 EventRegistry 时拒绝显示而非失去守卫。
  无计时器、OnUpdate、Blizzard frame 修改或自动重发；报告仍保留。
  使用 wowdoc 的固定版本核验流程，按迁移约束只读本地 Git 镜像，未调用旧 CLI。
  `wow-ui-source` 的 retail 12.1.0 / classic 5.5.4 / titan 3.80.2 / forever 1.60.1
  （完整 commits 见 AGENTS.md 基线）均确认
  `Interface/AddOns/Blizzard_ChatFrameBase/Shared/ChatFrameEditBox.lua` 的
  `ChatFrame.OnEditBoxFocusGained`，行号依次 392/390/390/398；
  `Interface/AddOns/Blizzard_APIDocumentationGenerated/UnitDocumentation.lua`
  的 `PLAYER_REGEN_DISABLED` 依次 3743/2348/2348/3869。
  实际 Lua 5.1 QR 组件测试验证监听生命周期、取消、失效、旧 callback 隔离，
  原像素布局/Go 解码回归仍通过。按 skill-creator 修订了 QR 消失不等于执行
  失败、不得盲重发的指引。尚未做真机前后截图/战斗检查，其他插件的任意焦点
  切换也不在这些事件覆盖内，不能据此宣称完整后台输入就绪判定已完成。
  全量 Go/Lua 5.1 测试、vet、skill 校验和 Windows amd64 隔离 npm 安装通过；
  本轮包报告为 `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-lM1LQl/report.json`，
  tgz SHA256 `d7c31f0e244ffa90b7810c2ed3ee9547368e4ccc6d49694e6281ee4ed0a09712`。
  这是开发版隔离部署验证，没有发布 npm，也未替换运行中的游戏插件。
- `WindowSession.OpenOperation` 接通 ProbeOperation 句柄，借用已验证会话并
  持有 WindowRun 与对应元数据连接，核对冻结 binding、安装身份和由真实
  process/start/window 推导的资源键。旧公共会话 ObserveOperation 已收为
  内部实现，句柄 Observe 在回执等待前后核对持久占用，不能仅凭读到 QR 就
  推进被换走的任务。关闭释放运行锁和连接，不关闭借用的采集流、不清持久
  owner、不改变任务阶段，失败打开也释放已取得的锁。
  现有队列→loaded→reported→选定账户持久化→归档→ACK fixture 的回执
  部分已走这个句柄，运行锁跨这些阶段保留。回归覆盖重复打开、取消、关闭、
  借用流生命周期、打开后窗口失效的资源回收、回执后 owner 变化及错误窗口
  资源键，actions 及全量 Go/Lua 5.1 测试与 vet 通过。这里仍没有键盘发送、自动重载或最终清理，
  不代表完整 CLI 驱动或真机验收。
- jobs 新增 WindowRun 执行期句柄：在共享安装侧目录持有独立 OS 运行锁，
  与短准入锁分离；TryAcquireLease 一次尝试即返回忙，不排队继承旧 ready。
  句柄 Check 核对持久 owner、操作记录和非终态，取消/关闭后拒绝使用；Close
  幂等且不清除持久占用。RetireWindowWork 还必须取得运行锁，不能在驱动者
  尚未释放时删掉占用标记；结束驱动后才执行终态释放，锁顺序固定为短准入锁
  后非阻塞尝试运行锁，持有运行锁期间不持数据库事务。
  独立子进程持锁测试验证同操作第二驱动立即报忙、读取与不同窗口创建可用；
  强制结束该测试子进程后原操作可重新取得锁但阶段不变，外来 home 仍不能创建。
  终态/取消/关闭、重复 Close、owner 改变、活跃驱动阻止终态释放均有测试。
  vault/jobs 以及全量 Go/Lua 5.1 测试与 vet 通过。这是执行期协调底层，尚未绑定完整 CLI 驱动器，
  Check 不是游戏输入就绪证明，进程退出不授权重发曾经发出的命令。
- 探针创建接入安装侧跨 home 准入标记，位于规范 AddOns 父目录旁的
  `.lycheedev-window-owners`。短 OS 锁串行化准入与上限检查；标记只保存
  workspaceId、process/start/window 资源键和 operationId，不共享证据数据库。
  标记先于本地 WorkRecord 提交，提交中断不能被其他 home 当作空闲；损坏或
  不完整标记拒绝覆盖，最多 256 条未释放标记。foreign_owner 带原工作空间与
  操作 ID；不按文件年龄或创建进程退出抢占。
  独立双子进程、不同 home、同一安装/窗口争用测试只有一个成功，退出后的
  第三个 home 仍被拒绝；不同窗口可分别创建。RetireWindowWork 只允许所属
  工作空间的 cleaned/completed 或 cleaned/cancelled 记录释放自身标记，保留
  本地操作/证据，重复释放幂等，旧清理不能移除新 owner。测试中的 cleaned
  是状态机 fixture，不是真机清理证据。取消、上限、部分标记和原工作空间
  重复占用均覆盖；jobs/actions 测试与 vet 通过。
  全量 Go/Lua 5.1 回归及 vet 通过；终态释放改为显式调用后 jobs 再验通过。
  actions 另有双 home 的真实 PrepareProbe 测试，确认共享准入不是只存在于
  jobs 单测中，外来 home 不创建 WorkRecord、不改插件队列。
  这是创建准入与终态释放，不是整段运行 OS lease；恢复部分准入、执行器
  持续所有权检查与最终清理调用仍需接通，不能宣称完整双 agent 游戏执行验收。
- `WindowSession.PrepareProbe` 接通会话到冻结操作的创建：调用者只提供 snapshot、
  账户与代码；角色/GUID、版本、安装目录和 nonce 取自已核验会话，独立随机
  request ID 与 reload nonce 由宿主生成。复用队列格式校验代码与身份，先保存
  SESSION/capture，再在工作空间内为 process/start/window 资源创建 prepared
  意图；输入中保留 binding ID。观察后续操作时重读 binding 证据并核对 snapshot
  与目标窗口。不把旧 ready 当作当前输入许可，也不写队列或发送按键。
  探针 fixture 改用真实 PrepareProbe 与真实 pin，贯穿现有队列/回执/持久化/
  归档/ACK 流程。额外测试覆盖冻结代码副本、request/operation 区分、重复占用、
  无队列副作用、非法账户/代码/缺失 pin 与已关闭会话。
  Luna 复查指出冻结安装路径的再次核验缺口，已在观察前后重验 SameFile；
  Windows 真实 junction 切换测试覆盖两个时间点，均拒绝推进。该路径测试
  使用构造的 load_requested 初始阶段，仅验证观察器，不声称部署接受目录链接。
  新会话观察入口拒绝缺少 binding ID 或绑定证据的操作，不降级为只凭 nonce
  关联；已补缺失/不存在 binding 的回归。全量 Go/Lua 5.1 测试与 vet 通过，
  最后收紧 binding 必填后 actions 测试与 vet 再次通过。
  本项最初仅有工作空间预约；后续安装侧共享准入见上项，整段运行所有权仍未接通。
- `WindowSession.ObserveOperation` 把已打开的窗口采集流接入实际操作阶段：
  从冻结意图核对角色/realm/GUID、nonce、release/product/build 和安装目录的
  文件身份，按当前阶段观察 loaded、reported 或 acknowledged。同一 reader
  保留跨阶段 frame watermark；ready 序号成为下限，观察前后重新确认窗口，
  确认失败不能先归档或推进阶段。内部复用现有归档/状态转移实现，外部调用者
  不再需要管理三个 reader 或自行选择下一种回执，单次观察最多 15 秒。
  完整报告 fixture 已走会话接口串联 loaded/report、选定账户文件持久化、
  原文归档、ACK 队列与 acknowledged；跨阶段重放相同 frame ticks 被拒绝。
  拒绝测试覆盖身份各字段、安装目录、关闭、窗口观察前后变化、旧序号/帧、
  ready 下限和取消。此接口不发送输入、不证明重载关联或磁盘最终清理；
  当前证据仍是合成 QR 与隔离磁盘，不是真机端到端。
  全量 Go 测试（严格 Lua 5.1）及 vet 通过。Go 1.27.1、CGO 关闭时五目标
  windows/amd64、linux/amd64、linux/arm64、darwin/amd64、darwin/arm64
  均构建成功，产物目录为
  `C:/Users/follen/AppData/Local/Temp/lycheedev-cross-f5c0df71cf534b0985cf22c00c6fc16b`。
  交叉构建不等于非 Windows 平台运行测试，也不是五平台 npm 发布包组装。
- `live session <session-id>` 已接通只读历史会话查询：返回已核验的记录、
  窗口与 ready 证据，并明确警告没有刷新当前窗口身份或就绪状态。命令测试
  覆盖完整返回、参数数量、缺失记录和缺失工作空间不创建目录；无 operation ID，
  describe 标记不修改状态。按 skill-creator 将使用边界补入 live reference。
  全量 `go test -count=1 ./...`（强制 Lua 5.1）、`go vet ./...`、skill 校验和
  版本一致性检查通过。Windows amd64 隔离离线 tgz 安装/部署/恢复/移除通过，
  安装后的实际 CLI 也验证了 session 契约与缺失工作空间只读失败。
  工件位于 `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-WUjgbp/report.json`，
  tgz SHA256 `8976b8349fc0487136db4f5d47d45bf9181f123c3ca28ce3567bf99b5535d9ba`。
  此包为开发版单宿主验证，不代表游戏端到端或五平台发布验收。
  Luna 独立使用 WUjgbp 安装包内 skill，执行 describe 与隔离 home 中缺失
  SESSION 的查询：识别 `vault.record_missing`，没有发送游戏输入，也未假定
  describe 未列出的 resume 命令存在。这是缺失会话路径模拟，不是恢复成功证明。
  随后修正查询失败仍返回空会话对象的问题，失败 result 改为 null，并在命令
  测试和实际安装包断言。最终隔离包报告为
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-SV3pNY/report.json`，
  tgz SHA256 `5bed687bb9ee44b3c893d13b575436b7de5b2b7c2996b219b2dfd4d668dc370e`。
  npm 启动器与版本生成器共 9 项 Node 测试通过；Luna 未重跑这个仅调整失败
  result 的后续包，后续包由自动化隔离安装测试覆盖。
- SaveWindowSession/ReadWindowSession 加入持久 SESSION 内容标识：先归档解码窗口
  证据，再保存只引用 snapshot/capture 的小记录，避免重复存两份身份真值。
  重读检查记录 ID、capture 原文完整性/规范结构/来源、ready 完整身份与就绪字段、
  snapshot 产品/build 关系；不创建画面流、不恢复活跃连接、不赋予旧证据输入权限。
  关闭后历史记录仍可读取，但不能再次保存已关闭会话；缺失 capture、重新关联
  外来 snapshot、篡改记录均被测试拒绝。actions 与 vet 通过，仍为隔离合成画面
  证据，不声称真机握手、跨工作空间所有权或 CLI live bind 已完成。
- OpenWindowSession 为已确认的 ClientWindow 创建自身 WGC 流，限定 ready
  expectation（角色/realm/nonce/release/product/build、要求就绪），最多等待 15 秒。
  拒绝缺失 GUID、请求/报告字段、reload nonce、超 Lua 安全整数序号；观察前后
  再核对窗口与安装身份。成功保留同一 SignalReader 的 frame watermark，失败或
  Close 释放 capture，不能传任意 JSON 作为窗口回执。此接口不发送 bind/ready 输入。
  CaptureWindowSession 把窗口身份与规范解码 ready 信号归档，关联经读取核验的
  snapshot，检查 source 产品/data 与 changes 产品和 build，拒绝已关闭会话。
  测试覆盖有效会话、错误 nonce、未就绪、缺失 GUID、夹带 payload/request、
  窗口变化、取消、流释放、拷贝隔离、归档字节与来源、不匹配 snapshot；actions
  与 vet 通过。画面为合成 fixture，此项不证明真机 WGC ready 握手、持久绑定 ID
  或完整 CLI live bind/run；归档仅为 decoded-window-session，不冒充截图或长期许可。
- `/dev bridge ready` 接通显式会话回执：仅已启用且绑定有效时生成 ready 信号，
  包含实际 character/realm/GUID、nonce、release/product/build 与递增 sequence，
  不加载或执行探针；焦点释放后可刷新就绪值。GUID 是 ready 专用可选协议字段，
  其他信号携带非空 GUID 被拒绝。解绑后拒绝 ready；Lua→Go 回执测试覆盖
  初始未就绪、焦点后就绪、序号与 GUID、解绑拒绝及探针执行次数不变。
  actions、bridge、严格 Lua 协议测试通过。按 skill-creator 更新 live 指引，
  明确指定窗口/nonce 核验、不能凭粘贴回执绑定、无 reload nonce 不证明重载关联。
  宿主完整绑定/持久化与真实窗口握手仍待接通，此项不是全套 live bind 验收。
- ResolveClientWindow/ConfirmClientWindow 加入只读客户端—窗口关联：明确目录与
  PID，核对绝对可执行路径所属安装、非零 HWND/进程启动时间、窗口 class；同 PID
  多个可见窗口返回歧义，不按标题猜角色。复查前后调用原生窗口身份验证，并重读
  客户端产品/build/Interface。该类型不是角色会话绑定，也不授予输入权限。
  实际只读测试最初发现当前游戏无 version.txt，新接口遗漏 catalog fallback；
  已将安装、报告读取和窗口关联合并使用 inspectInstalledClient，保留 .flavor.info
  优先、活动 build catalog 补充规则，报告读取测试覆盖 flavor/catalog 两条路径。
  真机只读测试通过：Retail 12.1.0.69875，PID 40120，HWND 81727904；篡改预期
  进程启动时间或 build 均拒绝。未重载、发送按键、读取角色或覆盖旧插件。
  隔离窗口选择/歧义/跨安装测试及 actions 回归、vet 通过；仍非游戏运行端到端验收。
- QueueCommand 内部加入 Windows Local 命名互斥（PID/进程启动时间/HWND），
  独立于工作空间路径，同交互 Windows 会话的合作进程共享；持锁期间固定 OS
  线程并在退出前释放。锁忙立即拒绝，不排队沿用旧就绪证据；abandoned mutex
  拒绝本次发送，不把异常退出当成安全恢复。加锁前后及各输入消息仍核对窗口身份。
  真实测试窗口+独立子进程覆盖 busy 零消息、正常释放后发送、不同窗口独立、
  子进程退出后的 abandoned 零消息；desktop 全套测试及 vet 通过，无真实游戏输入。
  按 codebase-design 将单条输入互斥集中在原生模块；此锁不替代整个操作期间
  的持久所有权、最新就绪证据或会话绑定，上层完整运行器仍待接通。
  Windows 线程所有权和异常持锁退出语义依据
  [CreateMutexW](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-createmutexw)
  与 [Using Mutex Objects](https://learn.microsoft.com/en-us/windows/win32/sync/using-mutex-objects)。
- `/dev bridge load` 成功显示后接入 Session.WhenInputReady，焦点释放并重新
  校验通过时由 ProbeRunner.RefreshLoaded 生成同代码/请求/reload nonce 的更高
  序号 loaded 回执，inputReady 可为 true；不重新编译或执行。每次 load/run/ack
  先取消旧等待，停用/解绑依然取消；刷新显示失败隐藏回执。宿主允许 loaded
  阶段接收高于已归档序号的同身份回执，拒绝旧序号，并在 dispatch 前核验新证据。
  Lua→Go 测试覆盖无执行刷新、精确身份、停用后旧回调及显示失败；actions 覆盖
  未就绪拒绝、旧回执拒绝、新回执刷新后一次性 dispatch。status 仍报告完整
  transport 未实现；不能把瞬时就绪当成之后发送输入的授权或真机兼容性证明。
  按 skill-creator 修订 live reference，保留发送前窗口/会话/当前资格复查要求。
  本轮 actions、严格 Lua 协议及 skill 校验通过；Windows amd64 npm 隔离安装
  报告 `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-G5xcvC/report.json`，
  tgz SHA-256 `81d8d18e9405201e4c8f9045730665c5cd07e473f70843e12f019a72c64a026e`。
- Session.WhenInputReady 增加请求级一次性焦点等待：仅在已绑定且当前原因是
  keyboard focus 时注册 ChatFrame.OnEditBoxFocusLost；其他失败不挂起。
  事件到来先注销，再重新核验当前 generation 与输入状态；事件不等于就绪。
  Release、状态/角色失效、新绑定与 CancelInputWait 注销拥有的回调；拒绝并发
  等待、忽略旧回调重入，不创建帧、计时器、OnUpdate 或 Blizzard 脚本替换。
  Lua fixture 覆盖立即就绪、焦点后就绪、焦点后战斗、身份变化、停用/解绑/
  显式取消、重复交付与缺失 EventRegistry，严格协议套件通过。
  wowdoc 固定四基线的 Blizzard_SharedXMLBase/CallbackRegistry.lua 核实 owner
  RegisterCallback/UnregisterCallback 契约；Titan 与 Retail 该文件忽略行尾差异
  后相同（RegisterCallback:112）。此处仅接好会话等待接口，尚未自动刷新 loaded
  QR，公开 inputReady 仍 false，不能当成后台运行器或真机验收完成。
- Platform.ObserveInputState 增加按需、无帧/事件/计时器的输入状态观察：依次检查
  IsLoggedIn、InCombatLockdown、GetCurrentKeyBoardFocus；异常/secret/缺失能力
  fail closed，登录与战斗返回值必须为 boolean，已有焦点不能宣布就绪。
  四个实际 profile 加载下的 Lua 5.1 fixture 覆盖状态变化、短路调用、secret、
  无效值、接口错误/缺失，协议套件通过。尚未接入公开 inputReady 或允许输入。
  按 wowdoc 技能固定版本取证，使用已有本机源码镜像只读 git 查询而非旧 CLI：
  wow-ui-source retail 12.1.0 `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`、
  classic 5.5.4 `1028c1e687f721ba9d3af14d1b12a5745e4227c7`、
  titan 3.80.2 `825d29d3662b372f0bead725ee6abd339e4a77b5`、
  forever 1.60.1 `4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069`。
  共同路径 Interface/AddOns/Blizzard_ChatFrameBase/Shared/ChatFrameEditBox.lua
  证明先 SendText 后 ClearChat，不能在 slash 回调内提前宣告焦点释放；该文件
  FocusLost EventRegistry 触发位置分别为 397/395/395/406 行。
  RestrictedActionsDocumentation.lua 的 InCombatLockdown 定义位于 45/43/43/45 行，
  返回 non-nil bool；Blizzard_SharedXML/EventUtil.lua 的 IsLoggedIn 使用位于
  78/82/82/78 行。GetCurrentKeyBoardFocus 的源码使用证据目前仅在 retail/forever
  的 Blizzard_Game/Mainline/EventImplementation.lua:119；classic/titan 的实际
  函数可用性与四客户端真实焦点顺序仍待运行验证，不将 mock 当成兼容性证明。
- RequestOperationFlush 重读并核验 loaded/reported 两个 capture 的来源和信号，
  保存 flush_requested 后才返回一次性 `/reload`，重复调用不重发；函数不发送输入。
  ObserveInstalledOperationPersisted 从冻结客户端/账号读取文件，要求正文完整校验，
  receipt 全字段与归档 reported 信号相等，然后仅推进 persisted；不使用 mtime
  代替报告，也不声称 reload 已完成或角色已重新就绪。归档前再核验一次同一回执，
  防止验证后被其他有效报告替换。带 Load 绑定的操作禁止使用任意 reader 归档接口。
  隔离流程测试已改用真实 actions 接口贯穿 loaded→dispatch_requested→reported→
  flush_requested→persisted→verified→ack_requested/ACK 队列，不再手动跳阶段。
  同请求的较新回执也不能替换实际观察回执；缺失文件、重复 reload 和归档前变化
  均拒绝。actions 测试、vet 通过。窗口帧为合成 fixture、文件由测试写入，仍不
  代表真实后台输入、游戏 reload 或完整 cleaned 验收。
- RequestOperationDispatch 从已归档 loaded capture 重读原文，核验完整性、来源、
  操作/快照/角色/会话/reload nonce、序号及代码身份；要求 inputReady 为 true，
  再检查安装版本、先保存 dispatch_requested，最后返回一次性 `/dev bridge run`。
  不发送键盘事件、不替代发送前窗口身份/就绪性复查；重复调用拒绝重发。
  ObserveOperationReported 使用保留的窗口 reader，要求 reported 序号高于 loaded、
  身份与代码一致，将解码信号归档后仅推进 reported，不声称正文验证或落盘完成。
  双调用并发只返回一次指令；未就绪、错误代码/会话、旧序号、额外 reload nonce、
  缺帧均拒绝。actions 测试与 vet 通过。ready=true 测试信号为合成 fixture；
  当前实际插件仍返回 false，因此未绕过输入资格门槛，真机自动执行尚不可用。
  本轮全量 Go（严格 Lua 5.1）、vet、版本检查通过；Windows amd64 开发包
  npm 隔离安装报告 `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-PwcQ2G/report.json`，
  tgz SHA-256 `f7d03a4d124a2ab348d9cca915cfef1bdb2e9e460e185ee747e4d563928012bb`。
- ReadInstalledReport 按明确客户端与账号读取 WTF/Account/<account>/SavedVariables/
  Lychee Dev.lua，匹配产品/build 与原报告身份，不扫描其他账号、不回退 .bak。
  拒绝路径段注入、重定向、非常规文件、超限与读取期间可检测的替换/尺寸/时间变化；
  此检查不是恶意并发文件系统保证，也不证明 post-ACK flush。
  ArchiveInstalledOperationReport 从冻结 ProbeLoadIntent 的 Installation/Account
  选择源，仅在 persisted 阶段归档并推进 verified，保存源路径和文件 SHA-256。
  隔离客户端实际文件→归档→ACK 队列测试通过，覆盖缺失账号、错误身份、旧根、
  无效正文、backup 拒绝、取消与重定向。actions、严格 Lua 协议及相关 vet 通过。
  真实账号绑定和 flush 协调器尚未接通；不能据此宣称真机端到端完成。
- 新增 VerifyPersistedReportRemoval：先验证原始落盘报告的身份、代码和正文，
  再解析完整后态，要求 schema 1、reports 表和目标键缺席。目标键即使为 false
  或坏记录也拒绝；空文件、旧根、坏结构、可执行尾缀和不匹配原报告均失败。
  不比较其他请求，允许其他 agent 的报告并发变化。接口不证明来源/新鲜度，
  不删除队列、不推进 cleaned；真实源绑定与 ACK 后 flush 仍待实现。
  Go 单元及实际 Lua ACK 后表序列化→Go 移除验证通过，外来报告保持不变。
  本轮严格 Lua 协议、bridge 测试和相关 vet 通过，未重新声称真机落盘。
  独立 Luna 模拟读取已隔离安装包 UErRIk 的能力：ACK 显示失败后不重发、不
  重跑，区分 Request-A 与 OP-123，先读取操作映射和已有证据，不把缺失恢复
  命令当成已实现。该模拟无真实操作记录，不能作为完整运行恢复验收。
- ACK 传输新增规范队列字段 acknowledgement、安装锁内精确阶段替换和
  `/dev bridge ack <request-id>` 路由。PrepareOperationAcknowledgementQueue
  仅接受已持久化 ack_requested 的操作，从归档重读并核验原报告，匹配安装
  产品/build 后更新队列；不接收调用方回执，不发送输入，不推进 acknowledged。
  文件准备可幂等重试，旧阶段、不同回执、已移除请求不能覆盖或复活确认条目。
  新增 16 条 ACK 编解码、身份/摘要/整数边界、精确替换/重试/清理与其他请求
  保留测试，以及归档→操作→队列测试。实际 Lua 5.1 执行报告→Go 核验并生成
  ACK 队列→Lua `/dev bridge ack`→Go 校验确认信号通过，篡改拒绝且保留报告，
  重复 ACK 不重跑代码。该路由使用显示替身，不代表真机或 SV 清理落盘验收。
  按 skill-creator 修订 live 指引，区分文件重试、输入重发和持久化清理。
  本轮全量 Go（严格 Lua 5.1）、go vet、版本检查及 skill 验证通过。
  Windows amd64 实际 npm pack/离线隔离安装 smoke 通过，报告：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-UErRIk/report.json`；
  tgz SHA-256 `3537c67122aca191a9caa80669251ed65eebf805ff592a1ebbc93917f4763980`。
  仍为开发包，未发布 npm，不代表五平台正式发行验收。
- ReportStore.Acknowledge 收敛为接收原始 reported 字段表：编码为规范原文后与
  已存 receipt 精确比较，核对正文长度/Adler32 与当前会话/角色/版本，拒绝
  伪造字段、错误身份、secret/坏值和重复 ACK。成功前先生成不超过 2048 字节的
  acknowledged 信号，最后才删除该内存报告；编码失败或序号耗尽保留报告。
  Session 私有序号支持已验证原报告的下界，重建会话后 ACK 仍大于原报告序号，
  失败编码允许序号空洞、不回退。CaptureWriter 对安全范围整数使用精确十进制，
  避免大序号 tostring 的舍入/指数表示。Lua→Go 实际 ACK 校验保留原代码/正文
  身份，覆盖会话重建、编码失败、其他报告保留、调用方表不变和 2^53-1 边界。
  全量 Go、严格 Lua 5.1、vet、版本检查通过；补充序号耗尽不删除后协议套件重跑通过。
  当时仅有内部 ACK 信号生成与内存清理；传输接线现见上项，SV 清理仍未落盘，
  不代表真实输入/ACK/cleaned 闭环完成。
  含本轮代码的开发 npm 隔离安装通过：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-9cBWYT/report.json`，
  tgz SHA-256 `05852c15ee725ec5d360fe4881f76deceedd8eb6469f48ef8ee3c6af7e5d4cc1`。
- `/dev bridge load <request-id>` 与 `/dev bridge run <request-id>` 接通
  ProbeQueue/ProbeRunner/ReceiptView：命令词不区分大小写，请求 ID 保留大小写；
  load 只编译选中定义并显示 loaded 回执，run 先消费定义再执行、存报告并
  显示 reported 回执。加载/执行前隐藏旧 QR；回执打印不再次 JSON 编码。
  QR 显示失败可能发生在执行和存储之后，已执行定义不可重跑。
  注册回调 fixture 覆盖混合大小写 ID、完整回执、重复执行/停用拒绝、secret/
  过长参数、显示失败后报告仍在且不重跑。该路由测试使用显示替身，实际绘制
  的 Lua→矩形→Go 解码另有独立覆盖，不冒充完整游戏实测。
  skill-creator 指引下修订 live reference，明确手动开发入口、显示失败恢复
  边界、禁止绕过不可用 CLI 手改队列/发键盘输入。skill 校验、全量 Go/严格 Lua、
  vet 和实际 npm 隔离安装通过：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-Pjiyvn/report.json`，
  tgz SHA-256 `d253c07dd8e7762e96af147f366ef268bf22ac9ece158cba60ab35125b90b9cd`。
  inputReady 仍为 false；真机视觉验收、自动加载/输入、落盘/ACK/清理闭环待完成。
  独立 Luna 使用当前 skill 与已安装开发 CLI，只读模拟 run 后 QR 显示失败场景，
  实际检查 version/help/describe，未重跑或声称完成。但答复混同 request ID 和
  operationId；主 agent 据此补充必须从记录核对映射，不能凭 OP- 前缀等同。
  该模拟无真实 operation/report，不是 live 恢复或真机验收。
  映射指引修订后，skill 校验和包含新 skill 的实际 npm 隔离安装再次通过：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-HX6qjM/report.json`，
  tgz SHA-256 `b18075d07bcf4d4384136ee8b7badf0f38f5814a80917f87a58c0d000cb845bb`。
- MatrixSymbol 与 ReceiptView 接入四 TOC，前者适配已有 BSD-3-Clause
  luaqrcode 副本并保留完整许可，改用私有新接口、惰性初始化和 16x16 nibble XOR
  表；固定 M 纠错，输入上限 2048 字节。新回执视图按黑色水平连续段绘制，
  4 模块静区、4 物理像素模块、最多 8192 纹理；只在有效会话首次显示时创建
  自有 frame，不轮询或挂钩 Blizzard 对象。停用/解绑隐藏视图并注销事件，
  PLAYER_LEAVING_WORLD 隐藏旧回执；大码变小码时隐藏多余纹理。
  相关 SetColorTexture/GetEffectiveScale 与事件名称在四个固定源码基线核对。
  真实 Lua 5.1 生成绘制矩形，Go 按矩形栅格化并用宿主解码器还原原始字节，
  中文/数字/字母数字/2048 字节上限与大码转小码通过；禁用零 frame、重复显示
  不增纹理、secret/超限拒绝、隐藏后事件清理通过。这里是模拟绘制，不是截图。
  全量 Go/严格 Lua、vet、实际开发 npm 隔离安装通过：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-O7EumL/report.json`，
  tgz SHA-256 `5dfdad091b64dbad102360ed5baf3904176643f83a461a41640f435f36f007fa`。
  ReceiptView 的显示不改变 inputReady=false；手动指令接线见上述记录。尚未部署游戏，
  前后截图、四端视觉/性能/战斗检查和真实 QR 回执验收仍待完成。
- PrepareOperationQueue 将队列准备接入 WorkRecord：从冻结 ReportIntent.Load
  读取安装/GUID/reload nonce，核对当前安装产品和 Build；先持久化
  load_requested，再调用受安装锁保护的队列合并。失败/取消保留已记录 intent，
  恢复只幂等准备文件，不发送 reload 或 dispatch，不把文件写入当成 loaded。
  ObserveOperationLoaded 通过调用方保留的 SignalReader 验证新帧、身份、
  reload nonce、序号和实际代码校验，先归档规范化解码信号，再 CAS 推进 loaded。
  队列准备/加载观察与既有报告归档/ACK 共用每操作 OS lease；它不替代
  machine-wide 游戏输入锁。等待帧时不持有 metadata 写事务。
  actions race 回归验证持安装锁时 intent 已落盘但文件未改、取消后恢复、
  并发幂等准备、安装 Build 改变拒绝，以及无帧/错误 nonce/代码/旧序号不推进；
  正确合成 QR 回执的 capture 可复验，loaded 后不能再次准备队列；全量 Go、
  严格 Lua 5.1、vet 和版本一致性检查通过。
  这是模拟安装与合成 QR 验证；未发送游戏输入或证明真实加载/执行闭环。
- 队列文件操作归入 delivery，bridge 只保留协议编解码；ChangeProbeQueue 在
  安装锁内自行验证安装收据、release 和所有不可变文件，不再依赖调用方记得
  校验。只允许 Definitions.lua 按规范队列变化，收据中的原始队列必须是
  正确的空队列基线；既有请求也必须属于当前安装 release。缺收据、坏收据、
  改码、额外文件、错误 release/队列基线或其他版本请求均拒绝且保留原字节。
  普通安装/升级/卸载检查仍严格比较发布基线，不因运行时例外放宽；活动队列
  阻止卸载，精确移除最后一个请求后恢复 managed。安装收据是归属记录，
  不是用来防御本机恶意篡改者的签名。
  delivery race、8 进程队列测试、全量 Go/严格 Lua 5.1、vet、版本检查通过；
  本轮实际开发 tgz 隔离安装/升级/恢复/移除通过，报告为
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-one1fz/report.json`，
  tgz SHA-256 `b66e79df3799c7cb72faeaf93356ad573c44d6714d839302967affa3c3ba29a9`。
  未增加公开 live 命令、游戏输入或真机验收声明。
- ChangeProbeQueue 增加宿主跨进程队列合并/精确移除，复用安装/升级/移除的
  parent-scoped OS lease，不按工作空间分裂锁域。同 ID 同定义可幂等重试，
  同 ID 不同定义拒绝覆盖或移除；不创建缺失队列、不接受非规范/用户修改文件。
  同目录独占临时文件写入并 Sync，重新核对源字节后单次替换，不就地截断。
  保留其他请求；失败清理自有临时文件，不删除锁文件。这里保证协作进程串行，
  不声称任意外部编辑器隔离、断电持久性或源文件加载确认。
  8 个实际独立进程同时加入请求无丢失；精确移除/幂等重试、安装锁冲突取消、
  容量失败不改原字节、缺失/用户文件拒绝、Windows addon/Bridge/锁目录 junction
  拒绝且外部目录不变通过。bridge race、全量 Go 测试（严格 Lua 5.1）、vet
  和版本一致性检查通过。
  这是内部文件操作模块，调用方仍必须先记录操作 intent；
  尚未接到公开 live 命令、自动重载或执行，不改动真实游戏目录。
- 宿主 ProbeQueue 编解码与插件 ProbeQueue.Load 已贯通：规范化 Lua 字面量只
  携带定义，代码由宿主计算 SHA-256/Adler-32/字节数；宿主回读不执行 Lua，
  拒绝额外字段、摘要篡改、非规范格式和可执行尾部。插件选择请求时核对
  release/产品/Build/角色/服务器/GUID/session nonce，并验证实际代码字节校验；
  loaded 回执携带 reload nonce。插件不把携带的 SHA-256 当作自己计算的证据。
  默认 Definitions.lua 为空，四 TOC 接入；发布载荷检查和 npm smoke 打包前
  拒绝非空/修改队列。Go→真实 Lua 5.1→Go 回执验证包含中文/转义代码、
  另一个 Agent 请求不执行、身份错配、secret/坏校验、加载后定义改动不影响
  已冻结代码。全量 Go 测试、vet、严格四端 Lua 回归通过；队列 fuzz 5 秒预算、
  2 workers、170,313 次执行通过。
  实际开发 tgz 隔离安装/升级/恢复/移除通过，报告为
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-bDyBh5/report.json`，
  tgz SHA-256 `3c0c8b4e91b28b2badc76a0db74178cdc2a7f252e41cf6b2cb5460af4ac7f16b`。
  尚未接入队列跨进程写锁/原子合并、宿主 executor 或游戏部署；这是格式与
  请求隔离验证，不是多 Agent 并发写入、后台输入或真机端到端通过。
- Bridge/ProbeRunner 接入四 TOC，提供加载与显式执行：文本编译不执行，拒绝
  二进制 chunk；每会话最多 16 项/1 MiB、单项 256 KiB。加载回执保留实际代码
  字节校验，执行先消费定义再调用 pcall，报告区分 probeStatus completed/failed。
  重复/递归执行及结果无法保存后的重跑被拒绝。Session 增加私有分配的代际，
  相同 nonce 解绑重绑也使旧定义失效；执行期间换会话不写到新会话报告中。
  四端实际 Lua 5.1 fixture 与 Go 加载/报告回执互通通过，包含语法/容量限制、
  运行错误、不可序列化结果、已有/损坏报告拒绝、重绑与执行中身份变化。
  此模块只采集第一个 Lua 返回值，pcall 不提供沙箱或同步超时抢占。
  尚未接入宿主队列文件、QR、后台输入或公开 live 执行命令，inputReady 仍为 false；
  没有部署或游戏实测。loadstring 用法只读核对四个固定源码基线的
  Blizzard_ChatFrameBase/Shared/SlashCommands.lua，未调用旧工具入口。
  本轮全量 `go test -count=1 ./...`、`go vet ./...`、版本一致性检查通过；补充
  secret 错误/返回值和加载后报告损坏测试后，严格 Lua 5.1 协议套件再次通过。
  包含 `/dev` 与 ProbeRunner 的实际 tgz 隔离安装/升级/恢复/移除通过：
  `C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-0TVsJ0/report.json`，
  tgz SHA-256 `844b379cbedd5c7965ce30e41457ee78754a57d7a8d2615e14111fa0884f4a6f`。
  这是 windows-amd64 开发包和模拟客户端验收，不是五平台正式发布或游戏验收。
- 游戏内公共入口按用户要求统一为 `/dev`，不注册额外别名；帮助提示、
  四端注册回归断言和 skill 指引同步。仅保留入口拼写，不恢复旧实现或旧数据。
- ReportStore.Commit 收窄为 requestId/code/value，不再接收调用方自填身份和序号。
  角色/服务器/nonce 来自活动 Session，Session.NextIdentity 私有分配递增序号并
  返回副本；相同 nonce 重复绑定不重置计数，失败操作允许序号有空洞，不回退序号。
  会话同时核对当前数据库根身份，re-root 后旧绑定失效，不通过旧根读取配置。
  四端 Lua fixture 覆盖返回副本篡改隔离、连续报告序号、重复请求不覆盖/不分配、
  拒绝旧的伪造身份参数以及 re-root 失效。SV→Go→归档的实际 Lua 互通通过。
  此次不包含 ACK 信号生成、输入资格或真机执行，不扩大现有完成声明。
- Bridge/Session 接入四 TOC：显式 opt-in 后用宿主提供的 32 位小写 hex nonce
  绑定当前 UnitFullName/UnitGUID，返回副本；竞争 nonce 拒绝，读取时身份变化/
  secret/不可用则失效，停用/解绑/重载不继承会话。/dev bridge bind 和 unbind
  接入手动入口，仍恒为 inputReady=false，不构成执行许可。ReportStore.Commit
  现在要求角色/服务器/nonce 与活动绑定匹配。四端真实 Lua 5.1 fixture 覆盖
  竞争绑定、返回值篡改隔离、GUID 变化、secret/缺失身份、禁用、无绑定/错角色报告，
  SV→Go→归档互通测试同步。身份 API 在四个固定源码基线只读核对；未部署游戏，
  尚无机器输入资格信号、自动会话绑定命令或完整 executor。
- 新 addon 的 Core/Controls 接入四 TOC，在自身加载完成后注册 /dev，提供
  status / bridge on / bridge off。只操作新数据库显式 opt-in；停用删去默认值
  而非报告，不创建可选 frame/event/timer。配置启用仍返回 inputReady=false 与
  transport_unavailable，不把核心初始化或设置状态冒充机器就绪。四端 Lua 5.1
  fixture 覆盖真实注册回调、已有启用配置、冲突保留、secret/过长输入、启停与
  re-root；没有游戏部署或实测。skill 的 live 指引已明确这些实现限制。
  注册合同只读核对四个 AGENTS 基线的 Blizzard_ChatFrameBase/Shared/
  SlashCommandsRegistry.lua、SlashCommands.lua 和 ChatFrameEditBox.lua，未调用旧 CLI。
- CLI version 接入 internal/buildinfo 的二进制内嵌 VCS 身份：commit 与三态
  workspaceDirty（true/false/null），无构建记录、非 Git、非法摘要和重复字段
  不伪造干净提交。开发 npm smoke 强制 -buildvcs=true，核对安装后的版本/提交/
  dirty 与清单和组装时工作树观测一致；go run 的实际输出为未知身份而非干净。
  buildinfo/command 测试及 npm 隔离包通过，证据目录
  C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-CkcW9K，tgz SHA256
  29d7e7ce928e0c47e8d9c7974614ffd473562a203a51da5b36cb4c9ef410233a。
  dirty 状态与摘要并不证明可复现性；正式 clean Commit/tag/五平台发布门槛仍待完成。
- release/version.json 成为版本源（仍为 2.0.0-dev），tools/version.mjs 提供
  --check 与显式 --write，覆盖 Go buildinfo 常量、npm package/lock、四 TOC 和
  addon Runtime。CLI 直接引用 buildinfo.Version；新 npm lock 无运行依赖。
  生成前验证全部目标，重复 TOC 头/非法版本拒绝；只读检查不修复差异，写入可重复。
  四项 Node 回归、全量 Go 测试/vet 通过；CI 与 npm smoke 接入版本检查。
  更新后二进制的实际 npm 隔离安装也通过，证据目录
  C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-THzW6Z，tgz SHA256
  7c8c960d728a3c0f9ca8510e6dcc2fa3335583fe9230be3a0ef9843aa58196ef。正式内嵌
  Commit、五平台产物与发行授权绑定仍待完成，不因版本生成器存在即可发布。
- protocol 五个真实 Lua 测试统一解释器选择，支持 LYCHEEDEV_LUA51 显式路径与
  LYCHEEDEV_REQUIRE_LUA51=1；缺少必需解释器直接失败，非必需本地测试才允许跳过。
  tests/tools/build-lua.mjs 从 Lua 官网下载固定 5.1.5 源包，核对 SHA256 后在独立
  新目录用 C 编译器构建，记录源码、编译器与解释器身份；Windows contract 接入
  此步骤并归档报告。主机 Windows 实际构建及全部五项协议测试通过，显式错误路径
  负向门槛也实跑失败；远端 workflow 仍未执行，不据此宣称远端 Windows CI 已绿。
- 独立 Luna 使用已安装 npm 包的 skill 与 CLI，在独立临时模拟客户端/skill 根
  完成首次与相同载荷重复安装。两份报告经主代理复核：损坏所有权收据时返回
  invalid_installation（exit 4），普通载荷编辑后 status=modified、再次安装
  installation_conflict（exit 3）；原有模拟用户 AddOn/marker 保留。报告位于
  C:/Users/follen/AppData/Local/Temp/lycheedev-forward-test-71ad624dc69e4fc3823f2c6842345149/report.json
  和 C:/Users/follen/AppData/Local/Temp/lycheedev-forward-test-2ff0d3eec4e3470f8444e9494422a924/report.json。
  此次是安装场景 skill 前向测试，不覆盖 live/data/source 全模式，也未验证游戏加载。
- npm 开发 smoke 扩展为宿主原生二进制 + 当前 addon/skill 全部源资源，生成大小/
  SHA256 inventory 并核验 tgz 与安装后字节。只用 npm 安装后的 CLI/载荷执行
  skill 与模拟 retail 客户端的安装、归档升级、恢复、移除；验证用户编辑冲突
  保留和 WTF 哨兵不变。报告记录 HEAD 与 workspaceDirty，不把脏工作树冒充可复现
  提交。最新运行通过，目录 C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-RT5C0r，
  tgz SHA256 4fb646051da980b8d232e93538019bb6d71815624bcaa1583dee3e9140a8b7a1。
  launcher 五项测试、skill quick_validate 通过。CI 步骤已引用扩展测试，但尚未在
  远端执行；这不是完整功能、五平台、真实游戏或正式许可/发布门槛的验收。
- addon install 增加 --output 归档升级、--resume 无发行源恢复，addon remove 增加
  显式归档移除；三者复用已验证的客户端目标和安装事务，升级同首次安装一样
  验证独立快照及四 TOC。CLI 临时目录完整生命周期通过；Luna 补充旧版本归档、
  源清单移除后恢复、无效 TOC 拒绝、用户编辑拒绝和客户端 WTF 数据保留测试，
  主代理复核并修正测试中 WTF 的客户端相对位置。Windows 真 junction 拒绝测试
  覆盖四种写入入口，外部目标无写入；这些仍是临时目录测试，不是真机部署。
  skill-creator 同步升级/恢复/移除指引并通过 quick_validate。最新 host-only npm
  隔离 smoke 通过：C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-HN0ZDD，
  tgz SHA256 3f0a41bd0938cfbcc546572a3d4269e962699d1c91aa1eb0e3aa67570f1ec0e5。
  仍非完整五平台 addon/skill 发行载荷或正式发布验收，未发布 npm。
- addon install --release <distribution-root> --installation <client-directory>
  已接入 CLI。actions 解析支持客户端、拒绝被重定向的 Interface/AddOns 父目录，
  复制独立载荷快照后验证完整清单、四 TOC 和递归 Lua/XML，再复核客户端身份，
  复用受管理的排他首次安装。只接受缺失目标或完全相同的既有受管理版本，不覆盖
  旧 addon/任务注册表；不重载游戏、不启用桥接、不导入旧数据。真实命令路由的
  临时客户端首次/重复安装与 status 回归通过；Luna 补充旧目录/用户编辑保留、
  错误 TOC、未支持产品、缺少父目录及 active catalog 回退测试，主代理已复核。
  全量 Go 测试、actions/command race、vet 通过；skill-creator 修订并校验安装指引。
  addon 升级、归档移除及中断恢复尚未接入命令，当前不等于完整部署验收。
- actions 增加 ObserveReportAcknowledgement：沿用选定窗口的 SignalReader，
  重新验证原报告归档，以更高 sequence 和固定会话/角色/请求/版本接收 ACK，
  再核对原始代码与正文长度/摘要；归档规范化的已解码 ACK 字段后才推进 acknowledged。
  capture 明确标为 decoded-game-acknowledgement，不冒充原始 QR 文本或截图；
  acknowledged 仍为 running，不据此认定 SV 清理落盘或完成整个操作。生成二维码
  帧回归覆盖无帧、旧序号、外部会话、错误摘要、有效 ACK 和重复确认，actions
  非缓存测试与 vet 通过。实际 WGC 游戏画面、插件 ACK 信号输出及清理落盘待验收。
- actions 增加 ArchiveOperationReport / RequestReportAcknowledgement，将固定
  WorkIntent 中的身份/代码用于 SV 校验，双 capture 归档成功后才推进 verified；
  ACK 前重新读取归档，再以 generation CAS 写入 ack_requested，最后返回确认数据。
  错误 SV 不推进阶段，双并发恢复只有一个成功取得确认数据，已处于 ack_requested
  时拒绝盲目重放。actions 非缓存测试与 actions/evidence race、actions vet 通过。
  Luna 新增真实 Lua 5.1 fixture→SV 解析→双 capture→数据库重开→确认准备集成测试，
  主代理已审查。此处不发送输入，不处理未决 ACK 的游戏证据协调，也不宣称操作完成；
  executor、机器信号和真实游戏端到端仍待实现。
- evidence 增加 CommitReport 与 PrepareReportAcknowledgement：先验证游戏报告，
  分别归档正文和回执原文；确认准备阶段重新读取两个 capture，核验完整性、
  operation/snapshot/session/build/request 来源及独立代码/角色身份。VerifiedReport
  现在保留独立复制的回执原始字节，避免 JSON 重编码破坏游戏侧精确回执比较。
  临时 vault 关闭重开后准备成功；缺失/交换 capture、错误操作/快照/会话/代码、
  已取消 context 拒绝且不返回 ACK 数据。evidence/bridge/protocol 非缓存测试与
  evidence/bridge vet 通过。归档中途失败可返回已保存 capture，不自动发送 ACK；
  此接口尚未与 executor 的持久 intent、输入、回执及清理落盘串联。
- ReportStore 增加确认清理接口：显式启用后，精确匹配请求、回执原文、正文长度与
  Adler32 才删除该内存记录；不匹配、secret、坏参数、缺失记录和禁用状态均不删除。
  Lua 5.1 回归验证失败保留、成功只删除目标、重复确认不伪报成功；运行
  `go test -count=1 ./tests/protocol ./internal/bridge` 通过（强制执行外部 Lua fixture，
  不把 Go 缓存结果作为 Lua 修改的验证）。这只是报告存储接口，不证明宿主已经
  持久归档、ACK 输入传递、游戏确认信号或 SV 清理落盘；完整 executor 仍待接入。
- 新 addon ReportStore 接入四 TOC，桥接未显式启用时拒绝提交；新记录仅最终一次
  写入 reports[requestId]，禁止覆盖重复请求或自动淘汰未确认报告。正文最大 512 KiB、
  100 条、正文与回执合计 16 MiB，回执保留独立会话/角色/版本/请求/代码及正文校验。
  Go ReadPersistedReport 从新 schema 的 SV 原文选择精确请求，再用宿主独立身份
  与代码校验，不执行 Lua、不发送 ACK。真实 Lua 5.1 写入记录→序列化 SV literal→
  Go 受限读取/验证互通通过；重复、数量/容量预算、secret/循环/过大值和坏状态拒绝
  回归已加入。此处未验证 WoW 落盘、输入执行、QR 或 ACK，仍非游戏端到端闭环。
- 新 addon 落地四 TOC（开发版本 2.0.0-dev）、单选客户端 profile、集中平台身份
  检查、Persistence 与一次性 Runtime 加载器。只在自身 ADDON_LOADED 后写入
  LycheeToolkitDB，未知/较新 schema 不覆写，不读取或清除旧数据库；读取状态
  时重新取当前根，不缓存旧 profile 表。核心仅一个加载事件 frame，完成后撤销
  event/script，无可选桥接 frame、hook、timer 或轮询；默认 bridge 不启用。
  真正 Lua 5.1 四 TOC 加载测试覆盖 fresh、既有数据、较新/损坏 schema、错误
  Interface、secret 身份、其他 addon 事件及 re-root，全部通过。此处只是初始化
  层，未实现工作台、执行器或机器就绪信号；未部署到真实游戏或声明功能齐全。
- codebase 新增无写入的 InspectLocalLoad，复用原有有界 TOC/XML 递归加载器和
  Lua/XML 语法分析；LoadedDocument 明确 Archived，纯本地摘要不再暗示已发布
  vault blob，原归档入口必须具有 store。actions 的四 TOC 发行检查接入该加载
  图，并把每个实际分析文件的摘要/大小与发行清单对照，返回有序 LoadedFiles。
  多层 XML 正常加载、缺失文件、循环、畸形 XML、错误 Lua 及本地/归档证据区分
  专项回归通过。不声称静态加载有效即运行安全、无 taint 或游戏已就绪；真实
  addon 生命周期及部署仍未验收。
- actions 增加 InspectAddonRelease 发行前置检查：先验证发行清单与字节，再复用
  codebase.AnalyzeDocument 读取四 TOC，核对已批准 Interface、统一 release 版本、
  新 LycheeToolkitDB（拒绝旧持久变量和非空角色级变量），并检查直接 Lua/XML
  引用在载荷清单中存在、顺序保留、无重复/越界路径。正常四端及错误 Interface、
  版本、存储名、重复头、缺失/越界/重复/空加载列表回归与 actions vet 通过。
  这是合成发行资源的 TOC 合同测试，不是四客户端运行矩阵；递归 XML、Lua 运行
  与就绪协议未由该接口验证，实际 addon 安装命令仍未开放。
- selection 增加显式客户端目录身份解析，集中维护四端 product code、已批准版本
  系列、TOC 与实测 Interface；数据 product 映射复用同一表。优先 .flavor.info，
  再版本证据，最后已知目录；缺 version.txt 时由 actions 提供 active build catalog
  候选。未知产品、矛盾/不支持版本、歧义 active build 和无身份的复用测试目录拒绝。
  addon status --installation <client-directory> 接入该路径；仅只读，不部署。
  真实 D:/Game/World of Warcraft/_retail_ 返回 wow/12.1.0.69875/120100，旧 addon
  状态 unmanaged，未改动。合成 Forever/MoP 复用槽位与身份优先级回归通过。
  TOC 内容核验及真正 addon 安装仍待实现，此状态查询不证明实际游戏窗口或角色。
- skill install 增加显式 --output <archive> 升级模式，以及 --resume --path <target>
  --output <archive> 恢复模式（恢复禁止 --release）。actions 固定 skill 组件，
  ResumeUpgrade 额外核对调用方期望组件，不能跟随日志把 addon 当 skill 部署。
  命令回归覆盖首次安装→升级→完成后恢复重试→修改冲突→归档移除；skill-creator
  指引同步区分三种安装模式及无 journal 的准备失败。真实独立子进程在旧版本
  移走后被 Process.Kill，父进程随后重新取得锁并恢复安装，Windows 实跑通过。
  此结果仅覆盖该强杀检查点，不是断电持久性、所有中断点或真实用户安装验收。
  全量 Go 测试/vet、delivery/command race 及 skill quick_validate 通过。
  最新 host-only npm 隔离 smoke 通过，目录
  C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-zTly7T，tgz SHA256
  6b6e1f456915646a7162ef0a268e9b93870bd079f9b5fd12e535b2f71ecc125b；仍不含完整载荷。
- delivery 增加 UpgradeInstallation/ResumeUpgrade 内核：显式目录保留 previous，
  next 独立安装校验，替换前同步写入 replacement.json；持目标安装锁，根据三棵
  实际目录及前后收据判定 prepared/old-moved/completed，恢复不依赖原始发行源。
  目标被占用、候选内容改变或状态不一致均拒绝继续，不删除旧版本归档。
  升级、完成后重试、文件系统检查点、候选篡改/意外目标、移走旧版本后取消并
  恢复等临时目录测试和 delivery vet 通过。此内核尚未接 CLI；准备失败且未写
  journal 的目录需明确处理，不自动接管。当前不是进程强杀或断电持久性验收，
  不声称目录元数据已跨断电持久化，也未覆盖最终的部署全套恢复流程。
- skill remove --path <parent/lycheedev> --output <recovery-directory> 已接入 CLI：
  复用安装 OS 锁并核验所有权与全部文件后，排他移动至明确归档位置，不删除内容。
  归档必须在同一文件系统且位于安装发现目录之外，防止宿主继续加载副本；不会
  覆盖已有归档，也不接管旧目录或忽略用户编辑。归档完整性、重复移除、已有目标、
  用户编辑、嵌套/同发现目录及无所有权拒绝回归通过。skill 指引同步明确归档语义。
  仅临时目录测试，未移除用户实际安装；升级、恢复命令和进程中断归档发现仍待完成。
- 首次 skill 安装接入 actions 与 CLI：skill install --release <distribution>
  --path <parent/lycheedev>；skill status/addon status 只读检查 --path，不使用 home。
  统一结果封装及 delivery 错误码落地，describe 仅声明实际范围。真实命令路由的
  临时载荷安装、重复执行、状态读取、用户编辑冲突及参数拒绝回归通过。
  新增四个独立测试子进程的 barrier 并发安装，同一目标均成功重试且最终内容
  受管理，Windows 本机实跑通过。仍未测试安装进程死亡、升级与断电恢复。
  按 skill-creator 修订安装状态解读和能力限制，安装不再错误要求 snapshot；
  该路径尚未用完整正式 addon/skill 包进行独立 agent 前向验收或 npm 发布验收。
- delivery 新增 InstallFresh：完整清单验证→独立暂存→生成所有权记录→暂存复核→
  目标父目录作用域 OS 锁→目标复核→原生 no-replace 目录发布。相同版本/提交/
  资源的干净安装可重复执行；旧目录、用户编辑及不同发行版本均返回冲突。
  Windows MoveFileEx 未启用 REPLACE_EXISTING，Linux/macOS 使用排他 rename；
  不回退到可覆盖目录的普通 rename。临时目录实际安装、重复执行、四并发调用、
  保留用户文件和拒绝覆盖已有空目录测试通过；全包 Go 测试、delivery race/vet
  通过。Luna 独立复核发现发行资源可夹带所有权记录，已前移到资源路径检查拒绝。
  当前只实现首次安装/相同载荷重试，不包含升级事务、断电恢复、CLI 路由或真实
  插件部署；并发回归为同进程多 goroutine，尚不是跨进程安装验收。
- delivery 增加只读 InspectInstallation 与版本化所有权记录，区分 absent、
  unmanaged、managed、modified。已存在空目录也不自动接管；缺失/额外/被编辑
  文件均阻止视为干净安装。资源检查复用同一校验内核，只允许忽略已核验的
  安装记录文件。畸形记录和组件身份不匹配返回错误，不提供覆盖授权。
  同时拒绝 JSON 大小写别名重复键，避免 Go typed decoder 覆写字段。
  目录分类、用户编辑、未列出/缺失文件、错误记录回归及 delivery vet 通过。
  检查是即时状态，后续 Apply 必须持锁重检；当前仍没有实际部署和恢复入口。
- delivery 增加 InspectRelease，读取统一 release.json 的 schema/version/commit、
  binaries 和 resources；限制 1 MiB/16 层 JSON，拒绝重复（含转义同名）与未知
  字段，核对调用方期望版本、平台和二进制记录，要求 skill 入口及四客户端 TOC。
  载荷文件随后走完整 VerifyPayload，失败不返回部分 Release。回归覆盖正常读取、
  取消、版本差异、清单畸形/超限、资源缺失及摘要错误；delivery 测试/vet 通过。
  此处只校验二进制记录，不校验其内容，也不验证发行签名或 TOC 语义；尚未接到
  CLI 部署。当前 host-only 开发 npm 包不满足这一完整载荷合同。
- delivery 新增 StagePayload：先验证源清单，再在明确的暂存父目录创建独立目录，
  通过受根目录约束的文件句柄限长复制、同步文件，最后验证整份暂存内容。
  成功返回暂存目录供后续部署使用；失败只清理本次创建的目录，不删除父目录、
  源目录或旁边文件。源内容与暂存副本独立、初始失败无产物、创建后取消清理
  回归均通过，delivery vet 通过。暂存不等于不可变存储或部署事务，仍未接入
  CLI、发行认证、安装所有权及替换/恢复流程；没有修改真实游戏或已安装 skill。
- 新 delivery 模块增加 addon/skill 资源整包校验：清单限定相对路径、SHA-256、
  字节数，拒绝缺失、多余、篡改文件、大小写路径冲突、文件/目录冲突、Windows
  保留设备名及链接；累计 256 MiB、32768 文件、65536 遍历节点预算，支持取消。
  校验通过不代表 manifest 已获认证，也不承诺抵抗验证后源目录被修改；安装器
  后续须消费不可变的已验证内容。当前尚未接入 CLI 安装、所有权和原子部署。
  本机 Go 全包测试通过、delivery vet 通过；Windows 目录联接拒绝测试实跑通过。
  文件符号链接用例因本机缺少创建权限跳过，不能据此声称该用例已验收。
- npm 启动入口修复链接路径识别：将 argv 入口解析为真实路径后判断直接执行，
  避免链接安装路径下静默跳过 launcher。新增目录链接入口以及包外 native
  目录链接（即使摘要匹配也拒绝）的回归，Windows 本机五组 launcher 测试通过。
  修复后重新实际 pack/offline/ignore-scripts 隔离安装、Windows npm shim、
  version/init/describe 通过，证据目录
  C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-cLgAvw，tgz SHA256
  274ad7a7a7f40ba37d034959885d2b5fb1e448dc1e536554639ea03c98b4e523。
  Windows CI 已配置 Node 22.14.0/24.18.1、npm 11.18.0 的安装 smoke 矩阵及
  报告归档，并纳入 foundation required；尚未在远端运行，不能算 CI 验收通过。
  当前仍是仅本机二进制的开发包，不代表完整发行包或全部端到端验收。
- 新 npm 源目录 packages/npm/lycheedev 落地零依赖启动包装：限定五个平台映射、
  manifest/包版本匹配、路径限制、二进制大小/SHA-256 校验、无 shell argv 转发、
  stdio/退出码/信号传递，无下载和 postinstall。开发 manifest 为 2.0.0-dev、
  private=true、UNLICENSED 防误发布；最终统一许可仍待核定，不代表正式发行许可。
  平台映射、损坏摘要/版本/路径拒绝与参数转发三组 Node 测试通过。
  host-only 开发 smoke 实际 Go CGO_ENABLED=0 构建→npm pack→隔离 global install，
  --offline --ignore-scripts 后 version/init/describe 通过；另实际运行 Windows
  npm .cmd shim 得 windows/amd64、go1.27.1、2.0.0-dev。
  测试根 C:/Users/follen/AppData/Local/Temp/lycheedev npm 隔离-35IoSS，tgz SHA256
  b443cc1682b421d1e706c524db80a22b6e74fa2ba875ae20487c5f4d4698949a，压缩
  10,474,287 字节、展开 18,296,540 字节。此包只携带本机二进制，不含最终
  五平台/addon/skill 载荷，非 PKG 全验收、非正式发布产物，未替换旧 npm 包。
- 聚合输入与 GROUP BY 中的表达式子查询已开放，预绑定区分聚合前的原始行
  与聚合后允许的分组键。重复 SELECT/GROUP BY 子查询按忽略源码位置的结构
  等价比较，输出复用分组键而非在最后一条原始行上重跑；分组后相关性检查
  跳过已绑定分组键，但继续拒绝其他未分组字段。相关 SUM(COUNT 子查询)、
  子查询键分组/投影/排序组合回归、全量 Go 测试/vet、relational race 通过。
  skill 数据参考更新此支持范围并通过 quick_validate；复杂递归、规模优化及
  其余 Toolkit 功能和发布验收仍未完成。
- Luna 独立按新版 skill 使用真实编译 CLI，在全新隔离 home 中完成 target pin、
  SQL 单条/全表统计、在线/离线重读和四份 evidence verify；主代理重新核验
  四份 capture 并读取原始 blob。ID 1 为 WARRIOR，COUNT/MIN/MAX 为 15/1/15。
  测试根：C:/Users/follen/AppData/Local/Temp/lycheedev-independent-86f87fa6ac1b4b91b7c39966b625e3c7。
  单条 capture 在线 CAP-512e27beba0291f13877d79ce208926f5fed00a77fafa31d6cf2c9d36558939f、
  离线 CAP-081f60c270b4ad829f4c02c7054c6ae892fa60654e15e5ec74dcaa13cd48eaff，
  同 blob b2086ca235c90d10b566b5d29da8b8ea03ef1c221346ed96938a3184e06a098d。
  统计 capture 在线 CAP-0dd15d3492dc057300b727b50fb230e42c98b20ba58378485669664e262b71e0、
  离线 CAP-b9a2cd866741b839b369f92c106dff78a29511c48871cff1b04b1c4866d917f1，
  同 blob ae5d9086159542429c9ae8aece1a5ff882dfbee779ad05d3e4337eab412bb69e。
  初次将已有文件的临时根作为 home 被 legacy 检查拒绝，改为空 home 子目录后
  成功，未继承旧数据。此结果是 SQL skill 前向模拟，不是 npm 安装或游戏 E2E。
- 完整 Compile→Execute 入口新增有界 fuzz，覆盖子查询、JOIN/聚合、递归及
  EXPLAIN seeds，检查失败零部分结果、成功行列形状一致。双 worker、目标
  10 秒完成 622,705 次执行并通过；每次执行限制 1 MiB/10000 工作单位。
- SQL CLI 新增稳定 query 错误分类与语法位置：语法/绑定 exit 2，未支持能力/
  预算 exit 3，类型/数值范围/行列基数 exit 4；stage=query，语法附 offset/
  line/column，来源 I/O 保留原故障分类。预算 sentinel 更名 budget_exceeded，
  不把运行期预算误报为编译限制。包装错误映射及实际命令语法失败测试、全量
  Go 测试/vet、command/relational race 通过；skill 数据参考补充对应处理规则。
- `data sql --snapshot <pin> --installation <root> --file <query.json>` 已接正式
  CLI 路由和 describe，支持 --offline/--max-bytes。请求为最多 1 MiB 的 UTF-8
  JSON：sql 字符串及可选 parameters 标量对象；拒绝重复键、未知字段、嵌套值、
  尾随文档及不相关 flags，数字不经过 float64 中转。分页使用 SQL LIMIT/OFFSET。
  真实 Retail ChrClasses 命令离线查询 ID=1 得 WARRIOR，capture
  CAP-40b43ba3f276bfc3a29c72cc082a4194ad40f7714f099071ae92f5f0f24b4745
  通过 evidence verify；完整安装数据测试、全量 Go 测试/vet、command race 通过。
  按 skill-creator 更新 data-investigation 的真实请求格式、输出和未支持范围，
  quick_validate 通过。独立实际二进制 skill 前向模拟尚待进行，不是 npm 隔离
  安装或整个 Toolkit 的端到端验收。
- 应用层 `QueryLocalData` 接通固定 DataPin→固定定义→PrepareLocalTable→SQL→
  capture。只接 local static catalog，不静默混入 Hotfix/CDN；最多 32 表、
  累计原始表内容 512 MiB、查询工作 1000 万单位、执行记账 64 MiB，JSON 证据
  最多 16 MiB。输入 SQL/参数快照使用 UseNumber 保持整数，证据包含请求、结果、
  每表来源/定义/布局；locator 使用请求 SHA-256，complete 指查询结果而非全表。
  真实 Retail 12.1.0.69875 ChrClasses 参数 ID=1 返回 WARRIOR，capture
  CAP-21d054db4c0fc3d65c43bde700b03f8243ee1ca8567642c7c3324ff8846176be
  在隔离测试工作空间内通过 evidence verify。`TestInstalledAssetCommand` 的
  DBD 网络开关分支、全量 Go 测试/vet、actions race 通过。该测试仍直接调用
  action，不是 SQL CLI 或 npm 隔离安装；表内容/查询预算不是整个进程 RSS 上限。
- `EXPLAIN` 已接 schema-only 预绑定后返回层级执行形态，不调用 Source.Scan；
  来源 resolver 仍可读取元数据/准备表，因此不声称零 I/O。`EXPLAIN ANALYZE`
  真正执行后返回各实体表扫描次数/回调行数、输出行数、工作单位和累计预算
  记账字节；失败不返回成功计划或部分结果。计划标明右表物化的 nested-loop、
  流式聚合、稳定归并排序、CTE/delta 递归及 compound 尾部排序语义；不提供
  虚构估算代价。ChargedBytes 不是 RSS 或峰值内存。零扫描与实际计数回归、
  全量 Go 测试/vet、relational race 通过。SQL CLI/证据链及剩余语法形态、
  优化与真实多表规模验收仍待完成。
- `Program.Execute` 新增 schema-only 预绑定阶段，校验完再开始任何 Source.Scan；
  递归成员即使 seed 为空也绑定一次。表达式子查询在未执行 CASE/空输入中仍
  校验字段/函数/参数和输出列数，分组子查询的外层字段限制也在扫描前检查。
  物理 Source 按 catalog/name 规范化缓存，复制列名并计预算，预绑定和实际
  执行复用同一来源，不因相关子查询反复调用 resolver。仍需数据才能判断的
  标量子查询行数/值类型/算术错误保留运行期校验。
  隐藏错误零扫描、空递归成员、空分组相关引用、来源只解析一次的回归、
  全量 Go 测试/vet 和 relational race 通过。此处不是类型化查询优化器；
  聚合输入/GROUP BY 内子查询、复杂递归形态、EXPLAIN 和 CLI 仍待完成。
- 分组投影/HAVING/ORDER BY 中的表达式子查询已接入。分组完成阶段安装只读
  分组键值的 column resolver，不再向相关子查询暴露最后一条原始输入行；
  读取未分组字段失败，退出恢复原 resolver。已验证分组相关计数、HAVING EXISTS、
  子查询排序、空聚合组的独立子查询和未分组字段失败；全量 Go 测试/vet、
  relational race 通过。该拒绝目前发生在实际字段读取，空子查询/未执行分支
  的完整静态预绑定仍待完成；聚合输入及 GROUP BY 内子查询仍未开放。
- 表达式子查询新增词法外层行 scope，支持相关 EXISTS、IN 和标量聚合子查询、
  多层外部引用。字段先绑定最近层，内层 qualifier 即使缺失所求列也遮蔽外层，
  本层歧义不能回退；绑定节点记录外层深度，不修改编译 AST。JOIN 条件子查询
  仅看到当前连接前缀字段；嵌套结束恢复外层字段/callback。相关计数、两层
  嵌套、同名别名遮蔽及歧义回归、全量 Go 测试/vet 和 relational race 通过。
  分组投影中的子查询、所有空路径的完整预绑定、结果缓存与 EXPLAIN 仍待完成。
- 非相关表达式子查询已接入标量/EXISTS/IN/NOT IN 求值，共享执行预算及当前
  CTE scope。标量子查询零行返回 NULL，多行或非单列返回 cardinality 错误；
  IN 子查询要求单列，空集合为 false，NULL 候选保留三值逻辑。嵌套执行后
  恢复子查询 callback；错误不返回外层部分结果。对应回归、全量 Go 测试/vet
  与 relational race 通过。仍缺相关外层字段、分组子查询及空输入/未执行
  分支中的子查询完整预绑定；当前未做子查询结果缓存，不能视为完整 SQL 验收。
- 递归 CTE 新增 seed + 单一递归 UNION ALL 分支的 delta 迭代执行，递归引用
  只读取上一轮新增行而非累计结果，所有轮次共享预算。保留初始列名；列数
  不匹配、初始查询自引用和多个递归引用失败，递归 scope 在退出时恢复。
  普通 CTE 可位于 WITH RECURSIVE 中，显式 catalog 引用不算自引用。空 seed、
  有限序列及循环触发工作/内存预算的回归通过；全量 Go 测试/vet、relational
  race 通过。目前不支持多个 UNION 递归分支、递归体内排序/分页等形态，
  尚待完整绑定与旧能力对照验收；表达式子查询、EXPLAIN 和 CLI 仍待接入。
- 查询执行入口新增非递归 CTE、FROM 派生表和 UNION ALL，复用同一个 evaluation
  的工作/内存预算。CTE 按词法 scope 和声明顺序求值，预留本层名称以拒绝
  自引用/前向引用误落到物理表；显式 catalog 仍直接选物理来源。合并校验列数，
  结果列名取首分支，末分支 ORDER/LIMIT/OFFSET 提升为合并结果操作；非末分支
  无括号的排序/分页拒绝。CTE 串联、派生表、CTE 在 UNION 两侧复用、合并排序
  分页及错误回归通过；全量 Go 测试/vet、relational race 通过。
  递归 CTE、表达式/相关子查询及 EXPLAIN 尚未实现，CLI SQL 未开放。
- 本地表准备抽出共享的已认证 View，单条/分页与 `PrepareLocalTable` 共用
  manifest/DBD/CASC/布局绑定路径。`View.Scan` 一次建立受预算限制的升序 ID
  索引，逐行解码（包含复制行），避免每页重扫索引；回调错误和取消立即返回。
  `TableSource` 将 View 接入统一查询入口，数组显式展开为需引用的 `Name[0]`
  等列名，原 `data db2` 的数组结构不变。
  真实 Retail 12.1.0.69875、固定定义 83057bdc0cbe13062850ebf8ad530031e128a1cd
  的 ChrClasses 通过 SQL 筛选 ID=1 得 WARRIOR、COUNT/MIN/MAX 得 15/1/15、
  ID 自连接 COUNT 得 15。复现为 `TestInstalledBuildMetadata` 设置安装、产品、
  build 和 `LYCHEEDEV_TEST_TABLE=ChrClasses`；此测试会读取固定远程定义。
  扫描顺序/复制行/预算/取消/回调停止回归、全量 Go 测试/vet、table/relational
  race 通过。真实测试从认证字节绑定 View 后进入 TableSource，尚未验收完整
  PrepareLocalTable→SQL CLI→证据链路，也不是全部表或 npm/游戏 E2E。
- 新增 `Program.Execute` 和固定表 `Source`/`Resolver` interface，统一承担
  行绑定、INNER/LEFT/CROSS JOIN、WHERE、投影/聚合及结果处理。adapter 提供
  schema 和同步可取消扫描，并在 callback 报错时停止；来源版本/资源生命周期
  由 adapter 所属调用负责。右表单次扫描、复制保留值并记入内存预算，连接
  候选消耗执行预算；重复别名/歧义字段/越作用域 ON 拒绝，LEFT 未匹配行补 NULL。
  连接与分组/排序组合、失败不返回部分结果及预算回归通过；全量 Go 测试/vet、
  relational race 通过。按 codebase-design 保持一个查询 module，不向数据源
  adapter 下放 SQL 语义。此轮仅内存 adapter 验证，真实 DB2 adapter、CTE、
  UNION、子查询和 EXPLAIN 尚未实现，CLI 仍未开放 SQL。
- 内部行流执行已贯通绑定→WHERE→普通/聚合投影→DISTINCT/排序/分页。
  绑定复制 AST 并统一字段身份，不修改可复用 Program；字段大小写不敏感、
  歧义/缺失字段与参数明确失败。星号按 schema 顺序展开，ORDER BY 支持输出
  名称/别名，分页参数在扫描前校验。未知函数/转换、错误函数参数个数、嵌套
  聚合及 WHERE 聚合在读取行前拒绝，空表不掩盖这些错误。流读取失败不返回
  部分结果，执行后恢复外部求值作用域。组合路径及原子失败回归、全量 Go
  测试/vet 和 relational race 通过。目前使用内存测试行流，尚未接本地 DB2
  扫描、JOIN/CTE/子查询或公开 SQL 命令，不构成真实数据 SQL 端到端验收。
- 内部结果阶段接入 DISTINCT → 稳定 ORDER BY → OFFSET/LIMIT，支持多排序键、
  显式 NULLS FIRST/LAST 和参数化整数分页；默认 NULL 在升序最前、降序最后，
  与旧查询规则一致。排序使用可返回错误的归并比较，取消/类型错误/预算失败
  不返回部分结果，也不重排输入切片。复合去重键使用长度前缀、数值规范化，
  排序临时空间和去重键计入预算。已验证分组→HAVING→排序→分页组合，
  稳定同值顺序、极大 offset、零 limit 和原子失败。
  Luna 独立补齐分组取消/预算、复合键、数值类型等值、HAVING NULL、错误无
  部分输出及作用域恢复回归；主代理复核测试。全量 Go 测试/vet、relational
  race 通过。仍未接表扫描/连接/子查询及公开 `data sql`，不是完整 SQL 验收。
- 聚合状态接入内部 GROUP BY/HAVING/投影阶段，支持表达式分组、多个聚合、
  NULL 分组和排序表达式求值。无分组键的空输入生成一个空聚合组，有键的
  空输入不生成组；HAVING 仅保留 true。规划阶段拒绝未分组字段、裸星号及
  嵌套聚合，不从任意代表行取值。仅保留分组键、累加状态和最终投影，分组/
  DISTINCT/结果文本共享内存预算，逐行检查工作预算；求值替换作用域在返回
  或报错时恢复。分组路径回归、全量 Go 测试/vet 与 relational race 通过。
  当前输入仍为内部绑定行，未接正式数据源、连接、排序或子查询；不是完整
  查询执行器，也没有开放 `data sql`。
- 新增流式聚合状态 COUNT/MIN/MAX/SUM/AVG，支持 DISTINCT、空组与 NULL，
  COUNT(*) 独立于 COUNT(expr)。精确累加后才物化 SUM/AVG，允许中间整数和
  暂超 uint64 后抵消，最终整数溢出显式失败。DISTINCT 数字键与比较器一致，
  不混淆大整数及其舍入浮点；去重键和文本极值受执行内存预算限制，逐项
  检查取消与工作预算。全量 Go 测试/vet、relational race 通过。
  此处仅为聚合累加器，尚未接入 GROUP BY/HAVING/投影或公开 SQL 命令。
- 表达式函数新增 LOWER/UPPER/LENGTH、惰性 COALESCE 和 NULLIF；CAST 支持
  STRING/TEXT/VARCHAR、INT/INTEGER/BIGINT、FLOAT/DOUBLE/REAL、BOOL/BOOLEAN。
  普通文本函数要求显式文本，不再通过任意类型隐式字符串化；LENGTH 计算
  Unicode 码点。CAST 文本保留完整 uint64，整数目标为有符号 int64，有限
  小数向零截断，越界不回绕；非法转换/参数个数/未知函数不静默变为 NULL。
  与旧 wowdata 函数清单核对后新增成功及失败回归，全量 Go 测试/vet 和
  relational race 通过。COUNT/MIN/MAX/SUM/AVG 尚待分组执行层接入，SQL
  绑定、子查询和表级执行未完成，不能作为公开 SQL 能力发布。
- 编译表达式已接入内部求值器，支持参数/字段回调、算术/比较/布尔运算、
  IS NULL、IN 列表、BETWEEN、searched/simple CASE 与 Unicode LIKE/NOT LIKE。
  CASE 只执行选中分支，缺失参数/字段显式失败；每个节点及 LIKE 动态规划
  单元共享执行步数预算并检查取消。LIKE 大小写敏感，`_` 对应一个 Unicode
  码点、`%` 对应任意序列，无隐式反斜杠转义，使用两行状态避免递归回溯。
  编译到求值回归、取消/预算测试、全量 Go 测试/vet 和 relational race 通过。
  函数、CAST、子查询与表级执行仍待接通；尚无 `data sql` 公共命令。
- SQL 标量运算基础新增精确整数/有限浮点比较、算术、严格布尔类型及 NULL
  三值逻辑。整数不先转 float64，支持完整 uint64；超出 int64/uint64 联合集合
  显式报错。除法返回浮点，除零/模零返回 NULL；余数按向零截断定义。
  字符串连接限制 1 MiB，拒绝任意对象隐式字符串化，未知运算符即使参数为
  NULL 也必须失败。该层尚未接到表达式执行器或 `data sql`，不能视为查询完成。
  全量 Go 测试/vet、relational race 通过；整数运算与独立 big.Int 结果对照的
  `FuzzScalarIntegerArithmetic` 双 worker 10 秒完成 889,710 次执行并通过。
- `records/relational.Compile` 新增有界的只读 SQL 语法前端，独立于旧工具入口
  和数据库连接。覆盖 SELECT、CTE/递归声明、inner/left/cross join、表达式
  子查询、CASE/CAST、过滤/分组/排序、命名参数、LIMIT/OFFSET、UNION ALL
  及 EXPLAIN 标记；拒绝多语句。保留数字原文，不先转 float64。
  输入最多 256 KiB、32768 token/遍历节点，递归及左深表达式树深度均限 128，
  注释嵌套限 16；诊断保留 UTF-8 字节偏移和可读行列。
  `SourceTables`/`NamedParameters` 从整个语法树提取依赖，包括投影、WHERE、
  EXISTS/IN 内子查询；CTE 名称使用词法作用域，不泄漏到同级子查询。
  这是语法准备，不是已完成的 SQL 执行器：字段/函数绑定、三值逻辑、连接/
  聚合/递归执行、查询资源计量和 `data sql` 命令仍待接通。现有 skill 与
  describe 继续将 SQL 标为未实现，不能用解析通过冒充数据查询通过。
  独立 Luna 回归验证 CTE/同级作用域、表达式子查询依赖、参数、引号关键字、
  写语句/多语句拒绝、预算、取消和返回值归属；主代理验证 AST 运算优先级、
  CASE、整数原文及诊断坐标。全量 Go 测试/vet、该包 race 通过；
  `FuzzCompileBounded` 双 worker 10 秒完成 840,450 次执行并通过。
- `data db2` 在不传 `--id` 时支持有界分页：`--limit` 默认 50、最大 200，
  `--after-id` 为排他性逻辑 ID 游标；nil 起点包括 ID 0，复制行与普通行统一排序。
  `View.Page` 使用最多 limit+1 个候选的 max-heap，不额外复制整个 ID 索引；
  行数组 JSON 预算 8 MiB，任何记录/预算失败均不返回部分页。单行与分页共用
  相同的固定定义、表身份、CASC 和布局绑定流程。
  页 capture 仅在从首行开始且无后续页时标 complete；尾页只表示没有后续，
  不冒充此前页已包含。返回 `after/next/more/ids/rows` 并显式警告未覆盖全表。
  真实 Retail `12.1.0.69875` ChrClasses 用每页 3 行遍历得到 ID 1–15，
  拼接后的 IDs 和全部命名字段与单页完整小表结果相同；真实命令竞态测试通过。
  独立新工作空间/实际二进制按更新后的 skill 首读 `[1,2,3]`、离线续读
  `[4,5,6]` 并校验证据通过。测试根为
  `C:/Users/follen/AppData/Local/Temp/lycheedev-page-4885c126660240de86c286f3d64b563d`，
  第二页 capture `CAP-00dcaa44963ec20a1e5217ff61b09b0d4108bae165ffb154e7196ec6fa2ee816`。
  此次不是 npm 隔离安装；每次 CLI 翻页仍重新打开/校验本地来源，SQL 与流式
  长寿命查询会话尚未实现。skill-creator 数据指引与结构验证已同步。
  Luna 独立回归覆盖复制行、零/最大 ID、空尾页、游标/返回值归属、精确 JSON
  字节预算、取消与 1 MiB 行文本；全量 Go 测试/vet、相关包 race 均通过。
  `FuzzPageLimits` 双 worker 10 秒完成 649,686 次执行并通过。
- `data db2 --snapshot <pin> --installation <root> --table <name> --id <n>`
  已接通固定定义、统一 CASC 读取和单条命名记录；支持 `--offline` 和
  `--max-bytes`。定义只取 WoWDBDefs 的 DataPin exact commit，原始 manifest/DBD
  校验后放入同一 vault，metadata 采用原子创建；缓存每次重新校验 SHA-256
  和语法，拒绝重定向、未知提交格式、路径注入、超限和静默网络修复损坏缓存。
  `ReadLocalRecord` 再核对 manifest 身份、实际 WDC table/layout hash 和 DBD，
  结果包含命名字段、原始定义引用、布局、完整 FileReading 和固定 snapshot capture。
  不将一条记录的 complete 状态扩大为全表或 SQL 覆盖。
  Retail `12.1.0.69875` ChrClasses ID 1 的首次在线固定定义读取、离线缓存重读、
  空缓存离线失败和 evidence verify 已通过；返回 43 字段和 layout `AFC9B0C2`。
  复现命令测试是在 `TestInstalledAssetCommand` 的显式
  `LYCHEEDEV_TEST_DBD_NETWORK=1` 分支，另须指定安装/product/build 三个环境变量。
  skill 数据指引已按 skill-creator 更新实际命令与预算；SQL、表扫描、域查询、
  CDN/Hotfix、密钥提供参数和完整 npm/游戏端到端仍不在已完成范围。
  本轮全量 Go 测试/vet、records/command 竞态测试、真实命令在线/离线竞态
  测试及 skill 结构验证通过。Luna 独立使用实际编译二进制完成同样查询与
  evidence verify，主代理复核原始输出和保留证据。临时根目录：
  `C:/Users/follen/AppData/Local/Temp/lycheedev-forward-a88ab563f21f4b25bed8ecea576f9d85`；
  在线 capture `CAP-a59262116295e3e130598aa67847521285de14411e9d786b787d9ab5e36d3916`，
  离线 capture `CAP-7104d8a46a53e42c9c7c0bcf1c7469d1c76fbb1562a1f540537d0fa9c5a6cf18`。
  两者的 JSON 证据 blob 相同；只证明一个本地固定记录，不是 npm 安装验收。
- 数据读取应用链路已接入 `asset inspect --snapshot <pin> --installation <root>
  --file-id <id> [--max-bytes <n>]`：`selection.DataIdentity` 统一产品/语言映射，
  `records.Reader.ReadLocalFile` 核对当前安装与固定 DataPin 的两个配置键，
  读取前后复核构建、拒绝 locale 回退/多候选，不以损坏或密钥错误触发替代读取。
  同一 CKey 的另一个 EKey 仅在本地对象明确不存在时尝试。完整 Root 和文件
  CKey 校验后写入新 vault，应用层生成含 snapshot/build/FDID/CKey 的原始 capture。
  CLI 无网络或游戏输入，不改安装；目前未暴露 key provider，未完成导出/转换。
  用户声明 region 不冒充本地档案能够证明的下载地区；Encoding 的校验范围
  明确是 EKey/访问页，不声称完整 Encoding CKey 扫描。
  实际 Retail `12.1.0.69875` ChrClasses 经正式命令路由和 evidence verify 通过，
  `TestInstalledAssetCommand` 及其 `-race` 运行通过；独立测试覆盖错误配置、
  未支持身份、预算、取消和歧义。skill 的 asset reference 按 skill-creator
  增加实际可用参数与限制，结构验证通过。不是 npm 隔离安装或整套游戏 E2E。
  Luna 独立前向调用也通过：在临时目录
  `C:/Users/follen/AppData/Local/Temp/lycheedev-forward-test-39f6db33c31049c384a5ca8880f0ed59`
  编译真实 Go CLI，使用全新 `home/`，经 skill 完成 init→target resolve→
  asset inspect→evidence verify。主代理复核保留的 capture
  `CAP-c49903f5e2ac00d4291841064ce08a1fbad6b3103e0319e242982b946eb3ff9d`；
  6452 字节 SHA-256 与真实 ChrClasses 验收一致。此目录仅为本机测试证据，
  不进发布载荷，不能替代尚未实现的 npm 隔离安装验收。
- `schema.ParseManifest` 保存原字节 SHA-256，严格解析表名、table hash 和
  分开的 DB2/DBC FileDataID；拒绝未知/重复字段、名称/文件 ID 冲突、越界和
  空值。hash 碰撞本身合法，`Resolve` 同时核对名称、DB2 ID 和实际文件 hash，
  不用旧 DBC ID 替代。固定上游清单中的 `Item-sparse` 是合法名称。
  本机 Retail `12.1.0.69875` 的 ChrClasses（1361031，enUS locale mask 514）
  已通过 CASC→完整 CKey 校验→新 vault→manifest 身份→实际 WDC layout→
  DBD→类型化行链路。6452 字节内容 SHA-256 为
  `4ffc543ad33964d34fdb8f645b90152a5c94831923a8c30e4fd5972000f2fe20`，
  table hash `F5889D8C`、layout `AFC9B0C2`，ID 1 的 43 字段返回 Warrior。
  DBD/manifest 固定于 WoWDBDefs `83057bdc0cbe13062850ebf8ad530031e128a1cd`；
  manifest SHA-256 `4314266fe055e30207e1c39498a7a31058a1aa2ab6cd674726491b313d135265`。
  复现：设置 `LYCHEEDEV_TEST_INSTALLATION`、`LYCHEEDEV_TEST_PRODUCT=wow`、
  `LYCHEEDEV_TEST_BUILD=12.1.0.69875`、`LYCHEEDEV_TEST_TABLE=ChrClasses`，
  执行 `go test -v -run TestInstalledBuildMetadata -count=1 ./internal/records`。
  此选择显式启用固定来源网络读取；不改变默认 SpellName 的缺失密钥失败证据。
  已通过普通和 `-race` 真实读取测试、全量 `go test ./...` 与 `go vet ./...`；
  manifest 独立回归覆盖严格 JSON、输入归属、冲突及预算，10 秒双 worker
  fuzz 完成 67,166 次执行并通过。skill 本轮未改动，也未声称 npm 隔离安装通过。
  这只证明一个真实表的读取，不代表 DataPin/CLI/SQL、所有表或游戏执行已完成。
- `Records.Bind` / `View.Row` 将实际 WDC layout hash 与 DBD 定义连接，核对
  inline 列数、普通行 ID 位置、直接字段位宽和调色板数组宽度。返回命名的
  类型化值，保留定义 SHA-256/Build/layout 身份；64-bit 整数不经浮点中转。
  稀疏行按字段顺序推进，非内联 ID/关联不消耗行字节，字符串不越出当前记录，
  尾部仅允许不足一字节的零填充。共享文本预算、坏 UTF-8、非有限浮点和
  任意字段失败都不返回部分行；缺失关联为 nil 而不是零。普通字符串复用当前
  行字节。fixture 和上述 ChrClasses 真实链路已接通；DataPin/CLI/SQL 接线
  和广泛真实表覆盖仍待验收，不是完整数据产品交付。
- `records/schema` 已加入 DBD 有界解析和精确选择，保存原字节 SHA-256、
  请求 Build/布局身份、字段顺序、类型/位宽/数组、非内联/ID/关联、外键和
  未验证标记；未知语法、重复字段、坏版本范围或注解均拒绝，不静默漏字段。
  明确布局优先且不回退 Build；没有布局时使用完整 Build/元组范围匹配；
  多定义命中报歧义。返回字段副本，不共享可变解析状态。实际字段绑定和
  稀疏逐字段游标见上一项；SQL 与 DataPin/CLI 应用接线仍待完成。
- DB2 首段：`records/table.Inspect` 以显式文件/元数据/行/列/分区预算读取
  WDC2–WDC5 布局，保留 table/layout hash、WDC5 schema 身份、字段位置、
  存储 codec/参数及分区描述。校验行数、存储表长度、palette/common 声明
  汇总及分区范围；该接口只读取结构元数据，不读取行载荷，也不创建旧式
  全表缓存。行解码、稀疏映射、关联/复制行和 DBD 绑定由后续读取接口实现；
  SQL 与完整数据产品验收仍未完成。
- `table.OpenColumns` 从同一有界布局编译列级公共值/调色板，`Values` 处理
  codec 0–5 的原始字段位、数组、符号扩展、公共值覆盖及调色板索引。
  通过明确列序号保持存储身份，不按相同描述符反查列；返回值副本不修改编译状态。
  记录切片之外的字节、越界调色板、重复公共值 ID 和错误数组宽度都拒绝；
  非对齐 64-bit 字段支持跨九字节。此处输出原始位模式，不猜字符串/浮点/字段名；
  行寻址、字符串表、稀疏/复制/关联记录和 DBD 语义绑定尚待接入。
- `table.OpenRecords` 已接上固定长度行索引，支持外置/内置 ID、复制链和
  ordinal 关联表；索引不可变，不缓存全部行，Lookup 返回请求自有字节。
  复制行保留逻辑 ID 和来源 ID，普通数据从来源读取，内置 ID 值替换为逻辑 ID；
  缺失关联与关联值零区分。循环/悬空/冲突复制、重复 ID、越界或重复关联拒绝。
  行和复制数共享预算，辅助表受元数据预算约束；稀疏布局当前明确返回 unsupported。
  这尚未包含字符串和 DBD 类型/字段绑定，也没有 SQL 或真实 DB2 查询验收。
- `Records.Strings` 已补固定行的直接 32-bit 字符串引用及数组：WDC2 文件相对
  位置、WDC3+ 跨分区虚拟偏移，复制行按实际来源定位；UTF-8/终止符必须在
  目标字符串分区内，总输出字节预算跨数组共享。零指针保留为空串，非法指针、
  无终止符、坏 UTF-8 和超预算明确失败，不返回部分数组。尚不支持稀疏内联字符串。
  与 DBCD 格式实现交叉核对后修正了两个旧实现假设：WDC2 普通字符串不是内联，
  压缩字段读取/符号扩展使用 codec 参数的位宽而非存储区域宽度。
- 稀疏记录已接入同一 `OpenRecords/Lookup`：WDC2 的 ID 范围映射/空洞/共享
  区间别名，WDC3+ 的独立 map ID；记录按实际区间长度读取，重复 ID、部分重叠、
  零长度和越出记录区都拒绝。WDC4/5 SecondaryKey 决定 map ID/关联表顺序
  及关联引用身份，不把所有版本强行套用同一顺序。加密 ID 清单改为只读取
  KeyID 非零分区；无 KeyID 的多分区不再误读行数据。稀疏类型化 Values/Strings
  仍明确 unsupported，须接入有序 DBD 字段解码后才开放。
- 数据迁入首段：`records/container` 的有界 BLTE 解码，支持原始块、zlib、
  多块及嵌套帧；校验非零块 MD5、声明解码长度、完整输入边界。
  预算覆盖编码/解码总量、单块缓冲、共享块数和嵌套深度；取消与写入错误传播。
  Headerless 或零校验块本身没有完整性证明，不能将其标成已验证 CASC 身份。
- `records.PayloadArchive` 将解码结果流式送入同一 vault，核对外部完整
  decoded content key（MD5）后才发布 SHA-256 对象；失败不发布部分对象。
  不导入旧缓存、环境变量或全局密钥。已在固定 Build 的本地 Root 验证中接入
  CASC 寻址；数据 pin 关联、密钥提供方接线及数据 CLI 尚未完成，不能据此
  声明 wowdata 已迁完。
  格式处理改编来源与 AGPL-3.0-or-later 标记见 `THIRD_PARTY_NOTICES.md`；
  组合发行许可审查及完整许可/源码载荷仍是发布前门禁。
- BLTE Salsa20/20 加密块：请求级显式 `KeyLookup`，16/32 字节密钥、
  4/8 字节 IV、块序号 nonce 混合；加密封装与嵌套帧共用深度预算。
  先验证加密头，再查询密钥；取消传播，提供方错误不回显可能含密钥的文本。
  不支持的算法、缺失/无效密钥、格式损坏分别报错，不零填充或假装成功。
  `PayloadArchive` 已传入密钥查询并继续以 decoded content key 校验完整输出。
  密钥不由解码器自行联网获取或写盘，来源验证与数据 pin 接线仍待实现。
- `records.ReadKeySet` 显式接收 text/JSON 文档及预期原字节 SHA-256，限制
  4 MiB/65,536 条，严格核对十六进制 ID、16/32 字节密钥及重复 ID。
  结果是请求级不可变集合，Lookup 返回副本，支持并发读；错误不回显文档或
  I/O 错误中的内容，常规格式化隐藏密钥。不会自行读取环境变量、旧文件、
  联网或落盘。公开源下载目前仅接入显式 opt-in 安装测试，不是生产自动更新器。
  尚未接入数据 pin/CLI 配置或密钥来源持久化管理。
- `container.Ranges` 保存不可变 BLTE 块目录，按需读取目标区间相交块，
  与顺序解码共用头表解析、原始/zlib/加密/嵌套解码及预算规则。
  初始化只读块目录并校对声明编码总长；区间失败不返回部分字节。
  不常驻缓存所有解压块。ReaderAt/密钥查询支持并发且源不变时可并行查询。
  Headerless 容器明确要求顺序解码；块校验只覆盖实际访问块，不证明完整
  content key 或未访问块。调用方仍须用外部 encoding identity 验证头表来源。
  范围接口已接入下面的本地 Encoding 头验证链路，CDN 仍待实现。
- 本地 CASC 归档首段：`records/archive.FindIndexSpans` 有界流式扫描一个
  显式 v7 连续索引，检查头部/条目区 Jenkins 校验、排序、长度和区间预算，
  保留最多 64 个前缀冲突候选，不把九字节前缀视为完整身份。
  `OpenEncodedSpan` 校验归档前导区前缀及 BLTE 完整 encoding key，再交给
  同一 BLTE 解码器。当前只支持连续条目区；页式/旧版索引、
  远程 CDN、Root 条目映射、数据 pin 关联仍待实现；Encoding 映射见下文。
  真实安装样本确认 MaxFileOffset 不是单个归档长度，且本地前导区可能只存
  九字节前缀并补零；已修正这两处假设，没有降低完整 BLTE MD5 核对要求。
- 固定安装元数据：`ReadInstallCatalog` 保留原始产品槽位和 inactive 行；
  `ResolveLocalBuild` 只选显式产品/完整 Build 的唯一 active 行，校验 build/CDN
  配置原始字节 MD5，保存原始内容、SHA-256 和未知字段；重复/坏字段、产品
  UID 冲突、缺配置均拒绝，不根据目录名猜身份或回退到其他版本。
  root-confined 文件读取和大小预算生效。不推断 region、locale、DBD commit，
  尚未把该结果自动构造为完整 DataPin 或接入数据 CLI。
- `OpenLocalObject` 按桶选择最高索引代际，固定配置中的完整 encoding key 和
  编码大小驱动归档查找；不掩盖损坏最新索引、不退旧代际或 CDN，句柄归调用方。
  真实 Encoding 清单存在超预算的大块，范围目录因此仅校验全局大小/块数预算；
  单块预算推迟至访问时检查。未访问大块不阻断小段元数据读取，访问大块仍拒绝。
- `records.EncodingIndex` 解析 Encoding v1 的 CKey 页目录，按内容键只读取
  候选页，验证页 MD5、首键/排序/相邻页边界/条目长度，返回 40-bit 解码大小
  及全部 EKey 候选，不静默截成一个。`FindEncoding` 按需读取 EKey 目录和
  候选页，核对页摘要/排序/边界，返回 40-bit 编码大小及未解释的 ESpec 索引。
  目录和页均有明确预算；尚未接入 DB2。
- `records.LookupRoot` 从已验证、不可变的解码源投影 FileDataID，处理无头旧式、
  12 字节头、v1/v2 的 Root 组布局；保留全部内容键、locale/content masks、
  名称哈希存在性与组编号，不隐式选择语言或低暴力内容版本。
  64 KiB 分批扫描 delta，检查所有组区间、ID 溢出和字节/记录/组/结果预算；
  取消或任意后续损坏不返回前面的部分结果。内容键完整性由调用方先验证。
  修正旧实现遗漏 12 字节头 named count 的问题。尚无面向用户的数据命令，
  locale/内容版本选择策略及固定 DataPin 应用接线仍待完成。
- `Ranges.ReadCheckedSpan` 接受来自已验证格式元数据的独立逻辑页 MD5，
  对落在单个 N 块内的区间只读 mode 和目标字节；其他编码继续走完整块校验。
  这只证明目标页，不证明未读取的块内容。普通 `ReadSpan` 对访问到的零块
  校验值明确拒绝；无块校验的文件仍可用顺序解码加外部完整 content key 验证。
- 单 Go module，语言版本 1.27.0、工具链固定 1.27.1。
- `command -> actions -> desktop/vault` 的首批真实调用；没有旧 CLI/Python 代理。
- 新工作空间 marker、随机 workspaceId、独立目录和原子目录发布。
  检测到无 marker 的非空旧根会拒绝操作，不读取或继承其中配置。
- 原始字节 SHA-256 对象库：有界写入、取消、临时文件清理、重复发布合并、
  已有对象完整性检查。损坏对象不会被静默覆盖。
- Windows LockFileEx 与 Unix flock 资源锁：竞争等待可取消，进程退出自动释放。
  这是低层原语，尚未接入游戏窗口所有权或 jobs 状态机。
- Windows 原生窗口枚举与进程身份复核，包含进程创建时间。
  不切前台、不发按键、不创建捕获会话。
- 原生 WGC 帧流：WinRT/D3D11、free-threaded 事件、单帧缓冲、GPU ROI 裁切、
  受限 CPU 拷贝与关闭释放。窗口尺寸变化会明确失败，不静默读取错位 ROI。
- 纯 Go QR 解码和新信号接收：身份、会话、请求、序号及字节摘要字段检查；
  拒绝旧帧/其他会话，缺帧不构成 ACK。
- 原生 PostMessage 输入：UTF-16、逐消息进程身份复核、控制字符/长度限制、
  取消与部分入队回执；尚未连接到游戏 CLI，必须先接通 bridge 输入资格与锁。
- 统一结果 envelope、JSON/JSONL 错误、退出码和 broken-pipe 失败处理。
- 新版统一 skill 草稿及六类按需参考，使用 skill-creator；格式校验通过。
- SQLite 短事务文档存储与多键 CAS：冲突整体回滚，未来数据库格式拒绝降级。
- jobs 持久化阶段、世代校验和资源归属：未决任务保持归属，清理与释放原子提交。
- 不可变 source/data/change pins：内容寻址，派生保留原固定引用，拒绝替换父引用。
- evidence 原始字节归档、来源与完整/截断状态，读取时校验 manifest 与 blob 摘要。
- Go 数据型 SV 解码与新根入口：预算、UTF-8、转义、深度、重复键检查；不执行 Lua，
  `ReadToolkitState` 不导入旧根。精确报告提取与新 Lua writer 协议仍待接通。
- 新 Lua CaptureWriter：有界 JSON 编码、UTF-8/secret/cycle/metatable 检查、
  总 entry 与深度预算、字节 Adler-32；只提供纯函数，尚未接入新 TOC 或游戏生命周期。
- bridge 报告验证：强制完整预期身份，核对原始提交代码、报告字节数及 Adler-32，
  对原始报告另算 SHA-256，返回独立字节副本。拒绝重复 JSON 字段、旧序号与超限正文。
  该步骤不执行 ACK，不证明 SV 已落盘；必须由后续 Executor 接通。
- 三仓库[能力对照表](capability-inventory.md)，明确能力迁入和待决项。
- codebase 首批能力：五仓库/完整静态产品分支目录、指定引用同步、永久 commit pin、
  固定提交的有界文件片段读取与原始完整文件归档。Git 在新 workspace 的 mirrors
  中运行，不检出工作树、不执行过滤器、不读取旧 wowdoc home。
  已接通 Lua/XML/TOC 语法索引与精确名称查询：统一事实结构、批量 Git 对象读取、
  对象字节身份验证、私有 SQLite 索引构建后整体发布；解析失败保留诊断与不完整标记。
  已接通两个固定索引之间的文档内容/声明位置差异，保留同名多处定义及双端来源；
  不完整索引拒绝给出声明增删结论。已有本地 TOC/XML 加载闭包与语法、源码名称存在性、
  项目固定 Interface 基线检查；原始字节进入同一 vault，未决引用保持可见。
  版本标签规则、更广搜索、别名/作用域分析、模板类型细化及矩阵聚合仍未迁入；
  不能称为 wowdoc 全迁入。
- 原生基础 CI：Windows 2025、Ubuntu、macOS，Go tests/vet、无 CGO 构建，
  Linux race 与失败聚合。Actions 固定到查询过的实际 commit。
  此 workflow 尚未在远端运行，不等同于完整发布验收。

## 当前可以运行

从仓库根目录执行：

```powershell
go run ./cmd/lycheedev version --format json
go run ./cmd/lycheedev describe --format json
go run ./cmd/lycheedev live instances --format json
go run ./cmd/lycheedev target resolve --file selection.json --home <new-root> --format json
go run ./cmd/lycheedev target show <pin-id> --home <new-root> --format json
go run ./cmd/lycheedev live status <operation-id> --home <new-root> --format json
go run ./cmd/lycheedev evidence verify <capture-id> --home <new-root> --format json
go run ./cmd/lycheedev source list --format json
go run ./cmd/lycheedev source sync --source wow-ui-source --product retail --ref <exact-commit> --home <new-root> --format json
go run ./cmd/lycheedev source inspect --snapshot <pin-id> --path <repository-file> --line 1 --count 80 --home <new-root> --format json
go run ./cmd/lycheedev source index --snapshot <pin-id> --home <new-root> --format json
go run ./cmd/lycheedev source query <exact-name> --snapshot <pin-id> --limit 50 --home <new-root> --format json
go run ./cmd/lycheedev source diff --from <pin-a> --to <pin-b> --limit 50 --home <new-root> --format json
go run ./cmd/lycheedev source validate --path <addon-root> --toc <relative-toc> --snapshot <pin-id> --home <new-root> --format json
# 使用明确的新目录；不要用现有 1.x 用户根做此示例。
go run ./cmd/lycheedev init --home D:\Temp\lycheedev-v2-new --format json
go test -count=1 ./...
go vet ./...
```

初始化接受不存在或为空的根，或已是有效的新格式工作空间。
显式旧根归档尚待实现。`target resolve --file` 目前接收已经解析为精确身份的
SelectionSpec，不会自行访问远程服务或把 latest 当精确身份。
`describe` 只公布当前可执行入口，不放置成功返回的空实现。
`live instances` 仅列出候选窗口，不代表已验证角色、build 或绑定会话。

## 已执行的验证

| 范围 | 结果与边界 |
| --- | --- |
| 原 Lychee addon 四客户端离线矩阵 | `add-on/tests/TestAll.ps1` 通过，含 build/静态/打包检查 |
| 原 wowdoc | `go test ./...` 全部通过 |
| 原 wowdata | `go test ./...` 全部通过，包括该仓库现有 analyze/tools 测试 |
| 新 Go 基础内核 | `go test ./...` 与 `go vet ./...` 通过 |
| Windows 数据竞争检测 | `go test -race ./...` 通过，使用本机 GCC；不是无 CGO 发行构建 |
| 五平台构建 | `CGO_ENABLED=0`，Windows amd64、Linux amd64/arm64、Darwin amd64/arm64 全部编译成功；非 Windows 未在本机运行 |
| 新对象库 | 字节往返、超限、不匹配摘要、损坏拒绝和并行重复发布通过 |
| 工作空间/锁 | 旧数据哨兵不变、marker 拒绝、锁竞争、取消、杀死子进程后释放通过 |
| CLI | 根目录优先级、只读命令不创建 workspace、错误 envelope、JSONL、输出失败通过 |
| 编译后 CLI 跨进程 | 新隔离根初始化、文件固定 pin、另一个进程读取同 pin、读取未完成任务、验证原始证据通过；只读状态查询没有推进任务 |
| 持久化状态机 | 完整阶段顺序、拒绝跳步/旧世代、重开数据库、未决归属、清理原子性与独立资源启动通过 |
| SV 数据解析 | 转义 UTF-8、类型化键、预算、拒绝代码/损坏/旧数据库根通过；未读取真实账号 SV |
| Skill 独立场景测试 | Luna 在“探针已验证但 ACK 未确认、另一个 agent 查同版本数据”的样本中先建议只读状态/证据核验，保持 pin，未建议重跑探针；未执行真实命令 |
| 新 Go 对真实客户端的只读枚举 | PID 40120、HWND 81662368、进程创建计数 134343611467620227，成功识别；标识仅为当次观测 |
| 新 Go 原生 WGC 对象 | 显式 PID 40120，WinRT 创建 GraphicsCaptureItem 成功，尺寸 2560×1440 |
| 真实游戏 WGC ROI | 三次打开/关闭，每次收到三帧 640×480 ROI；race 模式重复通过，关闭后 delegate 引用计数为零；未发游戏按键 |
| 原生 WGC→QR | 独立测试窗口在后台绘制 QR，WGC 获取 ROI 后纯 Go 解码，UTF-8 文本精确一致；race 通过。这不是新游戏协议闭环 |
| Lua→Go QR | 实际现有 Lua QR 编码库生成矩阵，纯 Go 解码还原 ASCII/中文/扩展字符原始字节；独立 Luna 测试通过 |
| 后台输入消息 | 仅向测试程序自有窗口发送，正确收到 UTF-16 代理对、Return down/up 参数和字符；没有向游戏发送命令 |
| SignalReader | 其他会话、过期帧被忽略；缺帧返回错误，不产生 ACK 成功 |
| Lua writer→Go 报告验证 | Lua 5.1 生成正文和信号，Go 校验代码与正文 Adler-32，保留 UTF-8 原始字节；篡改正文拒绝；不是游戏端到端测试 |
| 报告验证回归 | 身份/代码/正文错配、旧序号、重复字段、嵌套预算、512 KiB 边界与副本隔离通过；bridge/protocol race 通过 |
| 报告验证有界 fuzz | 本机 10 秒预算、2 workers，57,714 次执行通过；不是持续 fuzz 或完整协议安全审计 |
| 固定源码离线回归 | Git 对象 fixture：CRLF/UTF-8 保留、分支移除后原 commit 仍可读、符号链接/路径穿越拒绝、输出预算与取消；codebase race 通过 |
| 源码 CLI 跨进程 | 真实二进制读取固定源码 pin，返回行片段并归档完整文件；独立进程 evidence verify 通过，原字节/commit/snapshot 匹配 |
| 源码真实网络链路 | 全新临时 home 同步 Retail commit `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`，读取 EventImplementation.lua 110–133 行并验证完整文件证据；未复用旧 wowdoc 数据 |
| Skill 实际调用模拟 | 独立 Luna 加载新 skill，实际运行新二进制 help/describe/source inspect/evidence show/verify；保留精确 commit 与 CAP-84758eee4e64298e4e57d9109356db34f0d22c472f564af8c2e7ca593defb3a4，正确区分完整文件与节选；未调用游戏或旧工具。这不是 npm 隔离安装测试 |
| 固定 Retail 源码索引 | 同一隔离 home 的精确 commit 共分析 4,036 个 Lua/XML/TOC，提取 65,320 个声明及 228,674 个关系，语法诊断 0；此统计不代表加载闭包或运行兼容性通过 |
| 实际精确名称查询 | C_Spell.GetSpellInfo 返回生成 API 定义、签名及推断调用；GetCurrentKeyBoardFocus 返回 EventImplementation.lua:119 的推断调用。限额截断及 capture 完整性状态明确返回 |
| 源码语法回归 | Lua 动态调用/事件、API AST 元数据、XML 属性顺序/内嵌 Lua 行号、TOC 顺序重复项、索引诊断/取消/固定 commit/重开通过。修正了第三方 AST 不填表结束行的问题；静态解析没有执行 Lua |
| 固定源码差异回归 | 文件/声明增删改、同名多处定义、仅行号移动、稳定排序、分类限额/完整计数、同 commit 无差异、索引缺失/不完整/不兼容拒绝通过；跨进程 CLI capture 保存两端 snapshot 与 commit 并通过 verify |
| 真实相邻 Retail 差异 | `710f59e457317676c0f699e6addaf2c405c2a1a4` → `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`，两端均 4,036 文档且诊断 0，索引文档/声明差异为 0。独立 Git 检查只改 version.txt，处于该命令索引范围外；capture CAP-a0e0faabfeef55cdda449031fdfe4bfcaaf1cc8b7c6eacf1141bfbf524123b6b 验证通过 |
| 本地 addon 闭包回归 | TOC/XML 有序深度优先、大小写/反斜线/内根相对路径、缺失文件、重复/循环、越界与 symlink、语法错误、取消和字节证据验证通过；相关 race 通过 |
| 静态验证跨进程 | 缺失加载文件返回 exit 4，同时保留结果与 capture；未知源码基线不推断 Interface，成功但未完成的覆盖明确告警 |
| 当前仓库 Mainline 静态检查 | 新 CLI 读取 add-on/Lychee Dev_Mainline.toc：31 文档、加载/语法无问题、项目 Interface 基线匹配；12 个源码名称有证据、3,653 个引用未决，complete=false。未执行游戏加载或声明完整兼容性通过 |
| BLTE 解码与对象发布 | 独立 Luna fixture 测试通过：原始/zlib/多块/嵌套、坏校验、长度错配、截断、尾随字节、预算、取消与写入失败；主 agent 补充压缩膨胀、超大头表及精确预算测试。外部 content key 不匹配、截断、取消、超限均不发布对象且清理 staging；尚非真实 CASC 数据链路 |
| 数据解码本轮验证 | `go test -count=1 ./...`、`go vet ./...` 和 `go test -race -count=1 ./internal/records/...` 通过；最终 BLTE fuzz 10 秒、2 workers、1,404,118 次执行通过。预算内 fixture/fuzz 不替代完整数据迁入或真实 CDN/本地 CASC 验收 |
| 加密块回归 | 独立 PyCryptodome 3.23.0 固定密文验证 16/32 字节密钥、4/8 字节 IV、非零块序号、跨三个密码块；测试运行不依赖 Python。加密原始块/zlib 到 vault 完整链路通过，错误密钥或 content key 不发布对象且清理 staging。缺失密钥、坏加密头、不支持算法、提供方错误脱敏、取消与深度预算通过；全量 Go 测试、vet 与 records race 通过。尚未读取真实加密 CASC 数据 |
| BLTE 范围回归 | 独立 Luna 读取位置探针验证只读头表/相交块、跨块原始/zlib/嵌套、未访问坏块不读取、访问坏块不返回部分结果、短读、取消、越界/溢出/预算、并发和原始加密块序号；相关 race 通过。主 agent 另以真实临时归档文件的 SectionReader 枚举所有有效区间，并验证关闭句柄后失败；不是游戏 CASC 索引或实际 CDN 测试 |
| 范围目录有界 fuzz | 10 秒、2 workers，356,269 次执行通过；只验证受限输入下的目录/范围解析，不代表完整数据格式覆盖 |
| 本地索引与归档 | Jenkins 原作者独立向量、头/条目校验破坏、截断、取消、错误桶、排序和前缀冲突保留回归通过；完整 encoding key 错配拒绝。全量 Go 测试、vet、records race 通过 |
| 真实安装只读格式链路 | 显式索引 `D:/Game/World of Warcraft/Data/data/00000002fe.idx`（2,752,512 字节）通过保护校验；首条前缀定位 `data.024` offset 489610946、extent 18,355 字节，BLTE encoding identity 与索引前缀匹配，顺序解码得到 44,900 字节。只读、没有游戏命令或文件修改；完整 key 由读取内容计算后与索引前缀交叉核对，尚未来自固定 Build 的 Encoding manifest，因此不是 build-pinned 查询或 DB2/端到端验收 |
| 固定 Build 元数据及 Encoding 头 | `wow / 12.1.0.69875`：build config `9258fbe8b88a178e130b0318b6b86217`、CDN config `5525ea1ce6668e895569c89c2d6a154c` 原字节 MD5 通过，临时新 vault SHA-256 入库/verify 通过；Encoding EKey `ba2e7489a72be875ea80c8de72f91669` 定位索引 `0b000002f1.idx`、归档 `data.156`，BLTE 头 MD5 与配置键匹配，范围读取 22 字节 EN 头成功，声明解码长度 199,496,988 与配置一致。只验证头和访问块，未声称完整 Encoding content key、映射表或 DB2 查询通过 |
| 安装选择回归 | 独立 Luna 隔离 fixture 验证 exact active 选择、不跨产品/Build 回退、歧义拒绝、配置原字节/摘要、未知字段保留、重复/坏值/哈希错配拒绝；主 agent 补充最新索引代际、取消、不回退损坏最新索引、大块未访问/访问预算区分。全量 Go 测试、vet、records race 通过 |
| 固定 Build Root 内容映射 | 同一 `wow / 12.1.0.69875` 的 Encoding 第 17,637 页经页校验得到 Root CKey `a815ffe69c7d5d99c344d2b6717614c6` → EKey `701b023374519bc16c0d4282af5c3d0f`，解码大小 67,167,107；只读实测及 race 模式通过。未读取/解析 Root 内容，也未验证整份 Encoding content key |
| 逻辑页读取与 fuzz | 读取位置探针验证只读 N 块 mode+目标页；错页摘要、零摘要拒绝；跨块/zlib 回退、未访问坏字节与完整块校验的范围区别、缺失块校验拒绝通过。Encoding 页解析有界 fuzz 10 秒、2 workers、273,121 次执行通过 |
| Encoding 映射回归 | CKey 独立 fixture 经主 agent 修正后验证多 EKey、40-bit 大小、缺失键、页校验、目录/页排序及边界、空页和取消。EKey 新回归覆盖物理大小、ESpec 索引、缺失键、校验、首键、重复/跨页键、无效大小、坏尾部、空页、目录边界及取消。全量 Go 测试、vet、records race 通过；EKey 页 fuzz 10 秒预算、2 workers、17,929 次执行通过 |
| 固定 Build Root 完整内容校验 | `wow / 12.1.0.69875`：EKey `701b023374519bc16c0d4282af5c3d0f` 的页校验得到编码大小 50,502,153；完整本地 EKey 验证、解码后的 Root CKey 验证、声明大小 67,167,107 核对全部通过。只向全新临时 vault 发布，SHA-256 `1ce709dca9ca1aa54bde1259797ef9546ec2ae55aa8999d7671706c2b4df4ca0` 复验通过，race 下重复通过；未解析 Root 条目或查询 DB2，未触碰游戏输入及旧工作空间 |
| Root 条目投影 | 后续在上述完整已验证 Root 查询 FileDataID 1990283，保留 11 个 locale 候选；enUS 位匹配的组 63、locale mask 514、CKey `1b3994ed28e3cd054a979a8ef1391983`。Root 投影真文件 race 通过；未把该候选内容声明为已解码的 DB2 |
| Root 格式回归 | 旧式/v0/v1/v2、delta +1、named count、名称哈希、组合 flags、多 locale 同 ID、重复请求 ID、截断、ID 溢出、尾部损坏拒绝部分结果、各预算及取消通过；全量 Go 测试/vet、records race 通过。Root fuzz 10 秒、2 workers、366,212 次执行通过；最后的小计数头判别修正另以 records race 回归通过 |
| 选中文件提取尚未通过 | 尝试提取上述 enUS 候选，第 31 块报告缺少 key ID `14f4b11d7b067aa2`，没有完成内容入库，不能视为文件端到端通过。`LYCHEEDEV_TEST_FILE_CONTENT=1` 显式开启这一额外验收；常规安装 fixture 只验收 Root 内容和投影，不掩盖该缺口。后续必须接入显式密钥提供方再重跑完整内容校验 |
| 密钥提供方与真实公开源 | 后续接入显式提供方并重跑完整提取，仍缺同一 key ID。固定 `wowdev/TACTKeys` commit `89627658c0480f6ec12f82693298d564895a399d` 的 WoW.txt 原始 983,200 字节，SHA-256 `80f1460f9ac2d8504479f417e198a6381a016a406adbe3958169c10d23c5f21f` 核验通过，载入 19,664 条，独立 Lookup 确认该 ID 不在表中。完整提取仍未通过，没有零填充或发布部分内容；`LYCHEEDEV_TEST_PUBLIC_KEYS=1` 明确启用该固定公开源，不读取旧工具缓存 |
| 密钥提供方回归 | text/JSON、大小写 ID、16/32 字节、请求副本隔离、并行读取、重复/坏字段/尾随文档拒绝、摘要错配、字节预算、取消和错误/格式化脱敏通过；全量 Go 测试、vet、records race 通过。密钥文档 fuzz 10 秒预算、2 workers、227,843 次执行通过。公开源载入与 Root 安装链路 race 通过，但这一运行未启用选中文件提取，不代表缺失密钥已解决 |
| WDC 元数据首段回归 | WDC2/3/4/5 fixture 校验哈希、schema 身份、存储字段和偏移；截断、声明不一致、越界、无效 codec、预算和取消拒绝通过。ReaderAt 读取探针证明未读取行载荷。全量 Go 测试、vet、records race 通过；Inspect fuzz 10 秒、2 workers、1,915,472 次执行通过。不是行解码或真实 DB2 端到端验收 |
| WDC 字段存储回归 | 直接数组、位压缩、有符号值、公共默认/覆盖、调色板标量/数组、零位字段；1–64 位和 0–7 位对齐的 512 种组合以独立逐位算法核验通过。相同描述符不同列保留各自公共值；返回副本、越界索引/短记录/重复 ID 拒绝和取消通过。全量 Go 测试、vet、records race 通过；字段 fuzz 10 秒、2 workers、1,411,007 次执行通过。尚未验证真实 DB2 行或 DBD 字段绑定 |
| WDC 固定行索引回归 | 内置/外置 ID、正向引用的多级复制链、逻辑/来源 ID、零值关联、返回副本和并行查询通过；循环/悬空/冲突复制、重复 ID、关联越界/重复/坏长度、缺失记录、取消、关闭源及预算拒绝通过。全量 Go 测试、vet、records race 通过；行索引 fuzz 10 秒、2 workers、1,810,992 次执行通过。限 fixture，不冒充稀疏行或真实 DB2 端到端验证 |
| WDC 字符串及压缩宽度回归 | 四格式普通字符串、中文、复制行、跨分区双向引用、数组共享预算、零指针、坏 UTF-8/终止符/指针及取消通过。独立 32-bit 存储区域/5-bit 编码宽度样例先复现错误值 29，修正后为 -3。全量 Go 测试、vet、records race 通过；字符串 fuzz 10 秒、2 workers、1,607,694 次执行通过。仍是 fixture 验证，未声明真实 DB2 类型化查询通过 |
| WDC 稀疏行映射回归 | WDC2/3/4/5 与 SecondaryKey 组合、变长记录、复制行、WDC2 空洞/共享区间及继承关联通过；错误范围/部分重叠/零长度/重复 ID/数量错配/坏关联/截断拒绝。加密 ID 清单按 KeyID 读取、无 KeyID 多分区不读清单的独立 fixture 通过。全量 Go 测试、vet、records race 通过，最终别名身份与取消修订另跑 table race 通过；稀疏索引 fuzz 10 秒、2 workers、1,907,346 次执行通过。未声称稀疏字段/内联字符串或真实 DB2 查询完成 |
| DBD 定义解析/选择回归 | 原字节摘要、外键/未验证标记、float 显式宽度、非内联标记、数组、精确布局/Build/元组范围、无布局回退、歧义拒绝、坏语法/重复字段/版本/预算/取消及副本隔离通过。全量 Go 测试、vet、records race 通过；定义 fuzz 10 秒预算、2 workers、199,679 次执行通过 |
| 真实固定 DBD 定义 | `wowdev/WoWDBDefs` commit `83057bdc0cbe13062850ebf8ad530031e128a1cd`：Map 45,595 字节、SHA-256 `972819af8ae37fcdcf77c1e6fa0b349852199d96f5b991651587ff68d57575c0`；SpellName 23,086 字节、`712ccb94315b8bd2e7c0c0f52ced7d7a21a8dc3bfd5ad0cbebff6703775ee75d`；ChrClasses 53,098 字节、`f63a29a608648c76f98cabb7b2253781d2f0c6a416bc36404adf6f17c6a99ad4`。显式网络 fixture 及 race 通过，按 Build `12.1.0.69875` 选出 26/2/43 字段；未与实际 DB2 layout hash 或行数据绑定，不代表数据查询端到端通过 |
| DBD→WDC 类型化绑定回归 | 普通/稀疏命名行、复制逻辑 ID、零值/缺失关联、中文、负数、float、数组、64-bit 有符号/无符号精度和副本隔离通过；布局/字段数/直接位宽/ID 位置错配、坏稀疏内容、文本预算、非有限浮点及取消拒绝。全量 Go 测试、vet、records race 通过；绑定稀疏行 fuzz 10 秒、2 workers、654,581 次执行通过。使用独立合成 fixture，不冒充真实 DB2 命名行验收 |

客户端可执行文件为 `D:\Game\World of Warcraft\_retail_\Wow.exe`，文件版本
`12.1.0.69875`，`.flavor.info` 产品为 `wow`。安装 TOC 为 Interface 120100、
addon 1.2.0。以上来自进程/磁盘检查，并非游戏内 GetBuildInfo 回执。
当前安装 registry 存在其他任务，未覆盖、清理、重载或执行这些任务。

## 尚未完成，不能作为 2.0 交付

S1 已验证 WGC 像素帧和纯 Go QR 基础链路；后台输入资格、完整新 Lua 协议、
真实运行/ACK 闭环和广泛窗口状态/泄漏压力测试仍未通过。jobs 与 pins 的应用接线、资源准入、源码/数据/Hotfix/资产迁入、安装器、npm
新载荷、完整 Windows 发布门禁、全量文档切换与四客户端真机回归仍待完成。
当前代码不是全部 Toolkit，也未达到发布条件；不能把窗口枚举当作游戏运行实测。
跨进程测试仅证明持久化链路，不冒充游戏端到端或隔离 npm 安装测试。

## 下一实施段

优先关闭 S1 风险：原生 WGC/QR、指定窗口输入与新 Lua 信号；随后接通已经
落地的 jobs/evidence/SV 内核。源码/数据迁入仍必须复用同一 selection/vault，
不复制旧 home/runtime/CLI。发布前更新所有入口、skill、文档并完成全套验收。
WGC ABI 已按本机 Windows SDK 10.0.26100.0 的
`windows.graphics.capture.h` / `windows.graphics.capture.interop.h` 校对。
下一步是新 Lua 协议的懒启用、可观测输入资格、报告写盘/恢复/ACK，再将已经
验证的 WGC/QR/PostMessage 接入。不能以自有窗口测试替代游戏端到端结论。

来源基线：Lychee `41af9cb616dcb7a9e604619a4fc0ad6c8d6d8210`；
wowdoc `bd1fa8a010cb2b8d7cea8f21cb971705f54a1423`；
wowdata `6191d3dc567966b7a474849f3a11e7411390091c`。
迁入第三方代码前仍需落实许可及署名；不能把 AGPL 来源重新标成 MIT。
