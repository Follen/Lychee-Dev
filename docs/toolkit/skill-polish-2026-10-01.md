# Whole-skill polish — 2026-10-01

This change updates the versioned Lychee Dev skill for the integrated 3.1.0
Mailbox/source/data implementation. It changes agent instructions and UI metadata;
the CLI, addon, generated command reference and thin launcher remain unchanged.
It neither publishes a release nor updates an installed skill or running client.

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

## Verification

- skill-creator quick validator: passed; implicit invocation policy preserved.
- All 21 Markdown files: 70 relative links, including heading anchors, valid.
- Offline baseline: passed; build/vet, 2998 named Go test outcomes across
  43 packages, mandatory Lua 5.1, real LuaLS, 98 Node tests,
  version consistency and generated skill command consistency.
- Go includes 40 explicit helper/environment/manual skips; no failures.
  Interactive acceptance and optional external fixtures remain separate.
- Skill payload SHA-256: `1a32ffd4b0d9e946832d2682efb1a2b5f0a79bae4533825f8bcc9d36564c9ee1`.
  The baseline ran with this unchanged payload; this review record was added afterward.

The launcher was inspected and its existing tests passed; a documentation polish
does not justify another launcher or transport. No runtime behavior, performance
improvement, installation activation or newly tested client is claimed here.
