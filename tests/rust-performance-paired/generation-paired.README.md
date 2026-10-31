# Current Go 3.2.4 / Rust Agent generation pairs

The current Go reference is `29bdbdaff5f064ec212ff2064380b1ccac652eda`.
Current generation measurements use typed Agent query/Page/Capture with explicit
`data cache prepare`; the current entry accepts a request JSON:

```text
node tests/rust-performance-paired/agent-prepare-fixtures.mjs --csv-numeric
node tools/rust-data-generation-paired.mjs --prepare-importer
node tools/rust-data-generation-paired.mjs <request.json>
node tools/rust-data-generation-paired.mjs <request.json> --acceptance
```

Use schema `lycheedev.paired-agent-request-inputs.recovered.v1`, `mode: "generation"`,
absolute `artifact`, `goReference`, `importerReceipt` paths,
and the five machine-condition strings documented in the paired README.
`fixtures` has exactly `numeric`, whose fields are `directory`, `preparation`,
`preparationSHA256` from the fresh `csv-numeric` preparation. `goReference` is
the explicit external fixed Go 3.2.4 reference artifact directory (artifact.json
+ dev-npm-stage/ + private tarball); the repository never builds Go.
See [the current owner](agent-paired-owned.mjs) for the exact request contract.

Fixture preparation uses a 600-second controller deadline. Importer compilation
uses the ordinary existing target, default stack, locked/offline `-j 1`, isolated
user state and the owned Cargo runner's default 60-minute compilation deadline. Reuse a matching
optimized artifact; otherwise prepare it explicitly with
`node tools/rust-build.mjs <new-artifact-directory> release` before measurement.
The current acceptance mode preserves twenty pairs and full result/CAP replay.

## Complete workload and timing

The current producer constructs NumericSample with 16,384 rows and five columns.
The independent reader checks the fixed WDC/DBD hashes, every exact numeric cell,
CASC identities, definitions and the cold fixture inventory. ABI values describe
the current compilation; no historical allocation estimate is subtracted.

Each engine initializes its own fresh and workflow homes. The Rust query uses
Agent Query, every Page, Capture, Release and Close; Go uses the fixed external
public query. Full ordered rows and CAP contents must match the independent
expectations. The prepared Arrow generation must have the same authenticated
identity and remain unchanged through reuse.

| Phase | Rust task | Go task | Performance decision |
| --- | --- | --- | --- |
| fresh-wdc | Query against the fresh input | Fresh query | Twenty pairs, ratio <=0.50 |
| prepare-inclusive-first | Explicit prepare plus first query in the workflow home | First query in the workflow home | Twenty pairs, ratio <=0.50 |
| first-after-prepare | First query after prepare | The same first workflow query | Correctness and observation only; preparation is excluded here |
| subsequent-hit | Repeated query in the same workflow home | Repeated workflow query | Twenty pairs, ratio <=0.50 |

Both engines use the complete business-process lifecycle. Direct CLI calls use
the owned interval from before process creation through tree retirement and
output drain. The Agent controller measures from immediately before spawning
the CLI through its final close event, including Query, every Page, Capture,
Release, Close, worker retirement and output drain. The original complete
controller interval remains in each sample's `controllerTiming`; controller
startup and final audit-packet serialization are outside the business interval.
Independent replay checks both intervals against their archived observations.
Application caches are controlled; Windows filesystem caches are not flushed.
The shared 600-second controller/replay deadline is unchanged.
Acceptance runs an independent preflight before the twenty alternating pairs.
A failed or missing sample stays a failure and is never replaced.

The generation owner reports only this selected workload. It does not establish
full Data performance, actual-client behavior or describe/Live performance.
CSV diagnostics compare different output workloads and remain correctness-only.
