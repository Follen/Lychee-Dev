package command

import (
	"encoding/json"
	"reflect"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/codebase"
	"github.com/follenfang/lycheedev/internal/publish"
	"github.com/follenfang/lycheedev/internal/records"
)

// Business schemas use the actual output types, so field additions and JSON
// names cannot silently drift from help. Dynamic row/object payloads remain open.
func businessResultSchema(route string) map[string]any {
	var value any
	switch route {
	case "data sql":
		return map[string]any{"oneOf": []any{jsonTypeSchema(reflect.TypeFor[records.QueryResult](), map[reflect.Type]bool{}), jsonTypeSchema(reflect.TypeFor[records.QueryCSVManifest](), map[reflect.Type]bool{})}}
	case "data db2":
		value = records.TableReading{}
	case "data db2 schema":
		value = records.DB2SchemaResult{}
	case "data db2 search":
		value = records.DB2SearchResult{}
	case "data db2 foreign-key":
		value = records.DB2ForeignKeyResult{}
	case "data spell info":
		value = records.SpellInfoResult{}
	case "data spell auras":
		value = records.SpellAurasResult{}
	case "data spell summons":
		value = records.SpellSummonsResult{}
	case "data item get":
		value = records.ItemGetResult{}
	case "data item models":
		value = records.ItemModelsResult{}
	case "data item geosets":
		value = records.ItemGeosetResult{}
	case "data item textures":
		value = records.ItemTexturesResult{}
	case "data creature display":
		value = records.CreatureDisplayResult{}
	case "data creature model":
		value = records.CreatureModelResult{}
	case "data encounter get":
		value = records.EncounterGetResult{}
	case "data decor list":
		value = records.DecorListResult{}
	case "data decor get":
		value = records.DecorGetResult{}
	case "source query":
		value = codebase.SearchResponse{}
	case "source refs":
		value = codebase.RelationResult{}
	case "source context":
		value = codebase.ContextResult{}
	case "source batch":
		value = codebase.BatchResult{}
	case "source prune":
		value = codebase.SourcePruneResult{}
	case "asset export":
		value = records.AssetExportManifest{}
	case "asset demux":
		value = records.AssetDemuxManifest{}
	case "asset inspect":
		value = records.FileReading{}
	}
	if strings.HasPrefix(route, "publish ") {
		value = publish.Result{}
	}
	if value == nil {
		return nil
	}
	return jsonTypeSchema(reflect.TypeOf(value), map[reflect.Type]bool{})
}

func jsonTypeSchema(t reflect.Type, active map[reflect.Type]bool) map[string]any {
	if t == reflect.TypeFor[time.Time]() {
		return map[string]any{"type": "string", "format": "date-time"}
	}
	if t == reflect.TypeFor[json.Number]() {
		return map[string]any{"type": "number"}
	}
	if t == reflect.TypeFor[json.RawMessage]() {
		return map[string]any{}
	}
	if t.Kind() == reflect.Pointer {
		return map[string]any{"anyOf": []any{jsonTypeSchema(t.Elem(), active), map[string]any{"type": "null"}}}
	}
	switch t.Kind() {
	case reflect.Interface:
		return map[string]any{}
	case reflect.Bool:
		return map[string]any{"type": "boolean"}
	case reflect.String:
		return map[string]any{"type": "string"}
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64, reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64:
		return map[string]any{"type": "integer"}
	case reflect.Float32, reflect.Float64:
		return map[string]any{"type": "number"}
	case reflect.Slice:
		if t.Elem().Kind() == reflect.Uint8 {
			return map[string]any{"type": []string{"string", "null"}, "contentEncoding": "base64"}
		}
		return map[string]any{"type": []string{"array", "null"}, "items": jsonTypeSchema(t.Elem(), active)}
	case reflect.Array:
		return map[string]any{"type": "array", "items": jsonTypeSchema(t.Elem(), active), "minItems": t.Len(), "maxItems": t.Len()}
	case reflect.Map:
		return map[string]any{"type": []string{"object", "null"}, "additionalProperties": jsonTypeSchema(t.Elem(), active)}
	case reflect.Struct:
		if active[t] {
			return map[string]any{"type": "object", "description": "Recursive " + t.String()}
		}
		active[t] = true
		defer delete(active, t)
		properties := map[string]any{}
		required := []string{}
		for i := 0; i < t.NumField(); i++ {
			field := t.Field(i)
			if !field.IsExported() {
				continue
			}
			name, options, _ := strings.Cut(field.Tag.Get("json"), ",")
			if name == "-" {
				continue
			}
			if name == "" && field.Anonymous && field.Type.Kind() == reflect.Struct {
				embedded := jsonTypeSchema(field.Type, active)
				if values, ok := embedded["properties"].(map[string]any); ok {
					for key, value := range values {
						properties[key] = value
					}
				}
				if names, ok := embedded["required"].([]string); ok {
					required = append(required, names...)
				}
				continue
			}
			if name == "" {
				name = field.Name
			}
			properties[name] = jsonTypeSchema(field.Type, active)
			if !strings.Contains(options, "omitempty") && !strings.Contains(options, "omitzero") {
				required = append(required, name)
			}
		}
		return map[string]any{"type": "object", "properties": properties, "required": required}
	}
	return map[string]any{}
}
