package schema_test

import (
	"context"
	"io"
	"net/http"
	"os"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/records/schema"
)

func TestPinnedPublishedDefinitions(t *testing.T) {
	if os.Getenv("LYCHEEDEV_TEST_DBD_NETWORK") != "1" {
		t.Skip("explicit fixed-source network test required")
	}
	const commit = "83057bdc0cbe13062850ebf8ad530031e128a1cd"
	for _, name := range []string{"Map", "SpellName", "ChrClasses"} {
		t.Run(name, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
			defer cancel()
			request, err := http.NewRequestWithContext(ctx, http.MethodGet, "https://raw.githubusercontent.com/wowdev/WoWDBDefs/"+commit+"/definitions/"+name+".dbd", nil)
			if err != nil {
				t.Fatal(err)
			}
			response, err := http.DefaultClient.Do(request)
			if err != nil {
				t.Fatal(err)
			}
			defer response.Body.Close()
			if response.StatusCode != http.StatusOK {
				t.Fatalf("HTTP %d", response.StatusCode)
			}
			raw, err := io.ReadAll(io.LimitReader(response.Body, (4<<20)+1))
			if err != nil {
				t.Fatal(err)
			}
			doc, err := schema.Parse(ctx, raw)
			if err != nil {
				t.Fatal(err)
			}
			definition, err := doc.Select(ctx, "12.1.0.69875", "")
			if err != nil {
				t.Fatal(err)
			}
			t.Logf("commit=%s name=%s bytes=%d sha256=%s match=%s fields=%d", commit, name, len(raw), definition.SHA256, definition.Match, len(definition.Fields))
		})
	}
}
