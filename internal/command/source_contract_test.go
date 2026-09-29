package command

import (
	"context"
	"errors"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/codebase"
	"github.com/follenfang/lycheedev/internal/codebase/environment"
	"github.com/follenfang/lycheedev/internal/codebase/flow"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestSourceFaultClasses(t *testing.T) {
	for _, item := range []struct {
		cause error
		exit  int
		code  string
	}{
		{codebase.ErrInvalidSearchCursor, 2, "codebase.invalid_search_cursor"},
		{codebase.ErrUnknownRepository, 2, "codebase.unknown_repository"},
		{codebase.ErrUnknownProduct, 2, "codebase.unknown_product"},
		{codebase.ErrInvalidSourceRef, 2, "codebase.invalid_source_ref"},
		{codebase.ErrSourceNotReady, 3, "codebase.source_not_ready"},
		{codebase.ErrSourceBudget, 3, "codebase.source_budget"},
		{codebase.ErrSourceIntegrity, 4, "codebase.source_integrity"},
		{environment.ErrBudget, 3, "environment.input_budget"},
		{environment.ErrIdentity, 4, "environment.invalid_identity"},
		{flow.ErrBudget, 3, "flow.budget"},
		{flow.ErrInput, 4, "flow.invalid_input"},
	} {
		exit, code, ok := queryFault(errors.Join(errors.New("wrapped"), item.cause))
		if !ok || exit != item.exit || code != item.code {
			t.Fatalf("%v mapped to %d %s %t", item.cause, exit, code, ok)
		}
	}
}

func TestSourceSyncRejectsUnknownCatalogSelectionAsArguments(t *testing.T) {
	workspace := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(context.Background(), workspace); err != nil {
		t.Fatal(err)
	}
	for _, item := range []struct{ source, product, ref, code string }{
		{"weakauras", "retail", "", "codebase.unknown_product"},
		{"missing-repository", "main", "", "codebase.unknown_repository"},
		{"weakauras", "main", "main", "codebase.invalid_source_ref"},
	} {
		args := []string{"source", "sync", "--source", item.source, "--product", item.product, "--home", workspace, "--format=json"}
		if item.ref != "" {
			args = append(args, "--ref", item.ref)
		}
		response, exit := invoke(t, args...)
		if exit != 2 || response.OK || response.Error.Code != item.code {
			t.Fatalf("%+v mapped incorrectly: %+v (%d)", item, response.Error, exit)
		}
	}
}

func TestDoctorIncludesPackagedLuaLSCheck(t *testing.T) {
	root := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(context.Background(), root); err != nil {
		t.Fatal(err)
	}
	response, code := invoke(t, "doctor", "--offline", "--home", root, "--format=json")
	if code != 0 || !response.OK {
		t.Fatalf("doctor: %+v (%d)", response, code)
	}
	check, ok := doctorChecks(t, response)["luals.runtime"].(map[string]any)
	if !ok || check["status"] == nil {
		t.Fatalf("packaged runtime health missing: %#v", check)
	}
}

func TestMatrixRejectsSingleClientEnvironment(t *testing.T) {
	response, code := invoke(t, "source", "validate", "--matrix", "matrix.json", "--semantic", "--environment", "PIN-client", "--format=json")
	if code != 2 || response.OK {
		t.Fatalf("single environment silently applied to matrix: %+v (%d)", response, code)
	}
}

func TestSourceResearchContractParsesOnlyItsOwnFlags(t *testing.T) {
	refs, err := parseOptions([]string{"source", "refs", "--snapshot", "PIN-test", "--symbol-id", "symbol-1", "--direction", "incoming", "--limit", "7", "--cursor", "next", "--environment", "PIN-client", "--static-only"})
	if err != nil || refs.snapshot != "PIN-test" || refs.symbolID != "symbol-1" || refs.direction != "incoming" || refs.limit != 7 || refs.cursor != "next" || refs.environmentSnapshot != "PIN-client" || !refs.staticOnly {
		t.Fatalf("refs options: %+v, %v", refs, err)
	}
	context, err := parseOptions([]string{"source", "context", "--symbol", "Widget:OnEvent", "--max-bytes", "4096", "--max-lines", "32", "--depth", "2", "--flow"})
	if err != nil || context.symbol != "Widget:OnEvent" || context.maxBytes != 4096 || context.sourceMaxLines != 32 || context.sourceDepth != 2 || !context.sourceFlow {
		t.Fatalf("context options: %+v, %v", context, err)
	}
	defaults, err := parseOptions([]string{"source", "context", "--symbol-id", "symbol-1"})
	if err != nil || defaults.sourceDepth != 1 || defaults.sourceMaxLines != 120 || defaults.maxBytes != 16<<10 {
		t.Fatalf("context defaults: %+v, %v", defaults, err)
	}
	query, err := parseOptions([]string{"source", "query", "GetCVar", "--snapshot", "PIN-test", "--cursor", "page-2"})
	if err != nil || query.cursor != "page-2" {
		t.Fatalf("query cursor: %+v, %v", query, err)
	}
	validation, err := parseOptions([]string{"source", "validate", "--semantic", "--environment", "PIN-client", "--snapshot", "PIN-addon", "--path", "addon", "--toc", "Addon.toc"})
	if err != nil || !validation.semantic || validation.environmentSnapshot != "PIN-client" {
		t.Fatalf("validation environment: %+v, %v", validation, err)
	}
	prune, err := parseOptions([]string{"source", "prune", "--target-bytes", "0"})
	if err != nil || prune.targetBytes != 0 {
		t.Fatalf("source prune: %+v, %v", prune, err)
	}
	if _, err := parseOptions([]string{"source", "prune"}); err == nil {
		t.Fatal("source prune accepted missing explicit target")
	}
	for _, args := range [][]string{
		{"source", "refs", "--direction", "sideways"},
		{"source", "refs", "--flow"},
		{"source", "context", "--max-bytes", "1048577"},
		{"source", "context", "--max-lines", "2001"},
		{"source", "context", "--depth", "5"},
		{"source", "query", "needle", "--symbol-id", "symbol-1"},
	} {
		if _, err := parseOptions(args); err == nil {
			t.Fatalf("accepted %v", args)
		}
	}
}

func TestSourceResearchThroughCLI(t *testing.T) {
	workspace := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(context.Background(), workspace); err != nil {
		t.Fatal(err)
	}
	fixture := filepath.Join("..", "..", "tests", "fixtures", "codebase", "sources", "valid-retail")
	indexed, err := codebase.IndexFixtureSource(context.Background(), workspace, "wow-ui-source", "retail", fixture)
	if err != nil {
		t.Fatal(err)
	}
	common := []string{"--snapshot", indexed.Snapshot.ID, "--home", workspace, "--format=json"}
	queryArgs := append([]string{"source", "query", "GetRestrictedInfo", "--mode", "precise", "--topic", "api"}, common...)
	query, code := invoke(t, queryArgs...)
	if code != 0 || !query.OK {
		t.Fatalf("query: %+v (%d)", query, code)
	}
	results, ok := resultMap(t, query)["results"].([]any)
	if !ok || len(results) == 0 {
		t.Fatalf("no query results: %+v", query)
	}
	var id string
	for _, raw := range results {
		item, ok := raw.(map[string]any)
		if ok && item["symbolId"] != nil {
			id, _ = item["symbolId"].(string)
		}
		if id != "" {
			break
		}
	}
	if id == "" {
		t.Fatalf("query result lacks symbolId: %#v", results)
	}
	pageArgs := append([]string{"source", "query", "Get", "--mode", "exploratory", "--topic", "api", "--limit", "1"}, common...)
	firstPage, firstCode := invoke(t, pageArgs...)
	if firstCode != 0 || !firstPage.OK {
		t.Fatalf("first query page: %+v (%d)", firstPage, firstCode)
	}
	if cursor, ok := resultMap(t, firstPage)["nextCursor"].(string); ok && cursor != "" {
		secondArgs := append(pageArgs, "--cursor", cursor)
		secondPage, secondCode := invoke(t, secondArgs...)
		if secondCode != 0 || !secondPage.OK || resultMap(t, secondPage)["snapshotId"] != indexed.Snapshot.ID {
			t.Fatalf("second query page: %+v (%d)", secondPage, secondCode)
		}
		// A valid cursor is still invalid for another question or command.
		for _, changed := range [][]string{
			{"source", "query", "GetRestrictedInfo", "--mode", "exploratory", "--topic", "api", "--limit", "1", "--cursor", cursor},
			{"source", "refs", "--symbol-id", id, "--static-only", "--limit", "1", "--cursor", cursor},
		} {
			rejected, exit := invoke(t, append(changed, common...)...)
			if exit != 2 || rejected.OK || rejected.Error.Code != "codebase.invalid_search_cursor" {
				t.Fatalf("cross-scope cursor accepted: %v %+v (%d)", changed, rejected, exit)
			}
		}
	} else {
		t.Fatalf("fixture query did not offer cursor: %+v", firstPage)
	}
	for _, route := range []string{"refs", "context"} {
		args := append([]string{"source", route, "--symbol-id", id, "--static-only"}, common...)
		response, code := invoke(t, args...)
		if code != 0 || !response.OK || len(response.Captures) != 1 {
			t.Fatalf("%s: %+v (%d)", route, response, code)
		}
		result := resultMap(t, response)
		if result["snapshot"] != indexed.Snapshot.ID || result["commit"] != indexed.Pin.ExactCommit {
			t.Fatalf("%s lost fixed identity: %#v", route, result)
		}
		coverage, ok := result["coverage"].(map[string]any)
		if !ok || coverage["semantic"] == "complete" {
			t.Fatalf("%s overclaimed static semantic coverage: %#v", route, result)
		}
	}
	defaultArgs := append([]string{"source", "refs", "--symbol-id", id}, common...)
	defaultResponse, defaultCode := invoke(t, defaultArgs...)
	if defaultCode != 0 || !defaultResponse.OK || len(defaultResponse.Warnings) == 0 {
		t.Fatalf("missing packaged runtime did not degrade with a visible warning: %+v (%d)", defaultResponse, defaultCode)
	}
	if coverage, ok := resultMap(t, defaultResponse)["coverage"].(map[string]any); !ok || coverage["semantic"] == "complete" {
		t.Fatalf("missing runtime marked semantic complete: %+v", defaultResponse)
	}
	flowArgs := append([]string{"source", "context", "--symbol-id", id, "--static-only", "--flow"}, common...)
	flowResponse, flowCode := invoke(t, flowArgs...)
	if flowCode != 0 || !flowResponse.OK {
		t.Fatalf("explicit flow: %+v (%d)", flowResponse, flowCode)
	}
	if coverage, ok := resultMap(t, flowResponse)["coverage"].(map[string]any); !ok || coverage["flow"] == nil || coverage["flow"] == "not_run" {
		t.Fatalf("explicit flow was silently skipped: %+v", flowResponse)
	}
	invalid := append([]string{"source", "context", "--symbol-id", id, "--static-only", "--cursor", "bad"}, common...)
	response, code := invoke(t, invalid...)
	if code != 2 || response.OK || response.Error.Code != "codebase.invalid_search_cursor" {
		t.Fatalf("bad cursor mapped incorrectly: %+v (%d)", response, code)
	}
	prune, pruneCode := invoke(t, "source", "prune", "--target-bytes", "0", "--home", workspace, "--format=json")
	if pruneCode != 0 || !prune.OK {
		t.Fatalf("source-only prune: %+v (%d)", prune, pruneCode)
	}
	retained, retainedCode := invoke(t, queryArgs...)
	if retainedCode != 0 || !retained.OK || resultMap(t, retained)["resolvedCommit"] != indexed.Pin.ExactCommit {
		t.Fatalf("source prune lost fixed fixture pin: %+v (%d)", retained, retainedCode)
	}
}
