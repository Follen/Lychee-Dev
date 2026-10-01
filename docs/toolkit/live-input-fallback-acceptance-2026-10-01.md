# Retail shared-installation INPUT v3 / F11 fallback acceptance

Date: 2026-10-01. This records a local, unpublished 3.1.0 development
candidate. The two real Retail processes share one managed installation.
The complete logical suite passed with three original-request recoveries;
it was not an all-first-invocation-success run. The earlier four-client
Mailbox acceptance used INPUT v2 / hybrid v1 and does not certify this input
protocol or fallback change.

## Candidate and deployment

The tested package is
`.tmp/retail-dual-fallback-candidate/dev-npm-stage/`, from source commit
`5cd696b946c40f5f5c03a502ebfaac87703e8a07` with `dirty=true`. Its manifest
contains 1,295 resources; it is a development package, not a release/tagged
clean-tree artifact.

| Artifact | SHA-256 |
| --- | --- |
| CLI `native/windows-amd64/lycheedev.exe` | `b7ae3140418658a9233f7594200258fa5edeed1583b376a3963b4a6e5b58e237` |
| Development `channel-lab.exe` | `f144523d0aa099b2953968fdf3cee7dc887b13ac5de8d6c75e27aecafac4cad9` |
| `addon/Bridge/ReceiverBindings.lua` | `a5f5d87244407f405addf51216b9fc672abd05289e34a14e16bb86eab79ef69f` |

All addon source, payload and manifest resource bytes matched before managed
`addon install --release` deployment. The original connections were already
retired, with no pending slot reservations or window owners. There was no
overlay copying, npm publication, global CLI/skill replacement or `latest` change.

Both running clients initially still published hybrid v1 after disk installation.
The owner performed `/reload` in both clients. Read-only named Mailbox checks
then independently verified INPUT `lycheedev.input.v3`, identity capability
`lycheedev.input.hybrid.v2`, 200-slot inventory and the exact primary profile
`ALT-CTRL-F12` / `ALT-CTRL-SHIFT-F12` / `ALT-CTRL-[` before new connections.
No old native journal was migrated into the new candidate.

Root evidence is `.tmp/retail-dual-fallback-20261001/`. Candidate provenance,
managed installation, source/payload comparison and per-process observations
are retained there, together with each project's whole `.lycheedev/live`
recovery unit. Raw command envelopes, argv, original fixture bytes and WGC
captures remain separate from this summary.

## Fixed real targets

Both OS-selected executables are
`D:/Game/World of Warcraft/_retail_/Wow.exe`, Retail 12.1.0.69933 / interface
120100, executable SHA-256
`d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd`.
PIDs, creation identities and actors stayed fixed throughout this suite.

| Target | PID / native creation identity | Actor / realm | GUID | Initial activated runtime |
| --- | --- | --- | --- | --- |
| A | `3452:134353180527444180` | 次年雪 / 白银之手 | `Player-707-06C79A5D` | `0000000500014ff3a2ee1a6961d842a8` |
| B | `55712:134353181276801384` | 荔枝小月亮 / 燃烧之刃 | `Player-829-03DAABDF` | `0000001f00014ff1f6ae1846d81193a3` |

OS creation times were respectively `2026-10-01T16:47:32.744418+08:00`
and `2026-10-01T16:48:47.680138+08:00`. Same build and shared files did not
make the two runtime/actor owners interchangeable.

## Real staged-business slot competition

`tests/channel-live/slot-skip-baseline.mjs` completed on real clients. A
development-only stage paused an ordinary read-only observation after prepare
publication and before game input; it did not bypass input or execute business.

| Fact | Retained value |
| --- | --- |
| A connection | `CON-3dd857987f7cc86818be052d3461918f` |
| A logical operation | `LMO-63c48f9e74e7c37065a732a56f8976fe` |
| A published prepare slot / nonce | `2` / `76a94cbc26288741faf3b95aef7e6bc2` |
| B connection | `CON-4a85f6f2df10ab19a40cfefe053c2702` |
| B prepare start / allocated slot | `2` / `3` |
| A original commit nonce | `53abafc349cf6f902e60037672295003` |

B executed its actor-tagged request while A's slot remained reserved. A's
payload and reservation were byte-for-byte unchanged. A then resumed the
original operation, retaining prepare index, nonce, runtime and owner; its
commit `preparedNonce` references that exact original prepare. The journal
contains one prepare input intent and one commit input intent for that operation.
Both reports matched their selected GUIDs and completed cleanup. A's same-request
retry was read-only and left its journal unchanged. Both CONs closed.

Evidence: `slot-skip/report.json`, `stage-a.json`, saved A payload/reservation,
the two full journals, and independent `slot-skip/journal-audit.json`.

## Actual F11 input and primary restoration

The subsequent shared connections were
`CON-6db14670a17fce35240f1fbe0ecfaa6b` (A) and
`CON-b77cd7735915ff2ed14281d68b51d97f` (B).
The checked-in `tests/channel-live/fixtures/binding_fallback.lua` ran on A
with a five-second execution budget and observation policy. It created only
a probe-owned temporary high-priority F12 CLICK override and button, registered
cleanup for those resources, and selected F11 through the private binding reset.
It did not edit account/character bindings or leave a persistent hook.

The verified report recorded primary F12 before the fixture, selected F11 / F11
submit afterward, `playerBindingUnchanged=true`, and zero fixture clicks.
A subsequent actor-tagged `actual-f11-host` request returned A's exact GUID
under the F11 profile. All four invoke input intents for that request contained
INPUT v3 F11/F11-submit observations and matching submitted outcomes. Thus
fallback was exercised by actual host input, not merely a returned configuration.
The explicit `Controls.Handle("receiver reset")` probe restored F12 and completed
cleanup. Evidence is `fallback/report.json`, its raw envelopes and A's journal.

The frozen candidate's Reset path called `applyNative(false)`. Therefore this
fixture did not invoke the localized initial-Register warning path. The retained
native WGC image shows the scene, but no fallback warning is visible. Warning
visual acceptance is **not passed** by this round. A focused warning delta and
fresh fixture/WGC verification are pending; this boundary does not invalidate
the verified F11 input/profile/restoration evidence above.

## Shared pressure, capacity and reload isolation

The same owned A/B connections completed concurrent 25-second actor-tagged async
observations. Both reported released transport input. Each also returned exactly
81,920 UTF-8 bytes with `--no-cache`; exact same-request retries retained the
operation and left their journals byte-for-byte unchanged.

The shared-installation runner held the six-second OS publication lease without
game input. Both original requests produced the required one-second pending
result, then resumed their original prepare nonces after the dependency changed.
It performed 47 logical paired requests per process, retaining actor/runtime
qualification on every result. Continuations after the failures below ran only
unfinished planned requests; completed requests were read back without execution.

Each process had one actual `automatic_reload_intent`, with `automatic=true`,
a `capacity-...` request and `identity.nextSlot=188` (above the 185 admission
threshold). Both continued on new runtimes. These facts are separately retained
in `capacity-threshold-proof.json`; runtime inequality alone was not the oracle.

During the final peer async observation, A reloaded from
`00000007000153b2f45575c75ffbe523` to
`000000080001543f4974988018dcb397`. B's running and finished runtime were both
`00000021000152923baf8b498742d031`, with the same original operation
`LMO-9c76859fe79c723812faa721611ea8e7` and B GUID in its verified result.
This establishes one-sided reload isolation on these two real processes.

## Three retained recovery events

| Original failure | Verified outcome and continuation | Evidence |
| --- | --- | --- |
| A `paired-29`, exit 5, `Access is denied.` | The business report was already verified and resource release confirmed. Host consumption/persistence had not completed. Exact-CON resume completed the original operation with its existing release input/nonce. The subsequent exact request lookup was read-only. The denied access has no established root cause; no shared-lock or retry-policy change was made. | `shared-pressure/a-paired-29-attempt-0.json`; `recover-a-paired29.json` |
| A automatic capacity transition for `paired-45`, exit 3, `short_read` / `memory.ineligible_read` | Reload input was already submitted. Original-CON resume continued the same automatic control budget, verified the new runtime and completed the original operation without resending reload input. | `shared-pressure-continuation/a-paired-45-attempt-0.json`; `recover-a-capacity.json` |
| B peer async finish, exit 3, Mailbox `path_changed` | Original-CON resume verified and finished the same operation on the same B runtime, without repeating the business probe. | `shared-pressure-final/b-peer-finish-attempt-0.json`; `recover-b-peer-async.json` |

The original failed reports are preserved in `shared-pressure/`,
`shared-pressure-continuation/` and `shared-pressure-final/`. The final logical
report references their chain and has `firstInvocationAllPassed=false`.
Verified business results, complete cleanup, transient observation/persistence
failures and invocation exit codes are separate facts.

## Final closure and audit

`shared-completed/report.json` records complete logical acceptance and both exact
CON disconnects with `closed=true`, `complete=true`, `cleanup=complete`.
`tests/channel-live/shared-installation-audit.mjs` independently read current and
rotated journals and passed:

| Audit | A | B |
| --- | --- | --- |
| Logical paired commands | 47 | 47 |
| Actual capacity reload intents | 1 | 1 |
| Original publication-wait prepare nonce count | 1 | 1 |
| Submitted effect identities | 220 | 210 |
| Recorded zero-send attempts | 29 | 43 |

It found no repeated submitted/uncertain effect identity, no pending reservation
in the 200-slot pool, and zero window-owner claims. Both connection recovery
units and original failures remain preserved. The source and development CLI
hash remained frozen through this real acceptance. No general cross-client,
future-build, latency-distribution or warning-visual result is inferred.

## Warning delta

Verified separately after the full suite closed: `Reset` now calls
`applyNative(true)`. ReceiverBindings source and packaged payload SHA256 are
`0d145166e97008839f247958d6af5a3b1fc9fb2c462e129d2e7e20716f2d43a7`.
Fresh development package `.tmp/retail-dual-warning-final-candidate` contains
1295 resources at commit `5cd696b946c40f5f5c03a502ebfaac87703e8a07`, dirty=true.
Its CLI and lab hashes remain the exact `b7ae314...` and `f144523...` values above;
the addon and skill payload changed. TGZ SHA256 is
`11339dbe2590ecfbddda47a8da19920d467e07ea0cda156fe17a19baf52d7dbd`;
release.json SHA256 is
`2a2f228d709d8e7ef05d1487a78be2f1e04d3d571abdce52283288f62c78b8b1`.
Managed installation succeeded; no overlay or registry publication occurred.

Both fixed real processes activated through the supported input-v3/hybrid-v2
CLI path. A's first activation read failed with short_read/ineligible_read after
its reload was submitted; exact original-CON resume succeeded without resending
that reload. B activation completed on its first invocation. New runtimes were
A `00000009000156bf95c588699b446c91` and
B `00000022000156da9fcbfe1d2e721b26`. This recovery is preserved separately from
the three full-suite recoveries.

Evidence root: `.tmp/retail-dual-warning-final-20261001`. Its fresh fallback
fixture, subsequent actor request, four actual input-v3 F11 invoke intents and
submitted outcomes, and explicit restoration to F12 all passed. Player binding
state stayed unchanged and the temporary conflicting button received zero clicks.
Native WGC frame
`a/evidence/20261001T101820.351600100-capture/frame.png` visibly shows the Chinese
warning naming occupied Ctrl+Alt+F12, the temporary fixture action, fallback
Ctrl+Alt+F11, submit Ctrl+Alt+Shift+F11 and close Ctrl+Alt+[.

Both final connections closed complete:
`CON-4b1dc3899f6e71d44d34179e67caa66f` (A, cleanup=complete) and
`CON-1d6795bdfafc92a699a5150e98666445` (B, cleanup=none because no business
operation was loaded). `closure-audit.json` proves pendingSlots=0, claims=0,
no active transaction, and only completed retained recovery/reload records.
All CLI/lab holders exited; persistent zero-byte OS lease files remain normally.
The full pressure suite above remains bound to its original frozen candidate;
it was not rerun for this warning-only delta. All live drivers are inactive.
