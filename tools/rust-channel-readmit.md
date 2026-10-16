# 本次用户停止授权后的 ordinary 再准入

这是针对 PR #14 单次中断的独立入口：owner
`9f053173-8472-4860-be36-19a455a32e62`、request
`11299fbf-7a09-4dc8-ba5d-e440145136f6`、原 boot preparation fence
`cc31783a-9ed6-47a8-a150-e656b88bd90e`。用户已要求停止本批后重跑，且不方便重启。
它只建立当前资源空闲下的新 generation；旧执行结果、匿名 Job、输出完成及历史回收继续 **unknown**。
没有 kill、重启、系统配置变更、旧 receipt 改写或新 Cargo target。

原 `rust-channel-recovery.{mjs,ps1,test.mjs,md}` 和 `prepared.json` 不修改。
旧 boot apply 仍严格要求实际 counter 下降；新模式不是旧 preparation 的既有授权。
新入口只借用其原字节快照和经过原工具 SHA 验证的 LockedFile/no-replace rename。

## 本机执行

先在保存这四个新增文件的精确 commit 上运行：

```powershell
node --test tools/rust-channel-recovery.test.mjs tools/rust-channel-readmit.test.mjs
```

Windows native fixture 只使用自己新建的 fixture 文件，验证实际 C# 编译、Restart Manager、
精确自身创建身份、文件监听、锁持续持有及 no-replace 提交。它不操作真实 lease 或跑 Cargo。
任何失败保留该 fixture 证据，先修复，不执行正式 apply。

随后使用**此前保存的原 prepared SHA256**，在原 repo 执行：

```powershell
node tools/rust-channel-readmit.mjs apply '<repo绝对路径>' '<原prepared SHA256>' --user-authorized-stop
```

这一步会改变原 lease 的目录位置，必须在本次已明确授权范围内执行。
不需要重启。它不会把前两次已释放的诊断句柄当作当前互斥证据。

每次尝试先输出新的 generation UUID，并创建独立：
`.tmp/rust-disk/recovery/<旧owner>/<原boot fence>/readmission/<generation>/intent.json`。
新 attempt 永不覆盖旧 attempt。请保留 generation 与输出。

只有 `readmission_commit_observed` 表示精确原 lease 已归档；随后可用原入口、原 target：

```powershell
node tools/rust-cargo.mjs --owner pr14-post-stop-check --one-shot -- check --locked --offline -p lycheedev-data --lib -j 1
```

该 Cargo 命令的真实执行结果必须另记，恢复成功不代表编译或测试通过。

## 同一次临界区内的条件

- 校验原 preparation SHA、同机 SMBIOS UUID 哈希、固定 repo/owner/token/request 和三原文件完整字节。
  新 native receipt 或多 request 出现即拒绝。
- 持有原 lease 的原生可重命名只读句柄，以及 owner/request 的只读句柄；它们禁止并发写入和删除。
  另以 FileShare.None 持有本机已报告的十二个资源，共十五个文件。
- 十二资源是 debug/release 各三 Cargo 协调锁、固定 Data EXE/PDB、debug CLI EXE/PDB、
  `target/.rustc_info.json`、`Cargo.lock`。Data 对实时重算原固定 bytes/SHA；CLI 对只用于资源互斥。
- 实际 Win32_Process 与 Restart Manager 检查均须无匹配活动对象。仅排除本次 PowerShell 自身和其
  直接 Node coordinator，使用仍持有的 Process 对象、PID 和精确创建 FILETIME 共同核对。
  不笼统排除 PowerShell，也不依据旧 PID 消失。
- target 全子树监听不少于十秒，前后路径/长度/LastWriteTicks 元数据摘要相等，零事件、零错误；
  检查权限失败、监听溢出、资源缺失、Data 身份变化或任何锁冲突均拒绝。
- 将独立 admission record 写通并 Flush 后，再查当前进程/资源和 watcher，最后通过仍持有的原 lease
  句柄执行唯一 no-replace rename。所有原文件与资源锁持续持有到这个提交点之后。

这是本次受控 Cargo/test 场景的当前资源再准入，不是对任意恶意、隐蔽、绕过协调的未来写入的证明。
Cargo/rustc/cl/link/vctip 等不携带可判定工作目录的活动进程会保守阻断；不会自动杀其它项目。
这套一次性观察不加入每个普通 Cargo 步骤。

## 失败、重试与提交事实

所有 pre-commit 失败保留原 lease、owner、request、scratch 和原 boot preparation。
`admission.json` 的 prepared 状态本身**不表示已解锁**；唯一提交事实是精确原 lease 出现在该 attempt 的
`ordinary.lease.original`。提交后输出丢失或关闭句柄报错也不能伪装回滚。所有显式关闭都会逐项尝试；原始错误和关闭错误分别保留在 wrapper 诊断中。

```powershell
node tools/rust-channel-readmit.mjs inspect '<repo绝对路径>' '<原prepared SHA256>' '<generation UUID>'
```

- `attempt_not_committed` / `prepared_not_committed`：原 lease 字节仍匹配。可新开一次 apply，重新获取所有锁并观察；旧 attempt 留存。
- `readmission_commit_observed`：归档字节与独立记录匹配。若 ordinary 已有新 lease，只报告 `present_do_not_modify`。
- 任何不一致：停止并检查；不手工删除/替换 lease、不修改旧 prepared 或 native receipt。

## 来源与验证边界

- [本机第一轮当前资源观察](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6216084938)
- [明确停止授权后的第二轮，实际终止 0 个](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6216190221)
- [精确十五资源及 Data bytes/SHA](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6216305777)
- [Windows 文件共享与持柄生命周期](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)
- [Restart Manager 当前资源占用列表](https://learn.microsoft.com/en-us/windows/win32/api/restartmanager/nf-restartmanager-rmgetlist)
- [Cargo 1.99 协调锁布局](https://doc.rust-lang.org/stable/nightly-rustc/src/cargo/compiler/layout.rs.html)

云端 Node model/static 测试不等于真实 Windows API 成功。正式 prepare 历史事实保留；
本新增入口的 native fixture、正式 apply 与后续 Cargo 结果以本机实测为准，未跑前均 NOT_RUN。
