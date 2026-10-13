# Rust 候选兼容性决定

依据：[用户修订目标](rust-refactor-requirements-2026-10-08.md)。冻结 Go v3.1.2 的 89 条公开命令仍是完整业务及参数清单；下列明确差异不能推广成其他业务的删减许可。

| 范围 | 决定与依据 | 必须保留的业务 | 当前验证边界 |
| --- | --- | --- | --- |
| 色块、光学心跳、WGC | 用户明确取消；候选不保留光学回退。Mailbox 是唯一状态、输入条件、回执及结果读取通道。 | 状态可判定性、输入门控、回执、结果、清理与恢复；缺少或无效 Mailbox 时拒绝业务按键。首次激活单独设计。 | 新 Native 协议尚待完整设计和独立审计。已有光学/WGC 代码与 fixture 仅为历史材料，不能算新候选通过。 |
| 光学相关参数和路径 | 逐项列出旧公开参数与移除理由；至少包含旧 `live bind --capture-area`。最终拒绝行为和命令合同需随最终协议明确并回归。 | 其余命令、身份选择、绑定、正常及异常业务不因参数移除自动取消。 | 参数及路径完整盘点尚未完成；不能笼统将整个 `live bind` 或恢复命令删掉。 |
| EXECUTE + SETTLE 草案 | 用户要求重新设计，消息划分和状态机均未预先批准。 | 500 槽、多 build、多实例隔离、有界恢复、私有授权及持久结果。 | 旧原型、47 任务容量 fixture 等证据保留原版本范围，最终协议需重新建立不变量与回归映射。 |
| Mailbox 定位（2026-10-12 最新） | 参考 Go `v3.1.0`：运行时指令特征匹配/RIP 动态根，再沿 Lua state/globals/Mailbox 读取；停止私有堆扫描及 hints/附近搜索回退。 | 固定进程/模块、唯一命中、操作数/anchor/root/path/layout 验证、认证、本次观察、BODY、隔离与统一预算。 | 总方案 §7；堆扫描 tests 保留历史，新 RuntimeModule/RootBinding 生产接线尚未验收。 |
| Lua 5.1 中的 `goto` | Lua 5.1 将其允许为普通标识符；新版接受合法标识符是已说明的正确性改进。`goto label`、`::label::` 仍须由真实 luac 5.1.5 拒绝。 | 语法检查质量、原位置、XML 内联映射和诊断，不允许引入跳转语法。 | 已有实际编译器正反例保留；最终封存包仍须绑定并验证同一固定编译器。 |
| Forever 支持范围 | 用户指定 Forever Asia Beta → `wow_classic_beta` 与 Forever CN Beta → `wow_cn_beta`，两者身份独立，取代历史 Go 项目文件中的排除条款。 | Retail、Classic、Titan 也全部必需；不同 build 各自多实例混合及上限压力验收。 | 已有四类固定源码验证不等于全部客户端实机通过，缺失场景继续记为未验收。 |

其余公开字段、结果、完整性、退出码、分页、截断、取消、超时和恢复默认保持等价。新增分析结论必须区分文本命中与语义绑定；新增完整性字段不得用来掩盖旧字段的业务差异。

命令注册快照见 `rust-command-coverage-2026-10-08.json`；其 `binary.sha256` 仅绑定该份封存二进制，不表示当前源码或最终候选。该快照的注册数量与分项证据均不能单独证明命令或参数已完成。完整的最终“旧行为—实现—用例—证据”矩阵仍待逐项闭环。

## 光学相关公开参数盘点

2026-10-08 对冻结的 89 条命令表进行参数检索，并核对 Go
`internal/command/command_contract.go` 与 `docs/toolkit/design.md` 的语义。
受查 `crates/lycheedev-cli/src/commands.json` SHA-256 为
`45b22260c7705b277459714804c1cea51e0328e97d5c7b28f3a514f7b979cff5`；
原始检索结果保存在 `.tmp/rust-supervision/optical-public-flag-inventory-01.json`。
这是公开参数盘点，不是实现依赖已清除或最终协议已通过的证据。

| 公开参数或能力 | 本次迁移处理 | 依据与待验项目 |
| --- | --- | --- |
| `live bind --capture-area <window\|x,y,width,height>` | 随 WGC 区域选择移除；最终命令合同不再宣传，显式传入应返回参数错误和迁移说明，不静默忽略。 | 冻结合同唯一显式截图区域参数；旧设计将它用于 WGC/QR 读取。最终 parser、help、describe、Skill 与错误输出须统一回归。 |
| `live bind/reset/reload fallback --wake-binding` | 属于输入绑定能力，不能因取消光学而直接删掉。 | 新协议须明确它对应哪个真实入口、绑定冲突与恢复规则；未定稿前不承诺旧键位可直接复用。 |
| `live instances --passive` | 保留无游戏输入的实例发现语义。 | 实例发现不授权角色、runtime 或执行资格；不能用发现结果代替 Mailbox。 |
| `data hotfix --from`、`--scan` 中的 capture，以及 `evidence bundle --ids` | 保留。 | 这里的 capture 是证据对象/数据分页，不是截图或光学通道。 |
| `data hotfix`、`target resolve/add/available --region` | 保留数据地区语义。 | 不是截图区域；不能借光学移除更改客户端地区选择。 |
| `live bind`、`live hide`、`live finish` 等整个命令 | 不根据光学实现占比整条删除。 | 身份绑定、工具拥有的界面/焦点清理、持久结果与恢复等保留业务需映射到最终 Native 协议；只取消已经批准的光学读取/确认。 |

待完成的独立盘点：旧输出中的光学状态与诊断字段、安装 payload、运行依赖、
Skill 指引及 fallback 入口。历史 Go 文档和 fixture 可以保留原事实；最终 Rust
产品和新协议测试不得继续借用 WGC/色块结果作为准入或完成证明。
