# Third-party notices for Lychee Dev Toolkit

Project-owned code uses the [MIT license](LICENSE). The owner's licensing
decision is recorded in [docs/licensing.md](docs/licensing.md). Third-party
components retain their own licenses and attribution.

The [npm notices file](packages/npm/lycheedev/THIRD_PARTY_NOTICES) contains the
full license texts carried by the distribution. Releases also include
corresponding source tied to the commit in `release.json`.

## Lua Language Server

The Windows LuaLS runtime is pinned by [release/tools/luals.json](release/tools/luals.json).
LuaLS is MIT-licensed; its upstream LICENSE is retained in the payload.
Bundled Microsoft Visual C++ runtime DLLs retain Microsoft's redistribution
terms, as recorded in the npm notices. Those DLLs are not covered by LuaLS's MIT
license.

## Go dependencies

Versions and integrity are pinned in [go.mod](go.mod) and [go.sum](go.sum).
Distributed notices cover the following components and their applicable licenses:

| Component | License |
| --- | --- |
| Microsoft/go-winio | MIT |
| DuckDB engine, duckdb-go and duckdb-go-bindings | MIT; bundled native component notices retained |
| go-viper/mapstructure/v2 | MIT |
| google/uuid | BSD-3-Clause |
| HugoSmits86/nativewebp | MIT |
| yuin/gopher-lua | MIT |
| spf13/cobra, inconshreveable/mousetrap | Apache-2.0 |
| spf13/pflag, golang/snappy, google/go-querystring | BSD-3-Clause |
| gorilla/websocket | BSD-2-Clause |
| tencentyun/cos-go-sdk-v5, clbanning/mxj, mitchellh/mapstructure, mozillazg/go-httpheader | MIT |
| dustin/go-humanize, mattn/go-isatty, ncruces/go-strftime | MIT |
| remyoudompheng/bigfft | BSD-3-Clause |
| golang.org/x/exp, image, net, sys | BSD-3-Clause; applicable patent grant retained |
| modernc.org/libc, mathutil, memory | BSD-3-Clause and applicable bundled notices |
| modernc.org/sqlite | BSD-3-Clause; SQLite core is public domain |

`golang.org/x/image` is used by ordinary nativewebp builds. The release source bundle and dependency inventory are generated from the actual build.

DuckDB 2.0.0 is statically linked through the pinned official Windows module.
Its source is `d54cbbe3df17e8e5ce68312f483b4155836112aa`; upstream library
provenance is retained in the source bundle. Bundled native components,
Unicode data and TPC generators keep their upstream notices in the npm file.
GCC 14.2 runtime libraries carry GPLv3 with the GCC Runtime Library Exception
3.1; the MinGW-w64 and winpthreads notices are retained as well. The compiler
is a build input, not part of the application payload. The Arrow modules in
go.mod are dependencies of the optional driver build tag and are absent from
the shipped CLI's runtime dependency graph.

The Lua 5.1.5 test interpreter is built from the official Lua source under its
MIT license and is not distributed as a product component. Test-only fixtures
and adapters retain their own source records.

## Blizzard event metadata

`addon/Modules/Events/CatalogData_*.lua` contains generated factual API metadata
from Blizzard's `Blizzard_APIDocumentationGenerated`, mirrored by
[wow-ui-source](https://github.com/Gethe/wow-ui-source). Each generated file
records its source commit. These catalogs are included in the current addon
payload.

World of Warcraft and Blizzard Battle.net are trademarks or registered
trademarks of Blizzard Entertainment, Inc. The distribution carries the same
attribution in its npm notices.

## Images

The current addon includes `addon/Media/GitHub.png`, `Logo.png` and
`RoundedPanel.png`. GitHub.png is a GitHub mark; Logo.png is a lychee
illustration. The repository has no separate verified provenance or permission
record for those two images. This remains unresolved; they must not be described
as absent from the current payload.

## Historical material

Retired Python helpers, old addon trees, comparison checkouts and temporary
release archives are not current product inputs. Their history remains in Git.
The retained copyright notices, generated source headers and fixture provenance
continue to apply to material present in the current tree.
