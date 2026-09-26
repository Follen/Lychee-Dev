package delivery

import (
	"context"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/codebase"
	"github.com/follenfang/lycheedev/internal/selection"
	"io"
	"os"
	"path"
	"path/filepath"
	"strconv"
	"strings"
)

type AddonManifest struct {
	Product     string   `json:"product"`
	TOC         string   `json:"toc"`
	Interface   int      `json:"interface"`
	Loads       []string `json:"loads"`
	LoadedFiles []string `json:"loadedFiles"`
}

// InspectAddonRelease verifies release bytes and the four shipped TOC contracts.
// It checks recursive XML loads and Lua syntax, not Lua execution safety,
// runtime readiness, or the installed client's identity. Those are separate gates.
func InspectAddonRelease(ctx context.Context, directory, version string) ([]AddonManifest, error) {
	release, err := InspectRelease(ctx, directory, version)
	if err != nil {
		return nil, err
	}
	files := make(map[string]Resource)
	for _, resource := range release.Resources {
		files[resource.Path] = resource
	}
	root, err := os.OpenRoot(filepath.Join(directory, "payload"))
	if err != nil {
		return nil, err
	}
	defer root.Close()
	if _, exists := files["addon/Bridge/Definitions.lua"]; exists {
		file, err := root.Open("addon/Bridge/Definitions.lua")
		if err != nil {
			return nil, err
		}
		definitions, parseErr := bridge.DecodeProbeQueue(file)
		closeErr := file.Close()
		if parseErr != nil || len(definitions) != 0 {
			return nil, fmt.Errorf("%w: release probe queue must be canonical and empty", ErrPayload)
		}
		if closeErr != nil {
			return nil, closeErr
		}
	}
	var result []AddonManifest
	name := "addon/" + selection.MainTOC
	record, ok := files[name]
	if !ok || record.Bytes > 1<<20 {
		return nil, fmt.Errorf("%w: TOC size or presence %s", ErrPayload, name)
	}
	file, err := root.Open(name)
	if err != nil {
		return nil, err
	}
	raw, readErr := io.ReadAll(io.LimitReader(file, (1<<20)+1))
	closeErr := file.Close()
	if readErr != nil {
		return nil, readErr
	}
	if closeErr != nil {
		return nil, closeErr
	}
	if len(raw) > 1<<20 {
		return nil, ErrPayload
	}
	facts, err := codebase.AnalyzeDocument(ctx, name, raw)
	if err != nil {
		return nil, err
	}
	headers := make(map[string]string)
	for _, header := range facts.Headers {
		key := strings.ToLower(header.Key)
		if _, exists := headers[key]; exists {
			return nil, fmt.Errorf("%w: duplicate TOC header %s in %s", ErrPayload, key, name)
		}
		headers[key] = header.Value
	}
	// One manifest declares every supported engine; the runtime client gate
	// selects the product from the observed build. A deployment target never
	// re-spells the manifest: per-client TOC variants are exactly the engine
	// selection rule this architecture removed. Version and storage headers
	// stay release-wide invariants.
	declared := map[string]bool{}
	for _, part := range strings.Split(headers["interface"], ",") {
		declared[strings.TrimSpace(part)] = true
	}
	baselineInterfaces := map[string]bool{}
	for _, baseline := range selection.VerifiedClientBaselines() {
		baselineInterfaces[strconv.Itoa(baseline.Interface)] = true
	}
	for declaredInterface := range declared {
		if !baselineInterfaces[declaredInterface] {
			return nil, fmt.Errorf("%w: TOC declares unsupported interface %s in %s", ErrPayload, declaredInterface, name)
		}
	}
	for baselineInterface := range baselineInterfaces {
		if !declared[baselineInterface] {
			return nil, fmt.Errorf("%w: TOC %s must declare every supported interface; %s is missing", ErrPayload, name, baselineInterface)
		}
	}
	if headers["version"] != version || headers["savedvariables"] != "LycheeToolkitDB" || headers["savedvariablespercharacter"] != "" {
		return nil, fmt.Errorf("%w: TOC identity/version/storage mismatch in %s", ErrPayload, name)
	}
	if len(facts.Loads) == 0 {
		return nil, fmt.Errorf("%w: empty TOC %s", ErrPayload, name)
	}
	first := facts.Loads[0].Path
	if !strings.EqualFold(strings.ReplaceAll(first, "\\", "/"), "Core/ClientGate.lua") {
		return nil, fmt.Errorf("%w: %s must load Core/ClientGate.lua first, got %q", ErrPayload, name, first)
	}
	manifest := AddonManifest{Product: "multi", TOC: selection.MainTOC, Interface: 0, Loads: []string{}}
	seen := make(map[string]bool)
	for _, load := range facts.Loads {
		ref := strings.ReplaceAll(load.Path, "\\", "/")
		if ref == "" || path.IsAbs(ref) || strings.ContainsAny(ref, ":\x00") || path.Clean(ref) != ref || ref == ".." || strings.HasPrefix(ref, "../") {
			return nil, fmt.Errorf("%w: invalid TOC load %q", ErrPayload, load.Path)
		}
		ext := strings.ToLower(path.Ext(ref))
		if ext != ".lua" && ext != ".xml" {
			return nil, fmt.Errorf("%w: unsupported TOC load %q", ErrPayload, ref)
		}
		if _, exists := files["addon/"+ref]; !exists || seen[strings.ToLower(ref)] {
			return nil, fmt.Errorf("%w: MISSING/REPEATED TOC load %q", ErrPayload, ref)
		}
		seen[strings.ToLower(ref)] = true
		manifest.Loads = append(manifest.Loads, ref)
	}
	graph, err := codebase.InspectLocalLoad(ctx, codebase.AddonInput{Root: filepath.Join(directory, "payload", "addon"), Manifest: selection.MainTOC})
	if err != nil {
		return nil, err
	}
	if !graph.LoadValid {
		return nil, fmt.Errorf("%w: invalid load graph for %s: %+v", ErrPayload, selection.MainTOC, graph.Issues)
	}
	manifest.LoadedFiles = []string{}
	for _, document := range graph.Documents {
		expected, exists := files["addon/"+document.Path]
		if !exists || document.Blob.SHA256 != expected.SHA256 || document.Blob.Bytes != expected.Bytes {
			return nil, fmt.Errorf("%w: changed or unlisted load %s", ErrPayload, document.Path)
		}
		manifest.LoadedFiles = append(manifest.LoadedFiles, document.Path)
	}
	result = append(result, manifest)
	return result, nil
}
