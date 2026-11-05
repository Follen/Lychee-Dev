package command

import (
	"reflect"

	"github.com/follenfang/lycheedev/internal/codebase"
	"github.com/follenfang/lycheedev/internal/records"
)

// Input admission differs from result serialization: an interface-valued SQL
// parameter accepts scalars only, and batch fields can use domain defaults.
// Keep those input rules separate from the output schema's required fields.
func queryInputSchema() map[string]any {
	schema := jsonTypeSchema(reflect.TypeFor[records.DataQuery](), map[reflect.Type]bool{})
	schema["additionalProperties"] = false
	properties := schema["properties"].(map[string]any)
	properties["sql"].(map[string]any)["minLength"] = 1
	properties["parameters"] = map[string]any{
		"type":                 "object",
		"additionalProperties": map[string]any{"type": []string{"string", "number", "boolean", "null"}},
	}
	properties["hotfix"] = map[string]any{
		"type": "array", "minItems": 1, "maxItems": 16,
		"items": map[string]any{"type": "string"},
	}
	return schema
}

func sourceBatchInputSchema() map[string]any {
	schema := jsonTypeSchema(reflect.TypeFor[codebase.BatchRequest](), map[reflect.Type]bool{})
	schema["additionalProperties"] = false
	requests := schema["properties"].(map[string]any)["requests"].(map[string]any)
	requests["type"], requests["minItems"], requests["maxItems"] = "array", 1, 32
	item := requests["items"].(map[string]any)
	item["additionalProperties"] = false
	item["description"] = "Exactly one non-null query, refs or context is required. Omitted fields use that operation's defaults."
	properties := item["properties"].(map[string]any)
	for _, kind := range []string{"query", "refs", "context"} {
		shape := properties[kind].(map[string]any)["anyOf"].([]any)[0].(map[string]any)
		shape["additionalProperties"] = false
		shape["required"] = []string{}
		if kind == "query" {
			shape["required"] = []string{"text"}
		} else {
			shape["description"] = "Exactly one non-empty symbol or symbolId is required."
		}
	}
	return schema
}
