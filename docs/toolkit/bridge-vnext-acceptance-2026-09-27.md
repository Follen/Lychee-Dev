# 桥接 vNext 工作区验收 — 2026-09-27

状态：修复后自动检查通过；Retail 生产身份连接已验证，探针完整游戏链未通过。本记录针对当前未提交的开发工作区，
不代表已发布的 2.0.5 或正式 2.0.6 包。

后续全量审计的测试、安装、新会话和未决接收器证据见[全量审计](full-audit-2026-09-27.md)。

## 已实现的工作区合同

生产桥以专用接收器承载 first-contact identify/connect/reset 和后续业务动作。
LDB1 严格暂存后，CLI 核对新鲜 staged 回执；物理提交只请求
`receiver_commit_ready` challenge，精确关联 nonce、attempt、challenge 与正文摘要
的 LDC1 才能 dispatch。默认键为 Ctrl+Alt+] 唤醒、Ctrl+Alt+Shift+] 提交、
Ctrl+Alt+[ 关闭。接收器绑定可在设置页或 `/dev receiver bind` 配置，冲突、
无效及未生效 override 拒绝；自定义首连 wake 由 `--wake-binding` 明确提供，
ready 回执返回实际三键 profile。

`live probe load` 要求 1..120 秒执行预算并固定在请求中。完整业务报告走
SavedVariables，QR 只承载短状态、身份、摘要与最小错误。首连输入预先记录
`BTP-...`，固定 Esc×3 Enter /reload Enter 兜底记录 `OP-...`；status/resume
恢复原尝试，不重发未知输入。用户明确决定停止时，abandon 保存未知效果并
释放宿主所有权。Skill 已更新 source→假设→有界探针→预算→报告→finish 的
编排及 load 后重建场景的要求。

工作区 UI 已接入共享 Theme、工作台八页、可编辑接收器绑定的 Settings、
对象详情的 secondary 页面及有界动画。用于真机检查的
`.tmp/bridge-vnext-ui-probe.lua`、`.tmp/bridge-vnext-ui-about-display.lua` 和
`.tmp/bridge-vnext-ui-settings-display.lua` 夹具已准备，尚未在生产客户端
执行；实现与视觉验收分别计量。

## 已执行的验证

| 范围 | 结果与证据 |
| --- | --- |
| Go 编译与静态检查 | `go build ./...`、`go vet ./...` 均通过。 |
| Go/Lua 全量自动测试 | `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` 全部通过；完整输出保存在 `.tmp/bridge-vnext-root-final.log`。 |
| 定向输入测试 | 带标签的 desktop/bridge-input 测试通过。 |
| 发行工具自动测试 | 16 项 Node release 测试通过，包含包内固定 LuaLS；`node tools/version.mjs --check` 通过。 |
| Skill 合同 | 仓库和开发包内 `tools/skill-contract.mjs` 均通过；84 条命令、226 个引用、0 违规。 |
| 本地 npm 安装 smoke | 私有 developmentOnly 开发包 `.tmp/bridge-vnext-dev-20260927/lycheedev-local-2.0.6.tgz` 在隔离目录以 `--ignore-scripts` 安装，包内 CLI 的 version 可运行。此包来自未提交工作区，不是发布组装、签名或 registry 回读。 |
| 独立 Retail 输入实验 | [两次后台 PostMessage 回显](../../tests/bridge-input/acceptance-2026-09-27.md)已通过；实验插件不执行生产探针。 |
| Retail 受管插件磁盘安装 | 使用私有开发包内 CLI，以 `--release .tmp/bridge-vnext-dev-20260927/dev-npm-stage --installation D:/Game/World of Warcraft/_retail_` 安装成功；`.tmp/bridge-vnext-addon-install.json` 记录安装及 `.tmp/bridge-vnext-addon-upgrade-20260927` 归档旧文件。随后 `.tmp/bridge-vnext-addon-status.json` 报告 Retail 12.1.0.69933 / 120100、`managed`、53 个清单文件。仅证明磁盘受管字节与 receipt 一致。 |

上表记录首次 Retail 生产首连前的工作区与开发包。下述首连暴露新故障；
修复后的完整自动验收结果另见本节下方，不能把两次构建混为同一产物。

## Retail 生产首连故障与后续恢复

第一次 `live connect` 在发送前的 preflight 失败：用于计算报文容量的最大
runtime epoch 超出合法协议范围，操作停在 prepared，未完成接收提交。
`.tmp/bridge-vnext-retail-connect.json` 记录
`BTP-8876e46981f3aec17327a8a966a499f0` 与 `live.receiver_wire_capacity`。
用户明确决定后，该 BTP 已 abandon；此结果不证明任何业务动作执行。
对应 Go 修复已完成定向测试、`go build ./...` 与 `go vet ./...`，并进入
下述修复后全量复验。

第二次首连的 `BTP-febd8c817ef3f697483678ad9c7d4797` 停在
`wake_requested`；`.tmp/bridge-vnext-retail-connect-fixed.json` 显示
`context deadline exceeded`。真实 WGC 看到接收窗口出现，但它只完成部分
初始化；没有 staged、LDC1 commit、accepted 或业务动作证据。BugGrabber 的
session 996 指向 `addon/Bridge/Receiver.lua:221`：EditBox `SetFont` 缺少第三个
flags 参数。用户另外报告 `addon/UI/Window.lua:338` 设置页及
`addon/UI/Widgets.lua:566` 文本区同类问题。后续修复覆盖全部四处 EditBox
字体 flags 与 Theme 字体路径；Settings、Run、Receiver 的 strict stub、
constructor/before timer/after show 回滚测试及 `go build ./...`、`go vet ./...`
已通过。

用户随后退出游戏，当时暂停游戏输入。第二个 BTP 在该观察时仍为
`pending`；后续用户给予持续的 abandon 授权后，已对原 ID 执行 abandon。
这保留了当时未知的输入效果，不证明按键未送达。

修复后另生成私有开发包 `.tmp/bridge-vnext-ui-dev-20260927`（release 清单
1275 项资源），加入紧凑圆角资源，并再次向 Retail 安装受管 addon。
`.tmp/bridge-vnext-ui-install.json` 为成功安装回执；
`.tmp/bridge-vnext-ui-status.json` 报告 Retail 12.1.0.69933 / 120100、
`managed`、54 个文件。这只核验当时的磁盘字节。随后只读进程发现看到 WoW
以新 PID 61784 重启；在该轮没有再次发送游戏输入，也没有确认新文件已在
运行态加载。后续进行了新的身份连接尝试，结果如下。

`BTP-2d703...` 的身份回执经原尝试恢复为 confirmed，获得验证会话
`SESSION-af21566369ba7a9548899142dbd6d707ff2206cbf942f22f1729c8a43a14859d`。
这证明 Retail 生产桥的身份与连接阶段已经成功，替代了此前“新运行态身份未验证”
的当时状态（`.tmp/bridge-vnext-ui-connect-verified.json`）。随后请求 probe load 时，
会话重连产生 `BTP-db822e90e39c1f6fe5172f79a01adad2`，其 action 是 `connect`，
还未触及探针业务输入。它在同一 session nonce 下遇到错误的回执匹配；
修复恢复路径过滤 legacy records 后，原 BTP 的只读恢复未找到
有效 QR，玩家已转区，等待超时。根代理依照用户已给的 abandon 授权对该原 BTP
执行 abandon（`.tmp/bridge-vnext-reconnect-abandoned.json`）。没有 probe stage/commit/accepted、probe load 完成、run、SV 报告或
finish 的证据；abandon 也不证明那些未知按键未送达。
新的 active-session 首连重连合同要求 `receiver_ready` 携带原 session 的
sequence/epoch；同 nonce 只有在同 epoch 且新的业务 ready sequence 前进时才接受。
冻结后的 live/bridge 定向测试、command 测试、Lua 5.1 addon/bridge-input/protocol
套件以及 build/vet 已通过；这验证离线路径，不把这次中断的生产 probe load
写成成功。固定 fallback 在新安装包上的真实运行仍为 `not_run`。

修复后 `LYCHEEDEV_REQUIRE_LUA51=1 go test -count=1 ./...` 全部通过，退出 0；
完整输出为 `.tmp/bridge-vnext-ui-final.log`，其中 live 包 131.466 秒、
addon 测试 5.213 秒、protocol 测试 14.514 秒。`node tools/version.mjs
--check`、skill-contract（84 条命令、226 引用、0 违规）及 `git diff
--check` 均通过。这些结果验证当时代码与离线协议，不将未决 BTP 或
未运行的真机链升级为 passed。

## 未完成的验收

后续全量审计使用新的私有开发包 `.tmp/full-audit-dev-20260927`：
`.tmp/full-audit-addon-status.json` 证实 Retail 12.1.0.69933 / 120100 为
54 文件 clean managed 安装，`.tmp/full-audit-connect.json` 证实新会话
`SESSION-dec98b3d556d366cadc11d2b28b11d2ae46c63ce23366d30848879912170b7f2`。
这两项分别证明磁盘 receipt 与身份连接，不证明探针业务已运行。
`BTP-43744f28f245b8b4bc74fcb13f8f53ae` 的只读 resume 仍停在
`wake_requested`，随后依用户持续授权明确 abandon；
`.tmp/full-audit-ui-load-abandoned.json` 保留原 receiver attempt 的
`wake_requested` 和未知输入效果。固定 fallback reload 的
`OP-ed3040d36fa1585003d3156a7a8617f7` 发出 reload 输入后，下一次
identify wake 超时；该 OP 亦依授权 abandon，见
`.tmp/full-audit-activation-abandoned.json`。其 `report.state=unavailable`、
`cleanup=abandoned`、`complete=false` 不表示 ACK 或 runtime unload 已验证。
全量强制 Lua Go 测试 `.tmp/full-audit-final-go.log` 通过，Node release
测试 `.tmp/full-audit-package-tests.log` 为 16/16 通过；接收器 accepted
清理缺少持久证据、输入发送后观察 fence 不足仍是开放修复项。计划中的显式
Close / `receiver_closed` 回执及 host 精确读回，尚未计为通过。

生产 Retail 首次身份连接已通过，但完整连接→加载→运行→SV 核验→finish 链
未到达经验证的 load 阶段，不能记为 passed。最初安装时未向游戏输入或 reload；
旧构建首连观察到接收器半初始化的记录仍保留为历史故障，后续身份回执和会话
成功不等于探针执行成功。
旧独立 LycheeInputLab 的删除被本机策略阻止；核对其清单和文件
摘要后仅将准确的 `LycheeInputLab.toc` 改名为
`LycheeInputLab.toc.disabled`，让下一次加载跳过该实验插件，未删除其文件。
全局 npm 安装未替换；本次匹配 CLI 位于开发包 `dev-npm-stage`，隔离 npm
smoke 与上述受管安装均不构成正式发布。

Classic 50504、Titan 38002、其他键盘布局、IME、玩家干扰、真实鼠标分派、
战斗、relogin、首次安装/升级、工作台和接收器视觉前后截图、内存/帧时间与
长时间空闲资源测量均为 `not_run`。自动测试和独立回显不能替代这些结果。
