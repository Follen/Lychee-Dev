package command

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"reflect"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/records"
)

// Dispatch bodies for the pinned-data verbs. Every verb reads one fixed
// snapshot through the shared --installation/--cdn source selection, populates
// the standard result envelope, and reports module honesty flags (truncated or
// partial coverage) as envelope warnings.

// requireDataSource enforces the shared pinned-data source selection.
func requireDataSource(route string, opts Options) error {
	if opts.snapshot == "" || !opts.cdn && opts.installation == "" {
		return fmt.Errorf("%s requires --snapshot, and either --installation or --cdn", route)
	}
	return nil
}

// runDataVerb dispatches the db2 metadata verbs and the domain navigations.
// It returns the exit code and the error that the standard envelope rendering
// should carry.
func runDataVerb(ctx context.Context, route string, argument string, opts Options, response *Envelope) (int, error) {
	if err := requireDataSource(route, opts); err != nil {
		return 2, err
	}
	root, err := workspaceRoot(opts.home)
	if err != nil {
		return 0, err
	}
	query := opts.fileQuery()
	switch route {
	case "data db2 schema":
		if argument == "" {
			return 2, errors.New("data db2 schema requires a table argument")
		}
		reading, err := records.InspectDataSchema(ctx, root, opts.snapshot, argument, query)
		if err != nil {
			return 0, err
		}
		return commitDataVerb(response, opts, reading.Result, reading.Captures)
	case "data db2 search":
		if argument == "" {
			return 2, errors.New("data db2 search requires a table argument")
		}
		if opts.field == "" || opts.queryText == "" {
			return 2, errors.New("data db2 search requires --field and --query")
		}
		if !opts.limitSet {
			return 2, errors.New("data db2 search requires --limit")
		}
		reading, err := records.InspectDataSearch(ctx, root, opts.snapshot, argument, query, records.DB2SearchRequest{Field: opts.field, Query: opts.queryText, Limit: opts.limit})
		if err != nil {
			return 0, err
		}
		return commitDataVerb(response, opts, reading.Result, reading.Captures)
	case "data db2 foreign-key":
		if argument == "" {
			return 2, errors.New("data db2 foreign-key requires a table argument")
		}
		if opts.field == "" || !opts.value.set {
			return 2, errors.New("data db2 foreign-key requires --field and --value")
		}
		if !opts.limitSet {
			return 2, errors.New("data db2 foreign-key requires --limit")
		}
		reading, err := records.InspectDataForeignKey(ctx, root, opts.snapshot, argument, query, records.DB2ForeignKeyRequest{Field: opts.field, Value: opts.value.value, Limit: opts.limit})
		if err != nil {
			return 0, err
		}
		return commitDataVerb(response, opts, reading.Result, reading.Captures)
	case "data spell info":
		if opts.spellID == 0 {
			return 2, errors.New("data spell info requires --spell-id")
		}
		reading, err := records.InspectSpellInfo(ctx, root, opts.snapshot, query, records.SpellInfoRequest{SpellID: opts.spellID, MaxDepth: opts.maxDepth})
		if err != nil {
			return 0, err
		}
		return commitDataVerb(response, opts, reading.Result, reading.Captures)
	case "data spell auras":
		if opts.spellID == 0 {
			return 2, errors.New("data spell auras requires --spell-id")
		}
		reading, err := records.InspectSpellAuras(ctx, root, opts.snapshot, query, records.SpellAurasRequest{SpellID: opts.spellID})
		if err != nil {
			return 0, err
		}
		return commitDataVerb(response, opts, reading.Result, reading.Captures)
	case "data spell summons":
		if opts.spellID == 0 {
			return 2, errors.New("data spell summons requires --spell-id")
		}
		reading, err := records.InspectSpellSummons(ctx, root, opts.snapshot, query, records.SpellSummonsRequest{SpellID: opts.spellID, NPCID: opts.npcID})
		if err != nil {
			return 0, err
		}
		return commitDataVerb(response, opts, reading.Result, reading.Captures)
	case "data item get":
		if !opts.itemIDSet {
			return 2, errors.New("data item get requires --item-id")
		}
		reading, err := records.InspectItem(ctx, root, opts.snapshot, query, records.ItemGetRequest{ItemID: opts.itemID})
		if err != nil {
			return 0, err
		}
		return commitDataVerb(response, opts, reading.Result, reading.Captures)
	case "data item models":
		if !opts.itemIDSet {
			return 2, errors.New("data item models requires --item-id")
		}
		reading, err := records.InspectItemModels(ctx, root, opts.snapshot, query, records.ItemModelsRequest{ItemID: opts.itemID, RaceID: opts.raceID, Gender: opts.gender})
		if err != nil {
			return 0, err
		}
		return commitDataVerb(response, opts, reading.Result, reading.Captures)
	case "data item geosets":
		if !opts.itemIDSet {
			return 2, errors.New("data item geosets requires --item-id")
		}
		reading, err := records.InspectItemGeosets(ctx, root, opts.snapshot, query, records.ItemGeosetRequest{ItemID: opts.itemID})
		if err != nil {
			return 0, err
		}
		return commitDataVerb(response, opts, reading.Result, reading.Captures)
	case "data item textures":
		if !opts.itemIDSet {
			return 2, errors.New("data item textures requires --item-id")
		}
		reading, err := records.InspectItemTextures(ctx, root, opts.snapshot, query, records.ItemTexturesRequest{ItemID: opts.itemID})
		if err != nil {
			return 0, err
		}
		return commitDataVerb(response, opts, reading.Result, reading.Captures)
	case "data creature display":
		reading, err := records.InspectCreatureDisplay(ctx, root, opts.snapshot, query, records.CreatureDisplayRequest{DisplayID: opts.displayID, FileDataID: opts.fileDataID.value})
		if err != nil {
			return 0, err
		}
		return commitDataVerb(response, opts, reading.Result, reading.Captures)
	case "data creature model":
		if !opts.fileDataID.set {
			return 2, errors.New("data creature model requires --file-data-id")
		}
		reading, err := records.InspectCreatureModel(ctx, root, opts.snapshot, query, records.CreatureModelRequest{FileDataID: opts.fileDataID.value})
		if err != nil {
			return 0, err
		}
		return commitDataVerb(response, opts, reading.Result, reading.Captures)
	case "data encounter get":
		if opts.journalEncounterID == 0 {
			return 2, errors.New("data encounter get requires --journal-encounter-id")
		}
		reading, err := records.InspectEncounter(ctx, root, opts.snapshot, query, records.EncounterGetRequest{JournalEncounterID: opts.journalEncounterID, MaxDepth: opts.maxDepth})
		if err != nil {
			return 0, err
		}
		return commitDataVerb(response, opts, reading.Result, reading.Captures)
	case "data decor list":
		if !opts.limitSet {
			return 2, errors.New("data decor list requires --limit")
		}
		reading, err := records.InspectDecorList(ctx, root, opts.snapshot, query, records.DecorListRequest{Limit: opts.limit})
		if err != nil {
			return 0, err
		}
		return commitDataVerb(response, opts, reading.Result, reading.Captures)
	case "data decor get":
		var request records.DecorGetRequest
		if opts.recordIDSet {
			id := opts.recordID
			request.ID = &id
		}
		if opts.itemIDSet {
			id := opts.itemID
			request.ItemID = &id
		}
		if opts.modelFileDataID.set {
			id := opts.modelFileDataID.value
			request.ModelFileDataID = &id
		}
		reading, err := records.InspectDecorGet(ctx, root, opts.snapshot, query, request)
		if err != nil {
			return 0, err
		}
		return commitDataVerb(response, opts, reading.Result, reading.Captures)
	}
	return 2, fmt.Errorf("unknown command %q; use --help", route)
}

// commitDataVerb fills the envelope from one data reading and surfaces the
// module's honesty flags as warnings.
func commitDataVerb(response *Envelope, opts Options, result any, captures []evidence.CaptureRef) (int, error) {
	response.Result = result
	response.Context["snapshot"] = opts.snapshot
	for _, capture := range captures {
		response.Captures = append(response.Captures, capture)
	}
	if truncated, partial, ok := dataHonesty(result); ok {
		if truncated {
			response.Warnings = append(response.Warnings, "This result is truncated; preserve the snapshot and the stated truncation reason when continuing.")
		}
		if partial {
			response.Warnings = append(response.Warnings, "This result is only partially covered; inspect partialReasons for the unavailable tables and unresolved relations.")
		}
	}
	return 0, nil
}

// dataHonesty extracts the honesty flags every data result carries through its
// embedded DataContext, without enumerating each result type.
func dataHonesty(result any) (truncated, partial, ok bool) {
	value := reflect.ValueOf(result)
	if value.Kind() != reflect.Struct {
		return false, false, false
	}
	context := value.FieldByName("DataContext")
	if !context.IsValid() || context.Kind() != reflect.Struct {
		return false, false, false
	}
	flag := func(name string) bool {
		field := context.FieldByName(name)
		return field.IsValid() && field.Kind() == reflect.Bool && field.Bool()
	}
	return flag("Truncated"), flag("Partial"), true
}

// runDataStream streams one bounded table as typed JSONL frames. It returns
// handled=true when the stream already wrote everything stdout should carry:
// a legal end frame plus exit 0 on success, or the module's error frame and no
// end frame after a mid-stream failure.
func runDataStream(ctx context.Context, argument string, opts Options, stdout io.Writer) (bool, int, error) {
	if opts.format != "jsonl" {
		return false, 2, errors.New("data db2 stream requires --format jsonl")
	}
	if err := requireDataSource("data db2 stream", opts); err != nil {
		return false, 2, err
	}
	if argument == "" {
		return false, 2, errors.New("data db2 stream requires a table argument")
	}
	if !opts.limitSet {
		return false, 2, errors.New("data db2 stream requires --limit")
	}
	root, err := workspaceRoot(opts.home)
	if err != nil {
		return false, 0, err
	}
	encoder := json.NewEncoder(stdout)
	encoder.SetEscapeHTML(false)
	yielded := false
	summary, err := records.StreamDataRows(ctx, root, opts.snapshot, argument, opts.fileQuery(), records.DB2StreamRequest{Fields: opts.fieldList, Filter: opts.filter, Limit: opts.limit}, func(frame records.StreamFrame) error {
		yielded = true
		return encoder.Encode(frame)
	})
	if err != nil {
		if yielded {
			// Frames already reached stdout; the module ended the stream with
			// an error frame and no envelope may follow the partial JSONL.
			return true, streamFaultExit(err), nil
		}
		return false, 0, err
	}
	_ = summary
	return true, 0, nil
}

// streamFaultExit maps a failed stream onto the documented exit codes without
// re-rendering an envelope over emitted frames.
func streamFaultExit(err error) int {
	if errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
		return 7
	}
	if exit, _, ok := queryFault(err); ok {
		return exit
	}
	return 5
}

// runDataHotfix dispatches the explicit-source hotfix query. handled=true
// means stdout already carries the complete typed JSONL stream and Execute
// must return the given code immediately.
func runDataHotfix(ctx context.Context, opts Options, response *Envelope, stdout, stderr io.Writer) (bool, int, error) {
	switch opts.source {
	case "":
		return false, 2, errors.New("data hotfix requires --source")
	case "dbcache":
		return runDataHotfixDBCache(ctx, opts, response)
	case "wago":
		return runDataHotfixRemote(ctx, opts, response, stdout, stderr, false)
	case "raidbots":
		return runDataHotfixRemote(ctx, opts, response, stdout, stderr, true)
	}
	return false, 2, errors.New("--source must be wago, dbcache, or raidbots")
}

// runDataHotfixDBCache keeps the pinned local-cache flow: an explicit snapshot
// plus either a freshly read DBCache.bin or an archived cache capture.
func runDataHotfixDBCache(ctx context.Context, opts Options, response *Envelope) (bool, int, error) {
	if opts.snapshot == "" {
		return false, 2, errors.New("data hotfix --source dbcache requires --snapshot")
	}
	if (opts.dbcache == "") == (opts.from == "") {
		return false, 2, errors.New("data hotfix --source dbcache requires exactly one of --dbcache or --from")
	}
	for flag, present := range map[string]bool{
		"--raidbots":     opts.raidbots != "",
		"--product":      opts.product != "",
		"--build":        opts.fullBuild != "",
		"--region":       opts.dataRegion != "",
		"--locale":       opts.locale != "",
		"--search":       opts.search != "",
		"--to":           opts.to != "",
		"--cursor":       opts.cursor != "",
		"--page":         opts.page != 0,
		"--max-pages":    opts.maxPages != 0,
		"--max-requests": opts.maxRequests != 0,
		"--encoding csv": opts.encoding == "csv",
	} {
		if present {
			return false, 2, fmt.Errorf("%s is not supported by the dbcache hotfix source", flag)
		}
	}
	request := records.HotfixRequest{File: opts.dbcache, From: opts.from, Table: opts.table, Offline: opts.offline, MaxBytes: opts.maxBytes, Filter: records.CacheFilter{TableHash: opts.tableHash, AfterIndex: opts.afterIndex, Limit: opts.limit, Latest: opts.latest}}
	if opts.recordIDSet {
		request.Filter.RecordID = &opts.recordID
	}
	if opts.pushSet {
		push := int32(opts.push)
		request.Filter.Push = &push
	}
	if opts.statusSet {
		status := uint8(opts.status)
		request.Filter.Status = &status
	}
	if opts.regionIDSet {
		request.Filter.Region = &opts.regionID
	}
	if err := request.Validate(); err != nil {
		return false, 2, err
	}
	root, err := workspaceRoot(opts.home)
	if err != nil {
		return false, 0, err
	}
	reading, err := records.InspectHotfix(ctx, root, opts.snapshot, request)
	if err != nil {
		return false, 0, err
	}
	response.Result = reading.Result
	response.Context["snapshot"] = reading.Result.Snapshot
	response.Captures = append(response.Captures, reading.Result.Source, reading.Capture)
	response.Warnings = append(response.Warnings, "Independent local-cache records in physical order, not a DB2 overlay. Complete describes the selected cache query, not server coverage; product, region and locale are selected context, not authenticated by this file. Named tables use the pinned build definition; records without valid payloads retain raw metadata without fields.")
	return false, 0, nil
}

// runDataHotfixRemote runs the wago remote query or the raidbots snapshot
// query. Both carry the shared HotfixQuery filters and produce the unified
// RemoteHotfixResult, so both support the CSV export encoding and the typed
// JSONL record stream.
func runDataHotfixRemote(ctx context.Context, opts Options, response *Envelope, stdout, stderr io.Writer, raidbots bool) (bool, int, error) {
	source := "wago"
	if raidbots {
		source = "raidbots"
	}
	if raidbots && opts.raidbots == "" {
		return false, 2, errors.New("data hotfix --source raidbots requires --raidbots")
	}
	if !raidbots && opts.raidbots != "" {
		return false, 2, errors.New("--raidbots is not supported by the wago hotfix source")
	}
	if opts.dbcache != "" {
		return false, 2, errors.New("--dbcache is not supported by the " + source + " hotfix source")
	}
	if opts.product == "" || opts.fullBuild == "" || opts.dataRegion == "" || opts.locale == "" {
		return false, 2, errors.New("data hotfix --source " + source + " requires --product, --build, --region, and --locale")
	}
	if opts.afterIndex != nil {
		return false, 2, errors.New("data hotfix --source " + source + " pages with --cursor, not --after-index")
	}
	query := records.HotfixQuery{Product: opts.product, FullBuild: opts.fullBuild, Region: opts.dataRegion, Locale: opts.locale, Table: opts.table, Search: opts.search, Latest: opts.latest, Page: opts.page, Cursor: opts.cursor, Limit: opts.limit}
	if opts.tableHash != nil {
		query.TableHash = opts.tableHash
	}
	if opts.recordIDSet {
		query.RecordID = &opts.recordID
	}
	if opts.pushSet {
		push := int32(opts.push)
		query.PushID = &push
	}
	if opts.regionIDSet {
		query.RegionID = &opts.regionID
	}
	if opts.statusSet {
		status := uint8(opts.status)
		query.Status = &status
	}
	if opts.from != "" {
		parsed, err := records.ParseHotfixTime(opts.from)
		if err != nil {
			return false, 2, err
		}
		query.From = &parsed
	}
	if opts.to != "" {
		parsed, err := records.ParseHotfixTime(opts.to)
		if err != nil {
			return false, 2, err
		}
		query.To = &parsed
	}
	if err := query.Validate(); err != nil {
		return false, 2, err
	}
	root, err := workspaceRoot(opts.home)
	if err != nil {
		return false, 0, err
	}
	var result records.RemoteHotfixResult
	if raidbots {
		snapshot, err := records.RaidbotsSnapshot(opts.raidbots, query)
		if err != nil {
			return false, 2, err
		}
		result, err = snapshot.QueryHotfix(ctx, query)
		if err != nil {
			return false, 0, err
		}
		pinned, err := records.PinHotfixChange(ctx, root, opts.snapshot, result.Change)
		if err != nil {
			return false, 0, err
		}
		result.Snapshot = pinned.ID
	} else {
		result, err = records.QueryWagoHotfix(ctx, root, records.WagoHotfixRequest{
			Query:    query,
			Offline:  opts.offline,
			MaxPages: opts.maxPages,
			Budget:   records.HotfixFetchBudget{MaxRequests: opts.maxRequests, MaxBytes: opts.maxBytes},
			Parent:   opts.snapshot,
		})
		if err != nil {
			return false, 0, err
		}
	}
	if opts.encoding == "csv" {
		if opts.output == "" {
			return false, 2, errors.New("data hotfix --encoding csv requires --output")
		}
		manifest, err := writeHotfixCSV(opts.output, result)
		if err != nil {
			return false, 0, err
		}
		response.Result = manifest
	} else {
		response.Result = result
	}
	response.Context["snapshot"] = result.Snapshot
	if result.Capture != nil {
		response.Captures = append(response.Captures, *result.Capture)
	}
	response.Warnings = append(response.Warnings, result.Warnings...)
	if result.NextCursor != "" {
		response.Warnings = append(response.Warnings, "This page is not the complete record set; preserve the cursor and query scope when continuing.")
	}
	if opts.format == "jsonl" && opts.encoding != "csv" {
		if err := records.WriteHotfixJSONL(stdout, result); err != nil {
			fmt.Fprintln(stderr, "command.output_failed:", err)
			return true, 5, nil
		}
		return true, 0, nil
	}
	return false, 0, nil
}

// writeHotfixCSV encodes the unified result as RFC 4180 CSV at the requested
// location and publishes its machine-readable manifest beside it.
func writeHotfixCSV(output string, result records.RemoteHotfixResult) (records.CSVExportManifest, error) {
	var manifest records.CSVExportManifest
	var buffer bytes.Buffer
	encoded, err := records.EncodeHotfixCSV(&buffer, result)
	if err != nil {
		return manifest, err
	}
	if err := os.WriteFile(output, buffer.Bytes(), 0600); err != nil {
		return manifest, err
	}
	sidecar, err := os.OpenFile(output+".manifest.json", os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0600)
	if err != nil {
		return manifest, err
	}
	if err := records.WriteCSVManifest(sidecar, encoded); err != nil {
		sidecar.Close()
		return manifest, err
	}
	return encoded, sidecar.Close()
}
