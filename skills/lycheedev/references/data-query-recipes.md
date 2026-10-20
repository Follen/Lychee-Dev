# Typed investigation recipes

Use the installed `data agent schema` for exact fields and bounds. Send one JSONL
request at a time or follow its bounded queue rules, with protocol
`lycheedev.data-agent.v1`, a strictly increasing positive id and task. The enclosing
worker binds the immutable snapshot and source. No production legacy SQL fallback.

A static ID query is task=query, query kind=plan, plan op=lookup, with table
{name:"SpellMisc",mode:"static"} and ids:[1]. A project can select named typed
column expressions; sort keys specify expression, descending and nulls_first.
Filters, aggregates and joins are plans, not string-interpolated SQL.

For named flags, first use task=semantics with the static table. Preserve the
returned Field, Kind, Definition, Value, Name, Comment, ConditionField and
ConditionValue columns. They describe original metadata, not a filtered claim
that every flag applies to the selected record.

For effective values, explicitly choose table mode=effective and ordered
hotfix_captures from retained raw cache CAPs. Compare with a static baseline at
the same pin. Merely collecting a cache does not alter static queries.

For domain relationships, task=navigate carries a request with the advertised
kind: item_get, item_models, item_textures, item_geosets, spell_info, spell_auras,
spell_summons, encounter_get, decor_list, decor_get, creature_display or
creature_model. Consult each exact schema rather than guessing selectors.
These use the same engine's authenticated physical rows and mature domain
algorithms, with owned result evidence. They are not substitutes for all generic
relational expressions.

Keep the returned handle for page/export. Task=capture explicitly persists the
complete result and typed request/source proof, not only the current page. Domain
navigation creates its own result CAP. A query handle is not evidence of CAP
commit. A 16 MiB capture limit fails rather than silently emitting a truncated CAP.
After uncertain publication or transport failure inspect capturePublication,
completedCaptures and the catalog. Missing output does not prove no commit;
never automatically repeat a request that may already have committed evidence.
