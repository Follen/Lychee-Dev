<!-- Generated from `lycheedev describe --format json`. Regenerate after any
     command-surface change; tools/skill-contract.mjs fails the build on drift.
     Do not edit by hand. -->

# Command reference

62 implemented commands. Every command accepts `--format text|json|jsonl`;
`--help` works on the root and on any command. Commands whose summary mentions a
snapshot accept `--snapshot <pin>` with project-lock fallback (`--project <directory>`);
workspace commands accept `--home <root>`.

Read the JSON envelope, not just the exit code: `ok` reports the contract outcome,
`error.code`/`error.stage` the failure, `captures` the evidence references, `warnings`
partial/truncated boundaries.

## addon

- `addon status` — read-only. Inspect addon files with --path <addon-directory>, or resolve a supported client with --installation <client-directory>.
  Flags: `--path <addon-directory>`, `--installation <client-directory>`.
- `addon install` — mutates. Install: --release <distribution-root> --installation <client-directory>; --output <archive> upgrades; --resume --installation <client> --output <archive> recovers without --release.
  Flags: `--release <distribution-root>`, `--installation <client-directory>`, `--output <archive>`, `--resume`.
- `addon remove` — mutates. Archive an unchanged owned addon: --installation <client-directory> --output <recovery-directory>; never deletes SavedVariables.
  Flags: `--installation <client-directory>`, `--output <recovery-directory>`.

## asset

- `asset inspect` — mutates. Verify CASC file: --snapshot <pin> (--installation <client-or-game-root> | --cdn) --file-id <id> [--max-bytes <n>] [--offline]; archive original bytes; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--file-id <id>`, `--max-bytes <n>`, `--offline`.
- `asset export` — mutates. Export CASC file: --snapshot <pin> (--installation <client-or-game-root> | --cdn) --file-id <id> --output <file> [--encoding raw|png|webp] [--mipmap <0..15>] [--channels <rgba-subset>] [--max-pixels <n>] [--overwrite] [--max-bytes <n>] [--offline]; raw default, BLP2 image conversion, existing parent required; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--file-id <id>`, `--output <file>`, `--encoding raw|png|webp`, `--mipmap <0..15>`, `--channels <rgba-subset>`, `--max-pixels <n>`, `--overwrite`, `--max-bytes <n>`, `--offline`.

## cache

- `cache status` — read-only. Report managed cache accounting, protection visibility and the configured limit.
- `cache verify` — read-only. Verify every managed cache object against its content hash; strictly read-only, repairs and deletes nothing.
- `cache prune` — mutates. Reclaim ordinary cache toward --target-bytes <n> (zero reclaims all ordinary cache), capped by --max-objects <n>; --uncommitted also reclaims staging files; --dry-run reports without deleting; protected and in-use objects are always skipped.
  Flags: `--target-bytes <n>`, `--max-objects <n>`, `--uncommitted`, `--dry-run`.

## config

- `config show` — read-only. Read the workspace user preference and resource budget document; a missing document reports the documented defaults.
- `config set` — mutates. Update the workspace budget: --cache-max-bytes <n> and/or --download-workers <n>; at least one flag is required.
  Flags: `--cache-max-bytes <n>`, `--download-workers <n>`.

## data

- `data hotfix` — mutates. Inspect independent Hotfix records with an explicit source: --source <wago|dbcache|raidbots>. dbcache reads a pinned --snapshot <pin> (--dbcache <DBCache.bin> | --from <cache-capture>); wago queries the remote record set from --product/--build/--region/--locale with an optional --snapshot parent; raidbots reads --raidbots <DBCache.bin> under 30 days. Filters: --table --table-hash --record --push --status --region-id --search and wago --from/--to; --latest selects the largest matching push batch before pagination; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--source <wago|dbcache|raidbots>`, `--snapshot <pin>`, `--dbcache <DBCache.bin>`, `--raidbots <DBCache.bin>`, `--from <cache-capture-or-time>`, `--to <time>`, `--product <retail|classic|titan|forever>`, `--build <full-build>`, `--region <us|eu|cn|kr|tw>`, `--locale <locale>`, `--table <name>`, `--table-hash <8-hex>`, `--record <n>`, `--push <n>`, `--status <0-255>`, `--region-id <n>`, `--search <text>`, `--latest`, `--offline`, `--limit <1-200>`, `--cursor <opaque>`, `--page <n>`, `--after-index <n>`, `--max-pages <n>`, `--max-requests <n>`, `--max-bytes <n>`, `--encoding csv`, `--output <file>`.
- `data sql` — mutates. Query pinned static tables: --snapshot <pin> (--installation <client-or-game-root> | --cdn) --file <query.json> [--max-bytes <n>] [--offline]; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--file <query.json>`, `--max-bytes <n>`, `--offline`.
- `data db2` — mutates. Read typed rows: --snapshot <pin> (--installation <client-or-game-root> | --cdn) --table <name> [--id <n> | --limit <n> --after-id <n>] [--max-bytes <n>] [--offline]; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--table <name>`, `--id <n>`, `--limit <n>`, `--after-id <n>`, `--max-bytes <n>`, `--offline`.
- `data db2 schema <Table>` — mutates. Read parsed table schema fields, keys and relationship columns: data db2 schema <Table>; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`.
- `data db2 search <Table>` — mutates. Case-insensitive substring search over one field: data db2 search <Table> --field <name> --query <text> --limit <n>; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--field <name>`, `--query <text>`, `--limit <n>`.
- `data db2 foreign-key <Table>` — mutates. Select rows by one foreign-key field and value with a mandatory bound: data db2 foreign-key <Table> --field <name> --value <n> --limit <1-5000>; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--field <name>`, `--value <n>`, `--limit <n>`.
- `data db2 stream <Table>` — mutates. Stream bounded table rows as typed JSONL begin/record/end frames (requires --format jsonl): data db2 stream <Table> [--fields <a,b>] [--filter <field=value>] --limit <n>; only a legal end frame plus exit 0 is a complete success; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--fields <a,b>`, `--filter <field=value>`, `--limit <n>`.
- `data spell info` — mutates. Resolve the bounded trigger and description-reference closure around one spell: --spell-id <n> [--max-depth <n>]; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--spell-id <n>`, `--max-depth <n>`.
- `data spell auras` — mutates. Report aura-effect presence for one spell: --spell-id <n>; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--spell-id <n>`.
- `data spell summons` — mutates. List summon effects of one spell, optionally filtered to one NPC: --spell-id <n> [--npc-id <n>]; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--spell-id <n>`, `--npc-id <n>`.
- `data item get` — mutates. Read one item summary and slot name: --item-id <n>; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--item-id <n>`.
- `data item models` — mutates. Resolve model and texture file IDs for one item: --item-id <n> [--race-id <n> --gender <n>]; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--item-id <n>`, `--race-id <n>`, `--gender <n>`.
- `data item geosets` — mutates. Read geoset and helmet-hide data for one item: --item-id <n>; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--item-id <n>`.
- `data item textures` — mutates. Read character texture sections for one item: --item-id <n>; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--item-id <n>`.
- `data creature display` — mutates. Resolve one creature display by exactly one key: (--display-id <n> | --file-data-id <n>); omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--display-id <n>`, `--file-data-id <n>`.
- `data creature model` — mutates. List every display of one model file with variants: --file-data-id <n>; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--file-data-id <n>`.
- `data encounter get` — mutates. Build the bounded journal encounter section tree with related spells and the relation manifest: --journal-encounter-id <n> [--max-depth <n>]; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--journal-encounter-id <n>`, `--max-depth <n>`.
- `data decor list` — mutates. List house decor rows in ascending ID order with a mandatory bound: --limit <1-5000>; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--limit <n>`.
- `data decor get` — mutates. Resolve one decor by exactly one key: (--id <decor-id> | --item-id <n> | --model-file-data-id <n>); omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--installation <client-or-game-root>`, `--cdn`, `--offline`, `--max-bytes <n>`, `--id <n>`, `--item-id <n>`, `--model-file-data-id <n>`.

## describe

- `describe` — read-only. Read implemented command contracts.

## doctor

- `doctor` — read-only. Aggregate read-only workspace, target catalog and source-mirror health checks; the mirror check joins only when the nearest project declares a product, and exit 3 reports that at least one check found an error.
  Flags: `--offline`.

## evidence

- `evidence show <capture-id>` — read-only. Read a capture manifest: evidence show <capture-id>.
- `evidence verify <capture-id>` — read-only. Verify manifest and original bytes: evidence verify <capture-id>.
- `evidence list` — read-only. List archived capture manifests in ID order with a bounded page.
  Flags: `--limit <n>`.

## init

- `init` — mutates. Create a new-format workspace without importing old data; --plan resolves the fresh-isolation plan without changing anything, --fresh isolates an occupied legacy root into a deterministic sibling archive first, --resume completes an interrupted archive switch.
  Flags: `--plan`, `--fresh`, `--resume`.

## live

- `live run` — mutates. Run a bounded probe: --session <session-id> --file <probe.lua> [--account <account>]; discovers a unique stored account for the verified character unless explicit; retains operation ID on failure.
  Flags: `--session <session-id>`, `--account <account>`, `--file <probe.lua>`.
- `live instances` — mutates. Discover supported installations under known roots and every running game window with its identity state (installed/running; identified/busy/no_actor/identity_unreadable); sends exactly one fixed identity trigger per running window and no other input; output carries no protocol fields.
  Flags: `--installation <client-or-game-root>`.
- `live connect` — mutates. Discover, identify and connect a game window automatically: [--snapshot <pin>] [--character <name>] [--realm <realm>] [--pid <pid>] [--installation <client>] [--session <session-id> to revive] [--capture-area <window|x,y,width,height>]; a unique match connects without manual commands, ambiguity returns exit 2 with context.candidates; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin>`, `--character <name>`, `--realm <realm>`, `--pid <pid>`, `--installation <client>`, `--session <session-id>`, `--capture-area <window|x,y,width,height>`.
- `live bind` — mutates. Observe /dev connect in the selected game and save a connection: --snapshot <pin>; unique window discovered automatically, optional --pid/--installation/--character/--realm filters; whole-window capture by default; never types; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--installation <client>`, `--pid <pid>`, `--snapshot <pin>`, `--character <name>`, `--realm <realm>`, `--capture-area <window|x,y,width,height>`.
- `live status <operation-id>` — read-only. Read persisted work without sending input: live status <operation-id>.
- `live resume <operation-id>` — mutates. Recover live resume <operation-id> using its saved connection; revalidates before new input and never repeats submitted work; completed work performs no game input.
- `live session <session-id>` — read-only. Verify a retained session and its evidence: live session <session-id>; does not reconnect or authorize input.

## project

- `project init` — mutates. Declare a project product: --product <track> [--path <directory>]; creates lycheedev.json without choosing latest.
  Flags: `--product <track>`, `--path <directory>`.
- `project lock` — mutates. Save an existing fixed --snapshot <pin> in lycheedev.lock.json [--path <directory>]; replaces the previous known-format lock.
  Flags: `--snapshot <pin>`, `--path <directory>`.
- `project status` — read-only. Read project declaration and lock [--path <directory>]; no workspace or network required.
  Flags: `--path <directory>`.

## skill

- `skill install` — mutates. Install: --release <distribution-root> --path <parent/lycheedev>; add --output <archive> to upgrade; --resume --path <target> --output <archive> resumes without --release.
  Flags: `--release <distribution-root>`, `--path <target>`, `--output <archive>`, `--resume`.
- `skill status` — read-only. Inspect skill ownership and files: --path <installed-directory>.
  Flags: `--path <installed-directory>`.
- `skill remove` — mutates. Archive an unchanged owned skill: --path <parent/lycheedev> --output <recovery-directory outside parent>; deletes nothing.
  Flags: `--path <parent/lycheedev>`, `--output <recovery-directory>`.

## source

- `source list` — read-only. List source repositories and product branches.
- `source sync` — mutates. Fetch --source <key> --product <track> [--ref <full-ref-or-commit>] and return a fixed snapshot.
  Flags: `--source <key>`, `--product <track>`, `--ref <full-ref-or-commit>`.
- `source inspect` — mutates. Read --snapshot <pin-id> --path <file> [--line <n> --count <n>] and archive full source evidence; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin-id>`, `--path <file>`, `--line <n>`, `--count <n>`.
- `source index` — mutates. Build syntax index for --snapshot <pin-id>, retaining parse diagnostics; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin-id>`.
- `source query <term>` — mutates. Find exact symbol/relation names: source query <term> --snapshot <pin-id> [--limit <n>]; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--snapshot <pin-id>`, `--limit <n>`.
- `source diff` — mutates. Compare --from <pin-id> --to <pin-id> [--limit <n> per change category].
  Flags: `--from <pin-id>`, `--to <pin-id>`, `--limit <n>`.
- `source validate` — mutates. Check --path <addon-root> --toc <relative-manifest> --snapshot <pin-id>; report unresolved static coverage; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins.
  Flags: `--project <directory>`, `--path <addon-root>`, `--toc <relative-manifest>`, `--snapshot <pin-id>`.

## target

- `target resolve` — mutates. Pin data identity from --installation <client> or remote --product <track>, with --region and --locale; remote --build requires that exact currently advertised build. Optional --definitions/--from/--offline; --file pins explicit identities instead. Does not download data archives.
  Flags: `--file <selection.json>`, `--installation <client-directory>`, `--product <retail|classic|titan|forever>`, `--build <full-build>`, `--region <us|eu|cn|kr|tw>`, `--locale <locale>`, `--definitions <commit-or-full-ref>`, `--from <parent-pin>`, `--offline`.
- `target show <pin-id>` — read-only. Read a fixed selection: target show <pin-id>.
- `target list` — read-only. List every named target configuration in name order; a brand new workspace has no targets.
- `target add <name>` — mutates. Create or explicitly replace one named target: target add <name> --product <track> --region <region> --locale <locale> with exactly one of --installation <client> or --remote; optional --build <full-build> pins that exact release; --replace confirms replacement.
  Flags: `--product <retail|classic|titan|forever>`, `--region <us|eu|cn|kr|tw>`, `--locale <locale>`, `--build <full-build>`, `--installation <client-directory>`, `--remote`, `--replace`.
- `target remove <name>` — mutates. Remove one named target and its reference ledger: target remove <name>; resolved pins stay intact, running operations block removal.
- `target available` — read-only. List the releases the version manifests serve for one region: --region <region>; one bounded manifest fetch per supported product with per-product failures reported inline; --offline is refused because the listing is inherently current.
  Flags: `--region <us|eu|cn|kr|tw>`, `--offline`.

## version

- `version` — read-only. Inspect the native toolkit build.
