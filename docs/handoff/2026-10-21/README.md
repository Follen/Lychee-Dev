# Developer handoff, 2026-10-21

Start with [HANDOFF.md](HANDOFF.md), then [VALIDATION-QUEUE.md](VALIDATION-QUEUE.md). The product baseline is cfdefd46a11ec8dce470d296b424a3eaa1372b8b. This handoff does not change product code or claim business/performance acceptance.

- `SOURCE-DECLARED-TARGETS.json` and `ORACLE-FEATURE-TARGETS.json` are source inventories, not test results.
- [Maintenance instructions](maintenance/README.md) describe a narrowly scoped, explicit-worktree cache operation. Publication does not authorize deletion. Preserve all evidence and independently revalidate the operation's prerequisites.
- `SHA256SUMS` binds the eight content files in this directory. Verify it before using the scripts.

The scripts now take `--worktree`; they intentionally differ from the earlier private handoff archive. No private machine path, conversation identifier, raw validation log, or credential is included here. Code is already in this repository; no source ZIP is needed.
