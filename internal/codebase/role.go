package codebase

import "strings"

// pathRole classifies an indexed file by its repository path. The rule mirrors
// the legacy source-research role taxonomy so role-penalty ranking stays
// comparable across tools: generated API metadata, locale data, vendored
// libraries, generated data and tooling are distinct from project code.
func pathRole(value string) string {
	p := strings.ToLower(strings.ReplaceAll(value, "\\", "/"))
	switch {
	case strings.Contains(p, "apidocumentationgenerated"):
		return "official-generated-api"
	case strings.Contains(p, "locale") || strings.Contains(p, "localization"):
		return "locale"
	case strings.Contains(p, "libs/") || strings.Contains(p, "vendor/"):
		return "vendor"
	case strings.Contains(p, "modelpaths") || strings.Contains(p, "generated"):
		return "generated-data"
	case strings.Contains(p, "tools/") || strings.HasSuffix(p, "babelfish.lua"):
		return "tool"
	default:
		return "project"
	}
}

// roleRankPenalty demotes off-topic roles during ranking: vendored code is
// penalized 15, locale/generated-data/tooling 20, project code and official
// generated API metadata are never penalized.
func roleRankPenalty(role string) int {
	switch role {
	case "vendor":
		return 15
	case "locale", "generated-data", "tool":
		return 20
	default:
		return 0
	}
}
