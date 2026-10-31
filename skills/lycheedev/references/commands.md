# Agent command discovery

Start locally with `lycheedev cli search "<task>"`. At most five ranked matches
point to exact schema commands. Then use `lycheedev schema <command...>` and the
leaf command's `--help`. Root/group help shows only immediate children, bounded
at 32; totalChildren and truncated disclose omissions. `describe` retains the
full installed catalog. Recheck after a binary change; a copied catalog is not
installed capability evidence.

Responses are structured, noninteractive and uncolored. Help, search and schema
read command metadata without opening a worker, cache, workspace or game, or
fetching network data. JSON results use stdout; diagnostics use stderr.
A successful discovery call identifies available commands; use the operation's
result to establish what it actually completed.

The `effects` fields describe possible effects. `gameInput: true` includes
connection, recovery and cleanup exchanges; it does not mean input was already
sent. `gameInputWhen` lists required options: `doctor` can send input only
with `--fix`. Its default diagnosis, `live status` and `live session` send none.

For Data, `data agent schema` gives the authoritative versioned task contract.
`schema data agent serve` includes it and exact CLI inputs. Open a single bounded
`data agent serve` process, send typed tasks and keep owned handles for paging.
Use the tasks declared by that schema for catalog, fields, semantics, queries
and navigation. Report missing commands, fields or supported combinations as
capability gaps; retain the selected version and the original task's scope.

`schema source batch` and `schema source validate` expose `fileInput` schemas
and complete examples for their `--file` batch and `--matrix` configuration.
Single-addon validation still takes an addon directory and TOC manifest.

Read each result's captures, completeness and coverage, not just its exit status.
Agent matching-ID JSONL differs from ordinary command begin/end JSONL. See
[data investigation](data-investigation.md) for session and coverage rules.
