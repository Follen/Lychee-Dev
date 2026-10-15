# Fixed Windows Emmy declaration-ingestion contract experiment

This directory is a frozen validation payload, not a product workspace or an
accepted no-patch migration. Its only runtime command is `contracts` on a new
GitHub-hosted Windows job in Follen/Lychee-Dev's rust-refactor branch. The full
payload must be placed at `tests/emmy-public-ingestion-three-way` and compared
with the independently approved `freeze.json` SHA before execution.

A uses the frozen old owned constructor plus the current 853-file recipe. B
uses the new constructor plus that recipe. C uses the new constructor and all
848 original official upstream files, with no added upstream tests. The same
31 tests, complete 450 query traces, six constructor outcomes and each side's
actual single-shot/persistent checkpoint exhaustion are required. The focused
203-package lock is not the product lock; its 198 registry blocks are unchanged.

The existing frozen Windows process owner and disk controls are reused without
edits. Product runCargo guards are untouched. One new CI machine's symbols
lease covers serial fetch, preparation, compilation and test execution. This
never adopts or repairs the historical interrupted Linux owner.

Only the existing fixed official-archive downloader/extractor and Cargo's
normal official registry fetch are used. Tool inventories, effective Cargo
configurations, source copies, lock bytes, registry archives, extracted crate
sources, exact test executable identities and original process receipts are
retained. Cargo uses a newly created owner-scoped Cargo home and the one leased
target. Known MSVC/linker identities are fixed by the existing policy; unexpected
tools or drift fail closed. System DLL/.NET runtime remains the hosted Windows
OS trust boundary, not a claim of reproducing every operating-system byte.

The fixture A-parity/outcomes/checkpoints JSON files are historical parser unit
inputs. They are never read by the real A/B/C runtime or substituted for a side.
No 8 MiB resource-pressure probe, memory cap, memory-equivalence claim, Windows
product-worker deadline test, full Retail acceptance or performance target is
part of this contract-only delivery. Those remain separate reviewed work.

No runner, test, Python module, Cargo command or workflow in this new payload
has been executed as part of preparation. The initial checks were static input
hash comparisons and Python AST parsing only. Approval of this candidate's exact
freeze and workflow is required before any actual execution.

## R3 fixed official Cargo correction

The first Windows attempt stopped at the Cargo identity check before any A/B/C
Rust test execution. Its copied local auxiliary policy had a different Cargo
identity. The R3 pin is independently derived from Rust's fixed 2026-10-01
1.99.0 MSVC component: official HTTPS channel manifest plus its checksum,
manifest-named archive checksum, then the exact cargo.exe archive member.
`official-cargo.json` records that chain; the archive was read without installing
or executing it. Detached-signature verification is not claimed.

Only the copied auxiliary policy's Cargo digest changes. Both the top-level
runner check and the frozen PowerShell Cargo-build helper check still compare
against that same exact digest. The owner implementations, MSVC identities,
process cleanup rules, fixed test inputs and all A/B/C contracts are unchanged.
The product runner/policy and unresolved Linux owner are untouched. The new
payload requires a new independently reviewed freeze and Windows run; this
correction is not evidence that any of the three variants passed.
