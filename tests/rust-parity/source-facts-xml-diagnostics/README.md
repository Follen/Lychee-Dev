# Fixed Go public XML lexical diagnostics

Status: implementation and finite differential inputs prepared, **NOT_RUN** for
Rust execution and the new fixed-Go differential. Static checks are not product
parity. This is a supplement to the unchanged 52-case malformed corpus, not a
replacement or a claim that all invalid XML is covered.

## Contract and bounded implementation

The oracle remains Go v3.2.3 commit
`3766f9cacb52e93a803a626187e2a1d54a445468`, official Go1.27.1, and its real
`internal/addoncheck.AnalyzeDocument` wrapper. Its `xml.go` calls
`encoding/xml.Decoder.Token`; public `SyntaxNote.line` is the position captured
before the failed token, while the line inside the error message is the
consumed decoder position. Already returned declarations, relationships and
loads survive a later token failure.

[Fixed Go1.27.1 encoding/xml source](https://github.com/golang/go/blob/go1.27.1/src/encoding/xml/xml.go)
provides the implementation authority:

- `rawToken`, lines 560–577: end-name parsing and suffix validation happen
  before `Token` performs nesting/mismatch checks.
- Lines 633–672: malformed comment/CDATA opening markers report the first
  unexpected consumed byte. An incomplete marker reports ordinary EOF.
- `nsname`/`name`/`readName`, lines 1117–1172: scan the complete name before
  validating it; EOF takes precedence over name validity. Empty namespace
  prefix/suffix is permitted, but multiple colons are not.
- Lines 762–809 do not require spaces between attributes. The public parser
  must preserve that tolerance, including repeated same-name attributes.

The production change is limited to public `facts.rs`: lexical failures for
comment/CDATA opening markers and ASCII end tags are checked before consuming
quick-xml's event result. The helper returns the consumed raw-byte offset, so
it preserves both diagnostic positions and prior Facts. Correct markers and
end tags continue through the existing complete path. Native XML/profile,
inline source maps, useful native facts, upstream Emmy, and public schemas are
unchanged. Private Facts/index cache identities advance to v10/v11 so cached
older malformed results cannot hide the change.

The 32 inputs comprise 11 malformed bang-prefix cases, one valid mixed
comment/CDATA handler, 14 malformed end-tag cases, and six controls: ordinary
mismatched ends, end whitespace, empty namespace prefix/suffix, adjacent
attributes, and adjacent duplicate attributes. The existing 52 cases remain
byte-identical and are compared separately. Complete Facts/error values and
all array order are compared; no summary normalization is used.

Non-ASCII names retain the existing public path. Go's Unicode name ranges are
not the native XML Fifth Edition name ranges. Start/attribute/PI name validity,
generic attribute failures and the remaining full diagnostic space still need
independent fixed-Go observations; this batch does not tighten them by spec.

## Consolidated local verification

Use the existing owned process and scratch admission route. Do not start a
bare compiler, create another cache/channel, modify the fixed-Go checkout or
change the old corpus/report. Follow the authentication, overlay, isolation,
sealing and cleanup requirements in
[the original fixture instructions](../source-facts-malformed/README.md).

The original `oracle_test.go` already reads its input through
`LYCHEEDEV_SOURCE_FACTS_CASES`; no runner or Go production change is necessary.
Use that unchanged test as the sole test-file overlay, with cwd at the fixed
checkout and a new nonexistent owned report path for each invocation. For the
new report, select this directory's absolute `cases.json` path. For the old
report, select the original 52-case file (or reuse its independently sealed,
identity-matching report). Through the existing `ownedSpawnSync` wrapper run:

```powershell
# Resolve while cwd is the Rust candidate; pass these into the existing wrapper.
$env:LYCHEEDEV_SOURCE_FACTS_CASES = (Resolve-Path 'tests/rust-parity/source-facts-xml-diagnostics/cases.json').Path
$env:LYCHEEDEV_SOURCE_FACTS_REPORT = '<absolute-nonexistent-owned-new-32-report.json>'
```

```text
go test -overlay <owned-overlay.json> -count=1 -run ^TestFixedGoPublicFactsMalformedOracle$ -v ./internal/addoncheck
```

Set both report variables to the absolute sealed report paths. One controlled
Rust invocation then compares the old 52 and new 32 independently:

```powershell
$env:LYCHEEDEV_SOURCE_FACTS_GO_REPORT = '<absolute-old-52-report.json>'
$env:LYCHEEDEV_SOURCE_FACTS_XML_GO_REPORT = '<absolute-new-32-report.json>'
node tools/rust-cargo.mjs --owner source-xml-diagnostic-differential -- test -p lycheedev-source --lib --locked --offline facts::tests::fixed_go_ -- --nocapture
```

Both must be present: without the corresponding report, an observation test
explicitly prints `SOURCE_FACT_ORACLE_NOT_COMPARED` and is not a differential
pass. Each test validates report schema, fixed commit, Go1.27.1, its own fixture
SHA256, exact case coverage, every Facts field, order and returned error.

Include the following six new tests in the normal consolidated Rust batch:

- `facts::tests::fixed_go_xml_diagnostic_observations`
- `facts::tests::public_xml_bang_markers_keep_first_consumed_failure`
- `facts::tests::public_xml_end_syntax_precedes_mismatch_and_preserves_go_tolerance`
- `facts::tests::public_xml_lexical_failures_keep_prior_facts_and_distinct_diagnostic_lines`
- `index::facts_cache_tests::xml_lexical_contract_invalidates_previous_index_and_file_facts`
- `index::facts_cache_tests::xml_lexical_contract_rebuilds_previous_schema_even_with_valid_digests`

Also rerun the actual full normal 4036-document field/order comparison when
verifying the candidate; this finite corpus does not replace that regression,
retained/native acceptance or the same-machine performance requirements.

Static input checks, which never execute a parser or compiler:

```text
node --test tests/rust-parity/source-facts-xml-diagnostics/fixture.test.mjs
```
