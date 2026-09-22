# Source attribution for the 2.0 development tree

Inventory basis: repository HEAD `87efeed83cb3f90b69ccd9a8765c28a6925b5aae`
plus the uncommitted 2.0 tree observed 2026-09-23. Go module versions are the
versions pinned in `go.sum`; "compiled into the binaries" was verified with
`go version -m` on a `-trimpath` build of `./cmd/lycheedev`. Full license texts
for everything compiled into the shipped binaries are reproduced in
[`packages/npm/lycheedev/THIRD_PARTY_NOTICES`](packages/npm/lycheedev/THIRD_PARTY_NOTICES),
which is staged into the npm payload.

## Owner license decision (2026-09-23)

The project owner resolved the combined-work licensing decision as the
recommended option (iii)+(iv): same-author relicensing plus dual licensing with
per-file annotation. The git history of this repository, wowdata
(https://github.com/follenfang/wowdata) and wowdoc
(https://github.com/Follen/wowdoc) shows a single author (Follen / FollenFang)
with no external contributors, so the sole copyright holder grants an
additional MIT license option for every wowdata-derived file: those files are
dual-licensed AGPL-3.0-or-later OR MIT, keeping their per-file
`SPDX-License-Identifier: AGPL-3.0-or-later` markers and provenance comments.
The combined work is distributed under MIT
([package LICENSE](packages/npm/lycheedev/LICENSE)); the corresponding source
for the AGPL-3.0-or-later option continues to be delivered through the release
source archive and GitHub Release attachments recorded in the package notices.
wowdoc-derived portions remain MIT with attribution; all other third-party
components retain their own licenses in the sections below. The package
manifest declares `"license": "MIT"` while staying `private` until the 2.0.0
release freeze flips it for publication.

## luaqrcode

`addon/Bridge/MatrixSymbol.lua` adapts Patrick Gundlach and contributors'
`speedata/luaqrcode` (https://github.com/speedata/luaqrcode) from the repository's
existing licensed copy at
`add-on/Libs/AutomationQR.lua` (Lychee baseline
`41af9cb616dcb7a9e604619a4fc0ad6c8d6d8210`). The full BSD-3-Clause notice is
retained in the distributed Lua source. Toolkit changes provide a private,
lazy, bounded interface with fixed M correction and replace the 256x256 byte
XOR table with a 16x16 nibble table. The third-party QR algorithm is not
represented as newly authored toolkit code; no old automation entrypoint or
user data is loaded.

## wowdata

wowdata (`D:/Code/wow/wowdata`, https://github.com/follenfang/wowdata) at
baseline commit `6191d3dc567966b7a474849f3a11e7411390091c` declares
**AGPL-3.0-or-later**. Every toolkit file adapted from it carries
`// SPDX-License-Identifier: AGPL-3.0-or-later` on line 1 plus a one-line
provenance comment. The current complete list (31 files, grown after the review
snapshot as further adapted files landed):
`internal/records/{cache_scan,catalog,cdn_index,encode_csv,encoding,
encoding_test,filemeta,filenames,hotfix_remote,hotfix_snapshot,hotfix_wago,
hotfix_wago_cache,hotfix_wago_parse,listfile,listfile_source,local_object,
remote_catalog,root,video_demux}.go`,
`internal/records/archive/{index,span}.go`,
`internal/records/container/{cipher,decode,layout,ranges}.go`,
`internal/records/schema/definition.go`,
`internal/records/table/{columns,layout,records}.go`,
`internal/records/texture/decode.go`,
`internal/records/video/avi.go`.
Files without that marker
(e.g. `cache_fields.go`, `cdn_files.go`, `encoding_size.go`, `table/{page,sparse,
strings,stream,view,enumerate}.go`, `schema/manifest.go`, `relational/`,
`remote_target.go`, `archive/checksum.go`) implement published file formats or
new toolkit policy and are not claimed as wowdata adaptations. Provenance-review
caveat: `cache_fields.go` (XFTH payload field decoding) is deliberately unmarked
and attributes its semantics to the DBCD readers, but its sequential-decode shape
parallels wowdata's `internal/hotfix/decoder.go` (it uses zero-terminated strings
where wowdata uses length-prefixed ones), so its independence is recorded as
uncertain pending the owner's determination.

Modified-work statement (AGPL-3.0-or-later section 5): all files listed above are
modified works derived from wowdata; the modifications were made during the
2026 Lychee Dev Toolkit 2.0 rewrite and are described per file below. They are
distributed under AGPL-3.0-or-later, not MIT. When the combined work is
distributed, AGPL-3.0-or-later applies to it as a whole; the corresponding
source mechanism for the binaries is recorded in
`packages/npm/lycheedev/THIRD_PARTY_NOTICES`.

License reference: https://www.gnu.org/licenses/agpl-3.0.html

Before distributing the combined toolkit, complete the combined-work license
review, include the full applicable license text and corresponding source, and
retain all upstream notices. This development notice does not declare the rest
of the repository relicensed or satisfy the final release-license gate.

`internal/records/texture/decode.go` adapts BLP2 container and block decoding
from wowdata's `internal/blp/blp.go` at the baseline below, under
AGPL-3.0-or-later. The new decoder owns no files or global image registrations,
validates mip spans before allocation, decodes independent palette alpha planes,
and rejects incomplete selected levels. Block semantics were cross-checked with
[Microsoft's BC1/BC2/BC3 documentation](https://learn.microsoft.com/en-us/windows/win32/direct3d10/d3d10-graphics-programming-guide-resources-block-compression).
No game texture fixture is distributed with its synthetic pixel tests.

`internal/records/cache_scan.go` adapts XFTH DBCache record layouts from wowdata's
`internal/hotfix/dbcache.go` under AGPL-3.0-or-later (baseline below). The new
implementation preserves physical records, bounds output, validates the whole
input, and rejects ambiguous version-8 framing; it does not reuse the old query,
sidecar, workspace or overlay policy. Layouts were cross-checked against
[DBCD HTFXReader](https://github.com/wowdev/DBCD/blob/master/DBCD.IO/Readers/HTFXReader.cs).
No DBCD runtime or reflection-based field decoder is bundled. The toolkit's
`internal/records/cache_fields.go` implements sequential XFTH field decoding
against the shared pinned DBD schema. Zero-terminated strings, header-supplied
non-inline IDs and payload-supplied relations were checked against that reader
and [DBCDBuilder's metadata typing](https://github.com/wowdev/DBCD/blob/master/DBCD/DBCDBuilder.cs).
It does not reuse wowdata's length-prefixed string decoder or silently consume
only a prefix of a payload.

`internal/records/container/decode.go`, `layout.go`, `ranges.go` and `cipher.go` adapt BLTE format and
Salsa20 handling from
`D:/Code/wow/wowdata`, baseline commit
`6191d3dc567966b7a474849f3a11e7411390091c`.
The source repository declares **AGPL-3.0-or-later**. This adapted file is
distributed under that license, not MIT. Its implementation has been redesigned
for explicit budgets, streaming output, cancellation, and chunk validation; the
old tool's runtime, global key provider, workspace and CLI are not imported.

`internal/records/archive/index.go` and `span.go` also adapt wowdata CASC layout
handling under AGPL-3.0-or-later. Format checks were cross-checked against
[CascLib's format declarations](https://github.com/ladislav-zezula/CascLib/blob/master/src/CascStructs.h)
and [index reader](https://github.com/ladislav-zezula/CascLib/blob/master/src/CascIndexFiles.cpp).
No CascLib runtime is bundled.

`internal/records/catalog.go`, `encoding.go`, `remote_catalog.go` and `local_object.go` adapt the wowdata metadata
and installed-archive workflows under AGPL-3.0-or-later, with new explicit build
selection, confined paths, configuration byte verification and index generation
selection. They do not import the old tool's workspace or runtime.

Remote target metadata uses the same configuration parser. Release/CDN catalog
fields and HTTPS configuration addressing were checked against the live TACT
version service and [TACTSharp's CDN reader](https://github.com/wowdev/TACTSharp/blob/main/TACTSharp/CDN.cs).
`remote_target.go` owns bounded retrieval, immutable pinning and explicit offline
observations; it does not import wowdata's mutable remote source or stale-on-error
cache policy, or bundle TACTSharp.

`internal/records/cdn_index.go` adapts TACT archive/group/loose index layouts
from wowdata's `internal/casc/locator.go` under AGPL-3.0-or-later. Footer,
TOC and page authentication are implemented in the new reader and checked
against real keyed CDN indexes and the CascLib declarations linked above.
`cdn_files.go` implements the toolkit's bounded range cache and encoded-object
adapter; it does not import the old remote source or resource scheduler.

`internal/records/root.go` adapts wowdata's Root block layouts under
AGPL-3.0-or-later, with explicit bounded ReaderAt projection and request-owned
results. Root header and group formats were cross-checked against
[CascLib's Root reader](https://github.com/ladislav-zezula/CascLib/blob/master/src/CascRootFile_WoW.cpp).
It does not reuse wowdata's mutable source registry or implicit locale choice.

`internal/records/listfile.go` and `listfile_source.go` adapt wowdata's listfile
handling under AGPL-3.0-or-later (community CSV, wow.export text and wow.export
binary source kinds with explicit per-kind provenance). No community listfile
data is redistributed by the toolkit; only the pinned default source URLs are
recorded, and nothing downloads until a caller selects a kind and allows network
access.

`internal/records/table/layout.go`, `columns.go`, and `records.go` adapt wowdata's
WDC2-5 binary layouts, field storage, and identity/relationship semantics under
AGPL-3.0-or-later. It exposes bounded structural inspection with preserved table
and layout identities plus request-owned column decoding, not the old reader's
query engine or mutable row caches.

WDC string addressing and packed-width semantics were cross-checked against
[DBCD WDC2Reader](https://github.com/wowdev/DBCD/blob/master/DBCD.IO/Readers/WDC2Reader.cs)
and [DBCD WDC3Reader](https://github.com/wowdev/DBCD/blob/master/DBCD.IO/Readers/WDC3Reader.cs).
The new bounded UTF-8/string-table implementation in `table/strings.go` does
not bundle DBCD or copy its reflection-based row reader implementation.
`internal/records/schema/definition.go` adapts wowdata's DBD parsing concepts
under AGPL-3.0-or-later with strict bounded parsing and ambiguity rejection.
Syntax was checked against the
[WoWDBDefs format specification](https://github.com/wowdev/WoWDBDefs/blob/master/README.md).
Network regression tests read explicit commit-pinned definitions; no upstream
definition collection is bundled by this addition.
`table/sparse.go` implements bounded sparse-map projection using those format
references, including WDC4/5 SecondaryKey-dependent ID/relation ordering:
[WDC4Reader](https://github.com/wowdev/DBCD/blob/master/DBCD.IO/Readers/WDC4Reader.cs),
[WDC5Reader](https://github.com/wowdev/DBCD/blob/master/DBCD.IO/Readers/WDC5Reader.cs).

`internal/records/encoding_size.go` implements the EKey page layout from the
`FILE_ESPEC_ENTRY` format declaration in CascLib's `CascStructs.h` linked above
(16-byte key, big-endian specification index and 40-bit encoded size). No
CascLib implementation or runtime is included by this addition.

`internal/records/video/avi.go` adapts wowdata's
`internal/video/vp9_avi_demuxer.go` AVI/RIFF frame demuxing under
AGPL-3.0-or-later. Frame metadata deliberately preserves the legacy demux
record fields and the legacy integer-truncated microsecond arithmetic so
metadata stays comparable with the old tool; that is behavioral derivation in
addition to the shared RIFF/AVI chunk layout (a published container format).
It does not port wowdata's VP9 superframe parsing beyond what frame indexing
requires.

`internal/records/archive/checksum.go` implements Bob Jenkins' public-domain
lookup3 algorithm (May 2006). Its reference and independent published test vectors
are at https://burtleburtle.net/bob/c/lookup3.c. It is a format corruption check,
not a cryptographic integrity primitive.

The SQL feature baseline for `internal/records/relational/` was reviewed against
wowdata's `internal/sqlquery` at the revision recorded above. The new compiler
uses private syntax nodes, explicit context/depth/token budgets and lexical CTE
scopes; it does not import or invoke the old parser/engine, preserve its runtime
registry, or expose the workspace SQLite connection to user SQL. No file in
`relational/` carries a wowdata-derived marker; the provenance review of this
area is recorded in the review report that accompanies this notice.

## wowdoc

`internal/codebase/` migrates behavior from wowdoc (`D:/Code/wow/wowdoc`,
https://github.com/Follen/wowdoc, npm `@follenfang/wowdoc`, baseline commit
`bd1fa8a010cb2b8d7cea8f21cb971705f54a1423`), licensed **MIT**,
Copyright (c) 2026 follenfang. The MIT permission notice is reproduced in
[`packages/npm/lycheedev/THIRD_PARTY_NOTICES`](packages/npm/lycheedev/THIRD_PARTY_NOTICES).
Portions are close adaptations of wowdoc expression (for example the
`role.go`/`assets.go` role taxonomy, role penalties, asset extension/MIME tables
and asset metadata excerpt taken from wowdoc's `internal/indexer` and
`internal/query`, as their comments state; the Lua/descriptor walker shares
wowdoc's sentinel strings and signature grammar; `closure.go` shares the
escape/missing/unreadable resolution taxonomy and the `load_path_escape` /
`load_file_missing` diagnostic codes), which MIT permits provided the
copyright and permission notice are retained. wowdoc-derived code is not
wowdata-derived code: it does not import AGPL terms.

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

## Legacy tree (baseline inputs, not shipped in any 2.0 payload)

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
- `Lychee Dev skill/scripts/requirements.txt` pins `zxing-cpp==3.1.1`,
  `windows-capture==2.0.1`, `numpy==2.5.3`, `opencv-python==5.0.0.93`. These
  are installed at runtime into `~/.lycheedev/python` and are not redistributed
  by this repository.
- The legacy TOCs declare `## Dependencies: !BugGrabber`; that addon is not
  bundled.
- `Analyze/` holds third-party addons used as comparison material (for example
  DevTool with Ace3/LibStub/CallbackHandler bundles, MIT per `Analyze/DevTool/LICENSE`;
  BraunerrsDevTools, AGPL-3.0 per its LICENSE) and is excluded from all payloads.

No root `LICENSE` file currently exists in this repository even though
`packages/cli/package.json` declares `"license": "MIT"`; that gap must be closed
as part of the release-license gate.

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

Before distributing the combined toolkit, complete the combined-work license
review, include the full applicable license text and corresponding source, and
retain all upstream notices. This development notice does not declare the rest
of the repository relicensed or satisfy the final release-license gate; the
final combined-work licensing determination belongs to the project owner (and,
if they choose, their lawyer).
