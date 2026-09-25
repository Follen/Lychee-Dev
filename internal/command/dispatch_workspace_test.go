package command

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

func initWorkspace(t *testing.T) string {
	t.Helper()
	root := filepath.Join(t.TempDir(), "workspace")
	if result, code := invoke(t, "init", "--home", root, "--format=json"); code != 0 || !result.OK {
		t.Fatalf("init: code=%d response=%+v", code, result)
	}
	return root
}

func TestTargetAddListShowRemoveLifecycle(t *testing.T) {
	root := initWorkspace(t)
	client := t.TempDir()

	listed, code := invoke(t, "target", "list", "--home", root, "--format=json")
	if code != 0 || !listed.OK {
		t.Fatalf("empty list: code=%d response=%+v", code, listed)
	}
	if targets, ok := listed.Result.(map[string]any)["targets"].([]any); !ok || len(targets) != 0 {
		t.Fatalf("empty list = %#v", listed.Result)
	}

	added, code := invoke(t, "target", "add", "retail-cn", "--product", "retail", "--region", "cn", "--locale", "zhCN", "--installation", client, "--home", root, "--format=json")
	if code != 0 || !added.OK {
		t.Fatalf("add: code=%d response=%+v", code, added)
	}
	config := added.Result.(map[string]any)
	if config["source"] != "installation" || config["installation"] != client || config["name"] != "retail-cn" {
		t.Fatalf("add result = %#v", config)
	}

	conflict, code := invoke(t, "target", "add", "retail-cn", "--product", "retail", "--region", "cn", "--locale", "zhCN", "--installation", client, "--home", root, "--format=json")
	if code != 3 || conflict.OK || conflict.Error.Code != "selection.target_exists" {
		t.Fatalf("conflict: code=%d response=%+v", code, conflict)
	}
	replaced, code := invoke(t, "target", "add", "retail-cn", "--product", "classic", "--region", "cn", "--locale", "zhCN", "--remote", "--replace", "--build", "5.5.4.12345", "--home", root, "--format=json")
	if code != 0 || !replaced.OK {
		t.Fatalf("replace: code=%d response=%+v", code, replaced)
	}
	if config = replaced.Result.(map[string]any); config["product"] != "classic" || config["source"] != "remote" || config["fullBuild"] != "5.5.4.12345" {
		t.Fatalf("replace result = %#v", config)
	}

	second, code := invoke(t, "target", "add", "beta", "--product", "retail", "--region", "us", "--locale", "enUS", "--remote", "--home", root, "--format=json")
	if code != 0 {
		t.Fatalf("second add: %d %+v", code, second)
	}
	listed, code = invoke(t, "target", "list", "--home", root, "--format=json")
	if code != 0 || !listed.OK {
		t.Fatalf("list: code=%d response=%+v", code, listed)
	}
	targets := listed.Result.(map[string]any)["targets"].([]any)
	if len(targets) != 2 || targets[0].(map[string]any)["name"] != "beta" {
		t.Fatalf("list = %#v", targets)
	}

	// Doctor reports the remote target's unknown currency without failing:
	// offline state never fakes remote freshness.
	doctored, code := invoke(t, "doctor", "--offline", "--home", root, "--format=json")
	if code != 0 || !doctored.OK || doctored.Result.(map[string]any)["healthy"] != true {
		t.Fatalf("doctor with remote target: code=%d response=%+v", code, doctored)
	}

	removed, code := invoke(t, "target", "remove", "retail", "--home", root, "--format=json")
	if code != 0 || !removed.OK {
		t.Fatalf("remove by prefix: code=%d response=%+v", code, removed)
	}
	if report := removed.Result.(map[string]any); report["name"] != "retail-cn" {
		t.Fatalf("remove report = %#v", report)
	}
	again, code := invoke(t, "target", "remove", "retail-cn", "--home", root, "--format=json")
	if code != 3 || again.OK || again.Error.Code != "selection.target_missing" {
		t.Fatalf("remove missing: code=%d response=%+v", code, again)
	}

	// An active operation reference blocks removal so running work survives.
	if err := selection.RegisterTargetOperation(context.Background(), root, "beta", "OP-1"); err != nil {
		t.Fatal(err)
	}
	blocked, code := invoke(t, "target", "remove", "beta", "--home", root, "--format=json")
	if code != 3 || blocked.OK || blocked.Error.Code != "selection.target_in_use" {
		t.Fatalf("remove in-use: code=%d response=%+v", code, blocked)
	}
	if err := selection.ReleaseTargetOperation(context.Background(), root, "beta", "OP-1"); err != nil {
		t.Fatal(err)
	}
	freed, code := invoke(t, "target", "remove", "beta", "--home", root, "--format=json")
	if code != 0 || !freed.OK {
		t.Fatalf("remove released: code=%d response=%+v", code, freed)
	}
}

func TestTargetAvailableRequiresRegionAndRefusesOffline(t *testing.T) {
	offline, code := invoke(t, "target", "available", "--region", "cn", "--offline", "--format=json")
	if code != 3 || offline.OK || offline.Error.Code != "selection.listing_requires_network" {
		t.Fatalf("offline listing: code=%d response=%+v", code, offline)
	}
	// The listing never pretends a remembered observation is current.
	if offline.Result != nil {
		t.Fatalf("offline listing result = %#v", offline.Result)
	}
}

func TestCacheVerbsAccountVerifyAndPrune(t *testing.T) {
	root := initWorkspace(t)
	first := writeCacheObject(t, root, []byte("lychee cache fixture"))
	second := writeCacheObject(t, root, []byte("second ordinary object"))
	protected := writeCacheObject(t, root, []byte("protected payload"))
	writeRetainMarker(t, root, protected, "pinned", "PIN-fixture")

	status, code := invoke(t, "cache", "status", "--home", root, "--format=json")
	if code != 0 || !status.OK {
		t.Fatalf("status: code=%d response=%+v", code, status)
	}
	report := status.Result.(map[string]any)
	if report["objectCount"] != float64(3) || report["protectedCount"] != float64(1) {
		t.Fatalf("status = %#v", report)
	}
	if report["limitBytes"] != float64(20<<30) || report["withinLimit"] != true {
		t.Fatalf("status limit = %#v", report)
	}

	// Dry run accounts the reclaim without deleting anything.
	dry, code := invoke(t, "cache", "prune", "--target-bytes", "0", "--dry-run", "--home", root, "--format=json")
	if code != 0 || !dry.OK {
		t.Fatalf("dry prune: code=%d response=%+v", code, dry)
	}
	dryReport := dry.Result.(map[string]any)
	if dryReport["removedObjects"] != float64(2) || dryReport["dryRun"] != true || dryReport["skippedProtected"] != float64(1) {
		t.Fatalf("dry prune = %#v", dryReport)
	}
	if _, err := os.Stat(cacheObjectPath(root, first)); err != nil {
		t.Fatal("dry run deleted an object", err)
	}

	// An object cap stops the pass early and reports truncation.
	capped, code := invoke(t, "cache", "prune", "--target-bytes", "0", "--max-objects", "1", "--home", root, "--format=json")
	if code != 0 || !capped.OK {
		t.Fatalf("capped prune: code=%d response=%+v", code, capped)
	}
	cappedReport := capped.Result.(map[string]any)
	if cappedReport["removedObjects"] != float64(1) || cappedReport["truncated"] != true || cappedReport["complete"] != false {
		t.Fatalf("capped prune = %#v", cappedReport)
	}
	remaining := first
	if _, err := os.Stat(cacheObjectPath(root, first)); os.IsNotExist(err) {
		remaining = second
	} else if err != nil {
		t.Fatal(err)
	}
	for _, digest := range []string{first, second} {
		if digest == remaining {
			continue
		}
		if _, err := os.Stat(cacheObjectPath(root, digest)); !os.IsNotExist(err) {
			t.Fatal("capped prune removed more than the cap", err)
		}
	}
	if _, err := os.Stat(cacheObjectPath(root, protected)); err != nil {
		t.Fatal("protected object was deleted", err)
	}

	// The uncapped pass reclaims the rest of the ordinary cache and skips the
	// protected object; complete stays false because protected bytes remain.
	bounded, code := invoke(t, "cache", "prune", "--target-bytes", "0", "--home", root, "--format=json")
	if code != 0 || !bounded.OK {
		t.Fatalf("prune: code=%d response=%+v", code, bounded)
	}
	pruneReport := bounded.Result.(map[string]any)
	if pruneReport["removedObjects"] != float64(1) || pruneReport["skippedProtected"] != float64(1) || pruneReport["complete"] != false {
		t.Fatalf("prune = %#v", pruneReport)
	}
	if _, err := os.Stat(cacheObjectPath(root, remaining)); !os.IsNotExist(err) {
		t.Fatal("ordinary object survived prune", err)
	}
	if _, err := os.Stat(cacheObjectPath(root, protected)); err != nil {
		t.Fatal("protected object was deleted", err)
	}

	verified, code := invoke(t, "cache", "verify", "--home", root, "--format=json")
	if code != 0 || !verified.OK {
		t.Fatalf("verify: code=%d response=%+v", code, verified)
	}
	if verified.Result.(map[string]any)["ok"] != true {
		t.Fatalf("verify = %#v", verified.Result)
	}

	// Corrupting the retained object makes verify fail with the integrity
	// fault while still reporting the exact object.
	corrupt := cacheObjectPath(root, protected)
	if err := os.WriteFile(corrupt, []byte("tampered!!"), 0600); err != nil {
		t.Fatal(err)
	}
	failed, code := invoke(t, "cache", "verify", "--home", root, "--format=json")
	if code != 4 || failed.OK || failed.Error.Code != "vault.cache_integrity" {
		t.Fatalf("corrupt verify: code=%d response=%+v", code, failed)
	}
	if failed.Result == nil || len(failed.Result.(map[string]any)["corrupt"].([]any)) != 1 {
		t.Fatalf("corrupt verify report = %#v", failed.Result)
	}

	// config set raises the budget that cache status reports.
	if _, code := invoke(t, "config", "set", "--cache-max-bytes", "1024", "--home", root, "--format=json"); code != 0 {
		t.Fatal("config set failed")
	}
	status, code = invoke(t, "cache", "status", "--home", root, "--format=json")
	if code != 0 || status.Result.(map[string]any)["limitBytes"] != float64(1024) {
		t.Fatalf("status after config set = %#v", status.Result)
	}
}

func TestConfigShowDefaultsAndSet(t *testing.T) {
	root := initWorkspace(t)
	shown, code := invoke(t, "config", "show", "--home", root, "--format=json")
	if code != 0 || !shown.OK {
		t.Fatalf("show: code=%d response=%+v", code, shown)
	}
	config := shown.Result.(map[string]any)
	if config["schema"] != "lycheedev.config.v1" || config["cache"].(map[string]any)["maxBytes"] != float64(20<<30) {
		t.Fatalf("default config = %#v", config)
	}

	set, code := invoke(t, "config", "set", "--cache-max-bytes", "4096", "--download-workers", "8", "--home", root, "--format=json")
	if code != 0 || !set.OK {
		t.Fatalf("set: code=%d response=%+v", code, set)
	}
	config = set.Result.(map[string]any)
	if config["cache"].(map[string]any)["maxBytes"] != float64(4096) || config["download"].(map[string]any)["workers"] != float64(8) {
		t.Fatalf("set result = %#v", config)
	}
	shown, _ = invoke(t, "config", "show", "--home", root, "--format=json")
	if shown.Result.(map[string]any)["download"].(map[string]any)["workers"] != float64(8) {
		t.Fatalf("set did not persist: %#v", shown.Result)
	}

	// Unset fields survive a partial update.
	if _, code := invoke(t, "config", "set", "--download-workers", "2", "--home", root, "--format=json"); code != 0 {
		t.Fatal("partial set failed")
	}
	shown, _ = invoke(t, "config", "show", "--home", root, "--format=json")
	config = shown.Result.(map[string]any)
	if config["cache"].(map[string]any)["maxBytes"] != float64(4096) || config["download"].(map[string]any)["workers"] != float64(2) {
		t.Fatalf("partial update lost a field: %#v", config)
	}
}

func TestDoctorReportsFreshWorkspaceHealthyAndLegacyRootUnhealthy(t *testing.T) {
	fresh := initWorkspace(t)
	doctored, code := invoke(t, "doctor", "--offline", "--home", fresh, "--format=json")
	if code != 0 || !doctored.OK {
		t.Fatalf("fresh doctor: code=%d response=%+v", code, doctored)
	}
	checks := doctorChecks(t, doctored)
	if doctored.Result.(map[string]any)["healthy"] != true {
		t.Fatalf("fresh workspace unhealthy: %#v", checks)
	}
	if _, ok := checks["vault.workspace"]; !ok {
		t.Fatalf("doctor lost the workspace check: %#v", checks)
	}

	legacy := filepath.Join(t.TempDir(), "legacy")
	if err := os.Mkdir(legacy, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(legacy, "config.json"), []byte("old user data"), 0600); err != nil {
		t.Fatal(err)
	}
	doctored, code = invoke(t, "doctor", "--offline", "--home", legacy, "--format=json")
	if code != 3 || doctored.OK || doctored.Error.Code != "doctor.check_failed" {
		t.Fatalf("legacy doctor: code=%d response=%+v", code, doctored)
	}
	workspace := doctorChecks(t, doctored)["vault.workspace"].(map[string]any)
	if workspace["status"] != "error" || workspace["code"] != "vault.legacy_detected" {
		t.Fatalf("legacy workspace check = %#v", workspace)
	}
	// The doctor never touches legacy contents.
	data, err := os.ReadFile(filepath.Join(legacy, "config.json"))
	if err != nil || string(data) != "old user data" {
		t.Fatalf("legacy changed: %q %v", data, err)
	}

	// A missing workspace is an environment failure, reported precisely.
	absent := filepath.Join(t.TempDir(), "absent")
	doctored, code = invoke(t, "doctor", "--home", absent, "--format=json")
	if code != 3 || doctored.OK {
		t.Fatalf("absent doctor: code=%d response=%+v", code, doctored)
	}
	if got := doctorChecks(t, doctored)["vault.workspace"].(map[string]any)["code"]; got != "vault.workspace_missing" {
		t.Fatalf("absent workspace check = %#v", got)
	}
}

func TestInitVariantsPlanFreshAndResume(t *testing.T) {
	base := t.TempDir()
	empty := filepath.Join(base, "planned")
	if err := os.Mkdir(empty, 0700); err != nil {
		t.Fatal(err)
	}
	plan, code := invoke(t, "init", "--plan", "--home", empty, "--format=json")
	if code != 0 || !plan.OK {
		t.Fatalf("plan: code=%d response=%+v", code, plan)
	}
	if plan.Result.(map[string]any)["state"] != "planned" {
		t.Fatalf("plan = %#v", plan.Result)
	}
	// Planning changes nothing on disk.
	if entries, err := os.ReadDir(empty); err != nil || len(entries) != 0 {
		t.Fatalf("plan touched the root: %v %v", entries, err)
	}

	// A plain init into the same root still works and stays plain.
	plain, code := invoke(t, "init", "--home", empty, "--format=json")
	if code != 0 || !plain.OK {
		t.Fatalf("plain init: code=%d response=%+v", code, plain)
	}
	exists, code := invoke(t, "init", "--fresh", "--home", empty, "--format=json")
	if code != 3 || exists.OK || exists.Error.Code != "vault.workspace_exists" {
		t.Fatalf("fresh over workspace: code=%d response=%+v", code, exists)
	}

	// Fresh init isolates an occupied legacy root into the sibling archive.
	legacy := filepath.Join(base, "legacy")
	if err := os.Mkdir(legacy, 0700); err != nil {
		t.Fatal(err)
	}
	sentinel := filepath.Join(legacy, "state")
	if err := os.WriteFile(sentinel, []byte("old"), 0600); err != nil {
		t.Fatal(err)
	}
	fresh, code := invoke(t, "init", "--fresh", "--home", legacy, "--format=json")
	if code != 0 || !fresh.OK {
		t.Fatalf("fresh: code=%d response=%+v", code, fresh)
	}
	result := fresh.Result.(map[string]any)
	if result["plan"].(map[string]any)["state"] != "complete" {
		t.Fatalf("fresh plan = %#v", result["plan"])
	}
	archive := result["plan"].(map[string]any)["archivePath"].(string)
	if _, err := os.Stat(filepath.Join(archive, "state")); err != nil {
		t.Fatalf("legacy data not isolated intact: %v", err)
	}
	if _, err := os.Stat(filepath.Join(legacy, "workspace.json")); err != nil {
		t.Fatalf("fresh workspace missing: %v", err)
	}
	// Repeating a completed switch stays idempotent rather than archiving the
	// new workspace.
	repeat, code := invoke(t, "init", "--fresh", "--home", legacy, "--format=json")
	if code != 0 || !repeat.OK {
		t.Fatalf("fresh repeat: code=%d response=%+v", code, repeat)
	}

	// Resume without a journal is refused precisely.
	nowhere := filepath.Join(base, "nowhere")
	resumed, code := invoke(t, "init", "--resume", "--home", nowhere, "--format=json")
	if code != 3 || resumed.OK || resumed.Error.Code != "vault.archive_state" {
		t.Fatalf("resume without journal: code=%d response=%+v", code, resumed)
	}

	// A recorded planned switch resumes: the legacy root moves, the workspace
	// is created, and the journal completes.
	interrupted := filepath.Join(base, "interrupted")
	if err := os.Mkdir(interrupted, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(interrupted, "automation.py"), []byte("old"), 0600); err != nil {
		t.Fatal(err)
	}
	writeArchiveJournal(t, interrupted, "planned")
	resumed, code = invoke(t, "init", "--resume", "--home", interrupted, "--format=json")
	if code != 0 || !resumed.OK {
		t.Fatalf("resume: code=%d response=%+v", code, resumed)
	}
	if resumed.Result.(map[string]any)["plan"].(map[string]any)["state"] != "complete" {
		t.Fatalf("resume = %#v", resumed.Result)
	}
	if _, err := os.Stat(filepath.Join(interrupted, "workspace.json")); err != nil {
		t.Fatalf("resumed workspace missing: %v", err)
	}
}

func writeArchiveJournal(t *testing.T, root, state string) {
	t.Helper()
	absolute, err := filepath.Abs(root)
	if err != nil {
		t.Fatal(err)
	}
	journalPath := filepath.Join(filepath.Dir(absolute), filepath.Base(absolute)+".archive-state.json")
	plan := map[string]any{
		"schema":      "lycheedev.archive.v1",
		"root":        absolute,
		"archivePath": filepath.Join(filepath.Dir(absolute), filepath.Base(absolute)+"-legacy-20260101T000000Z"),
		"journalPath": journalPath,
		"markers":     []string{"automation.py"},
		"state":       state,
		"createdAt":   time.Now().UTC().Format(time.RFC3339),
		"steps": []map[string]string{
			{"id": "journal", "detail": "record the resolved archive plan"},
			{"id": "move", "detail": "isolate the legacy root"},
			{"id": "create", "detail": "create new-format workspace"},
		},
	}
	raw, err := json.MarshalIndent(plan, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(journalPath, raw, 0600); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { os.Remove(journalPath) })
}

func writeCacheObject(t *testing.T, root string, payload []byte) string {
	t.Helper()
	sum := sha256.Sum256(payload)
	digest := hex.EncodeToString(sum[:])
	directory := filepath.Join(root, "cache", "objects", digest[:2])
	if err := os.MkdirAll(directory, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(directory, digest[2:]), payload, 0600); err != nil {
		t.Fatal(err)
	}
	return digest
}

func cacheObjectPath(root, digest string) string {
	return filepath.Join(root, "cache", "objects", digest[:2], digest[2:])
}

func writeRetainMarker(t *testing.T, root, digest, kind, owner string) {
	t.Helper()
	marker := map[string]any{
		"sha256": digest,
		"owners": []map[string]any{{"kind": kind, "owner": owner, "retainedAt": time.Now().UTC().Format(time.RFC3339)}},
	}
	raw, err := json.MarshalIndent(marker, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	directory := filepath.Join(root, "cache", "retain")
	if err := os.MkdirAll(directory, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(directory, digest+".json"), raw, 0600); err != nil {
		t.Fatal(err)
	}
}

func doctorChecks(t *testing.T, response Envelope) map[string]any {
	t.Helper()
	checks, ok := response.Result.(map[string]any)["checks"].([]any)
	if !ok {
		t.Fatalf("doctor checks = %#v", response.Result)
	}
	byID := map[string]any{}
	for _, value := range checks {
		check := value.(map[string]any)
		byID[check["id"].(string)] = check
	}
	return byID
}

func TestEvidenceListReportsArchivedCaptures(t *testing.T) {
	root := initWorkspace(t)
	if _, err := vault.WriteMetadata(context.Background(), root, func(s *vault.Store, m *vault.Metadata) (struct{}, error) {
		_, err := evidence.OpenArchive(s, m).CommitCapture(context.Background(), evidence.CaptureDraft{
			Reader: strings.NewReader("capture payload"), MaxBytes: 1 << 20, MediaType: "text/plain",
			Provenance: evidence.Provenance{Kind: "fixture", Locator: "dispatch_workspace_test"}, Complete: true,
		})
		return struct{}{}, err
	}); err != nil {
		t.Fatal(err)
	}
	listed, code := invoke(t, "evidence", "list", "--home", root, "--format=json")
	if code != 0 || !listed.OK {
		t.Fatalf("evidence list: code=%d response=%+v", code, listed)
	}
	captures := listed.Result.(map[string]any)["captures"].([]any)
	if len(captures) != 1 || !strings.HasPrefix(captures[0].(map[string]any)["id"].(string), "CAP-") {
		t.Fatalf("captures = %#v", captures)
	}
}

func TestEvidenceKeepAndRemoveCommandsAreAuditable(t *testing.T) {
	root := initWorkspace(t)
	var ref evidence.CaptureRef
	if _, err := vault.WriteMetadata(context.Background(), root, func(s *vault.Store, m *vault.Metadata) (struct{}, error) {
		var err error
		ref, err = evidence.OpenArchive(s, m).CommitCapture(context.Background(), evidence.CaptureDraft{
			Reader: strings.NewReader("lifecycle"), MaxBytes: 1 << 20, MediaType: "text/plain",
			Provenance: evidence.Provenance{Kind: "fixture", Locator: "dispatch_lifecycle"}, Complete: true,
		})
		return struct{}{}, err
	}); err != nil {
		t.Fatal(err)
	}
	kept, code := invoke(t, "evidence", "keep", ref.ID, "--home", root, "--format=json")
	if code != 0 || !kept.OK || resultMap(t, kept)["created"] != true {
		t.Fatalf("keep: code=%d response=%+v error=%+v", code, kept, kept.Error)
	}
	removed, code := invoke(t, "evidence", "remove", ref.ID, "--home", root, "--format=json")
	if code != 0 || !removed.OK || resultMap(t, removed)["manifestRemoved"] != true || resultMap(t, removed)["retentionRemoved"] != true {
		t.Fatalf("remove: code=%d response=%+v", code, removed)
	}
}
