# Rust candidate boundaries

This candidate follows the fixed Go 3.2.3 Skill at
`3766f9cacb52e93a803a626187e2a1d54a445468` and complete Lycheedev mailbox
protocol v1 contract. The ordinary [Skill entry](../SKILL.md) and
[source research](source-research.md) are the workflow; this page records only
migration differences and current gaps. It does not define a reduced toolkit.

Use the explicitly selected candidate binary and its actual `describe --format
json` / help. Do not fall back silently to another CLI, release, source revision
or input protocol. The checked-in [command directory](commands.md) is the fixed
Go target with engine wording adapted, not proof of candidate availability.
A command absent from the selected CLI, an unsupported profile, or a failed
check remains a concrete gap. Preserve useful fixed-source evidence and the
structured error; do not invent flags or repeatedly retry an unchanged failure.

## Engine difference

Rust links EmmyLua parser/analysis libraries directly. Bounded native workers
and optional sessions own analysis, cancellation and resource retirement; there
is no requirement to install or start LuaLS for this candidate. Structural
mapping, Lua 5.1 `luac -p` compilation and Emmy semantic analysis remain separate
evidence. Never execute studied Lua or repository configuration. Preserve the
actual engine identity, source/environment pins, original locations, captures,
coverage, truncation and unresolved edges returned by each command.

## Open acceptance gaps

- The `live doctor` and top-level selected-target diagnostics, including repair
  and startup recovery, are implemented but still require complete final-candidate parity evidence. Use the
  [startup contract](live-startup.md) as the target; missing diagnostics never
  authorize raw keys, manual claim edits or another transport.
- `source batch` still requires the full fixed-Go request/file limits, ordered
  query/refs/context results, shared preparation, deadlines and error behavior.
  Its documented workflow remains required; a missing handler does not make
  the batch contract optional or authorize a fabricated successful batch.
- Source full-repository query, refs, context, flow, validation, worker/session
  lifecycle and output quality are not yet accepted. Small fixtures and command
  listings cannot establish correctness, complete coverage or performance.
  Full-Retail ordinary refs/context/API have successful candidate runs; retained
  sessions still have a cold-start failure and cannot supply warm/steady evidence.
  Keep the required 60-second idle, original lifetime/batch bounds and exact-run
  evidence distinct; a previous component pass is not final acceptance.
- An independent agent must exercise this Skill against the real candidate CLI
  with fixed original sources and retain the actual call/output trail. The
  author and a static reference check cannot provide that acceptance.
- Real-game or user-assisted activation, input, recovery and client coverage
  remain unverified and are tested only after independent offline/automated
  Windows checks. Source success and installed slot files do not establish
  live readiness or authorize game tests.

For source research, keep the existing snapshot and use the ordinary
[bounded recovery](source-preparation.md#recover-without-changing-the-revision).
A query miss is not permission to replace the pin with guessed GitHub raw URLs.
When another capability is unavailable, report only the gap that affects the
answer; do not discard a signature or original source location already read.
