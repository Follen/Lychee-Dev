# Transport feasibility lab (not production)

Retail observations and limits: [experiment report](../../docs/toolkit/transport-experiment-2026-09-28.md).
This lab does not replace the production bridge. External memory access is read-only via installed wowdump.

## Offline

From the repository root, create `.tmp/transport-lab/fixtures`, then run:

```powershell
lua tests/transport-lab/offline.lua .tmp/transport-lab/fixtures
node tests/transport-lab/fixture-test.mjs
go build -tags lycheedev_input_lab -o .tmp/transport-lab/host.exe ./tests/transport-lab/host
```

The Lua fixture creates all six phases. The Node suite verifies valid payloads and rejects corruption/truncation (33 assertions).
The host is build-tagged, checks the selected process/window and existing production ownership, and only accepts fixed lab commands. It saves WGC before/after frames. Input submission alone is not execution proof.

## Live setup and bounds

Copy `addon/LycheeMemoryLab2` to the explicitly selected client's AddOns directory. For the inbox experiment, append `InboxController.lua` to its TOC and precreate four directories named `LycheeInboxLab01` through `LycheeInboxLab04`, each containing:

```text
## Interface: 120100
## Title: Lychee inbox lab slot
## LoadOnDemand: 1
## Dependencies: LycheeMemoryLab2
Inbox.lua
```

Name each TOC after its directory. Initially write `_G.LycheeInboxLabPayload = nil` to Inbox.lua. Reload through the production CLI. `/memlod status` must show four known slots and no occupied-key warning. The temporary binding is Ctrl+Alt+F9; it consumes one slot on key release, with a four-slot limit and combat refusal.

After reload, write `{ sequence=N, nonce="unique-value" }` to `_G.LycheeInboxLabPayload` in the next slot's Inbox.lua. Use the host with explicit `--pid`, `--installation`, unique `--out`, and `--command LOAD`. Check the matching chat receipt in the WGC frame. `/memlod replay` re-requests already-loaded slot 1 after clearing the payload, to test the load-once behavior. `/memlod stop` clears the temporary binding. Remove the controller TOC line after testing; the standalone slots have no startup execution.

## Memory workflow

Read the current run and phase from the game's WGC chat; do not infer freshness from memory hits.

```powershell
node tests/transport-lab/parallel-find.mjs PID 8 all > scan.json
node tests/transport-lab/census.mjs PID scan.json RUN PHASE > census.json
node tests/transport-lab/cache-benchmark.mjs PID census.json
node tests/transport-lab/scoped-find.mjs PID census.json > scoped.json
```

The scanner divides readable regions into eight bounded ranges and records each range's coverage. Incomplete coverage is not absence. Census double-reads and validates payload bytes, run, ticket, phase, length, checksum and trailer. It includes old valid candidates and read failures; only the externally confirmed current head selects reports. Cached-region misses require fresh discovery. `/memlab2 next` advances through phases 0–5; `/memlab2 reset` creates a new run; `/memlab2 clear` releases the lab's current references.

The cache benchmark measures eight concurrent reads of one immutable report, including wowdump process startup; it is not a latency guarantee or freshness protocol. No stable Lua root or cross-build recipe has been demonstrated.

Original live evidence remains in `.tmp/transport-lab/evidence/`, including full WGC frames; the report records exact target identity. Do not publish screenshots or raw memory evidence without reviewing unrelated on-screen data.
