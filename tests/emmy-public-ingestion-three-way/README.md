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

At initial preparation, no runner, test, Python module, Cargo command or workflow
in this payload had executed; only static input hashes and Python AST parsing
were checked. Later attempts and focused R4 checks are recorded below. A new
freeze and workflow require independent review before a new Windows run.

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

## R4 narrow hosted Python alias identity correction

The R3 Windows attempt reached tool inventory and failed with
`windows-contract.reparse` at the individual-file hash, before `beginScratch`,
any owned child or any Rust execution. The log did not identify the path, so the
exact offender is not claimed as observed. Its unchanged archive and log hashes
are retained in `tool-identity-policy.json`.

The official [Python installer](https://github.com/actions/python-versions/blob/49858b91c92d27403416ee43117032e91d00ce24/installers/win-setup-template.ps1)
creates `python3.exe` as a symbolic link to `python.exe`. The hosted-image
[toolcache root](https://github.com/actions/runner-images/blob/349014bb757f0dadea17d6caee1308189aa7ee6c/images/windows/templates/variable.windows.pkr.hcl)
is `C:\hostedtoolcache\windows`; its installer and PATH setup use that Python
version/architecture directory. The runner enumerates every Python-root `.exe`
and previously keyed the result by its resolved path while hashing its original
path. This is a concrete supported failure candidate. The historical log alone
cannot exclude another tool-file link, including a PowerShell file.

R4 allows only that one leaf alias under the fixed hosted Python root, for the
same Python 3.12–3.14 x64 versions already accepted by the runtime check. It must
point directly to the independently selected sibling `python.exe`. Linked
ancestors, indirect chains, escaping targets and every other tool link remain
rejected, including PowerShell/.NET files. No general reparse waiver is added.

Each original tool path now retains its canonical target, raw link text and
file identity. Target bytes are read through a handle whose identity matches
the checked file; handle, pathname and alias identities are compared after the
bounded read. Duplicate canonical targets must agree, without dropping their
original-path mappings. The existing checks before and after every owned child
and at final sealing compare the full snapshot. Initial hosted Python bytes
are trusted as part of the hosted toolcache, not claimed to be signature-verified
or pinned to an independently downloaded installer. Future pre-owner errors name
the exact requested path, so an unknown layout remains diagnosable and closed.

The strict source/payload/stage/configuration hashes and inventories, Python
cache checker, copied Cargo/MSVC pins and all three process/disk owner files are
unchanged. No Rust operation, process-owner recovery, security-setting change,
CI dispatch, push or public write was performed for this preparation.

Actual focused local validation: Node v24.19.0 on Linux, 19 new real-filesystem/
policy fixtures plus 12 existing parser fixtures, 31 passed with no skips.
This proves the exercised POSIX filesystem and pure policy cases only. Windows
symlink/junction fixtures, the Windows runner, 31×3 Rust contracts, resources,
whole-Retail behavior and performance remain NOT_RUN for R4. The original
Windows failure is preserved and no A/B/C success is inferred.
