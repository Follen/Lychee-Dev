package addoncheck

import (
	"strings"
	"unicode"
)

// LoadContext describes the selected source client's ordinary in-world load.
// Empty fields remain unknown; manifest names and version numbers do not
// establish a different client. Original TOC/XML references remain unmodified.
type LoadContext struct {
	Family string `json:"family,omitempty"`
	Game   string `json:"game,omitempty"`
}

func (c LoadContext) condition(key, value string) (include, known bool) {
	switch strings.ToLower(key) {
	case "allowload":
		switch strings.ToLower(strings.TrimSpace(value)) {
		case "game", "both":
			return true, true
		case "glue":
			return false, true
		default:
			return false, false
		}
	case "allowloadgametype", "excludeloadgametype":
		tags := strings.FieldsFunc(strings.ToLower(value), func(r rune) bool { return r == ',' || unicode.IsSpace(r) })
		known = len(tags) > 0
		for _, tag := range tags {
			var match, resolved bool
			switch tag {
			case "mainline", "classic":
				match, resolved = strings.EqualFold(c.Family, tag), c.Family != ""
			case "standard":
				match, resolved = c.Game != "" && !strings.EqualFold(c.Game, "Camelot"), c.Game != ""
			case "plunderstorm", "wowhack":
				match, resolved = false, true
			case "vanilla", "tbc", "wrath", "cata", "mists", "camelot":
				match, resolved = strings.EqualFold(c.Game, tag), c.Game != ""
			}
			if match && resolved {
				include, known = true, true
				break
			}
			known = known && resolved
		}
		if strings.EqualFold(key, "ExcludeLoadGameType") {
			include = !include
		}
		return include, known
	default:
		return false, false
	}
}

func (c LoadContext) target(raw string) (target string, include, known bool) {
	target, include, known = strings.TrimSpace(raw), true, true
	for strings.HasSuffix(target, "]") {
		start := strings.LastIndex(target, "[")
		if start <= 0 || !unicode.IsSpace(rune(target[start-1])) {
			break
		}
		fields := strings.Fields(target[start+1 : len(target)-1])
		if len(fields) == 0 {
			known = false
		} else {
			allow, resolved := c.condition(strings.TrimSuffix(fields[0], ":"), strings.Join(fields[1:], " "))
			if resolved && !allow {
				include = false
			}
			known = known && resolved
		}
		target = strings.TrimSpace(target[:start])
	}
	// A known false term excludes the line even if another term is unknown.
	if !include {
		return target, false, true
	}
	for token, value := range map[string]string{"[Family]": c.Family, "[Game]": c.Game} {
		if strings.Contains(target, token) {
			if value == "" {
				known = false
			} else {
				target = strings.ReplaceAll(target, token, value)
			}
		}
	}
	if strings.ContainsAny(target, "[]") {
		known = false
	}
	return target, include, known
}
