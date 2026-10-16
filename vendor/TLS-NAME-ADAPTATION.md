# Pinned Windows TLS names

This is a local adaptation of two official locked packages, not an unmodified
upstream claim. Source identities, original per-file hashes, archive checksums,
candidate hashes and the exact reverse-replayable delta are in
[tls-name-adaptation.json](tls-name-adaptation.json) and
[tls-verification-name.patch](tls-verification-name.patch). All 34 files from the
two official `.crate` archives, including their complete licenses, are retained.
The archives were authenticated against the original Cargo.lock checksums before
copying. No registry cache is modified.

- schannel 0.1.29: one optional builder/stream verification-name field and setter;
  the existing certificate policy uses that field or its existing domain.
- native-tls 0.2.18: a Windows-only optional builder field/setter; its existing
  Schannel connector propagates it. The default is None.
- tokio-native-tls 0.3.1: zero changed lines. All original handshake, stream,
  cancellation, socket-drop and error ownership is retained.

The total upstream source delta is 38 added and four replaced/removed lines in
three files, including API documentation. It adds no unsafe code, new FFI,
certificate-chain implementation, TLS implementation, async stream wrapper or
new dependency package/version/feature. Root stores, revocation/chain flags,
certificate callbacks, invalid-name/certificate flags, protocol range and client
identity settings retain their original code and defaults. The original stream
owns the optional UTF-16 name throughout initial and renegotiated validation;
the same policy call consumes it synchronously, just as it consumed domain.

Data still derives literal-IP/zone suppression from the original host and calls
its existing native connector. Only Windows DNS SNI strips every trailing dot;
certificate verification strips exactly one. This follows fixed Go 1.27.1
`crypto/tls/handshake_client.go:hostnameInSNI` and
`crypto/x509/root_windows.go:checkChainSSLServerPolicy`. It does not categorically
reject multi-dot names, so platform exact-name/SAN behavior remains reachable.
The formal Go business baseline remains v3.2.3 commit
`3766f9cacb52e93a803a626187e2a1d54a445468`.

## Build and upgrade scope

The root manifest patches these two packages to the checked-in directories and
excludes them from workspace membership. All package versions/features and all
other package/checksum entries are unchanged. The candidate lock merely removes
registry source/checksum from these two path packages. It has not been parsed or
validated by Cargo here; the next admitted normal Cargo build must validate it.
The license inventory authenticates both path packages against this exact source
record and includes their original license text. Corresponding-source staging
copies these packages beside `vendor/registry`; only that registry subdirectory
is configured as the Cargo registry replacement. The existing source digest
recursively covers both. This adaptation is independent of the unmodified Emmy
dependency requirement.

On upgrade, authenticate the official new archives, review/replay the three-file
delta, regenerate the exact source record, and run the existing Node custody
checks plus the Windows `remote::` group and its fixed Go five-case oracle.
Keep the original no-override behavior, strict wrong-name/untrusted-root failure,
IP/zone no-SNI rules, proxy TLS, original deadline/cancellation and socket closure
regressions. A real dependency upgrade may require changing the small patch; do
not accept a successful patch apply as proof of runtime or trust equivalence.

The private CA used in these tests is trusted only by a per-connection builder.
It changes no system root store. Matching these cases cannot prove equivalence
of every default Windows chain/root/revocation policy. The inherited platform
policy remains strict, and system-trust coverage stays separately scoped.
