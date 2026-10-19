# Asset search and export

Use a verified FileDataID from the relevant domain relationship when possible.
A listfile helps name discovery; it is a separately captured community listing,
not an authenticated complete build inventory. Missing names do not prove missing
CASC files. Search results are candidates and may have aliases/multiple mappings.
A truncated name search has no invented continuation cursor.

Reuse the [fixed data target](data-targets.md) and requested local/CDN source.
Use installed help for inspect/export/search/demux syntax and formats. An explicit
export already verifies its source; a separate inspect is optional for a known
file. Local game-data reads can still require public key/definition downloads;
offline forbids network and fails on missing verified cache. Missing keys,
ambiguous variants, integrity and size errors are not successful empty exports.
No source/build/locale fallback or hand-built CDN routing is permitted.

## Match the requested artifact

- Raw is the default export encoding regardless of filename. Renaming BLP to
  .png does not convert it. Select a supported encoding explicitly.
- A mip level selects existing pixels, not resizing. Alpha-only channel export
  is opaque grayscale for inspection; preserve RGBA for ordinary transparency.
  Compare decoded pixels for lossless equivalence, not PNG/WebP file hashes.
- A content-variant filter selects the exact standard/low-violence Root flag.
  Locale alone does not select it, opposite variants are not fallbacks, and
  remaining ambiguity must be retained. Shared FileDataID does not prove geometry.
- Supported VP9 AVI demux yields frame payloads and timing, not decoded images
  or a playable transcode. Bounded partial output is distinct from corrupt input.

Encoded/decoded file limits are not peak memory limits. Unsupported formats or
missing mip levels fail. A lossless WebP encode can buffer and cannot preempt its
CPU phase; a smaller existing mip/pixel bound may be appropriate. A raw capture
does not prove image conversion, DB2 interpretation or external publication.

## Publish and verify

Use an existing output parent outside managed Toolkit storage. Replace an
existing artifact only when replacement is in scope. Export only requested bytes
with manifest/captures, not a whole game data store. Keep fixed source, file ID,
encoding, size and hashes; complete raw exports need the complete verified file.

Archived evidence verifies captured bytes, not the continuing existence of an
external file. A lost stdout/interrupt may occur after publication: inspect the
actual output and compare hashes before repeating or overwriting. A staging file
is not completed publication. For demux, frames can precede the completion
manifest, so failure can leave some published frames. Check the manifest and
all expected files; preserve partial status and use a fresh directory when
replacement was not requested. A partial manifest is not a complete asset set.
