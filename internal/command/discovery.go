package command

import (
	"github.com/follenfang/lycheedev/internal/codebase"
	"github.com/follenfang/lycheedev/internal/publish"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
	"reflect"
	"sort"
	"strings"
)

type FlagSchema struct {
	Required     bool     `json:"required"`
	RequiredWhen string   `json:"requiredWhen,omitempty"`
	Name         string   `json:"name"`
	Type         string   `json:"type"`
	Description  string   `json:"description"`
	Repeatable   bool     `json:"repeatable"`
	Default      any      `json:"default,omitempty"`
	Enum         []string `json:"enum,omitempty"`
	Minimum      *int64   `json:"minimum,omitempty"`
	Maximum      *int64   `json:"maximum,omitempty"`
	Pattern      string   `json:"pattern,omitempty"`
}

type CommandSchema struct {
	Schema      string                         `json:"schema"`
	Command     Definition                     `json:"command"`
	Inputs      []FlagSchema                   `json:"inputs"`
	Positional  map[string]any                 `json:"positional"`
	Constraints []string                       `json:"constraints"`
	Examples    [][]string                     `json:"examples"`
	Effects     []string                       `json:"effects"`
	Result      map[string]any                 `json:"result"`
	Products    []selection.DataProductProfile `json:"products,omitempty"`
	Details     map[string]any                 `json:"details,omitempty"`
}

func (c commandContract) schema() CommandSchema {
	result := CommandSchema{Schema: "lycheedev.command-schema.v1", Command: c.publicDefinition(), Inputs: []FlagSchema{}, Positional: map[string]any{"required": c.positional != "", "syntax": c.positional, "variadic": c.variadic}, Constraints: []string{"Unknown flags and repeated scalar flags are rejected. --param allows distinct names; update allows repeated --path and --installation.", "Use -- before a positional value beginning with -. Boolean flags accept =true or =false. Omitted values use the reported defaults; empty scalar values are rejected.", "Source, API environment, Data, Changes and Live identities are independent. Explicit --snapshot wins over project selection."}, Examples: [][]string{}, Effects: []string{}, Result: resultEnvelopeSchema()}
	for _, display := range c.acceptedFlagDisplays() {
		name := strings.Fields(display)[0]
		spec, _ := c.flag(name)
		result.Inputs = append(result.Inputs, describeFlag(c.Path, spec))
	}
	result.Constraints = append(result.Constraints, commandConstraints(c.Path)...)
	if business := businessResultSchema(c.Path); business != nil {
		result.Result["properties"].(map[string]any)["result"] = business
	}
	result.Examples = commandExamples(c)
	if c.Mutates {
		result.Effects = append(result.Effects, "May write local workspace state, captures or managed files; see the command summary and conditional effects below.")
	}
	if c.Path == "schema" || c.Path == "discover" || c.Path == "describe" || c.Path == "version" {
		result.Effects = []string{"Static, offline: no workspace initialization, credentials, network, worker or game access."}
	}
	if c.projectSelection() {
		result.Constraints = append(result.Constraints, "Omitted --snapshot resolves --project or the nearest project lock; it does not create a new pin.")
	}
	if strings.HasPrefix(c.Path, "source ") {
		result.Effects = append(result.Effects, "Semantic research may prepare pinned source and start the packaged LuaLS worker. --static-only (where supported) requests bounded static evidence.")
	}
	if strings.HasPrefix(c.Path, "data ") || strings.HasPrefix(c.Path, "asset ") {
		result.Effects = append(result.Effects, "Reads pinned data; preparation may read the selected installation or use network metadata/CDN. --offline prohibits preparation downloads where supported; captures/cache remain local writes.")
	}
	if strings.HasPrefix(c.Path, "live ") {
		result.Effects = append(result.Effects, "Game input occurs only for connect/execute/reload/disconnect/recovery when their protocol stage requires it. Preserve original CON/request and deadline; submitted or uncertain input is never replayed. Status/instances/probe storage do not send game input.")
	}
	if c.Path == "doctor" {
		result.Effects = append(result.Effects, "Default is read-only diagnosis. --fix performs managed repair and original connection recovery; --hard-reset requires --fix and archives slots before initialization.")
	}
	if c.Path == "update" {
		result.Effects = append(result.Effects, "--plan inspects targets without writes; normal execution updates managed local artifacts and may invoke npm. Running game/Agent activation is separate.")
	}
	if c.Path == "target available" || c.Path == "target resolve" || c.Path == "target add" || c.Path == "discover" {
		result.Products = selection.DataProducts()
	}
	if c.Path == "data sql" {
		result.Details = map[string]any{
			"language":   "DuckDB 2.0.0 native SELECT, WITH/CTE, joins, subqueries, grouping, windows, ordering and LIMIT/OFFSET. One read-only statement; EXPLAIN and EXPLAIN ANALYZE are supported.",
			"sources":    "Pinned table names (also static.Table), meta.Table metadata and explicit effective.Table Hotfix overlays from query JSON. External files, network reads and extension loading are disabled.",
			"parameters": "Repeated --param name=scalar binds $name. The supplied names must exactly match the query. null, booleans, signed/unsigned integers and floating scalars retain their types; other values are strings. Duplicate names fail.",
			"precision":  "BIGINT/UBIGINT inputs and integer/DECIMAL outputs retain exact JSON numbers. Fractional parameters use binary64; use a string parameter with an explicit DECIMAL cast for exact decimal input. Returned types identify each native result column.",
			"functions":  "Native DuckDB built-in functions. Use (Flags & $mask) <> 0 for flag tests; bit_and(value) is a native aggregate. No legacy function or parameter translation.",
			"budgets":    "Whole-query timeout defaults to 300 seconds. DuckDB uses one thread, 64 MiB memory, no disk spill; results are limited to 16 MiB and source scans to 10 million rows. Data reader budgets are separate. Exhaustion is an error.",
			"explain":    "plan.entries contains native DuckDB plan text; EXPLAIN ANALYZE executes the query. scans records Data source calls, supplied rows and indexed lookups, including engine prefetch.",
			"pagination": "Use a stable ORDER BY with LIMIT/OFFSET and retain the same PIN and Hotfix captures.",
		}
	}
	if c.Path == "data sql" {
		result.Details["jsonInput"] = jsonTypeSchema(reflect.TypeFor[records.DataQuery](), map[reflect.Type]bool{})
	}
	if c.Path == "source batch" {
		result.Details = map[string]any{"jsonInput": jsonTypeSchema(reflect.TypeFor[codebase.BatchRequest](), map[reflect.Type]bool{})}
	}
	if c.Path == "target resolve" {
		result.Details = map[string]any{"selectionFile": jsonTypeSchema(reflect.TypeFor[selection.SelectionSpec](), map[reflect.Type]bool{})}
	}
	if strings.HasPrefix(c.Path, "publish ") {
		spec, ok := publishInputContract(c.Path)
		if ok {
			result.Details = map[string]any{"jsonInput": spec, "inputLimitBytes": publish.MaxInputBytes, "inputSemantics": "Omission, null, false, zero and empty arrays are distinct. Unknown and duplicate fields are rejected by this same input contract."}
			result.Effects = spec.Effects
			result.Constraints = append(result.Constraints, "Choose --file or --stdin; omitted input is {}. --plan validates locally without reading credentials, logging in or submitting.")
			if publishRequestRequired(c.Path) {
				result.Constraints = append(result.Constraints, "--request is required. Preserve it when checking or resuming the original operation.")
			}
		}
	}
	return result
}

func describeFlag(route string, s flagSpec) FlagSchema {
	f := FlagSchema{Name: s.name, Type: "string", Description: s.display, Repeatable: s.name == "--param" || route == "update" && (s.name == "--path" || s.name == "--installation")}
	for _, name := range requiredFlags(route) {
		if name == s.name {
			f.Required = true
		}
	}
	if s.name == "--snapshot" {
		for _, c := range commandContracts {
			if c.Path == route && c.projectSelection() {
				f.RequiredWhen = "No compatible project lock supplies the requested fixed identity"
			}
		}
	}
	if s.name == "--environment" {
		f.RequiredWhen = "Third-party source research requires a compatible pinned client API environment"
	}
	if !s.value {
		f.Type = "boolean"
		f.Default = false
		return f
	}
	bounds := func(min, max int64) { f.Type = "integer"; f.Minimum = &min; f.Maximum = &max }
	switch s.name {
	case "--format":
		f.Enum = []string{"text", "json", "jsonl"}
		f.Default = "text"
	case "--id", "--record", "--after-id", "--after-index", "--value", "--region-id", "--npc-id":
		bounds(0, 4294967295)
	case "--pid", "--file-id", "--spell-id", "--item-id", "--display-id", "--journal-encounter-id", "--file-data-id", "--model-file-data-id":
		bounds(1, 4294967295)
	case "--mipmap":
		bounds(0, 15)
	case "--max-pixels":
		bounds(1, 64<<20)
	case "--budget-seconds":
		bounds(1, 120)
	case "--wait-seconds":
		bounds(1, 600)
		f.Default = 120
		if route == "doctor" {
			f.Default = 30
		}
	case "--timeout-seconds":
		bounds(1, 3600)
		f.Default = 300
	case "--max-bytes":
		bounds(1, 512<<20)
		f.Default = 128 << 20
		if route == "source context" {
			bounds(1, 1<<20)
			f.Default = 16 << 10
		}
		if route == "asset search" {
			f.Default = 256 << 20
		}
	case "--max-lines":
		bounds(1, 2000)
		f.Default = 120
	case "--depth":
		bounds(0, 4)
		f.Default = 1
	case "--max-frames":
		bounds(1, 1000000)
	case "--line":
		bounds(1, 9223372036854775807)
		f.Default = 1
	case "--count":
		bounds(1, 2000)
		f.Default = 80
	case "--limit":
		bounds(1, int64(dataLimitCeiling(route)))
		f.Default = 50
	case "--status":
		bounds(0, 255)
	case "--push":
		bounds(-2147483648, 2147483647)
	case "--race-id", "--gender":
		bounds(-9223372036854775808, 9223372036854775807)
	case "--max-depth", "--page", "--max-objects":
		bounds(0, 9223372036854775807)
	case "--max-requests":
		bounds(1, 9223372036854775807)
	case "--max-pages":
		bounds(1, 2048)
	case "--target-bytes", "--cache-max-bytes":
		bounds(0, vault.MaxCacheMaxBytes)
	case "--download-workers":
		bounds(0, vault.MaxDownloadWorkers)
	case "--table-hash":
		f.Pattern = "^[0-9a-fA-F]{8}$"
	case "--mode":
		f.Enum = []string{"precise", "exploratory"}
	case "--topic":
		f.Enum = []string{"api", "lua", "xml", "toc", "asset"}
	case "--direction":
		f.Enum = []string{"incoming", "outgoing", "both"}
	case "--policy":
		f.Enum = []string{"opaque", "observation"}
		f.Default = "opaque"
	case "--content-variant":
		f.Enum = []string{"standard", "low-violence"}
	case "--source":
		if route == "data hotfix" {
			f.Enum = []string{"wago", "dbcache", "raidbots"}
		}
	case "--encoding":
		if route == "data sql" {
			f.Enum = []string{"csv"}
		} else if route == "data hotfix" {
			f.Enum = []string{"csv", "raw", "png", "webp"}
		} else {
			f.Enum = []string{"raw", "png", "webp"}
		}
	}
	return f
}

func resultEnvelopeSchema() map[string]any {
	return map[string]any{"schema": "lycheedev.result.v1", "required": []string{"schema", "ok", "operationId", "context", "result", "captures", "warnings", "error"}, "properties": map[string]any{"schema": map[string]any{"const": "lycheedev.result.v1"}, "ok": map[string]any{"type": "boolean"}, "operationId": map[string]any{"type": "string"}, "context": map[string]any{"type": "object"}, "result": map[string]any{"description": "Command-specific business result. Source coverage, truncated output, pending external state and pending cleanup remain separate facts."}, "captures": map[string]any{"type": "array"}, "warnings": map[string]any{"type": "array", "items": map[string]any{"type": "string"}}, "error": map[string]any{"type": []string{"object", "null"}, "properties": map[string]any{"code": map[string]any{"type": "string"}, "message": map[string]any{"type": "string"}, "stage": map[string]any{"type": "string"}, "retryable": map[string]any{"type": "boolean"}}}}, "exitCodes": map[string]string{"0": "complete", "2": "arguments or selection", "3": "capability or environment", "4": "invalid input or protocol", "5": "external or business failure", "6": "pending", "7": "cancelled", "8": "internal"}, "jsonl": "Only a valid end frame and exit 0 establish successful completion."}
}

func discoverCommands(question string) map[string]any {
	normalized := strings.ToLower(question)
	aliases := map[string]string{"无限服": "forever target", "国服": "cn target", "正式服": "retail target", "怀旧": "classic target", "法术": "spell data", "技能": "spell data", "物品": "item data", "源码": "source", "调用": "source refs", "报错": "doctor source", "恢复": "live resume doctor", "安装": "addon install", "备份": "publish backup evidence", "数据库": "data sql", "查询": "query", "发布": "publish", "网易": "publish dd", "黑盒": "publish blackbox", "牛盒": "publish newbee", "作者": "publish", "图标": "asset", "贴图": "asset", "运行": "live execute", "热修": "data hotfix"}
	for key, value := range aliases {
		if strings.Contains(normalized, key) {
			normalized += " " + value
		}
	}
	type candidate struct {
		definition Definition
		score      int
	}
	var matches []candidate
	for _, c := range commandContracts {
		if c.Path == "discover" || c.Path == "schema" || c.Path == "describe" {
			continue
		}
		score := 0
		for _, term := range strings.Fields(normalized) {
			if strings.Contains(c.Path, term) {
				score += 5
			} else if strings.Contains(strings.ToLower(c.Summary), term) {
				score++
			}
		}
		if score > 0 {
			matches = append(matches, candidate{c.publicDefinition(), score})
		}
	}
	sort.SliceStable(matches, func(i, j int) bool { return matches[i].score > matches[j].score })
	if len(matches) > 8 {
		matches = matches[:8]
	}
	commands := []Definition{}
	for _, match := range matches {
		commands = append(commands, match.definition)
	}
	return map[string]any{"query": question, "commands": commands, "next": "lycheedev schema <command...>", "products": selection.DataProducts()}
}
