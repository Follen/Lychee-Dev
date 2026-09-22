package records_test

import (
	"bytes"
	"context"
	"crypto/md5"
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/records/container"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestContentPublication(t *testing.T) {
	for _, scenario := range []string{"verified", "wrong-key", "truncated", "cancelled", "limit"} {
		t.Run(scenario, func(t *testing.T) {
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			store, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "workspace"))
			if err != nil {
				t.Fatal(err)
			}
			body := []byte("record bytes\r\n\x00")
			hash := md5.Sum(body)
			raw := append([]byte{'B', 'L', 'T', 'E', 0, 0, 0, 0, 'N'}, body...)
			input := records.ContentInput{ContentKey: hex.EncodeToString(hash[:]), Limits: container.Limits{EncodedBytes: 4096, DecodedBytes: 4096, ChunkBytes: 4096, Chunks: 16, Depth: 4}}
			switch scenario {
			case "wrong-key":
				input.ContentKey = strings.Repeat("0", 32)
			case "truncated":
				raw = raw[:5]
			case "cancelled":
				cancel()
			case "limit":
				input.Limits.DecodedBytes = 2
			}
			input.Encoded = bytes.NewReader(raw)
			ref, err := records.OpenPayloadArchive(store).ExtractContent(ctx, input)
			if scenario == "verified" {
				if err != nil {
					t.Fatal(err)
				}
				got, err := store.ReadBlob(ctx, ref, 4096)
				if err != nil || !bytes.Equal(got, body) {
					t.Fatalf("content %q: %v", got, err)
				}
			} else {
				if err == nil || ref.SHA256 != "" {
					t.Fatalf("published failure: %+v %v", ref, err)
				}
				if scenario == "wrong-key" && !errors.Is(err, container.ErrIntegrity) {
					t.Fatal(err)
				}
				entries, err := os.ReadDir(filepath.Join(store.Root(), "blobs"))
				if err != nil || len(entries) != 0 {
					t.Fatalf("partial publication: %v %v", entries, err)
				}
			}
			entries, err := os.ReadDir(filepath.Join(store.Root(), "tmp"))
			if err != nil || len(entries) != 0 {
				t.Fatalf("staging leak: %v %v", entries, err)
			}
		})
	}
}

// Fixed independently generated Salsa20 ciphertext (PyCryptodome 3.23.0).
// No Python runtime is needed by the toolkit or this regression.
func TestEncryptedContentPublication(t *testing.T) {
	for _, compressed := range []bool{false, true} {
		for _, failure := range []string{"", "wrong-key", "wrong-content-key"} {
			ctx := context.Background()
			store, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "workspace"))
			if err != nil {
				t.Fatal(err)
			}
			encoded := "63a3a685d34d49920c2224620bf4f1f92ebda59d3ec7b0dd2f9aff"
			if compressed {
				encoded = "77ad5fdcf106eabba34e0c5d204bc3a7f0f4e9bcfcacdb9a330988c3e29b07b51f"
			}
			ciphertext, err := hex.DecodeString(encoded)
			if err != nil {
				t.Fatal(err)
			}
			raw := append([]byte("BLTE\x00\x00\x00\x00E\x08\x01\x00\x00\x00\x00\x00\x00\x00\x04\x00\x00\x00\x00S"), ciphertext...)
			key := make([]byte, 16)
			for i := range key {
				key[i] = byte(i)
			}
			if failure == "wrong-key" {
				key[0] ^= 1
			}
			body := []byte("verified encrypted payload")
			sum := md5.Sum(body)
			contentKey := hex.EncodeToString(sum[:])
			if failure == "wrong-content-key" {
				contentKey = strings.Repeat("0", 32)
			}
			ref, err := records.OpenPayloadArchive(store).ExtractContent(ctx, records.ContentInput{
				Encoded: bytes.NewReader(raw), ContentKey: contentKey,
				Limits: container.Limits{EncodedBytes: 4096, DecodedBytes: 4096, ChunkBytes: 4096, Chunks: 8, Depth: 4},
				Keys: func(_ context.Context, name uint64) ([]byte, error) {
					if name != 1 {
						t.Fatalf("key identity %x", name)
					}
					return key, nil
				},
			})
			if failure == "" {
				if err != nil {
					t.Fatal(err)
				}
				got, err := store.ReadBlob(ctx, ref, 4096)
				if err != nil || !bytes.Equal(got, body) {
					t.Fatalf("%q %v", got, err)
				}
			} else {
				if err == nil || ref.SHA256 != "" {
					t.Fatalf("failed input published: %+v %v", ref, err)
				}
				entries, err := os.ReadDir(filepath.Join(store.Root(), "blobs"))
				if err != nil || len(entries) != 0 {
					t.Fatalf("failed input retained: %v %v", entries, err)
				}
			}
			entries, err := os.ReadDir(filepath.Join(store.Root(), "tmp"))
			if err != nil || len(entries) != 0 {
				t.Fatalf("staging leak: %v %v", entries, err)
			}
		}
	}
}
