# Bounded shared-slot admission prototype

This package contains **test-only Go and Lua prototypes**, not a shipping protocol or a CLI feature.
The Lua adapter follows the current v2 single-envelope engine and transport metadata.
The collection schema and frozen 64-slot Go model remain historical experiments;
neither is the adopted 200-slot routing protocol.
Run `go test ./tests/slotset -count=1 -v`. No game input, installed addon files,
production connection journals, or production publisher are changed.

## Frozen experimental capacity

The model admits at most **four stable process-lifecycle consumers**. The host
must derive each consumer from PID plus creation time; it is not a CON ID or
runtime token. Across the entire 64-slot pool, each admitted consumer gets:

- One unresolved data member, at most 1 MiB + 4 KiB of generated Lua bytes.
- Two unresolved control members, each at most 4 KiB of generated Lua bytes.

The data limit covers the existing 256 KiB code maximum with worst-case Lua
decimal escaping. The control reserve covers an unknown old bind plus a new
bind during one recovery. Another unconfirmed recovery exhausts only that
consumer's reserve. Runtime changes and repeated admission do not renew quota.
This is not a claim of recovery from unlimited reloads, or a final product
instance limit. The model uses the real `bridge.SlotPayload` encoder to measure
member sizes, not the unescaped code length.

The measured worst-case fixture is 12 members and **4,199,960 bytes** before
collection framing. The existing production publisher reads a slot with a
2 MiB bound. Therefore this capacity cannot be plugged into the current reader
unchanged. A production format must enforce an explicit total serialized bound
(for example the sum of these quotas plus a separately bounded framing budget),
and its increased Lua allocation and file verification costs must be measured.
Reducing normal code capacity or admitted consumers is a product tradeoff,
not something this prototype silently does.

## What the tests establish

- Serialized publisher decisions preserve A's immutable unknown member while
  admitting B and A's fresh runtime into the same physical slot.
- The member digest stays valid when another consumer changes file generation.
- Reusing a nonce with changed bytes fails; exact late retirement cannot remove
  another runtime or consumer's member.
- A consumer cannot spend another admitted consumer's member/byte allocation.
- Known runtime collisions across consumers fail closed, even across slots.
- An executed LoD payload consumes the slot on no match or ambiguous match;
  later file edits cannot cause that slot to execute again in the model.
- A late wake still advances nextSlot. The test exposes this unresolved
  transport behavior; it does not authorize input replay.
- A tiny write-ahead model uses the existing `vault.ReplaceFile` primitive on
  real temporary files. Interrupted merge and removal converge at intent-written,
  payload-written, and intent-cleared boundaries, preserving foreign members.
  Unknown file bytes fail closed and are not overwritten.

## Admission gates still open

**Do not promote this prototype on the strength of these passing tests.**

1. The Lua 5.1 prototype now executes the real SlotProtocol and SlotRuntime
   modules with a simulated file loader (details below). Actual
   `C_AddOns.LoadAddOn` loading state, disabled behavior, reload/relogin, and
   each pinned client remain unverified. A Lua parser exception demonstrably
   precedes Receive in the current runtime; production does not yet implement
   the prototype's explicit failed-attempt consumption boundary.
2. Known collision rejection is insufficient for an undiscovered process with
   the same runtime. The addon cannot validate Windows PID. Cold-start routing,
   initial binding in the fixed process, and collision/ambiguity handling need
   an explicit production contract and experiments. No random-token uniqueness
   claim follows from these tests.
3. The persistence experiment serializes one JSON model snapshot. It does not
   test the production Lua payload + installation manifest two-file commit,
   actual publication locks, input timing, power loss, disk corruption, or
   bounded read/parsing of untrusted journal bytes. Those need their own tests.
4. Retirement is supplied with already-verified evidence in this model. It does
   not prove that descriptors or caller-provided identities authorize GC. Real
   GC still needs the exact receipt, fresh binding, or OS process-end evidence.
5. Four maximum-size data members require a larger read budget than production
   allows today. CPU, Lua heap, file merge time, and lock duration are not measured.

## Version and migration proposal

Keep the prototype schema `lycheedev.slotset.prototype.v1` outside production.
If the remaining gates pass, introduce an explicit new collection/pool schema
and loader capability; do not reinterpret `lycheedev.slot.v1` files silently.
The inner prepare/commit challenge contract may remain unchanged, but both
publisher and loader must agree on the new outer format and capacity policy.

Migration requires exclusive installation maintenance and inspection of **all**
processes sharing that installation. Finish or retain v1 unknown transactions
under the v1 recovery path first; do not translate unknown consumption into
success or wipe their files. Switch payloads, manifest and loader together only
when no running v1 loader can consume new-format slots. Record disk version and
each loaded runtime version separately. If old live instances or unknown v1
reservations prevent safe migration, return that concrete blocker; the model
does not invent permission to restart clients or erase evidence.

The accepted model is an argument for implementing and testing a real prototype,
not an implementation of shared-installation fault isolation.

## Real Lua 5.1 loader experiment (2026-09-29)

`lua_test.go` follows the repository protocol runner's interpreter policy:
`LYCHEEDEV_LUA51` selects Lua 5.1; `LYCHEEDEV_REQUIRE_LUA51=1` makes absence
fatal. `loader_prototype.lua` uses actual `SlotProtocol.lua`, `SlotRuntime.lua`,
`MemoryProtocol.lua`, and `CaptureWriter.lua`, with a simulated LoD adapter.
The verified run used Lua 5.1.5 and passed all six top-level Go tests (including
this Lua test) plus the six persistence fault subtests.

- Two independent real protocol engines parse identical collection bytes and
  bind only their unique runtime member. Neither compiles nor executes business
  code embedded in the members.
- No match, ambiguous match, invalid outer format, and actual Lua syntax failure
  each consume the prototype slot. A corrected file cannot retry that attempted
  index. The prototype loader records attempted before parsing and deliberately
  calls `Receive(index, nil)` on failure, which uses the real engine's existing
  nil-envelope consumption rule (`slot_skipped_empty`). This is an experimental mechanism, not a
  production receipt or permission to fabricate a successful bind.
- The current single-envelope Receive rejects the collection schema, consumes
  its engine slot, and executes no business code. This tests its callback
  rejection, not safe coexistence of old and new installed loaders.
- The actual current SlotRuntime, with LoadInputSlot throwing on invalid Lua,
  returns `slot_wake_failed`, releases its input lease and Receive capability,
  but leaves `consumed[1]` unset. A second wake attempts that same index. This
  demonstrates why a production collection design must handle failure before
  its callback; the Go model alone could not establish this gap.

The parser here accepts only test-generated Lua through an empty environment.
An empty environment is **not a bounded parser for hostile Lua** (for example,
loops still run). Neither serialized framing validation nor production-safe
parsing is implemented by this fixture. The runner has a 20-second process
bound; that is a test harness safeguard, not a shipping loader budget.

The experiment does not establish whether WoW marks a syntax-failed LoD addon
as loaded, and does not solve dual-file payload/manifest atomic publication.
The 4,199,960-byte worst-case Go fixture still exceeds the existing **2 MiB**
production reader. No production format, capacity, or read limit was changed;
real LoD lifecycle, parser bounds, publication crashes, and measured CPU/heap
cost remain migration gates.
