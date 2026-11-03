package publish

import (
	"archive/zip"
	"context"
	"encoding/json"
	"io"
	"path"
	"regexp"
	"sort"
	"strconv"
	"strings"
)

// These are ModUs platform labels, independent of our current target catalog.
// A platform's historical wow_forever label is not a CLI target or CDN code.
type modusGameVersion struct {
	GameVersion string `json:"gameVersion"`
	Server      string `json:"server"`
}

var modusInterfaces = map[string]modusGameVersion{
	"110000": {"11.0.0", "wow_retail"}, "110002": {"11.0.2", "wow_retail"},
	"111000": {"11.1.0", "wow_retail"}, "111005": {"11.1.5", "wow_retail"},
	"120100": {"12.1.0", "wow_retail"}, "120105": {"12.1.5", "wow_retail"},
	"11506": {"Classic Era", "wow_classic_era"}, "11507": {"Classic Era", "wow_classic_era"},
	"11508": {"Classic Era", "wow_classic_era"}, "11509": {"1.15.9", "wow_classic_era"},
	"40401": {"Cataclysm Classic", "wow_classic_cata"}, "40402": {"4.4.2", "wow_classic_cata"},
	"16001": {"1.60.1", "wow_forever"}, "20506": {"2.5.6", "wow_anniversary"},
	"38002": {"3.80.2", "wow_classic_titan"}, "50504": {"5.5.4", "wow_classic"},
}

type modusZIPMetadata struct {
	TOCVersion string             `json:"toc_version"`
	Games      []modusGameVersion `json:"supported_game_versions"`
	Interfaces []string           `json:"interface_values"`
	TOCFiles   []string           `json:"toc_files"`
	Unmapped   []string           `json:"unmapped_interface_values"`
	UnzipSize  uint64             `json:"unzip_size"`
}

var modusFlavorTOC = regexp.MustCompile(`_(?:mainline|vanilla|classic|bcc|tbc|wrath|cata|mists)\.toc$`)

// The caller holds the same artifact handle through inspection and upload.
func inspectModusZIP(ctx context.Context, a artifact) (modusZIPMetadata, error) {
	m := modusZIPMetadata{Games: []modusGameVersion{}, Interfaces: []string{}, TOCFiles: []string{}, Unmapped: []string{}}
	if a.size > 200<<20 {
		return m, invalid("$.file", "ModUs ZIP exceeds 200 MiB")
	}
	z, err := zip.NewReader(a.file, a.size)
	if err != nil {
		return m, invalid("$.file", "invalid ZIP")
	}
	groups := map[string]map[string]bool{}
	seen := map[string]bool{}
	for _, f := range z.File {
		if err := ctx.Err(); err != nil {
			return m, err
		}
		if f.UncompressedSize64 > ^uint64(0)-m.UnzipSize {
			return m, invalid("$.file", "ZIP size overflows")
		}
		m.UnzipSize += f.UncompressedSize64
		n := strings.ToLower(strings.ReplaceAll(f.Name, "\\", "/"))
		if f.FileInfo().IsDir() || !strings.HasSuffix(n, ".toc") {
			continue
		}
		if seen[n] {
			return m, invalid("$.file", "duplicate TOC path")
		}
		seen[n] = true
		library := false
		for _, part := range strings.Split(path.Dir(n), "/") {
			library = library || part == "libs"
		}
		if library {
			continue
		}
		if f.UncompressedSize64 > 1<<20 {
			return m, invalid("$.file", "TOC exceeds 1 MiB")
		}
		r, err := f.Open()
		if err != nil {
			return m, invalid("$.file", "cannot read TOC")
		}
		raw, err := io.ReadAll(io.LimitReader(r, (1<<20)+1))
		r.Close()
		if err != nil || len(raw) > 1<<20 {
			return m, invalid("$.file", "cannot read bounded TOC")
		}
		// Interface syntax is ASCII in both UTF-8 and the supported legacy
		// Windows-1252 TOCs. Other metadata need not be decoded for this field.
		values := map[string]bool{}
		for _, line := range strings.Split(strings.TrimPrefix(string(raw), "\xef\xbb\xbf"), "\n") {
			key, val, ok := strings.Cut(strings.TrimSpace(line), ":")
			if !ok || !strings.EqualFold(strings.TrimSpace(strings.TrimPrefix(key, "##")), "Interface") || !strings.HasPrefix(key, "##") {
				continue
			}
			parts := strings.FieldsFunc(val, func(r rune) bool { return r == ',' || r == ';' || r == ' ' || r == '\t' || r == '\r' })
			if len(parts) == 0 {
				return m, invalid("$.file", "TOC Interface is empty")
			}
			for _, v := range parts {
				if _, err := strconv.ParseUint(v, 10, 32); err != nil || strings.Trim(v, "0123456789") != "" {
					return m, invalid("$.file", "TOC Interface must contain decimal codes")
				}
				if _, ok := modusInterfaces[v]; !ok && v != "30405" {
					return m, invalid("$.file", "unsupported ModUs Interface: "+v)
				}
				values[v] = true
			}
		}
		if len(values) == 0 {
			return m, invalid("$.file", "TOC has no Interface field")
		}
		group := modusFlavorTOC.ReplaceAllString(n, ".toc")
		if groups[group] == nil {
			groups[group] = map[string]bool{}
		}
		for v := range values {
			groups[group][v] = true
		}
		m.TOCFiles = append(m.TOCFiles, f.Name)
	}
	if len(groups) == 0 {
		return m, invalid("$.file", "ZIP contains no addon TOC outside Libs")
	}
	var expected map[string]bool
	for _, values := range groups {
		if expected == nil {
			expected = values
			continue
		}
		if len(values) != len(expected) {
			return m, invalid("$.file", "addon TOC Interface unions disagree")
		}
		for v := range values {
			if !expected[v] {
				return m, invalid("$.file", "addon TOC Interface unions disagree")
			}
		}
	}
	for v := range expected {
		m.Interfaces = append(m.Interfaces, v)
	}
	sort.Slice(m.Interfaces, func(i, j int) bool {
		a, _ := strconv.Atoi(m.Interfaces[i])
		b, _ := strconv.Atoi(m.Interfaces[j])
		return a < b
	})
	sort.Strings(m.TOCFiles)
	games := map[modusGameVersion]bool{}
	for _, v := range m.Interfaces {
		g, ok := modusInterfaces[v]
		if !ok {
			m.Unmapped = append(m.Unmapped, v)
			continue
		}
		if !games[g] {
			m.Games = append(m.Games, g)
			games[g] = true
		}
	}
	m.TOCVersion = strings.Join(m.Interfaces, ",")
	return m, nil
}

// Defaults never silently opt into a newer version offered by the service.
func selectModusGames(derived []modusGameVersion, config any, supplied []modusGameVersion) ([]modusGameVersion, error) {
	rows, ok := config.([]any)
	if !ok {
		return nil, invalid("$.supported_game_versions", "live wow_builds configuration is missing")
	}
	var builds map[string]struct {
		Versions []string `json:"versions"`
	}
	for _, v := range rows {
		row, ok := v.(map[string]any)
		if !ok || row["key"] != "wow_builds" {
			continue
		}
		raw, ok := row["value"].(string)
		if !ok || json.Unmarshal([]byte(raw), &builds) != nil {
			return nil, invalid("$.supported_game_versions", "invalid live wow_builds")
		}
		break
	}
	if len(builds) == 0 {
		return nil, invalid("$.supported_game_versions", "live wow_builds configuration is missing")
	}
	clients := map[string]bool{}
	for _, g := range derived {
		clients[g.Server] = true
	}
	live := map[modusGameVersion]bool{}
	for server, build := range builds {
		if build.Versions == nil {
			return nil, invalid("$.supported_game_versions", "invalid live build versions")
		}
		for _, v := range build.Versions {
			if strings.TrimSpace(v) == "" {
				return nil, invalid("$.supported_game_versions", "invalid live build version")
			}
			if clients[server] {
				live[modusGameVersion{v, server}] = true
			}
		}
	}
	selected := supplied
	if supplied == nil {
		selected = []modusGameVersion{}
		for _, g := range derived {
			if live[g] {
				selected = append(selected, g)
			}
		}
	}
	if len(selected) == 0 {
		return nil, invalid("$.supported_game_versions", "select a nonempty set of live versions on ZIP client branches")
	}
	seen := map[modusGameVersion]bool{}
	for _, g := range selected {
		if !live[g] || seen[g] {
			return nil, invalid("$.supported_game_versions", "version is unavailable, outside ZIP clients or duplicated")
		}
		seen[g] = true
	}
	return selected, nil
}
