# Data failure recovery

Read only after a structured failure or incomplete coverage prevents the next
useful query. Preserve the original pin, source, command, error and captures.

## Choose one corrective action

Separate installation identity, archive location, payload integrity, DB2 decoding and query binding;
a failure in one layer does not establish failure in another.

- A client subdirectory can legitimately share its parent's `.build.info` and
  `Data/`. Their absence inside the client folder does not establish a damaged
  or unofficial installation. Use the CLI's installation resolver.
- `archive.index_integrity` requires identifying the failing index/span/check;
  do not assume mixed products caused it or bypass checksum validation.
- `records.local_object_unavailable` concerns the requested content. A missing
  localized table can coexist with readable numeric tables. Inspect useful
  tables under the same pin; report what remains unavailable.
- `records.remote_http` needs the failing request/stage before interpretation.
  Installation flavor and publishing slot can differ; the CLI owns that mapping.
  A guessed config URL or unavailable loose copy does not prove the build is
  absent from CDN archives. Do not substitute another build or region.
- A healthy `doctor` checks workspace health, not selected-build readiness.
  Its cache object count is not an inventory of all evidence blobs or prepared
  source records. Test the requested table rather than infer availability.
- Missing keys preserve partial coverage. Static values and client Hotfixes do
  not prove server-side coefficients; retain them as inputs to the live fitting
  task instead of treating a static query as completion of that task.

## Retry and stop

- Identity conflict: resolve the requested identity; never change a project lock
  merely to make a read succeed. Compare separate pins as described in
  [data-targets.md](data-targets.md) when delivery configurations differ.
- Unknown field or invalid SQL: inspect schema/error location, correct the query,
  then retry. Unsupported expressions/formats need a supported alternative or an
  explicit capability limit, not repeated unchanged calls.
- Missing cache under `--offline`: use available evidence and report the missing
  prerequisite. Do not remove the restriction to make the command succeed.
- Missing keys: continue useful readable records, retaining partial coverage;
  changing CDN hosts cannot supply a missing key.
- Deadline: static queries default to 300 seconds total. If progress or a known
  preparation cost justifies it, make one deliberate budget extension with
  `--timeout-seconds <1-3600>` within the task budget. A second deadline stops that
  branch; do not keep raising budgets. Hotfix uses its own scan/request bounds.
- Transient I/O: the CLI already owns mirrors and route refresh. Allow at most
  one unchanged retry within the task budget; repeated failure stops that branch
  until its cause changes. Never script a second CDN-routing layer or bypass
  integrity checks. Diagnose corrupt bytes rather than trying hosts until accepted.
- Interrupted Hotfix scan: recover the saved checkpoint under
  [data-hotfix.md](data-hotfix.md), preserving its source and filters.

Stopping one unavailable branch does not prevent independent useful queries.
Report the unresolved part, exact failure, retained pin/capture/checkpoint and
what must change to proceed. An empty success is not a substitute for failure.
