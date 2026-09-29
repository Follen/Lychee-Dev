# Skill entry, discovery and live workflow review

Scope: repository Skill on main after merge `995b405`. The existing source/data
references remain unchanged. This work edits instructions and metadata, not the
CLI, game addon, managed installations or publishing configuration.

## Discovery and entry

`SKILL.md` frontmatter describes WoW technical research, source/data/assets,
validation, live observation/recovery and requested toolkit installation/update.
Chinese task wording covers addon errors, spell/item data, live verification and
stuck connections without requiring the user to name the CLI. General gameplay,
fiction and raw address/offset work are not routed into the native live workflow.
The description selects the domain; scope and the loaded references select actions.

`agents/openai.yaml` retains `allow_implicit_invocation: true`. Its UI description
and default prompt agree with the entry. The prompt is an invocation example, not
a keyword trigger engine. No undocumented trigger list or host settings are added.

The entry shrinks from 172 to 95 lines. It routes by evidence and retained state,
keeps source/data/live identities separate, preserves explicit/project pins, and
distinguishes source-only requests from existing live authorization. Short follow-ups
inherit the task and original evidence; they do not silently retarget it.

## Live references

- `live-investigation`: existing CON versus a new task; an idle authorized CON can
  be reused, while unfinished work retains its request. Same-build instances stay distinct.
- `live-recovery`: continuation, durable budgets, input uncertainty, contention,
  process exit and persisted runtime replacement proof. Closing reload retires the
  old CON without rebinding or reviving its business operation.
- `live-startup`: installation/contact/activation, linked to the shared recovery rules.
- `live-probes`: bounded Lua/API/cleanup, simple observation versus diagnostic
  hypotheses, source correspondence and secret/protected boundaries.

Old live heading anchors remain valid for source and other references. Mechanical
scanning, colors, slot files and input transactions remain CLI responsibilities.
The once-per-budget distant-record relocation is described as a CLI capability,
not a scan/retry recipe for an agent. A verified report, its assertion result and
cleanup completion remain separate facts.

## Independent forward testing

Two fresh agents receive only requests, permitted Skill content and small synthetic
result snapshots. They receive no expected answers or prior conclusions. Evaluations
are offline and prohibit CLI execution, game input, network and installed-file changes.

**Selection:** the evaluator reads only frontmatter for lycheedev, wowdump and
diagnosing-bugs. The final 22 cases yield the expected Lychee Dev selection in 15
domain cases and no Lychee Dev selection in 7 out-of-domain cases. Positive cases
include versioned API/source, spell data, addon errors, retained-CON recovery,
assets, performance, Hotfix, TOC, continued work and toolkit installation/update.
Negative cases include general gameplay, fiction, explicit raw offsets, React,
standalone Lua, browser plugins and Linux reload faults. Initial review identified
that installation was only implied by the toolkit name; it is now explicit and the
final description was reevaluated. Raw decisions and the initial pass are retained
under `.tmp/skill-polish-20260929/trigger-evaluation*.json`.

**Workflow:** twelve synthetic cases in `.tmp/skill-polish-20260929/cases.json`
exercise active-driver waits, uncertain closing reload, verified failed reports
with pending cleanup, shared-slot contention, PID replacement, persisted runtimeEnd,
idle-CON reuse, partial empty data, explicit source pins, source-only secret errors,
authorized source-to-live continuation and an older installed command contract.
All twelve decisions satisfy the reviewed scope and recovery invariants. Raw
decisions are in `.tmp/skill-polish-20260929/workflow-evaluation.json`:

| Cases | Observed decision |
| --- | --- |
| L1 | Wait for the active driver; no competing resume, Escape or reload. |
| L2 | Resume the same CON/reload-one within its remaining deadline; no replay. |
| L3 | Report the failed assertion separately; recover cleanup, then disconnect. |
| L4 | Preserve both targets and the shared slot; no bypass promise or slot deletion. |
| L5 | Verify old process exit and retire its CON; use a new CON for authorized PID 303. |
| L6 | Continue retirement from saved runtimeEnd at character selection, without input. |
| L7 | Reuse the idle CON with a distinct bounded request; do not rebuild the connection. |
| D1 | Empty partial data cannot establish absence; retain pin/local source and gap. |
| S1 | Answer from explicit PIN-B without changing project PIN-A or adding game work. |
| S2 | Continue bounded static secret-value tracing; no unauthorized live check. |
| S3 | Continue authorized runtime investigation with a bounded, reversible observation. |
| D2 | Stop the unsupported timeout branch; do not invent a flag or silently upgrade. |

These are decision outcomes, not executed commands or game results. In particular,
the probe outline in S3 is not a tested Lua implementation. Simple live state
queries explicitly avoid invented competing hypotheses or unrelated source research.

These checks exercise model selection from a small supplied catalog and subsequent
workflow decisions. They do not measure automatic discovery in every host/session
or claim a universal trigger rate. Editing repository metadata alone does not
replace the installed Skill or refresh a running conversation's loaded metadata.

## Mechanical validation

Quick validation, UI metadata and the 87-command contract pass. All 69 relative
links/anchors resolve, including retained source-to-live anchors. Generated command
reference is unchanged and current. The fresh full offline baseline at
`.tmp/skill-polish-20260929/offline/report.json` passes build, vet, mandatory Lua 5.1
and Go tests (42 packages / 2533 test events), all 88 Node tests, version checks,
Skill contracts, generated command reference and source stability. Its 34 optional
environment/helper skips remain explicitly recorded, not counted as passes.
Frozen source digest: `92c736e83fec847e0b076a1c912eaa5f4f5c4e594da4f3d5484d8a5067f6307a`.
Only this acceptance note was updated after the run; Skill files remained frozen.
