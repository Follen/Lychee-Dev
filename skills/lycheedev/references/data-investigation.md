# Data investigation

Use this branch for static game records, SQL, Hotfixes and their preparation
failures. Follow the decisions below; open only the reference needed now.
The CLI owns product routing, archive recovery and verification.

## 1. Choose the evidence

| Requested answer | Next step |
| --- | --- |
| Names, IDs, static coefficients or related records | Static lookup below; details in [data-tables.md](data-tables.md) |
| Changed records or retained Hotfix pushes | [data-hotfix.md](data-hotfix.md) |
| Values after applying a specified cache | Explicit `effective` query in [data-query-recipes.md](data-query-recipes.md) |
| Actual server behavior or measured coefficients | Establish the static baseline, then [live-investigation.md](live-investigation.md) when live work is authorized |

Static rows, client Hotfixes and server measurements are distinct evidence.
Do not silently substitute one for another or open a game for a static-only task.

## 2. Fix the identity once

Use an explicit snapshot first, otherwise the invoking project's matching data
pin. Inspect it with `target show <pin>`; use `project status` if project context
is unclear. Check product, full build, region, locale and definitions against the
request. A conflicting project pin must not silently answer a different product:
use a matching explicit pin when the requested scope is clear; ask only for a
remaining identity choice. Do not overwrite the project lock for an investigation.

No matching pin? Read [data-targets.md](data-targets.md). `target list` lists saved
names, not supported products. Use canonical `forever`; the CLI determines its
publishing slot. Folder names and conversation language do not establish identity.
Choose the requested local installation or CDN source, then retain that source
and pin throughout the query. Different delivery configurations require separate,
labeled pins; they are not interchangeable evidence.
For local resolution, constrain the game root/client path with the requested
product; use returned candidates only when selection remains ambiguous.

## 3. Query the smallest useful scope

- Verified ID: prefer the domain command that owns the needed relationship;
  otherwise inspect table schema and query the ID or foreign key.
- Business name only: inspect the relevant name table schema, search its actual
  localized field with an explicit limit, then resolve candidates by relationships
  and class/rank/context. Do not pick the first matching name or ask the user to
  supply a table/ID the toolkit can discover. If evidence leaves multiple valid
  interpretations, present the remaining choices.
- Multiple tables, filtering or calculation: inspect the schemas and use SQL
  with named parameters. Read [data-tables.md](data-tables.md) for mechanics and
  [data-query-recipes.md](data-query-recipes.md) for parameter/overlay examples.

Use installed `describe --format json` or help for exact commands and flags.
If the installed CLI lacks a documented capability, report that version mismatch;
do not claim source-only changes are installed or invent a replacement flag.

## 4. Decide whether the evidence answers the question

Read structured errors and coverage, not just exit status. Retain the snapshot,
source, record IDs, filters and capture IDs with the conclusion.

| Result | Decision |
| --- | --- |
| Complete for the requested scope | Answer within that scope; SQL LIMIT and a single record do not prove whole-table coverage |
| More pages/truncated | Use a returned cursor with unchanged pin/source/filters; search without a cursor needs the bounded alternative below |
| Missing keys/partitions or partial result | Use readable evidence with the limitation; empty partial results cannot establish absence |
| Failure/deadline | [data-recovery.md](data-recovery.md); correct the cause or stop the affected branch after bounded retry |

For JSONL, require a legal end frame and exit 0, then inspect coverage separately.
`db2 search` and `foreign-key` have no cursor: narrow to the requested scope,
increase the limit within documented bounds, or use ordered SQL pagination.
Do not invent `--after-id` for those verbs. With SQL, retain a stable unique order
and filters across pages; use the last ID as a parameter for keyset continuation.
For Hotfix continuation, retain the archived source/checkpoint rather than reread
a mutable cache. A healthy workspace or successful target resolution does not
prove that the requested content is available.

Finish with the finding, fixed provenance and unresolved scope. Static evidence
can be a completed baseline while server-side fitting remains unverified.
