# Fixed Go public Source Facts malformed inputs

Status: prepared, **NOT_RUN**. Source-derived expectations are not executed Go
observations. The fixture calls the real fixed `addoncheck.AnalyzeDocument`
wrapper; in particular lexer EOF line -1 becomes public diagnostic line 0.
Every output contains the complete ordered Facts JSON and returned error.

Use the existing local owned-process entry and an independent `beginScratch`
owner. Do not run a bare compiler, acquire another Cargo channel, reuse an old
owner, modify an authenticated fixed-Go checkout, or relabel historical output.
This directory deliberately does not introduce a process runner.

1. Authenticate an unchanged checkout at
   `3766f9cacb52e93a803a626187e2a1d54a445468`, its fixed `go.mod`/`go.sum`,
   gopher-lua v1.1.1 dependency, and official Go1.27.1. Preserve those receipts.
2. In the new owner's scratch, prepare a Go `-overlay` JSON with one `Replace`
   entry only: the absolute nonexistent fixed-checkout path
   `internal/addoncheck/source_facts_malformed_oracle_test.go` maps to this
   directory's absolute `oracle_test.go` path. Never replace production files.
   Both the fixed checkout and original source authentication stay unchanged.
3. Set `LYCHEEDEV_SOURCE_FACTS_CASES` to this directory's `cases.json` and
   `LYCHEEDEV_SOURCE_FACTS_REPORT` to a nonexistent report in that owner's
   scratch. Use private GOCACHE/GOTMPDIR, `GOTOOLCHAIN=local`, `GOPROXY=off`,
   `GOSUMDB=off`, `GOWORK=off`, and the already authenticated module cache.
4. Through the existing `ownedSpawnSync` Windows Job wrapper, run this exact Go
   command with cwd at the fixed checkout and a bounded timeout:

   `go test -overlay <owned-overlay.json> -count=1 -run ^TestFixedGoPublicFactsMalformedOracle$ -v ./internal/addoncheck`

   The overlay test independently authenticates all six scanner source files,
   calls AnalyzeDocument for every case and fails on output truncation/write
   errors. Seal the fixture/overlay/source hashes, complete stdout/stderr,
   report, exit and real process-reaping receipts before retiring own scratch.
   A successful test records observations; it does not assert Rust parity.
5. The normal Rust `facts::tests::fixed_go_malformed_observations` test emits the
   same `SOURCE_FACT_ORACLE_JSON` rows. Optionally supply the independently
   sealed Go report as `LYCHEEDEV_SOURCE_FACTS_GO_REPORT`; then this test checks
   its fixed commit, fixture SHA256, complete case coverage and exact full
   Facts/error equality, including all array order, line numbers and messages.
   Without that report it is explicitly observation-only, not a differential
   pass. Run it through the existing `tools/rust-cargo.mjs` controlled entry in
   the consolidated local batch. Do not rerun already passed unaffected groups.

The finite inputs cover incomplete Lua functions/strings/comments, newline
calls and comment variants, literal/comment versus token NUL, reserved goto
versus valid labels, valid multiline assignment and actual parse failure;
XML declaration/version/encoding, reference/comment errors, namespace-prefix
matching, ignored directives, and malformed inline Lua. They do not replace
the full 4036-document comparison, semantic/native validation, or performance
acceptance. New Rust regression declarations remain NOT_RUN until executed.
