package selection

import (
	"context"
	"fmt"
	"os"
	"strings"

	"github.com/follenfang/lycheedev/internal/vault"
)

// SelectionChecks returns read-only health checks for product catalog facts
// and named target configurations. Remote freshness is never faked: it is only
// probed through the bounded availability listing elsewhere, so these checks
// stay offline and report "unknown" for remote currency.
func SelectionChecks(ctx context.Context, root string) []vault.Check {
	checks := make([]vault.Check, 0, 2)
	checks = append(checks, catalogCheck())
	configs, err := ListTargets(ctx, root)
	if err != nil {
		checks = append(checks, vault.Check{
			ID: "selection.named_targets", OK: false, Status: "error", Code: "selection.target_format",
			Detail: err.Error(), NextStep: "run target list against a valid workspace root",
		})
		return checks
	}
	checks = append(checks, namedTargetsCheck(ctx, configs))
	return checks
}

func catalogCheck() vault.Check {
	baselines := VerifiedClientBaselines()
	problems := make([]string, 0)
	products := map[string]bool{}
	codes := map[string]bool{}
	for _, baseline := range baselines {
		if baseline.Product == "" || products[baseline.Product] {
			problems = append(problems, "duplicate or empty product "+baseline.Product)
		}
		products[baseline.Product] = true
		if baseline.ProductCode == "" || codes[baseline.ProductCode] {
			problems = append(problems, "duplicate or empty productCode "+baseline.ProductCode)
		}
		codes[baseline.ProductCode] = true
		if !digest(baseline.SourceCommit, 40) {
			problems = append(problems, "invalid sourceCommit for "+baseline.Product)
		}
		if baseline.Interface <= 0 {
			problems = append(problems, "invalid interface for "+baseline.Product)
		}
		if !strings.HasSuffix(baseline.TOC, ".toc") {
			problems = append(problems, "invalid toc for "+baseline.Product)
		}
		if !build(baseline.BuildSeries + ".0") {
			problems = append(problems, "invalid buildSeries for "+baseline.Product)
		}
	}
	if len(baselines) == 0 {
		problems = append(problems, "empty baseline catalog")
	}
	if len(problems) > 0 {
		return vault.Check{
			ID: "selection.catalog", OK: false, Status: "error", Code: "selection.catalog_invalid",
			Detail:   strings.Join(problems, "; "),
			NextStep: "regenerate or repair the verified client baselines before resolving targets",
		}
	}
	return vault.Check{
		ID: "selection.catalog", OK: true, Status: "ok",
		Detail: fmt.Sprintf("%d verified client baselines are self-consistent", len(baselines)),
	}
}

func namedTargetsCheck(ctx context.Context, configs []TargetConfig) vault.Check {
	if len(configs) == 0 {
		return vault.Check{
			ID: "selection.named_targets", OK: true, Status: "ok",
			Detail: "no named target configurations",
		}
	}
	problems := make([]string, 0)
	unverified := make([]string, 0)
	for _, config := range configs {
		if err := config.Validate(); err != nil {
			problems = append(problems, config.Name+": "+err.Error())
			continue
		}
		switch config.Source {
		case TargetSourceInstallation:
			if info, err := os.Lstat(config.Installation); err != nil || !info.IsDir() {
				problems = append(problems, config.Name+": installation directory is missing ("+config.Installation+")")
			}
		case TargetSourceRemote:
			unverified = append(unverified, config.Name)
		}
		if err := ctx.Err(); err != nil {
			break
		}
	}
	if len(problems) > 0 {
		return vault.Check{
			ID: "selection.named_targets", OK: false, Status: "error", Code: "selection.target_unresolvable",
			Detail:   strings.Join(problems, "; "),
			NextStep: "fix or re-add the named target (target add replaces it explicitly)",
		}
	}
	detail := fmt.Sprintf("%d named target(s) validate and resolve locally", len(configs))
	if len(unverified) > 0 {
		// Remote currency is intentionally unknown without a bounded network
		// check; a fake "fresh" status would violate the offline contract.
		detail += "; remote currency unknown offline for " + strings.Join(unverified, ", ")
	}
	return vault.Check{
		ID: "selection.named_targets", OK: true, Status: "ok", Code: "",
		Detail:   detail,
		NextStep: "run target resolve (or the remote availability listing) to observe current remote releases",
		Data:     map[string]any{"remote": unverified},
	}
}
