# Forever 1.60.1.70124: bounded Mailbox discovery stopped at protected text

Tested process: PID 10376, creation identity 134352957565325600,
`D:/Game/World of Warcraft/_classic_beta_/WowB.exe`.
The passive installation catalog identifies `forever` / `wow_forever`, interface
16001, full build 1.60.1.70124. `.flavor.info` says `wow_classic_beta` and
`version.txt` is absent. The executable SHA256 is
`3d2fbfb0a20567097fa9cedbeb55a8fed895cead6c1ff86103c89ee96f5ff58f`.
The managed addon was upgraded using the candidate CLI `addon install` to 3.1.0;
the main addon and all 200 slots inspect as managed. This disk result does not
prove that the running process loaded the new runtime.

The initial candidate stopped before any input with `memory.module_mapping_limit`.
A read-only VirtualQueryEx inventory of the exact OS main-module range found
4,955 regions at base `0x7ff7f71e0000`, size 140,742,656 bytes. The inventory is
complete; all regions are MEM_MAPPED / MEM_COMMIT, with AllocationBase equal to
the main-module base, and all region boundaries and sizes align to 4096 bytes.
The smallest region is one page. This demonstrates that the loader's 4096-entry
module count bound was incorrectly reused for one module's region geometry.
The corrected module inventory derives its bound from its own size/system page
size and retains a 512 MiB module size maximum; readable-page, type, allocation,
range, issuance and before/after read guards remain enforced.

Retesting the corrected candidate stopped in `recipe_unique_text` discovery with
`live.channel_mailbox_build_unsupported`. The current implementation requires one
complete runtime `.text` pass to prove a unique 58-byte code anchor for unknown
executable hashes. Its first text read at RVA `0x1000` encounters the sealed
PAGE_NOACCESS region `[0x1000,0xd5000)`. The guard rejects that region as
`issued_ineligible` before reading bytes. Skipping it cannot prove a unique anchor
across the required text range. This is incomplete coverage, not anchor absence,
not evidence of an incompatible Lua table layout, and not a character observation.

Both failed connection attempts returned no CON ID and sent no input. There is
no connection cleanup obligation. API actor identity, ordinary and 81,920-byte
results, no-cache execution, read-only request retry, runtime reload/rebinding,
and disconnect tests were not run. No root recipe/profile was registered or
confirmed for this build, and no old Retail or Classic RVA or section hash was
copied. Cross-process reuse remains unverified.

Evidence:

- `connect-mapping-limit.json`: original module geometry refusal.
- `module-mappings.json` and `module-mapping-summary.json`: metadata only;
  no game-memory contents or heap scan.
- `connect-protected-text.json`: corrected candidate's exact guarded refusal.
- `wowdump-describe.json`: installed read-only investigation contract.

The full live evidence/recovery directory remains under
`.tmp/mailbox-live/forever-project`; candidate attempt results are preserved in
`.tmp/mailbox-live/forever-acceptance/attempt-2` and `attempt-3`.

Additional root research used the existing Go PE parser and `luaRootMatches` /
`luaRootTarget` helpers, with the current sealed module reader. An explicit
research-only test (retained as `research-helper.go.txt`, removed from the normal
package tests) scanned the complete disk `.text` bytes (76,776,448 bytes): the
existing 58-byte pattern had zero matches. This does not establish what loader
transformations occur. A separate runtime scan read 23,392,256 bytes through
2,177 guarded module reads. It skipped 2,179 inaccessible or changed mapping
regions, reset pattern tails across every gap, and found zero candidates in
that partial coverage. One region changed its sealed region-end geometry before
reading; the reader rejected it, and the observation retained that rejection.

These results rule out a same-pattern disk candidate for this exact file and
leave runtime coverage incomplete. They cannot supply a known-build root,
justify skipping missing bytes in unique-anchor production discovery, or confirm
a root capability. The IDA connector returned zero currently open databases.
No production root resolver or root binding records were modified.

For semantic follow-up, wowdump captured `.rdata` and `.pdata` completely
(19,057,316 bytes) and `.data` completely (44,140,001 bytes). A separate `.text`
dump recorded 20,381,696 readable bytes out of 76,776,444 requested bytes, with
1,849 gap entries; these are independent sample-time results, not the same
coverage as the earlier guarded research scan. Manifests preserve exact process
creation identity, executable hash, section hashes, RVA/file mapping, and gaps.
The installed wowdump names this product `classicbeta@1.60.1.70124`; that is its
catalog label for the same PID/executable/hash that lycheedev names Forever.
The workspace was created using actual PID identity without changing that label.

The complete runtime `.rdata` search finds `GetBuildInfo` at RVA `0x4f08750`,
while the original disk IDA view finds it at RVA `0x4e7e390`. `RunScript`'s runtime
name is at RVA `0x4f39de0`. The runtime and disk data are therefore demonstrably
different at these semantic locations. Searching complete module `.rdata` and
`.data` for the actual GetBuildInfo string's raw VA finds no naked pointer table
entry; this does not exclude RIP-relative references, dynamic encoding, other
registration forms or heap structures. No heap search was performed.

An independent `.tmp` PE copy overlays runtime `.rdata`, `.pdata` and partial
`.text` using the standard Go `debug/pe` section offsets. The source executable
is preserved. `runtime-overlay-provenance.json` records each source manifest,
section SHA256, raw offset and RVA, the complete gap ledger, and the derived
file hash. Zero-filled missing pages are absence of evidence, never evidence of
original zero bytes. This derived image is only an IDA analysis aid and cannot
issue a production layout/root capability or substitute for the original
executable's hash. Original and derivative IDA sessions are separate.

IDA's initial original-file view (database `9102c0fa`, no automatic analysis)
found the expected Lua API and assertion strings. It had no established code
references for GetBuildInfo or the state-pointer diagnostics, so those empty
xref results do not prove absence. Opening the derivative with automatic
analysis timed out at the MCP's 300-second call limit; a subsequent inventory
query also timed out. The analysis worker remained active. A single further
inventory query is the bounded recovery attempt; no worker was killed, no
live debugger attached, and no missing page was read or modified.
`ida-research-status.json` retains original source queries and the recovery point.
The existing 58-byte recipe still has no new-build candidate, and the semantic
Lua-state root remains unverified.

The final candidate-wake CLI (with the updated addon telemetry payload) was
installed through managed `addon install`, using a fresh backup archive. It
inspected as managed with all 200 slots managed and zero pending slots. Fresh
`live connect --no-cache` against the same PID returned exit 3 and
`live.channel_mailbox_build_unsupported` before reading the first protected text
span; it returned no CON, operation ID, activation or capture. A subsequent
passive inventory retains the same process creation identity and shows no durable
busy owner for this process. The independent Forever project has no connection
files. No live input was sent and no connection cleanup is owed. Final CLI and
executable hashes, exact error, managed installation evidence and all not-run
checks are retained in `final-acceptance-summary.json` and linked JSON evidence.

The one permitted IDA inventory recovery attempt also timed out at 300 seconds.
Further retries against the same busy worker would not improve the evidence
within this bounded investigation. The original and derived files, section
manifests, gap ledger and loose database state are retained for future recovery.
No Lua-state root was verified, no profile/recipe was confirmed, and no known
hash binding was added. This completes the bounded research with an explicitly
unsupported live result for this candidate and exact untested boundaries.

## Re-entered actor and independently verified RunScript root (follow-up)

The preceding unsupported results remain historical results of the first
58-byte-only candidate. After the owner re-entered the same process, independent
bounded runtime code analysis followed RunScript's real call chain to a distinct
49-byte state-load wrapper at RVA `0x75a69d`, deriving the module-data root
`0x7b780b8`. The exact anchor SHA256 is
`f04a6ad4b769b3e1dc12e72dee586d58d3c87f17097c66a60244a3da5f1de46c`.
Lua stack code independently establishes TValue stride 24; the -10002 global
index branch resolves state+0x90. This is semantic evidence for this exact
executable hash, not a claim of whole-text uniqueness across protected gaps.

Fresh raw anchor reads and typed root queries verify thread tag 8, globals table
tag 5 and secret flag 0. An independent read-only public-path reader reaches
`_G.LycheeDevInternal.Mailbox`, verifies identity/input wire checksums and
locating guards, and observes release 3.1.0, 200 slots and actor Aotu/Aotu,
GUID Player-4618-01188FA2. Its fixture self-test and exact byte replay pass.
No game input is involved in these reads. The immutable root-runscript anchor,
profile and capability were linted, fixture-tested, saved and confirmed for read
in the local wowdump library. wowdump's catalog labels this exact process
`classicbeta@1.60.1.70124`; observed game product remains Forever.

`root-runscript-*`, `capability-runscript-*`, `runscript-*` and
`relogin-mailbox-reader*` retain this evidence. Production connection, execution,
reload and pressure acceptance are pending the rebuilt candidate. Same-process
reload and cross-process reuse remain not_checked at this evidence checkpoint.
The original failed candidate records are preserved without reinterpretation.

## Sealed candidate live completion

Sealed candidate 3ebce6f071af6a35053d37639a86d511b57e23561b9e4e6f227d9e8177e229fc was installed with managed addon
install and fresh backup. Explicit activation reload retained the exact PID/actor
and completed through original-CON resume. Core completed 12 semantic checks
(14 CLI outcomes including two continuations): real API actor, normal/81920-byte
results, no-cache, original-request journal invariance, reload/retry, post-reload
execution and disconnect/retry. The 47 unique normal requests completed with
verified capacity reload, historical journal invariance, correct compile error,
runtime destruction after cleanup failure, bugs snapshot and disconnect.

This is acceptance with observed recoveries, not first-call-all-green. The first
pressure run stopped at normal20 commit_ready short_read; its input gate recorded
not_sent/zero messages. Original-CON resume completed it. The resumed runner read
the first 20 original results before continuing. Further bounded continuations
are listed in sealed-pressure-resumed-report.json. The original reader discarded
short-read address and cause; these envelopes cannot establish why it failed.
All activation, core and pressure CONs are closed; no cleanup is owed.

Independent registered root queries and full public-path readers pass after core
reload and pressure/cleanup reloads, as does exact-byte replay. Lua state changed
from 0x1df305a7398 to 0x1e0389740d8 after core reload while the semantic module
root stayed valid. Same-process reload is verified. Cross-process reuse remains
not_checked; protected-gap whole-text uniqueness is not claimed. The local
capability stays read-confirmed; formal semantics/reuse are not promoted here.
Historical unsupported results remain intact. See sealed-live-acceptance-summary.

## Final actual-location candidate acceptance

Final candidate SHA256
`9b4846015f9a22d64994b494be8c0412eace70c0fbdd24344239283bc125f7a8`
passed the complete fresh Forever round: managed addon and all 200 slots,
explicit activation reload, core 12 checks, 47 unique pressure requests
(54 pressure CLI calls including history/errors/cleanup/bugs/disconnect).
Direct JSON statistics show zero nonzero exits, zero short reads and zero
continuation/recovery calls. Capacity reload, historical retry journal invariance,
compile error, cleanup-error runtime destruction/reload, bugs snapshot and all
three connection closures passed. No cleanup is owed.

The intermediate span candidate still had two first-call failures at normal8
and normal41. Both were recovered through the original connection and are
preserved in span-pressure-* alongside the earlier sealed candidate failures.
The new diagnostics identify their module-anchor before-read geometry check:
querying the old issued region start returned a split prefix that did not cover
the actual anchor. This establishes the mechanism for those captured failures;
it does not retrospectively assign a cause to all historical short_read strings.
The final candidate validates the actual requested address and its bounded
intersection with the issued module coverage. No such error was reproduced in
this complete final run; this is not a guarantee of universal future absence.

Registered typed root queries, independent complete public Mailbox paths and
exact byte replay pass before/after core reload and after capacity/cleanup
reloads. Final actor remains Aotu/Aotu, Player-4618-01188FA2, product Forever,
build 1.60.1.70124, runtime 0000001e000104e8453b04ccfd49fe3d, owner empty.
Recipe/capability read confirmations were appended to the immutable local
library evidence. Formal semantics/reuse levels stay not_confirmed; cross-process
reuse remains not_checked. Whole-text uniqueness across protected gaps is not
claimed. See current-live-acceptance-summary.json and current report assets.
