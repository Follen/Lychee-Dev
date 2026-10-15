# AGENTS.md

## Project baseline

Lychee Dev Toolkit is a Go CLI, a Lua workbench addon, an Agent skill and an npm distribution. The current development baseline is 3.2.3; release/version.json remains the version authority. Windows amd64 is the shipped platform.

Read these current documents:

1. Design.md — architecture, module boundaries, products and contracts.
2. docs/live-protocol.md — Lycheedev mailbox protocol v1.
3. todo.md — incomplete work and honest real-client coverage.
4. docs/development.md — build, tests, workspace hygiene and release.
5. Changelog.md — changes in this development baseline.

Do not recreate historical design ledgers or use Git history as current acceptance.

## Multi-agent handover

- Read [handover.md](handover.md) before starting or taking over repository work. It is the shared current task state; the architecture and contracts remain in the documents listed above.
- Keep one concise, current snapshot: branch/PR, completed changes, open issue IDs, task owners and file boundaries, validation actually performed, blockers, and the next concrete action. Update it at assignment changes, completed implementation/validation, and handoff; replace stale entries instead of appending a run diary.
- The coordinating agent maintains the shared handover. Workers report their owned changes and evidence to the coordinator; avoid concurrent edits to this file and coordinate any overlapping product files before editing them.
- Preserve the distinction between confirmed defects, candidates, implemented fixes, offline verification, real-client verification, and published work. Link detailed findings instead of copying the full audit into the handover.
- Handover records context, not additional authorization or approval gates. Follow the current user's scope and existing repository rules; do not infer permission to merge, publish, operate a game, or discard another agent's changes from a handover entry.
- Store reusable tests with the code and completed-task temporary material under the existing cleanup policy. Do not put secrets, full logs, machine-local artifacts, or unverified success claims in handover.md.

## Working rules

Follow the user's task scope. Reuse the current branch/worktree; do not create repositories, worktrees or PRs just to satisfy a workflow. Preserve unrelated edits. Do not commit or push without task authorization.

Use a focused change and verify its actual risks. Existing contracts belong at their boundary; do not add redundant guards, fallback frameworks or new approval gates. Keep logs only when needed for a current failure or requested result.

## Code and contracts

- internal/command/command_contract.go is the only command directory. Parser, help, describe and generated Skill references derive from it.
- Query results use lycheedev.result.v1. JSONL requires a valid end frame and exit 0. Preserve explicit truncation and partial coverage.
- Exit codes: 0 complete, 2 arguments/selection, 3 capability/environment, 4 invalid input/protocol, 5 external/business failure, 6 pending, 7 cancelled, 8 internal.
- Source/Data/Changes use fixed references. A folder is a location, not product identity. The two Forever beta products share the forever source family but retain independent Data identities.
- Use pinned API facts and current code. Client differences belong in addon/Core/Compat.lua, ClientGate or generated catalogs.
- Never use retired wowdoc/wowdata/Python entrypoints or import legacy user data.

## Addon and Live

- Disabled features create no frames, hooks, events, timers or OnUpdate. The minimal bootstrap and enabled-only INPUT/EditBox handling follow the explicit protocol exceptions.
- Protected/forbidden UI, combat and secret values are trust boundaries. Chinese and English locale changes stay paired.
- Native Live uses 500 LoD slots, runtime instruction/RIP Lua-root discovery and named Mailbox reads. No private-heap scan or cached address authorizes input.
- The two formal bindings default to Ctrl+Alt+F12 and Ctrl+Alt+F11. Read their actual effective assignments from fresh INPUT. Native input sends zero Esc and no chat text.
- Bind a fixed installation/PID/creation time/HWND, then GUID/runtime/owner/fence. Shared-installation tests must keep each process identity separate.
- Use clean managed installation through delivery; do not overlay-copy source into managed game files.
- Persist input intent before sending. Recover the original project/CON/request and deadline. Submitted or uncertain input never licenses replay.
- Read-only doctor diagnoses; doctor --fix uses managed installation and original connection recovery. Without compatible fresh INPUT, first activation needs one manual reload.
- A verified report and pending cleanup are separate facts. Complete cleanup without discarding the useful report.
- Offline fixtures do not prove game behavior. Keep the current product matrix honest in todo.md.

## Validation and delivery

Run go build ./... and go vet ./... after code edit batches, followed by affected tests. The full Lua-required Go suite must pass before presenting a code commit as done; use bounded package/test concurrency and the documented installation-test timeout. Missing required interpreters are failures, not passes.

Check tools/version.mjs --check and the Skill contract when their inputs change. Do not repeat successful checks unless a new change, actual failure or unresolved issue requires it.

Do not change npm tags while merging code or cleaning docs. Release through the pinned workflow and channel manifest; the user authorized 3.2.3 on latest. Future channel changes require explicit user authorization. Published artifacts are immutable.

Temporary work belongs in ignored .tmp/<task>/ and is removed when the task ends. Keep reusable tests in tests/ and current documentation concise; do not create permanent run diaries or archive copies for routine work.

Before cleanup, distinguish active .lycheedev projects from completed experiments and inspect worktree changes and ignored files. Remove only targets covered by the user's cleanup scope, using checked absolute paths and git worktree remove where applicable. Preserve explicitly retained worktrees; .worktrees/rust-refactor is currently retained. Do not discard unrelated edits or unresolved recovery records.

For documentation-only hygiene, check links, referenced paths and affected packaging contracts. Do not rerun full Go or real-client suites when no executable behavior changed.
