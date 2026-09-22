package protocol_test

import (
	"encoding/json"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

var lowerCamelCase = regexp.MustCompile(`^[a-z][A-Za-z0-9]*$`)

type schemaNode map[string]any

func loadSchema(t *testing.T) schemaNode {
	t.Helper()
	raw, err := os.ReadFile("signal.v1.schema.json")
	if err != nil {
		t.Fatal(err)
	}
	var schema schemaNode
	if err := json.Unmarshal(raw, &schema); err != nil {
		t.Fatal(err)
	}
	return schema
}

// The schema documents one contract; it must at least agree with itself and
// with the parser. No runtime code depends on this file.
func TestSignalSchemaIsSelfConsistent(t *testing.T) {
	schema := loadSchema(t)
	var walk func(node schemaNode)
	walk = func(node schemaNode) {
		properties, _ := node["properties"].(map[string]any)
		if required, ok := node["required"].([]any); ok {
			for _, entry := range required {
				name, _ := entry.(string)
				if _, exists := properties[name]; !exists {
					t.Fatalf("required property %q is not declared", name)
				}
			}
		}
		for name, child := range properties {
			if !lowerCamelCase.MatchString(name) {
				t.Fatalf("property %q is not lowerCamelCase", name)
			}
			if node, ok := child.(map[string]any); ok {
				walk(node)
			}
		}
		if clauses, ok := node["allOf"].([]any); ok {
			for _, clause := range clauses {
				for _, branch := range []string{"if", "then", "else"} {
					if nested, ok := clause.(map[string]any)[branch].(map[string]any); ok {
						walk(nested)
					}
				}
			}
		}
	}
	walk(schema)
	var kinds []any
	if properties, ok := schema["properties"].(map[string]any); ok {
		if kind, ok := properties["kind"].(map[string]any); ok {
			kinds, _ = kind["enum"].([]any)
		}
	}
	var declared []string
	for _, kind := range kinds {
		value, _ := kind.(string)
		declared = append(declared, value)
	}
	want := []string{"ready", "loaded", "reported", "acknowledged", "cancelled", "cleared", "identity"}
	slices.Sort(declared)
	slices.Sort(want)
	if !slices.Equal(declared, want) {
		t.Fatalf("kind enum %v, want %v", declared, want)
	}
	if schema["additionalProperties"] != false {
		t.Fatal("schema must reject unknown fields like ParseSignal")
	}
}

func TestSignalSamplesParseAndMarshalToTheSameShape(t *testing.T) {
	matches, err := filepath.Glob("samples/*.json")
	if err != nil || len(matches) < 5 {
		t.Fatalf("samples: %v %d", err, len(matches))
	}
	schema := loadSchema(t)
	properties, _ := schema["properties"].(map[string]any)
	topRequired, _ := schema["required"].([]any)
	for _, path := range matches {
		t.Run(filepath.Base(path), func(t *testing.T) {
			raw, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			var want bridge.Signal
			if err := json.Unmarshal(raw, &want); err != nil {
				t.Fatal(err)
			}
			parsed, err := bridge.ParseSignal(raw)
			if err != nil {
				t.Fatal(err)
			}
			if parsed != want {
				t.Fatalf("sample and parser disagree: %+v vs %+v", parsed, want)
			}
			remarshaled, err := json.Marshal(parsed)
			if err != nil {
				t.Fatal(err)
			}
			if again, err := bridge.ParseSignal(remarshaled); err != nil || again != parsed {
				t.Fatalf("round trip changed sample: %+v %v", again, err)
			}
			var sample map[string]any
			if err := json.Unmarshal(raw, &sample); err != nil {
				t.Fatal(err)
			}
			var emitted map[string]any
			if err := json.Unmarshal(remarshaled, &emitted); err != nil {
				t.Fatal(err)
			}
			if !slices.Equal(sortedKeys(sample), sortedKeys(emitted)) {
				t.Fatalf("sample keys %v, host emission keys %v", sortedKeys(sample), sortedKeys(emitted))
			}
			for name := range sample {
				if _, declared := properties[name]; !declared {
					t.Fatalf("sample field %q is not declared in the schema", name)
				}
			}
			for _, entry := range topRequired {
				name, _ := entry.(string)
				if _, present := sample[name]; !present {
					t.Fatalf("sample is missing required field %q", name)
				}
			}
		})
	}
}

func sortedKeys(value map[string]any) []string {
	keys := make([]string, 0, len(value))
	for key := range value {
		keys = append(keys, key)
	}
	slices.Sort(keys)
	return keys
}
