//go:build ignore

// CI-only adapter for the production documentation generator. Its caller
// supplies an exact, verified Gethe checkout; this never executes source Lua.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/codebase/environment"
)

func main() {
	if len(os.Args) != 4 {
		panic("usage: go run tools/ci-source-definitions.go <Retail checkout> <commit> <new output directory>")
	}
	root, commit, output := os.Args[1], os.Args[2], os.Args[3]
	const prefix = "Interface/AddOns/Blizzard_APIDocumentationGenerated"
	files, err := os.ReadDir(filepath.Join(root, filepath.FromSlash(prefix)))
	if err != nil {
		panic(err)
	}
	documents := map[string][]byte{}
	for _, file := range files {
		if file.IsDir() || !strings.HasSuffix(file.Name(), ".lua") {
			continue
		}
		name := prefix + "/" + file.Name()
		data, err := os.ReadFile(filepath.Join(root, filepath.FromSlash(name)))
		if err != nil {
			panic(err)
		}
		documents[name] = data
	}
	result, err := environment.Build(context.Background(), environment.Identity{Repository: "Gethe/wow-ui-source", Commit: commit, Client: "retail", GeneratorVersion: environment.GeneratorVersion}, documents, environment.Limits{})
	if err != nil {
		panic(err)
	}
	if len(result.Facts) == 0 || len(result.Mappings) == 0 || !strings.Contains(string(result.Definitions), "\nfunction UnitHealth(") {
		panic("pinned API definitions missing UnitHealth")
	}
	if err := os.Mkdir(output, 0700); err != nil {
		panic(err)
	}
	if err := os.WriteFile(filepath.Join(output, "api.d.lua"), result.Definitions, 0600); err != nil {
		panic(err)
	}
	receipt, err := json.MarshalIndent(struct {
		Manifest environment.Manifest `json:"manifest"`
		Coverage environment.Coverage `json:"coverage"`
	}{result.Manifest, result.Coverage}, "", "  ")
	if err != nil {
		panic(err)
	}
	if err := os.WriteFile(filepath.Join(output, "manifest.json"), append(receipt, '\n'), 0600); err != nil {
		panic(err)
	}
	fmt.Printf("Generated %d documents, %d mappings; complete=%t\n", result.Manifest.Documents, len(result.Mappings), result.Coverage.Complete)
}
