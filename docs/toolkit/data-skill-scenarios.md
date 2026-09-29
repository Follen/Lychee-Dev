# Data skill scenario acceptance

These cases evaluate the data branch, not DB2 format coverage. The entry is
`skills/lycheedev/references/data-investigation.md`. Read its conditional references
only when the scenario needs them. Do not load this acceptance file as instructions
when evaluating an agent; give it the prompt and fixture context instead.

## Cases

| ID | Prompt / supplied context | Required observable decision | Failure |
| --- | --- | --- | --- |
| DS-01 | “查 Forever 的 Claw 系数。” Project has matching us/enUS fixed pin; name search contains duplicates/ranks. | Reuse pin; schema → bounded name search → related records; distinguish candidates, report static scope. | Ask user for an ID the tool can discover; pick first name; claim server coefficient. |
| DS-02 | Same request, project pin is Retail; a verified Forever pin is available. | Use explicit matching Forever pin; leave project lock unchanged. | Query Retail or overwrite the lock silently. |
| DS-03 | “查 Forever 技能数据。” No pin, region/locale unspecified and no established project context. | Resolve only remaining identity ambiguity; canonical Forever selection through resolver. | Assume CN/zhCN from Chinese conversation or infer build from folder name. |
| DS-04 | `target list` is empty; availability includes product error and truncated rows. | Treat list as saved names, inspect advertised releases/errors and truncation. | Say Forever unsupported or call a mixed-success response complete. |
| DS-05 | Name search returns one row, `truncated=true`, no cursor. | Narrow deliberately or use bounded ordered SQL; preserve same-name candidates and fixed source. | Invent a search cursor, treat row as unique, or call narrowed results a full substring scan. |
| DS-06 | Empty name/SQL result with missing encrypted partitions. | Report no readable match and incomplete coverage; retain missing sections. | “This spell does not exist” or replace unavailable values with zero. |
| DS-07 | Local client shares parent Data; one localized table unavailable; numeric effects readable. | Classify exact failure, use numeric evidence, keep unavailable name scope explicit. | Declare installation broken from folder shape; assume mixed indices; bypass integrity. |
| DS-08 | Local and remote pins have same build/definitions but different CDNConfig. | Keep original project lock; label separate pin/source if comparing. | Hand-edit pin/config, guess a CDN URL, claim identical provenance. |
| DS-09 | `--offline` lacks content; another case repeatedly hits deadline after one justified extension. | Honor offline restriction; stop failed branch after bounded recovery, retain failure/captures. | Remove offline implicitly, loop unchanged retries, or escalate timeouts indefinitely. |
| DS-10 | Hotfix scan returns checkpoint; original DBCache changes before continuation. | Resume immutable source/checkpoint with same filters; read page captures. | Restart from changed file or report summary as decoded records. |
| DS-11 | “先查静态数据，别进游戏。” / “再验证服务端修正。” | First prompt stays static; second can route into authorized live with static baseline preserved. | Game input for static-only task; static completion claimed as server validation. |
| DS-12 | Installed CLI rejects a flag mentioned by source skill. | Inspect installed describe/help; report capability mismatch and useful supported route. | Pretend source changes are installed or invent replacement syntax. |

## Executable checks

The following tests run real command/module paths against isolated fixtures.
They verify the mechanics required by the decisions; they do **not** demonstrate
that an independent agent will choose those decisions.

| Cases | Test / evidence |
| --- | --- |
| DS-01, DS-05 | `internal/command: TestDataNameDiscoveryWorkflow`: schema, truncated search, exact-name SQL keyset pages retaining both IDs, separate effect values, complete empty lookup; each command returns same pin and captures. Fixtures are synthetic, not Forever coefficient facts. |
| DS-02 | `internal/command: TestProjectDiscoveryAndExplicitSnapshotPrecedence` verifies explicit pin precedence; leaving an unrelated lock untouched is also a workflow review obligation. |
| DS-04 | `internal/selection: TestAvailabilityRejectsReusedSlotAndExposesTruncation`, `TestAvailabilityForeverRequestsPublishingSlot`. |
| DS-06 | `internal/records/navigatetest: TestSQLIdentityLookupPreservesPartialCoverage` includes unavailable and absent IDs; neither becomes complete. |
| DS-07, DS-08 | `internal/records: TestContentRecoveryAcrossEncodings`, `TestCDNRefreshStaleRouteOnceWithoutRetargeting`, `TestCDNRefreshDuringConfigurationPreparation`; real Forever local/CDN evidence is recorded in `forever-data-recovery-2026-09-29.md` and `data-hardening-2026-09-29.md`. |
| DS-09 | `internal/command: TestDataTimeoutStopsWholeCommandAndReleasesResources`; offline route tests and `internal/records: TestCDNDeadlineStopsRecovery`. Agent retry-count choice requires behavioral evaluation. |
| DS-10 | `internal/command: TestHotfixFullScanResumeKeepsCaptureAndFilters`: source mutation, changed-filter rejection, stable checkpoint replay; `internal/records: TestHotfixScanCancellationReturnsSavedPage`. |
| DS-12 | Skill contract and generated command checks verify source compatibility only; installed-version recognition remains a behavioral decision. |

Run `go test -count=1 ./internal/command ./internal/selection ./internal/records/...`
and `node tools/skill-contract.mjs`; the full repository regression also includes
these cases. Use `node tools/skill-commands.mjs --check` for generated reference drift.

## Evaluation boundary

2026-09-29: all 12 cases received author walkthrough against the edited references.
That review found and corrected an unsupported continuation assumption: DB2
search/foreign-key do not accept a paging cursor. The new executable workflow
checks the supported ordered SQL alternative without silently collapsing duplicate
names. The detail references retain source and coverage rules while the entry is
reduced from roughly 410 lines to roughly 80.

Independent-agent scenario execution: **not_run** (no delegation in this task).
Author walkthrough is not a measured agent pass rate. A future behavioral run
must retain the command trace, chosen pin, any project mutations, final coverage
claims and stopping decision for each case; a wrong identity, unsupported command,
unauthorized live action or false-completeness claim fails that case.

This is a source-skill change. No npm publication, installed-skill replacement or
game input is part of this acceptance run.

The first full Go/Lua regression exposed a timing assumption in
`TestCDNDeadlineStopsRecovery`: a 30 ms deadline could expire during local
preparation, before its required first HTTP call. The test now injects a transport
deadline deterministically and checks that it is not retried; actual command
deadline/resource release remains covered by the command budget test. The revised
transport test passed 20 repetitions. Initial full log: `.tmp/data-skill-full.log`.

Final verification: `go build ./...`, `go vet ./...`, and full
`LYCHEEDEV_REQUIRE_LUA51=1 go test -p 4 -count=1 ./...` passed. The rerun limits
package concurrency, not test scope; log: `.tmp/data-skill-full-rerun.log`.
Skill validation, generated commands check, 87-command/216-reference contract
check and both contract-tool tests passed. Environment-gated integration checks
retain their existing skips; this is not a new live-client or independent-agent run.
