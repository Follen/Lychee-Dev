# Third-party notices for Lychee Dev Toolkit

Lychee Dev Toolkit's project-owned code is distributed under the [MIT license](LICENSE).
The owner's licensing decision for this tree is recorded in
[the licensing record](docs/toolkit/licensing.md). Third-party components retain
their own licenses and attribution below. Full license texts for distributed
components are reproduced in
[the package notices](packages/npm/lycheedev/THIRD_PARTY_NOTICES).
Go module versions are pinned in go.sum; the release includes corresponding source.

## luaqrcode

`tests/fixtures/legacy-addon/MatrixSymbol.lua` (test/lab only, not the shipped addon) adapts Patrick Gundlach and contributors'
`speedata/luaqrcode` (https://github.com/speedata/luaqrcode) from the repository's
existing licensed copy at
`add-on/Libs/AutomationQR.lua` (Lychee baseline
`41af9cb616dcb7a9e604619a4fc0ad6c8d6d8210`). The full BSD-3-Clause notice is
retained in the distributed Lua source. Toolkit changes provide a private,
lazy, bounded interface with fixed M correction and replace the 256x256 byte
XOR table with a 16x16 nibble table. The third-party QR algorithm is not
represented as newly authored toolkit code; no old automation entrypoint or
user data is loaded.

## Image codecs and Go modules compiled into the native binaries

PNG uses Go's standard library. Lossless WebP encoding uses
[HugoSmits86/nativewebp v1.3.0](https://github.com/HugoSmits86/nativewebp/tree/v1.3.0),
commit `732aa4ca729d1f9dc2068158af309e260792cb67`, under MIT.

Correction to earlier wording: `golang.org/x/image v0.24.0` is not only a test
decoder. nativewebp's `reader.go` imports `golang.org/x/image/webp` in ordinary
builds (and registers a global `webp` image format in `init()`), so
`golang.org/x/image` (`webp`, `riff`, `vp8`, `vp8l`) is compiled into every
shipped binary; toolkit tests use the same decoder for independent pixel
verification. `golang.org/x/text v0.22.0` is compiled in as well, through
gozxing's character-set decoding.

The complete set of third-party Go modules compiled into the shipped
`lycheedev` binaries (`go version -m`, `go.sum` pins):

| Module | Version | License | Used for |
| --- | --- | --- | --- |
| github.com/Microsoft/go-winio | v0.6.2 | MIT | Windows named pipe transport for the opt-in LuaLS broker |
| github.com/HugoSmits86/nativewebp | v1.3.0 | MIT | lossless WebP encode/decode in `internal/records/image_export.go` |
| github.com/makiuchi-d/gozxing | v0.1.1 | MIT + Apache-2.0 (upstream ZXing core) | QR decode/encode in `internal/desktop/symbols.go` and tests |
| github.com/yuin/gopher-lua | v1.1.1 | MIT | Lua 5.1 parse/AST in `internal/codebase/{lua,descriptors}.go` |
| github.com/dustin/go-humanize | v1.0.1 | MIT | via modernc.org/sqlite |
| github.com/mattn/go-isatty | v0.0.20 | MIT | via modernc.org/libc |
| github.com/ncruces/go-strftime | v0.1.9 | MIT | via modernc.org/libc |
| github.com/remyoudompheng/bigfft | 2023-01-29 | BSD-3-Clause | via modernc.org/mathutil |
| golang.org/x/exp | 2025-06-20 | BSD-3-Clause | via modernc.org/libc |
| golang.org/x/image | v0.24.0 | BSD-3-Clause | WebP/RIFF/VP8(L) decoding via nativewebp |
| golang.org/x/sys | v0.36.0 | BSD-3-Clause | terminal detection via go-isatty |
| golang.org/x/text | v0.22.0 | BSD-3-Clause | character-set decoding via gozxing |
| golang.org/x/xerrors | 2020-08-04 | BSD-3-Clause | via gozxing |
| modernc.org/libc | v1.66.10 | BSD-3-Clause (+ LICENSE-GO) | pure-Go SQLite runtime |
| modernc.org/mathutil | v1.7.1 | BSD-3-Clause | via modernc.org/libc |
| modernc.org/memory | v1.11.0 | BSD-3-Clause (+ LICENSE-GO, LICENSE-MMAP-GO) | via modernc.org/libc |
| modernc.org/sqlite | v1.39.1 | BSD-3-Clause (SQLite core public domain) | `internal/codebase/index.go`, `internal/vault/metadata.go` |

gozxing is a port of the ZXing core library (Apache-2.0, Copyright 2007-2018
ZXing authors); its distributed LICENSE carries both that Apache-2.0 text and
its own MIT text, and both are reproduced in the npm notices file. Build- and
test-only modules (`modernc.org/cc/v4`, `ccgo/v4`, `gc/v2`, `goabi0`,
`fileutil`, `opt`, `sortutil`, `strutil`, `token`, `golang.org/x/{mod,sync,tools}`,
`github.com/google/{uuid,pprof,go-cmp}`, `github.com/chzyer/*`) are not compiled
into the shipped binaries and are not redistributed.

The Lua 5.1.5 interpreter used by the test matrix is built at test time from
the [official Lua source](https://www.lua.org/ftp/) (MIT, PUC-Rio) and is not
redistributed.

## Legacy tree (retired 2026-09-23; history in git, never shipped in 2.0 payloads)

`add-on/`, `packages/cli/` and `Lychee Dev skill/` are declared MIT baseline
inputs (design.md section 12) and are not packaged by the 2.0 pipeline. Their
own third-party content, for completeness:

- `add-on/Libs/AutomationQR.lua` (and its generated copies under
  `packages/cli/vendor/`): the luaqrcode BSD-3-Clause adaptation above.
- `add-on/Modules/Events/CatalogData_*.lua` (and vendored copies): the Blizzard
  event-catalog data above.
- `add-on/docs/validation/2026-09-12-file-delivery/` embeds short verbatim
  excerpts of Blizzard interface source (for example quoted lines of
  `Blizzard_DebugTools.toc` and `ChatFrameOverrides.lua`) as investigation
  evidence. Same provenance as the event catalogs; not shipped in payloads.
- `add-on/Media/GitHub.png` (GitHub logo/mark) and `add-on/Media/Logo.png`
  (lychee illustration) are redistributed in the legacy addon ZIP and npm
  vendor payload with no license or attribution recorded in the repository;
  their provenance is unverified and is flagged for the owner. The 2.0
  `addon/` payload ships neither image.
- `Lychee Dev skill/scripts/requirements.txt` pins the retired 1.x Python
  helpers (`zxing-cpp==3.1.1`, `windows-capture==2.0.1`, `numpy==2.5.3`,
  `opencv-python==5.0.0.93`). They are historical provenance only: Toolkit 2.0
  does not install them, does not use `~/.lycheedev/python`, and does not
  redistribute them.
- The legacy TOCs declare `## Dependencies: !BugGrabber`; that addon is not
  bundled.
- `Analyze/` holds third-party addons used as comparison material (for example
  DevTool with Ace3/LibStub/CallbackHandler bundles, MIT per `Analyze/DevTool/LICENSE`;
  BraunerrsDevTools, AGPL-3.0 per its LICENSE) and is excluded from all payloads.

The repository root `LICENSE` is the authoritative combined-work license file;
the npm package copies that file byte-for-byte during release assembly.

## Blizzard event catalog data (conditional)

`add-on/Modules/Events/CatalogData_*.lua` in the legacy tree (and its vendored
copies in `packages/cli/vendor/`) are generated from the game client's
`Blizzard_APIDocumentationGenerated` interface files as mirrored by
[wow-ui-source](https://github.com/Gethe/wow-ui-source), with the source commit
recorded in each generated file header. That data is Blizzard Entertainment
material used for factual API metadata. The 2.0 `addon/` payload currently
ships **no** event-catalog data; if such data is added to `addon/` for release,
this attribution must travel with it and the addon ZIP/npm payload must carry
the Blizzard trademark attribution recorded in the npm notices file. Historical
ZIPs under `publish/` and `add-on/publish/` (legacy release artifacts containing
the same luaqrcode, catalog and Media files) and analysis copies under
`Analyze/` (third-party addons
with their own licenses, e.g. Ace3/LibStub bundles) are investigation material
and are not part of any 2.0 payload.

Releases include the project LICENSE, applicable third-party notices and
corresponding source. The owner's project licensing decision is recorded in
[the licensing record](docs/toolkit/licensing.md); release artifact and integrity
checks remain required by the release contract.

## EmmyLua-derived owned semantic code

The owned control-flow, relation, member, transform, signature, diagnostic and
checker implementations in
`crates/lycheedev-emmy-adapter/src/session/backend/` adapt portions of
[EmmyLua Analyzer Rust, commit 08b0873962e83bb8307e9a81052e3f41578fbc78](https://github.com/CppCXY/emmylua-analyzer-rust/tree/08b0873962e83bb8307e9a81052e3f41578fbc78).
Those portions remain copyright 2024–2026 CppCXY, licensed under MIT. The exact
upstream permission and warranty notice is preserved in
[the adapter's EmmyLua license](crates/lycheedev-emmy-adapter/LICENSE-EmmyLua)
and in the npm package notices. Moving implementation into an owned crate does
not reattribute the upstream code or remove its license requirements.

The production engine is the unmodified fixed upstream tree. Contextual business
semantics and derived diagnostic algorithms live in the owned adapter. A frozen
historical patched reference is retained only for differential tests and keeps
its original attribution; it is not the production engine.

## Go-derived Data HTTP compatibility code

The Rust Data redirect/reference and SOCKS5 compatibility code under
`crates/lycheedev-data/src/remote/transport/` adapts the Go 1.27.1 standard
library's `net/url/url.go`, `net/http/request.go`, and
`net/http/socks_bundle.go` (which bundles `golang.org/x/net/internal/socks`).
These portions remain Copyright 2009 The Go Authors, under BSD-3-Clause.
Toolkit changes adapt the fixed Go behavior to the existing asynchronous
Rust connector and IO owners; the upstream attribution is retained.
The complete copyright, conditions and disclaimer are in
[the transport Go license](crates/lycheedev-data/src/remote/transport/GO-LICENSE)
and in the distributed npm package notices.

## Pinned Windows TLS name adaptation

`vendor/native-tls-0.2.18` preserves the rust-native-tls Developers' MIT OR
Apache-2.0 license and copyright in [LICENSE-MIT](vendor/native-tls-0.2.18/LICENSE-MIT)
and [LICENSE-APACHE](vendor/native-tls-0.2.18/LICENSE-APACHE).
`vendor/schannel-0.1.29` preserves Copyright (c) 2015 steffengy and its
[MIT license](vendor/schannel-0.1.29/LICENSE.md).

These exact official registry releases have a narrowly identified local
adaptation: separate SNI and certificate-verification names inside the existing
Windows connector. Original chain/name validation, trust policy and asynchronous
owners remain in their original libraries. They are not unmodified upstream
packages; the complete source identities and replayable three-file delta are in
[the adaptation record](vendor/tls-name-adaptation.json) and
[patch](vendor/tls-verification-name.patch). Other platforms use the original
native-tls API and implementation. The unmodified Emmy dependency requirement
is separate and unchanged.
