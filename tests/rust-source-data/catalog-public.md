# Item/decor public differential acceptance

This runner executes a **specified sealed current Rust CLI** and the frozen Go
oracle against small authenticated synthetic CASC/Encoding/Root/DBD/WDC inputs.
It does not build a CLI, generate fixtures, contact a service, or run a game.
Preparing the files and passing `node --check` are not differential acceptance.
Only a completed `report.json` with `passed: true` is a successful execution.

From the repository root, in an authorized serialized execution window:

```powershell
node tests/rust-source-data/catalog-public.mjs <current-artifact.json> tests/rust-source-data/catalog-public-fixtures.example.json <NEW-evidence-directory>
```

The artifact must pass `verifyArtifact` and its source digest must equal the
current Rust inputs. No fallback executable is selected. The Go oracle is
`.tmp/lycheedev-oracle.exe`, pinned to SHA-256
`5738418756ee57c536518ff71223f2e7843ac3495a846b4f063d3104d83aae87`.
The output directory must not exist. Every child uses `ownedSpawnSync`, a
180-second timeout, retained ownership receipts, and verified handle reaping.
Use a Node runtime with `node:sqlite` and JSON reviver source support.

## Fixture provenance

`catalog-public-fixtures.example.json` is an explicit local fixture manifest,
including descriptor hashes. It currently identifies the new baseline prepared
by the sealed test executable at `.tmp/rust-catalog-navigation-public-input-01`
and six preserved variants from the verified catalog fixture suite. These paths
are local evidence, not tracked fixtures guaranteed to exist on another checkout.
Missing or changed files fail the run. Prepare fresh fixtures separately and
update the manifest with their exact paths and hashes for another machine.
Do not substitute production installations or copy an existing user home.

The runner verifies each descriptor, selection PIN content address, exact
definition documents and blob bytes, manifest-to-table bindings, DBD hashes,
and WDC bytes extracted from the fixture's bounded local archive spans. It
records file hashes for the complete synthetic installation and retains the
raw PIN/definition documents, manifest/DBD blobs and WDC table bytes. The original
fixture is never edited. Installation inventories are limited to 128 regular
files and 16 MiB; symlinks are rejected. Inputs are checked again after execution.

Each engine initializes its own new home for each fixture. Only the explicitly
verified PIN and definition documents/blobs are seeded into that home. Existing
captures, results and caches are never imported. Both engines receive the same
installation and snapshot, with `--offline` on every data command.

## Cases and comparison

The checked-in matrix has 45 cases, each executed as JSON and JSONL (90 pairs):

- Item Unicode/control-text preservation, slot naming, missing Item class row,
  absent records and invalid/missing IDs.
- Decor sorted enumeration, limits 1/2/3/5000, truncation and exact completion,
  all three selectors, ambiguity, zero optional IDs, absent records and conflicts.
- Parser/domain boundary limits, malformed/overflow/plus IDs, timeout boundaries,
  unknown-flag precedence and missing installation.
- Empty tables, missing/incorrect field types, missing presentation fields,
  the 5001-row relation budget, locale mismatch, enUS and a corrupt extra row.

Every pair compares exit status, stderr (CRLF normalized), the full envelope,
and every JSONL frame. Assertions in the matrix independently check selected
expected values for **both** engines. JSONL success requires begin/record/end;
failure requires begin/error. The record must equal the terminal result.
Integer tokens outside JavaScript's safe range remain exact BigInts in memory;
reports encode these explicitly as `{ "$exactInteger": "..." }`.

For every successful capture, each engine independently verifies its content
address, size/hash, metadata document, timestamp syntax and raw captured JSON
body equality with the public result, then runs its own `evidence verify`.
Only after these checks are capture IDs, timestamps and content-derived blob
hashes/sizes normalized. Captured body equality is checked **before** any
normalization. Isolated home paths are the only path normalization.

There are two explicit diagnostic differences, recorded in `diagnostics` and
raw stdout; these are not claimed as byte-for-byte parity:

1. All five resource counters must be nonnegative safe integers within the
   default limits. Offline network requests/bytes must be zero. Different
   Go/Rust charged counters are replaced only after validation.
2. Rust's additive `tableVerification` must have exactly one expected five-flag
   proof per table. Its `resourceAccounting` must exactly declare conservative
   cumulative admission, positive bounded WDC metadata bytes, incomplete whole
   query accounting, and the three existing documented gaps. Unknown fields or
   different declarations fail validation. These additions are retained in the
   diagnostic audit and removed from the legacy-field comparison.

All remaining fields, including pin, table identity, errors/details, context,
warnings, row values, ordering, relations, completeness and provenance, compare
exactly. A failure records `passed: false`; the runner never relabels missing
execution as success. Raw process outputs and process proof remain in the new
evidence directory even on validation failure.

This acceptance is limited to these synthetic local inputs. It provides no
real-client, remote/CDN, first-install, reload, relogin, performance, or complete
whole-query resource-accounting claim.
