# Asset search and export

Use this workflow for locating files, inspecting encoding/content keys,
exporting raw files, decoding images, or demuxing supported media.

## Locate before writing

### Raw inspection and export

The current native CLI can archive a complete CASC file into the new
workspace using an already resolved DataPin:

```text
lycheedev asset inspect --snapshot <pin> --installation <game-root> --file-id <id> --max-bytes 134217728 --format json
```

The installation may be the selected client directory or its game root containing
`.build.info` and `Data/`. This command reads game archives and writes only Toolkit evidence,
not game files. It performs no network access or game input. The pin must carry
canonical Toolkit product, explicit region/language, full build, both config
keys and exact definition commit. Use the local target preparation described in
[data-investigation.md](data-investigation.md); do not invent missing values
to satisfy `target resolve --file`.

For CDN content, replace `--installation <game-root>` with `--cdn`. This uses
the pin's exact configurations and the same Root/content verification, not a
fresh latest-build selection. `--offline` reuses verified workspace fragments
only and fails on a cache miss. Local and CDN selectors cannot be combined;
neither silently falls back to the other. Inspect `result.source` to retain
which source supplied the file. CDN reads do not send game input either.

Inspect `result.entry`, `result.content`, `result.pin` and `captures`. Content
and Root have complete content-key checks; the Encoding index has encoding-key
and visited-page checks, not a full decoded content-key scan. Region is a
declared selection, not independently established by a local archive. A changed
build, ambiguous locale variant, missing file/key, or size limit is an error;
do not switch locale/build or describe it as an empty successful query.
The current CLI does not yet accept a decryption-key provider.

`--max-bytes` bounds both encoded and decoded selected-file sizes (default
128 MiB, maximum 512 MiB); Encoding/Root each have a 512 MiB bound. A completed
raw capture does not prove DB2 semantics, image conversion, export or demux.

Use `asset export` when the user requests an actual raw file:

```text
lycheedev asset export --project <directory> --cdn --offline --file-id <id> --output <file> --format json
```

It shares the same source selectors, fixed snapshot/project selection and size
bounds as inspection. The output parent must already exist, outside the managed
workspace. Default publication refuses an existing file, even if identical.
Use `--overwrite` only when replacement is in scope; a directory or symlink leaf
is not an export file. Source errors and cancellation before publication preserve
the previous file. A crashed process may leave a hidden staging file, not a
completed artifact; do not infer completion from its presence.

On success, `result` is the export manifest: canonical output path, encoding,
byte count/SHA-256, fixed source identity and source capture. For raw export, the
two `captures` preserve the original bytes and that manifest. `evidence verify` verifies archived
bytes, not whether an external file still exists or was later changed. Publication
may have succeeded if stdout was lost: inspect the explicit output and compare
its hash before deciding to repeat or overwrite it. Do not use an archived manifest
alone as proof that publication succeeded.

### Image conversion

For BLP2 images, use the same export command with `--encoding png` or `webp`
(lossless). The default is `raw` regardless of the output filename; naming a raw
BLP file `.png` does not convert it.

```text
lycheedev asset export --project <directory> --installation <client> --offline --file-id <id> --encoding png --output <file.png> --format json
```

`--mipmap <0..15>` selects an existing level (default 0), not a request to resize.
`--channels` selects a lowercase subset of `rgba` (default `rgba`). A single
channel, including `a`, produces opaque grayscale for inspection. Multiple
channels retain their source values; omitted RGB becomes zero and omitted alpha
becomes opaque. Use `--channels a` when investigating opacity, not when the user
simply wants an icon with transparency.

The manifest's `image.texture` describes the selected level and source storage;
`image.pixelsSHA256` hashes straight-alpha RGBA bytes after channel selection.
`content` describes the output file, while `source.content` remains the original
BLP. Image exports return three captures: original source, manifest and encoded
artifact. Compare decoded pixels for lossless equivalence, not PNG/WebP file hashes.

Supported storage is palette with 0/1/4/8-bit alpha, BC1/BC2/BC3, and BGRA8.
Unsupported formats, missing mip levels and truncated payloads fail without
publishing a partial image. `--max-pixels` bounds the selected level before pixel
allocation (default 16,777,216, maximum 67,108,864); WebP dimensions must each be
at most 16,384. `--max-bytes` also bounds encoded output, not total process memory.
The lossless WebP encoder buffers internally and cannot interrupt its CPU phase;
cancellation is checked before/after encoding and before publication, not a hard
encoding deadline. Choose a smaller existing mip or pixel limit for large images.

### Name search and media demux

Find named files in a pinned target with a supported listfile:

```text
lycheedev asset search --snapshot <pin> --listfile <community-csv|wowexport-text|wowexport-binary> --query <text> --limit 50 --format json
lycheedev asset search --snapshot <pin> --listfile <community-csv|wowexport-text|wowexport-binary> --extension blp --limit 50 --format json
lycheedev asset search --snapshot <pin> --listfile <community-csv|wowexport-text|wowexport-binary> --name 'Interface/Icons/*' --format json
lycheedev asset search --snapshot <pin> --listfile <community-csv|wowexport-text|wowexport-binary> --file-id <id> --format json
```

Choose exactly one lookup mode: `--query <text>`, `--extension <ext>`,
`--name <path>`, or `--file-id <id>`. `--limit` bounds text search and extension
pages. `--max-bytes` bounds total listfile input (default 256 MiB, maximum
512 MiB); the current community listfile exceeds 150 MB. A budget or download
failure is not a negative file lookup. The command reuses a verified cached
listfile when available. Search
results identify candidates; inspect or export a selected file ID before
interpreting its contents.

Demux a supported VP9 AVI from a local file or pinned CASC source:

```text
lycheedev asset demux --path <local-file> --output <existing-directory> --max-frames 500 --format json
```

For CASC input, supply `--snapshot <pin> --file-id <id>` and exactly one of
`--installation <client-or-game-root>` or `--cdn`. Frame, byte and partial
output bounds are explicit; retain the returned completion and truncation
state. Do not treat partial output as a complete media export.

Confirm the resolved file data ID or path, size, encoding, content hash, build,
and locale where applicable before interpreting the artifact. Metadata is not
raw content; request an explicit export when the user needs bytes or a decoded
file.

Do not copy an entire game data store to create evidence. Store the smallest
requested artifact plus its manifest and fixed source identity. If the command
reports a partial or bounded result, preserve that status rather than treating
the artifact as a complete asset set.
