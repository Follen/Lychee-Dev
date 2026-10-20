# Agent command discovery

Start locally with `lycheedev cli search "<task>"`. At most five ranked matches
point to exact schema commands. Then use `lycheedev schema <command...>` and the
leaf command's `--help`. Root/group help shows only immediate children, bounded
at 32; totalChildren and truncated disclose omissions. `describe` retains the
full installed catalog. Recheck after a binary change; a copied catalog is not
installed capability evidence.

The command contract drives clap parsing, help and input schemas. Responses are
structured, noninteractive and uncolored; no separate human-terminal mode or
locale guessing is needed. Help/search/schema must not open a worker, cache,
workspace or game or fetch network data. JSON results use stdout; diagnostics
use stderr. A successful discovery call proves advertised metadata, not that a
business operation has executed.

For Data, `data agent schema` gives the authoritative versioned task contract.
`schema data agent serve` includes it and exact CLI inputs. Open a single bounded
`data agent serve` process, send typed tasks and keep owned handles for paging.
The old SQL/DB2/navigation command family is not a production fallback. Missing
commands, fields or supported combinations are explicit capability gaps, not
permission to invent flags or infer equivalence from a different task.

Read each result's captures, completeness and coverage, not just its exit status.
Agent matching-ID JSONL differs from ordinary command begin/end JSONL. See
[data investigation](data-investigation.md) and [candidate boundaries](source-candidate.md).
