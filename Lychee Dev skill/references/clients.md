# 选择客户端、实例与证据位置

发送前使用本节。目标顺序是 **版本 → 该版本实例 → 角色/服务器 → 安装目录与 SV**。PID/HWND 是内部绑定参数，不是要求用户理解的选择菜单。

## 产品识别

| 产品 | 版本基线 | Interface | 常见位置 |
| --- | --- | --- | --- |
| retail | 12.1.0 | 120100 | `_retail_` |
| classic | 5.5.4 | 50504 | `_classic_` |
| titan | 3.80.2 | 38002 | `_classic_titan_` |
| forever | 1.60.1 | 16001 | `_classic_beta_` 或 `_forever_` |

读取 `.flavor.info` 的 product code，再读取 `version.txt` 或可执行文件版本，最后才以文件夹名兜底。`_classic_beta_` 可能是 Forever，也可能是 MoP 测试轨；所有非 Retail 产品都可能使用 WowClassic.exe。窗口标题和 exe 文件名不证明产品。不自动扩展到 Era、Anniversary、PTR 等产品。

`lycheedev clients` 列出安装；`lycheedev instances` 枚举运行实例。用户已选版本就沿用；仅一种受支持版本运行时可直接使用，多种且未指定时先让用户选版本。只有选定产品的窗口进入后续扫描，不先把所有 PID 列给用户。

## 实例选择

选定版本只有一个实例：自动绑定该实例，不再问 PID。记录本次枚举得到的 HWND、PID、完整 exe 路径、实际版本和安装目录。对多个安装目录不能仅按产品名选配置。

同版本多个实例：逐个后台触发身份标记，然后读取该窗口；完成整轮后展示“角色—服务器”供选择。每个窗口使用本次枚举的精确参数：

```text
python <skill>/scripts/automation.py send --mode messages --hwnd <hwnd> --pid <pid> --exe-path <exe> --text "/dev auto identify"
python <skill>/scripts/automation.py identify --hwnd <hwnd> --pid <pid> --exe-path <exe> --timeout 3
```

`identify` 是只读解码器，不会自动让游戏显示二维码。`instances --identify` 同样不能替代触发步骤。不要并发给多个窗口打字；当前 Python 批量读取器虽然支持并发捕获，编排时仍逐个触发和读取，以隔离窗口状态。

核对身份 QR 的 `v/id/realm/client/build` 与所选产品和进程版本一致。完成标记含 `ticket`，不能当作角色身份标记。若有他人的运行中色块或待收取完成码，先处理其归属，不能用身份标记覆盖证据。只清理自己触发的身份码：`/dev auto unidentify`。

名字缺失、realm 缺失、重复角色+服务器或读取失败均保持未识别；不能静默挑第一个/ordinal 0。先说明未识别窗口的原因，继续可独立完成的读取；仍无法消歧时让用户指明角色或关闭多余实例。不要以“后台不需要焦点”为由跳过角色识别。

## 绑定的有效期与现有 CLI 限制

每次开始输入以及中断恢复时重新枚举。进程退出、重启、角色改变或安装目录改变后重新绑定；不要把旧 PID/HWND 或旧 ordinal 当作持久身份。

当前 wrapper 的 pin 仍可能回退 ordinal，默认配置客户端也可能与选中运行实例不同。不要依赖这个回退；Agent 按上述步骤选择后，直接调用 Python，显式传 HWND/PID/exe、安装目录和 SV。当前 helper 只核对 PID/exe，尚未核对进程创建时间；不能声称它已排除所有句柄/PID 重用。

同版本多实例扫描目前由 Agent 组织，CLI 没有完整的“先版本后角色”交互向导；不要编造参数或声称自动向导已实现。

## SavedVariables 绑定

每个安装的候选文件位于该客户端目录的 `WTF/Account/*/SavedVariables/Lychee Dev.lua`。配置中的 svPath 只是候选；最新 mtime 也不是账户身份。不要读取 `.bak` 代替当前数据。

已有 Ticket：在选中安装的候选文件中定位精确 Ticket，读完整报告后核对角色、服务器、产品与版本。

新调查但账户未绑定：先准备任务并取得匹配完成码，执行一次有去重记录的输出 reload，然后在候选 SV 中查找精确 Ticket。唯一且环境吻合后才绑定该路径。不能给 `run --sv` 随便填一个账户再把读失败视作任务失败。不同文件出现同一 Ticket 时结合 requestId/revision/environment 消歧，仍不唯一就停在定位阶段。

从 QR 取得的版本可能只含语义版本；完整 build 用进程版本和报告 environment 补全。每次实机结论都记录实际版本，不拿本表基线代替现场版本。
