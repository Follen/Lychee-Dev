// Command read is an offline differential-test adapter, not a product command.
// Input bytes are explicit fixtures; excluded sections must be specified from
// the authenticated missing-range evidence, never guessed from row values.
package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/records/table"
	"os"
	"sort"
	"strconv"
	"strings"
)

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
func run() error {
	db2 := flag.String("db2", "", "explicit DB2 fixture")
	dbd := flag.String("dbd", "", "explicit pinned DBD fixture")
	build := flag.String("build", "", "full build")
	skip := flag.String("skip", "", "unavailable partition indices from evidence")
	output := flag.String("output", "", "output JSON")
	flag.Parse()
	raw, err := os.ReadFile(*db2)
	if err != nil {
		return err
	}
	definition, err := os.ReadFile(*dbd)
	if err != nil {
		return err
	}
	ctx := context.Background()
	budget := table.Budget{FileBytes: 512 << 20, MetadataBytes: 64 << 20, Rows: 1000000, Columns: 4096, Partitions: 4096}
	layout, err := table.Inspect(ctx, bytes.NewReader(raw), int64(len(raw)), budget)
	if err != nil {
		return err
	}
	skipped := map[int]bool{}
	if *skip != "" {
		for _, part := range strings.Split(*skip, ",") {
			i, err := strconv.Atoi(part)
			if err != nil {
				return err
			}
			skipped[i] = true
		}
	}
	records, err := table.OpenAvailableRecords(ctx, bytes.NewReader(raw), int64(len(raw)), budget, skipped)
	if err != nil {
		return err
	}
	doc, err := schema.Parse(ctx, definition)
	if err != nil {
		return err
	}
	view, err := records.Bind(ctx, doc, *build)
	if err != nil {
		return err
	}
	digest := sha256.New()
	rows := []map[string]any{}
	err = view.ScanWithIDs(ctx, 4<<20, 1<<20, func(id uint32, row map[string]any) error {
		fmt.Fprintln(digest, id)
		if len(rows) < 200 {
			rows = append(rows, row)
		}
		return nil
	})
	if err != nil {
		return err
	}
	encrypted := map[string][]uint32{}
	for _, p := range layout.Partitions {
		if p.KeyID != 0 {
			key := fmt.Sprintf("%016x", p.KeyID)
			encrypted[key] = append(encrypted[key], p.EncryptedIDs...)
		}
	}
	for _, ids := range encrypted {
		sort.Slice(ids, func(i, j int) bool { return ids[i] < ids[j] })
	}
	result := map[string]any{"count": view.LogicalCount(), "idDigest": hex.EncodeToString(digest.Sum(nil)), "encryptedIDs": encrypted, "rows": rows}
	serialized, err := json.Marshal(result)
	if err != nil {
		return err
	}
	return os.WriteFile(*output, serialized, 0600)
}
