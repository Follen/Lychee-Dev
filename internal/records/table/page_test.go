package table_test

import (
	"bytes"
	"context"
	"encoding/binary"
	"encoding/json"
	"errors"
	"reflect"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/records/table"
)

const maxPageID uint32 = ^uint32(0)

func adjustexternalIDs(raw []byte, ids ...uint32) []byte {
	_, _, start := fixture(3)
	for i, id := range ids {
		binary.LittleEndian.PutUint32(raw[start+8+i*4:start+12+i*4], id)
	}
	return raw
}

func pageView(t *testing.T) *table.View {
	t.Helper()
	raw := recordFixture(true, words(10, 0, 20, maxPageID), nil)
	raw = adjustexternalIDs(raw, 0, maxPageID)
	return bind(t, raw, definition(t, "int ID\nint Value", "$noninline,id$ID\nValue<32>"))
}

func rowArrayBytes(t *testing.T, rows []map[string]any) int {
	t.Helper()
	raw, err := json.Marshal(rows)
	if err != nil {
		t.Fatal(err)
	}
	return len(raw)
}

func wantRows(ids []uint32, values ...int64) []map[string]any {
	rows := make([]map[string]any, len(ids))
	for i, id := range ids {
		rows[i] = map[string]any{"ID": uint64(id), "Value": values[i]}
	}
	return rows
}

func TestPageOrdersLogicalIDsAndRepeatsStablePages(t *testing.T) {
	v := pageView(t)
	ctx := context.Background()

	first, err := v.Page(ctx, nil, 2, 16<<20)
	if err != nil {
		t.Fatal(err)
	}
	wantIDs := []uint32{0, 10}
	want := wantRows(wantIDs, 1, 1)
	if !reflect.DeepEqual(first.IDs, wantIDs) || !reflect.DeepEqual(first.Rows, want) || !first.More || first.Next == nil || *first.Next != 10 || first.After != nil {
		t.Fatalf("first page: %+v", first)
	}

	repeated, err := v.Page(ctx, nil, 2, 16<<20)
	if err != nil || !reflect.DeepEqual(repeated, first) {
		t.Fatalf("repeated page: %+v %v", repeated, err)
	}

	after := uint32(0)
	fromAfter, err := v.Page(ctx, &after, 2, 16<<20)
	if err != nil {
		t.Fatal(err)
	}
	after = maxPageID
	if !reflect.DeepEqual(fromAfter.IDs, []uint32{10, 20}) || fromAfter.After == nil || *fromAfter.After != 0 {
		t.Fatalf("exclusive cursor or cursor ownership: %+v", fromAfter)
	}

	second, err := v.Page(ctx, first.Next, 2, 16<<20)
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(second.IDs, []uint32{20, maxPageID}) || !reflect.DeepEqual(second.Rows, wantRows([]uint32{20, maxPageID}, 2, 2)) || second.More || second.Next != nil || second.After == nil || *second.After != 10 {
		t.Fatalf("second page: %+v", second)
	}

	last := maxPageID
	empty, err := v.Page(ctx, &last, 200, 2)
	if err != nil || len(empty.IDs) != 0 || len(empty.Rows) != 0 || empty.More || empty.Next != nil || empty.After == nil || *empty.After != maxPageID {
		t.Fatalf("empty suffix: %+v %v", empty, err)
	}
}

func TestPageOwnsCursorsIDsAndRowValues(t *testing.T) {
	v := pageView(t)
	input := uint32(0)
	page, err := v.Page(context.Background(), &input, 1, 16<<20)
	if err != nil {
		t.Fatal(err)
	}
	input = maxPageID
	if page.After == nil || *page.After != 0 || page.Next == nil || *page.Next != 10 {
		t.Fatalf("cursor aliases input or is missing: %+v", page)
	}

	page.IDs[0] = maxPageID
	page.Rows[0]["ID"] = uint64(maxPageID)
	again, err := v.Page(context.Background(), &[]uint32{0}[0], 1, 16<<20)
	if err != nil || !reflect.DeepEqual(again.IDs, []uint32{10}) || again.Rows[0]["ID"] != uint64(10) {
		t.Fatalf("page values are not owned: %+v %v", again, err)
	}

	sparse := bind(t, typedSparseFixture(), definition(t, "int ID\nstring Name\nint Score\nfloat Ratio\nint Flags\nint Parent", "$noninline,id$ID\nName\nScore<8>\nRatio\nFlags<u16>[2]\n$noninline,relation$Parent"))
	sparsePage, err := sparse.Page(context.Background(), nil, 2, 16<<20)
	if err != nil || len(sparsePage.Rows) != 2 {
		t.Fatalf("sparse page: %+v %v", sparsePage, err)
	}
	sparsePage.Rows[0]["Name"] = "changed"
	sparsePage.Rows[0]["Flags"].([]any)[0] = uint64(999)
	againSparse, err := sparse.Page(context.Background(), nil, 2, 16<<20)
	if err != nil || againSparse.Rows[0]["Name"] != "猫" || againSparse.Rows[0]["Flags"].([]any)[0] != uint64(3) {
		t.Fatalf("nested row values are not owned: %+v %v", againSparse, err)
	}
}

func TestPageUsesExactJSONRowArrayBudget(t *testing.T) {
	v := pageView(t)
	full, err := v.Page(context.Background(), nil, 2, 16<<20)
	if err != nil {
		t.Fatal(err)
	}
	exact := rowArrayBytes(t, full.Rows)
	if exact != 2+len(mustJSON(t, full.Rows[0]))+1+len(mustJSON(t, full.Rows[1])) {
		t.Fatalf("row-array budget omitted brackets or comma: %d", exact)
	}

	got, err := v.Page(context.Background(), nil, 2, exact)
	if err != nil || !reflect.DeepEqual(got, full) {
		t.Fatalf("exact budget: %+v %v", got, err)
	}
	got, err = v.Page(context.Background(), nil, 2, exact-1)
	if !errors.Is(err, table.ErrLimit) || !reflect.DeepEqual(got, table.RowPage{}) {
		t.Fatalf("budget below exact row array returned partial data: %+v %v", got, err)
	}

	firstRowBudget := rowArrayBytes(t, full.Rows[:1])
	got, err = v.Page(context.Background(), nil, 2, firstRowBudget+1)
	if !errors.Is(err, table.ErrLimit) || !reflect.DeepEqual(got, table.RowPage{}) {
		t.Fatalf("budget after first row returned partial data: %+v %v", got, err)
	}

	single, err := v.Page(context.Background(), nil, 1, 16<<20)
	if err != nil {
		t.Fatal(err)
	}
	singleExact := rowArrayBytes(t, single.Rows)
	if got, err := v.Page(context.Background(), nil, 1, singleExact); err != nil || !reflect.DeepEqual(got, single) {
		t.Fatalf("single-row exact budget: %+v %v", got, err)
	}
}

func mustJSON(t *testing.T, value any) []byte {
	t.Helper()
	raw, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

func TestPageValidatesBoundsCancellationAndEmptySuffix(t *testing.T) {
	v := pageView(t)
	ctx := context.Background()
	max := maxPageID

	for _, limit := range []int{1, 2, 199, 200} {
		for _, budget := range []int{2, 16 << 20} {
			got, err := v.Page(ctx, &max, limit, budget)
			if err != nil || len(got.IDs) != 0 || len(got.Rows) != 0 || got.More || got.Next != nil {
				t.Fatalf("valid empty suffix limit=%d budget=%d: %+v %v", limit, budget, got, err)
			}
		}
	}
	for limit := 1; limit <= 200; limit++ {
		if got, err := v.Page(ctx, &max, limit, 2); err != nil || len(got.IDs) != 0 {
			t.Fatalf("valid limit %d: %+v %v", limit, got, err)
		}
	}

	for _, limit := range []int{0, -1, 201} {
		got, err := v.Page(ctx, nil, limit, 16<<20)
		if !errors.Is(err, table.ErrLimit) || !reflect.DeepEqual(got, table.RowPage{}) {
			t.Fatalf("bad limit %d: %+v %v", limit, got, err)
		}
	}
	for _, budget := range []int{0, 1, 16<<20 + 1} {
		got, err := v.Page(ctx, nil, 1, budget)
		if !errors.Is(err, table.ErrLimit) || !reflect.DeepEqual(got, table.RowPage{}) {
			t.Fatalf("bad byte budget %d: %+v %v", budget, got, err)
		}
	}

	canceled, cancel := context.WithCancel(context.Background())
	cancel()
	got, err := v.Page(canceled, nil, 1, 16<<20)
	if !errors.Is(err, context.Canceled) || !reflect.DeepEqual(got, table.RowPage{}) {
		t.Fatalf("cancellation returned page data: %+v %v", got, err)
	}
}

func TestPageGivesEachRowTheOneMiBTextBudget(t *testing.T) {
	const textBytes = 1 << 20
	text := make([]byte, 0, 2*(textBytes+1))
	text = append(text, bytes.Repeat([]byte{'a'}, textBytes)...)
	text = append(text, 0)
	text = append(text, bytes.Repeat([]byte{'b'}, textBytes)...)
	text = append(text, 0)
	raw := stringFixture(3, text, 8, uint32(textBytes+5))
	budget := limits
	budget.FileBytes = 4 << 20
	v := bindWithBudget(t, raw, definition(t, "int ID\nstring Name", "$noninline,id$ID\nName"), budget)

	page, err := v.Page(context.Background(), nil, 2, 16<<20)
	if err != nil || !reflect.DeepEqual(page.IDs, []uint32{1, 2}) || len(page.Rows) != 2 {
		t.Fatalf("1 MiB row text page: %+v %v", page, err)
	}
	for i, want := range []byte{'a', 'b'} {
		name, ok := page.Rows[i]["Name"].(string)
		if !ok || len(name) != textBytes || name[0] != want || name[len(name)-1] != want {
			t.Fatalf("row %d text: type=%T length=%d", i, page.Rows[i]["Name"], len(name))
		}
	}
}

func bindWithBudget(t *testing.T, raw []byte, doc *schema.Document, budget table.Budget) *table.View {
	t.Helper()
	records, err := table.OpenRecords(context.Background(), bytes.NewReader(raw), int64(len(raw)), budget)
	if err != nil {
		t.Fatal(err)
	}
	v, err := records.Bind(context.Background(), doc, "12.1.0.69875")
	if err != nil {
		t.Fatal(err)
	}
	return v
}

func FuzzPageLimits(f *testing.F) {
	f.Add(uint8(1), uint32(0))
	f.Add(uint8(200), maxPageID)
	f.Add(uint8(0), uint32(10))
	f.Fuzz(func(t *testing.T, rawLimit uint8, after uint32) {
		v := pageView(t)
		limit := int(rawLimit)
		got, err := v.Page(context.Background(), &after, limit, 16<<20)
		if limit < 1 || limit > 200 {
			if !errors.Is(err, table.ErrLimit) || !reflect.DeepEqual(got, table.RowPage{}) {
				t.Fatalf("invalid limit %d: %+v %v", limit, got, err)
			}
			return
		}
		if err != nil {
			t.Fatal(err)
		}
		for i := 1; i < len(got.IDs); i++ {
			if got.IDs[i-1] >= got.IDs[i] {
				t.Fatalf("IDs not strictly ascending: %v", got.IDs)
			}
		}
		if len(got.IDs) != len(got.Rows) || len(got.IDs) > limit {
			t.Fatalf("page shape: %+v", got)
		}
	})
}
