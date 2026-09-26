package command

import (
	"fmt"
	"strings"
)

// Definition is the public, machine-readable description of an implemented
// command. The parser and this description are both derived from the same
// commandContract values below.
type Definition struct {
	Path      string   `json:"path"`
	Summary   string   `json:"summary"`
	Mutates   bool     `json:"mutates"`
	Arguments []string `json:"arguments"`
	Flags     []string `json:"flags"`
}

type flagSpec struct {
	name    string
	display string
	value   bool
}

type commandContract struct {
	Definition
	flags      []flagSpec
	positional string
	allowsHome bool
}

// dataSourceFlags is the shared pinned-data source selection every data query
// verb accepts: a fixed snapshot plus installation or CDN reads, with the
// documented byte budget.
func dataSourceFlags(extra ...flagSpec) []flagSpec {
	return append([]flagSpec{
		{name: "--snapshot", display: "--snapshot <pin>", value: true},
		{name: "--installation", display: "--installation <client-or-game-root>", value: true},
		{name: "--cdn", display: "--cdn"},
		{name: "--offline", display: "--offline"},
		{name: "--max-bytes", display: "--max-bytes <n>", value: true},
	}, extra...)
}

var commandContracts = []commandContract{
	{Definition: Definition{Path: "project init", Summary: "Declare a project product: --product <track> [--path <directory>]; creates lycheedev.json without choosing latest", Mutates: true}, flags: []flagSpec{{name: "--product", display: "--product <track>", value: true}, {name: "--path", display: "--path <directory>", value: true}}},
	{Definition: Definition{Path: "data hotfix", Summary: "Inspect independent Hotfix records with an explicit source: --source <wago|dbcache|raidbots>. dbcache reads a pinned --snapshot <pin> (--dbcache <DBCache.bin> | --from <cache-capture>); wago queries the remote record set from --product/--build/--region/--locale with an optional --snapshot parent; raidbots reads --raidbots <DBCache.bin> under 30 days. Filters: --table --table-hash --record --push --status --region-id --search and wago --from/--to; --latest selects the largest matching push batch before pagination", Mutates: true}, flags: []flagSpec{
		{name: "--source", display: "--source <wago|dbcache|raidbots>", value: true},
		{name: "--snapshot", display: "--snapshot <pin>", value: true},
		{name: "--dbcache", display: "--dbcache <DBCache.bin>", value: true},
		{name: "--raidbots", display: "--raidbots <DBCache.bin>", value: true},
		{name: "--from", display: "--from <cache-capture-or-time>", value: true},
		{name: "--to", display: "--to <time>", value: true},
		{name: "--product", display: "--product <retail|classic|titan|forever>", value: true},
		{name: "--build", display: "--build <full-build>", value: true},
		{name: "--region", display: "--region <us|eu|cn|kr|tw>", value: true},
		{name: "--locale", display: "--locale <locale>", value: true},
		{name: "--table", display: "--table <name>", value: true},
		{name: "--table-hash", display: "--table-hash <8-hex>", value: true},
		{name: "--record", display: "--record <n>", value: true},
		{name: "--push", display: "--push <n>", value: true},
		{name: "--status", display: "--status <0-255>", value: true},
		{name: "--region-id", display: "--region-id <n>", value: true},
		{name: "--search", display: "--search <text>", value: true},
		{name: "--latest", display: "--latest"},
		{name: "--offline", display: "--offline"},
		{name: "--limit", display: "--limit <1-200>", value: true},
		{name: "--cursor", display: "--cursor <opaque>", value: true},
		{name: "--page", display: "--page <n>", value: true},
		{name: "--after-index", display: "--after-index <n>", value: true},
		{name: "--max-pages", display: "--max-pages <n>", value: true},
		{name: "--max-requests", display: "--max-requests <n>", value: true},
		{name: "--max-bytes", display: "--max-bytes <n>", value: true},
		{name: "--encoding", display: "--encoding csv", value: true},
		{name: "--output", display: "--output <file>", value: true},
	}, allowsHome: true},
	{Definition: Definition{Path: "project lock", Summary: "Save an existing fixed --snapshot <pin> in lycheedev.lock.json [--path <directory>]; replaces the previous known-format lock", Mutates: true}, flags: []flagSpec{{name: "--snapshot", display: "--snapshot <pin>", value: true}, {name: "--path", display: "--path <directory>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "project status", Summary: "Read project declaration and lock [--path <directory>]; no workspace or network required", Mutates: false}, flags: []flagSpec{{name: "--path", display: "--path <directory>", value: true}}},
	{Definition: Definition{Path: "live probe put", Summary: "Register immutable bounded Lua source under a mutable name without touching the game: --name <name> --file <probe.lua>; returns a content-addressed revision", Mutates: true}, flags: []flagSpec{{name: "--name", display: "--name <name>", value: true}, {name: "--file", display: "--file <probe.lua>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "live probe load", Summary: "Load one immutable probe revision into the selected client without executing it: --session <session-id> --probe <name-or-revision> --request <idempotency-key> [--account <account>]", Mutates: true}, flags: []flagSpec{{name: "--session", display: "--session <session-id>", value: true}, {name: "--probe", display: "--probe <name-or-revision>", value: true}, {name: "--request", display: "--request <idempotency-key>", value: true}, {name: "--account", display: "--account <account>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "live probe list", Summary: "List active probe names without game input; --include-removed shows tombstoned names", Mutates: false}, flags: []flagSpec{{name: "--include-removed", display: "--include-removed"}, {name: "--limit", display: "--limit <1-1000>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "live probe show", Summary: "Read an exact probe by active name or immutable PRB revision: live probe show <name-or-revision>", Mutates: false}, positional: "<name-or-revision>", allowsHome: true},
	{Definition: Definition{Path: "live probe remove", Summary: "Tombstone one probe name for future selection while retaining immutable revisions and operation evidence: live probe remove <name>", Mutates: true}, positional: "<name>", allowsHome: true},
	{Definition: Definition{Path: "live run", Summary: "Execute one loaded operation and return its verified report with cleanup pending; acknowledge separately with live ack: live run <operation-id>", Mutates: true}, positional: "<operation-id>", allowsHome: true},
	{Definition: Definition{Path: "skill install", Summary: "Install: --release <distribution-root> --path <parent/lycheedev>; add --output <archive> to upgrade; --resume --path <target> --output <archive> resumes without --release", Mutates: true}, flags: []flagSpec{{name: "--release", display: "--release <distribution-root>", value: true}, {name: "--path", display: "--path <target>", value: true}, {name: "--output", display: "--output <archive>", value: true}, {name: "--resume", display: "--resume"}}, allowsHome: false},
	{Definition: Definition{Path: "skill status", Summary: "Inspect skill ownership and files: --path <installed-directory>", Mutates: false}, flags: []flagSpec{{name: "--path", display: "--path <installed-directory>", value: true}}, allowsHome: false},
	{Definition: Definition{Path: "skill remove", Summary: "Archive an unchanged owned skill: --path <parent/lycheedev> --output <recovery-directory outside parent>; deletes nothing", Mutates: true}, flags: []flagSpec{{name: "--path", display: "--path <parent/lycheedev>", value: true}, {name: "--output", display: "--output <recovery-directory>", value: true}}, allowsHome: false},
	{Definition: Definition{Path: "addon status", Summary: "Inspect addon files with --path <addon-directory>, or resolve a supported client with --installation <client-directory>", Mutates: false}, flags: []flagSpec{{name: "--path", display: "--path <addon-directory>", value: true}, {name: "--installation", display: "--installation <client-directory>", value: true}}, allowsHome: false},
	{Definition: Definition{Path: "addon install", Summary: "Install: --release <distribution-root> --installation <client-directory>; --output <archive> upgrades; --resume --installation <client> --output <archive> recovers without --release", Mutates: true}, flags: []flagSpec{{name: "--release", display: "--release <distribution-root>", value: true}, {name: "--installation", display: "--installation <client-directory>", value: true}, {name: "--output", display: "--output <archive>", value: true}, {name: "--resume", display: "--resume"}}, allowsHome: false},
	{Definition: Definition{Path: "addon remove", Summary: "Archive an unchanged owned addon: --installation <client-directory> --output <recovery-directory>; never deletes SavedVariables", Mutates: true}, flags: []flagSpec{{name: "--installation", display: "--installation <client-directory>", value: true}, {name: "--output", display: "--output <recovery-directory>", value: true}}, allowsHome: false},
	{Definition: Definition{Path: "data sql", Summary: "Read-only SQL over a pinned static target: exactly one of --sql <text>, --file <query.sql|query.json>, or --stdin; repeated --param <name=scalar> binds named values; --encoding csv --output <file> writes a captured CSV and manifest", Mutates: true}, flags: []flagSpec{{name: "--snapshot", display: "--snapshot <pin>", value: true}, {name: "--installation", display: "--installation <client-or-game-root>", value: true}, {name: "--cdn", display: "--cdn"}, {name: "--sql", display: "--sql <text>", value: true}, {name: "--file", display: "--file <query.sql|query.json>", value: true}, {name: "--stdin", display: "--stdin"}, {name: "--param", display: "--param <name=scalar>", value: true}, {name: "--encoding", display: "--encoding csv", value: true}, {name: "--output", display: "--output <file>", value: true}, {name: "--overwrite", display: "--overwrite"}, {name: "--max-bytes", display: "--max-bytes <n>", value: true}, {name: "--offline", display: "--offline"}}, allowsHome: true},
	{Definition: Definition{Path: "data db2", Summary: "Read typed rows: --snapshot <pin> (--installation <client-or-game-root> | --cdn) --table <name> [--id <n> | --limit <n> --after-id <n>] [--max-bytes <n>] [--offline]", Mutates: true}, flags: []flagSpec{{name: "--snapshot", display: "--snapshot <pin>", value: true}, {name: "--installation", display: "--installation <client-or-game-root>", value: true}, {name: "--cdn", display: "--cdn"}, {name: "--table", display: "--table <name>", value: true}, {name: "--id", display: "--id <n>", value: true}, {name: "--limit", display: "--limit <n>", value: true}, {name: "--after-id", display: "--after-id <n>", value: true}, {name: "--max-bytes", display: "--max-bytes <n>", value: true}, {name: "--offline", display: "--offline"}}, allowsHome: true},
	{Definition: Definition{Path: "data db2 schema", Summary: "Read parsed table schema fields, keys and relationship columns: data db2 schema <Table>", Mutates: true}, flags: dataSourceFlags(), positional: "<Table>", allowsHome: true},
	{Definition: Definition{Path: "data db2 search", Summary: "Case-insensitive substring search over one field: data db2 search <Table> --field <name> --query <text> --limit <n>", Mutates: true}, flags: dataSourceFlags(
		flagSpec{name: "--field", display: "--field <name>", value: true},
		flagSpec{name: "--query", display: "--query <text>", value: true},
		flagSpec{name: "--limit", display: "--limit <n>", value: true},
	), positional: "<Table>", allowsHome: true},
	{Definition: Definition{Path: "data db2 foreign-key", Summary: "Select rows by one foreign-key field and value with a mandatory bound: data db2 foreign-key <Table> --field <name> --value <n> --limit <1-5000>", Mutates: true}, flags: dataSourceFlags(
		flagSpec{name: "--field", display: "--field <name>", value: true},
		flagSpec{name: "--value", display: "--value <n>", value: true},
		flagSpec{name: "--limit", display: "--limit <n>", value: true},
	), positional: "<Table>", allowsHome: true},
	{Definition: Definition{Path: "data db2 stream", Summary: "Stream bounded table rows as typed JSONL begin/record/end frames (requires --format jsonl): data db2 stream <Table> [--fields <a,b>] [--filter <field=value>] --limit <n>; only a legal end frame plus exit 0 is a complete success", Mutates: true}, flags: dataSourceFlags(
		flagSpec{name: "--fields", display: "--fields <a,b>", value: true},
		flagSpec{name: "--filter", display: "--filter <field=value>", value: true},
		flagSpec{name: "--limit", display: "--limit <n>", value: true},
	), positional: "<Table>", allowsHome: true},
	{Definition: Definition{Path: "data spell info", Summary: "Resolve the bounded trigger and description-reference closure around one spell: --spell-id <n> [--max-depth <n>]", Mutates: true}, flags: dataSourceFlags(
		flagSpec{name: "--spell-id", display: "--spell-id <n>", value: true},
		flagSpec{name: "--max-depth", display: "--max-depth <n>", value: true},
	), allowsHome: true},
	{Definition: Definition{Path: "data spell auras", Summary: "Report aura-effect presence for one spell: --spell-id <n>", Mutates: true}, flags: dataSourceFlags(
		flagSpec{name: "--spell-id", display: "--spell-id <n>", value: true},
	), allowsHome: true},
	{Definition: Definition{Path: "data spell summons", Summary: "List summon effects of one spell, optionally filtered to one NPC: --spell-id <n> [--npc-id <n>]", Mutates: true}, flags: dataSourceFlags(
		flagSpec{name: "--spell-id", display: "--spell-id <n>", value: true},
		flagSpec{name: "--npc-id", display: "--npc-id <n>", value: true},
	), allowsHome: true},
	{Definition: Definition{Path: "data item get", Summary: "Read one item summary and slot name: --item-id <n>", Mutates: true}, flags: dataSourceFlags(
		flagSpec{name: "--item-id", display: "--item-id <n>", value: true},
	), allowsHome: true},
	{Definition: Definition{Path: "data item models", Summary: "Resolve model and texture file IDs for one item: --item-id <n> [--race-id <n> --gender <n>]", Mutates: true}, flags: dataSourceFlags(
		flagSpec{name: "--item-id", display: "--item-id <n>", value: true},
		flagSpec{name: "--race-id", display: "--race-id <n>", value: true},
		flagSpec{name: "--gender", display: "--gender <n>", value: true},
	), allowsHome: true},
	{Definition: Definition{Path: "data item geosets", Summary: "Read geoset and helmet-hide data for one item: --item-id <n>", Mutates: true}, flags: dataSourceFlags(
		flagSpec{name: "--item-id", display: "--item-id <n>", value: true},
	), allowsHome: true},
	{Definition: Definition{Path: "data item textures", Summary: "Read character texture sections for one item: --item-id <n>", Mutates: true}, flags: dataSourceFlags(
		flagSpec{name: "--item-id", display: "--item-id <n>", value: true},
	), allowsHome: true},
	{Definition: Definition{Path: "data creature display", Summary: "Resolve one creature display by exactly one key: (--display-id <n> | --file-data-id <n>)", Mutates: true}, flags: dataSourceFlags(
		flagSpec{name: "--display-id", display: "--display-id <n>", value: true},
		flagSpec{name: "--file-data-id", display: "--file-data-id <n>", value: true},
	), allowsHome: true},
	{Definition: Definition{Path: "data creature model", Summary: "List every display of one model file with variants: --file-data-id <n>", Mutates: true}, flags: dataSourceFlags(
		flagSpec{name: "--file-data-id", display: "--file-data-id <n>", value: true},
	), allowsHome: true},
	{Definition: Definition{Path: "data encounter get", Summary: "Build the bounded journal encounter section tree with related spells and the relation manifest: --journal-encounter-id <n> [--max-depth <n>]", Mutates: true}, flags: dataSourceFlags(
		flagSpec{name: "--journal-encounter-id", display: "--journal-encounter-id <n>", value: true},
		flagSpec{name: "--max-depth", display: "--max-depth <n>", value: true},
	), allowsHome: true},
	{Definition: Definition{Path: "data decor list", Summary: "List house decor rows in ascending ID order with a mandatory bound: --limit <1-5000>", Mutates: true}, flags: dataSourceFlags(
		flagSpec{name: "--limit", display: "--limit <n>", value: true},
	), allowsHome: true},
	{Definition: Definition{Path: "data decor get", Summary: "Resolve one decor by exactly one key: (--id <decor-id> | --item-id <n> | --model-file-data-id <n>)", Mutates: true}, flags: dataSourceFlags(
		flagSpec{name: "--id", display: "--id <n>", value: true},
		flagSpec{name: "--item-id", display: "--item-id <n>", value: true},
		flagSpec{name: "--model-file-data-id", display: "--model-file-data-id <n>", value: true},
	), allowsHome: true},
	{Definition: Definition{Path: "asset inspect", Summary: "Verify CASC file: --snapshot <pin> (--installation <client-or-game-root> | --cdn) --file-id <id> [--max-bytes <n>] [--offline]; archive original bytes", Mutates: true}, flags: []flagSpec{{name: "--snapshot", display: "--snapshot <pin>", value: true}, {name: "--installation", display: "--installation <client-or-game-root>", value: true}, {name: "--cdn", display: "--cdn"}, {name: "--file-id", display: "--file-id <id>", value: true}, {name: "--max-bytes", display: "--max-bytes <n>", value: true}, {name: "--offline", display: "--offline"}}, allowsHome: true},
	{Definition: Definition{Path: "asset search", Summary: "Use one listfile mode: --query <text>, --extension <ext>, --name <path>, or --file-id <id>; requires --snapshot and --listfile <community-csv|wowexport-text|wowexport-binary>; --limit bounds search and extension pages; --offline reuses verified cache; --max-bytes bounds total listfile input (default 256 MiB, maximum 512 MiB)", Mutates: true}, flags: []flagSpec{{name: "--snapshot", display: "--snapshot <pin>", value: true}, {name: "--listfile", display: "--listfile <community-csv|wowexport-text|wowexport-binary>", value: true}, {name: "--query", display: "--query <text>", value: true}, {name: "--extension", display: "--extension <ext>", value: true}, {name: "--name", display: "--name <path>", value: true}, {name: "--file-id", display: "--file-id <id>", value: true}, {name: "--limit", display: "--limit <1-200>", value: true}, {name: "--max-bytes", display: "--max-bytes <n>", value: true}, {name: "--offline", display: "--offline"}}, allowsHome: true},
	{Definition: Definition{Path: "asset export", Summary: "Export CASC file: --snapshot <pin> (--installation <client-or-game-root> | --cdn) --file-id <id> --output <file> [--encoding raw|png|webp] [--mipmap <0..15>] [--channels <rgba-subset>] [--max-pixels <n>] [--overwrite] [--max-bytes <n>] [--offline]; raw default, BLP2 image conversion, existing parent required", Mutates: true}, flags: []flagSpec{{name: "--snapshot", display: "--snapshot <pin>", value: true}, {name: "--installation", display: "--installation <client-or-game-root>", value: true}, {name: "--cdn", display: "--cdn"}, {name: "--file-id", display: "--file-id <id>", value: true}, {name: "--output", display: "--output <file>", value: true}, {name: "--encoding", display: "--encoding raw|png|webp", value: true}, {name: "--mipmap", display: "--mipmap <0..15>", value: true}, {name: "--channels", display: "--channels <rgba-subset>", value: true}, {name: "--max-pixels", display: "--max-pixels <n>", value: true}, {name: "--overwrite", display: "--overwrite"}, {name: "--max-bytes", display: "--max-bytes <n>", value: true}, {name: "--offline", display: "--offline"}}, allowsHome: true},
	{Definition: Definition{Path: "asset demux", Summary: "Demux a bounded VP9 AVI from --path <local-file> or pinned CASC (--snapshot <pin> and --file-id <id> with --installation or --cdn); --output <existing-directory> [--max-bytes <n>] [--max-frames <n>] [--allow-partial] [--overwrite]", Mutates: true}, flags: []flagSpec{{name: "--path", display: "--path <local-file>", value: true}, {name: "--snapshot", display: "--snapshot <pin>", value: true}, {name: "--installation", display: "--installation <client-or-game-root>", value: true}, {name: "--cdn", display: "--cdn"}, {name: "--file-id", display: "--file-id <id>", value: true}, {name: "--output", display: "--output <existing-directory>", value: true}, {name: "--max-bytes", display: "--max-bytes <n>", value: true}, {name: "--max-frames", display: "--max-frames <n>", value: true}, {name: "--allow-partial", display: "--allow-partial"}, {name: "--overwrite", display: "--overwrite"}, {name: "--offline", display: "--offline"}}, allowsHome: true},
	{Definition: Definition{Path: "version", Summary: "Inspect the native toolkit build", Mutates: false}, allowsHome: true},
	{Definition: Definition{Path: "describe", Summary: "Read implemented command contracts", Mutates: false}, allowsHome: true},
	{Definition: Definition{Path: "init", Summary: "Create a new-format workspace without importing old data; --plan resolves the fresh-isolation plan without changing anything, --fresh isolates an occupied legacy root into a deterministic sibling archive first, --resume completes an interrupted archive switch", Mutates: true}, flags: []flagSpec{{name: "--plan", display: "--plan"}, {name: "--fresh", display: "--fresh"}, {name: "--resume", display: "--resume"}}, allowsHome: true},
	{Definition: Definition{Path: "doctor", Summary: "Aggregate read-only workspace, target catalog and source-mirror health checks; the mirror check joins only when the nearest project declares a product, and exit 3 reports that at least one check found an error", Mutates: false}, flags: []flagSpec{{name: "--offline", display: "--offline"}}, allowsHome: true},
	{Definition: Definition{Path: "config show", Summary: "Read the workspace user preference and resource budget document; a missing document reports the documented defaults", Mutates: false}, allowsHome: true},
	{Definition: Definition{Path: "config set", Summary: "Update the workspace budget: --cache-max-bytes <n> and/or --download-workers <n>; at least one flag is required", Mutates: true}, flags: []flagSpec{{name: "--cache-max-bytes", display: "--cache-max-bytes <n>", value: true}, {name: "--download-workers", display: "--download-workers <n>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "cache status", Summary: "Report managed cache accounting, protection visibility and the configured limit", Mutates: false}, allowsHome: true},
	{Definition: Definition{Path: "cache verify", Summary: "Verify every managed cache object against its content hash; strictly read-only, repairs and deletes nothing", Mutates: false}, allowsHome: true},
	{Definition: Definition{Path: "cache prune", Summary: "Reclaim ordinary cache toward --target-bytes <n> (zero reclaims all ordinary cache), capped by --max-objects <n>; --uncommitted also reclaims staging files; --dry-run reports without deleting; protected and in-use objects are always skipped", Mutates: true}, flags: []flagSpec{
		{name: "--target-bytes", display: "--target-bytes <n>", value: true},
		{name: "--max-objects", display: "--max-objects <n>", value: true},
		{name: "--uncommitted", display: "--uncommitted"},
		{name: "--dry-run", display: "--dry-run"},
	}, allowsHome: true},
	{Definition: Definition{Path: "live instances", Summary: "Discover supported installations under known roots and every running game window with its identity state (installed/running; identified/busy/no_actor/identity_unreadable); sends exactly one fixed identity trigger per running window and no other input; output carries no protocol fields", Mutates: true}, flags: []flagSpec{{name: "--installation", display: "--installation <client-or-game-root>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "live connect", Summary: "Discover, identify and connect a game window automatically: [--snapshot <pin>] [--character <name>] [--realm <realm>] [--pid <pid>] [--installation <client>] [--session <session-id> to revive] [--capture-area <window|x,y,width,height>]; a unique match connects without manual commands, ambiguity returns exit 2 with context.candidates", Mutates: true}, flags: []flagSpec{
		{name: "--snapshot", display: "--snapshot <pin>", value: true},
		{name: "--character", display: "--character <name>", value: true},
		{name: "--realm", display: "--realm <realm>", value: true},
		{name: "--pid", display: "--pid <pid>", value: true},
		{name: "--installation", display: "--installation <client>", value: true},
		{name: "--session", display: "--session <session-id>", value: true},
		{name: "--capture-area", display: "--capture-area <window|x,y,width,height>", value: true},
	}, allowsHome: true},
	{Definition: Definition{Path: "live reset", Summary: "Unblock a stuck window whose in-game queue blocks identity after abandon or a lost receipt: [--snapshot <pin>] [--character <name>] [--realm <realm>] [--pid <pid>] [--installation <client>]; sends one fixed nonce-correlated reset trigger per unowned matching window, expects its receipt, then reconnects; disk-owned windows are never touched", Mutates: true}, flags: []flagSpec{
		{name: "--snapshot", display: "--snapshot <pin>", value: true},
		{name: "--character", display: "--character <name>", value: true},
		{name: "--realm", display: "--realm <realm>", value: true},
		{name: "--pid", display: "--pid <pid>", value: true},
		{name: "--installation", display: "--installation <client>", value: true},
	}, allowsHome: true},
	{Definition: Definition{Path: "live reload", Summary: "Perform one correlated UI reload on the selected client and verify the new runtime: --session <session-id> --request <idempotency-key>", Mutates: true}, flags: []flagSpec{{name: "--session", display: "--session <session-id>", value: true}, {name: "--request", display: "--request <idempotency-key>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "live ack", Summary: "Acknowledge one verified operation, retire only its exact queue entry and release its window ownership without forcing another reload: live ack <operation-id>", Mutates: true}, positional: "<operation-id>", allowsHome: true},
	{Definition: Definition{Path: "live bugs", Summary: "Capture 1-100 existing addon errors from the selected client as a verified report and stop before acknowledgement", Mutates: true}, flags: []flagSpec{{name: "--session", display: "--session <session-id>", value: true}, {name: "--request", display: "--request <idempotency-key>", value: true}, {name: "--count", display: "--count <1-100>", value: true}, {name: "--account", display: "--account <account>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "live hide", Summary: "Dismiss the displayed bridge receipt on the selected client after its evidence is archived: --session <session-id>; refuses windows owned by in-flight operations and verifies the clear from valid frames", Mutates: true}, flags: []flagSpec{{name: "--session", display: "--session <session-id>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "live bind", Summary: "Observe /dev connect in the selected game and save a connection: --snapshot <pin>; unique window discovered automatically, optional --pid/--installation/--character/--realm filters; whole-window capture by default; never types", Mutates: true}, flags: []flagSpec{{name: "--installation", display: "--installation <client>", value: true}, {name: "--pid", display: "--pid <pid>", value: true}, {name: "--snapshot", display: "--snapshot <pin>", value: true}, {name: "--character", display: "--character <name>", value: true}, {name: "--realm", display: "--realm <realm>", value: true}, {name: "--capture-area", display: "--capture-area <window|x,y,width,height>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "target resolve", Summary: "Pin data identity from one named --target <name>, --installation <client>, remote --product <track>, or --file <selection.json>; named targets resolve their exact configured identity at command start", Mutates: true}, flags: []flagSpec{
		{name: "--file", display: "--file <selection.json>", value: true},
		{name: "--target", display: "--target <name>", value: true},
		{name: "--installation", display: "--installation <client-directory>", value: true},
		{name: "--product", display: "--product <retail|classic|titan|forever>", value: true},
		{name: "--build", display: "--build <full-build>", value: true},
		{name: "--definitions", display: "--definitions <commit-or-full-ref>", value: true},
		{name: "--region", display: "--region <us|eu|cn|kr|tw>", value: true},
		{name: "--locale", display: "--locale <locale>", value: true},
		{name: "--from", display: "--from <parent-pin>", value: true},
		{name: "--offline", display: "--offline"},
	}, allowsHome: true},
	{Definition: Definition{Path: "target show", Summary: "Read a named target configuration and its references, or an explicit PIN- selection ID", Mutates: false}, positional: "<name-or-pin-id>", allowsHome: true},
	{Definition: Definition{Path: "target list", Summary: "List every named target configuration in name order; a brand new workspace has no targets", Mutates: false}, allowsHome: true},
	{Definition: Definition{Path: "target add", Summary: "Create or explicitly replace one named target: target add <name> --product <track> --region <region> --locale <locale> with exactly one of --installation <client> or --remote; optional --build <full-build> pins that exact release; --replace confirms replacement", Mutates: true}, flags: []flagSpec{
		{name: "--product", display: "--product <retail|classic|titan|forever>", value: true},
		{name: "--region", display: "--region <us|eu|cn|kr|tw>", value: true},
		{name: "--locale", display: "--locale <locale>", value: true},
		{name: "--build", display: "--build <full-build>", value: true},
		{name: "--installation", display: "--installation <client-directory>", value: true},
		{name: "--definitions", display: "--definitions <commit-or-full-ref>", value: true},
		{name: "--remote", display: "--remote"},
		{name: "--replace", display: "--replace"},
	}, positional: "<name>", allowsHome: true},
	{Definition: Definition{Path: "target remove", Summary: "Remove one named target and its reference ledger: target remove <name>; resolved pins stay intact, running operations block removal", Mutates: true}, positional: "<name>", allowsHome: true},
	{Definition: Definition{Path: "target available", Summary: "List the releases the version manifests serve for one region: --region <region>; one bounded manifest fetch per supported product with per-product failures reported inline; --offline is refused because the listing is inherently current", Mutates: false}, flags: []flagSpec{{name: "--region", display: "--region <us|eu|cn|kr|tw>", value: true}, {name: "--offline", display: "--offline"}}, allowsHome: false},
	{Definition: Definition{Path: "live status", Summary: "Read persisted work without sending input: live status <operation-id>", Mutates: false}, positional: "<operation-id>", allowsHome: true},
	{Definition: Definition{Path: "live resume", Summary: "Recover live resume <operation-id> using its saved connection; revalidates before new input and never repeats submitted work; completed work performs no game input", Mutates: true}, positional: "<operation-id>", allowsHome: true},
	{Definition: Definition{Path: "live cancel", Summary: "Cancel only a prepared operation before queue publication or game input: live cancel <operation-id>; later stages require live resume for safe cleanup", Mutates: true}, positional: "<operation-id>", allowsHome: true},
	{Definition: Definition{Path: "live abandon", Summary: "Explicitly abandon a probe before ACK: verified report, or dispatch_requested/flush_requested with durable fully queued dispatch input; preserve evidence and unknown results, retire its exact disk queue entry and release ownership without game input or claiming ACK; interrupted abandonment resumes by the same operation ID", Mutates: true}, positional: "<operation-id>", allowsHome: true},
	{Definition: Definition{Path: "live session", Summary: "Verify a retained session and its evidence: live session <session-id>; does not reconnect or authorize input", Mutates: false}, positional: "<session-id>", allowsHome: true},
	{Definition: Definition{Path: "evidence show", Summary: "Read a capture manifest: evidence show <capture-id>", Mutates: false}, positional: "<capture-id>", allowsHome: true},
	{Definition: Definition{Path: "evidence verify", Summary: "Verify manifest and original bytes: evidence verify <capture-id>", Mutates: false}, positional: "<capture-id>", allowsHome: true},
	{Definition: Definition{Path: "evidence list", Summary: "List archived capture manifests in ID order with a bounded page", Mutates: false}, flags: []flagSpec{{name: "--limit", display: "--limit <n>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "evidence bundle", Summary: "Write a complete ZIP of 1..100 verified captures: --ids <CAP-a,CAP-b> --output <new-file>; refuses existing output and caps unique payload bytes at 128 MiB", Mutates: true}, flags: []flagSpec{{name: "--ids", display: "--ids <capture-id,...>", value: true}, {name: "--output", display: "--output <new-file>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "evidence keep", Summary: "Record an explicit retention decision for one capture; repeated calls are idempotent: evidence keep <capture-id>", Mutates: true}, positional: "<capture-id>", allowsHome: true},
	{Definition: Definition{Path: "evidence remove", Summary: "Explicitly remove one unreferenced capture manifest and its unshared blob: evidence remove <capture-id>; never follows cache pruning or deletes a shared blob", Mutates: true}, positional: "<capture-id>", allowsHome: true},
	{Definition: Definition{Path: "source list", Summary: "List source repositories and product branches; --snapshot <pin> reports fixed indexed-source readiness", Mutates: false}, flags: []flagSpec{{name: "--snapshot", display: "--snapshot <pin>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "source sync", Summary: "Fetch --source <key> --product <track> [--ref <full-ref-or-commit>] and return a fixed snapshot", Mutates: true}, flags: []flagSpec{{name: "--source", display: "--source <key>", value: true}, {name: "--product", display: "--product <track>", value: true}, {name: "--ref", display: "--ref <full-ref-or-commit>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "source inspect", Summary: "Inspect one fixed source: --path <file> for an excerpt, --symbol <qualified-name> for indexed symbol search, or --target-path <path> for indexed file/asset metadata; --snapshot required", Mutates: true}, flags: []flagSpec{{name: "--snapshot", display: "--snapshot <pin-id>", value: true}, {name: "--path", display: "--path <file>", value: true}, {name: "--symbol", display: "--symbol <qualified-name>", value: true}, {name: "--target-path", display: "--target-path <path>", value: true}, {name: "--line", display: "--line <n>", value: true}, {name: "--count", display: "--count <n>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "source index", Summary: "Build syntax index for --snapshot <pin-id>, retaining parse diagnostics", Mutates: true}, flags: []flagSpec{{name: "--snapshot", display: "--snapshot <pin-id>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "source query", Summary: "Search a fixed source index by topic and evidence tier: source query <term> --snapshot <pin-id> [--mode precise|exploratory] [--topic api|lua|xml|toc|asset] [--limit <1-200>]", Mutates: true}, flags: []flagSpec{{name: "--snapshot", display: "--snapshot <pin-id>", value: true}, {name: "--mode", display: "--mode precise|exploratory", value: true}, {name: "--topic", display: "--topic <api|lua|xml|toc|asset>", value: true}, {name: "--limit", display: "--limit <1-200>", value: true}}, positional: "<term>", allowsHome: true},
	{Definition: Definition{Path: "source diff", Summary: "Compare --from <pin-id> --to <pin-id> [--limit <n> per change category]", Mutates: true}, flags: []flagSpec{{name: "--from", display: "--from <pin-id>", value: true}, {name: "--to", display: "--to <pin-id>", value: true}, {name: "--limit", display: "--limit <n>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "source validate", Summary: "Validate one pinned addon closure with --path/--toc/--snapshot, or a fixed multi-client --matrix <config.json>; report unresolved static coverage", Mutates: true}, flags: []flagSpec{{name: "--path", display: "--path <addon-root>", value: true}, {name: "--toc", display: "--toc <relative-manifest>", value: true}, {name: "--snapshot", display: "--snapshot <pin-id>", value: true}, {name: "--matrix", display: "--matrix <config.json>", value: true}}, allowsHome: true},
}

var definitions = publicDefinitions()

func publicDefinitions() []Definition {
	result := make([]Definition, len(commandContracts))
	for i := range commandContracts {
		result[i] = commandContracts[i].publicDefinition()
	}
	return result
}

func (c commandContract) publicDefinition() Definition {
	definition := c.Definition
	definition.Arguments = []string{}
	if c.positional != "" {
		definition.Arguments = []string{c.positional}
	}
	definition.Flags = c.acceptedFlagDisplays()
	if c.projectSelection() {
		definition.Summary += "; omitted --snapshot uses --project or the nearest parent project lock; explicit snapshot wins"
	}
	return definition
}

func (c commandContract) flag(name string) (flagSpec, bool) {
	if name == "--project" && c.projectSelection() {
		return flagSpec{name: name, display: "--project <directory>", value: true}, true
	}
	if name == "--format" {
		return flagSpec{name: name, display: "--format text|json|jsonl", value: true}, true
	}
	if name == "--home" && c.allowsHome {
		return flagSpec{name: name, display: "--home <root>", value: true}, true
	}
	for _, flag := range c.flags {
		if flag.name == name {
			return flag, true
		}
	}
	return flagSpec{}, false
}

func (c commandContract) acceptedFlagDisplays() []string {
	result := make([]string, 0, len(c.flags)+2)
	if c.allowsHome {
		result = append(result, "--home <root>")
	}
	result = append(result, "--format text|json|jsonl")
	if c.projectSelection() {
		result = append(result, "--project <directory>")
	}
	for _, flag := range c.flags {
		result = append(result, flag.display)
	}
	return result
}

// Commands consuming a snapshot share project fallback; project lock itself
// deliberately requires a newly selected, explicit snapshot.
func (c commandContract) projectSelection() bool {
	if strings.HasPrefix(c.Path, "project ") {
		return false
	}
	for _, flag := range c.flags {
		if flag.name == "--snapshot" {
			return true
		}
	}
	return false
}

func findCommandContract(words []string) (*commandContract, string, error) {
	if len(words) == 0 {
		return nil, "", nil
	}
	// Longest path wins so "data db2 schema <t>" routes to the schema verb
	// while "data db2 --table <t>" still routes to row read.
	best := -1
	bestWords := 0
	for i := range commandContracts {
		contract := &commandContracts[i]
		pathWords := strings.Fields(contract.Path)
		if len(words) < len(pathWords) || strings.Join(words[:len(pathWords)], " ") != contract.Path {
			continue
		}
		if best < 0 || len(pathWords) > bestWords {
			best, bestWords = i, len(pathWords)
		}
	}
	if best < 0 {
		return nil, "", fmt.Errorf("unknown command %q; use --help", strings.Join(words, " "))
	}
	contract := &commandContracts[best]
	pathWords := strings.Fields(contract.Path)
	extra := len(words) - len(pathWords)
	if extra > 1 || extra == 1 && contract.positional == "" {
		return nil, "", fmt.Errorf("unexpected argument %q", words[len(pathWords)])
	}
	return contract, contract.Path, nil
}

func collectCommandWords(args []string) ([]string, error) {
	words := make([]string, 0, len(args))
	for i := 0; i < len(args); i++ {
		arg := args[i]
		if arg == "--help" || arg == "-h" || arg == "--offline" || arg == "--resume" || arg == "--allow-partial" || arg == "--stdin" {
			continue
		}
		if !strings.HasPrefix(arg, "-") {
			words = append(words, arg)
			continue
		}
		key, _, inline := strings.Cut(arg, "=")
		spec, known := anyFlagSpec(key)
		if !known {
			return nil, fmt.Errorf("unknown flag %s", key)
		}
		if !spec.value && inline {
			return nil, fmt.Errorf("unknown flag %s", key)
		}
		if spec.value && !inline {
			if i+1 < len(args) {
				i++
			}
		}
	}
	return words, nil
}

func anyFlagSpec(name string) (flagSpec, bool) {
	if name == "--project" {
		return flagSpec{name: name, display: "--project <directory>", value: true}, true
	}
	if name == "--home" {
		return flagSpec{name: name, display: "--home <root>", value: true}, true
	}
	if name == "--format" {
		return flagSpec{name: name, display: "--format text|json|jsonl", value: true}, true
	}
	for _, contract := range commandContracts {
		for _, flag := range contract.flags {
			if flag.name == name {
				return flag, true
			}
		}
	}
	return flagSpec{}, false
}

func commandFlag(contract *commandContract, name string) (flagSpec, bool) {
	if contract == nil {
		if name == "--home" || name == "--format" {
			return anyFlagSpec(name)
		}
		return flagSpec{}, false
	}
	return contract.flag(name)
}
