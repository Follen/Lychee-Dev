# Rust 重构期间的磁盘与进程管理

日期：2026-10-08。范围仅为当前 `.worktrees/rust-refactor`；这是实施记录，不代表 Rust 全量重构或发行验收已经完成。

## 已实施规则

- 普通开发、测试和 release 共用项目 `target`；完整符号构建使用 `target-symbols`，最多两个受控通道。每个通道有独占 owner lease，遗留 lease 不按 PID 或时间自动夺取。
- 日常 dev/test 使用 debug=1，依赖 debug=0，保留增量；一次性构建关闭增量。`symbols` 使用完整 debug=2，保存同次构建的 EXE/PDB 及哈希记录。
- `release/rust-disk-policy.json` 的初始限制为缓存 20 GiB、受控 scratch 5 GiB、可用空间预留 2 GiB。调整通过显式配置或进程环境完成，不能静默绕过。
- 构建入口 `tools/rust-cargo.mjs` 支持 build/test/check/clippy；产物入口 `tools/rust-build.mjs` 支持封存与按哈希复用。命令、输入和环境身份、stdout/stderr 原始字节保存在独立 evidence 中。
- 只有明确终态、已回收进程句柄且 retention 为 rebuildable 的自有 scratch 才能清理。超时或不能证明进程已回收时保留 scratch 和 lease。
- 对应源码的正式门禁仍需两个独立 Cargo home、顺序执行的离线全新构建及产物比较；新增 runner 的模拟测试不能替代该正式构建证据。

## 本轮历史缓存回收

开始前核实各构建 owner 已暂停；宿主进程枚举未发现工作树中的活动 EXE 或 Cargo/Rust/native 测试进程。8 组 npm 测试分别已有成功或失败的终态证据；失败用例没有被提升为通过。

明确回收的目录为 `.tmp/` 下以下测试各自的 **compile-target 子目录**：

- `rust-npm-handoff-local-e2e-01` 至 `rust-npm-handoff-local-e2e-06`
- `rust-npm-handoff-prefix-fault-01`
- `rust-npm-rejected-next-successors-01`

另外回收了 `target/debug/incremental`，以及 `target/debug/deps`、`target/release/deps` 中可重建的 `.rlib`、`.rmeta`。全部 EXE/PDB 均保留。

合计删除 **44,490,506,971 字节（约 41.44 GiB）**逻辑文件内容。两次操作间，D 盘可用空间从 77,606,756,352 增至 120,526,548,992 字节，净增 **42,919,792,640 字节（约 39.97 GiB）**。可用空间净变化包含同盘其他进程的影响，不能当作精确物理释放量。

第一次操作在检测到短暂原生进程后停止，终态为 failed，已删除范围完整记账；其守卫没有保存该进程的 PID，因此不推断归属。再次宿主复核后，第二次操作完成剩余已审查的静态中间文件清理，并增加了阻止进程的身份记录。未终止任何外部进程。

清理后，82 个封存产物/日志哈希、8 份 npm 测试源码树哈希，以及 Live 47 任务 fixture 的匹配 EXE/PDB 均与清理前一致。

实际准入复核通过：受控缓存为 14,983,802,973 字节（约 13.95 GiB），受控 scratch 为 0，可用空间为 120,524,070,912 字节。历史 `.tmp` 的独立盘点为 15,459,933,923 字节，包含受保护的产物和证据，不能将其全算作可删除 scratch。报告见 `.tmp/rust-supervision/disk-admission-after-03.json`。后续构建只实时复核受控缓存/scratch 和空闲空间；完整 legacy 盘点使用显式 `inspect . --legacy`，未盘点时明确标记 `not_measured`。

明确保留：npm 包和源码、失败日志、恢复 journal、Source AV 调查包及匹配符号/私有 dump、全部 `.tmp/live-static` 捕获和调查资料、固定 LuaLS/luac 产物、其他未审查的 scratch。主 checkout 和它的 `.tmp` 未改动。

## 验证与证据

- 主代理独立运行磁盘与构建入口测试：**10/10 通过**，包括通道冲突、配额拒绝、超时保留、封存日志被修改时拒绝删除、哈希复用及独立 fresh Cargo home 生命周期。
- 后续补入真正的 Windows Job 整树回收，主代理独立运行 **5/5 真实进程用例通过**：二进制输出及精确参数、父进程先退出而有限子进程正常结束、真正的常驻后代、超时以及无限输出上限。仅自有 Job 可以被终止，清理要求内核活动进程为 0、root 句柄已等到退出、输出读取结束且句柄关闭。日志为 `.tmp/rust-supervision/owned-process-independent-01.log`。
- 实际 Cargo 场景曾被旧版整树判定标为强制终止（`f1854791-ada4-49d5-8ac9-b32b6c3c67f1`）。修正有界排空和截止点内核计数复核后，同命令、同源码哈希重放通过，`d9ae01c7-306e-416f-9cd3-a3ac604c37cc` 记录 exit=0、forced=false、treeReaped/handlesClosed/outputReadersComplete=true；该重放只验证工具修复，不增加业务覆盖。
- 之后的另一次实际链接发现 MSVC `VCTIP.EXE` 仍留在自有 Job，30 秒排空后被回收；其业务测试虽通过，整体 runner 仍失败。记录 `8d55b129-517c-4170-b537-64abca5106cf` 包含真实残留 PID/image。这是实际残留，不能当作旧竞态或放宽所有子进程规则；编译器辅助进程策略仍需处理和复验。
- 首次子代理在系统 TEMP 中遇到 rename EPERM；改用本工作树 TEMP/TMP 后通过。环境失败和成功结果分别保留，不以成功覆盖失败事实。
- 原始清理报告、完整文件清单与哈希见 `.tmp/rust-supervision/cache-reclaim-02.json`、`cache-reclaim-03.json`。
- 额外保全校验见 `.tmp/rust-supervision/cache-preservation-after-03.json`；独立测试日志见 `.tmp/rust-supervision/disk-runner-independent-03.log`。
- npm 包装 runner 的全生命周期接入仍在收尾；最新对应源码的两次真实离线重建、候选发行物及全量业务回归仍未完成。
