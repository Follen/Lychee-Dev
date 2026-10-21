# Interrupted Windows ordinary-channel recovery

For current runs, recover the recorded named Job without restarting Windows:

```powershell
node tools/rust-owned-recovery.mjs <repository> <interrupted-owner-UUID>
```

The recovery checks the recorded wrapper identity, reaps its named Job if it
still exists, and archives only that owner's channel lease. It preserves the
interrupted owner and logs; recovery does not turn an incomplete test into a
pass. Run the original command again through the normal owned runner using the
same target. A still-running wrapper must finish or be stopped before recovery.

The procedure below is retained for historical runs made before named Job
recovery existed. It is not the normal response to a test interruption.

This narrow tool fences a pre-restart lease so the existing ordinary channel and
existing `target` can be used again. It never claims the old command succeeded or
that its original handles were observed reaped. The old owner receipt, scratch,
request, logs and missing native receipt remain unchanged.

## Prepare before restarting

From the selected Windows repository, with the original ordinary lease still in
place:

```powershell
$repo = (Get-Location).Path
node tools/rust-channel-recovery.mjs prepare $repo 9f053173-8472-4860-be36-19a455a32e62
```

This reads the original lease, owner and unique process request, reads the actual
Windows working-uptime counter and a hash of the SMBIOS UUID, and creates a new
`prepared.json` under `.tmp/rust-disk/recovery/<owner>/<fence>/`. It preserves the
full original bytes and SHA256 of the three files, the native receipt's required
absence, the exact tool identities and the pre-restart counter. It returns the
new fence ID and preparation SHA256. Save both. No restart, process termination,
lease movement, target creation or old receipt modification occurs.

Preparation must finish before a user-authorized Windows Restart. A reboot said
to have happened earlier cannot replace this observation. Choose Restart after
saving other work; this tool never restarts the machine. Do not substitute sleep
or hibernation. No application or security setting needs to change.

## Apply after Windows returns

Use the exact values returned by prepare, in the same repository and on the same
computer, with unchanged recovery-tool bytes:

```powershell
node tools/rust-channel-recovery.mjs apply $repo <owner-UUID> <fence-UUID> <prepared-SHA256>
```

Apply promptly: current working uptime must be strictly less than the captured
pre-restart uptime. The result reports the saved threshold in working milliseconds.
If the current value is equal or greater, the tool conservatively refuses; it does
not infer that a restart did or did not occur. Keep the lease. A fresh preparation
and a subsequent authorized restart can establish a new comparison, preserving
all older preparations. Never edit a preparation or its hashes to get past this.

Apply revalidates the actual counter, host binding, preparation/tool identities,
exact original files and missing native receipt. It holds the three original
files against concurrent writing/deletion, writes and flushes a separate fence
record, then performs one no-replace rename of the held original lease into
`ordinary.lease.original` beside that record. This final namespace operation is
the release commit. Earlier errors leave the original lease in place. The existing
runner can then acquire `.tmp/rust-disk/channels/ordinary.lease` normally and use
`target`; there is no alternate channel or target.

The fence record remains `prepared`: commit is established by the exact archived
lease, not by a later status write that could be lost. The generation is its unique
fence ID. If the final response is lost, inspect before retrying:

```powershell
node tools/rust-channel-recovery.mjs inspect $repo <owner-UUID> <fence-UUID> <prepared-SHA256>
```

- `waiting_for_verified_restart`: preparation exists; original lease unchanged.
- `prepared_not_committed`: fence exists; original lease unchanged. After inspection,
  the same explicit apply may retry its no-replace commit while all checks still pass.
- `commit_observed`: archived original bytes and fence match. Never alter a newly
  present ordinary lease, which may now belong to a different invocation.
- Any mismatch, unknown path, missing evidence or read error stays blocked.

## Proof and limits

`QueryUnbiasedInterruptTime` is a public Windows API returning an unsigned
64-bit counter in 100-nanosecond units. Microsoft documents that it starts from
zero with the system, increases during working time, excludes sleep/hibernation,
and is unaffected by user/Windows time-service clock adjustments. A strict
decrease on the same host is therefore sufficient evidence of a new system
counter epoch; a non-decrease is deliberately inconclusive. The tool uses exact
decimal strings/UInt64, rejects values at or above 2^63, and reads the counter
around observation/commit. It does not use wall-clock subtraction or PID death.
SMBIOS UUID is retained only as a SHA256 host binding; unavailable and known
placeholder UUIDs are rejected. This is an ordinary local Windows reboot
recovery, not support for VM snapshot rollback, cloned disks or hostile kernel
counter modification.

The old process tree's termination chronology, output completeness and original
handle closure remain unknown. The restart fence proves only that pre-restart
execution cannot continue into this newly admitted channel generation. It does
not permit old scratch cleanup or invent the absent native process receipt.

## Verification boundary

Portable tests cover counter validation, original-byte binding, wrong owners,
foreign paths, appearing receipts, ambiguous requests, and read-only inspection
of simulated commit cutpoints. These are not Windows native execution. Actual
PowerShell/C# compilation, native file locking/no-replace rename, real reboot,
and the separate atomic Job creation change require Windows verification.

Official references:

- [QueryUnbiasedInterruptTime](https://learn.microsoft.com/en-us/windows/win32/api/realtimeapiset/nf-realtimeapiset-queryunbiasedinterrupttime)
- [SMBIOS UUID via Win32_ComputerSystemProduct](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-computersystemproduct)
- [SetFileInformationByHandle](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setfileinformationbyhandle)
- [FILE_RENAME_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info)
- [Restart and Fast Startup](https://learn.microsoft.com/en-us/troubleshoot/windows-client/setup-upgrade-and-drivers/fast-startup-causes-system-hibernation-shutdown-fail)
