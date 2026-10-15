# Historical UI/optical addon source

This archive preserves original bytes from 00fd643908badaf282a5ddbd1e06a88c0e291d29.
There is no executable entry for this archive. Every original path, Git blob,
SHA-256, byte count and relocation reason is in manifest.json. All source files
end in .source, so Go ./..., Lua t_*.lua and Node *.test.mjs discovery cannot
run them. Original relative paths inside source have not been rewritten.
Reconstruct the full original commit for historical investigation; this is
not a standalone runnable checkout or a compatibility layer for today's addon.

Current oracle: Go3.2.3 3766f9cacb52e93a803a626187e2a1d54a445468.
Current Rust candidate: 3.5.0 beta. All old test/failure evidence retains its
original source identity and status. The prior Native-v4 archive is unchanged.

Definitions and MatrixSymbol are additionally retained byte-for-byte in
tests/fixtures/legacy-addon for the queue codec, QR decoder and explicit lab.
Only those named consumers use them. Current production TOC and payloads do
not include any retired addon module. Rejection tests remain current.
