# Source skill scenario acceptance

Entry: `skills/lycheedev/references/source-research.md`. Ordinary lookup uses the
entry alone; preparation, relationships and value flow have conditional references.
These scenarios check decisions, not instruction wording. Give a future evaluator
the prompt and fixture context without this expected-outcome table.

| ID | Prompt / context | Required observable decision | Failure |
| --- | --- | --- | --- |
| SS-01 | “这个固定版本的 GetRestrictedInfo 有哪些约束？” Matching pin exists. | Precise API query, inspect original declaration, report exact commit and constraints; stop when sufficient. | Unnecessary full relation/flow investigation; runtime behavior asserted from a declaration. |
| SS-02 | “查 WeakAuras 指定版本在 Retail 下怎么处理事件。” | Discover repository track from catalog; resolve requested release exactly; keep Retail API environment as a separate pin. | `--product retail` blindly applied to WeakAuras; current branch substituted for named release. |
| SS-03 | Requested tag cannot be resolved; source list does not show tags. | Use repository evidence for version/ref mapping; ask only if unresolved; retain exact request on failure. | Claim tag absent from branch catalog; guess tag or choose latest. |
| SS-04 | Same symbol name in different files/scopes; request asks who calls one definition. | Inspect candidate locations, retain chosen symbol ID, distinguish confirmed/static/unknown edges. | Merge by spelling, select first arbitrarily, or treat query relation preview as all callers. |
| SS-05 | Truncated query with nextCursor; caller later changes query or switches to refs. | Continue original query unchanged or begin a distinct refs investigation; never reuse its cursor across scope/command. | Mixed pages reported as one complete result. |
| SS-06 | LuaLS unavailable or client environment missing; original source readable. | Continue useful source inspection with partial semantic coverage; report missing prerequisite. | Mark relationships complete, abandon readable evidence, or install a random analyzer. |
| SS-07 | One-line excerpt; capture archives whole file. Mapping has a separate parse diagnostic. | Keep line-range evidence distinct from whole-file capture and mapping completeness; unparsed file remains a gap. | Complete capture used to claim whole-file review or no missing symbols. |
| SS-08 | Two fixed revisions differ only by declaration position; diff truncated. | Retain both commits, category totals and omitted scope; inspect relevant source; no diff cursor. | Infer API break from line move or fabricate a continuation flag. |
| SS-09 | SecretArguments is AllowedWhenUntainted; a guard precedes reassignment. | Preserve conditional rule and value identity, check consumer and reassignment, distinguish secret propagation from secure taint. | Conditional permission called unconditional safety; Blizzard-only stack blamed as proven origin. |
| SS-10 | Exact source preparation fails repeatedly / installed CLI lacks a flag. | Classify error, correct cause; at most one unchanged transient retry; retain pin. Check installed capability and report remaining limit. | Switch revision to get results, loop indefinitely or copy data-only flags. |
| SS-11 | “只看源码。” Later, separately: “结合游戏验证这个路径。” | First remains read-only; second hands fixed locations, conditions and unknown links to authorized live workflow. | Game input in source-only scope, or static unknown treated as final when an authorized live check can distinguish it. |
| SS-12 | “验证这个插件的 Classic 和 Forever 兼容性。” | Use ordered TOC closure and separately pinned matrix targets; preserve per-client unresolved coverage. | Recursive file scan replaces load closure; one client success generalized to both. |

## Executable evidence

These existing tests exercise the mechanisms required by the workflow; they do
not prove an agent will choose the correct next command. Tests use synthetic
source/metadata or isolated local Git objects, not a fresh remote/client run.

| Cases | Tests |
| --- | --- |
| SS-01, SS-07 | `internal/codebase: TestReadFixedSpanPreservesBytes` verifies excerpt/full blob and exact commit after branch deletion; `TestContextFlowUsesVersionedGeneratedMetadata` retains API facts and their original declaration. |
| SS-02, SS-03, SS-10 | `internal/command: TestSourceSyncRejectsUnknownCatalogSelectionAsArguments`; `internal/codebase: TestPrepareRejectsAmbiguousRefWithoutNetwork`, `TestPrepareSourceSelectionErrorsAreTypedBeforeFetch`. Exact release discovery and retry choices still require behavioral evaluation. |
| SS-04, SS-05, SS-06 | `internal/command: TestSourceResearchThroughCLI` runs fixed query, continuation, refs/context, unavailable runtime and explicit flow. This revision adds rejection of a valid query cursor reused with a different query or with refs. |
| SS-05, SS-07 | `internal/codebase: TestSearchLimitTruncatesHonestly`, `TestContextDepthExactLoadAndSnippetContinuation`, `TestContextExcerptContinuesLongLineWithoutDroppingBytes`, `TestIndexRetainsSyntaxDiagnostics`. |
| SS-08 | `internal/codebase: TestCompareTreesReportsDocumentAndDeclarationChanges`, `TestCompareTreesLimitIsPerCategoryAndCountsAllChanges`, `TestCompareTreesRejectsMissingIncompleteAndIncompatibleIndexes`. |
| SS-09 | `internal/codebase/flow: TestFlowGuardIdentityAndReassignment`, `TestFlowPermittedSinkAndUnknownEdges`, `TestFlowSameLineFunctionsRemainAmbiguous`; generated metadata context test above. |
| SS-12 | `internal/codebase: TestValidateSourceMatrixResolvesFixedEvidencePerTarget`, `TestInspectAddonLoadsTOCAndXMLClosureDepthFirst`. |

Repeat with `go test -count=1 ./internal/command ./internal/codebase/...`.
The full repository regression also includes these cases. Skill contract checks
validate command/flag availability and reference files, not reasoning quality.

## Evaluation boundary

2026-09-29: author walkthrough completed for SS-01…12. It prompted explicit
repository-track/client-environment separation and command-scoped cursor guidance.
The entry is now 76 lines; detailed value-flow guidance remains available without
loading it for ordinary API questions. Existing live links to the secret-values
anchor continue to work.

Independent-agent behavioral execution: **not_run** (no delegation in this task).
No agent success rate is inferred from fixture tests or author review. A future
run must retain actual commands, selected revisions/environments, coverage claims,
retries and any live actions. Wrong identity, false completeness, unsupported
commands or unauthorized game input fail the corresponding case.

No production source logic, npm release, installed skill or game state is changed
by this skill revision.

Verification passed: `go build ./...`, `go vet ./...`, full
`LYCHEEDEV_REQUIRE_LUA51=1 go test -p 4 -count=1 ./...` (log:
`.tmp/source-skill-full.log`), targeted CLI cursor regression, skill validation,
generated commands check, 87-command/210-reference contract check, both contract
tool tests and `git diff --check`. Environment-gated integration tests retain their
existing skips; no fresh remote-source, packaged LuaLS or real-client acceptance
is inferred from this run.
