package records_test

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"strings"
	"sync"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/records/container"
)

const sampleKey = "00112233445566778899aabbccddeeff"

func keyDigest(raw string) string {
	sum := sha256.Sum256([]byte(raw))
	return hex.EncodeToString(sum[:])
}

func TestKeySetFormatsAndIsolation(t *testing.T) {
	for format, raw := range map[string]string{
		"text": "# comment\r\n\r\n00000000000000AB " + sampleKey + "\r\n00000000000000AC " + sampleKey + sampleKey + "\n",
		"json": `{"00000000000000AB":"` + sampleKey + `","00000000000000AC":"` + sampleKey + sampleKey + `"}`,
	} {
		set, err := records.ReadKeySet(context.Background(), strings.NewReader(raw), format, keyDigest(raw))
		if err != nil || set.Count() != 2 || set.SHA256() != keyDigest(raw) {
			t.Fatalf("%s: %v", format, err)
		}
		for _, id := range []uint64{0xab, 0xac} {
			key, err := set.Lookup(context.Background(), id)
			if err != nil || !bytes.Equal(key[:16], []byte{0, 17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255}) {
				t.Fatal("key lookup mismatch")
			}
			clear(key)
			again, _ := set.Lookup(context.Background(), id)
			if again[1] != 17 {
				t.Fatal("key ownership leaked")
			}
		}
		if _, err := set.Lookup(context.Background(), 123); !errors.Is(err, container.ErrKeyUnavailable) {
			t.Fatal(err)
		}
		if strings.Contains(fmt.Sprintf("%v %#v", set, set), sampleKey) {
			t.Fatal("formatting leaked key")
		}
		var group sync.WaitGroup
		for i := 0; i < 8; i++ {
			group.Go(func() {
				for j := 0; j < 100; j++ {
					key, err := set.Lookup(context.Background(), 0xab)
					if err != nil {
						t.Error(err)
					}
					clear(key)
				}
			})
		}
		group.Wait()
	}
}

func TestKeySetRejectsInvalidDocuments(t *testing.T) {
	for _, raw := range []string{
		"", "null", "[]", `{}`, `{"bad":"secret-value"}`, `{"00000000000000ab":null}`,
		`{"00000000000000ab":"` + sampleKey + `","00000000000000AB":"` + sampleKey + `"}`,
		`{"00000000000000ab":"` + sampleKey + `"} {}`,
		`{"00000000000000ab":"` + sampleKey + `","00000000000000ab":"` + sampleKey + `"}`,
	} {
		set, err := records.ReadKeySet(context.Background(), strings.NewReader(raw), "json", keyDigest(raw))
		if !errors.Is(err, records.ErrKeyDocument) || set != nil {
			t.Fatalf("accepted invalid JSON: %v", err)
		}
	}
	for _, raw := range []string{"# comment\n", "+0000000000000ab " + sampleKey, "00000000000000ab " + sampleKey + " junk", "00000000000000ab " + sampleKey + "\n00000000000000AB " + sampleKey, "00000000000000ab " + strings.Repeat("z", 32)} {
		set, err := records.ReadKeySet(context.Background(), strings.NewReader(raw), "text", keyDigest(raw))
		if !errors.Is(err, records.ErrKeyDocument) || set != nil {
			t.Fatalf("accepted invalid text: %v", err)
		}
	}
}

type keyErrorReader struct{}

func (keyErrorReader) Read([]byte) (int, error) { return 0, errors.New("secret-value") }

func TestKeySetBudgetsCancellationIntegrityAndRedaction(t *testing.T) {
	raw := "00000000000000ab " + sampleKey
	if _, err := records.ReadKeySet(context.Background(), strings.NewReader(raw), "text", strings.Repeat("0", 64)); !errors.Is(err, container.ErrIntegrity) {
		t.Fatal(err)
	}
	if _, err := records.ReadKeySet(context.Background(), keyErrorReader{}, "text", keyDigest(raw)); err == nil || strings.Contains(err.Error(), "secret-value") {
		t.Fatal("I/O error was not redacted")
	}
	large := strings.Repeat("x", (4<<20)+1)
	if _, err := records.ReadKeySet(context.Background(), strings.NewReader(large), "text", keyDigest(large)); !errors.Is(err, records.ErrMetadataLimit) {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := records.ReadKeySet(ctx, strings.NewReader(raw), "text", keyDigest(raw)); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	set, err := records.ReadKeySet(context.Background(), strings.NewReader(raw), "text", keyDigest(raw))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := set.Lookup(ctx, 0xab); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}

func FuzzKeyDocument(f *testing.F) {
	f.Add("text", "00000000000000ab "+sampleKey)
	f.Add("json", `{"00000000000000ab":"`+sampleKey+`"}`)
	f.Fuzz(func(t *testing.T, format, raw string) {
		if len(raw) > 65536 {
			t.Skip()
		}
		set, err := records.ReadKeySet(context.Background(), strings.NewReader(raw), format, keyDigest(raw))
		if err != nil && set != nil {
			t.Fatal("partial key set")
		}
	})
}
