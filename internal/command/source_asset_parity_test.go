package command

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/codebase"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/testkit"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestSourceQueryModesThroughCLI(t *testing.T) {
	workspace := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(context.Background(), workspace); err != nil {
		t.Fatal(err)
	}
	fixture := filepath.Join("..", "..", "tests", "fixtures", "codebase", "sources", "valid-retail")
	indexed, err := codebase.IndexFixtureSource(context.Background(), workspace, "wow-ui-source", "retail", fixture)
	if err != nil {
		t.Fatal(err)
	}
	status, code := invoke(t, "source", "list", "--snapshot", indexed.Snapshot.ID, "--home", workspace, "--format=json")
	if code != 0 || !status.OK || status.Context["snapshot"] != indexed.Snapshot.ID {
		t.Fatalf("source readiness: %+v (%d)", status, code)
	}
	invalidMatrix := filepath.Join(t.TempDir(), "matrix.json")
	if err := os.WriteFile(invalidMatrix, []byte(`{"targets":[]}`), 0600); err != nil {
		t.Fatal(err)
	}
	matrix, code := invoke(t, "source", "validate", "--matrix", invalidMatrix, "--home", workspace, "--format=json")
	if code == 0 || matrix.OK || matrix.Error.Code == "selection.project_missing" {
		t.Fatalf("matrix dispatch fell back to project selection: %+v (%d)", matrix, code)
	}
	for _, args := range [][]string{
		{"--mode", "precise", "--topic", "api"},
		{"--mode", "exploratory", "--topic", "lua"},
	} {
		call := append([]string{"source", "query", "GetCVar", "--snapshot", indexed.Snapshot.ID, "--home", workspace, "--format=json"}, args...)
		response, code := invoke(t, call...)
		if code != 0 || !response.OK || len(response.Captures) != 1 {
			t.Fatalf("%v: code=%d response=%+v", args, code, response)
		}
		result := resultMap(t, response)
		if result["snapshotId"] != indexed.Snapshot.ID || result["resolvedCommit"] != indexed.Pin.ExactCommit {
			t.Fatalf("lost fixed identity: %#v", result)
		}
	}
	inspected, code := invoke(t, "source", "inspect", "--symbol", "GetCVar", "--snapshot", indexed.Snapshot.ID, "--home", workspace, "--format=json")
	if code != 0 || !inspected.OK || len(inspected.Captures) != 1 {
		t.Fatalf("symbol inspect: %+v (%d)", inspected, code)
	}
	bad, code := invoke(t, "source", "query", "GetCVar", "--snapshot", indexed.Snapshot.ID, "--topic", "unsupported", "--home", workspace, "--format=json")
	if code != 2 || bad.OK {
		t.Fatalf("invalid topic accepted: %+v (%d)", bad, code)
	}
}

func TestAssetSearchCachedListfileAndDemuxCLI(t *testing.T) {
	workspace := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(context.Background(), workspace); err != nil {
		t.Fatal(err)
	}
	pin := testkit.CachedAsset(t, workspace, []byte("asset bytes"))
	listfile := records.ListfileRequest{Kind: records.ListfileCommunityCSV, Fetch: records.ListfileFetchFunc(func(context.Context, string, int64) ([]byte, error) {
		return []byte("11;Interface\\Icons\\Test.blp\n12;Other.blp\n"), nil
	})}
	if _, err := records.PrepareListfile(context.Background(), workspace, listfile); err != nil {
		t.Fatal(err)
	}
	result, code := invoke(t, "asset", "search", "--snapshot", pin.ID, "--listfile", "community-csv", "--query", "blp", "--limit", "1", "--offline", "--home", workspace, "--format=json")
	if code != 0 || !result.OK || resultMap(t, result)["truncated"] != true || len(result.Warnings) == 0 {
		t.Fatalf("cached search: %+v (%d)", result, code)
	}
	for _, mode := range [][]string{{"--extension", "blp"}, {"--name", "Interface/Icons/Test.blp"}, {"--file-id", "11"}} {
		args := append([]string{"asset", "search", "--snapshot", pin.ID, "--listfile", "community-csv", "--offline", "--home", workspace, "--format=json"}, mode...)
		found, code := invoke(t, args...)
		if code != 0 || !found.OK || found.Result == nil {
			t.Fatalf("mode %v: %+v (%d)", mode, found, code)
		}
	}
	output := filepath.Join(t.TempDir(), "frames")
	if err := os.Mkdir(output, 0700); err != nil {
		t.Fatal(err)
	}
	sample := filepath.Join("..", "..", "tests", "fixtures", "inputs", "minimal-vp9.avi")
	demux, code := invoke(t, "asset", "demux", "--path", sample, "--output", output, "--home", workspace, "--format=json")
	if code != 0 || !demux.OK || resultMap(t, demux)["schema"] != records.AssetDemuxSchema || len(demux.Captures) < 2 {
		t.Fatalf("local demux: %+v (%d)", demux, code)
	}
	conflict, code := invoke(t, "asset", "demux", "--path", sample, "--output", output, "--home", workspace, "--format=json")
	if code != 3 || conflict.OK || conflict.Error.Code != "records.export_conflict" {
		t.Fatalf("demux conflict: %+v (%d)", conflict, code)
	}
}
