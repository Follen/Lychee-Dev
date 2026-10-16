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

## Windows SNI/verification-name correction

The ec503 Windows results are retained in PR14 comments
[6213137313](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6213137313)
and [6213169897](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6213169897):
all five cases were actually observed, and both multi-dot cases had different
SNI bytes despite matching TLS rejection. That failure is not converted into a
pass by changing the expected observations.

The successor uses the same native-tls → Schannel → tokio-native-tls connector.
Two pinned dependency adaptations expose a separate Windows verification name;
Data supplies Go's all-final-dots-trimmed SNI and one-final-dot-trimmed Windows
certificate-policy input. No hostname pre-rejection is introduced: unusual SAN
and exact-name behavior remains in the original platform certificate policy.
No ClientHello bytes, trust policy, validation flags, TLS algorithms, protocol
versions, async IO, resolver, proxy routing or cancellation owners are changed.
Normal native-tls callers with no override retain the original domain behavior.
The stream owns both names, so any original renegotiation validation uses the
same verification input rather than reverting to SNI.

The source and upgrade procedure are recorded in
[the pinned adaptation](../../../vendor/TLS-NAME-ADAPTATION.md).
The existing five-case oracle, normal/IP/proxy loopbacks, and original stop
checks remain in `remote::`; the dot-name cancellation loop now includes both
multi-dot cases. A separate private-root regression checks the pinned builder's
None/default behavior, clone propagation, independent positive/negative names
and an untrusted root. Its observations do not add cases to the fixed five-case
oracle output or relax the oracle. These new Rust declarations require the
next ordinary Windows build/test batch; source checks are not runtime evidence.
