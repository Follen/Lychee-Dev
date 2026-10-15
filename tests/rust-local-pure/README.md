# Local pure Rust regression harness

This small Linux development check compiles **unchanged production files and
their existing tests**: raw JSON byte grammar (4) and mailbox code-byte/string
and Lua payload boundaries (6). The runner copies the five selected files
byte-for-byte into an isolated crate with normal Rust module lookup. It never
introduces a second implementation or edits the product workspace.

Run from any working directory:

```sh
node /path/to/checkout/tools/rust-local-pure.mjs
```

Prerequisites are an already installed Rust 1.99-compatible Linux toolchain,
Node, Python 3.11+, bubblewrap with private PID/user namespace support, the
existing Cargo cache containing every locked crate, and an available `symbols`
channel. No tool, crate or network download is performed. Configure an existing
installation using `LYCHEEDEV_LOCAL_RUST_BIN`, `CARGO_HOME`, and optionally
`LYCHEEDEV_LOCAL_PYTHON`. Otherwise the runner discovers the existing sibling
`rust-dev` installation, or `RUSTUP_HOME` for its stable Linux toolchain.

The command runs `cargo test --offline --locked --profile symbols -j 1 --lib`
with one test thread. The profile is genuinely optimized, release-derived,
debug=2 and nonincremental, matching the repository's symbols purpose. It
holds only the `symbols` lease and reuses `target-symbols`; it never adopts,
removes or overrides an existing lease. An occupied channel stops admission.
`--initialize-lock` explicitly resolves/refreshes this harness's separate lock
offline; this is not a test result. Ordinary test runs cannot change the lock.

The harness directly pins serde 1.0.229, serde_json 1.0.151 and hex 0.4.3.
Its complete lock is independent of the product lock. In particular cached
transitive versions such as syn 3.0.3 and zmij 1.0.21 need not equal the Windows
product lock. Exact official registry archive checksums, extracted source
bytes/inventory and lock identities are checked and recorded before and after
compilation. The compiler's installed rustc and host standard-library manifest
files, including the dynamic compiler/LLVM libraries, and direct tools are
hashed before and after. This does not authenticate the whole host OS.

Each process runs in bubblewrap's new PID/user namespace with dropped
capabilities and nested user namespaces disabled. Parent death terminates the
namespace. Normal and failure paths require wrapper exit, completed output
readers and a separately successful empty-namespace inspection. Timeout and
output-budget termination remain failures even when cleanup succeeds. These
controls do not claim a separate network sandbox; Cargo is explicitly offline.
The exact compiled stage is read-only for tests and rehashed afterward. Cargo
configuration files, including absent ancestor/home files, are also bound to
the receipt and rechecked.

Existing `tools/rust-disk.mjs` admits disk reservations, records ownership and
seals logs. Successful scratch is retired only after verified cleanup. Failed
scratch stays available; unverified process cleanup also retains the lease.
Evidence remains under `.tmp/rust-disk/evidence/<owner-id>/`, including command,
source/tool/dependency hashes, process receipts and the named test results.
Zero, filtered, ignored, missing or duplicate expected tests cannot pass.

Runner regression checks:

```sh
node --test tools/rust-local-owned-process.test.mjs tools/rust-local-pure.test.mjs
```

This is not Windows runtime verification, whole-workspace coverage, native
input or game validation, corresponding-source acceptance, or performance
comparison with Go. Those checks remain separate. Windows-owned process and
Emmy/luac support tools are neither imported nor modified by this harness.
