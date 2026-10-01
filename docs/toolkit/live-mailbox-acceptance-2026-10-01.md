# Native Mailbox 3.1.0 acceptance

This is a local development candidate, not a published release. The source and
managed addon advance together to the new protocol; npm latest stays 2.5.1.
The final frozen candidate completed Retail, Classic and Forever live suites.
Classic and Forever passed without continuation recovery; Retail activation
used an exact-CON continuation after a transient post-input read refusal. Offline
baseline verification is recorded separately below. Results describe their
actual candidate and do not inherit earlier memory-scanner acceptance.

## Exact targets and root evidence

| Client | Build / interface | Executable SHA-256 | Root evidence |
| --- | --- | --- | --- |
| Retail | 12.1.0.69933 / 120100 | `d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd` | Known hash, guarded anchor `0x6749b4`, root `0x79c0c18` |
| Classic | 5.5.4.70032 / 50504 | `5cb9bcb453c006eb2ff37b8342b582ec4ea7fca323b6d66e6c12fefcb3df463f` | Unique runtime text recipe, anchor `0x62dc34`, root `0x701dda8` |
| Forever | 1.60.1.70124 / 16001 | `3d2fbfb0a20567097fa9cedbeb55a8fed895cead6c1ff86103c89ee96f5ff58f` | New independent 49-byte RunScript site `0x75a69d`, root `0x7b780b8`; exact original code guard, no full-text uniqueness claim |

The initial Retail process instance was `34732:134352920362860468`; its selected actor was
暗夜荔枝 / 罗宁, `Player-729-06AA5F07`. A preceding read-only sample on
荔枝小月亮 / 燃烧之刃 had no connection or submitted input. The user changed
characters before connection; the new VM root and actor were independently
verified before continuing. This is same-process relocation, not cross-process
reuse of the complete Mailbox reader.

Later real-process instances and actors are recorded per acceptance round below;
the initial instance is not the target of every final-candidate test.

Classic process instance is `50368:134352957691792733`, 次年雪 / 祈福,
`Player-4778-073BD91C`. Its unique text pass covered 56,605,244 bytes and used
71 module reads / 56,605,910 bytes including guards. Anchor digest is
`50b9920287e4cc8712f8c74d8a6dd1e42fede452ee774009b858c6cdcd97ef16`.
These are independently derived Classic RVAs, not Retail offsets.

## Completed initial core acceptance

The candidate at `.tmp/mailbox-candidate-acceptance/dev-npm-stage/` included
the strict publication-kind validators, locked-file layout proof and module
mapping guard fix. Retail report is
`.tmp/mailbox-live/acceptance-success/report.json`; Classic report is
`.tmp/mailbox-live/classic-acceptance/resumed/report.json`.

Both passed an independent in-game actor/API probe, ordinary result, exactly
81,920 UTF-8 bytes of large result, both ordinary and large requests with
`--no-cache`, reload, execution after reload and disconnect. Each ordinary,
reload and disconnect historical retry reused its original request and left
the connection journal byte-for-byte unchanged. Probe reports were verified
with `report.ok=true` and completed cleanup. Both core CONs ended closed.

Retail ordinary/large calls took about 4.9–5.2 seconds; actor probe 4.8 seconds,
reload 9.9 seconds and disconnect 1.9 seconds. Historical retries took 36–46 ms.
Classic corresponding calls took about 4.8–6.2 seconds; reload 13.2 seconds,
disconnect 2.0 seconds and historical retries 37–43 ms. These are individual
observations, not latency distributions.

Classic first-installation activation on the upgraded managed addon encountered
a transient `short_read` during the reload transition. Its original activation
and CON were retained; `live resume` bound the replacement without replaying
input. There was no second connection or replacement request.

An example Retail receipt lookup used 128 reads / 2,988 bytes in 2 ms and
completed only the named-field path. The known root initialization read 724
module bytes with 18 calls, verified 296 header bytes and scanned zero text.
No-cache follows this same current Lua table path, with no heap-address hint.

## Failure retained and retired

The additional Retail 47-request capacity suite stopped on `normal-2` of
`CON-c22e7e9e094073983fdde4a31e1c8260`: commit slot 7 was journaled as submitted
with six queued messages, but no qualified exact receipt appeared. Of 256
observations, 198 receipt lookups completed with no match and took 481 ms total;
53 identity lookups succeeded and took 79 ms total. Repeated wait intervals,
not memory lookup time, exhausted the invocation observation budget. This
does not prove that the key was delivered or that the business did not execute.

The original business deadline later expired. The owner-approved recovery used
the same project and CON: disconnect reported `closing_exchange_unconfirmed`,
then explicit reload request `capacity-retire-reload` verified a replacement
runtime through two increasing current input samples and retired the old CON.
The final result was `closed=true`, `complete=true`, business report unavailable.
The unknown commit was never replayed. Original failure and recovery are in
`.tmp/mailbox-live/capacity-project/` and
`.tmp/mailbox-live/capacity-retire-reload.json`. Capacity is not passed by this run.

## Structural issues established by real clients

Retail maps its executable using `MEM_MAPPED` and multiple allocations. Runtime
PE fields unrelated to locating section tables can differ from disk. The module
reader therefore seals the OS-selected range, actual mapping plan and layout
from the same read-locked hashed executable, and compares locating metadata plus
the complete section table. It neither rewrites the runtime header nor admits
private pages to module reads. Querying the issued region start avoids mistaking
VirtualQueryEx's forward-only interior-page result for a changed mapping.

Forever has 4,955 legitimate page-aligned module regions in 140,742,656 bytes.
The loader inventory's 4,096 entry bound was incorrectly reused for this mapping
plan. The mapping bound is now derived from the module's virtual byte size and
system page size, under a 512 MiB virtual module cap. Issued segments are searched
with binary search; every read still checks the exact original region and holes.

After that correction, Forever root discovery correctly refused a `PAGE_NOACCESS`
text segment before any connection or activation input. A separate research
pass found zero current-pattern candidates in complete disk text and zero in
23,392,256 readable runtime text bytes; runtime coverage was incomplete. It
does not establish that the full runtime text has no candidate or no Lua VM.
No old RVA, fabricated binding or permission bypass was installed. See the
Forever research bundle for exact gaps and module evidence.

## wowdump assets and confirmation scope

[Retail bundle](research/lua-mailbox/README.md) preserves the immutable root
recipe and complete fixed-build JavaScript reader. The local recipe is
`retail-lua-state-root-rip-v1`; root capability is
`retail-lua-mailbox-root-v1-69933`. Both have read confirmations. Complete
reader source, all sealed assets, original byte samples and replays, root
queries and independent API probe were copied into local recipe evidence:
`C:/Users/follen/.wowdump/evidence/retail@12.1.0.69933/retail-lua-state-root-rip-v1-ac8a8673f5ff.json`
(SHA-256 `ac8a8673f5ff054e3224741cfc868cb240fa1f3836f7d504abf87831bf4d8e1b`).
The full JavaScript reader was observed before and after a character change and
verified runtime retirement: eight later samples each used 135 reads / 3,165
bytes; replay matched the recorded bytes. It never grants input authority.

[Classic bundle](research/lua-mailbox-classic-70032/README.md) has its own
recipe `classic-lua-state-root-rip-v1-70032`, root capability and read
confirmations. Full live acceptance plus the production Go binding are retained
in library evidence. The source Retail recipe has an appended Classic read
confirmation; its original bytes and build binding remain unchanged.

Semantic observations and same-process reload are recorded explicitly. No
complete-reader cross-process reuse confirmation is claimed. A passing schema,
fixture or migration does not certify a future build's Lua ABI. Titan live,
first clean install and complete-reader process restart remain `not_run` here.

## Preceding candidate live acceptance

This preceding development CLI SHA-256 is
`707e47cedb4219932dd001c98b7278d40c3bae5fdb30e42deea551df30a33929`.
Its addon SlotRuntime digest is
`602e70a57da66dbb9bc656e637a65bc10fdb36f62b438b8434291a68a640d129`,
InputState digest
`c7faa3c42289b34f0b86bef9539ca6291b16e1b9297fa999ab59b9590c6a0631`.
Managed installation digests matched the stage, and explicit reload immediately
after connect loaded these new bytes despite the unchanged candidate version.

| Final suite | Result / recovery state | Evidence |
| --- | --- | --- |
| Retail core | All 13 steps passed; `CON-69bb2147f75124b1c94d8c211b244f75` closed, cleanup complete | `.tmp/mailbox-live/final-retail-acceptance-attempt2/report.json` |
| Classic core | All 13 steps passed; `CON-3e44fd5798b0ae446e10eb9f45716406` closed, cleanup complete | `.tmp/mailbox-live/final-classic-acceptance/report.json` |
| Retail capacity | All 54 steps passed; `CON-dc9039e13fe42c5cbe327ee8c75a269b` closed | `.tmp/mailbox-live/final-retail-capacity-project/baseline.json` |
| Classic capacity | All 54 steps passed; `CON-217f6cb3eea88e7b58151f49e5294f03` closed | `.tmp/mailbox-live/final-classic-stress-project/baseline.json` |
| Forever final candidate | Clean managed addon and 200 slots; exit 3 before CON/input; current recipe unsupported | `research/lua-mailbox-forever-70124/final-acceptance-summary.json` |

Each capacity suite ran 47 ordinary requests, verified automatic capacity runtime
replacement, rotated history and reused `normal-1` without journal changes.
Expected compile failure was a verified business report, not transport failure.
The intentional cleanup failure preserved `resourcesReleased=false`, completed
verified reload and reported `cleanupMethod=runtime_destroyed`. Bugs and normal
disconnect then completed; Classic also verified byte-identical no-cache close
retry. The independent projects and windows retained their own driver/CON;
physical input uses target-window PostMessage, not foreground SendInput.

The new callback diagnostic appeared in real INPUT and journal observations:
same runtime, increasing attempt counter, exact latest slot and `received` stage.
The sampler still follows its original budget and disabled-state contract.
Neither final pressure suite reproduced the earlier unknown commit; this is
positive coverage, not proof of that original input loss's cause or impossibility.
The old request remains unavailable, retired and preserved separately.

Final complete reader observations and replay, candidate manifest, source
digests and both core reports were additionally copied to local recipe evidence
`C:/Users/follen/.wowdump/evidence/retail@12.1.0.69933/retail-lua-state-root-rip-v1-425bda77e3ed.json`
(SHA-256 `425bda77e3edc7bbce7c4c3e18fb77f6aed6c5dabf63eb011da4d0ac721776b8`).
Its 43 embedded assets survive removing this checkout. Final JavaScript samples
used 135 reads each, about 3,336 bytes including the callback diagnostic, with
eight samples / no indeterminate reads; exact byte replay passed.

## Offline acceptance

The final full baseline uses mandatory Lua 5.1, the pinned real LuaLS runtime,
all Go build/vet/tests, Node distribution/runner tests, version and generated
skill contract checks, plus source-unchanged validation. Its authoritative raw
report is `.tmp/mailbox-baseline-final/report.json`. Targeted module/migration,
strict wire-header and diagnostic regression/race suites passed before this
run. New required-bool negative tests confirmed the already-present gate;
production code and candidate bytes did not change for that test-only batch.

The full baseline completed **passed** at `2026-10-01T03:23:37.622Z`:
BUILD, VET, GO (2,844 named tests across 42 packages, with optional/helper skips
listed separately), NODE, VERSION, SKILL, SKILL-GENERATED and SOURCE all passed.
Mandatory Lua and real LuaLS integration ran. Frozen tree fingerprint was
`e8a1f4629f31a020046671de23e1ca86d6da4f94fd6fc23abf5b056e45327158`;
the baseline includes the optional real Retail dump/current runtime-header
replay. This result paragraph and the final evidence index were appended after
the check; production source and tested candidate bytes remained unchanged.

After the user re-entered the Forever character, the same process/build was
rechecked. Connect still refused the same first protected text span before
input; current mapping count changed from 4,955 to 4,147. An independent bounded
readable-text research pass then covered 26,234,880 bytes with zero current-pattern
candidates and incomplete coverage. Re-entering did not clear this specific
discovery blocker. At this stage the actual actor was independently unverified
while no Mailbox root was available. The new round is preserved at
`.tmp/mailbox-live/forever-acceptance/relogin-round/`; it does not alter the
earlier immutable research or certify a future unsupported fallback.

## Subsequent Forever semantic root research

The same re-entry round subsequently followed the actual runtime `RunScript`
string reference into its Lua caller, rather than searching for the old pattern.
This independently located module-global RVA `0x7b780b8`. The caller supplies
that state to Lua operations; a second API path demonstrates 24-byte TValue
stride and `LUA_GLOBALSINDEX` (-10002) resolving to state + `0x90`. Independent
wowdump reads then verified thread tag 8, globals tag 5/secret 0 and the complete
named `LycheeDevInternal.Mailbox` publication path, including current wire and
actor Aotu / Aotu, `Player-4618-01188FA2`. The read used 135 calls / 3,148 bytes;
exact raw-byte replay passed. No connection or input was used for this research.

The researched runtime site is RVA `0x75a69d`, 49 bytes, SHA-256
`f04a6ad4b769b3e1dc12e72dee586d58d3c87f17097c66a60244a3da5f1de46c`.
The source text capture remains partial; this is an exact-build semantic site,
not proof of full-text uniqueness. Production support must check the original
executable identity, complete current site, section targets and complete Lua
path, then verify reload and live execution separately. These later facts
supersede the earlier *unverified root* boundary without deleting failed reads.

An independent final review also found that the old diagnostic `Snapshot`
returned writable private protocol tables. That alias could affect pending
challenge/function/envelope state and was removed by detached, bounded
fact snapshots and private envelope copies. Four-profile functional negatives
failed before the fix and passed afterward: changing snapshot or caller-owned
prepare data cannot authorize a forged commit, change execution budget or swap
the original function. The valid challenge/prepared nonce still execute the
original result once. Live tests below use the rebuilt candidate containing
that fix, rather than inherit the earlier passing binary.

## Preceding sealed candidate acceptance

The rebuilt development CLI SHA-256 is
`3ebce6f071af6a35053d37639a86d511b57e23561b9e4e6f227d9e8177e229fc`.
SlotProtocol SHA-256 is
`3485e5f6a1953e5dcb14d6a00b98ed033ebd5ecfab1660965fad21075f04830a`.
All three installations were upgraded through managed install and the new
runtime activated explicitly. The new Forever binding reads 706 module bytes
in 18 calls, including 296 locating header bytes, scans zero text and rechecks
the complete 49-byte site and root. Full named-publication reads and byte replay
also passed after reload, with new Lua state/globals pointers rather than
retained heap addresses.

| Sealed suite | Verified result | Closed connection |
| --- | --- | --- |
| Retail core | 13 steps passed | `CON-0831924dacf5c795ff3fc9131f5dbe6a` |
| Retail capacity | 54 steps, including all 47 ordinary requests, passed | `CON-9d4f2ec0a75dd2de372ec4531e9f0e4a` |
| Classic core | Initial activation observation limit retained; same-CON resume bound the original runtime, then 12 remaining steps passed | `CON-518d3178a70bb522b18ad2320efe24b5` |
| Classic capacity | 54 steps, including all 47 ordinary requests, passed | `CON-d8246f26caff3366535543f2e0a05b08` |
| Forever core | 12 semantic checks plus two continuation reads completed; normal/large and cache-off execution passed | `CON-7e8cff11955a075d218011a77aef5d99` |
| Forever capacity | All 47 original requests and capacity/history/error/bugs/disconnect chain completed through recorded continuation recovery | `CON-b12a1708ca09a92c5798f362a47a8631` |

All test connections are closed with no outstanding cleanup. Forever's initial
activation connection `CON-3c1c3bf9b8cc2977ab603ebd5bdbcb9d` was also resumed
and closed before core testing. Root recipe and capability are independently
saved and read-confirmed in wowdump for its actual catalog label
`classicbeta@1.60.1.70124`, the same executable/hash that lycheedev names Forever.

These results include recovery; they do not claim every first invocation passed.
Forever pressure `normal-20` first returned a short-read error at `commit_ready`.
Its input gate recorded `not_sent`, zero queued messages; exact-CON resume then
submitted the original constrained commit and completed it. The first failed
pressure report is retained separately. Later intermittent short reads at
normal-35/39/45/46 and bugs also used the original request/connection continuation.
Historical request reuse was read-only; no unknown submitted business input was
replayed. Missing low-level read location/cause in those original records cannot
establish whether the failure arose from heap turnover or module mapping changes.

Expected compile failure remained a verified business report. Expected cleanup
failure retained `resourcesReleased=false`, used verified runtime destruction,
then completed cleanup. Each capacity chain verified actual runtime change and
history rotation. Complete structured reports are retained in the
[acceptance evidence directory](research/mailbox-acceptance-20261001/README.md)
and client research bundles; initial failures remain alongside successful recovery.

The full rebuilt baseline passed at `2026-10-01T03:48:34.671Z`: all eight checks
including source-unchanged, build/vet, 2,862 named Go tests in 42 packages,
94 Node tests (zero skipped), mandatory Lua 5.1 and real LuaLS integration.
Frozen source fingerprint was
`1ffdc08070ab15bc25487a83dd1e3ed2829b0937dcf8d03cf3cd28b137c455a2`.
Subsequent edits append these evidence reports and documentation only; production
source and tested candidate bytes remained unchanged for this checkpoint.
No release was published.

## Bounded diagnosis of the mapping guard

A separate 33.083-second read-only diagnostic used the production module and
Mailbox reader with wrappers recording the original read result. It ran 24
identity/input lookups, 2,182 read requests / 50,092 bytes, zero region scans,
zero game input and zero new connections. Four lookup failures occurred at
the first 49-byte module anchor check, before any heap read. All four underlying
causes were `memory.module_mapping_check`, `stage=before_read`,
`changed_region_end`: the issued VirtualQuery run was 90,112 bytes and the
current run 61,440 bytes. The requested offset 22,173 plus 49 bytes remained
fully covered, with the same start, allocation, type, MEM_COMMIT and protection.
Twenty lookups succeeded, including later lookups through the same issued object.

This reproduces a specific false refusal caused by treating the complete
VirtualQuery run end as permanent object identity. It is not a partial
ReadProcessMemory result or a demonstrated heap relocation. Original pressure
errors lacked that low-level cause, so this does not retroactively prove every
earlier error had the same cause. The old read wrapper also discarded the
underlying error and labeled any failure `short_read`, obscuring diagnosis.

The correction proves coverage of the actual requested pieces inside the
original issued plan, retaining origin/allocation/type/state/protection and
before/after guards. It admits no originally missing/ineligible bytes. Changes
cutting into the requested piece still fail; exact anchor/hash/header and all
Lua path guards remain unchanged. Original read errors are preserved. This is
a mapping proof correction, not a blind retry or broader authorization.
It required a fresh candidate and separately recorded regression/live run.

The intermediate span candidate (`afade381894940c84a5583899c52e973ef2503914c1785218b7f668a5c0711c6`)
passed the full baseline at `2026-10-01T04:12:17.270Z`, including 2,879 named Go
tests. Retail and Classic core/capacity checks passed. Forever core passed, but
the pressure evidence contains first-call failures at normal-8 and normal-41,
each recovered on the original connection. The earlier progress claim that
normal-1 through normal-27 all initially passed was incorrect; report inspection
found normal-8's continuation. All original records remain preserved.

Those errors now retain a precise cause: querying the old issued start returned
a prefix ending *before* the requested anchor. At normal-8 the prefix had become
PAGE_NOACCESS, which does not describe the later requested page. At normal-41
the prefix remained readable but ended before the anchor. Neither query could
prove that the actual requested interval had changed. Keeping the old query
origin was therefore another unnecessary dependency on unrelated page geometry.
The complete correction queries the actual requested location and composes
coverage from the original issued/current/requested intersections. It retains
original allocation/type/state/protection, old-hole refusal and both IO guards;
it does not treat VirtualQuery run beginnings or ends as allocation identity.

## Final requested-location candidate

The final development CLI SHA-256 is
`9b4846015f9a22d64994b494be8c0412eace70c0fbdd24344239283bc125f7a8`.
It includes the complete requested-location mapping proof and original-cause
preservation, the two root templates, detached private protocol state and the
latest-only passive input diagnostic. Production source remained frozen during
these checks; the following evidence additions do not change that source.

The full offline baseline passed at `2026-10-01T04:27:11.407Z`: build, vet,
2,888 named Go tests in 42 packages with mandatory Lua 5.1, 94 Node tests,
version/skill/generated-command checks and unchanged source fingerprint
`84635a8868d67d3f08e732c06cff6b0869cce68e2154d657c85dd3c49bceebf9`.
Real LuaLS integration passed. Environment-specific/helper/manual skips remain
explicit in the raw report; these counts do not assert that every Go test ran.
The offline runner's real-client entries remain `not_run`; the separate real
client evidence below does not rewrite those offline entries.

| Client | Final candidate real-client result | Closed connections |
| --- | --- | --- |
| Classic | Core 13 steps and capacity 54 steps including 47 ordinary requests; all first invocations exit 0, no recovery | `CON-663500f910df2fc23c0bbe6b424b9555`, `CON-dd51b69cd9f2e4041aabd0b6796fd39b` |
| Forever | Activation, core 12 steps and capacity 54 steps including 47 ordinary requests; all first invocations exit 0, no short reads or recovery | `CON-2116b05fd563bbe7cc625dfe9f4ddc5d`, `CON-039af23c4067d76eab1e0bd847394dd8`, `CON-05fd4f99d8f5d03589de337f192b67fc` |
| Retail | Core 13 steps completed after exact-CON activation continuation; final capacity 54 steps including 47 ordinary requests all exit 0, no pressure recovery | `CON-f7cfd8cf8cf44e02c07eac6af8febd15`, `CON-ba87711a9a9cb76c3385b18c4f3e87a3` |

These final counts come from complete JSON reports, including examination of
every nonzero exit code, rather than truncated progress output. Root queries,
public named Mailbox reads and captured-byte replay independently passed for
Forever after the pressure run. All three clients finished with complete cleanup.

Retail's final-candidate activation was interrupted by `lab.key_already_held`:
9 messages had already been queued, so the original reload input remained
`uncertain` and was not replayed. The owner subsequently identified an external
script continuously sending spaces, and supplied a screenshot of the affected
chat command. This explains an actual source of conflicting input; it does not
establish the cause of every earlier receipt miss. The original CON and raw
failure remain preserved. Subsequent named INPUT reads observed new runtime
`0000001a0001050d505341dec3d8119f`, but did not establish the two-sample
replacement proof before the original PID exited. Public disconnect then
closed `CON-2eda9cdcf05088ce5c1358e2c22ee994` with `processEnd=process_exited`,
without input; status confirmed it unbound and closed. An OS query restricted
to the exact real Retail executable path found no replacement process.
At that checkpoint, final Retail core/pressure acceptance was incomplete and
required a fresh login. Closed process cleanup does not turn the interrupted
activation into a successful test. Later independent successful acceptance is
recorded below. Titan, a first clean installation, unattended relogin/fault-injected
restart coverage and future Lua ABI reuse remain unverified. No release was published.

On a later login, exact-path selection fixed the real Retail process to PID
448, native creation `134353034288690796`, with the same executable hash.
The initial runner's missing host project directory returned exit 2 before
any session or input; that report is retained. The corrected runner created
`CON-da201076541bff284bf253447c205a0f`, whose connect waited for an optical edge
and exhausted its original budget without attempting physical input.

Independent root and public Mailbox reads passed, but eight INPUT observations
had identical sequence 51 and sampleMillis 67819300. Bounded WGC initialized
and captured zero frames in four seconds. The OS reported the exact window
visible, not minimized or cloaked, and all 108 process threads suspended,
with zero CPU advancement in one second. This establishes process suspension;
it does not identify which tool or actor suspended it. StartupBeacon.Stop
does not stop the independent InputState sampler, so its 45-second lifetime
cannot explain this observation. Public disconnect retained a Closing intent
under ineligible/short reads; the original connection and deadlines remained
preserved. The owner paused this work, then reported the game online again;
recovery and further final acceptance use that original connection first.

On resumption, PID 448 had exited. Public disconnect closed the original
pending CON with `process_absent`, preserving its no-input history. The unique
exact-path replacement was PID 60944, native creation `134353066099007409`,
with the same Retail executable hash. Its INPUT publications progressed at
one-second intervals. The current actor was 荔枝小月亮 / 燃烧之刃,
`Player-829-03DAABDF`; the owner's existing arbitrary-character authorization
allowed a new independent connection after all previous connections were closed.
No old operation or identity was transferred to that character.

The fresh connection `CON-f7cfd8cf8cf44e02c07eac6af8febd15` connected in
1.608 seconds. Activation reload first encountered `memory.ineligible_read`
after its input had been submitted. Exact-CON resume verified the new runtime
without resending that input. One temporary host recovery runner compared its
current read-back runtime with itself and failed an assertion; the original
connect/runtime evidence was then used correctly. Its same-project connect and
same-request reload read-backs did not create another connection or issue
another reload. The failure reports and reuse provenance remain preserved.

All 13 logical core checks completed, including independent actor API matching,
normal/large payload, cache-off, byte-identical execute/reload/disconnect retries,
and post-reload execution. The final independent pressure connection
`CON-ba87711a9a9cb76c3385b18c4f3e87a3` completed all 54 calls with exit 0,
including 47 ordinary requests, capacity runtime rollover at normal-46 with
normal-47 continuing on the new runtime, retained-history
retry/rotation, verified compile error, cleanup error with runtime destruction,
bugs and disconnect. This final pressure run required no continuation recovery.
Both final connections are closed. Retail's core result includes activation
recovery, rather than an all-first-invocation-success claim.

The original immutable Retail recipe/profile/binding/complete JavaScript reader
was then queried in the new same-build process. Root profile and confirmed
library reads passed; eight complete public-reader observations replayed with
eight matches and zero mismatches. A new `read` confirmation with its copied
evidence was appended to wowdump, leaving the recipe SHA-256 unchanged.
The initial report-copy size refusal is retained; only the explicit large-report
confirmation counted as successful. This verifies complete-reader reuse in a
second process of this exact Retail build, without promoting formal semantics,
formal reuse or future ABI support.

The [final Retail summary](research/lua-mailbox/current-final-candidate/pid60944/summary.json)
and raw reports are sealed in the Retail research bundle alongside all previous
failures. Final bundle seals verified 187 Retail, 188 Classic and 89 Forever
assets. The source commit `072061388c9b4a3c17b3b6a527cdedc72cc6f1e5` passed
all 12 Windows CI checks across push and PR runs, including required aggregation.
The follow-up changes append documentation/evidence only; no production source
changed during final live verification and no release was published.

The original 185-asset Retail seal is retained as a checkpoint. An overlay-only
diagnostic Go source is archived with a `.go.txt` suffix, preserving every byte,
so Go does not mistake evidence for a standalone package. The original seal and
an archive-format explanation are appended, bringing the delivered count to 187.
The sealed capture log is explicitly included despite the ordinary `*.log`
ignore rule. Staged Git blobs are checked against every delivered asset digest.
