package codebase

import (
	"bytes"
	"context"
	"encoding/xml"
	"io"
	"strings"
)

func scanMarkup(ctx context.Context, data []byte) DocumentFacts {
	facts := emptyFacts()
	decoder := xml.NewDecoder(bytes.NewReader(data))
	type element struct{ tag, owner string }
	stack := []element{}
	var script strings.Builder
	scriptLine := 0
	for {
		if ctx.Err() != nil {
			return facts
		}
		line, _ := decoder.InputPos()
		token, err := decoder.Token()
		if err == io.EOF {
			break
		}
		if err != nil {
			facts.Diagnostics = append(facts.Diagnostics, SyntaxNote{Message: err.Error(), Line: line})
			break
		}
		switch item := token.(type) {
		case xml.StartElement:
			attrs := map[string]string{}
			for _, attribute := range item.Attr {
				attrs[attribute.Name.Local] = attribute.Value
			}
			owner := attrValue(attrs, "name")
			if owner != "" {
				facts.Declarations = append(facts.Declarations, Declaration{Name: owner, Category: "xml-" + item.Name.Local, Line: line, EndLine: line})
			} else if len(stack) > 0 {
				owner = stack[len(stack)-1].owner
			}
			stack = append(stack, element{item.Name.Local, owner})
			if kind := loadKind(item.Name.Local); kind != "" {
				if file := attrValue(attrs, "file"); file != "" {
					facts.Loads = append(facts.Loads, LoadReference{Path: file, Line: line, Kind: kind})
				}
			}
			for _, base := range strings.Split(attrValue(attrs, "inherits"), ",") {
				if base = strings.TrimSpace(base); base != "" {
					facts.Relationships = append(facts.Relationships, Relationship{From: owner, To: base, Category: "xml-inherits", Confidence: "exact", Line: line})
				}
			}
			for _, attr := range []string{"function", "method"} {
				if target := attrValue(attrs, attr); target != "" {
					facts.Relationships = append(facts.Relationships, Relationship{From: owner, To: target, Category: "xml-" + attr, Confidence: "exact", Line: line})
				}
			}
			if isScriptElement(item.Name.Local) {
				script.Reset()
				scriptLine = 0
			}
		case xml.CharData:
			if len(stack) > 0 && isScriptElement(stack[len(stack)-1].tag) {
				if scriptLine == 0 {
					scriptLine = line
				}
				script.Write(item)
			}
		case xml.EndElement:
			if isScriptElement(item.Name.Local) && strings.TrimSpace(script.String()) != "" {
				inner := scanProgram(ctx, []byte(script.String()), false)
				for _, decl := range inner.Declarations {
					decl.Line += scriptLine - 1
					decl.EndLine += scriptLine - 1
					facts.Declarations = append(facts.Declarations, decl)
				}
				for _, edge := range inner.Relationships {
					edge.Line += scriptLine - 1
					if edge.From == "<file>" && len(stack) > 0 {
						edge.From = stack[len(stack)-1].owner + ":" + item.Name.Local
					}
					facts.Relationships = append(facts.Relationships, edge)
				}
				for _, note := range inner.Diagnostics {
					if note.Line > 0 {
						note.Line += scriptLine - 1
					} else {
						note.Line = scriptLine
					}
					facts.Diagnostics = append(facts.Diagnostics, note)
				}
			}
			if len(stack) > 0 {
				stack = stack[:len(stack)-1]
			}
		}
	}
	return facts
}
func isScriptElement(name string) bool { return len(name) > 2 && strings.HasPrefix(name, "On") }

// WoW XML accepts mixed-case tag and attribute names (for example x:FiLe), so
// load references are matched case-insensitively like the game client does.
func loadKind(tag string) string {
	switch {
	case strings.EqualFold(tag, "Script"):
		return "script"
	case strings.EqualFold(tag, "Include"):
		return "include"
	}
	return ""
}
func attrValue(attrs map[string]string, name string) string {
	for key, value := range attrs {
		if strings.EqualFold(key, name) {
			return value
		}
	}
	return ""
}
