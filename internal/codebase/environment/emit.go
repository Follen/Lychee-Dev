package environment

import (
	"encoding/json"
	"fmt"
	"regexp"
	"sort"
	"strings"
)

type definitionWriter struct {
	text strings.Builder
	line int
	max  int
}

var enumInteger = regexp.MustCompile(`^(0|[1-9][0-9]*)$`)

func (w *definitionWriter) write(line string) {
	w.text.WriteString(line)
	w.text.WriteByte('\n')
	w.line++
}

func emitDefinitions(records []APIRecord, maxBytes int) ([]byte, []DefinitionMapping, []string, []Gap) {
	w := &definitionWriter{max: maxBytes}
	w.write("---@meta")
	w.write("-- Fixed Blizzard API declarations. Unknown metadata remains in coverage/facts.")
	w.write("---@alias UnitToken string")
	w.write("---@alias UnitTokenVariant string")
	w.write("---@alias luaIndex integer")
	w.write("---@alias fileID integer")
	w.write("---@alias textureID integer")
	w.write("")
	mappings := []DefinitionMapping{}
	gaps := []Gap{}
	unknown := map[string]bool{}
	known := map[string]bool{"UnitToken": true, "UnitTokenVariant": true, "luaIndex": true, "fileID": true, "textureID": true}
	enumNamespace := false
	for _, record := range records {
		if record.Kind == "structure" || record.Kind == "enumeration" || record.Kind == "callback" || record.Kind == "script-object" {
			if typeName(record.Name) {
				known[record.Name] = true
			}
		}
	}
	// Type definitions precede callables so the language server resolves every
	// known structure/callback/enum name in function signatures.
	for _, record := range records {
		if record.Kind == "function" || record.Kind == "event" {
			continue
		}
		if !typeName(record.Name) {
			gaps = append(gaps, Gap{Path: record.Path, Line: record.Line, Code: "invalid_type_name", Detail: record.Name})
			continue
		}
		switch record.Kind {
		case "structure", "script-object":
			w.write("---@class " + record.Name)
			for _, slot := range record.Fields {
				if !identifier(slot.Name) {
					gaps = append(gaps, Gap{Path: record.Path, Line: slot.Line, Code: "invalid_field_name", Detail: slot.Name})
					continue
				}
				typeText := slotType(record, slot, known, unknown, &gaps)
				w.write("---@field " + slot.Name + " " + typeText)
			}
			w.write("")
		case "callback":
			args := []string{}
			for _, slot := range record.Parameters {
				name := slot.Name
				if !identifier(name) {
					name = fmt.Sprintf("arg%d", slot.Index)
					gaps = append(gaps, Gap{Path: record.Path, Line: slot.Line, Code: "invalid_parameter_name", Detail: slot.Name})
				}
				args = append(args, name+":"+slotType(record, slot, known, unknown, &gaps))
			}
			returns := []string{}
			for _, slot := range record.Returns {
				returns = append(returns, slotType(record, slot, known, unknown, &gaps))
			}
			alias := "---@alias " + record.Name + " fun(" + strings.Join(args, ",") + ")"
			if len(returns) > 0 {
				alias += ":" + strings.Join(returns, ",")
			}
			w.write(alias)
			w.write("")
		case "enumeration":
			valid := len(record.Values) > 0
			for _, slot := range record.Values {
				value, numeric := slot.Raw["EnumValue"].(json.Number)
				if !identifier(slot.Name) || !numeric || !enumInteger.MatchString(string(value)) {
					valid = false
				}
			}
			w.write("---@alias " + record.Name + " integer")
			if !valid {
				gaps = append(gaps, Gap{Path: record.Path, Line: record.Line, Code: "enum_values_unknown", Detail: record.Name})
				w.write("")
				continue
			}
			if !enumNamespace {
				w.write("Enum = Enum or {}")
				enumNamespace = true
			}
			w.write("Enum." + record.Name + " = {")
			for _, slot := range record.Values {
				w.write("  " + slot.Name + " = " + string(slot.Raw["EnumValue"].(json.Number)) + ",")
			}
			w.write("}")
			w.write("")
		default:
			gaps = append(gaps, Gap{Path: record.Path, Line: record.Line, Code: "unsupported_record_kind", Detail: record.Kind})
		}
	}
	// A duplicate callable is kept as separate facts but withheld from LuaLS;
	// emitting two same-name functions would silently choose one signature.
	counts := map[string]int{}
	for _, record := range records {
		if record.Kind == "function" {
			counts[record.Name]++
		}
	}
	namespaces := map[string]bool{}
	for _, record := range records {
		if record.Kind != "function" {
			continue
		}
		if counts[record.Name] > 1 {
			gaps = append(gaps, Gap{Path: record.Path, Line: record.Line, Code: "ambiguous_callable", Detail: record.Name})
			continue
		}
		parts := strings.Split(record.Name, ".")
		valid := len(parts) == 1 || len(parts) == 2
		for _, part := range parts {
			valid = valid && identifier(part)
		}
		if !valid {
			gaps = append(gaps, Gap{Path: record.Path, Line: record.Line, Code: "invalid_callable", Detail: record.Name})
			continue
		}
		if len(parts) == 2 && !namespaces[parts[0]] {
			w.write(parts[0] + " = " + parts[0] + " or {}")
			namespaces[parts[0]] = true
		}
		args := []string{}
		for _, slot := range record.Parameters {
			name := slot.Name
			if !identifier(name) {
				name = fmt.Sprintf("arg%d", slot.Index)
				gaps = append(gaps, Gap{Path: record.Path, Line: slot.Line, Code: "invalid_parameter_name", Detail: slot.Name})
			}
			w.write("---@param " + name + " " + slotType(record, slot, known, unknown, &gaps))
			args = append(args, name)
		}
		for _, slot := range record.Returns {
			w.write("---@return " + slotType(record, slot, known, unknown, &gaps) + " " + safeComment(slot.Name))
		}
		if record.Raw["MayReturnNothing"] == true {
			gaps = append(gaps, Gap{Path: record.Path, Line: record.Line, Code: "may_return_nothing", Detail: "LuaLS annotation cannot express zero returns separately from nil"})
		}
		column := len("function ")
		if len(parts) == 2 {
			column += len(parts[0]) + 1
		}
		mappings = append(mappings, DefinitionMapping{Callable: record.Name, SourcePath: record.Path, SourceLine: record.Line, GeneratedLine: w.line, GeneratedUTF16Column: column, Kind: "function"})
		w.write("function " + record.Name + "(" + strings.Join(args, ", ") + ") end")
		w.write("")
	}
	result := []byte(w.text.String())
	if len(result) > maxBytes {
		gaps = append(gaps, Gap{Code: "definition_budget", Detail: fmt.Sprintf("%d bytes > %d", len(result), maxBytes)})
	}
	unknownTypes := make([]string, 0, len(unknown))
	for name := range unknown {
		unknownTypes = append(unknownTypes, name)
	}
	sort.Strings(unknownTypes)
	return result, mappings, unknownTypes, gaps
}

func slotType(record APIRecord, slot Slot, known, unknown map[string]bool, gaps *[]Gap) string {
	base := strings.TrimSpace(slot.Type)
	switch base {
	case "bool":
		base = "boolean"
	case "cstring", "string", "localizedstring", "cstringOptional":
		base = "string"
	case "number", "float", "double":
		base = "number"
	case "int", "integer":
		base = "integer"
	case "function":
		base = "function"
	case "table":
		if slot.InnerType != "" {
			inner := slot
			inner.Type = slot.InnerType
			inner.InnerType = ""
			inner.Nilable = false
			base = slotType(record, inner, known, unknown, gaps) + "[]"
		}
	case "boolean", "unknown", "any", "nil", "thread", "userdata":
	case "":
		base = "unknown"
		*gaps = append(*gaps, Gap{Path: record.Path, Line: slot.Line, Code: "missing_slot_type", Detail: slot.Name})
	default:
		if !typeName(base) || !known[base] {
			unknown[base] = true
			*gaps = append(*gaps, Gap{Path: record.Path, Line: slot.Line, Code: "unknown_type", Detail: base})
			base = "unknown"
		}
	}
	if slot.Nilable && base != "nil" {
		base += "|nil"
	}
	return base
}

func safeComment(value string) string {
	return strings.Map(func(r rune) rune {
		if r == '\n' || r == '\r' {
			return ' '
		}
		return r
	}, value)
}
func typeName(value string) bool {
	if value == "" {
		return false
	}
	for _, part := range strings.Split(value, ".") {
		if !identifier(part) {
			return false
		}
	}
	return true
}
func identifier(value string) bool {
	if value == "" || strings.Contains(" and break do else elseif end false for function if in local nil not or repeat return then true until while ", " "+value+" ") {
		return false
	}
	for i, r := range value {
		if r == '_' || r >= 'A' && r <= 'Z' || r >= 'a' && r <= 'z' || i > 0 && r >= '0' && r <= '9' {
			continue
		}
		return false
	}
	return true
}
