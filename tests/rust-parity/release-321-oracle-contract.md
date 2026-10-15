# Current Go 3.2.1 machine oracle, stage D

## Identity and scope

Current oracle is released Go 3.2.1 peeled commit
`10b561db5706702cd42e047eaeed4ccb5e825351`. The separately named
`go-3.2.1.manifest.json` authenticates 84 command definitions, seven full delivery
profiles, 55 addon resources and 361 Go test-source files from immutable Git
objects. It does not claim those test sources have been translated or executed.

The previous `oracle.mjs`, `oracle.test.mjs`, `run.mjs`,
`go-3.2.0.manifest.json` and old addon-delivery test remain byte-for-byte retained.
Their results stay tied to `3057a9666ab078270b63c7f06de748069b0d1f4b`. A Retail
upgrade is no longer assessed exclusively by the previous release's gate.

### Current source-derived authority

- [Command contract](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/command/command_contract.go):
  SHA-256 `b56c73427279a89196dcdebc720827e4eb4bc79f55f0d76f7561d49f69eed9fa`.
  The historical fixture first independently authenticates against 3057. Then
  complete current command-source bytes and SHA-256 must equal the old source.
  Product names and ordering must also match before reusing the reviewed command
  adapter (including the data-product flag). No old result is relabeled.
- [Current profiles](https://github.com/Follen/Lychee-Dev/blob/10b561db5706702cd42e047eaeed4ccb5e825351/internal/selection/products.go):
  SHA-256 `53921c7bf0adcfde2b39c467ec480a80e70c1e1fa562edc2450c75f19a1d9bb2`.
  A bounded literal parser reads the seven rows only after whole-source hash
  verification. Comparison includes product, family, delivery code, folder,
  series and reused-slot flag. Retail is 12.1.5; both Forever regional products
  remain distinct. Stage A separately checks verified API/build baselines.
- [Addon tree](https://github.com/Follen/Lychee-Dev/tree/10b561db5706702cd42e047eaeed4ccb5e825351/addon):
  every current path, length and hash is collected from fixed Git blobs; extra
  candidate files remain failures. No generated fixture copies Lua into payload.
- Go test inventory uses the same fixed commit. Its 361 files are references,
  not compilation, Windows, Lua, gameplay, coverage or performance evidence.

## Exact comparison and authorized differences

`run-3.2.1.mjs` emits both `rawExact`/`rawExactStatus` and
`accepted`/`acceptedStaticStatus`. Acceptance never mutates or hides raw evidence.

1. Exactly the existing two approved engine-description changes are recognized:
   `doctor` LuaLS wording to embedded Emmy worker smoke, and `source batch`
   request-scoped LuaLS to native Emmy session. Each match requires exact command
   path, original summary and replacement summary. Flags, ordering, mutation,
   arguments and all other fields still match exactly. These constants are the
   already reviewed `APPROVED_ENGINE_SUMMARIES`, not a new wildcard adapter.
2. No release bump was authorized. Only `addon/Core/Runtime.lua`'s exact
   `ns.Release = "3.2.1"` to `ns.Release = "3.2.0"` and `addon/Lychee Dev.toc`'s
   exact `## Version: 3.2.1` to `## Version: 3.2.0` are accepted as retained
   candidate metadata. Each exception also locks the entire expected and
   retained file's SHA-256 and byte length. An extra byte, unrelated edit, other
   path or invented manifest rule fails. The report exposes both identities and
   exact text substitutions. All other addon bytes remain exact.
3. Folder/family/reused-slot mismatch, missing/extra command or addon, parser
   arity change, modified default/flag display and unreviewed contract filter
   remain failures or explicitly unverified. There is no `--accept-gaps` mode.

The fixture records the approved release identities, but the comparator uses its
reviewed immutable constants rather than trusting a caller-supplied allowlist.
Changing manifest rules cannot authorize an additional candidate difference.

## CLI and Skill routing

`tools/rust-cli-smoke.mjs --contract` defaults to `loadCurrentFixture` and the
new `compareCurrentDescribeContract` entrypoint. It still verifies the sealed
binary before and after execution, retains all output and process ownership
receipts, and reports the current oracle/fixture identity in both the contract
and outer smoke report. The historical comparator remains 3057-only; old
injected checker fixtures exercise that historical path without being renamed.
Controlled Node injection is not execution of a Rust binary.

Skill target rendering and command recognition use the current manifest and
10b identity. Generated `references/commands.md` changes only its target comment;
command definitions and established Emmy adaptations are preserved. Historical
Skill checker tests may still explicitly supply the previous command fixture,
whose command source is unchanged. This does not move their historical pin.

A separate `workflow.patch` is a coordinator-owned proposal, excluded from this
candidate's owner patch. It independently authenticates both fixture generations,
runs old runner tests plus current checks, switches the actual static report to
`run-3.2.1.mjs`, and changes the Skill target to the current fixture. It also
references separately reviewed A/B2/C increment tests. No workflow or remote state
was changed by this worker. Actual CI execution remains NOT_RUN.

## Observed current candidate result

The current report is **raw exact FAIL; accepted static FAIL**. Command metadata
has exactly the two approved descriptions; seven complete profiles and parser
flags match the current target. The two retained addon release fields are
explicitly recorded. The remaining failure is **21 extra historical addon
files**, including old Native runtime modules and `Lychee Dev Rust.toc`.
They are not filtered or silently admitted. Their consumer/retention closure is
an independent coordinator-owned task. This stage fixes oracle routing, not those
payloads. Existing release/package consistency failure is also not waived.

## Validation boundary

Node checks authenticate fixed sources, test deliberate positive/negative
comparisons, exercise the injected smoke checker and check Skill references.
They do not compile Rust, install a hook, run a game or prove complete business
parity. The current CLI executable and workflow must still run through the
repository's controlled official-tool runner. No bare cargo/rustc, cache adoption,
release change, network fetch, main-tree source edit or publication occurs here.
