# Public Lua Mailbox research assets

This immutable candidate bundle is `retail-lua-mailbox-v1-69933`. It reads only
`_G.LycheeDevInternal.Mailbox.identity` and `.input`. It does not read the old
Snapshot closure, enumerate heap regions, scan for records, read BODY, dispatch
input, or retain a heap address between calls. Retail 12.1.0.69933 public Mailbox
reads, same-process character change, and verified runtime replacement have now
been observed. Cross-process reuse of this complete reader and cross-build live
validation remain **not_checked**; the Go production resolver has its own evidence.

`root.anchor.json` has the same source build, executable/runtime `.text` digests,
instruction pattern, instruction codes and RIP resolution as the existing
`retail-lua-state-root-69933` investigation. Its new recipe ID is
`retail-lua-state-root-rip-v1`, matching the Go resolver. The original historical
release/input observations predate this protocol and are not new Mailbox evidence.
The root anchor is saved as `retail-lua-state-root-rip-v1` under
`C:/Users/follen/.wowdump/recipes/retail@12.1.0.69933/`; its immutable digest is
`11d48aa512fd890df85066f30b4ffd1a086d30c1b75dcab0eb59ad033d4ceb0e`.
The root profile is saved as capability `retail-lua-mailbox-root-v1-69933`, with
read confirmation and a successful named capability query. This capability
exposes only the root, thread tag and globals TValue, not the complete reader.
The complete sealed reader bundle, original sample recordings, replay output,
root queries and independent CLI actor probe are retained together in the
recipe confirmation evidence. Confirmation does not grant execution authority
or certify future Lua layouts.

`root.profile.json` is a candidate root/layout profile. Its three fields are:

| Field | Resolution | Meaning |
| --- | --- | --- |
| `luaState` | module + root RVA; read pointer | Current Lua state pointer |
| `threadTag` | dereference root; add `0x10`; read u8 | Must independently verify thread tag 8 |
| `globalsValue` | dereference root; add `0x90`; read TValue | Must verify table tag 5 and clear secret byte |

All three roots have explicit recipe bindings. Every numeric layout offset or
size is declared as an invariant with a mandatory revalidation rationale; tag
and hash assumptions are declared separately. These are assumptions, not evidence
that a future build preserves the layout. The profile only exposes roots and
layout definitions; its successful read does not establish the whole Mailbox path.

`reader.mjs` performs the complete named lookup and verifies Mailbox schema,
release `3.1.0`, nonzero runtime, `LYCMEM06`/`LYCEND06`, identity v2 and input v2.
It follows Lua hash collisions by exact key bytes and checks secret bytes before
using key/value pointers. Each invocation permits at most 512 physical reads and
1 MiB, including failed IO, wire/header rereads and all final path guards. Bounds
are 64 collision nodes, node log <=20, 256-byte keys, 64-byte metadata, 32-byte
runtime and 16 KiB identity/input payloads. Adler-32 validates payload and header
checksums. Root, state tag, globals TValue, table layout, key pointer/tags/key
bytes/hash/length, next links, current value pointer/tags and publication TString
tag/length are rechecked before any output. A changed path rejects the observation.
These checks do not produce an atomic snapshot or detect an ABA change.
Identity kind 1 uses zero nonce/ticket and permits sequence zero, matching the
production `SlotProtocol.Describe`. Input kind 5 uses runtime nonce, zero ticket,
and a positive sequence below `0xffffffff`.

The probe emits observations with `authorization:false`. It does not compare Lua
uptime with wall-clock time or establish the production 500 ms/after gate. It
does not implement Go's process creation identity or private-page permission
adapter. Those remain production reader responsibilities.

## Build binding and migration

The JavaScript reader is fixed to the binding exported at the top of `reader.mjs`
and recorded in `root.binding.json`: Retail `12.1.0.69933`, executable SHA-256
`d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd`, root RVA
`0x79c0c18`. ASLR is handled by the actual module base. This script does **not**
automatically derive a new RVA on another build. The current wowdump `ctx.module`
primitive exposes no executable digest. Before live use, the caller must verify
and bind the wowdump workspace/build to the matching executable digest. A present
`ctx.buildKey` mismatch is rejected; fixture selftest has no build context. Output
therefore marks the executable hash check `external_workspace_required` rather
than claiming the script verified a digest.

Use the actual wowdump 1.0.0 migration contract, with a complete new module dump
manifest and new output paths:

```powershell
wowdump analyze migrate --manifest <new-complete-manifest.json> --profile root.profile.json --recipe root.anchor.json --out-profile <new-candidate.profile.json> --out <new-migration-report.json>
wowdump profiles lint --profile <new-candidate.profile.json>
```

Relocation must return exactly one candidate; missing/ambiguous anchors must stay
unavailable. A migrated profile always remains candidate. Verify the new module
identity, root semantics, every layout/tag/hash assumption and the complete
current public Mailbox path before generating a new reader binding. Keep the old
assets unchanged. Do not copy the old root RVA merely because an executable hash
is new, and do not treat `unresolved:[]` as layout validation.

The Go native reader differs intentionally: it uses a bounded unique runtime
`.text` masked-RIP resolver and validates the target and current Mailbox path.
This asset does not promise JavaScript parity with that module-code resolver.

## Offline verification

`wowdump-describe.json` records the installed command contract. No unsafe-script
bypass was used. All memory bytes and actor names in fixtures are synthetic.

```powershell
node selftest.mjs
wowdump analyze selftest --script reader.mjs --fixture fixture.json
wowdump profiles lint --profile root.profile.json
wowdump profiles test --profile root.profile.json --fixture root.fixture.json
wowdump profiles test --profile root.profile.json --fixture root-wrong-hash.fixture.json
```

The Node suite covers fresh root/heap relocation and ASLR, new current publication
with valid old strings retained, real hash collisions, secret/tag/count/length
failures, wire/checksum/runtime/release/version/build mismatch, short IO,
root/value/key/structure guard changes, collision cycles, physical budget caps,
cancellation and driver-budget error propagation. A valid 61-node collision path
reaches exactly 512 IO calls and fails before issuing call 513. The successful
original wowdump reader selftest used 119 reads and 2,869 bytes; the corrected
production fresh-identity fixture uses 119 reads and 2,839 bytes.
The identity fixture is generated by the actual Lua `CaptureWriter`,
`MemoryProtocol` and `SlotProtocol.Describe`, using Lua 5.1 (`LYCHEEDEV_LUA51` may
select the interpreter). It verifies a fresh descriptor with sequence zero, plus
valid-checksum wrong identity nonce and zero/exhausted input sequence negatives.
The recorded RED rejected the production-generated descriptor as
`record_identity`; the corrected kind-dependent validator passes it. Source
digests and the independently generated header are stored in `selftest-report.json`.

The four KiB `migration-text.fixture.bin` and its complete manifest are explicitly
synthetic: the pattern was placed at RVA `0x1100`, with RIP target RVA `0x4000`.
`migration-test-report.json` shows that the actual CLI migrated all three root
bindings, left no unresolved layout values, and preserved all 25 layout
assumptions as `not_checked`. `root.migrated.fixture.json` is test output for
`fixture@lua-mailbox-migration`; it must never be used with a real client. To rerun
the migration, choose fresh output paths because wowdump refuses to overwrite:

```powershell
wowdump analyze migrate --manifest migration.fixture.manifest.json --profile root.profile.json --recipe root.anchor.json --out-profile <new-synthetic-output.profile.json> --out <new-synthetic-migration-report.json>
```

`selftest.mjs` regenerates synthetic fixtures and its offline report in this
directory. Run a copied bundle if the sealed validation reports must be retained
byte for byte. `asset-digests.json` seals this delivered candidate bundle and its
reports; it is not a live confirmation or a recipe-library registration.
