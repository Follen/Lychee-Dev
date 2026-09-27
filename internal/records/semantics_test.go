package records

import (
	"context"
	"testing"
)

func TestPinnedSemanticSyntaxBuildsAndConditions(t *testing.T) {
	mappings, err := parseSemanticMappings([]byte("FLAGS SpellMisc::Attributes[0] SpellAttributes0\nENUM SpellEffect::EffectMiscValue[0] SpellModOp SpellEffect::EffectAura=107\n"), "SpellEffect")
	if err != nil || len(mappings) != 1 || mappings[0].conditionField != "SpellEffect::EffectAura" || mappings[0].conditionValue != "107" {
		t.Fatalf("%+v %v", mappings, err)
	}
	raw := []byte("0x8000000000000000 HIGH // high bit\n(BUILD 3.0.1.8303-3.3.5.12340, 4.0.0.11792) 146 OLD\n(BUILD 12.1.0.69933) 146 CURRENT\n5\n-1 NONE\n")
	rows, err := parseSemanticValues(context.Background(), raw, "12.1.0.69933")
	if err != nil || len(rows) != 4 || rows[0][0] != uint64(1)<<63 || rows[1][1] != "CURRENT" || rows[2][1] != nil || rows[3][0] != int64(-1) {
		t.Fatalf("%+v %v", rows, err)
	}
	if _, err := parseSemanticValues(context.Background(), []byte("(BOGUS 1) 2 UNKNOWN"), "12.1.0.69933"); err == nil {
		t.Fatal("unknown applicability accepted")
	}
	if _, err := parseSemanticMappings([]byte("FLAGS SpellMisc::Attributes ../../escape"), "SpellMisc"); err == nil {
		t.Fatal("unsafe metadata name")
	}
}
