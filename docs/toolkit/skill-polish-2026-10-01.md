# Whole-skill polish — 2026-10-01

The initial skill commit updates the versioned Lychee Dev skill for the
integrated 3.1.0 Mailbox/source/data implementation. Its instruction and UI
metadata changes leave runtime code, generated commands and launcher unchanged.
Follow-up authorized dual-instance acceptance also repairs the tagged workbench
build, strengthens lifecycle assertions, and adds verified F12/F11 input profiles
with conflict warnings and binding recovery. Those runtime changes and their
acceptance are separate from the offline skill evaluation below. No release is published.

## Changes that affect decisions

- Main entry routes by the missing evidence and retained task state, reads only
  relevant references, preserves explicit pins and separates source/data/live identities.
- Native Live follows the build-bound root and named Mailbox. Removed distant-address
  relocation, input-v1 fallback and old-native journal migration promises. Recovery
  follows actual continuation kinds; an already authorized action does not need a
  fresh permission prompt. Unknown input stays unknown across cleanup or a new actor.
- Source preserves separate third-party/client environments, command-bound cursors
  and opt-in LuaLS reuse. Validation distinguishes single/matrix shapes and static
  fallback; bounded flow cannot prove unmodeled event routing or runtime taint safety.
- Data separates query/output/table coverage, cumulative resource budgets and CLI
  sessions. Hotfix scan/remote cursors, raw captures, latest push batches and effective
  membership retain distinct meanings. SQL continuation handles tied join rows.
- Assets use exact-name lookup rather than a wildcard example, retain listfile/variant
  limits and verify external publication before repeating an export or demux.
- Installation preserves the chosen release/channel: the npm updater fetches
  @latest and has no version selector. Clean files are distinct from loaded runtime.
  Probes detach owned event scripts; errors retain provider/history coverage.

## Independent forward review

Three GPT 6.1 sol agents implemented separate source, data/assets and Live-support
sections; the root implemented the entry and Live connect/startup/recovery. Four
fresh agents then handled supplied requests without prior conclusions, diffs or
expected answers. The root reviewed their actual outputs against scope, identity,
coverage and supported-command invariants. All 25 decisions were accepted with
their stated limits; they are offline plans, not executed CLI or game results.

| Cases | Observed decision |
| --- | --- |
| S1–S3 | Explicit source pin wins; third-party/Classic environments stay separate; changed direction starts fresh pagination using supported commands |
| S4–S5 | Incomplete matrix/flow does not prove all-client or secret safety; failed semantic setup leaves a supported static fallback |
| L1–L3 | Real executable selected; active driver waited for; exited process retired before fresh actor binding, preserving unknown old outcome |
| L4–L6 | Verified business failure retained; unsupported root not replaced by a scanner/reload loop; same-build instances keep separate ownership |
| D1–D2 | Duplicate names remain candidates; incomplete encrypted/offline coverage cannot establish absence or justify networking |
| D3–D4 | Hotfix resumes the archived scope; maximum push differs from effective state; incomplete Wago text matches are not absence |
| D5–D6 | Existing export checked before replacement; variant ambiguity retained; installed CLI limits do not trigger silent upgrade |
| R1–R8 | Five relevant addon/data/asset/live/update requests select the skill; gameplay, raw RVA discovery and browser JS do not |

Inputs, full actual responses, conditions and unresolved facts are retained in
[forward-review evidence](skill-polish-forward-2026-10-01.json). Tokens in those
fixtures are normalized placeholders, not runnable process identities or evidence IDs.
All 69 proposed CLI command paths/flags passed the actual 89-command contract;
that check does not execute commands or validate placeholder values.

## Initial skill verification

- skill-creator quick validator: passed; implicit invocation policy preserved.
- All 21 Markdown files: 70 relative links, including heading anchors, valid.
- Offline baseline: passed; build/vet, 2998 named Go test outcomes across
  43 packages, mandatory Lua 5.1, real LuaLS, 98 Node tests,
  version consistency and generated skill command consistency.
- Go includes 40 explicit helper/environment/manual skips; no failures.
  Interactive acceptance and optional external fixtures remain separate.
- Skill payload SHA-256: `1a32ffd4b0d9e946832d2682efb1a2b5f0a79bae4533825f8bcc9d36564c9ee1`.
  The baseline ran with this unchanged payload; this review record was added afterward.

The launcher was inspected and its existing tests passed. The initial offline
skill review does not establish runtime behavior or performance improvement.

## Live follow-up and current contract

Retail dual-instance acceptance exposed the F12 collision and binding recovery
gap. The current skill startup/recovery references follow INPUT v3/hybrid v2,
the effective complete F12/F11 profile and exact CLICK ownership. Explicitly
unsupported loaded capabilities do not authorize automatic fallback input;
clean installation and selected-client manual protocol activation remain separate.

A fresh independent read-only evaluator handled two additional requests using
the final references: an unsupported loaded capability with unknown old input,
and same-build peers where only A publishes empty blocked bindings. Both retained
the original process/project/CON and uncertainty, avoided guessed keys or peer
identity, and selected supported status/resume/disconnect paths. These were
offline decisions, not executed commands.

The new [real-client acceptance](live-input-fallback-acceptance-2026-10-01.md)
records staged slot competition, 47 requests per peer, capacity rollover, async
and reload isolation, actual F11 input, warning WGC and zero final ownership.
Three full-suite recoveries and one warning-candidate activation recovery remain
visible; there is no first-invocation-success or new other-client claim.
Final Windows offline baseline passed at
`.tmp/retail-dual-fallback-offline-final/report.json`: build/vet, 3,018 named
Go outcomes across 43 packages (40 explicit helper/environment skips, no
failures), mandatory Lua 5.1, real LuaLS, 98 Node tests, version and both skill
contract gates. The 13 separate live/manual cases retain `not_run` in this offline
report; real-client results are in the linked record, not inferred from fixtures.
Source remained unchanged during the run, SHA-256
`f7ef6629cfdb89760cc95dd54a1f2ae6962fb50c53f2d3ce59d8b19ce420a735`.
Tagged channel-lab build/vet and changed Node entrypoint syntax checks also
passed. This verification paragraph was added after the frozen baseline.
