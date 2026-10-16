# Fixed unmodified EmmyLua source

The production dependency is the complete, **unmodified** public EmmyLua commit
`08b0873962e83bb8307e9a81052e3f41578fbc78` (MIT). Its 848 files and 8,382,174 bytes
have tree SHA256 `375d92c5d5e503d77232fabf22a21a82f504c2b38dc5def8fcca1d6ecf06e104`.
`identity.json` lists no patches; its final tree, count and byte size must equal
the authenticated base. The original upstream license is included unchanged.
This does not claim recovery of the missing historical private fork.

Preparation authenticates the full archive before extraction, then checks every
file against `files.json`. It does not execute upstream programs or configuration.
The generated source remains a corresponding-source build input, accompanied by
this recipe and its identity receipt; an upstream URL alone is insufficient.

Preparation: `python3 third_party/emmy/prepare.py --archive /path/base.tar.gz`.
The destination remains `.tmp/emmy-reconstruction-v1/upstream` for path stability.
An existing destination is verified and never silently overwritten. `--download`
explicitly fetches the pinned archive to the supplied archive path if absent.

## Owned contextual behavior

`crates/lycheedev-emmy-adapter` supplies contextual query/session, selected value
identity, flow, type relations and checker scheduling using upstream public facts
and syntax. `crates/lycheedev-binding-contracts` supplies shared evidence records.
Emmy no longer imports or re-exports either owned crate. The recipe continues to
authenticate the small owned-contract inventory used by the surrounding build.

Fixed historical patched-engine bytes are retained only in
`tests/rust-source-engine/fixed-emmy-oracle/historical-engine` for independent
reference tests. They are not production dependency bytes. The reference phase
executes the old checks; the clean phase compares their captured full values,
producer-ordered coverage and query sequences. Missing reference evidence fails.

Implementation, unrun validation and remaining acceptance work are recorded in
`handover.md`. Clean dependency custody is separate from semantic equivalence,
resource measurements, Retail acceptance and the final audit loops.
