# AGENTS.md

## Project baseline

Lychee Dev Toolkit is a Go CLI, a Lua workbench addon, an Agent skill and an npm distribution. The current development baseline is 3.2.0; release/version.json remains the version authority. Windows amd64 is the shipped platform.

Read these current documents:

1. Design.md — architecture, module boundaries, products and contracts.
2. docs/live-protocol.md — Lycheedev mailbox protocol v1.
3. todo.md — incomplete work and honest real-client coverage.
4. docs/development.md — build, tests, workspace hygiene and release.
5. Changelog.md — changes in this development baseline.

Do not recreate historical design ledgers or use Git history as current acceptance.

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
- WGC supplies game-window screenshots. Offline fixtures do not prove game behavior. Keep the current product matrix honest in todo.md.

## Validation and delivery

Run go build ./... and go vet ./... after code edit batches, followed by affected tests. The full Lua-required Go suite must pass before presenting a code commit as done; use bounded package/test concurrency and the documented installation-test timeout. Missing required interpreters are failures, not passes.

Check tools/version.mjs --check and the Skill contract when their inputs change. Do not repeat successful checks unless a new change, actual failure or unresolved issue requires it.

Do not change npm tags while merging code or cleaning docs. Release through the pinned workflow and channel manifest; preserve latest at 2.5.1 unless the user explicitly changes that policy. Published artifacts are immutable.

Temporary work belongs in ignored .tmp. Keep reusable tests in tests/ and current documentation concise. Do not delete .lycheedev, another task's worktree, or another task's changes as repository hygiene.