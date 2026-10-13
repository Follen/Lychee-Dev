# Isolated P3 lifecycle revision — executed / failed maximum-code lifecycle

This directory is a new test-only candidate. Original P3 sources, its positive
matrix, S01–S04 negative tests, and their sealed evidence remain unchanged.
The thin `native_revised_lifecycle` integration test and `run-rust.mjs` are the
only entry points. No addon, production Live path or game-input API imports it.
All execution is reserved for Root's governed Cargo window.

The previous prepared/not_run label is historical. The governed run produced
`.tmp/rust-supervision/native-revised-lifecycle-a3d04951fa2844c716b653e8e5010cac/result.json`:
`state=failed_maximum_code_full_lifecycle`, `finalProtocolPassed=false`.
The prior positive scoped matrix passed and S02/S03/S04 revision assertions
ran for both transports, but complete MAX_CODE failed `observation_expired`
for both. This remains a business-test failure, not an acceptance pass.

The outer failure is separately preserved in
`.tmp/rust-supervision/native-p3-revision-build-9a8ae261-094e-4551-a501-7101ac158905/failure.json`
and receipt `386ba33b-439e-4496-a0ea-b28c82199f83`. Cargo exited 101; unreviewed
VCTIP auxiliary retirement then caused `owned.forced_tree_termination`.
Do not reinterpret the maximum-code failure as only a wrapper failure.
P1, clock/B1, installation-wide 500-slot allocation, OS carrier, independent
final audit, unexposed-intent restoration liveness and real clients remain
unaccepted, as listed by the preserved result.

## Carrier limit and maximum-code acceptance

MAX_CODE remains 262144 bytes. Canonical RUN is 34 + 2 × MAX_CODE = 524322
bytes. P3S1 is 176 + 2 × canonical wire = 1048820 bytes. The Lua `return`
file adds 10 bytes, so host source limit is exactly 1048830. Lua once-loader
limits the returned stage string to exactly 1048820. Both reject the next byte.
The RUN decoder has the exact canonical maximum and still rejects invalid
code/budget fields. Existing SHA256 input bounds cover the canonical maximum.

The regression test separately exercises exact/over-limit carriers and the
complete MAX_CODE RUN → real BODY → durable save → RECEIVED → cleanup path for
two/four transports. The full test preserves the original nonce origin and
750 ms qualification window. Pure Lua authentication of the maximum command
may exceed that window. Each transport records that actual failure and the
overall test fails: carrier acceptance alone cannot mark S01 passed. There is
no timing exemption or post-receipt reconstruction of the attempt origin.

## Independent exact RECEIVED observation (S02)

Signed business state changes from LDBC1 to **LDBC2**, with HMAC domain
`lycheedev.lifecycle.state.v2\0`. Existing fields 0–28 retain their positions;
29 = most recently accepted RECEIVED number, 30 = its logical digest,
31 = its outcome, 32 = cancel deadline, 33 = cancel attempts, 34 = MAC.
The decoder rejects old/mixed versions, invalid field shapes/bounds, impossible
zero/receipt/completion combinations and inconsistent last-command matches.

The private receiver retains exactly one latest RECEIVED entry independently
of lastCommand and the bounded duplicate-history map. Late duplicate RUN or a
later CANCEL may change lastCommand but cannot erase that receipt. A matching
local RECEIVED intent binds its digest to this binding, task and result. Only
an exact observed retryable outcome authorizes a *new* durable RECEIVED intent.
The saved BODY must still match actual bytes, task identity and authenticated
current result. The existing four-RECEIVED, close/clear attempts and original
cleanup deadline remain enforced. High-water alone grants no such authority.
Unknown input waits with a valid uncertainty witness; missing/corrupt witness
without a live exact preparation capability quarantines.

## Preparation ownership and persistent uncertainty (S03)

`create` atomically creates a new journal directory; an existing directory is
rejected with `journal_exists_use_restore`, including an empty partial journal.
`restore` validates existing intent bytes and never grants them new preparation
capabilities. Normal and restored calls use the same `advance` implementation.

An instance records each exact command in a bounded `Ready`/`Consumed` map tied
to its immutable binding and digest-checked journal. A newly persisted intent
receives one preparation capability. `mark_business_input_pending` consumes it
*before* any fallible validation or persistence. Removing a pending file, a
failed pending write or failed authentication cannot regenerate that capability.
The witness includes the full binding, number and digest and is checked as
exact bytes, not merely by existence. Explicit test duplicates need both an
authenticated exact acceptance and their existing witness; neither high-water
nor a witness alone grants duplicate exposure.

Every journal scan checks the initial RUN, contiguous numbering, and every
intent already known by that instance. Lost/changed known entries quarantine;
restore also rejects detectable gaps. Missing durable BODY or a BODY without
its metadata quarantines before RECEIVED. An entirely erased unexposed suffix
cannot be reconstructed from an untrusted filesystem alone; no claim of
arbitrary corruption recovery or power-loss directory durability is made.

**Remaining liveness gap:** restore of a genuinely unexposed intent lacks the
ephemeral capability and conservatively quarantines. This is an explicit tested
limitation, not a claim of automatic recovery at all interruption points.
Filesystem reparse races, multi-host concurrency and arbitrary journal
rollback remain outside this fixture's claims.

## Phase resource contract (S04)

The original receiver mixed cancellation and terminal cleanup under one shared
four-attempt/5-second allowance. This revision deliberately changes that total:

| Phase | First deadline start | Maximum side-effect attempts |
| --- | --- | --- |
| Cancellation / timeout stop | First cancellation or timeout-stop processing that can establish a clock | 4 total, including timeout stop |
| Terminal resource close | First valid exact RECEIVED for the actual terminal BODY | 4 |
| Public BODY clear | Shares that same terminal cleanup deadline | 4 |

Both phase windows remain 5000 ms. A retry never starts either deadline again;
the immutable execution deadline remains RUN-start + requested budget. Cancel
failure cannot start or exhaust the later terminal phase. Resource close and
BODY clear share one original terminal deadline, so a clear retry cannot extend
it. At most **12 adapter side-effect calls** (4 cancel + 4 close + 4 clear) are
permitted per task; successful inspect may avoid calls. This is an explicit
increase over the old shared cancel/close allowance, justified by separate
execution-stop and result-release obligations; it is not described as unchanged
total budget. The driver additionally allows only four RECEIVED intents and
32 total intents, while Lua duplicate history remains bounded at 16.

An unresolved cancel still preserves execution uncertainty and ownership.
Actual late producer completion supplies the result. No false/nil/throw return
is treated as confirmed resource closure. Tests check cancellation deadline
expiry, timeout not resetting it, four-attempt exhaustion, later terminal
cleanup and exactly one eventual close effect / BODY clear. The full original
positive fault matrix remains included with the new wire and phase semantics.

## Run and evidence

From the worktree root, Root runs:

```text
node protocol/native-lifecycle-revision/run-rust.mjs
```

The runner uses the existing governed ordinary target, owner
`native-p3-revision`, and `test --locked --offline -j1 -p lycheedev-live --test
native_revised_lifecycle -- --nocapture`. Its outer timeout remains 300000 ms.
The actual test emits its evidence pointer and input hashes before the original
positive matrix or any new scenario. The runner snapshots sources and seals
actual inputs plus matching EXE/PDB even on a normal test failure. Test records
distinguish positive-matrix scope, carrier-only checks, complete maximum-code
outcomes and unresolved restoration liveness.

No successful result sets `finalProtocolPassed=true`. Independent audit,
private key / cold binding / reflection, cross-pause clocks/B1, shared 500-slot
allocation, true OS input, real-client acceptance and product/performance parity
remain separate obligations. This candidate has not been compiled or run yet.
