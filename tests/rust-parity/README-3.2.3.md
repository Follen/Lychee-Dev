# Fixed Go 3.2.3 candidate static gate

This successor is source-only evidence for the active Rust 3.5.0 candidate. It is
not Rust execution, Windows acceptance, handler parity, performance, or game
validation. Historical 3.2.0/3.2.1/3.2.2 files and their results remain unchanged.

Run from a full-history checkout with Node 20+ and Git:

```sh
node --test tests/rust-parity/oracle-3.2.3.test.mjs
node tests/rust-parity/run-3.2.3.mjs --verify-fixture
node tests/rust-parity/run-3.2.3.mjs --json
```

`--verify-fixture` authenticates the fixed source-derived inputs; it does not
write a fixture or accept the worktree. Exit codes are 0 for accepted static
equality, 1 for a measured mismatch, and 2 for invalid invocation, missing or
corrupt source, unsupported extraction, or unverified extraction without another
measured mismatch. No `--accept-gaps` or automatic regeneration exists.

## Independent reference and permitted differences

- Go reference: `3766f9cacb52e93a803a626187e2a1d54a445468` (3.2.3), not a
  moving tag or branch. Source blobs are read with Git replacements disabled.
- CLI expectations reuse `cli-contract-oracle-3.2.3.mjs`: the exact authenticated
  historical 84 command definitions, five exact engine wording changes, the
  exact `cache prune --data-derived` extension, and the exact `data cache prepare`
  insertion. Reference expectations never come from candidate declarations.
- The command and product source hashes are authenticated independently. All
  seven complete product profiles are compared, including family, folder,
  delivery product code, build series, and reused-slot fields.
- All 55 addon files come from the fixed Go 3.2.3 tree. Only the two reviewed
  complete-file release identity changes to 3.5.0 are accepted. The builder
  proves replacing the single exact 3.2.3 release literal produces each already
  approved candidate hash. All other paths, lengths and hashes must match.
- Raw exact Go differences remain in `rawExact`; approved static comparison is
  reported separately. Metadata key sets and command/flag order stay exact.

## Reviewed candidate extraction

`candidate-adapter-3.2.3.mjs` authenticates the complete current `contract.rs`
SHA-256 `66240b8ebb9d2920d7308e5bce38ff859c58fcb9192b57ee7ee1b015b9e836c8`,
Git blob `1060a12cbd68e3913deb52c3d5610ee336751c99`, as present in candidate
`f0d0ec3aefb9926ecbb1dafb0b6c0d3b76cdd916`.

Independent comparison with reviewed `c508b10ef0e651fc8721d251feef5d25e41a0454`
confirmed `implemented()` is byte-identical. The only runtime change is adding
`--data-derived` to the false arity arm. The other differences are Rust tests.
The separate current adapters reproduce the unchanged routing and parse the
actual current arities. They never supply historical bytes to bypass a candidate
hash check. All 115 frozen arities plus exactly `--data-derived=false` are checked;
extra/missing entries and unknown fallback changes cannot silently pass.

Any later whole-file edit, including test-only or comment edits, makes extraction
unverified pending review. Unit positive/mutation tests use the fixed reviewed
blob; real candidate loading always reads the worktree, with a separate mutation
regression proving this boundary. Actual business registration sources remain
candidate inputs, not frozen expected command lists.

## CI and retained historical failure

CI continues to authenticate all historical manifests and retains the actual
3.2.2 report in `rust-parity-report.json`. Its mismatch exit 1 is explicitly a
historical diagnostic, not rewritten as parity; extraction failures still fail
the step. The separately named 3.2.3 step is the current blocking gate and emits
`rust-parity-3.2.3-report.json`. Both reports are uploaded even on failure.

The known 3.2.2 report still fails on the approved command/flag/wording deltas and
marks its old candidate adapters unverified. A green current static gate does not
rewrite that historical failure and does not close any runtime acceptance gap.
