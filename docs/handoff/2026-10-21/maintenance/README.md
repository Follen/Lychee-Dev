# Explicit-worktree incremental retirement

These public tools are derived from the independently source-reviewed one-off maintenance implementation. This version replaces a private fixed host path with a required absolute `--worktree` parameter. It does not authorize deletion, change the 20 GiB policy, or become a generic cache sweeper. New byte hashes are in `../SHA256SUMS`; earlier private archive hashes do not describe these files.

## Preconditions

Obtain explicit authorization for the selected worktree's `target/debug/incremental` directory. Pause all test/build scheduling and consumers until completion. Independently confirm the selected path is the isolated worktree from the verified inventory. Never infer ownership from an old PID, age, or an empty lease directory.

The maintenance checkout must be exactly cfdefd46a11ec8dce470d296b424a3eaa1372b8b, with unchanged published rust-disk.mjs and rust-build.mjs. It must still have 6,819,246,069 bytes in the specified incremental directory, the verified 112 sealed manifests / 2135 referenced files, and the original six quota-failure files. A docs-only newer HEAD is still a different identity and is rejected. Do not reset a worktree or modify these constraints to force admission. Stop and re-review changed circumstances.

## Prepare evidence without deletion

`node <handoff-directory>/maintenance/seal-blocked-retirement-input.mjs --worktree <confirmed-absolute-worktree>`

This verifies existing manifest and seal hashes, resolves only a unique existing root-relative or manifest-relative file, validates the actual failed terminal/launch/log facts, and saves NEW blocked-stage/batch documents and `retirement-input.json` outside all caches. It preserves all originals and does not create fake final artifacts in the historical inventory root. Unsupported layout or ambiguity stops execution; do not guess paths. The input root is the selected realpath.

Windows PowerShell writes the terminal and launch JSON as UTF-8 with a BOM, and redirected stderr as UTF-16LE with a BOM. The reader decodes these known outputs in memory; hashes and protected inventories continue to cover every original byte. The initial Windows attempts rejected these encodings before creating any blocked receipt or attempting cache retirement.

## Retire only after authorization

`node <handoff-directory>/maintenance/retire-authorized-incremental.mjs --worktree <same-confirmed-absolute-worktree> --execute-authorized <returned-input-path>`

The input root must match. The script obtains both published ordinary/symbols leases, refuses active producers/consumers, checks native Windows reparse attributes, then preserves an immutable preparation receipt before deleting any file. Only inventoried files under `target/debug/incremental` are removed. Every other target/target-symbols file, all supplied evidence and input bytes are checked before/after. deps, EXE/PDB, source, logs and unknown paths are not cleanup candidates.

Both lease releases are attempted. Success is reported only after release and a durable final receipt. Caught failures attempt an append-only stopped receipt. Hard interruption or disk-write failure can leave only prepared evidence: outcome is uncertain; reconcile read-only before any retry. There is no automatic stale-lease deletion.

## Afterward

Preserve all receipts locally. Run the existing read-only `node <same-worktree>/tools/rust-disk.mjs inspect <same-worktree>` and independently verify no processes/leases. Its zero-reservation inspection is not the next Cargo admission: the unchanged runner still reserves 2 GiB. Resume correctness groups only after reviewing the maintenance result. Do not increase limits, remove deps or symbols, or enable one-shot.

These are cooperative quiescent-worktree safeguards, not a native handle-based defense against hostile concurrent reparse replacement. Windows execution was NOT_RUN when the original tools were published. After the PowerShell decoding correction, the fixed cfdef worktree completed blocked-evidence sealing (112 manifests / 2135 references) and retirement of exactly 6,819,246,069 incremental bytes. All 2534 protected file entries and 8663 retained cache files matched before and after; both leases were released and the final receipt reported `retired`. Post-maintenance inspection found 12,899,139,776 cache bytes, no related processes or leases, and the original 20 GiB policy. These maintenance results do not claim any product test execution.
