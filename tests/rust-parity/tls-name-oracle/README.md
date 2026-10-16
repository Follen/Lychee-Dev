# Windows private-root TLS name comparison

The Windows product workflow runs the existing `ci-data-remote` Cargo group once,
with the same filter/lease/one-shot policy and `--nocapture`. Its wrapper returns
only that invocation's owner UUID through the GitHub step output channel. A
separate step installs Go 1.27.1 through the repository's existing pinned official
`actions/setup-go` action and compares the original sealed Rust observations.
There is no additional Rust build or test invocation.

The Go step runs even when the Rust group failed. It verifies the exact owner,
command, current source-tree hash, terminal/reaped receipt, and every sealed log
hash before reading `stdout.bin`. It does not search for a latest successful run.
An absent owner, fewer than five records, duplicate case, malformed/truncated
JSON, wrong platform/toolchain, or unsealed output is a failure, not a skip or
parity result. A passing TLS comparison cannot clear another Rust group failure.

`run.mjs go` gives standard-library-only `go run` a separate `beginScratch` owner,
private GOCACHE/GOTMPDIR, disabled module/toolchain/network resolution and the
existing `ownedSpawnSync` Windows Job wrapper. Version discovery has a 15-second
bound; compile plus oracle has a 120-second bound. Each case has an original
5-second socket/handshake deadline and joins its server before emitting output.
Both process records, original Go stdout/stderr and the comparison are sealed in
`.tmp/rust-disk/evidence/<owner UUID>/`. Only successful, reaped owners retire their own
registered scratch/cache after evidence sealing. Failed runs retain their scratch;
unknown/unreaped execution retains scratch and reports interruption. Existing workflow artifact collection
already preserves these evidence directories.

The five inputs are `example.test`, `example.test.`, `example.test..`,
`example.test...`, and the certificate-negative control `wrong.test`. The test
fixture key/password is deliberately public. The test builds an in-memory trust
configuration for this connection only; it does not install a certificate or
modify system roots. The Rust test continues strict name and chain verification.

## Exact scope of the conclusion

These observations exercise actual native-TLS/Schannel SNI and certificate
results against the private test CA, compared with Go 1.27.1 using its private
`x509.NewCertPool`. Go's custom pool can select its Go verifier rather than the
Windows default-system-root verification path. Therefore even five matching
cases do not prove equivalence of default Windows system trust, OS root policy,
revocation, or all certificate chains. A real mismatch remains visible as a
failed Rust expectation and/or a failed comparison. No TLS verification flag is
disabled to make the comparison pass.

Pure Node runner/custody tests use synthetic bytes and injected executors. They
do not execute Go, Cargo, Rust, a TLS server, or change trust settings. Runtime
status belongs to the subsequent actual Windows workflow, not those unit tests.
