//go:build casc_oracle

// Test-only full-storage comparison. It is never shipped in the product CLI.
package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

const commit = "38a34665624b8775bb875274b36191b21c38d97b"

type manifest struct {
	Schema         string            `json:"schema"`
	Installation   string            `json:"installation"`
	Pin            selection.DataPin `json:"pin"`
	FileDataID     uint32            `json:"fileDataID"`
	LocaleMask     uint32            `json:"localeMask"`
	ContentVariant string            `json:"contentVariant"`
	CompleteKeys   bool              `json:"completeKeys"`
	KeyFile        string            `json:"keyFile"`
	KeySHA256      string            `json:"keySHA256"`
	ExpectedSHA256 string            `json:"expectedSHA256"`
	MaxBytes       int64             `json:"maxBytes"`
}

func main() {
	input := flag.String("manifest", "", "fixed case manifest")
	oracle := flag.String("oracle", "", "fixed CascLib executable")
	output := flag.String("output", "", "new result JSON")
	flag.Parse()
	if err := compare(*input, *oracle, *output); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func readJSON(path string, value any) error {
	f, err := os.Open(path)
	if err != nil {
		return err
	}
	defer f.Close()
	stat, err := f.Stat()
	if err != nil {
		return err
	}
	if stat.Size() > 1<<20 {
		return errors.New("bounded manifest required")
	}
	d := json.NewDecoder(f)
	d.DisallowUnknownFields()
	if err := d.Decode(value); err != nil {
		return err
	}
	var trailing any
	if err := d.Decode(&trailing); !errors.Is(err, io.EOF) {
		return errors.New("one bounded JSON document required")
	}
	return nil
}

func compare(input, oracle, output string) error {
	if runtime.GOOS != "windows" || runtime.GOARCH != "amd64" {
		return errors.New("Windows amd64 only")
	}
	if input == "" || oracle == "" || output == "" {
		return errors.New("manifest, oracle and new output required")
	}
	if _, err := os.Lstat(output); !errors.Is(err, os.ErrNotExist) {
		return errors.New("output must not exist")
	}
	var receipt struct{ Schema, Commit, Executable, ExecutableSHA256, AdapterSHA256, License, Platform string }
	if err := readJSON(filepath.Join(filepath.Dir(oracle), "oracle-build.json"), &receipt); err != nil {
		return err
	}
	raw, err := os.ReadFile(oracle)
	if err != nil {
		return err
	}
	digest := sha256.Sum256(raw)
	if receipt.Schema != "lycheedev.test.casc-oracle.v1" || receipt.Commit != commit || receipt.Platform != "windows-amd64" || hex.EncodeToString(digest[:]) != receipt.ExecutableSHA256 {
		return errors.New("oracle executable identity")
	}
	var request manifest
	if err := readJSON(input, &request); err != nil {
		return err
	}
	product, locale, err := selection.DataIdentity(request.Pin)
	if err != nil {
		return err
	}
	if request.Schema != "lycheedev.test.casc-case.v1" || request.Installation == "" || request.FileDataID == 0 || request.LocaleMask != locale || !request.CompleteKeys || request.MaxBytes < 1 || request.MaxBytes > 512<<20 || (request.ContentVariant != "standard" && request.ContentVariant != "low-violence") {
		return errors.New("explicit pinned identity, locale, variant, complete selected-file keys and byte bound required")
	}
	expected, err := hex.DecodeString(request.ExpectedSHA256)
	if err != nil || len(expected) != 32 {
		return errors.New("fixed expected raw SHA256 required")
	}
	keys := "-"
	if request.KeyFile != "" {
		if strings.EqualFold(filepath.Ext(request.KeyFile), ".json") {
			return errors.New("oracle common key file must be text: 16-digit name and 32-digit key")
		}
		data, err := os.ReadFile(request.KeyFile)
		if err != nil {
			return err
		}
		hash := sha256.Sum256(data)
		if len(data) > 4<<20 || hex.EncodeToString(hash[:]) != request.KeySHA256 {
			return errors.New("explicit key file digest mismatch")
		}
		keys = request.KeyFile
	}
	directory, err := os.MkdirTemp("", "lycheedev-casc-oracle-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(directory)
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()
	store, err := vault.Initialize(ctx, filepath.Join(directory, "workspace"))
	if err != nil {
		return err
	}
	q := records.FileQuery{Installation: request.Installation, Offline: true, FileDataID: request.FileDataID, ContentVariant: request.ContentVariant, MetadataBytes: 128 << 20, ContentBytes: request.MaxBytes, KeyFile: request.KeyFile}
	reading, err := records.OpenReader(store).ReadFile(ctx, request.Pin, q)
	if err != nil {
		return fmt.Errorf("strict Lychee selection failed; oracle cannot override it: %w", err)
	}
	if !reading.ContentVerified || reading.PartialContent != nil || len(reading.Missing) > 0 {
		return errors.New("complete content required; missing-key placeholders excluded")
	}
	lychee, err := store.ReadBlob(ctx, reading.Content, request.MaxBytes)
	if err != nil {
		return err
	}
	buildParts := strings.Split(request.Pin.FullBuild, ".")
	buildNumber := buildParts[len(buildParts)-1]
	cascOutput := filepath.Join(directory, "casc.bin")
	invocation := exec.CommandContext(ctx, oracle, "storage", request.Installation, product, request.Pin.BuildConfig, buildNumber, strconv.FormatUint(uint64(request.FileDataID), 10), strconv.FormatUint(uint64(locale), 10), request.ContentVariant, keys, cascOutput, strconv.FormatInt(request.MaxBytes, 10))
	metadataRaw, err := invocation.Output()
	if err != nil {
		return fmt.Errorf("CascLib intersection unavailable or failure: %w", err)
	}
	var metadata struct {
		Mode                                              string
		Bytes                                             int64
		ContentKey                                        string
		BuildNumber, FileDataID, LocaleMask, ContentFlags uint32
	}
	if err := json.Unmarshal(metadataRaw, &metadata); err != nil {
		return err
	}
	if metadata.Mode != "storage" || metadata.ContentKey != reading.Entry.ContentKey || metadata.FileDataID != request.FileDataID || metadata.LocaleMask&locale == 0 {
		return errors.New("oracle selected a different file/locale/content identity")
	}
	casc, err := os.ReadFile(cascOutput)
	if err != nil {
		return err
	}
	goHash, cascHash := sha256.Sum256(lychee), sha256.Sum256(casc)
	want := strings.ToLower(request.ExpectedSHA256)
	if hex.EncodeToString(goHash[:]) != want || hex.EncodeToString(cascHash[:]) != want || metadata.Bytes != int64(len(lychee)) {
		return fmt.Errorf("raw digest/size mismatch expected=%s lychee=%x casc=%x", want, goHash, cascHash)
	}
	result := map[string]any{"schema": "lycheedev.test.casc-comparison.v1", "state": "verified", "scope": "pinned-storage-file", "commit": commit, "pin": request.Pin, "fileDataID": request.FileDataID, "localeMask": locale, "contentVariant": request.ContentVariant, "contentKey": reading.Entry.ContentKey, "sha256": want, "bytes": len(lychee), "complete": true, "oracle": metadata}
	encoded, err := json.MarshalIndent(result, "", "  ")
	if err != nil {
		return err
	}
	f, err := os.OpenFile(output, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return err
	}
	if _, err = f.Write(append(encoded, '\n')); err != nil {
		f.Close()
		return err
	}
	return f.Close()
}
