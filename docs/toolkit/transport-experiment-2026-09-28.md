# 内存回传与按键 LoD 输入实验

日期：2026-09-28，约 02:23–02:40（Asia/Shanghai）。这是独立实验，不是生产桥的替换或发行验收。

## 结论

两个方向均已在当前 Retail 做到真实传输：外部只读提取 addon 报告；外部改写尚未加载的槽位文件，再通过一次模拟按键加载它。可行性成立，但内存状态的“当前性”仍需独立依据，没有得到可直接上线的跨 build 定位配方。

- **Addon → CLI：** 8 并发扫描能定位报告；已知地址读取快，但 Lua 字符串是不可变对象，状态变化会分配新位置，旧头和旧正文继续存在。ACK 不是内存擦除，重复读取相同字节也不证明仍有效。
- **CLI → Addon：** 本次新增的四个 LoD 插件在 `/reload` 后可见；reload 后写入的 Lua 在首次 `LoadAddOn` 时被读取。每个槽在本轮 Lua 运行期只能加载一次；下次 reload 后可复用。
- **生产建议边界：** 内存缓存只作加速，必须携带进程创建身份、运行 nonce、ticket、长度和完整性校验；缺少可信的当前 head 时应拒绝提供“当前状态”。LoD 需要预留槽位、明确耗尽、关联 nonce、丢回执恢复和一次性消费规则，不能无限循环覆写同一个已加载 addon。

## 目标与方法

| 项目 | 实测值 |
|---|---|
| 客户端 | Retail 12.1.0.69933 / Interface 120100 |
| PID / 创建身份 | 61784 / 134349776305986076 |
| 窗口 | 616501952 |
| Wow.exe SHA-256 | `d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd` |
| 工具 | Lychee Dev 2.5.1；wowdump 1.0.0 |
| 输入 / 观察 | 独立 lab 白名单命令，逐消息窗口与占用核验；WGC 截图 |
| 外部内存 | wowdump CLI，只读；没有注入、写内存或附加调试器 |

实验源与重跑步骤保存在 [tests/transport-lab](../../tests/transport-lab/README.md)。原始 JSON、WGC 和测试日志在仓库 `.tmp/transport-lab/evidence/`；本报告中的文件名均相对此目录。没有修改生产 addon、CLI 或 skill。

## 内存实验结果

Lab2 设五个 ticket，经历 loaded、running、reported、acknowledged，共六个阶段。正文覆盖 ASCII、中文 UTF-8、含 NUL 的二进制、相同内容与不同 ticket 身份。校验 run、ticket、报告阶段、长度、Adler32、尾标记、确定性完整正文，并计算 SHA-256。当前 run/阶段来自 WGC 中 addon 的状态输出，绝不由“最大 epoch 命中”推断。

| 用例 | 结果 / 证据 |
|---|---|
| 8 并发首次发现 | `scan-epoch0.json`：15,672 ms，读取 13,095,723,008 字节；一个分区 `complete=false`，未因命中而宣称全覆盖 |
| 阶段 0–4 当前头与对应报告 | `census-epoch0.json` 至 `census-epoch4.json`，均 `ok=true`；对应 WGC `status-initial`、`step1`…`step4` |
| 1 KiB ASCII / 64 KiB 中文 / 128 KiB 二进制 | 对应 ticket A/B/C 验证通过 |
| 512 KiB 二进制 / 4 KiB 重复内容 | 阶段 4 ticket D/E 验证通过 |
| ACK 后旧正文仍可校验 | 阶段 2 已 ACK 的 B 仍被发现；阶段 5 的第二轮发现五个旧正文候选，但当前头零个可用报告 |
| 缓存区域对新状态头 | `scoped-epoch4.json` 六个区域全部扫描完整，2,476 ms，但没找到当前头；全范围重新定位后验证通过。第一轮阶段 5 的区域缓存同样未命中，不计作验证通过 |
| 同进程 reload 之后 | `old-addresses-after-reload.json` 包含失效地址读取错误及旧 run 的残留正文；`census-after-reload.json` 成功定位新 run，同时列出两个 run 的候选 |
| reload 后再次发现 | `scan-after-reload.json`：14,609 ms，13,119,447,040 字节；仍有读取缺口，不能证明枚举了所有副本 |
| 8 并发定点读取 | `cache-eight.json`：8 次同一份 128 KiB 报告读取共 312 ms，全部验证通过；含 wowdump 子进程启动开销，非持续吞吐或八个独立 ticket 的基准 |
| 全部 ACK | `census-second-epoch5.json`：当前 epoch=5，五项 acknowledged，reports=[]；`second-step5-after.png` 确认实际阶段 |
| 离线损坏 / 截断 | `negative-fixtures.json`，33 项断言通过；迁移后的重跑脚本也通过 |

首次 run 为 `6ab95bb113725a4f`，第一次 reload 后为 `6ab961271387aeeb`。头地址分别观测到 `0x232a2a6d110`（首次 epoch 0）与 `0x232e3803630`（reload 后 epoch 0），不能将这些 VA 当成配方。

这些实测支持“首次扫描、按报告身份缓存正文”的优化，但否定“只扫一次旧区域就永远能拿到新状态”的假设。新 ticket 的分布没有表现为可直接复用的固定地址序列。

## 按需加载实验结果

控制器通过临时 Ctrl+Alt+F9 绑定，在按键释放时加载下一个 LoD addon，共四槽。无定时轮询；战斗中拒绝；无效但已经加载的槽仍被消费。实验内容只生成数据回执，不执行游戏业务。

| 用例 | 结果 / WGC 证据 |
|---|---|
| 运行中新增四个槽，reload 后发现 | `lod-status-after.png`：1–4 全部 known，next=1，attempts=0 |
| reload 后才写槽 1 | `lod-load1-after.png`：accepted slot=1，nonce=`written-after-reload-01` |
| 第二槽中文内容 | `lod-load2-after.png`：accepted slot=2，nonce=`中文-第二次-02` |
| 第三槽空内容 | `lod-load3-invalid-after.png`：invalid slot=3；不重复使用已加载槽 |
| 第四槽继续 | `lod-load4-after.png`：accepted slot=4，nonce=`after-invalid-04` |
| 第五次按键 | `lod-exhausted-after.png`：exhausted；不越界加载 |
| reload 后槽 1 复用 | `lod-reuse1-after.png`：accepted slot=1，nonce=`second-runtime-slot01` |
| 改写已加载槽并再次 LoadAddOn | `lod-replay-after.png`：loaded=true，payload=nil；新内容没有执行。API 成功不等于重复执行文件 |

参考项目 [wow-ai 安装脚本](https://github.com/chelinho139/wow-ai/blob/main/bridge/install-slots.js) 使用预建 LoD 槽，其启动索引说明不能代替本客户端实测。这里只确认当前 Retail 的 reload 发现行为，不能据此承诺所有客户端都无需完整重启。

本轮 reload 操作 `OP-fee8816355e7c52a7fe973371cf641a9`、`OP-1cebf0d58dd1854e8e6239df7f25962b` 均 complete=true、cleanup=complete。LoD 的逐槽执行由独立 WGC 回执证明，不把 reload 命令成功当作槽执行证据。

## 保留边界

- 同 build **完整进程重启**、跨 build、Classic/Titan/Forever、多实例：`not_run`。
- 活跃 Lua 全局表的稳定根、指针链、跨 build 可迁移 recipe：**未建立**；当前使用外部确认的 run/epoch，不声称纯内存自主发现了当前性。
- 扫描期游戏帧时间、长时间内存压力、GC 压力、按键长按/丢包、并发写槽、LoD Lua 语法错误和战斗实机拒绝：`not_run`。
- 当前头和正文跨多次读取不是原子快照。读取期间业务变化的竞态仍需生产协议处理。
- 性能数值来自单机单次测量，不作为 SLA，也不与前一轮不同条件的旧扫描数值直接算加速比。

实验范围内的成功和上述未覆盖项必须一起保留；这些结果足够决定下一步设计方向，不足以宣布生产信道零错误率。

## 验证与收尾

`go build ./...`、`go vet ./...`、强制 Lua 5.1 的 `go test -count=1 ./...` 全部通过；独立 host 的 build-tag 构建与 vet 通过；迁移后的 33 项 fixture 断言通过。全量测试输出为 `go-test.txt`。

已执行 `/memlod stop` 清除临时绑定，两个内存 Lab 已 clear，并恢复 Lab2 原 TOC，下一次 reload 不再启动 inbox 控制器。四个实验 LoD 目录保留供重跑，不会自动加载。正式桥显示已由 `final-hide.json` 确认 `cleared=true`，保留 `final-clean-after.png`。证据文件哈希清单为 `manifest.json`。本轮未提交、发版或更换生产通信实现。

最终只读截图出现了后续新 run `6ab9639913913c38`，说明清理之后又发生了 Lua 运行期重建；原有两个内存 Lab 的 PLAYER_LOGIN 初始化仍保留，会再次生成实验数据。因此这里的 clear 仅指执行时已释放引用，不代表永久禁用 Lab。最终截图未见桥二维码，也未见 inbox 控制器启动输出；没有继续干预画面中的其他插件窗口。
