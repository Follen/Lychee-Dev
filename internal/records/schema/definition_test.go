package schema_test

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/schema"
)

const sample = `COLUMNS
int ID
int<Other::ID> Parent?
locstring Name
float Scale
int Flags

BUILD 10.1.2.30-10.3.1.5
LAYOUT AABBCCDD
$noninline,id$ID
$noninline,relation$Parent
Name
Scale<f32>
Flags<u32>[2]

BUILD 12.1.0.69875
LAYOUT 12345678
$id$ID<u32>
Name
Flags<64>
`

func TestDefinitionIdentityFieldsAndSelection(t *testing.T) {
	doc, err := schema.Parse(context.Background(), []byte(sample))
	if err != nil {
		t.Fatal(err)
	}
	d, err := doc.Select(context.Background(), "10.2.0.1", "")
	if err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256([]byte(sample))
	if d.SHA256 != hex.EncodeToString(sum[:]) || d.Match != "build" || len(d.Fields) != 5 {
		t.Fatalf("%+v", d)
	}
	if d.Fields[0].Inline || !d.Fields[0].Identity || d.Fields[0].Bits != 32 {
		t.Fatalf("%+v", d.Fields[0])
	}
	if d.Fields[1].Verified || d.Fields[1].ForeignTable != "Other" || d.Fields[1].ForeignColumn != "ID" || !d.Fields[1].Relation {
		t.Fatalf("%+v", d.Fields[1])
	}
	if d.Fields[3].Bits != 32 || d.Fields[4].Signed || d.Fields[4].Elements != 2 || !d.Fields[4].Array {
		t.Fatalf("%+v", d.Fields)
	}
	d.Fields[0].Name = "changed"
	again, err := doc.Select(context.Background(), "99.0.0.1", "aabbccdd")
	if err != nil || again.Match != "layout" || again.Fields[0].Name != "ID" {
		t.Fatalf("%+v %v", again, err)
	}
	if _, err := doc.Select(context.Background(), "12.1.0.69875", "DEADBEEF"); !errors.Is(err, schema.ErrMissing) {
		t.Fatal("unmatched layout fell back to build")
	}
	for _, build := range []string{"10.0.99.99", "10.4.0.0"} {
		if _, err := doc.Select(context.Background(), build, ""); !errors.Is(err, schema.ErrMissing) {
			t.Fatal(err)
		}
	}
}

func TestDefinitionRejectsAmbiguityInvalidSyntaxAndMissingFields(t *testing.T) {
	for _, bad := range []string{
		strings.Replace(sample, "float Scale", "wat Scale", 1),
		strings.Replace(sample, "Scale<f32>", "NoSuchColumn<u32>", 1),
		strings.Replace(sample, "Flags<u32>[2]", "Flags<u32>[0]", 1),
		strings.Replace(sample, "Flags<u32>[2]", "Flags<u33>", 1),
		strings.Replace(sample, "$noninline,id$", "$noninline,unknown$", 1),
		strings.Replace(sample, "float Scale", "float Scale\nfloat Scale", 1),
		strings.Replace(sample, "10.1.2.30-10.3.1.5", "10.3.1.5-10.1.2.30", 1),
		strings.Replace(sample, "Scale<f32>", "Scale<f32>\nScale<f32>", 1),
		strings.Replace(sample, "AABBCCDD", "ZZBBCCDD", 1),
	} {
		if doc, err := schema.Parse(context.Background(), []byte(bad)); err == nil || doc != nil {
			t.Fatal("accepted invalid definition")
		}
	}
	raw := sample + "\nBUILD 12.1.0.69875\nLAYOUT 12345678\n$id$ID<u32>\n"
	doc, err := schema.Parse(context.Background(), []byte(raw))
	if err != nil {
		t.Fatal(err)
	}
	for _, hash := range []string{"", "12345678"} {
		if _, err := doc.Select(context.Background(), "12.1.0.69875", hash); !errors.Is(err, schema.ErrAmbiguous) {
			t.Fatal(err)
		}
	}
}

func TestDefinitionBudgetsCancellationAndStrictBuild(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := schema.Parse(ctx, []byte(sample)); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	if _, err := schema.Parse(context.Background(), make([]byte, (4<<20)+1)); !errors.Is(err, schema.ErrLimit) {
		t.Fatal(err)
	}
	doc, err := schema.Parse(context.Background(), []byte(sample))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := doc.Select(ctx, "12.1.0.69875", ""); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	for _, build := range []string{"12.1.0", "x12.1.0.69875", "12.1.0.-1", "12.1.0.69875x", "12.1.0.4294967296"} {
		if _, err := doc.Select(context.Background(), build, ""); !errors.Is(err, schema.ErrFormat) {
			t.Fatal(err)
		}
	}
}

func FuzzDefinition(f *testing.F) {
	f.Add([]byte(sample))
	f.Fuzz(func(t *testing.T, raw []byte) {
		if len(raw) > 1<<20 {
			t.Skip()
		}
		d, err := schema.Parse(context.Background(), raw)
		if err == nil {
			_, _ = d.Select(context.Background(), "12.1.0.69875", "12345678")
		}
	})
}
