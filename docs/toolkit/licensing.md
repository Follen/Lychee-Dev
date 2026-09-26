# Project licensing decision — 2026-09-26

The owner, Follen (FollenFang), confirms that wowdata and wowdoc are their own
projects and directs that project-owned code incorporated into Lychee Dev
Toolkit be distributed under Lychee Dev Toolkit's MIT license. The former
projects are being retired; they are not separate third-party dependencies of
this product.

This decision supersedes the dual-license presentation recorded on 2026-09-23
for the current development tree and future releases. The root and npm package
LICENSE files contain the same MIT text, and project-owned source headers use
`SPDX-License-Identifier: MIT`. Historical origin comments, fixtures and audit
records remain provenance, not a separate license requirement for project-owned
code in this tree. Existing releases and previously granted permissions are not
rewritten or revoked.

This decision covers only the owner's code. External libraries, copied external
material and data retain their respective rights and attribution. Their notices
remain in THIRD_PARTY_NOTICES.md and the distributed THIRD_PARTY_NOTICES file.
The release pipeline continues to ship license notices and corresponding source.
