---
name: lycheedev
description: Investigate WoW addon code, APIs, game data and live behavior with Lychee Dev Toolkit. Use for addon errors, compatibility checks, DB2/SQL/Hotfix queries, asset exports, connection recovery, author publishing and toolkit setup. Excludes general gameplay advice and raw process address discovery.
---

# Lychee Dev

Use Lychee Dev to answer the requested WoW question or complete the requested
toolkit operation. Choose the source, data query or live observation that can
answer it with the least preparation.

Invoke `lycheedev` on PATH or `node <skill-directory>/scripts/lycheedev.mjs`.
Start with `lycheedev discover "<task or platform>" --format json`, then read the
returned command's `schema` or `--help`. Discovery works offline. Get accepted
inputs, examples, dynamic options and recovery commands from that CLI version.

| Task | Starting point and relevant guidance |
| --- | --- |
| API, callers, TOC/XML compatibility, secrets or taint | Source; [source research](references/source-research.md) |
| IDs, names, DB2, SQL, Hotfix or assets | Data/Changes; [data queries](references/data-investigation.md) |
| Running-game behavior | Live; [Lua probes](references/live-probes.md) when writing a probe |
| Memory, CPU or repeated-operation cost | [Measurement interpretation](references/runtime-investigations.md) |
| Author publishing or selected cloud backups | Publish; [platform and artifact choices](references/publish.md) |
| Installation, startup or an existing pending operation | Discover the matching doctor, status or recovery command; continue retained state |

`Forever`, `无限服` and `国服无限服` mean the formal product `forever`.
Read its current product metadata from CLI discovery. Retired beta records keep
their original identity; a shared source family is not interchangeable Data or Live.

Reuse the user's selected inputs and matching project pins. Check the source
revision, client API environment, Data build/locale, Hotfix capture and live
process separately. Keep returned PIN/CAP/CON/request IDs for later queries and
recovery. Use an explicit pin when investigating another version without changing
the project's default selection.

Use Live when the task requires running-game evidence and the user has authorized
it. Install or update through the toolkit's managed commands. For pending work,
resume the original project/session/request within its original deadline; do not
resend submitted or uncertain input. If startup requests a manual reload, ask
for one reload in the selected game window. Finish cleanup through the returned
recovery commands and retain the original reports and recovery records.

Read `lycheedev.result.v1` together with its coverage and truncation fields.
Accept JSONL only with a valid end frame and exit 0. In the answer, distinguish
query completion from source coverage, report retrieval from `report.ok`, and
remote submission from readback. Include pending cleanup when present. An empty
result with partial coverage leaves the question open.
