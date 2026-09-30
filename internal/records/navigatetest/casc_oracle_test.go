package navigatetest

import (
	"bytes"
	"compress/zlib"
	"context"
	"crypto/md5"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/records/container"
)

const cascOracleCommit = "38a34665624b8775bb875274b36191b21c38d97b"

// This is a decoder-component oracle, not storage/build/FDID acceptance. It
// compares a provenance-pinned real DB2 sample wrapped in legal BLTE shapes.
// Storage selection is covered only by the separate explicit manifest harness.
func TestCascLibRawFixtureOracle(t *testing.T) {
	oracle := os.Getenv("LYCHEEDEV_CASC_ORACLE")
	if oracle == "" {
		t.Skip("not_run: fixed CascLib executable not configured")
	}
	if runtime.GOOS != "windows" || runtime.GOARCH != "amd64" {
		t.Fatal("oracle accepts Windows amd64 only")
	}
	receiptRaw, err := os.ReadFile(filepath.Join(filepath.Dir(oracle), "oracle-build.json"))
	if err != nil {
		t.Fatal(err)
	}
	var receipt struct{ Schema, Commit, ExecutableSHA256 string }
	if err := json.Unmarshal(receiptRaw, &receipt); err != nil {
		t.Fatal(err)
	}
	executable, err := os.ReadFile(oracle)
	if err != nil {
		t.Fatal(err)
	}
	digest := sha256.Sum256(executable)
	if receipt.Schema != "lycheedev.test.casc-oracle.v1" || receipt.Commit != cascOracleCommit || hex.EncodeToString(digest[:]) != receipt.ExecutableSHA256 {
		t.Fatal("unverified oracle executable")
	}
	expected, err := os.ReadFile(realSamplePath)
	if err != nil {
		t.Fatal(err)
	}
	// Verify the underlying DB2 independently using its existing provenance test.
	TestRealSampleMatchesProvenanceManifest(t)
	shapes := map[string][]byte{"headered-N": oracleFramedObject(expected, append([]byte{'N'}, expected...)), "headered-Z": oracleZlibObject(t, expected)}
	for name, encoded := range shapes {
		t.Run(name, func(t *testing.T) {
			directory := t.TempDir()
			input := filepath.Join(directory, "input.blte")
			output := filepath.Join(directory, "casc-output.bin")
			if err := os.WriteFile(input, encoded, 0600); err != nil {
				t.Fatal(err)
			}
			ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
			defer cancel()
			raw, err := exec.CommandContext(ctx, oracle, "raw", input, output, "1048576").CombinedOutput()
			if err != nil {
				t.Fatalf("CascLib: %s %v", raw, err)
			}
			var metadata struct {
				Mode  string
				Bytes int64
			}
			if err := json.Unmarshal(raw, &metadata); err != nil {
				t.Fatal(err)
			}
			var lychee bytes.Buffer
			_, err = container.Decode(ctx, &lychee, bytes.NewReader(encoded), container.Limits{EncodedBytes: 1 << 20, DecodedBytes: 1 << 20, ChunkBytes: 1 << 20, Chunks: 32, Depth: 4})
			if err != nil {
				t.Fatal(err)
			}
			casc, err := os.ReadFile(output)
			if err != nil {
				t.Fatal(err)
			}
			want, goDigest, cascDigest := sha256.Sum256(expected), sha256.Sum256(lychee.Bytes()), sha256.Sum256(casc)
			if metadata.Mode != "raw" || metadata.Bytes != int64(len(expected)) || goDigest != want || cascDigest != want {
				t.Fatalf("mismatch metadata=%+v want=%x go=%x casc=%x", metadata, want, goDigest, cascDigest)
			}
			t.Logf("decoder-component verified: fixed commit %s bytes=%d sha256=%x", cascOracleCommit, len(expected), want)
		})
	}
	for name, encoded := range map[string][]byte{"checksum-failure": func() []byte { raw := oracleZlibObject(t, expected); raw[20] ^= 1; return raw }(), "missing-key-no-zero-fill": oracleFramedObject(expected, []byte{'E', 8, 0xfd, 0xfd, 0xfd, 0xfd, 0xfd, 0xfd, 0xfd, 0xfd, 4, 1, 2, 3, 4, 'S', 1})} {
		t.Run(name, func(t *testing.T) {
			directory := t.TempDir()
			input := filepath.Join(directory, "input.blte")
			output := filepath.Join(directory, "output.bin")
			if err := os.WriteFile(input, encoded, 0600); err != nil {
				t.Fatal(err)
			}
			ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
			defer cancel()
			var goOutput bytes.Buffer
			_, goErr := container.Decode(ctx, &goOutput, bytes.NewReader(encoded), container.Limits{EncodedBytes: 1 << 20, DecodedBytes: 1 << 20, ChunkBytes: 1 << 20, Chunks: 32, Depth: 4})
			if goErr == nil {
				t.Fatal("strict Go decode accepted invalid/unavailable content")
			}
			want := container.ErrIntegrity
			if name == "missing-key-no-zero-fill" {
				want = container.ErrKeyUnavailable
			}
			if !errors.Is(goErr, want) {
				t.Fatalf("unexpected rejection: %v wanted %v", goErr, want)
			}
			if name == "missing-key-no-zero-fill" {
				if goOutput.Len() != 0 {
					t.Fatal("missing-key decode emitted substitute bytes")
				}
				// CascOpenLocalFile has no storage key owner. The pinned raw
				// API crashes while resolving a missing key; it is outside this
				// component oracle's intersection. Storage uses a real owner.
				// Keep the product's strict policy independently verified.
				t.Logf("Go strict rejection verified: %v; raw missing-key oracle=not_run (no storage key owner)", goErr)
				return
			}
			message, err := exec.CommandContext(ctx, oracle, "raw", input, output, "1048576").CombinedOutput()
			if err == nil {
				t.Fatalf("oracle accepted invalid/unavailable content: %s", message)
			}
			if _, err := os.Stat(output); !os.IsNotExist(err) {
				t.Fatalf("failed oracle left output: %v", err)
			}
			t.Logf("strict decoder rejection: Go=%v oracle=%s", goErr, message)
		})
	}
}

func oracleZlibObject(t testing.TB, raw []byte) []byte {
	t.Helper()
	var compressed bytes.Buffer
	encoder := zlib.NewWriter(&compressed)
	if _, err := encoder.Write(raw); err != nil {
		t.Fatal(err)
	}
	if err := encoder.Close(); err != nil {
		t.Fatal(err)
	}
	frame := append([]byte{'Z'}, compressed.Bytes()...)
	return oracleFramedObject(raw, frame)
}

func oracleFramedObject(raw, frame []byte) []byte {
	out := make([]byte, 36)
	copy(out, "BLTE")
	binary.BigEndian.PutUint32(out[4:], 36)
	binary.BigEndian.PutUint32(out[8:], 0x0f000001)
	binary.BigEndian.PutUint32(out[12:], uint32(len(frame)))
	binary.BigEndian.PutUint32(out[16:], uint32(len(raw)))
	digest := md5.Sum(frame)
	copy(out[20:], digest[:])
	return append(out, frame...)
}
