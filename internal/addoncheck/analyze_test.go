package addoncheck

import (
	"context"
	"strings"
	"testing"
)

func TestAnalyzeLuaNonUTF8CommentRetainsDeclarations(t *testing.T) {
	// Forever 1.60.1.70450 has this byte in a comment in
	// Blizzard_NewPlayerExperience/Camelot/Blizzard_TutorialData.lua.
	data := []byte("function Before() end\n-- Night Web\x92s Hollow\nfunction After() end\n")
	facts, err := AnalyzeDocument(context.Background(), "Tutorial.lua", data)
	if err != nil {
		t.Fatalf("legacy byte in a Lua comment aborted analysis: %v", err)
	}
	if len(facts.Declarations) != 2 || facts.Declarations[0].Name != "Before" || facts.Declarations[0].Line != 1 || facts.Declarations[1].Name != "After" || facts.Declarations[1].Line != 3 {
		t.Fatalf("lost declarations or source positions: %+v", facts)
	}
	if len(facts.Diagnostics) != 0 {
		t.Fatalf("encoding warning incorrectly invalidated syntax: %+v", facts.Diagnostics)
	}
	if len(facts.Warnings) != 1 || facts.Warnings[0].Line != 2 || facts.Warnings[0].ByteOffset != strings.IndexByte(string(data), 0x92) || !strings.Contains(facts.Warnings[0].Message, "0x92") {
		t.Fatalf("missing actionable encoding warning: %+v", facts.Warnings)
	}
}

func TestAnalyzeDocumentByteLimitNamesFileAndBudget(t *testing.T) {
	data := make([]byte, (32<<20)+1)
	_, err := AnalyzeDocument(context.Background(), "TooLarge.lua", data)
	if err == nil || !strings.Contains(err.Error(), "codebase.analysis_byte_limit") || !strings.Contains(err.Error(), "TooLarge.lua") || !strings.Contains(err.Error(), "33554433") || !strings.Contains(err.Error(), "33554432") {
		t.Fatalf("unhelpful byte-limit failure: %v", err)
	}
}

func TestNonUTF8LuaIsNotRewrittenIntoValidCode(t *testing.T) {
	facts, err := AnalyzeDocument(context.Background(), "Broken.lua", []byte("\x92function Invented() end\n"))
	if err != nil || len(facts.Declarations) != 0 || len(facts.Relationships) != 0 || len(facts.Diagnostics) == 0 {
		t.Fatalf("invalid token was repaired into a declaration: %+v, %v", facts, err)
	}
	if len(facts.Warnings) != 1 || facts.Warnings[0].ByteOffset != 0 {
		t.Fatalf("invalid token's encoding warning missing: %+v", facts.Warnings)
	}
}
