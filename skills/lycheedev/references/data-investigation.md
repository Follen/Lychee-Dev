# Data and asset interpretation

Static DB2 records, client Hotfixes and measured server behavior are distinct
evidence. Fix product, full build, source/configuration, region, locale and
schema identity before comparing records. A successful target resolution proves
identity, not content availability. Historical pins retain their original
product codes and configurations.

For a known ID, prefer the domain query that owns the relationship. For a name,
inspect the relevant localized schema and resolve candidates by related records
and the requested context; the first same-name match is not necessarily the
requested entity. Query the tables needed for the claim. Use SQL when joins,
filters or calculations make the question clearer, with named parameters and
the installed CLI's actual dialect and resource limits.

Keep signed/unsigned 64-bit IDs exact when processing JSON. A double-only client
can round them. DBD field names, array indexes, copy records and the selected
layout affect interpretation; do not assign meaning from column position alone.
A readable logical row count is not the total count of a partially encrypted
table. Missing keys, partitions and chunks remain coverage limits even if one
requested row is available.

A completed query may still have partial source coverage or an intentional
LIMIT. Negative claims require complete readable coverage of the exact filters.
For SQL pagination retain stable unique ordering, filters, PIN and overlay.
Use only the continuation mechanism exposed by that command; never invent a
cursor flag. JSONL is usable only with its valid terminal frame and exit 0.

## Hotfixes and effective values

A local cache and a remote provider have their own scope and history. A complete
local-cache scan does not cover every server Hotfix. Preserve provider, push
identity, raw status, parent Data PIN and capture/checkpoint. Continue the
archived source rather than rereading a changing cache for later pages.

Applying an overlay is explicit. Keep base and effective results labeled, bind
the Hotfix capture to the compatible product/build/table, and preserve source
coverage through the overlay. Provider time limits and empty partial searches
cannot prove that no change exists. A row may be static, overlaid or measured;
state which supports the answer.

## Assets

A listfile name is a discovery candidate; resolve file identity against the
selected build before export. Keep raw bytes separate from decoded image,
selected mip/face/layer, transformed media and bounded demux output. Some formats
have multiple meaningful variants; choose the requested variant explicitly.

Return the generated artifact with its manifest, fixed provenance and hashes.
A capture verifies archived bytes, not the perpetual existence of an external
output file. Preserve `partial`, omitted streams/frames and truncation in an
export manifest; successful file publication does not make partial media whole.
