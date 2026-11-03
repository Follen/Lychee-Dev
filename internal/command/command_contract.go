package command

import (
	"fmt"
	"strings"

	"github.com/follenfang/lycheedev/internal/publish"
	"github.com/follenfang/lycheedev/internal/selection"
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
	Aliases   []string `json:"aliases,omitempty"`
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
	variadic   bool
}

func dataProductFlag() flagSpec {
	var names []string
	for _, product := range selection.DataProducts() {
		names = append(names, product.Product)
	}
	return flagSpec{name: "--product", display: "--product <" + strings.Join(names, "|") + ">", value: true}
}

func liveDoctorTargetFlags() []flagSpec {
	return []flagSpec{
		{name: "--installation", display: "--installation <client>", value: true},
		{name: "--pid", display: "--pid <pid>", value: true},
		{name: "--session", display: "--session <CON-id>", value: true},
		{name: "--project", display: "--project <directory>", value: true},
	}
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
		{name: "--key-file", display: "--key-file <WoW.txt|keys.json>", value: true},
	}, extra...)
}

var commandContracts = append([]commandContract{
	curseForgeCommand("session doctor", false),
	curseForgeCommand("project list", false),
	curseForgeCommand("plugin game-versions", false),
	curseForgeCommand("plugin upload", true),
	curseForgeCommand("plugin status", false),
	curseForgeCommand("plugin verify", true),
	{Definition: Definition{Path: "discover", Summary: "Find implemented commands by task, resource, platform or product nickname; static and offline", Mutates: false}, positional: "<question>", variadic: true},
	{Definition: Definition{Path: "schema", Summary: "Describe exact command inputs, constraints, examples, effects and result envelope without accessing a workspace or network", Mutates: false}, positional: "<command...>", variadic: true},
	{Definition: Definition{Path: "update", Summary: "Update npm CLI and synchronize managed skills/addons; --plan inspects current targets without writes; --release applies an already installed release offline. Repeated --path/--installation scope targets and disable discovery. Filesystem completion does not activate a running game or Agent.", Mutates: true}, flags: []flagSpec{{name: "--file", display: "--file <targets.json> (explicit array; disables discovery)", value: true}, {name: "--release", display: "--release <distribution-root>", value: true}, {name: "--path", display: "--path <skill-directory> (repeatable)", value: true}, {name: "--installation", display: "--installation <client-directory> (repeatable)", value: true}, {name: "--plan", display: "--plan"}}, allowsHome: true},
	{Definition: Definition{Path: "project init", Summary: "Declare a project product: --product <track> [--path <directory>]; creates lycheedev.json without choosing latest", Mutates: true}, flags: []flagSpec{{name: "--product", display: "--product <track>", value: true}, {name: "--path", display: "--path <directory>", value: true}}},
	{Definition: Definition{Path: "data hotfix", Summary: "Inspect independent Hotfix records with an explicit source: --source <wago|dbcache|raidbots>. dbcache reads a pinned --snapshot <pin> (--dbcache <DBCache.bin> | --from <cache-capture>); wago queries the remote record set from --product/--build/--region/--locale with an optional --snapshot parent; raidbots reads --raidbots <DBCache.bin> under 30 days. Filters: --table --table-hash --record --push --status --region-id --search and wago --from/--to; --latest selects the largest matching push batch before pagination; dbcache --scan owns bounded pagination with --max-pages and a resumable --cursor", Mutates: true}, flags: []flagSpec{
		{name: "--source", display: "--source <wago|dbcache|raidbots>", value: true},
		{name: "--snapshot", display: "--snapshot <pin>", value: true},
		{name: "--dbcache", display: "--dbcache <DBCache.bin>", value: true},
		{name: "--raidbots", display: "--raidbots <DBCache.bin>", value: true},
		{name: "--from", display: "--from <cache-capture-or-time>", value: true},
		{name: "--to", display: "--to <time>", value: true},
		dataProductFlag(),
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
		{name: "--scan", display: "--scan (dbcache: complete bounded scan; --cursor resumes its capture)"},
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
	{Definition: Definition{Path: "live execute", Summary: "Execute a bounded probe through durable verified result and runtime release; native CON connections use project journals and stable request keys; --policy controls recovery after runtime loss", Mutates: true}, flags: []flagSpec{{name: "--file", display: "--file <lua-file>", value: true}, {name: "--session", display: "--session <session-id>", value: true}, {name: "--probe", display: "--probe <name-or-revision>", value: true}, {name: "--request", display: "--request <idempotency-key>", value: true}, {name: "--budget-seconds", display: "--budget-seconds <1-120>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "live probe list", Summary: "List active probe names without game input; --include-removed shows tombstoned names", Mutates: false}, flags: []flagSpec{{name: "--include-removed", display: "--include-removed"}, {name: "--limit", display: "--limit <1-1000>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "live probe show", Summary: "Read an exact probe by active name or immutable PRB revision: live probe show <name-or-revision>", Mutates: false}, positional: "<name-or-revision>", allowsHome: true},
	{Definition: Definition{Path: "live probe remove", Summary: "Tombstone one probe name for future selection while retaining immutable revisions and operation evidence: live probe remove <name>", Mutates: true}, positional: "<name>", allowsHome: true},
	{Definition: Definition{Path: "skill install", Summary: "Install: --release <distribution-root> --path <parent/lycheedev>; add --output <archive> to upgrade; --resume --path <target> --output <archive> resumes without --release", Mutates: true}, flags: []flagSpec{{name: "--release", display: "--release <distribution-root>", value: true}, {name: "--path", display: "--path <target>", value: true}, {name: "--output", display: "--output <archive>", value: true}, {name: "--resume", display: "--resume"}}, allowsHome: false},
	{Definition: Definition{Path: "skill status", Summary: "Inspect skill ownership and files: --path <installed-directory>", Mutates: false}, flags: []flagSpec{{name: "--path", display: "--path <installed-directory>", value: true}}, allowsHome: false},
	{Definition: Definition{Path: "skill remove", Summary: "Archive an unchanged owned skill: --path <parent/lycheedev> --output <recovery-directory outside parent>; deletes nothing", Mutates: true}, flags: []flagSpec{{name: "--path", display: "--path <parent/lycheedev>", value: true}, {name: "--output", display: "--output <recovery-directory>", value: true}}, allowsHome: false},
	{Definition: Definition{Path: "addon status", Summary: "Inspect addon files with --path <addon-directory>, or resolve a supported client with --installation <client-directory>", Mutates: false}, flags: []flagSpec{{name: "--path", display: "--path <addon-directory>", value: true}, {name: "--installation", display: "--installation <client-directory>", value: true}}, allowsHome: false},
	{Definition: Definition{Path: "addon install", Summary: "Install: --release <distribution-root> --installation <client-directory>; --output <archive> upgrades; --resume --installation <client> --output <archive> recovers without --release", Mutates: true}, flags: []flagSpec{{name: "--release", display: "--release <distribution-root>", value: true}, {name: "--installation", display: "--installation <client-directory>", value: true}, {name: "--output", display: "--output <archive>", value: true}, {name: "--resume", display: "--resume"}}, allowsHome: false},
	{Definition: Definition{Path: "addon remove", Summary: "Archive an unchanged owned addon: --installation <client-directory> --output <recovery-directory>; never deletes SavedVariables", Mutates: true}, flags: []flagSpec{{name: "--installation", display: "--installation <client-directory>", value: true}, {name: "--output", display: "--output <recovery-directory>", value: true}}, allowsHome: false},
	{Definition: Definition{Path: "data sql", Summary: "Read-only DuckDB SQL over pinned static tables, meta.Table enum/flags metadata, or explicit effective.Table overlays (query JSON hotfix array): exactly one of --sql <text>, --file <query.sql|query.json>, or --stdin; repeated --param <name=scalar> binds native $name values; --encoding csv --output <file> writes a captured CSV and manifest", Mutates: true}, flags: []flagSpec{{name: "--key-file", display: "--key-file <WoW.txt|keys.json>", value: true}, {name: "--snapshot", display: "--snapshot <pin>", value: true}, {name: "--installation", display: "--installation <client-or-game-root>", value: true}, {name: "--cdn", display: "--cdn"}, {name: "--sql", display: "--sql <text>", value: true}, {name: "--file", display: "--file <query.sql|query.json>", value: true}, {name: "--stdin", display: "--stdin"}, {name: "--param", display: "--param <name=scalar>", value: true}, {name: "--encoding", display: "--encoding csv", value: true}, {name: "--output", display: "--output <file>", value: true}, {name: "--overwrite", display: "--overwrite"}, {name: "--max-bytes", display: "--max-bytes <n>", value: true}, {name: "--offline", display: "--offline"}}, allowsHome: true},
	{Definition: Definition{Path: "data db2", Summary: "Read typed rows: --snapshot <pin> (--installation <client-or-game-root> | --cdn) --table <name> [--id <n> | --limit <n> --after-id <n>] [--max-bytes <n>] [--offline]", Mutates: true}, flags: []flagSpec{{name: "--key-file", display: "--key-file <WoW.txt|keys.json>", value: true}, {name: "--snapshot", display: "--snapshot <pin>", value: true}, {name: "--installation", display: "--installation <client-or-game-root>", value: true}, {name: "--cdn", display: "--cdn"}, {name: "--table", display: "--table <name>", value: true}, {name: "--id", display: "--id <n>", value: true}, {name: "--limit", display: "--limit <n>", value: true}, {name: "--after-id", display: "--after-id <n>", value: true}, {name: "--max-bytes", display: "--max-bytes <n>", value: true}, {name: "--offline", display: "--offline"}}, allowsHome: true},
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
	{Definition: Definition{Path: "asset inspect", Summary: "Verify CASC file: --snapshot <pin> (--installation <client-or-game-root> | --cdn) --file-id <id> [--max-bytes <n>] [--offline] [--content-variant standard|low-violence]; archive original bytes", Mutates: true}, flags: []flagSpec{{name: "--key-file", display: "--key-file <WoW.txt|keys.json>", value: true}, {name: "--content-variant", display: "--content-variant <standard|low-violence> (default: strict)", value: true}, {name: "--snapshot", display: "--snapshot <pin>", value: true}, {name: "--installation", display: "--installation <client-or-game-root>", value: true}, {name: "--cdn", display: "--cdn"}, {name: "--file-id", display: "--file-id <id>", value: true}, {name: "--max-bytes", display: "--max-bytes <n>", value: true}, {name: "--offline", display: "--offline"}}, allowsHome: true},
	{Definition: Definition{Path: "asset search", Summary: "Use one listfile mode: --query <text>, --extension <ext>, --name <path>, or --file-id <id>; requires --snapshot and --listfile <community-csv|wowexport-text|wowexport-binary>; --limit bounds search and extension pages; --offline reuses verified cache; --max-bytes bounds total listfile input (default 256 MiB, maximum 512 MiB)", Mutates: true}, flags: []flagSpec{{name: "--snapshot", display: "--snapshot <pin>", value: true}, {name: "--listfile", display: "--listfile <community-csv|wowexport-text|wowexport-binary>", value: true}, {name: "--query", display: "--query <text>", value: true}, {name: "--extension", display: "--extension <ext>", value: true}, {name: "--name", display: "--name <path>", value: true}, {name: "--file-id", display: "--file-id <id>", value: true}, {name: "--limit", display: "--limit <1-200>", value: true}, {name: "--max-bytes", display: "--max-bytes <n>", value: true}, {name: "--offline", display: "--offline"}}, allowsHome: true},
	{Definition: Definition{Path: "asset export", Summary: "Export CASC file: --snapshot <pin> (--installation <client-or-game-root> | --cdn) --file-id <id> --output <file> [--encoding raw|png|webp] [--mipmap <0..15>] [--channels <rgba-subset>] [--max-pixels <n>] [--overwrite] [--max-bytes <n>] [--offline] [--content-variant standard|low-violence]; raw default, BLP2 image conversion, existing parent required", Mutates: true}, flags: []flagSpec{{name: "--key-file", display: "--key-file <WoW.txt|keys.json>", value: true}, {name: "--content-variant", display: "--content-variant <standard|low-violence> (default: strict)", value: true}, {name: "--snapshot", display: "--snapshot <pin>", value: true}, {name: "--installation", display: "--installation <client-or-game-root>", value: true}, {name: "--cdn", display: "--cdn"}, {name: "--file-id", display: "--file-id <id>", value: true}, {name: "--output", display: "--output <file>", value: true}, {name: "--encoding", display: "--encoding raw|png|webp", value: true}, {name: "--mipmap", display: "--mipmap <0..15>", value: true}, {name: "--channels", display: "--channels <rgba-subset>", value: true}, {name: "--max-pixels", display: "--max-pixels <n>", value: true}, {name: "--overwrite", display: "--overwrite"}, {name: "--max-bytes", display: "--max-bytes <n>", value: true}, {name: "--offline", display: "--offline"}}, allowsHome: true},
	{Definition: Definition{Path: "asset demux", Summary: "Demux a bounded VP9 AVI from --path <local-file> or pinned CASC (--snapshot <pin> and --file-id <id> with --installation or --cdn); --output <existing-directory> [--max-bytes <n>] [--max-frames <n>] [--allow-partial] [--overwrite]", Mutates: true}, flags: []flagSpec{{name: "--key-file", display: "--key-file <WoW.txt|keys.json>", value: true}, {name: "--path", display: "--path <local-file>", value: true}, {name: "--snapshot", display: "--snapshot <pin>", value: true}, {name: "--installation", display: "--installation <client-or-game-root>", value: true}, {name: "--cdn", display: "--cdn"}, {name: "--file-id", display: "--file-id <id>", value: true}, {name: "--output", display: "--output <existing-directory>", value: true}, {name: "--max-bytes", display: "--max-bytes <n>", value: true}, {name: "--max-frames", display: "--max-frames <n>", value: true}, {name: "--allow-partial", display: "--allow-partial"}, {name: "--overwrite", display: "--overwrite"}, {name: "--offline", display: "--offline"}}, allowsHome: true},
	{Definition: Definition{Path: "version", Summary: "Inspect the native toolkit build", Mutates: false}, allowsHome: true},
	{Definition: Definition{Path: "describe", Summary: "Read implemented command contracts", Mutates: false}, allowsHome: true},
	{Definition: Definition{Path: "init", Summary: "Create a new-format workspace without importing old data; --plan resolves the fresh-isolation plan without changing anything, --fresh isolates an occupied legacy root into a deterministic sibling archive first, --resume completes an interrupted archive switch", Mutates: true}, flags: []flagSpec{{name: "--plan", display: "--plan"}, {name: "--fresh", display: "--fresh"}, {name: "--resume", display: "--resume"}}, allowsHome: true},
	{Definition: Definition{Path: "doctor", Summary: "Diagnose workspace, data, LuaLS, source and a selected live target or original CON; read-only by default; --fix repairs managed installation and recovers the original connection; --fix --hard-reset archives and initializes all 500 slots; exit 3 means an unhealthy check", Mutates: true}, flags: append(liveDoctorTargetFlags(),
		flagSpec{name: "--fix", display: "--fix"},
		flagSpec{name: "--hard-reset", display: "--hard-reset (requires --fix)"},
		flagSpec{name: "--offline", display: "--offline"},
		flagSpec{name: "--release", display: "--release <distribution-root>", value: true},
		flagSpec{name: "--output", display: "--output <upgrade-archive>", value: true},
		flagSpec{name: "--request", display: "--request <idempotency-key>", value: true},
		flagSpec{name: "--wait-seconds", display: "--wait-seconds <1-600> (default 30)", value: true}), allowsHome: true},
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
	{Definition: Definition{Path: "live instances", Summary: "List supported installations and running windows without game input or actor claims; use live connect for fresh native identity binding", Mutates: false}, flags: []flagSpec{{name: "--installation", display: "--installation <client-or-game-root>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "live connect", Summary: "Discover and freshly bind a native memory/500-slot connection in the invoking project; optional snapshot constrains build; --session CON-id resumes retained work", Mutates: true}, flags: []flagSpec{
		{name: "--snapshot", display: "--snapshot <pin>", value: true},
		{name: "--character", display: "--character <name>", value: true},
		{name: "--realm", display: "--realm <realm>", value: true},
		{name: "--pid", display: "--pid <pid>", value: true},
		{name: "--installation", display: "--installation <client>", value: true},
		{name: "--session", display: "--session <session-id>", value: true},
	}, allowsHome: true},
	{Definition: Definition{Path: "live reload", Summary: "Perform one correlated UI reload on the selected client and verify the new runtime: --session <session-id> --request <idempotency-key>", Mutates: true}, flags: []flagSpec{{name: "--session", display: "--session <session-id>", value: true}, {name: "--request", display: "--request <idempotency-key>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "live reload fallback", Summary: "Activate a clean managed addon and its 500 slots on an explicit installation/PID using fresh Mailbox readiness for a recorded reload, or waiting for one manual reload when no current input protocol is loaded, then verify a new runtime and fresh connection; --request is idempotent", Mutates: true}, flags: []flagSpec{{name: "--installation", display: "--installation <client>", value: true}, {name: "--pid", display: "--pid <pid>", value: true}, {name: "--request", display: "--request <idempotency-key>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "live bugs", Summary: "Read 1-100 retained provider errors; native CON connections persist and release the bounded observation result automatically", Mutates: true}, flags: []flagSpec{{name: "--session", display: "--session <session-id>", value: true}, {name: "--request", display: "--request <idempotency-key>", value: true}, {name: "--count", display: "--count <1-100>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "target resolve", Summary: "Pin data identity from --target <name>, --installation <game-root-or-client> optionally constrained by --product <track>, remote --product <track> without installation, or --file <selection.json>; local selection never falls back to remote; named targets resolve their exact configured identity at command start", Mutates: true}, flags: []flagSpec{
		{name: "--file", display: "--file <selection.json>", value: true},
		{name: "--target", display: "--target <name>", value: true},
		{name: "--installation", display: "--installation <game-root-or-client>", value: true},
		dataProductFlag(),
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
		dataProductFlag(),
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
	{Definition: Definition{Path: "live status", Summary: "Read persisted work or a BTP bootstrap attempt without sending input: live status <operation-id>", Mutates: false}, positional: "<operation-id>", allowsHome: true},
	{Definition: Definition{Path: "live resume", Summary: "Recover the original Native CON connection and request; revalidate identity and never repeat unknown input", Mutates: true}, positional: "<operation-id>", allowsHome: true},
	{Definition: Definition{Path: "live cancel", Summary: "Cancel a prepared historical OP record without game input; use live abandon to terminate later historical recovery", Mutates: true}, positional: "<operation-id>", allowsHome: true},
	{Definition: Definition{Path: "live abandon", Summary: "Explicitly stop unresolved probe, bugs, fixed reload, or bootstrap recovery; preserve evidence and unknown results, release host ownership without game input or claiming business completion", Mutates: true}, positional: "<operation-or-bootstrap-id>", allowsHome: true},
	{Definition: Definition{Path: "live session", Summary: "Verify a retained session and its evidence: live session <session-id>; does not reconnect or authorize input", Mutates: false}, positional: "<session-id>", allowsHome: true},
	{Definition: Definition{Path: "live disconnect", Summary: "Unbind a quiescent memory/slot connection and release its durable host ownership; repeat safely after interruption", Mutates: true}, positional: "<connection-id>"},
	{Definition: Definition{Path: "evidence show", Summary: "Read a capture manifest: evidence show <capture-id>", Mutates: false}, positional: "<capture-id>", allowsHome: true},
	{Definition: Definition{Path: "evidence verify", Summary: "Verify manifest and original bytes: evidence verify <capture-id>", Mutates: false}, positional: "<capture-id>", allowsHome: true},
	{Definition: Definition{Path: "evidence list", Summary: "List archived capture manifests in ID order with a bounded page", Mutates: false}, flags: []flagSpec{{name: "--limit", display: "--limit <n>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "evidence bundle", Summary: "Write a complete ZIP of 1..100 verified captures: --ids <CAP-a,CAP-b> --output <new-file>; refuses existing output and caps unique payload bytes at 128 MiB", Mutates: true}, flags: []flagSpec{{name: "--ids", display: "--ids <capture-id,...>", value: true}, {name: "--output", display: "--output <new-file>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "evidence keep", Summary: "Record an explicit retention decision for one capture; repeated calls are idempotent: evidence keep <capture-id>", Mutates: true}, positional: "<capture-id>", allowsHome: true},
	{Definition: Definition{Path: "evidence remove", Summary: "Explicitly remove one unreferenced capture manifest and its unshared blob: evidence remove <capture-id>; never follows cache pruning or deletes a shared blob", Mutates: true}, positional: "<capture-id>", allowsHome: true},
	{Definition: Definition{Path: "source list", Summary: "List source repositories and product branches; --snapshot <pin> reports fixed file-mapping readiness", Mutates: false}, flags: []flagSpec{{name: "--snapshot", display: "--snapshot <pin>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "source batch", Summary: "Run 1-32 query/refs/context requests from a JSON file against one fixed snapshot, sharing preparation and a request-scoped LuaLS session; each item retains its own cursor and coverage", Mutates: true}, flags: []flagSpec{{name: "--snapshot", display: "--snapshot <pin-id>", value: true}, {name: "--file", display: "--file <requests.json>", value: true}, {name: "--environment", display: "--environment <client-pin>", value: true}, {name: "--flow", display: "--flow"}, {name: "--session-reuse", display: "--session-reuse"}, {name: "--static-only", display: "--static-only"}, {name: "--release", display: "--release <release-root>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "source sync", Summary: "Fetch --source <key> --product <track> [--ref <full-ref-or-commit>] and return a fixed snapshot", Mutates: true}, flags: []flagSpec{{name: "--source", display: "--source <key>", value: true}, {name: "--product", display: "--product <track>", value: true}, {name: "--ref", display: "--ref <full-ref-or-commit>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "source inspect", Summary: "Inspect one fixed source: --path <file> for original lines, --symbol <qualified-name> for symbol candidates, or --target-path <path> for file/asset facts", Mutates: true}, flags: []flagSpec{{name: "--snapshot", display: "--snapshot <pin-id>", value: true}, {name: "--path", display: "--path <file>", value: true}, {name: "--symbol", display: "--symbol <qualified-name>", value: true}, {name: "--target-path", display: "--target-path <path>", value: true}, {name: "--line", display: "--line <n>", value: true}, {name: "--count", display: "--count <n>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "source index", Summary: "Explicitly warm the rebuildable file mapping for --snapshot <pin-id>, retaining parse diagnostics", Mutates: true}, flags: []flagSpec{{name: "--snapshot", display: "--snapshot <pin-id>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "source query", Summary: "Prepare and search the exact pinned source mapping by topic and evidence tier with bounded, resumable pages", Mutates: true}, flags: []flagSpec{{name: "--snapshot", display: "--snapshot <pin-id>", value: true}, {name: "--mode", display: "--mode precise|exploratory", value: true}, {name: "--topic", display: "--topic <api|lua|xml|toc|asset>", value: true}, {name: "--limit", display: "--limit <1-200>", value: true}, {name: "--cursor", display: "--cursor <token>", value: true}}, positional: "<term>", allowsHome: true},
	{Definition: Definition{Path: "source refs", Summary: "Find bounded relations for one pinned symbol ID (or an unambiguous symbol name); retain relation state, coverage and next cursor", Mutates: true}, flags: []flagSpec{{name: "--snapshot", display: "--snapshot <pin-id>", value: true}, {name: "--symbol-id", display: "--symbol-id <id>", value: true}, {name: "--symbol", display: "--symbol <name>", value: true}, {name: "--direction", display: "--direction <incoming|outgoing|both>", value: true}, {name: "--limit", display: "--limit <1-200>", value: true}, {name: "--cursor", display: "--cursor <token>", value: true}, {name: "--environment", display: "--environment <client-pin>", value: true}, {name: "--session-reuse", display: "--session-reuse"}, {name: "--static-only", display: "--static-only"}, {name: "--release", display: "--release <release-root>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "source context", Summary: "Read bounded source context for one pinned symbol ID (or an unambiguous name), with direct relations and load evidence; --flow requests bounded secret-value analysis", Mutates: true}, flags: []flagSpec{{name: "--snapshot", display: "--snapshot <pin-id>", value: true}, {name: "--symbol-id", display: "--symbol-id <id>", value: true}, {name: "--symbol", display: "--symbol <name>", value: true}, {name: "--limit", display: "--limit <1-200>", value: true}, {name: "--max-bytes", display: "--max-bytes <1-1048576>", value: true}, {name: "--max-lines", display: "--max-lines <1-2000>", value: true}, {name: "--depth", display: "--depth <0-4>", value: true}, {name: "--cursor", display: "--cursor <token>", value: true}, {name: "--environment", display: "--environment <client-pin>", value: true}, {name: "--flow", display: "--flow"}, {name: "--session-reuse", display: "--session-reuse"}, {name: "--static-only", display: "--static-only"}, {name: "--release", display: "--release <release-root>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "source session status", Summary: "Inspect the opt-in LuaLS broker for this home without starting it", Mutates: false}, flags: []flagSpec{{name: "--release", display: "--release <release-root>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "source session close", Summary: "Retire the verified opt-in LuaLS broker and release its worktree lease", Mutates: true}, flags: []flagSpec{{name: "--release", display: "--release <release-root>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "source diff", Summary: "Compare --from <pin-id> --to <pin-id> [--limit <n> per change category]", Mutates: true}, flags: []flagSpec{{name: "--from", display: "--from <pin-id>", value: true}, {name: "--to", display: "--to <pin-id>", value: true}, {name: "--limit", display: "--limit <n>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "source prune", Summary: "Explicitly reclaim only unused source worktrees toward --target-bytes <n>; fixed Git pins, facts and captures remain", Mutates: true}, flags: []flagSpec{{name: "--target-bytes", display: "--target-bytes <n>", value: true}}, allowsHome: true},
	{Definition: Definition{Path: "source validate", Summary: "Validate one pinned addon closure with --path/--toc/--snapshot, or a fixed multi-client --matrix <config.json>; add --semantic for bundled LuaLS using fixed client API metadata; report unresolved coverage", Mutates: true}, flags: []flagSpec{{name: "--semantic", display: "--semantic"}, {name: "--environment", display: "--environment <client-pin>", value: true}, {name: "--release", display: "--release <release-root>", value: true}, {name: "--path", display: "--path <addon-root>", value: true}, {name: "--toc", display: "--toc <relative-manifest>", value: true}, {name: "--snapshot", display: "--snapshot <pin-id>", value: true}, {name: "--matrix", display: "--matrix <config.json>", value: true}}, allowsHome: true},
}, platformPublishContracts()...)

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
	definition.Aliases = c.commandAliases()
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
	for _, flag := range c.sharedFlags() {
		if flag.name == name {
			return flag, true
		}
	}
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
	for _, flag := range c.sharedFlags() {
		result = append(result, flag.display)
	}
	return result
}

// These are part of the same contract table, shared by parser, describe and Skill.
func (c commandContract) sharedFlags() []flagSpec {
	if c.boundedDataQuery() {
		return []flagSpec{{name: "--timeout-seconds", display: "--timeout-seconds <1-3600> (default 300; entire query)", value: true}}
	}
	switch c.Path {
	case "live connect", "live execute", "live disconnect", "live status", "live resume", "live session", "live reload", "live reload fallback", "live bugs":
	default:
		return nil
	}
	flags := []flagSpec{{name: "--project", display: "--project <directory>", value: true}}
	if c.Path != "live status" && c.Path != "live session" {
		flags = append(flags, flagSpec{name: "--wait-seconds", display: "--wait-seconds <1-600> (default 120)", value: true})
	}
	if c.Path == "live execute" {
		flags = append(flags, flagSpec{name: "--policy", display: "--policy <opaque|observation> (default opaque)", value: true})
	}
	return flags
}

func (c commandContract) boundedDataQuery() bool {
	return strings.HasPrefix(c.Path, "data ") && c.Path != "data hotfix"
}

// Commands consuming a snapshot share project fallback; project lock itself
// deliberately requires a newly selected, explicit snapshot.
func (c commandContract) projectSelection() bool {
	if c.Path == "live connect" {
		return false
	}
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
	words = canonicalCommandWords(words)
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
	if !contract.variadic && extra > 1 || extra > 0 && contract.positional == "" {
		return nil, "", fmt.Errorf("unexpected argument %q", words[len(pathWords)])
	}
	return contract, contract.Path, nil
}

func anyFlagSpec(name string) (flagSpec, bool) {
	for _, contract := range commandContracts {
		for _, flag := range contract.sharedFlags() {
			if flag.name == name {
				return flag, true
			}
		}
	}
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

// These input constraints and examples are part of the command directory.
func commandConstraints(route string) []string {
	switch route {
	case "data sql":
		return []string{"Exactly one of --sql, --file, --stdin is required.", "--file accepts SQL text or a query JSON object; --param name=scalar may repeat with unique names.", "A pinned Data snapshot is required (explicitly or through the project); choose --installation or --cdn.", "CSV export requires --encoding csv and --output; --overwrite permits replacing the chosen output."}
	case "data hotfix":
		return []string{"--source is required: wago, dbcache or raidbots.", "dbcache requires --snapshot and exactly one of --dbcache or --from CAP; raidbots requires --raidbots; wago requires explicit product/build/region/locale or compatible pinned parent.", "--scan is a bounded dbcache scan; --cursor resumes its original capture, not another source.", "CSV export requires both --encoding csv and --output; other encodings are not supported."}
	case "target resolve":
		return []string{"Choose --file, --target, --installation, or remote --product with --region and --locale.", "--file cannot combine with target/installation/product/build/region/locale/definitions/from/offline.", "--target cannot combine with file/installation/product/build/region/locale. --build requires --product.", "--from preserves the parent fixed identity; it cannot silently switch products or configurations."}
	case "target add":
		return []string{"A name plus --product, --region, --locale and exactly one of --installation or --remote are required.", "--replace explicitly replaces an existing named target; pinned records remain immutable."}
	case "source refs", "source context":
		return []string{"Exactly one of --symbol or --symbol-id is required.", "Third-party source requires an explicit compatible --environment PIN. Blizzard source uses its fixed environment and rejects a conflicting explicit environment.", "Pagination must retain the same snapshot, environment and query. Bounded/truncated or unsupported flow does not prove absence."}
	case "source sync":
		return []string{"--source and --product identify the repository and track. --ref fixes the requested ref; the result records its exact commit."}
	case "source batch":
		return []string{"--file supplies the batch JSON request; every query keeps its source/environment identity and coverage."}
	case "source prune":
		return []string{"--target-bytes is required; explicit zero requests maximal eligible cache reclamation."}
	case "source validate":
		return []string{"Choose a fixed snapshot or a local --path; --toc selects an addon manifest, --matrix selects explicit client contexts.", "--semantic uses the compatible pinned API environment; incomplete loading context remains partial."}
	case "source diff":
		return []string{"--from and --to identify the two fixed source snapshots."}
	case "live execute":
		return []string{"--session CON, --request, --budget-seconds and exactly one of --probe or --file are required.", "--policy observation is explicit; unknown submitted input is never re-executed."}
	case "live connect":
		return []string{"Select the installation/PID explicitly when discovery is ambiguous. --session resumes the original connection.", "The actual installation/PID/creation time/HWND, actor, runtime, owner and fence must match before input."}
	case "live reload", "live bugs":
		return []string{"--session and --request identify the original connection and durable request."}
	case "live reload fallback":
		return []string{"--installation, --pid and --request are required; fallback is limited to the original recoverable connection."}
	case "live probe put":
		return []string{"--name and --file are required; source is stored as an immutable probe revision without game input."}
	case "doctor":
		return []string{"--hard-reset requires --fix. Default diagnosis is read-only; --fix uses managed repair and original connection recovery."}
	case "config set":
		return []string{"At least one of --cache-max-bytes or --download-workers is required. Explicit zero retains its documented default/reset meaning."}
	case "init":
		return []string{"At most one enabled flag among --plan, --fresh, --resume."}
	case "addon status":
		return []string{"Choose --installation or --path, not both."}
	case "skill install", "addon install":
		return []string{"Normal install requires --release and a destination. Upgrade uses --output for recovery; --resume uses original destination/output without a new release."}
	case "skill remove", "addon remove":
		return []string{"A destination and --output recovery directory are required; removal archives owned files and retains SavedVariables."}
	case "data db2":
		return []string{"--table is required. --id is mutually exclusive with --after-id and --limit; ID zero remains an explicit ID."}
	case "data db2 search":
		return []string{"Table argument, --field, --query and a bounded --limit select the substring search."}
	case "data db2 foreign-key":
		return []string{"Table argument, --field, --value and --limit are required."}
	case "data db2 stream":
		return []string{"Requires --format jsonl and a bounded --limit. Only a valid end frame plus exit 0 completes the stream."}
	}
	if strings.HasPrefix(route, "data ") || strings.HasPrefix(route, "asset ") {
		return []string{"Pinned Data and exactly one source (--installation or --cdn) are required for table/file reads; required domain IDs are named in the command summary."}
	}
	return []string{}
}

func commandExamples(c commandContract) [][]string {
	args := strings.Fields(c.Path)
	switch c.Path {
	case "discover":
		args = append(args, "无限服 法术查询")
	case "schema":
		args = append(args, "data", "sql")
	case "target add":
		args = append(args, "forever-cn", "--product", "forever", "--region", "cn", "--locale", "zhCN", "--remote")
	case "target resolve":
		args = append(args, "--product", "forever", "--region", "cn", "--locale", "zhCN")
	case "target available":
		args = append(args, "--region", "cn")
	case "data sql":
		args = append(args, "--snapshot", "PIN-example", "--cdn", "--sql", "SELECT ID FROM SpellName WHERE ID = $id", "--param", "id=441776")
	case "source refs", "source context":
		args = append(args, "--snapshot", "PIN-addon", "--environment", "PIN-client", "--symbol", "Example:OnEvent")
	case "source sync":
		args = append(args, "--source", "wow-ui-source", "--product", "forever", "--ref", "forever")
	case "source prune":
		args = append(args, "--target-bytes", "0")
	case "config set":
		args = append(args, "--download-workers", "2")
	case "project init":
		args = append(args, "--product", "forever")
	case "project lock":
		args = append(args, "--snapshot", "PIN-example")
	case "live execute":
		args = append(args, "--session", "CON-example", "--request", "inspect-1", "--probe", "inspect", "--budget-seconds", "10")
	case "live probe put":
		args = append(args, "--name", "inspect", "--file", "probe.lua")
	case "source batch":
		args = append(args, "--snapshot", "PIN-example", "--file", "batch.json")
	case "source diff":
		args = append(args, "--from", "PIN-before", "--to", "PIN-after")
	default:
		if c.positional != "" {
			value := "example"
			switch c.positional {
			case "<Table>":
				value = "SpellName"
			case "<term>":
				value = "GetBuildInfo"
			case "<operation-id>", "<session-id>":
				value = "CON-example"
			case "<capture-id>":
				value = "CAP-example"
			}
			args = append(args, value)
		}
		values := map[string]string{"--product": "forever", "--source": "wow-ui-source", "--region": "cn", "--locale": "zhCN", "--snapshot": "PIN-example", "--session": "CON-example", "--request": "inspect-1", "--budget-seconds": "10", "--pid": "1234", "--installation": "C:/Games/WoW/_forever_", "--file": "input.json", "--name": "inspect", "--path": "C:/Tools/lycheedev", "--output": "C:/Recovery/example", "--field": "Name_lang", "--query": "example", "--table": "SpellName", "--limit": "10", "--value": "1", "--spell-id": "441776", "--item-id": "19019", "--file-data-id": "134400", "--journal-encounter-id": "1"}
		for _, name := range requiredFlags(c.Path) {
			args = append(args, name, values[name])
		}
		if c.projectSelection() {
			args = append(args, "--snapshot", "PIN-example")
		}
		if _, ok := c.flag("--cdn"); ok {
			args = append(args, "--cdn")
		}
		switch c.Path {
		case "data hotfix":
			args = []string{"data", "hotfix", "--source", "dbcache", "--snapshot", "PIN-example", "--dbcache", "DBCache.bin"}
		case "data creature display":
			args = append(args, "--display-id", "1")
		case "data decor get":
			args = append(args, "--id", "1")
		case "source inspect":
			args = append(args, "--path", "Example.lua")
		case "source validate":
			args = append(args, "--path", "C:/AddOns/Example", "--toc", "Example.toc")
		case "skill install":
			args = append(args, "--release", "C:/Release", "--path", "C:/Skills/lycheedev")
		case "addon install":
			args = append(args, "--release", "C:/Release", "--installation", "C:/Games/WoW/_forever_")
		case "addon status":
			args = append(args, "--installation", "C:/Games/WoW/_forever_")
		case "asset inspect":
			args = append(args, "--file-id", "134400")
		case "asset export":
			args = append(args, "--file-id", "134400", "--output", "C:/Exports/icon.blp")
		case "asset demux":
			args = append(args, "--file-id", "134400", "--output", "C:/Exports/media")
		case "asset search":
			args = append(args, "--listfile", "community-csv", "--query", "icon")
		case "evidence bundle":
			args = append(args, "--ids", "CAP-example", "--output", "C:/Exports/evidence.zip")
		}
	}
	if c.Path == "data db2 stream" {
		args = append(args, "--format", "jsonl")
		return [][]string{args}
	}

	if strings.HasPrefix(c.Path, "publish ") {
		args = append(args, "--file", "input.json", "--plan")
	}
	args = append(args, "--format", "json")
	return [][]string{args}
}

func (c commandContract) commandAliases() []string {
	if c.Path == "data db2" || strings.HasPrefix(c.Path, "data db2 ") {
		return []string{strings.Replace(c.Path, "data db2", "data table", 1)}
	}
	return nil
}

func canonicalCommandWords(words []string) []string {
	result := append([]string(nil), words...)
	if len(result) >= 2 && result[0] == "data" && result[1] == "table" {
		result[1] = "db2"
	}
	return result
}

func requiredFlags(route string) []string {
	if publishRequestRequired(route) {
		return []string{"--request"}
	}
	switch route {
	case "publish curseforge plugin upload":
		return []string{"--request"}
	case "project init":
		return []string{"--product"}
	case "project lock":
		return []string{"--snapshot"}
	case "source sync":
		return []string{"--source", "--product"}
	case "source diff":
		return []string{"--from", "--to"}
	case "source prune":
		return []string{"--target-bytes"}
	case "target add":
		return []string{"--product", "--region", "--locale"}
	case "target available":
		return []string{"--region"}
	case "live probe put":
		return []string{"--name", "--file"}
	case "live execute":
		return []string{"--session", "--request", "--budget-seconds"}
	case "live reload", "live bugs":
		return []string{"--session", "--request"}
	case "live reload fallback":
		return []string{"--installation", "--pid", "--request"}
	case "data hotfix":
		return []string{"--source"}
	case "data db2":
		return []string{"--table"}
	case "data db2 search":
		return []string{"--field", "--query", "--limit"}
	case "data db2 foreign-key":
		return []string{"--field", "--value", "--limit"}
	case "data db2 stream", "data decor list":
		return []string{"--limit"}
	case "data spell info", "data spell auras", "data spell summons":
		return []string{"--spell-id"}
	case "data item get", "data item models", "data item geosets", "data item textures":
		return []string{"--item-id"}
	case "data creature model":
		return []string{"--file-data-id"}
	case "data encounter get":
		return []string{"--journal-encounter-id"}
	case "skill status":
		return []string{"--path"}
	case "skill remove":
		return []string{"--path", "--output"}
	case "addon remove":
		return []string{"--installation", "--output"}
	}
	return nil
}

func curseForgeCommand(action string, mutates bool) commandContract {
	flags := []flagSpec{{name: "--file", display: "--file <input.json>", value: true}, {name: "--stdin", display: "--stdin"}, {name: "--plan", display: "--plan (local validation; no credentials or network)"}}
	if action == "plugin upload" {
		flags = append(flags, flagSpec{name: "--request", display: "--request <original-upload-id>", value: true})
	}
	return commandContract{Definition: Definition{Path: "publish curseforge " + action, Summary: "CurseForge " + action + "; JSON fields and dynamic options are exposed by schema; omitted input is {}", Mutates: mutates}, flags: flags, allowsHome: true}
}

func helpDefinitions(words []string) ([]Definition, error) {
	if len(words) == 0 {
		return definitions, nil
	}
	c, _, err := findCommandContract(words)
	if err == nil && c != nil {
		return []Definition{c.publicDefinition()}, nil
	}
	prefix := strings.Join(canonicalCommandWords(words), " ") + " "
	var matches []Definition
	for _, candidate := range commandContracts {
		if strings.HasPrefix(candidate.Path, prefix) {
			matches = append(matches, candidate.publicDefinition())
		}
	}
	if len(matches) > 0 {
		return matches, nil
	}
	return nil, err
}

func platformPublishContracts() []commandContract {
	var result []commandContract
	add := func(platform string, actions []string) {
		for _, action := range actions {
			route := "publish " + platform + " " + action
			flags := []flagSpec{{name: "--file", display: "--file <input.json>", value: true}, {name: "--stdin", display: "--stdin"}, {name: "--plan", display: "--plan (local validation; no credentials or network)"}}
			if publishRequestRequired(route) {
				flags = append(flags, flagSpec{name: "--request", display: "--request <original-operation-id>", value: true})
			}
			result = append(result, commandContract{Definition: Definition{Path: route, Summary: platform + " " + action + "; schema exposes nested input, dynamic options, effects and recovery", Mutates: publishMutates(platform, action)}, flags: flags, allowsHome: true})
		}
	}
	add("newbee", publish.NewBeeActions())
	add("modus", publish.ModusActions())
	add("blackbox", publish.BlackboxActions())
	add("dd", []string{
		"session doctor", "session start", "session status", "session close",
		"options life-types", "options game-types", "options game-versions", "options vip-levels", "options channels", "options associated-acts",
		"plugin list", "plugin get", "plugin categories", "plugin versions", "plugin create", "plugin update", "plugin edit", "plugin delete", "plugin upload",
		"config list", "config get", "config create", "config update", "config edit", "config delete", "config upload",
		"wa list", "wa get", "wa categories", "wa parse", "wa create", "wa update", "wa edit", "wa delete", "wa upload",
		"backup list", "backup get", "backup scan", "backup generate", "backup upload", "backup rename", "backup delete", "backup delete-preview", "backup delete-many",
		"operation status", "operation verify", "operation resume",
	})
	return result
}
