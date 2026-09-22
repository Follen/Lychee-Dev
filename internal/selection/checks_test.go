package selection

import (
	"context"
	"strings"
	"testing"
)

func TestSelectionChecksReportCatalogAndTargets(t *testing.T) {
	ctx := context.Background()
	root := newTestWorkspace(t)

	checks := SelectionChecks(ctx, root)
	if len(checks) != 2 || !checks[0].OK || checks[0].ID != "selection.catalog" {
		t.Fatalf("checks = %+v", checks)
	}
	if checks[1].ID != "selection.named_targets" || !checks[1].OK {
		t.Fatalf("checks = %+v", checks[1])
	}

	installation := t.TempDir()
	local := testTarget("local-retail")
	local.Source = TargetSourceInstallation
	local.Installation = installation
	if _, err := PutTarget(ctx, root, local, StoreTargetOptions{}); err != nil {
		t.Fatal(err)
	}
	if _, err := PutTarget(ctx, root, testTarget("remote-retail"), StoreTargetOptions{}); err != nil {
		t.Fatal(err)
	}
	checks = SelectionChecks(ctx, root)
	targets := checks[1]
	if !targets.OK || !strings.Contains(targets.Detail, "remote currency unknown") {
		t.Fatalf("named target check = %+v", targets)
	}

	// A vanished installation is reported precisely, never faked as fine.
	broken := testTarget("broken-retail")
	broken.Source = TargetSourceInstallation
	broken.Installation = installation + "-missing"
	if _, err := PutTarget(ctx, root, broken, StoreTargetOptions{}); err != nil {
		t.Fatal(err)
	}
	checks = SelectionChecks(ctx, root)
	if checks[1].OK || checks[1].Code != "selection.target_unresolvable" || checks[1].NextStep == "" {
		t.Fatalf("broken installation check = %+v", checks[1])
	}
}
