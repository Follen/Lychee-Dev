# Fixed bbfe/ab4 historical source witnesses

This is the narrow update to the source-candidate section of `README.md`.
Its earlier **clean checkout only** condition remains the default of exported
`verifyCandidate`. The fixed three-pair controller now calls the separate
`verifyHistoricalCandidate` entry. No other benchmark, build or acceptance
identity policy changes.

## Why the two existing witnesses are admissible

The [original blocked recovery](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6232226662)
and [sealed evidence](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6232231317)
record that seven original physical buffers were authenticated, copied to
independent fixed-commit witnesses, and restored the two original source
digests. Git still reported seven unstaged modifications. That run stopped;
its failed/sealed/reaped receipt and six **NOT_RUN** product calls remain
historical evidence. It is not resumed or relabeled.

The newly authorized exception is bound to all of these checks together:

- The original fixed bbfe/ab4 HEAD, artifact-record SHA256, EXE SHA256 and
  artifact `inputHashes.sourceTree`, as already fixed in `source-candidates.mjs`.
- The original, unchanged `sourceDigest` implementation reads the whole input
  tree and must reproduce the original artifact digest.
- Every one of the seven files below is a regular, contained physical file with
  the exact length and SHA256. All seven are checked even if Git reports fewer
  or no modifications. There is no newline normalization in this verifier.
- Raw NUL-delimited Git porcelain may contain only ` M` records for these seven
  exact, case-sensitive paths. Staged, untracked, renamed, deleted, type-changed,
  conflicted, duplicate or other paths reject the candidate.
- Checkout-root, unmodified upstream Emmy and fixed 31-phase source identity
  checks remain mandatory. Source identity is rechecked before and after pairs.

The report records `sourceWitness.policy=artifact-bound-historical-inputs`,
the actual `gitDirty`, raw status rows and their SHA256, and all seven observed
physical identities. A dirty witness is never described as a clean checkout.
The runner does not change Git configuration, attributes, index, source files,
old EXEs or old artifact records. It accepts no configurable dirty-file list.

| Exact relative path | Bytes | SHA256 |
| --- | ---: | --- |
| crates/lycheedev-cli/tests/source_native_xml_public.rs | 8312 | fcb9372ce1be4226e8868bd0f66ef6255416e91eaa06365024db4c23d195d5e3 |
| crates/lycheedev-source/examples/emmy_native_context_probe.rs | 429 | 9170633c310e9aa7a9a78f2d6b7d185fe5206c3d7a1116d0c773433770237142 |
| crates/lycheedev-source/examples/emmy_wow_binding_probe.rs | 68 | 8083dc26123fdeb5d3c09a4ebda0c032ef118aff5d06a061b5cfc88d9df69f52 |
| crates/lycheedev-source/examples/support/wow_binding.rs | 20748 | 36d07bb41f28dfd80b597d607fd83ea9ab6df53d09ca535821d9c54d9ffe2419 |
| crates/lycheedev-source/src/closure.rs | 12657 | edbe9535a6c05f027029fb16bc1dcaf1a58232b030de4a834724f23d38dd6a16 |
| crates/lycheedev-source/tests/native_checker_families_real.rs | 6532 | 402f8df04955fc7bcaf5967935bd9ec8ea0fec03f9f3f8db9aa4f0cbf2a66c04 |
| crates/lycheedev-source/tests/native_return_count_real.rs | 8570 | 33aa2dfb8cdf05fc46fa70983367d964512bcd59c4615de21925f3d073fcb687 |

## Formal execution

Use the newly saved, reviewed runner commit and authenticate its archive and
per-file hashes through the existing export procedure. Export the same 18
runtime files listed in `README.md`; there is no new runtime dependency. Include
`source-candidates.test.mjs` and this document when exporting the review/test
bundle. No local source patch, hash substitution, rebuild or new metadata
publication is required.

Reuse the existing configuration schema and fixed hashes from `README.md`.
The authenticated source roots already prepared by the recovery are:

```text
D:/Code/wow/addons/Lychee Dev/.worktrees/bbfe-source-witness
D:/Code/wow/addons/Lychee Dev/.worktrees/ab4-source-witness
```

Only those `sourceRoot` values change if the previous configuration used a
different witness. Preserve its original `artifactRoot`, `artifact`, commit
and all three fixed identity hashes. Use the existing machine-conditions JSON.
Run through the normal sole Windows test queue, from a fresh owned invocation:

```powershell
node "$runner/tools/rust-source-candidate-diagnostics.mjs" "$candidateConfig" "$machineConditions"
```

This entry remains exactly three adjacent AB/BA/AB pairs, six independent cold
homes, 180-second product / 210-second invocation budgets and a 60-minute outer
deadline. Full 4036-document Facts/error/order, public summaries, 31 phase
counters, native timing and checked retirement are unchanged. Any identity or
result failure stops without replacement samples. Old failed owners stay sealed.

## Node checks and limits of their evidence

From the selected complete checkout:

```text
node --check tests/rust-performance-paired/source-candidates.mjs
node --test tests/rust-performance-paired/source-candidates.test.mjs
```

The new tests exercise exact published metadata, all rejected Git state classes,
changed HEAD/artifact/digest/file identities, actual bounded filesystem hashing,
and real porcelain from an isolated temporary Git fixture. The fixture's index
is only changed to construct staged-failure inputs; the verifier itself does not
change it. Two small public files are reconstructed only inside that test
fixture (the context probe's exact mixed endings and the two-line binding
probe's CRLF endings) and checked against their published hashes.

The public recovery bundle contains the seven identities, not all seven source
buffers. Synthetic positive metadata tests do not establish that a cloud fixture
reproduces the historical physical tree. Actual witness/artifact authentication
and six product invocations are established only by the formal Windows entry's
new receipts; Node checks do not establish performance success.
