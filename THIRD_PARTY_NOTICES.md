# Third-party notices for Lychee Dev

Lychee Dev's project-owned code is distributed under the [MIT license](LICENSE)
(Copyright (c) 2026 Follen / FollenFang). Third-party components retain their own
licenses and attribution, summarized here.

The shipped npm package carries its own assembled `THIRD_PARTY_NOTICES`, which
combines the package-level summary with the complete license texts of the Rust
dependency closure and the managed Lua 5.1.5 compiler/runtime. Crate versions are
pinned by `Cargo.lock`; exact license texts and supplements are recorded in
[release/licenses/catalog.json](release/licenses/catalog.json) and are checked by
`tools/rust-license-inventory.mjs`. The product version is defined only by
[release/version.json](release/version.json).

## EmmyLua Analyzer Rust and owned adapter code

The Source analysis engine is the complete, unmodified public
[EmmyLua Analyzer Rust](https://github.com/CppCXY/emmylua-analyzer-rust) commit
`08b0873962e83bb8307e9a81052e3f41578fbc78`, licensed under MIT (Copyright (c)
2024-2026 CppCXY). Its 848-file archive, tree SHA-256 and original MIT license
identity are recorded in [third_party/emmy/identity.json](third_party/emmy/identity.json)
and [third_party/emmy/README.md](third_party/emmy/README.md); the upstream tree is
not patched.

Contextual business semantics and derived control-flow, relation, member,
transform, signature, diagnostic and checker implementations in
`crates/lycheedev-emmy-adapter/src/session/backend/` adapt portions of that
upstream. They remain copyright 2024-2026 CppCXY under MIT. The exact upstream
permission and warranty notice is retained in
[crates/lycheedev-emmy-adapter/LICENSE-EmmyLua](crates/lycheedev-emmy-adapter/LICENSE-EmmyLua).
Moving implementation into an owned crate does not reattribute the upstream code.

## Pinned Windows TLS name adaptation

Two exact registry releases are consumed as local patch sources for the Windows
TLS connector:

- `vendor/native-tls-0.2.18`: the rust-native-tls Developers' MIT OR Apache-2.0
  license and copyright are retained in
  [LICENSE-MIT](vendor/native-tls-0.2.18/LICENSE-MIT) and
  [LICENSE-APACHE](vendor/native-tls-0.2.18/LICENSE-APACHE).
- `vendor/schannel-0.1.29`: Copyright (c) 2015 steffengy under MIT, retained in
  [LICENSE.md](vendor/schannel-0.1.29/LICENSE.md).

The narrow local adaptation separates SNI and certificate-verification names
inside the existing Windows connector; original chain/name validation, trust
policy and asynchronous owners remain in their original libraries. These are not
unmodified upstream packages. Complete source identities, archive hashes and the
replayable delta are recorded in
[vendor/tls-name-adaptation.json](vendor/tls-name-adaptation.json) and
[vendor/tls-verification-name.patch](vendor/tls-verification-name.patch). Other
platforms use the original native-tls API and implementation.

## Unicode data

`release/data/unicode-17.0.0/UnicodeData.txt` (SHA-256
`2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c`) is the fixed
official Unicode 17.0.0 character data used to generate the shipped simple-casing
table `crates/lycheedev-data/src/unicode_simple.rs` through
`tools/generate-unicode-simple.mjs`. It is distributed under the Unicode License
V3; the full notice is retained in
[crates/lycheedev-data/UNICODE-LICENSE](crates/lycheedev-data/UNICODE-LICENSE).
The source hash, license identity, generator and generated path are also recorded
in the Data resource section of [release/licenses/catalog.json](release/licenses/catalog.json).

## Go-derived compatibility code (BSD-3-Clause)

The Rust Data transport and Windows input code adapt fixed Go 1.27.1 standard
library behavior. Those portions remain Copyright 2009 The Go Authors under
BSD-3-Clause; complete notices are retained in the repository:

- `crates/lycheedev-data/src/remote/transport/` (redirect/reference handling,
  gzip framing, fixed Host/canonical-endpoint rules and the SOCKS5 handshake
  adapter): notice in
  [GO-LICENSE](crates/lycheedev-data/src/remote/transport/GO-LICENSE).
- `crates/lycheedev-data/src/hotfix/remote/entities.rs` (generated from Go
  `src/html/entity.go`): notice in
  [GO-HTML-LICENSE](crates/lycheedev-data/src/hotfix/remote/GO-HTML-LICENSE).
- `crates/lycheedev-windows/src/mailbox_input/go_unicode/` and
  `equal_fold_data.rs` (generated Go Unicode 17.0.0 case-folding data): notice in
  [go_unicode/LICENSE](crates/lycheedev-windows/src/mailbox_input/go_unicode/LICENSE).

These adapters retain the upstream attribution; they do not add a second runtime
or dependency set.

## Other third-party components in the shipped binaries

- **Bob Jenkins lookup3** (public domain, May 2006): the CASC index corruption
  check uses the lookup3 hash implemented in the project source
  (https://burtleburtle.net/bob/c/lookup3.c). It is not a cryptographic integrity
  primitive, and no upstream source file is redistributed.
- **SQLite** (public domain, https://sqlite.org/copyright.html): the bundled
  SQLite distribution is used through `rusqlite`/`libsqlite3-sys` for workspace
  metadata. The crate-level license text is part of the generated Rust dependency
  closure in the shipped package notices.
- **Lua 5.1.5** (MIT, PUC-Rio, https://www.lua.org/): the managed compiler used
  for Source syntax checks and Live probe tooling is built from the official Lua
  source; the complete notice is included in the shipped package notices.
- **Managed Lua 5.1 test interpreter**: the test suite builds the official Lua
  5.1.5 interpreter from source; it is not an npm dependency and is not part of
  the runtime payload outside the managed compiler above.

## Blizzard interface material

- The existing addon ships event catalog files under
  `addon/Modules/Events/CatalogData_*.lua` generated from official
  `Blizzard_APIDocumentationGenerated` interface documentation mirrored by
  [wow-ui-source](https://github.com/Gethe/wow-ui-source); each generated file
  records its source commit in its header.
- Test-only fixtures under [tests/fixtures/source-native](tests/fixtures/source-native/)
  contain fixed Blizzard UI source excerpts and XML schemas from recorded
  wow-ui-source commits (for example the commit in
  [xml-companion-manifest.json](tests/fixtures/source-native/xml-companion-manifest.json)
  and [callers/fixtures.json](tests/fixtures/source-native/callers/fixtures.json)).
  They are investigation/test inputs and are not shipped in release payloads.

World of Warcraft and Blizzard Battle.net are trademarks or registered
trademarks of Blizzard Entertainment, Inc. in the U.S. and/or other countries.

## Test-only fixed inputs from wowdata (AGPL-3.0-or-later)

Two fixed test inputs are minimal binary/HTML samples copied from the legacy
[`wowdata`](https://github.com/follenfang/wowdata) toolkit at commit
`6191d3dc567966b7a474849f3a11e7411390091c`. They remain licensed under
AGPL-3.0-or-later with attribution to the wowdata project. No wowdata user data,
workspace data or executable code is included, and these files are not part of
the shipped npm/addon payloads.

- [tests/fixtures/db2/JournalEncounterSection-classic-era.db2](tests/fixtures/db2/JournalEncounterSection-classic-era.db2)
  — a minimal real WDC5 DB2 sample (zero rows) consumed by
  `crates/lycheedev-data/tests/table_decode.rs` for real-format parsing and
  empty-result encounter semantics. Provenance, bytes and SHA-256 are recorded in
  [tests/fixtures/provenance.json](tests/fixtures/provenance.json).
- `tests/fixtures/hotfix-wago/response-page-1.html`,
  `response-page-2.html` and `response-page-2-real.html` — controlled offline
  Wago hotfix pages consumed by `crates/lycheedev-data/src/hotfix/remote/tests.rs`
  as parser and pagination fixtures. Provenance and the AGPL license note are
  recorded in
  [tests/fixtures/hotfix-wago/manifest.json](tests/fixtures/hotfix-wago/manifest.json).

## Addon media

`addon/Media/Logo.png` and `addon/Media/RoundedPanel.png` are project-owned Lychee
Dev assets. `addon/Media/GitHub.png` is the GitHub logo/mark used as a link
affordance; GitHub is a trademark of GitHub, Inc. No separate third-party license
is recorded for that mark in this repository.
