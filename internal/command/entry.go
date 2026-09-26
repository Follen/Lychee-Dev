// Package command contains only command parsing, dispatch and rendering.
package command

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/codebase"
	"github.com/follenfang/lycheedev/internal/delivery"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/records/relational"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
	"image"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"
)

const Version = buildinfo.Version

type Fault struct {
	Code              string         `json:"code"`
	Message           string         `json:"message"`
	Stage             string         `json:"stage"`
	Retryable         bool           `json:"retryable"`
	ResumeOperationID string         `json:"resumeOperationId"`
	Location          *QueryLocation `json:"location,omitempty"`
}

type Envelope struct {
	Schema      string         `json:"schema"`
	OK          bool           `json:"ok"`
	OperationID string         `json:"operationId"`
	Context     map[string]any `json:"context"`
	Result      any            `json:"result"`
	Captures    []any          `json:"captures"`
	Warnings    []string       `json:"warnings"`
	Error       *Fault         `json:"error"`
}

type Options struct {
	project                              string
	target                               string
	latest                               bool
	tableHash, afterIndex                *uint32
	dataRegion, locale, definitionRef    string
	session                              string
	account                              string
	probe, request, name                 string
	includeRemoved                       bool
	pid                                  uint32
	character, realm                     string
	region                               image.Rectangle
	release                              string
	output                               string
	overwrite                            bool
	encoding, channels                   string
	mipmap                               int
	maxPixels                            int64
	resume                               bool
	afterID                              *uint32
	table                                string
	recordID                             uint32
	recordIDSet, offline                 bool
	cdn                                  bool
	installation                         string
	fileID                               uint32
	maxBytes                             int64
	home, format, file                   string
	source, product, ref, snapshot, path string
	fullBuild                            string
	from, to                             string
	toc                                  string
	matrix, searchMode, topic, listfile  string
	symbol, targetPath                   string
	extension, fileName                  string
	maxFrames                            int
	allowPartial                         bool
	stdin                                bool
	sql                                  string
	parameters                           map[string]any
	ids                                  string
	line, count, limit                   int
	limitSet                             bool
	spellID, itemID, npcID               uint32
	itemIDSet                            bool
	displayID                            uint32
	fileDataID                           uint32Set
	journalEncounterID                   uint32
	modelFileDataID                      uint32Set
	raceID, gender, maxDepth             int
	field, queryText, filter             string
	value                                uint32Set
	fieldList                            []string
	dbcache, raidbots, cursor, search    string
	push                                 int64
	pushSet                              bool
	status                               int
	statusSet                            bool
	regionID                             uint32
	regionIDSet                          bool
	page, maxPages, maxRequests          int
	remote, replace                      bool
	uncommitted, dryRun                  bool
	plan, fresh                          bool
	targetBytes                          int64
	maxObjects                           int
	cacheMaxBytes                        int64
	cacheMaxBytesSet                     bool
	downloadWorkers                      int
	downloadWorkersSet                   bool
	words                                []string
	help                                 bool
}

// uint32Set pairs an optional uint32 flag with its presence, so a zero value
// stays distinguishable from an unset key.
type uint32Set struct {
	value uint32
	set   bool
}

// Execute returns the process exit code. It never exits the embedding process.
func Execute(ctx context.Context, args []string, stdout, stderr io.Writer) int {
	opts, parseErr := parseOptions(args)
	response := Envelope{Schema: "lycheedev.result.v1", Context: map[string]any{}, Captures: []any{}, Warnings: []string{}}
	var err error
	code := 0
	if parseErr != nil {
		err, code = parseErr, 2
	} else if opts.help {
		if len(opts.words) == 0 {
			response.Result = map[string]any{"commands": definitions, "flags": []string{"--home <root>", "--format text|json|jsonl", "--help"}}
		} else if contract, _, contractErr := findCommandContract(opts.words); contractErr != nil {
			err, code = contractErr, 2
		} else {
			definition := contract.publicDefinition()
			response.Result = map[string]any{"commands": []Definition{definition}, "flags": append(definition.Flags, "--help")}
		}
	} else {
		contract, route, _ := findCommandContract(opts.words)
		argument := ""
		if contract.positional != "" && len(opts.words) > len(strings.Fields(route)) {
			argument = opts.words[len(opts.words)-1]
		}
		if contract.projectSelection() && opts.snapshot == "" && route != "source list" && !(route == "source validate" && opts.matrix != "") && !(route == "asset demux" && opts.path != "") {
			var project selection.ProjectStatus
			project, err = selectProject(ctx, &opts)
			if err == nil && project.Lock != nil {
				response.Context["project"], response.Context["snapshot"] = project.Directory, project.Lock.Selection.ID
			}
		}
		if err == nil {
			switch route {
			case "project init":
				if opts.product == "" {
					err, code = errors.New("project init requires --product"), 2
					break
				}
				response.Result, err = selection.InitializeProject(ctx, opts.path, opts.product)
			case "project status":
				var dir string
				dir, err = projectDirectory(opts.path)
				if err == nil {
					response.Result, err = selection.InspectProject(ctx, dir)
				}
			case "project lock":
				if opts.snapshot == "" {
					err, code = errors.New("project lock requires --snapshot"), 2
					break
				}
				var root string
				root, err = workspaceRoot(opts.home)
				if err != nil {
					break
				}
				var dir string
				dir, err = projectDirectory(opts.path)
				if err == nil {
					response.Result, err = selection.LockProject(ctx, root, dir, opts.snapshot)
				}
				if err == nil {
					response.Context["snapshot"] = opts.snapshot
				}
			case "live probe put":
				response.Result, err, code = putLiveProbe(ctx, opts)
			case "live probe load":
				var root string
				root, err = workspaceRoot(opts.home)
				var record live.Outcome
				if err == nil {
					record, err = live.LoadProbe(ctx, root, live.LoadProbeRequest{Session: opts.session, Account: opts.account, Probe: opts.probe, Request: opts.request})
				}
				if record.OperationID != "" {
					response.Result, response.OperationID = record, record.OperationID
					response.Context["stage"], response.Context["snapshot"] = record.Stage, record.Snapshot
				}
			case "live probe list":
				var root string
				root, err = workspaceRoot(opts.home)
				if err == nil {
					response.Result, err = live.ListProbes(ctx, root, opts.includeRemoved, opts.limit)
				}
			case "live probe show":
				var root string
				root, err = workspaceRoot(opts.home)
				if err == nil {
					response.Result, err = live.ShowProbe(ctx, root, argument)
				}
			case "live probe remove":
				var root string
				root, err = workspaceRoot(opts.home)
				if err == nil {
					response.Result, err = live.RemoveProbe(ctx, root, argument)
				}
			case "live run":
				var record live.Outcome
				var root string
				root, err = workspaceRoot(opts.home)
				if err == nil {
					record, err = live.RunLoaded(ctx, root, argument)
				}
				if record.OperationID != "" {
					response.Result, response.OperationID = record, record.OperationID
					response.Context["stage"], response.Context["snapshot"] = record.Stage, record.Snapshot
				}
			case "live reload":
				var root string
				root, err = workspaceRoot(opts.home)
				var record live.Outcome
				if err == nil {
					record, err = live.ReloadClient(ctx, root, live.ReloadRequest{Session: opts.session, Request: opts.request})
				}
				if record.OperationID != "" {
					response.Result, response.OperationID = record, record.OperationID
					response.Context["stage"], response.Context["snapshot"] = record.Stage, record.Snapshot
				}
			case "live ack":
				var root string
				root, err = workspaceRoot(opts.home)
				var record live.Outcome
				if err == nil {
					record, err = live.AcknowledgeVerified(ctx, root, argument)
				}
				if record.OperationID != "" {
					response.Result, response.OperationID = record, record.OperationID
					response.Context["stage"], response.Context["snapshot"] = record.Stage, record.Snapshot
				}
			case "live finish":
				var root string
				root, err = workspaceRoot(opts.home)
				var record live.FinishOutcome
				if err == nil {
					record, err = live.FinishProbe(ctx, root, argument)
				}
				if record.OperationID != "" {
					response.Result, response.OperationID = record, record.OperationID
					response.Context["stage"], response.Context["snapshot"] = record.Stage, record.Snapshot
				}
			case "live bugs":
				var root string
				root, err = workspaceRoot(opts.home)
				var record live.Outcome
				if err == nil {
					record, err = live.Bugs(ctx, root, live.BugsRequest{Session: opts.session, Account: opts.account, Request: opts.request, Count: opts.count})
				}
				if record.OperationID != "" {
					response.Result, response.OperationID = record, record.OperationID
					response.Context["stage"], response.Context["snapshot"] = record.Stage, record.Snapshot
				}
			case "live hide":
				var root string
				root, err = workspaceRoot(opts.home)
				var dismissal live.HideReceiptResult
				if err == nil {
					dismissal, err = live.HideReceipt(ctx, root, opts.session)
				}
				if err == nil {
					response.Result = dismissal
					response.Context["session"] = dismissal.Session
				}
			case "live connect":
				request := live.ConnectRequest{Snapshot: opts.snapshot, Character: opts.character, Realm: opts.realm, PID: opts.pid, Installation: opts.installation, Session: opts.session, CaptureArea: opts.region}
				if err = request.Validate(); err != nil {
					code = 2
					break
				}
				var root string
				root, err = workspaceRoot(opts.home)
				if err != nil {
					break
				}
				var connection live.Connection
				connection, err = live.ConnectWindow(ctx, root, request)
				if err == nil {
					response.Result = connection
					response.Context["session"], response.Context["snapshot"] = connection.ID, connection.Snapshot
					response.Warnings = append(response.Warnings, "Connected through the two fixed bootstrap commands only; identity markers bound no session. The retained identity is a reconnection target, not standing input authority.")
				}
			case "live reset":
				request := live.ResetRequest{Snapshot: opts.snapshot, Character: opts.character, Realm: opts.realm, PID: opts.pid, Installation: opts.installation}
				if err = request.Validate(); err != nil {
					code = 2
					break
				}
				var root string
				root, err = workspaceRoot(opts.home)
				if err != nil {
					break
				}
				var outcome live.ResetOutcome
				outcome, err = live.ResetWindow(ctx, root, request)
				if err == nil {
					response.Result = outcome
					response.Context["session"], response.Context["snapshot"] = outcome.Connection.ID, outcome.Connection.Snapshot
					response.Warnings = append(response.Warnings, "Reset sent one fixed nonce-correlated recovery trigger to an unowned window and reconnected through the normal bootstrap; retained reports were never acknowledged or deleted.")
				}
			case "live bind":
				request := live.WindowBindingRequest{Installation: opts.installation, PID: opts.pid, Snapshot: opts.snapshot, Character: opts.character, Realm: opts.realm, Region: opts.region}
				if err = request.Validate(); err != nil {
					code = 2
					break
				}
				var root string
				root, err = workspaceRoot(opts.home)
				if err != nil {
					break
				}
				var bound live.RecordedSession
				bound, err = live.BindWindowSession(ctx, root, request)
				if err == nil {
					response.Result = bound.Connection()
					response.Context["session"], response.Context["snapshot"] = bound.Record.ID, bound.Record.Snapshot
					response.Warnings = append(response.Warnings, "Ready receipt observed and saved; capture stream is closed. This does not enable the addon, send input, or authorize later input from history.")
				}
			case "addon install":
				if opts.installation == "" || (!opts.resume && opts.release == "") || (opts.resume && (opts.output == "" || opts.release != "")) {
					err, code = errors.New("addon install requires --installation and --release, or --resume with --output and no --release"), 2
					break
				}
				if opts.output != "" {
					response.Result, err = delivery.UpgradeAddon(ctx, opts.release, opts.installation, opts.output, Version, opts.resume)
				} else {
					response.Result, err = delivery.InstallAddon(ctx, opts.release, opts.installation, Version)
				}
			case "addon remove":
				if opts.installation == "" || opts.output == "" {
					err, code = errors.New("addon remove requires --installation and --output"), 2
					break
				}
				response.Result, err = delivery.RemoveAddon(ctx, opts.installation, opts.output)
			case "skill install":
				if opts.path == "" || (!opts.resume && opts.release == "") || (opts.resume && (opts.output == "" || opts.release != "")) {
					err, code = errors.New("skill install requires --path and --release, or --resume with --output and no --release"), 2
					break
				}
				if opts.output != "" {
					response.Result, err = delivery.UpgradeSkill(ctx, opts.release, opts.path, opts.output, Version, opts.resume)
				} else {
					response.Result, err = delivery.InstallSkill(ctx, opts.release, opts.path, Version)
				}
			case "skill remove":
				if opts.path == "" || opts.output == "" {
					err, code = errors.New("skill remove requires --path and --output"), 2
					break
				}
				response.Result, err = delivery.RemoveSkill(ctx, opts.path, opts.output)
			case "skill status", "addon status":
				if route == "addon status" && opts.installation != "" {
					response.Result, err = delivery.InspectAddonDeployment(ctx, opts.installation)
					break
				}
				if opts.path == "" {
					err, code = errors.New("installation status requires --path"), 2
					break
				}
				response.Result, err = delivery.InspectDeployment(ctx, opts.path, opts.words[0])
			case "data hotfix":
				handled, hotfixCode, hotfixErr := runDataHotfix(ctx, opts, &response, stdout, stderr)
				if handled {
					return hotfixCode
				}
				code, err = hotfixCode, hotfixErr
			case "data sql":
				if opts.snapshot == "" || !opts.cdn && opts.installation == "" || boolCount(opts.file != "", opts.sql != "", opts.stdin) != 1 {
					err, code = errors.New("data sql requires --snapshot, one data source, and exactly one of --sql, --file or --stdin"), 2
					break
				}
				if opts.encoding == "csv" && opts.output == "" || opts.encoding != "csv" && (opts.output != "" || opts.overwrite) {
					err, code = errors.New("data sql CSV export requires --encoding csv and --output; --overwrite is CSV-only"), 2
					break
				}
				var request records.DataQuery
				request, err = readSelectedDataQuery(opts)
				if err != nil {
					code = 2
					break
				}
				var root string
				root, err = workspaceRoot(opts.home)
				if err != nil {
					break
				}
				var reading records.QueryReading
				reading, err = records.QueryData(ctx, root, opts.snapshot, opts.fileQuery(), request)
				if err == nil {
					if opts.encoding == "csv" {
						var exported records.QueryCSVExport
						exported, err = records.ExportQueryCSV(ctx, root, opts.snapshot, reading, opts.output, opts.overwrite)
						if err != nil {
							break
						}
						response.Result = exported.Manifest
						response.Captures = append(response.Captures, reading.Capture, exported.Capture)
					} else {
						response.Result = reading.Result
						response.Captures = append(response.Captures, reading.Capture)
					}
					response.Context["snapshot"] = opts.snapshot
				}
			case "data db2":
				if opts.snapshot == "" || !opts.cdn && opts.installation == "" || opts.table == "" {
					err, code = errors.New("data db2 requires --snapshot, --table, and either --installation or --cdn"), 2
					break
				}
				var root string
				root, err = workspaceRoot(opts.home)
				if err != nil {
					break
				}
				var reading records.RecordReading
				if opts.recordIDSet {
					reading, err = records.InspectDataRecord(ctx, root, opts.snapshot, opts.table, opts.fileQuery(), opts.recordID)
				} else {
					reading, err = records.InspectDataPage(ctx, root, opts.snapshot, opts.table, opts.fileQuery(), opts.afterID, opts.limit)
				}
				if err == nil {
					response.Result = reading.Result
					response.Context["snapshot"] = opts.snapshot
					response.Captures = append(response.Captures, reading.Capture)
					if reading.Result.Page != nil && (reading.Result.Page.More || reading.Result.Page.After != nil) {
						response.Warnings = append(response.Warnings, "This page is not the complete table; preserve the snapshot, table and cursor when continuing.")
					}
				}
			case "data db2 stream":
				handled, streamCode, streamErr := runDataStream(ctx, argument, opts, stdout)
				if handled {
					return streamCode
				}
				code, err = streamCode, streamErr
			case "data db2 schema", "data db2 search", "data db2 foreign-key",
				"data spell info", "data spell auras", "data spell summons",
				"data item get", "data item models", "data item geosets", "data item textures",
				"data creature display", "data creature model", "data encounter get",
				"data decor list", "data decor get":
				code, err = runDataVerb(ctx, route, argument, opts, &response)
			case "asset search":
				if opts.snapshot == "" || opts.listfile == "" || boolCount(opts.queryText != "", opts.extension != "", opts.fileName != "", opts.fileID != 0) != 1 {
					err, code = errors.New("asset search requires --snapshot, --listfile and exactly one of --query, --extension, --name, --file-id"), 2
					break
				}
				var root string
				root, err = workspaceRoot(opts.home)
				if err != nil {
					break
				}
				listfile := records.ListfileRequest{Kind: records.ListfileKind(opts.listfile), Offline: opts.offline, Fetch: records.PublicListfileFetcher{}, Limits: records.ListfileLimits{Bytes: opts.maxBytes}}
				var truncated bool
				switch {
				case opts.queryText != "":
					var found records.FileSearchResult
					found, err = records.SearchFileNames(ctx, root, records.FileSearchRequest{Snapshot: opts.snapshot, Listfile: listfile, Query: opts.queryText, Limit: opts.limit})
					response.Result, truncated = found, found.Truncated
				case opts.extension != "":
					var found records.FileExtensionResult
					found, err = records.ListFileExtensions(ctx, root, records.FileExtensionRequest{Snapshot: opts.snapshot, Listfile: listfile, Extension: opts.extension, Limit: opts.limit})
					response.Result, truncated = found, found.Truncated
				case opts.fileName != "":
					response.Result, err = records.ResolveFileDataIDs(ctx, root, records.FileNameResolveRequest{Snapshot: opts.snapshot, Listfile: listfile, FileName: opts.fileName})
				default:
					response.Result, err = records.LookupFileNames(ctx, root, records.FileNameLookupRequest{Snapshot: opts.snapshot, Listfile: listfile, FileDataIDs: []uint32{opts.fileID}})
				}
				if err == nil {
					response.Context["snapshot"] = opts.snapshot
					if truncated {
						response.Warnings = append(response.Warnings, "File-name search was truncated by --limit.")
					}
				} else {
					response.Result = nil
				}
			case "asset demux":
				if opts.output == "" || (opts.path == "") == (opts.fileID == 0) || opts.path == "" && (opts.snapshot == "" || !opts.cdn && opts.installation == "") || opts.path != "" && (opts.snapshot != "" || opts.cdn || opts.installation != "") {
					err, code = errors.New("asset demux requires --output and exactly one input: --path, or --snapshot/--file-id with --installation or --cdn"), 2
					break
				}
				var root string
				root, err = workspaceRoot(opts.home)
				if err != nil {
					break
				}
				var demux records.AssetDemux
				demux, err = records.DemuxAsset(ctx, root, opts.snapshot, records.AssetDemuxRequest{File: opts.fileQuery(), Path: opts.path, Output: opts.output, Overwrite: opts.overwrite, MaxInputBytes: opts.maxBytes, MaxFrames: opts.maxFrames, AllowPartial: opts.allowPartial})
				if err == nil {
					response.Result = demux.Manifest
					response.Context["snapshot"] = opts.snapshot
					response.Captures = append(response.Captures, demux.Source, demux.Capture)
					for _, capture := range demux.Frames {
						response.Captures = append(response.Captures, capture)
					}
					if demux.Manifest.Truncated {
						response.Warnings = append(response.Warnings, "Demux output is truncated; inspect truncationReason and limits.")
					}
				}
			case "asset inspect", "asset export":
				if opts.snapshot == "" || !opts.cdn && opts.installation == "" || opts.fileID == 0 {
					err, code = fmt.Errorf("%s requires --snapshot, --file-id, and either --installation or --cdn", route), 2
					break
				}
				if route == "asset export" && opts.output == "" {
					err, code = errors.New("asset export requires --output"), 2
					break
				}
				var root string
				root, err = workspaceRoot(opts.home)
				if err != nil {
					break
				}
				if route == "asset export" {
					var exported records.AssetExport
					exported, err = records.ExportAsset(ctx, root, opts.snapshot, records.AssetExportRequest{File: opts.fileQuery(), Output: opts.output, Overwrite: opts.overwrite, Encoding: opts.encoding, Mipmap: opts.mipmap, Channels: opts.channels, MaxPixels: opts.maxPixels})
					if err == nil {
						response.Result = exported.Manifest
						response.Context["snapshot"] = opts.snapshot
						response.Captures = append(response.Captures, exported.Manifest.SourceCapture, exported.Capture)
						if exported.Artifact.ID != exported.Manifest.SourceCapture.ID {
							response.Captures = append(response.Captures, exported.Artifact)
						}
					}
				} else {
					var reading records.AssetReading
					reading, err = records.InspectAsset(ctx, root, opts.snapshot, opts.fileQuery())
					if err == nil {
						response.Result = reading.Result
						response.Context["snapshot"] = opts.snapshot
						response.Captures = append(response.Captures, reading.Capture)
					}
				}
			case "version":
				response.Result = buildinfo.Current()
			case "describe":
				response.Result = map[string]any{"commands": definitions, "formats": []string{"text", "json", "jsonl"}, "development": strings.Contains(Version, "-")}
			case "init":
				var root string
				root, err = workspaceRoot(opts.home)
				if err == nil {
					switch {
					case opts.plan:
						response.Result, err = vault.PlanFreshInitialize(ctx, root)
					case opts.fresh:
						response.Result, err = vault.FreshInitialize(ctx, root)
					case opts.resume:
						response.Result, err = vault.CompleteArchive(ctx, root)
					default:
						response.Result, err = vault.InitializeWorkspace(ctx, root)
					}
				}
			case "doctor":
				code, err = runDoctor(ctx, opts, &response)
			case "config show", "config set":
				code, err = runConfigVerb(ctx, route, opts, &response)
			case "cache status", "cache verify", "cache prune":
				code, err = runCacheVerb(ctx, route, opts, &response)
			case "live instances":
				var root string
				root, err = workspaceRoot(opts.home)
				if err != nil {
					break
				}
				request := live.DiscoveryRequest{}
				if opts.installation != "" {
					request.Roots = []string{opts.installation}
				}
				response.Result, err = live.DiscoverCandidates(ctx, root, request)
			case "source list":
				if opts.snapshot == "" {
					response.Result = codebase.Repositories()
				} else {
					var root string
					root, err = workspaceRoot(opts.home)
					if err == nil {
						response.Result, err = codebase.SourceSnapshotStatus(ctx, root, opts.snapshot)
						response.Context["snapshot"] = opts.snapshot
					}
				}
			case "source validate":
				if opts.matrix != "" && (opts.snapshot != "" || opts.path != "" || opts.toc != "") || opts.matrix == "" && (opts.snapshot == "" || opts.path == "" || opts.toc == "") {
					err, code = errors.New("source validate requires either --matrix or --snapshot/--path/--toc"), 2
					break
				}
				var root string
				root, err = workspaceRoot(opts.home)
				if err != nil {
					break
				}
				if opts.matrix != "" {
					var assessment codebase.MatrixValidation
					assessment, err = codebase.ValidateSourceMatrix(ctx, root, opts.matrix, nil)
					if err == nil {
						response.Result = assessment.Result
						response.Captures = append(response.Captures, assessment.Capture)
						if !assessment.Result.Valid {
							err = codebase.ErrInvalidAddon
						}
					}
				} else {
					var assessment codebase.AddonAssessment
					assessment, err = codebase.AssessAddon(ctx, root, opts.snapshot, codebase.AddonInput{Root: opts.path, Manifest: opts.toc})
					if err == nil {
						response.Result = assessment.Result
						response.Context["snapshot"] = opts.snapshot
						response.Captures = append(response.Captures, assessment.Capture)
						if !assessment.Result.StaticValid {
							err = codebase.ErrInvalidAddon
						} else if !assessment.Result.Complete {
							response.Warnings = append(response.Warnings, "Static coverage is incomplete; inspect unresolved references and notChecked.")
						}
					}
				}
			case "source diff":
				if opts.from == "" || opts.to == "" {
					err, code = errors.New("source diff requires --from and --to snapshot IDs"), 2
					break
				}
				var root string
				root, err = workspaceRoot(opts.home)
				if err != nil {
					break
				}
				var comparison codebase.SourceComparison
				comparison, err = codebase.CompareSource(ctx, root, opts.from, opts.to, opts.limit)
				if err == nil {
					response.Context["fromSnapshot"], response.Context["toSnapshot"] = opts.from, opts.to
					response.Result = comparison.Result
					response.Captures = append(response.Captures, comparison.Capture)
				}
			case "source index", "source query":
				if opts.snapshot == "" || route == "source query" && argument == "" {
					err, code = errors.New("source index/query requires --snapshot; query also requires a term"), 2
					break
				}
				var root string
				root, err = workspaceRoot(opts.home)
				if err != nil {
					break
				}
				response.Context["snapshot"] = opts.snapshot
				if route == "source index" {
					response.Result, err = codebase.BuildSourceIndex(ctx, root, opts.snapshot)
				} else {
					var search codebase.SourceQueryReading
					search, err = codebase.QuerySource(ctx, root, opts.snapshot, codebase.SearchQuery{Mode: codebase.SearchMode(opts.searchMode), Text: argument, Topic: opts.topic, Limit: opts.limit})
					if err == nil {
						response.Result = search.Result
						response.Captures = append(response.Captures, search.Capture)
						if search.Result.Truncated {
							response.Warnings = append(response.Warnings, "Source search was truncated by --limit.")
						} else if !search.Result.Complete {
							response.Warnings = append(response.Warnings, "The source index has parse diagnostics; search coverage is incomplete.")
						}
					}
				}
			case "source sync", "source inspect":
				if route == "source sync" && (opts.source == "" || opts.product == "") || route == "source inspect" && (opts.snapshot == "" || boolCount(opts.path != "", opts.symbol != "", opts.targetPath != "") != 1) {
					err, code = errors.New("source sync requires --source/--product; source inspect requires --snapshot and exactly one of --path, --symbol, --target-path"), 2
					break
				}
				var root string
				root, err = workspaceRoot(opts.home)
				if err != nil {
					break
				}
				if route == "source sync" {
					response.Result, err = codebase.SynchronizeSource(ctx, root, opts.source, opts.product, opts.ref)
				} else if opts.symbol != "" || opts.targetPath != "" {
					var reading codebase.TargetReading
					reading, err = codebase.InspectSourceTarget(ctx, root, opts.snapshot, codebase.TargetQuery{Symbol: opts.symbol, Path: opts.targetPath})
					if err == nil {
						response.Result = reading.Result
						response.Context["snapshot"] = opts.snapshot
						response.Captures = append(response.Captures, reading.Capture)
					}
				} else {
					var reading codebase.SourceReading
					reading, err = codebase.InspectSource(ctx, root, opts.snapshot, codebase.SpanQuery{Path: opts.path, FirstLine: opts.line, LineCount: opts.count})
					if err == nil {
						response.Result = reading.Excerpt
						response.Context["snapshot"] = opts.snapshot
						response.Captures = append(response.Captures, reading.Capture)
					}
				}
			case "target resolve":
				request := records.LocalTargetRequest{Installation: opts.installation, Region: opts.dataRegion, Locale: opts.locale, Definitions: opts.definitionRef, Parent: opts.from, Offline: opts.offline}
				remote := records.RemoteTargetRequest{Product: opts.product, Region: opts.dataRegion, Locale: opts.locale, FullBuild: opts.fullBuild, Definitions: opts.definitionRef, Parent: opts.from, Offline: opts.offline}
				if opts.file == "" && opts.target == "" {
					var validation error
					if opts.product != "" {
						validation = remote.Validate()
					} else {
						validation = request.Validate()
					}
					if validation != nil {
						err, code = validation, 2
						break
					}
				}
				var root string
				root, err = workspaceRoot(opts.home)
				if err != nil {
					break
				}
				if opts.target != "" {
					var resolved selection.ResolvedTarget
					resolved, err = selection.ResolveTarget(ctx, root, opts.target, selection.ResolveOptions{Preparer: recordsTargetPreparer{}, Parent: opts.from, Definitions: opts.definitionRef, Offline: opts.offline})
					if err == nil {
						response.Result = resolved
						response.Context["target"], response.Context["snapshot"] = resolved.Config.Name, resolved.Pin.ID
					}
				} else if opts.file != "" {
					response.Result, err = selection.ResolveSelectionFile(ctx, root, opts.file)
				} else if opts.product != "" {
					var target records.RemoteTarget
					target, err = records.ResolveRemoteTarget(ctx, root, remote)
					if err == nil {
						response.Result = target.Pin
						response.Context["snapshot"], response.Context["observedAt"], response.Context["offline"] = target.Pin.ID, target.ObservedAt, target.Offline
						response.Captures = append(response.Captures, target.Capture)
						response.Warnings = append(response.Warnings, "Remote target metadata is pinned; this does not download data archives. Offline resolution reuses a recorded observation, not the server's current state.")
					}
				} else {
					var target records.LocalTarget
					target, err = records.ResolveLocalTarget(ctx, root, request)
					if err == nil {
						response.Result = target.Pin
						response.Context["snapshot"], response.Context["installation"], response.Context["client"] = target.Pin.ID, target.Installation, target.Client
						response.Captures = append(response.Captures, target.Capture)
					}
				}
			case "target list", "target add", "target remove", "target available":
				code, err = runTargetVerb(ctx, route, argument, opts, &response)
			case "target show", "live status", "live resume", "live cancel", "live abandon", "live session", "evidence show", "evidence verify", "evidence keep", "evidence remove":
				if argument == "" {
					err, code = errors.New("missing selection file or record ID; use describe"), 2
					break
				}
				var root string
				root, err = workspaceRoot(opts.home)
				if err != nil {
					break
				}
				switch route {
				case "target show":
					if strings.HasPrefix(argument, "PIN-") {
						response.Result, err = selection.InspectSelection(ctx, root, argument)
					} else {
						response.Result, err = selection.ShowTarget(ctx, root, argument)
					}
				case "live status":
					response.Result, err = live.Status(ctx, root, argument)
					response.OperationID = argument
				case "live resume":
					var record live.Outcome
					record, err = live.Resume(ctx, root, argument)
					response.OperationID = argument
					if record.OperationID != "" {
						response.Context["stage"] = record.Stage
						response.Result = record
					}
				case "live cancel", "live abandon":
					var record live.Outcome
					if route == "live abandon" {
						record, err = live.Abandon(ctx, root, argument)
					} else {
						record, err = live.Cancel(ctx, root, argument)
					}
					response.OperationID = argument
					if record.OperationID != "" {
						response.Result = record
						response.Context["stage"] = record.Stage
					}
					if record.Status == "abandoned" {
						response.Warnings = append(response.Warnings, "Cleanup abandoned explicitly; available evidence retained; missing report results remain unknown. No game ACK or runtime unload was performed.")
					}
				case "live session":
					var session live.RecordedSession
					session, err = live.ReadWindowSession(ctx, root, argument)
					if err == nil {
						response.Result = session.Connection()
						response.Context["session"] = argument
						response.Warnings = append(response.Warnings, "Historical session evidence only; live window identity and readiness have not been refreshed.")
					}
				case "evidence show", "evidence verify":
					response.Result, err = evidence.InspectEvidence(ctx, root, argument, route == "evidence verify")
				case "evidence keep":
					response.Result, err = vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (evidence.KeepResult, error) {
						return evidence.OpenArchive(s, m).KeepCapture(ctx, argument)
					})
				case "evidence remove":
					response.Result, err = vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (evidence.RemoveResult, error) {
						return evidence.OpenArchive(s, m).RemoveCapture(ctx, argument)
					})
				}
				if err != nil && (route == "live status" || route == "target show" || route == "evidence show" || route == "evidence verify") {
					response.Result = nil
				}
			case "evidence list":
				code, err = runEvidenceList(ctx, opts, &response)
			case "evidence bundle":
				if opts.ids == "" || opts.output == "" {
					err, code = errors.New("evidence bundle requires --ids and --output"), 2
					break
				}
				var root string
				root, err = workspaceRoot(opts.home)
				if err == nil {
					response.Result, err = evidence.Bundle(ctx, root, strings.Split(opts.ids, ","), opts.output)
				}
			default:
				err, code = fmt.Errorf("unknown command %q; use --help", strings.Join(opts.words, " ")), 2
			}
		}
	}
	if err != nil {
		faultCode := "command.failed"
		if code == 2 {
			faultCode = "command.invalid_arguments"
		} else {
			switch {
			case errors.Is(err, journal.ErrBusy):
				code, faultCode = 3, "journal.resource_busy"
			case errors.Is(err, journal.ErrTransition):
				code, faultCode = 3, "journal.invalid_transition"
			case errors.Is(err, live.ErrCandidateAmbiguous):
				code, faultCode = 2, "live.candidate_ambiguous"
			case errors.Is(err, live.ErrCandidateMissing):
				code, faultCode = 2, "live.candidate_missing"
			case errors.Is(err, live.ErrSessionConstraint):
				code, faultCode = 2, "live.session_constraint_mismatch"
			case errors.Is(err, live.ErrInputNotReady):
				code, faultCode = 3, "live.input_not_ready"
			case errors.Is(err, live.ErrIdentityUnreadable):
				code, faultCode = 3, "live.identity_unreadable"
			case errors.Is(err, live.ErrActorChanged):
				code, faultCode = 5, "live.actor_changed"
			case errors.Is(err, desktop.ErrIdentityChanged):
				code, faultCode = 5, "desktop.identity_changed"
			case errors.Is(err, selection.ErrClientIdentity):
				code, faultCode = 3, "selection.client_identity"
			case errors.Is(err, delivery.ErrConflict):
				code, faultCode = 3, "delivery.installation_conflict"
			case errors.Is(err, delivery.ErrInstallation):
				code, faultCode = 4, "delivery.invalid_installation"
			case errors.Is(err, delivery.ErrRelease):
				code, faultCode = 4, "delivery.invalid_release"
			case errors.Is(err, delivery.ErrPayload):
				code, faultCode = 4, "delivery.invalid_payload"
			case errors.Is(err, context.Canceled), errors.Is(err, context.DeadlineExceeded):
				code, faultCode = 7, "command.cancelled"
			case errors.Is(err, vault.ErrLegacyWorkspace):
				code, faultCode = 3, "vault.legacy_detected"
			case errors.Is(err, vault.ErrWorkspaceFormat):
				code, faultCode = 4, "vault.unsupported_workspace"
			case errors.Is(err, vault.ErrBlobIntegrity):
				code, faultCode = 4, "vault.blob_integrity"
			case errors.Is(err, vault.ErrGeneration):
				code, faultCode = 3, "vault.generation_conflict"
			case errors.Is(err, evidence.ErrCaptureReferenced):
				code, faultCode = 2, "evidence.capture_referenced"
			case errors.Is(err, evidence.ErrInvalidRetention):
				code, faultCode = 4, "evidence.invalid_retention"
			case errors.Is(err, evidence.ErrReferenceScanLimit):
				code, faultCode = 3, "evidence.reference_scan_limit"
			case errors.Is(err, evidence.ErrCaptureScanLimit):
				code, faultCode = 3, "evidence.capture_scan_limit"
			case errors.Is(err, codebase.ErrInvalidAddon):
				code, faultCode = 4, "codebase.addon_static_check_failed"
			case errors.Is(err, vault.ErrMissingRecord):
				code, faultCode = 2, "vault.record_missing"
			case errors.Is(err, desktop.ErrUnsupported):
				code, faultCode = 3, "desktop.unsupported_platform"
			default:
				code = 5
			}
		}
		stage := "dispatch"
		switch {
		case errors.Is(err, live.ErrCandidateAmbiguous), errors.Is(err, live.ErrCandidateMissing), errors.Is(err, live.ErrSessionConstraint):
			stage = "selection"
		case errors.Is(err, live.ErrInputNotReady), errors.Is(err, live.ErrIdentityUnreadable), errors.Is(err, live.ErrActorChanged):
			stage = "connect"
		}
		var accountChoice *live.AccountSelectionError
		if errors.As(err, &accountChoice) {
			code, faultCode, stage = 2, "live.account_selection_required", "selection"
			response.Context["accounts"] = append([]string{}, accountChoice.Candidates...)
		}
		if errors.Is(err, live.ErrAccountScanLimit) {
			code, faultCode, stage = 3, "live.account_scan_limit", "selection"
		}
		if queryExit, queryCode, ok := queryFault(err); ok && code != 2 {
			code, faultCode, stage = queryExit, queryCode, "query"
			if len(opts.words) >= 2 && opts.words[0] == "target" && opts.words[1] == "resolve" {
				stage = "selection"
			}
			if strings.HasPrefix(queryCode, "selection.project_") {
				stage = "selection"
			}
			if strings.HasPrefix(queryCode, "texture.") || queryCode == "records.image_output_limit" {
				stage = "export"
			}
		}
		if errors.Is(err, records.ErrExportConflict) {
			code, faultCode, stage = 3, "records.export_conflict", "export"
		} else if errors.Is(err, records.ErrExportPath) {
			code, faultCode, stage = 2, "records.export_path", "export"
		} else if errors.Is(err, records.ErrExportOptions) {
			code, faultCode, stage = 2, "records.export_options", "export"
		}
		response.Error = &Fault{Code: faultCode, Message: err.Error(), Stage: stage}
		if response.OperationID != "" && len(opts.words) >= 2 && opts.words[0] == "live" {
			if actual, ok := response.Context["stage"].(string); ok {
				response.Error.Stage = actual
				response.Error.ResumeOperationID = response.OperationID
				response.Error.Retryable = errors.Is(err, journal.ErrBusy) || errors.Is(err, live.ErrAckReadinessPending) || errors.Is(err, live.ErrReceiptHidePending)
			}
		}
		var diagnostic *relational.Diagnostic
		if errors.As(err, &diagnostic) {
			response.Error.Location = &QueryLocation{Offset: diagnostic.Offset, Line: diagnostic.Line, Column: diagnostic.Column}
		}
		// Selection ambiguity and precise connect states carry their data in the
		// envelope context so the user can choose and retry with constraints.
		var choice *live.CandidateSelectionError
		if errors.As(err, &choice) {
			response.Context["candidates"] = append([]live.Candidate{}, choice.Candidates...)
		}
		var notReady *live.InputNotReadyError
		if errors.As(err, &notReady) {
			response.Context["inputReason"] = notReady.Reason
			response.Error.Retryable = true
		}
		if errors.Is(err, live.ErrActorChanged) || errors.Is(err, live.ErrIdentityUnreadable) {
			response.Error.Retryable = true
		}
	} else {
		response.OK = true
	}
	if err := render(stdout, opts.format, response); err != nil {
		fmt.Fprintln(stderr, "command.output_failed:", err)
		return 5
	}
	return code
}

func parseOptions(args []string) (Options, error) {
	opts := Options{format: "text", line: 1, count: 80, limit: 50, maxBytes: 128 << 20}
	// Find output format even if an earlier argument is invalid, so errors obey
	// machine callers' requested encoding. Invalid formats fall back to JSON.
	for i, arg := range args {
		if arg == "--format" && i+1 < len(args) {
			opts.format = args[i+1]
		}
		if strings.HasPrefix(arg, "--format=") {
			opts.format = strings.TrimPrefix(arg, "--format=")
		}
	}
	if opts.format != "text" && opts.format != "json" && opts.format != "jsonl" {
		opts.format = "json"
		return opts, errors.New("format must be text, json, or jsonl")
	}
	words, err := collectCommandWords(args)
	if err != nil {
		return opts, err
	}
	opts.words = words
	contract, route, err := findCommandContract(opts.words)
	if err != nil {
		return opts, err
	}
	if route == "asset search" {
		// The public community listfile exceeds 128 MiB. Keep a bounded
		// route-specific default and allow callers to choose a smaller budget.
		opts.maxBytes = 256 << 20
	}
	for _, arg := range args {
		if arg == "--help" || arg == "-h" {
			opts.help = true
			break
		}
	}
	if !opts.help && contract != nil && contract.positional != "" && len(opts.words) == len(strings.Fields(route)) {
		return opts, fmt.Errorf("missing required argument %s", contract.positional)
	}
	seen := map[string]bool{}
	for i := 0; i < len(args); i++ {
		arg := args[i]
		if arg == "--offline" || arg == "--resume" || arg == "--latest" || arg == "--cdn" || arg == "--overwrite" || arg == "--allow-partial" || arg == "--stdin" || arg == "--include-removed" ||
			arg == "--remote" || arg == "--replace" || arg == "--uncommitted" || arg == "--dry-run" || arg == "--plan" || arg == "--fresh" {
			if _, ok := commandFlag(contract, arg); !ok {
				return opts, unsupportedRouteFlag(route, arg)
			}
			if seen[arg] {
				return opts, fmt.Errorf("repeated flag %s", arg)
			}
			seen[arg] = true
			switch arg {
			case "--stdin":
				opts.stdin = true
			case "--allow-partial":
				opts.allowPartial = true
			case "--overwrite":
				opts.overwrite = true
			case "--cdn":
				opts.cdn = true
			case "--offline":
				opts.offline = true
			case "--latest":
				opts.latest = true
			case "--resume":
				opts.resume = true
			case "--remote":
				opts.remote = true
			case "--replace":
				opts.replace = true
			case "--uncommitted":
				opts.uncommitted = true
			case "--dry-run":
				opts.dryRun = true
			case "--plan":
				opts.plan = true
			case "--fresh":
				opts.fresh = true
			case "--include-removed":
				opts.includeRemoved = true
			}
			continue
		}
		if arg == "--help" || arg == "-h" {
			opts.help = true
			continue
		}
		if !strings.HasPrefix(arg, "-") {
			continue
		}
		key, value, inline := strings.Cut(arg, "=")
		spec, known := commandFlag(contract, key)
		if !known {
			return opts, unsupportedRouteFlag(route, key)
		}
		if !spec.value {
			return opts, fmt.Errorf("unknown flag %s", key)
		}
		if seen[key] && key != "--param" {
			return opts, fmt.Errorf("repeated flag %s", key)
		}
		seen[key] = true
		if !inline {
			if i+1 >= len(args) || strings.HasPrefix(args[i+1], "--") {
				return opts, fmt.Errorf("missing value for %s", key)
			}
			i++
			value = args[i]
		}
		if value == "" {
			return opts, fmt.Errorf("empty value for %s", key)
		}
		if key == "--home" {
			opts.home = value
		}
		if key == "--file" {
			opts.file = value
		}
		switch key {
		case "--encoding":
			if value == "csv" {
				if route != "data hotfix" && route != "data sql" {
					return opts, errors.New("--encoding csv is only supported by data hotfix and data sql")
				}
				opts.encoding = value
				break
			}
			if route == "data sql" {
				return opts, errors.New("data sql --encoding must be csv")
			}
			if value != "raw" && value != "png" && value != "webp" {
				return opts, errors.New("--encoding must be raw, png, or webp")
			}
			opts.encoding = value
		case "--channels":
			opts.channels = value
		case "--mipmap":
			n, err := strconv.Atoi(value)
			if err != nil || n < 0 || n > 15 {
				return opts, errors.New("--mipmap must be between 0 and 15")
			}
			opts.mipmap = n
		case "--max-pixels":
			n, err := strconv.ParseInt(value, 10, 64)
			if err != nil || n < 1 || n > 64<<20 {
				return opts, errors.New("--max-pixels must be between 1 and 67108864")
			}
			opts.maxPixels = n
		case "--project":
			opts.project = value
		case "--target":
			opts.target = value
		case "--ids":
			opts.ids = value
		case "--session":
			opts.session = value
		case "--account":
			opts.account = value
		case "--probe":
			opts.probe = value
		case "--request":
			opts.request = value
		case "--pid":
			n, err := strconv.ParseUint(value, 10, 32)
			if err != nil || n == 0 {
				return opts, errors.New("invalid --pid")
			}
			opts.pid = uint32(n)
		case "--character":
			opts.character = value
		case "--realm":
			opts.realm = value
		case "--region":
			opts.dataRegion = value
		case "--capture-area":
			if value == "window" {
				opts.region = desktop.WholeWindowCapture()
				break
			}
			parts := strings.Split(value, ",")
			if len(parts) != 4 {
				return opts, errors.New("--capture-area requires window or x,y,width,height")
			}
			var values [4]int
			for i, part := range parts {
				n, err := strconv.ParseUint(part, 10, 15)
				if err != nil {
					return opts, errors.New("invalid --capture-area")
				}
				values[i] = int(n)
			}
			opts.region = image.Rectangle{Min: image.Pt(values[0], values[1]), Max: image.Pt(values[0]+values[2], values[1]+values[3])}
			if opts.region.Empty() || values[2] > 4096 || values[3] > 4096 || opts.region.Max.X > 16384 || opts.region.Max.Y > 16384 {
				return opts, errors.New("invalid --capture-area bounds")
			}
		case "--locale":
			opts.locale = value
		case "--definitions":
			opts.definitionRef = value
		case "--release":
			opts.release = value
		case "--output":
			opts.output = value
		case "--table-hash":
			if len(value) != 8 {
				return opts, errors.New("--table-hash requires eight hexadecimal digits")
			}
			n, err := strconv.ParseUint(value, 16, 32)
			if err != nil {
				return opts, errors.New("invalid --table-hash")
			}
			v := uint32(n)
			opts.tableHash = &v
		case "--after-index":
			n, err := strconv.ParseUint(value, 10, 32)
			if err != nil {
				return opts, errors.New("invalid --after-index")
			}
			v := uint32(n)
			opts.afterIndex = &v
		case "--after-id":
			n, err := strconv.ParseUint(value, 10, 32)
			if err != nil {
				return opts, errors.New("invalid --after-id")
			}
			value := uint32(n)
			opts.afterID = &value
		case "--table":
			opts.table = value
		case "--id":
			n, err := strconv.ParseUint(value, 10, 32)
			if err != nil {
				return opts, errors.New("invalid --id")
			}
			opts.recordID, opts.recordIDSet = uint32(n), true
		case "--record":
			n, err := strconv.ParseUint(value, 10, 32)
			if err != nil {
				return opts, errors.New("invalid --record")
			}
			opts.recordID, opts.recordIDSet = uint32(n), true
		case "--installation":
			opts.installation = value
		case "--file-id":
			n, err := strconv.ParseUint(value, 10, 32)
			if err != nil || n == 0 {
				return opts, errors.New("invalid --file-id")
			}
			opts.fileID = uint32(n)
		case "--max-bytes":
			n, err := strconv.ParseInt(value, 10, 64)
			if err != nil || n < 1 || n > 512<<20 {
				return opts, errors.New("--max-bytes must be between 1 and 536870912")
			}
			opts.maxBytes = n
		case "--source":
			if route == "data hotfix" && value != "wago" && value != "dbcache" && value != "raidbots" {
				return opts, errors.New("--source must be wago, dbcache, or raidbots")
			}
			opts.source = value
		case "--product":
			opts.product = value
		case "--build":
			opts.fullBuild = value
		case "--ref":
			opts.ref = value
		case "--snapshot":
			opts.snapshot = value
		case "--path":
			opts.path = value
		case "--symbol":
			opts.symbol = value
		case "--target-path":
			opts.targetPath = value
		case "--toc":
			opts.toc = value
		case "--matrix":
			opts.matrix = value
		case "--mode":
			if value != "precise" && value != "exploratory" {
				return opts, errors.New("--mode must be precise or exploratory")
			}
			opts.searchMode = value
		case "--topic":
			if value != "api" && value != "lua" && value != "xml" && value != "toc" && value != "asset" {
				return opts, errors.New("--topic must be api, lua, xml, toc, or asset")
			}
			opts.topic = value
		case "--listfile":
			if value != string(records.ListfileCommunityCSV) && value != string(records.ListfileWowExportText) && value != string(records.ListfileWowExportBinary) {
				return opts, errors.New("invalid --listfile source")
			}
			opts.listfile = value
		case "--extension":
			opts.extension = value
		case "--name":
			if route == "live probe put" {
				opts.name = value
			} else {
				opts.fileName = value
			}
		case "--sql":
			opts.sql = value
		case "--param":
			name, raw, found := strings.Cut(value, "=")
			if !found || name == "" || len(name) > 128 {
				return opts, errors.New("--param requires name=scalar")
			}
			if opts.parameters == nil {
				opts.parameters = map[string]any{}
			}
			if _, exists := opts.parameters[name]; exists {
				return opts, fmt.Errorf("duplicate --param %s", name)
			}
			opts.parameters[name] = parseQueryScalar(raw)
		case "--max-frames":
			n, err := strconv.Atoi(value)
			if err != nil || n < 1 || n > 1000000 {
				return opts, errors.New("--max-frames must be between 1 and 1000000")
			}
			opts.maxFrames = n
		case "--from":
			opts.from = value
		case "--to":
			opts.to = value
		case "--line", "--count", "--limit":
			n, err := strconv.Atoi(value)
			if err != nil || n < 1 || key == "--count" && n > 2000 || key == "--limit" && n > dataLimitCeiling(route) {
				return opts, fmt.Errorf("invalid %s", key)
			}
			if key == "--line" {
				opts.line = n
			} else {
				if key == "--count" {
					opts.count = n
				} else {
					opts.limit, opts.limitSet = n, true
				}
			}
		case "--spell-id", "--item-id", "--npc-id", "--display-id", "--journal-encounter-id":
			n, err := strconv.ParseUint(value, 10, 32)
			if err != nil || key != "--npc-id" && n == 0 {
				return opts, fmt.Errorf("invalid %s", key)
			}
			switch key {
			case "--spell-id":
				opts.spellID = uint32(n)
			case "--item-id":
				opts.itemID, opts.itemIDSet = uint32(n), true
			case "--npc-id":
				opts.npcID = uint32(n)
			case "--display-id":
				opts.displayID = uint32(n)
			case "--journal-encounter-id":
				opts.journalEncounterID = uint32(n)
			}
		case "--file-data-id", "--model-file-data-id":
			n, err := strconv.ParseUint(value, 10, 32)
			if err != nil || n == 0 {
				return opts, fmt.Errorf("invalid %s", key)
			}
			if key == "--file-data-id" {
				opts.fileDataID = uint32Set{value: uint32(n), set: true}
			} else {
				opts.modelFileDataID = uint32Set{value: uint32(n), set: true}
			}
		case "--race-id", "--gender", "--max-depth":
			n, err := strconv.Atoi(value)
			if err != nil || key == "--max-depth" && n < 0 {
				return opts, fmt.Errorf("invalid %s", key)
			}
			switch key {
			case "--race-id":
				opts.raceID = n
			case "--gender":
				opts.gender = n
			case "--max-depth":
				opts.maxDepth = n
			}
		case "--field":
			opts.field = value
		case "--query":
			opts.queryText = value
		case "--filter":
			opts.filter = value
		case "--fields":
			opts.fieldList = strings.Split(value, ",")
		case "--value":
			n, err := strconv.ParseUint(value, 10, 32)
			if err != nil {
				return opts, errors.New("invalid --value")
			}
			opts.value = uint32Set{value: uint32(n), set: true}
		case "--dbcache":
			opts.dbcache = value
		case "--raidbots":
			opts.raidbots = value
		case "--cursor":
			opts.cursor = value
		case "--search":
			opts.search = value
		case "--push":
			n, err := strconv.ParseInt(value, 10, 32)
			if err != nil {
				return opts, errors.New("invalid --push")
			}
			opts.push, opts.pushSet = n, true
		case "--status":
			n, err := strconv.Atoi(value)
			if err != nil || n < 0 || n > 255 {
				return opts, errors.New("--status must be between 0 and 255")
			}
			opts.status, opts.statusSet = n, true
		case "--region-id":
			n, err := strconv.ParseUint(value, 10, 32)
			if err != nil {
				return opts, errors.New("invalid --region-id")
			}
			opts.regionID, opts.regionIDSet = uint32(n), true
		case "--page":
			n, err := strconv.Atoi(value)
			if err != nil || n < 0 {
				return opts, errors.New("invalid --page")
			}
			opts.page = n
		case "--max-pages":
			n, err := strconv.Atoi(value)
			if err != nil || n < 1 || n > 2048 {
				return opts, errors.New("--max-pages must be between 1 and 2048")
			}
			opts.maxPages = n
		case "--max-requests":
			n, err := strconv.Atoi(value)
			if err != nil || n < 1 {
				return opts, errors.New("--max-requests must be at least 1")
			}
			opts.maxRequests = n
		case "--target-bytes":
			n, err := strconv.ParseInt(value, 10, 64)
			if err != nil || n < 0 || n > vault.MaxCacheMaxBytes {
				return opts, fmt.Errorf("--target-bytes must be between 0 and %d", vault.MaxCacheMaxBytes)
			}
			opts.targetBytes = n
		case "--max-objects":
			n, err := strconv.Atoi(value)
			if err != nil || n < 0 {
				return opts, errors.New("--max-objects must be a non-negative count")
			}
			opts.maxObjects = n
		case "--cache-max-bytes":
			n, err := strconv.ParseInt(value, 10, 64)
			if err != nil || n < 0 || n > vault.MaxCacheMaxBytes {
				return opts, fmt.Errorf("--cache-max-bytes must be between 0 and %d", vault.MaxCacheMaxBytes)
			}
			opts.cacheMaxBytes, opts.cacheMaxBytesSet = n, true
		case "--download-workers":
			n, err := strconv.Atoi(value)
			if err != nil || n < 0 || n > vault.MaxDownloadWorkers {
				return opts, fmt.Errorf("--download-workers must be between 0 and %d", vault.MaxDownloadWorkers)
			}
			opts.downloadWorkers, opts.downloadWorkersSet = n, true
		}
	}
	if len(opts.words) == 0 {
		opts.help = true
	}
	if route == "addon status" && seen["--installation"] && seen["--path"] {
		return opts, errors.New("addon status accepts --installation or --path, not both")
	}
	if route == "target resolve" && seen["--file"] {
		for _, flag := range []string{"--target", "--installation", "--product", "--build", "--region", "--locale", "--definitions", "--from", "--offline"} {
			if seen[flag] {
				return opts, fmt.Errorf("target resolve --file cannot be combined with %s", flag)
			}
		}
	}
	if route == "target resolve" && seen["--target"] {
		for _, flag := range []string{"--file", "--installation", "--product", "--build", "--region", "--locale"} {
			if seen[flag] {
				return opts, fmt.Errorf("target resolve --target cannot be combined with %s", flag)
			}
		}
	}
	if route == "target resolve" && (seen["--product"] && seen["--installation"] || seen["--build"] && !seen["--product"]) {
		return opts, errors.New("target resolve accepts either --installation or --product; --build requires --product")
	}
	if route == "target add" && !opts.help && opts.remote == (opts.installation != "") {
		return opts, errors.New("target add requires exactly one of --installation or --remote")
	}
	if route == "config set" && !opts.help && !seen["--cache-max-bytes"] && !seen["--download-workers"] {
		return opts, errors.New("config set requires --cache-max-bytes or --download-workers")
	}
	if route == "init" && !opts.help {
		selected := 0
		for _, flag := range []string{"--plan", "--fresh", "--resume"} {
			if seen[flag] {
				selected++
			}
		}
		if selected > 1 {
			return opts, errors.New("init accepts at most one of --plan, --fresh, --resume")
		}
	}
	if route == "data db2" && seen["--id"] && (seen["--after-id"] || seen["--limit"]) {
		return opts, errors.New("--id cannot be combined with --after-id or --limit")
	}
	if seen["--cdn"] && seen["--installation"] {
		return opts, errors.New("choose either --cdn or --installation, not both")
	}
	return opts, nil
}

func boolCount(values ...bool) int {
	count := 0
	for _, value := range values {
		if value {
			count++
		}
	}
	return count
}

func (opts Options) fileQuery() records.FileQuery {
	return records.FileQuery{Installation: opts.installation, CDN: opts.cdn, Offline: opts.offline, FileDataID: opts.fileID, MetadataBytes: 512 << 20, ContentBytes: opts.maxBytes}
}

func unsupportedRouteFlag(route, flag string) error {
	if route == "" {
		route = "the command"
	}
	return fmt.Errorf("%s is not supported by %s; use %s --help", flag, route, route)
}

// dataLimitCeiling keeps the documented 200 bound everywhere except the new
// data verbs whose modules enforce their own exact per-mode ranges; the module
// error then maps to exit 2 instead of a parser rejection.
func dataLimitCeiling(route string) int {
	switch route {
	case "data db2 search", "data db2 foreign-key", "data db2 stream", "data decor list":
		return 200000
	}
	return 200
}

// userHomeDirectory is the single home-resolution helper for command dispatch;
// the import contract keeps os.UserHomeDir in this file only.
func userHomeDirectory() (string, error) {
	return os.UserHomeDir()
}

func workspaceRoot(explicit string) (string, error) {
	if explicit != "" {
		return explicit, nil
	}
	if configured := os.Getenv("LYCHEEDEV_HOME"); configured != "" {
		return configured, nil
	}
	directory, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(directory, ".lycheedev"), nil
}

func render(out io.Writer, format string, response Envelope) error {
	encoder := json.NewEncoder(out)
	encoder.SetEscapeHTML(false)
	if format == "json" {
		return encoder.Encode(response)
	}
	if format == "jsonl" {
		if err := encoder.Encode(map[string]any{"type": "begin", "schema": response.Schema}); err != nil {
			return err
		}
		if !response.OK {
			return encoder.Encode(map[string]any{"type": "error", "envelope": response})
		}
		if err := encoder.Encode(map[string]any{"type": "record", "result": response.Result}); err != nil {
			return err
		}
		return encoder.Encode(map[string]any{"type": "end", "envelope": response})
	}
	if !response.OK {
		_, err := fmt.Fprintf(out, "%s: %s\n", response.Error.Code, response.Error.Message)
		return err
	}
	encoder.SetIndent("", "  ")
	return encoder.Encode(response.Result)
}
