# 当前交接

更新：2026-11-03，Asia/Shanghai。架构与合同见 [Design.md](Design.md)，本次整改与 DuckDB 迁移见 [整改计划](docs/rectification-plan.md)，后续验收见 [todo.md](todo.md)。

## 分支与任务

- 当前 main；基线整改已提交 `271b0c3a87e9c3104c865679ea3d0f0bdf5b12e2`，未推送或发布，npm 渠道保持 3.2.7/latest。用户随后授权隔离 Agent 评估及深入排查、修复同类问题；追加修复随本次提交交付，未推送或发布。
- 主 Agent 维护 command/vault、Skill 和共享文档；records、Source、Publish 修复以及独立复审均已完成，所有 worker 已停写，无文件交叉修改待处理。
- 既有 `.zcodeignore` 删除不纳入提交，`.worktrees/Rust-refactor` 保留。固定编译器缓存 `.tmp/toolchains`、活动项目及无关旧材料不在本轮清理范围。

## 本轮已完成

- 三个无历史上下文 Agent 分别完成 CLI 自解释、固定 Source 调查和本机 Data/CSV 查询。发现的问题均已复现修复：首次工作区错误给出同路径初始化参数；文本帮助逐级显示；发现结果解释产品别名并保留所选平台的恢复链；Source 空结果和 LuaLS 缺失给出有效下一步。
- DBD 元数据支持多词名称、可选注释、BUILD 预设及版本覆盖默认值；Hotfix 在读写前拒绝无效编码或不成对的 CSV 参数；Data schema 与执行共用行数、深度边界和默认值。
- CurseForge plan 明确本地检查范围、远端条件和可选版本选择；DD 新回执保留原 session，status 给出原 request/session 恢复输入，旧回执仍可读取。原有账号和不重放边界保持。
- Skill 改为直接的任务提示，命令细节继续由 CLI 提供。主文档与 Changelog 已同步；无新增平台写入或游戏操作。

## 验证证据

- 每批代码 `go build ./...`、`go vet ./...` 与受影响测试通过；最终 command、records、vault、publish、codebase 五包通过，Lua 5.1 为必需输入。新增回归均先复现失败再验证修复。
- 独立复审无新增确认缺陷或待复现候选：固定上游 358 份元数据 × 7 个构建，共 2506 组独立 oracle 对照通过；CF plan/恢复及 DD status→verify/resume 原会话检查通过。
- 最终全新 Agent 按 CLI/Skill 完成首次初始化、正式 Forever 入口识别、枚举查询、真实 SpellName CSV 与 CurseForge 离线预检，无阻断任务的 help/schema 冲突。真实 Data 固定 Retail 12.1.5.70688/zhCN；ID 17/116/133 及 CSV 哈希一致，SpellName 全表缺钥仍保留 partial，未叠加 Hotfix。Source 调查固定提交 `18c6d677b605b312f3d813d498404631fb941977`，API 声明与实际调用点已核实。
- Skill quick validate、6 份 Markdown/351 命令的 Skill 合同、845 个 Go 文件导入合同和版本检查通过，版本仍为 3.2.7。
- 最终 Lua 5.1 必需完整 Go 套件通过：固定 GCC14.2/CGO1，`-p=2 -parallel=4 -count=1 -timeout=0 ./...`，35 包、3446 条测试通过事件、48 项跳过（38 项环境材料、10 项 helper）、0 失败、exit0，另有 3 个无测试包。跳过项包括可选 LuaLS/DBCD/固定外部输入、真实客户端、跨盘环境与符号链接权限；基线集成证据单独保留。

## 待验与下一步

- 本轮验证和收尾已完成。PowerShell 清理脚本先核对绝对路径、目录链接及完整套件 exit0，预览后删除 `.tmp/agent-eval` 内全部隔离项目、临时报告和脚本；其中无 Live 连接或恢复事务。活动项目、编译器缓存和保留工作树未动。后续具体工作为 todo 中的真实平台/客户端与规模验收，按用户后续范围执行。
- 真实账号/平台写入、短信、UAC、游戏操作及云端 CI 未执行。正式 Forever 实机、六产品安装/DBD/缺钥/Hotfix矩阵、联网冷 CDN、迁移前后同输入性能及大输入/多进程资源测量继续列 todo。
- ModUs 图片字节无读回端点，图片修改保留待核验；未知 storage 登记无查询端点时保留 pending、不重放。NewBee Creator 缺少稳定账号 ID，原会话绑定有效 token 摘要，token 刷新暂停原 pending 续写。
- 基线已验证原生 SQL/CSV/Hotfix/资源导出、隔离 npm 安装、固定预编译原生库的 vendored 工作区快照离线重建；LuaLS/DBCD 集成和 98 项 Node 检查的证据属于基线。此次未重复无关包装测试，不扩大 tag 发行、DuckDB C++ 重编译或实机覆盖。
