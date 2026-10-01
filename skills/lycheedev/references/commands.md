<!-- Generated from `lycheedev describe --format json` by
     `node tools/skill-commands.mjs`. Do not edit by hand. -->

# Command reference

89 implemented commands. Every command accepts `--format text|json|jsonl`;
`--help` works on the root and on any command. Read the JSON envelope, not
just the exit code; preserve capture IDs and partial/truncated warnings.

## addon

- `addon status` — read-only. Inspect addon files with --path <addon-directory>, or resolve a supported client with --installation <client-directory>
  Flags: `--format text|json|jsonl`, `--path <addon-directory>`, `--installation <client-directory>`.
- `addon install` — mutates. Install: --release <distribution-root> --installation <client-directory>; --output <archive> upgrades; --resume --installation <client> --output <archive> recovers without --release
  Flags: `--format text|json|jsonl`, `--release <distribution-root>`, `--installation <client-directory>`, `--output <archive>`, `--resume`.
- `addon remove` — mutates. Archive an unchanged owned addon: --installation <client-directory> --output <recovery-directory>; never deletes SavedVariables
  Flags: `--format text|json|jsonl`, `--installation <client-directory>`, `--output <recovery-directory>`.

## asset

- `asset inspect` — mutates. Verify CASC file: --snapshot <pin> (--installation <client-or-game-root> | --cdn) --file-id <id> [--max-bytes <n>] [--offline] [--content-variant standard|low-violence]; archive original bytes; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--key-file <WoW.txt|keys.json>`, `--content-variant <standard|low-violence> (default: strict)`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--file-id <id>`, `--max-bytes <n>`, `--offline`.
- `asset search` — mutates. Use one listfile mode: --query <text>, --extension <ext>, --name <path>, or --file-id <id>; requires --snapshot and --listfile <community-csv|wowexport-text|wowexport-binary>; --limit bounds search and extension pages; --offline reuses verified cache; --max-bytes bounds total listfile input (default 256 MiB, maximum 512 MiB); omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin>`, `--listfile <community-csv|wowexport-text|wowexport-binary>`, `--query <text>`, `--extension <ext>`, `--name <path>`, `--file-id <id>`, `--limit <1-200>`, `--max-bytes <n>`, `--offline`.
- `asset export` — mutates. Export CASC file: --snapshot <pin> (--installation <client-or-game-root> | --cdn) --file-id <id> --output <file> [--encoding raw|png|webp] [--mipmap <0..15>] [--channels <rgba-subset>] [--max-pixels <n>] [--overwrite] [--max-bytes <n>] [--offline] [--content-variant standard|low-violence]; raw default, BLP2 image conversion, existing parent required; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--key-file <WoW.txt|keys.json>`, `--content-variant <standard|low-violence> (default: strict)`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--file-id <id>`, `--output <file>`, `--encoding raw|png|webp`, `--mipmap <0..15>`, `--channels <rgba-subset>`, `--max-pixels <n>`, `--overwrite`, `--max-bytes <n>`, `--offline`.
- `asset demux` — mutates. Demux a bounded VP9 AVI from --path <local-file> or pinned CASC (--snapshot <pin> and --file-id <id> with --installation or --cdn); --output <existing-directory> [--max-bytes <n>] [--max-frames <n>] [--allow-partial] [--overwrite]; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--key-file <WoW.txt|keys.json>`, `--path <local-file>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--file-id <id>`, `--output <existing-directory>`, `--max-bytes <n>`, `--max-frames <n>`, `--allow-partial`, `--overwrite`, `--offline`.

## cache

- `cache status` — read-only. Report managed cache accounting, protection visibility and the configured limit
  Flags: `--home <root>`, `--format text|json|jsonl`.
- `cache verify` — read-only. Verify every managed cache object against its content hash; strictly read-only, repairs and deletes nothing
  Flags: `--home <root>`, `--format text|json|jsonl`.
- `cache prune` — mutates. Reclaim ordinary cache toward --target-bytes <n> (zero reclaims all ordinary cache), capped by --max-objects <n>; --uncommitted also reclaims staging files; --dry-run reports without deleting; protected and in-use objects are always skipped
  Flags: `--home <root>`, `--format text|json|jsonl`, `--target-bytes <n>`, `--max-objects <n>`, `--uncommitted`, `--dry-run`.

## config

- `config show` — read-only. Read the workspace user preference and resource budget document; a missing document reports the documented defaults
  Flags: `--home <root>`, `--format text|json|jsonl`.
- `config set` — mutates. Update the workspace budget: --cache-max-bytes <n> and/or --download-workers <n>; at least one flag is required
  Flags: `--home <root>`, `--format text|json|jsonl`, `--cache-max-bytes <n>`, `--download-workers <n>`.

## data

- `data hotfix` — mutates. Inspect independent Hotfix records with an explicit source: --source <wago|dbcache|raidbots>. dbcache reads a pinned --snapshot <pin> (--dbcache <DBCache.bin> | --from <cache-capture>); wago queries the remote record set from --product/--build/--region/--locale with an optional --snapshot parent; raidbots reads --raidbots <DBCache.bin> under 30 days. Filters: --table --table-hash --record --push --status --region-id --search and wago --from/--to; --latest selects the largest matching push batch before pagination; dbcache --scan owns bounded pagination with --max-pages and a resumable --cursor; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--source <wago|dbcache|raidbots>`, `--snapshot <pin>`, `--dbcache <DBCache.bin>`, `--raidbots <DBCache.bin>`, `--from <cache-capture-or-time>`, `--to <time>`, `--product <retail|classic|titan|forever>`, `--build <full-build>`, `--region <us|eu|cn|kr|tw>`, `--locale <locale>`, `--table <name>`, `--table-hash <8-hex>`, `--record <n>`, `--push <n>`, `--status <0-255>`, `--region-id <n>`, `--search <text>`, `--latest`, `--scan (dbcache: complete bounded scan; --cursor resumes its capture)`, `--offline`, `--limit <1-200>`, `--cursor <opaque>`, `--page <n>`, `--after-index <n>`, `--max-pages <n>`, `--max-requests <n>`, `--max-bytes <n>`, `--encoding csv`, `--output <file>`.
- `data sql` — mutates. Read-only SQL over pinned static tables, meta.Table enum/flags metadata, or explicit effective.Table overlays (query JSON hotfix array): exactly one of --sql <text>, --file <query.sql|query.json>, or --stdin; repeated --param <name=scalar> binds named values; --encoding csv --output <file> writes a captured CSV and manifest; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--key-file <WoW.txt|keys.json>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--sql <text>`, `--file <query.sql|query.json>`, `--stdin`, `--param <name=scalar>`, `--encoding csv`, `--output <file>`, `--overwrite`, `--max-bytes <n>`, `--offline`, `--timeout-seconds <1-3600> (default 300; entire query)`.
- `data db2` — mutates. Read typed rows: --snapshot <pin> (--installation <client-or-game-root> | --cdn) --table <name> [--id <n> | --limit <n> --after-id <n>] [--max-bytes <n>] [--offline]; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--key-file <WoW.txt|keys.json>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--table <name>`, `--id <n>`, `--limit <n>`, `--after-id <n>`, `--max-bytes <n>`, `--offline`, `--timeout-seconds <1-3600> (default 300; entire query)`.
- `data db2 schema <Table>` — mutates. Read parsed table schema fields, keys and relationship columns: data db2 schema <Table>; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--key-file <WoW.txt|keys.json>`, `--timeout-seconds <1-3600> (default 300; entire query)`.
- `data db2 search <Table>` — mutates. Case-insensitive substring search over one field: data db2 search <Table> --field <name> --query <text> --limit <n>; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--key-file <WoW.txt|keys.json>`, `--field <name>`, `--query <text>`, `--limit <n>`, `--timeout-seconds <1-3600> (default 300; entire query)`.
- `data db2 foreign-key <Table>` — mutates. Select rows by one foreign-key field and value with a mandatory bound: data db2 foreign-key <Table> --field <name> --value <n> --limit <1-5000>; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--key-file <WoW.txt|keys.json>`, `--field <name>`, `--value <n>`, `--limit <n>`, `--timeout-seconds <1-3600> (default 300; entire query)`.
- `data db2 stream <Table>` — mutates. Stream bounded table rows as typed JSONL begin/record/end frames (requires --format jsonl): data db2 stream <Table> [--fields <a,b>] [--filter <field=value>] --limit <n>; only a legal end frame plus exit 0 is a complete success; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--key-file <WoW.txt|keys.json>`, `--fields <a,b>`, `--filter <field=value>`, `--limit <n>`, `--timeout-seconds <1-3600> (default 300; entire query)`.
- `data spell info` — mutates. Resolve the bounded trigger and description-reference closure around one spell: --spell-id <n> [--max-depth <n>]; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--key-file <WoW.txt|keys.json>`, `--spell-id <n>`, `--max-depth <n>`, `--timeout-seconds <1-3600> (default 300; entire query)`.
- `data spell auras` — mutates. Report aura-effect presence for one spell: --spell-id <n>; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--key-file <WoW.txt|keys.json>`, `--spell-id <n>`, `--timeout-seconds <1-3600> (default 300; entire query)`.
- `data spell summons` — mutates. List summon effects of one spell, optionally filtered to one NPC: --spell-id <n> [--npc-id <n>]; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--key-file <WoW.txt|keys.json>`, `--spell-id <n>`, `--npc-id <n>`, `--timeout-seconds <1-3600> (default 300; entire query)`.
- `data item get` — mutates. Read one item summary and slot name: --item-id <n>; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--key-file <WoW.txt|keys.json>`, `--item-id <n>`, `--timeout-seconds <1-3600> (default 300; entire query)`.
- `data item models` — mutates. Resolve model and texture file IDs for one item: --item-id <n> [--race-id <n> --gender <n>]; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--key-file <WoW.txt|keys.json>`, `--item-id <n>`, `--race-id <n>`, `--gender <n>`, `--timeout-seconds <1-3600> (default 300; entire query)`.
- `data item geosets` — mutates. Read geoset and helmet-hide data for one item: --item-id <n>; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--key-file <WoW.txt|keys.json>`, `--item-id <n>`, `--timeout-seconds <1-3600> (default 300; entire query)`.
- `data item textures` — mutates. Read character texture sections for one item: --item-id <n>; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--key-file <WoW.txt|keys.json>`, `--item-id <n>`, `--timeout-seconds <1-3600> (default 300; entire query)`.
- `data creature display` — mutates. Resolve one creature display by exactly one key: (--display-id <n> | --file-data-id <n>); omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--key-file <WoW.txt|keys.json>`, `--display-id <n>`, `--file-data-id <n>`, `--timeout-seconds <1-3600> (default 300; entire query)`.
- `data creature model` — mutates. List every display of one model file with variants: --file-data-id <n>; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--key-file <WoW.txt|keys.json>`, `--file-data-id <n>`, `--timeout-seconds <1-3600> (default 300; entire query)`.
- `data encounter get` — mutates. Build the bounded journal encounter section tree with related spells and the relation manifest: --journal-encounter-id <n> [--max-depth <n>]; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--key-file <WoW.txt|keys.json>`, `--journal-encounter-id <n>`, `--max-depth <n>`, `--timeout-seconds <1-3600> (default 300; entire query)`.
- `data decor list` — mutates. List house decor rows in ascending ID order with a mandatory bound: --limit <1-5000>; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--key-file <WoW.txt|keys.json>`, `--limit <n>`, `--timeout-seconds <1-3600> (default 300; entire query)`.
- `data decor get` — mutates. Resolve one decor by exactly one key: (--id <decor-id> | --item-id <n> | --model-file-data-id <n>); omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--key-file <WoW.txt|keys.json>`, `--id <n>`, `--item-id <n>`, `--model-file-data-id <n>`, `--timeout-seconds <1-3600> (default 300; entire query)`.

## describe

- `describe` — read-only. Read implemented command contracts
  Flags: `--home <root>`, `--format text|json|jsonl`.

## doctor

- `doctor` — read-only. Aggregate read-only workspace, target catalog, bundled LuaLS and source-mirror health checks; the mirror check joins only when the nearest project declares a product, and exit 3 reports that at least one check found an error
  Flags: `--home <root>`, `--format text|json|jsonl`, `--offline`.

## evidence

- `evidence show <capture-id>` — read-only. Read a capture manifest: evidence show <capture-id>
  Flags: `--home <root>`, `--format text|json|jsonl`.
- `evidence verify <capture-id>` — read-only. Verify manifest and original bytes: evidence verify <capture-id>
  Flags: `--home <root>`, `--format text|json|jsonl`.
- `evidence list` — read-only. List archived capture manifests in ID order with a bounded page
  Flags: `--home <root>`, `--format text|json|jsonl`, `--limit <n>`.
- `evidence bundle` — mutates. Write a complete ZIP of 1..100 verified captures: --ids <CAP-a,CAP-b> --output <new-file>; refuses existing output and caps unique payload bytes at 128 MiB
  Flags: `--home <root>`, `--format text|json|jsonl`, `--ids <capture-id,...>`, `--output <new-file>`.
- `evidence keep <capture-id>` — mutates. Record an explicit retention decision for one capture; repeated calls are idempotent: evidence keep <capture-id>
  Flags: `--home <root>`, `--format text|json|jsonl`.
- `evidence remove <capture-id>` — mutates. Explicitly remove one unreferenced capture manifest and its unshared blob: evidence remove <capture-id>; never follows cache pruning or deletes a shared blob
  Flags: `--home <root>`, `--format text|json|jsonl`.

## init

- `init` — mutates. Create a new-format workspace without importing old data; --plan resolves the fresh-isolation plan without changing anything, --fresh isolates an occupied legacy root into a deterministic sibling archive first, --resume completes an interrupted archive switch
  Flags: `--home <root>`, `--format text|json|jsonl`, `--plan`, `--fresh`, `--resume`.

## live

- `live probe put` — mutates. Register immutable bounded Lua source under a mutable name without touching the game: --name <name> --file <probe.lua>; returns a content-addressed revision
  Flags: `--home <root>`, `--format text|json|jsonl`, `--name <name>`, `--file <probe.lua>`.
- `live execute` — mutates. Execute a bounded probe through durable verified result and runtime release; native CON connections use project journals and stable request keys; --policy controls recovery after runtime loss
  Flags: `--home <root>`, `--format text|json|jsonl`, `--file <lua-file>`, `--session <session-id>`, `--probe <name-or-revision>`, `--request <idempotency-key>`, `--budget-seconds <1-120>`, `--account <account>`, `--project <directory>`, `--wait-seconds <1-600> (default 120)`, `--no-cache`, `--policy <opaque|observation> (default opaque)`.
- `live probe load` — mutates. Load one immutable probe revision with a declared 1..120 second execution budget: --session <session-id> --probe <name-or-revision> --request <idempotency-key> --budget-seconds <1-120> [--account <account>]
  Flags: `--home <root>`, `--format text|json|jsonl`, `--session <session-id>`, `--probe <name-or-revision>`, `--request <idempotency-key>`, `--budget-seconds <1-120>`, `--account <account>`.
- `live probe list` — read-only. List active probe names without game input; --include-removed shows tombstoned names
  Flags: `--home <root>`, `--format text|json|jsonl`, `--include-removed`, `--limit <1-1000>`.
- `live probe show <name-or-revision>` — read-only. Read an exact probe by active name or immutable PRB revision: live probe show <name-or-revision>
  Flags: `--home <root>`, `--format text|json|jsonl`.
- `live probe remove <name>` — mutates. Tombstone one probe name for future selection while retaining immutable revisions and operation evidence: live probe remove <name>
  Flags: `--home <root>`, `--format text|json|jsonl`.
- `live run <operation-id>` — mutates. Execute one loaded operation and return its verified report with cleanup pending; acknowledge separately with live ack: live run <operation-id>
  Flags: `--home <root>`, `--format text|json|jsonl`.
- `live instances` — read-only. List supported installations and running windows without game input or actor claims; use live connect for fresh native identity binding
  Flags: `--home <root>`, `--format text|json|jsonl`, `--installation <client-or-game-root>`, `--passive`.
- `live connect` — mutates. Discover and freshly bind a native memory/200-slot connection in the invoking project; optional snapshot constrains build; --session CON-id resumes retained work
  Flags: `--home <root>`, `--format text|json|jsonl`, `--snapshot <pin>`, `--character <name>`, `--realm <realm>`, `--pid <pid>`, `--installation <client>`, `--session <session-id>`, `--project <directory>`, `--wait-seconds <1-600> (default 120)`, `--no-cache`.
- `live reset` — mutates. Unblock a stuck window whose in-game queue blocks identity after abandon or a lost receipt: [--snapshot <pin>] [--character <name>] [--realm <realm>] [--pid <pid>] [--installation <client>]; sends one fixed nonce-correlated reset trigger per unowned matching window, expects its receipt, then reconnects; disk-owned windows are never touched; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin>`, `--character <name>`, `--realm <realm>`, `--pid <pid>`, `--installation <client>`, `--wake-binding <chord>`.
- `live reload` — mutates. Perform one correlated UI reload on the selected client and verify the new runtime: --session <session-id> --request <idempotency-key>
  Flags: `--home <root>`, `--format text|json|jsonl`, `--session <session-id>`, `--request <idempotency-key>`, `--project <directory>`, `--wait-seconds <1-600> (default 120)`, `--no-cache`.
- `live reload fallback` — mutates. Activate a clean managed addon and its 200 slots on an explicit installation/PID with a recorded reload using memory readiness when available (fixed bootstrap otherwise), then verify a new runtime and fresh connection; --request is idempotent
  Flags: `--home <root>`, `--format text|json|jsonl`, `--installation <client>`, `--pid <pid>`, `--request <idempotency-key>`, `--session <prior-session>`, `--wake-binding <chord>`, `--project <directory>`, `--wait-seconds <1-600> (default 120)`, `--no-cache`.
- `live ack <operation-id>` — mutates. Acknowledge one verified operation, retire only its exact queue entry and release its window ownership without forcing another reload: live ack <operation-id>
  Flags: `--home <root>`, `--format text|json|jsonl`.
- `live finish <operation-id>` — mutates. Acknowledge one verified operation and verify its receipt display is cleared; retry the same operation to finish pending display cleanup: live finish <operation-id>
  Flags: `--home <root>`, `--format text|json|jsonl`.
- `live bugs` — mutates. Read 1-100 retained provider errors; native CON connections persist and release the bounded observation result automatically
  Flags: `--home <root>`, `--format text|json|jsonl`, `--session <session-id>`, `--request <idempotency-key>`, `--count <1-100>`, `--account <account>`, `--project <directory>`, `--wait-seconds <1-600> (default 120)`, `--no-cache`.
- `live hide` — mutates. Dismiss the displayed bridge receipt on the selected client after its evidence is archived: --session <session-id>; refuses windows owned by in-flight operations and verifies the clear from valid frames
  Flags: `--home <root>`, `--format text|json|jsonl`, `--session <session-id>`.
- `live bind` — mutates. Observe /dev connect in the selected game, verify effective receiver bindings with one read-only identity transaction, and save a connection: --snapshot <pin>; optional --wake-binding for a custom first-contact wake; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--installation <client>`, `--pid <pid>`, `--snapshot <pin>`, `--character <name>`, `--realm <realm>`, `--capture-area <window|x,y,width,height>`, `--wake-binding <chord>`.
- `live status <operation-id>` — read-only. Read persisted work or a BTP bootstrap attempt without sending input: live status <operation-id>
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`.
- `live resume <operation-id>` — mutates. Recover the exact operation or BTP bootstrap attempt; revalidate identity and never repeat unknown input
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--wait-seconds <1-600> (default 120)`, `--no-cache`.
- `live cancel <operation-id>` — mutates. Cancel only a prepared operation before queue publication or game input: live cancel <operation-id>; later stages require live resume for safe cleanup
  Flags: `--home <root>`, `--format text|json|jsonl`.
- `live abandon <operation-or-bootstrap-id>` — mutates. Explicitly stop unresolved probe, bugs, fixed reload, or bootstrap recovery; preserve evidence and unknown results, release host ownership without game input or claiming business completion
  Flags: `--home <root>`, `--format text|json|jsonl`.
- `live session <session-id>` — read-only. Verify a retained session and its evidence: live session <session-id>; does not reconnect or authorize input
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`.
- `live disconnect <connection-id>` — mutates. Unbind a quiescent memory/slot connection and release its durable host ownership; repeat safely after interruption
  Flags: `--format text|json|jsonl`, `--project <directory>`, `--wait-seconds <1-600> (default 120)`, `--no-cache`.

## project

- `project init` — mutates. Declare a project product: --product <track> [--path <directory>]; creates lycheedev.json without choosing latest
  Flags: `--format text|json|jsonl`, `--product <track>`, `--path <directory>`.
- `project lock` — mutates. Save an existing fixed --snapshot <pin> in lycheedev.lock.json [--path <directory>]; replaces the previous known-format lock
  Flags: `--home <root>`, `--format text|json|jsonl`, `--snapshot <pin>`, `--path <directory>`.
- `project status` — read-only. Read project declaration and lock [--path <directory>]; no workspace or network required
  Flags: `--format text|json|jsonl`, `--path <directory>`.

## skill

- `skill install` — mutates. Install: --release <distribution-root> --path <parent/lycheedev>; add --output <archive> to upgrade; --resume --path <target> --output <archive> resumes without --release
  Flags: `--format text|json|jsonl`, `--release <distribution-root>`, `--path <target>`, `--output <archive>`, `--resume`.
- `skill status` — read-only. Inspect skill ownership and files: --path <installed-directory>
  Flags: `--format text|json|jsonl`, `--path <installed-directory>`.
- `skill remove` — mutates. Archive an unchanged owned skill: --path <parent/lycheedev> --output <recovery-directory outside parent>; deletes nothing
  Flags: `--format text|json|jsonl`, `--path <parent/lycheedev>`, `--output <recovery-directory>`.

## source

- `source list` — read-only. List source repositories and product branches; --snapshot <pin> reports fixed file-mapping readiness; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin>`.
- `source sync` — mutates. Fetch --source <key> --product <track> [--ref <full-ref-or-commit>] and return a fixed snapshot
  Flags: `--home <root>`, `--format text|json|jsonl`, `--source <key>`, `--product <track>`, `--ref <full-ref-or-commit>`.
- `source inspect` — mutates. Inspect one fixed source: --path <file> for original lines, --symbol <qualified-name> for symbol candidates, or --target-path <path> for file/asset facts; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin-id>`, `--path <file>`, `--symbol <qualified-name>`, `--target-path <path>`, `--line <n>`, `--count <n>`.
- `source index` — mutates. Explicitly warm the rebuildable file mapping for --snapshot <pin-id>, retaining parse diagnostics; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin-id>`.
- `source query <term>` — mutates. Prepare and search the exact pinned source mapping by topic and evidence tier with bounded, resumable pages; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin-id>`, `--mode precise|exploratory`, `--topic <api|lua|xml|toc|asset>`, `--limit <1-200>`, `--cursor <token>`.
- `source refs` — mutates. Find bounded relations for one pinned symbol ID (or an unambiguous symbol name); retain relation state, coverage and next cursor; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin-id>`, `--symbol-id <id>`, `--symbol <name>`, `--direction <incoming|outgoing|both>`, `--limit <1-200>`, `--cursor <token>`, `--environment <client-pin>`, `--session-reuse`, `--static-only`, `--release <release-root>`.
- `source context` — mutates. Read bounded source context for one pinned symbol ID (or an unambiguous name), with direct relations and load evidence; --flow requests bounded secret-value analysis; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--snapshot <pin-id>`, `--symbol-id <id>`, `--symbol <name>`, `--limit <1-200>`, `--max-bytes <1-1048576>`, `--max-lines <1-2000>`, `--depth <0-4>`, `--cursor <token>`, `--environment <client-pin>`, `--flow`, `--session-reuse`, `--static-only`, `--release <release-root>`.
- `source session status` — read-only. Inspect the opt-in LuaLS broker for this home without starting it
  Flags: `--home <root>`, `--format text|json|jsonl`, `--release <release-root>`.
- `source session close` — mutates. Retire the verified opt-in LuaLS broker and release its worktree lease
  Flags: `--home <root>`, `--format text|json|jsonl`, `--release <release-root>`.
- `source diff` — mutates. Compare --from <pin-id> --to <pin-id> [--limit <n> per change category]
  Flags: `--home <root>`, `--format text|json|jsonl`, `--from <pin-id>`, `--to <pin-id>`, `--limit <n>`.
- `source prune` — mutates. Explicitly reclaim only unused source worktrees toward --target-bytes <n>; fixed Git pins, facts and captures remain
  Flags: `--home <root>`, `--format text|json|jsonl`, `--target-bytes <n>`.
- `source validate` — mutates. Validate one pinned addon closure with --path/--toc/--snapshot, or a fixed multi-client --matrix <config.json>; add --semantic for bundled LuaLS using fixed client API metadata; report unresolved coverage; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins
  Flags: `--home <root>`, `--format text|json|jsonl`, `--project <directory>`, `--semantic`, `--environment <client-pin>`, `--release <release-root>`, `--path <addon-root>`, `--toc <relative-manifest>`, `--snapshot <pin-id>`, `--matrix <config.json>`.

## target

- `target resolve` — mutates. Pin data identity from --target <name>, --installation <game-root-or-client> optionally constrained by --product <track>, remote --product <track> without installation, or --file <selection.json>; local selection never falls back to remote; named targets resolve their exact configured identity at command start
  Flags: `--home <root>`, `--format text|json|jsonl`, `--file <selection.json>`, `--target <name>`, `--installation <game-root-or-client>`, `--product <retail|classic|titan|forever>`, `--build <full-build>`, `--definitions <commit-or-full-ref>`, `--region <us|eu|cn|kr|tw>`, `--locale <locale>`, `--from <parent-pin>`, `--offline`.
- `target show <name-or-pin-id>` — read-only. Read a named target configuration and its references, or an explicit PIN- selection ID
  Flags: `--home <root>`, `--format text|json|jsonl`.
- `target list` — read-only. List every named target configuration in name order; a brand new workspace has no targets
  Flags: `--home <root>`, `--format text|json|jsonl`.
- `target add <name>` — mutates. Create or explicitly replace one named target: target add <name> --product <track> --region <region> --locale <locale> with exactly one of --installation <client> or --remote; optional --build <full-build> pins that exact release; --replace confirms replacement
  Flags: `--home <root>`, `--format text|json|jsonl`, `--product <retail|classic|titan|forever>`, `--region <us|eu|cn|kr|tw>`, `--locale <locale>`, `--build <full-build>`, `--installation <client-directory>`, `--definitions <commit-or-full-ref>`, `--remote`, `--replace`.
- `target remove <name>` — mutates. Remove one named target and its reference ledger: target remove <name>; resolved pins stay intact, running operations block removal
  Flags: `--home <root>`, `--format text|json|jsonl`.
- `target available` — read-only. List the releases the version manifests serve for one region: --region <region>; one bounded manifest fetch per supported product with per-product failures reported inline; --offline is refused because the listing is inherently current
  Flags: `--format text|json|jsonl`, `--region <us|eu|cn|kr|tw>`, `--offline`.

## update

- `update` — mutates. Update npm CLI and synchronize managed skills/addons; --plan inspects current targets without writes; --release applies an already installed release offline. Repeated --path/--installation scope targets and disable discovery. Filesystem completion does not activate a running game or Agent.
  Flags: `--home <root>`, `--format text|json|jsonl`, `--file <targets.json> (explicit array; disables discovery)`, `--release <distribution-root>`, `--path <skill-directory> (repeatable)`, `--installation <client-directory> (repeatable)`, `--plan`.

## version

- `version` — read-only. Inspect the native toolkit build
  Flags: `--home <root>`, `--format text|json|jsonl`.
