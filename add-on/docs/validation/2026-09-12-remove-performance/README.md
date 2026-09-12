# Remove the performance tools

Date: 2026-09-12. Baseline: `0107369`. Implementation branch: `codex/remove-performance`, isolated at `D:/Code/wow/worktrees/lychee-dev-performance`.

This is the historical removal/restructure record for commit `49ffe54`. The skill was subsequently renamed from `wowdev` to `lychee-dev`; see the [rename record](../2026-09-12-skill-rename.md). Names, installation paths and hashes below describe the earlier verification.

## Repository layout

Root README files, AGENTS.md, .gitignore and Git metadata remain at repository level. `add-on/` owns runtime files plus tests/tools/docs; `Lychee Dev skill/` owns the `wowdev` skill. The original checkout and its ignored data were not moved. There was no Analyze/publish directory in this worktree before restructuring.

`add-on/tests/TestAll.ps1` anchors its working directory to the addon and restores the caller's directory/test-client environment. Event generation and compatibility audit already resolve runtime paths from their script directory. `add-on/tools/Package.ps1` packages only TOCs, referenced Lua and media beneath `Lychee Dev/`; packaging tests verify all TOC dependencies and exclude development/skill/investigation files.

## Product result

Removed the Performance page and navigation, health scan, capture sampler, object/function/storage capture analysis, measured function experiment, capture dock, profiling/reload switch, unused compatibility helpers, performance-only locale keys and tests, and the old performance screenshot. All three TOCs load the remaining shared implementation.

Run, object inspection, event monitoring, function tracing, error diagnosis, shared serialization, history and Ticket export remain. No database schema or pruning policy changed. Legacy `performance_*` evidence is still readable and labeled as historical evidence. Removing the tools does not rewrite a user's global `scriptProfile` setting.

The replacement workflow is task-specific: wowdev prepares one bounded investigation for Run, the user executes it, and the Agent reads the complete Ticket evidence. No replacement background monitor was added. See [Runtime investigations](../../RuntimeInvestigations.md).

## Verification

- `powershell -NoProfile -ExecutionPolicy Bypass -File add-on/tests/TestAll.ps1`: passed all 21 invocations (six suites per client, build matrix, static audit contract, packaging).
- The tests cover seven remaining pages, absence of performance navigation/runtime, Run result export with input metadata, legacy performance record migration/reinitialization and saved-record selection, and the existing object/event/trace/error workflows.
- `powershell -NoProfile -ExecutionPolicy Bypass -File add-on/tools/AuditCompatibility.ps1`: passed for Retail `120100`, Classic `50504`, Titan `38002`; 23 Lua files checked per client, zero errors/warnings. [Full audit](compatibility-audit.json).
- The audit now passes the already configured immutable commit to wowdoc. During verification, the version tags resolved to different commits than the project baselines; validating by tag would not establish the stated baseline.
- Event catalog generation from the relocated tool matched Retail, Classic and Titan catalogs after normalizing line endings and final blank lines. The generator emits an extra final blank line; event content is unchanged.
- Packaging tests validated 32 runtime files with all three TOCs directly under `Lychee Dev/`.
- `git diff --check`: passed. Runtime reference review found performance identifiers only in historical export labels. The TOC tests reject retained profiler/memory-refresh/settings calls.
- Both installed wowdev copies passed `skill-creator/scripts/quick_validate.py`; local reference links and installed copy hashes were checked. This validates packaging and instruction consistency, not execution of a newly generated game probe.

No code was deployed or copied into the running game. No game input, clipboard or SavedVariables was changed. In-game layout, screenshots, combat transitions and login/reload behavior have not been verified on the modified build. The baseline contains prior UI screenshots; a genuine after screenshot requires a later authorized deployment and game session. Offline UI tests are not pixel or taint verification.

## Skill installation and change tracking

The two installed `wowdev` directories were independent copies, not symlinks or hardlinks, with identical initial `SKILL.md` and `agents/openai.yaml`. Neither was under Git. The initial `SKILL.md` SHA-256 was `23112fd2b5f8b1400f89a630eb72086973f976ecb204c3fc3d77b9d577def944`.

The repository now owns the versioned source at [Lychee Dev skill](<../../../../Lychee Dev skill/SKILL.md>). The same three files are synchronized into `C:/Users/follen/.agents/skills/wowdev` and `C:/Users/follen/.codex/skills/wowdev`; neither installation was deleted or relinked. The original two files are backed up at `C:/Users/follen/.codex/backups/wowdev-2026-09-12-remove-performance`.

- `SKILL.md`: Run-based investigations, record-level evidence validation, historical performance records, asynchronous completion, single-paste delivery and task ownership.
- `references/runtime-investigations.md`: measurement semantics, scope matrix, observer cost, safe isolation/restoration, validation before delivery, honest coverage and decision trade-offs.
- `agents/openai.yaml`: discovery text and default prompt reflect the Run/evidence workflow; no invocation policy change.

Git tracks the complete skill source alongside the addon. [SHA-256 manifest](wowdev-hashes.json) identifies the final files shared by both installations.

Instruction review used these cases: a heap delta mistaken for addon retention; combining offline SDK timing with live acquisition; a one-paste request with a missing integration; a late callback after cancellation; and a Ticket sent directly to the owning task. The instructions require explicit scope/status, restoration checks and full payload reading in each case. This was a local instruction walkthrough, not an independent agent or real-client execution test.

## Evidence used to improve the diagnostic workflow

Read-only local references were Lychee's `PERFORMANCE.md`, `docs/architecture/2026-09-12-runtime-lifecycle.md`, and `docs/validation/2026-09-12-search-rebuild/README.md` plus `acquisition-ticket-0010.txt`. Their live acquisition data, hot-state retained memory and offline rebuild measurements have different scopes. Pending SDK/index game probes and lifecycle proposals were not promoted to completed verification. The skill keeps the reusable distinctions, not this project's provisional numeric targets or plan-only authorization.

EllesmereUI's [contribution criteria](https://github.com/EllesmereGaming/EllesmereUI/blob/8da5dfe182c7809e3f61b5a9ca856d16b891726f/.github/CONTRIBUTING.md) and [ticker implementation](https://github.com/EllesmereGaming/EllesmereUI/blob/8da5dfe182c7809e3f61b5a9ca856d16b891726f/EllesmereUI_Ticker.lua) were inspected at the fixed commit cited by Lychee. Their opt-in/idle-cost and driver-ownership guidance informed the reference; no claim was made that upstream attribution comments prove every client's behavior.

## Immediate paste hang: read-only investigation

The user clarified that the client became unresponsive immediately on paste, before clicking Run; a crash exit was not confirmed. Read-only inspection of the supplied study found 468,817 bytes, 9,237 lines and a 334-byte longest line. The study was not executed.

`UI/MainWindow.lua` creates an uncapped native multiline EditBox. Its editable TextChanged path calls the addon scroll helper, which reads native geometry and synchronizes the scrollbar. Cursor updates have an existing next-frame queue. There is no Lua per-character parse/coloring/line-number pass, compile or history save in this input path. This identifies an unbounded input surface and possible native layout cost, but does not prove a native stall or a particular API as the cause. No native reproduction or successful input-scale limit was established, and no input-editor fix is claimed.

`RunInput` calls Execute and then AddHistory only after explicit activation. Paste alone is not saved; reopening the same window in the same process retains the edit box, whereas no automatic execution or persisted giant-input restoration path was found for reload/login. The 12,000-byte history-code limit is not a validated paste budget.

The user chose a separate diagnostic addon for that source-heavy study, owned by the Lychee task. This task neither implements nor validates that carrier. The skill now distinguishes compact Run probes from large harness delivery and requires input-path/scale validation in addition to syntax and isolation tests. No game input, clipboard replacement or saved-data clearing occurred here.
