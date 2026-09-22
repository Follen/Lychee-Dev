package records_test

import (
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/records"
)

func TestHotfixQueryValidationEdges(t *testing.T) {
	valid := wagoTestQuery()
	if err := valid.Validate(); err != nil {
		t.Fatalf("valid query rejected: %v", err)
	}
	cases := map[string]func(records.HotfixQuery) records.HotfixQuery{
		"unknown product": func(q records.HotfixQuery) records.HotfixQuery { q.Product = "ptr"; return q },
		"unknown region":  func(q records.HotfixQuery) records.HotfixQuery { q.Region = "xx"; return q },
		"unknown locale":  func(q records.HotfixQuery) records.HotfixQuery { q.Locale = "jaJP"; return q },
		"partial build":   func(q records.HotfixQuery) records.HotfixQuery { q.FullBuild = "1.2.3"; return q },
		"table and hash": func(q records.HotfixQuery) records.HotfixQuery {
			q.Table = "SpellEffect"
			q.TableHash = records.HotfixUint32(1)
			return q
		},
		"from after to": func(q records.HotfixQuery) records.HotfixQuery {
			from := time.Date(2026, 8, 6, 0, 0, 0, 0, time.UTC)
			to := time.Date(2026, 8, 5, 0, 0, 0, 0, time.UTC)
			q.From, q.To = &from, &to
			return q
		},
		"zero limit":    func(q records.HotfixQuery) records.HotfixQuery { q.Limit = 0; return q },
		"huge limit":    func(q records.HotfixQuery) records.HotfixQuery { q.Limit = 201; return q },
		"negative page": func(q records.HotfixQuery) records.HotfixQuery { q.Page = -1; return q },
	}
	for name, mutate := range cases {
		if err := mutate(wagoTestQuery()).Validate(); err == nil {
			t.Fatalf("%s: accepted", name)
		}
	}
}

func TestHotfixStatusFilterRange(t *testing.T) {
	for _, value := range []int{0, 1, 255} {
		status, err := records.HotfixStatusFilter(value)
		if err != nil || status == nil || int(*status) != value {
			t.Fatalf("status %d = %v err = %v", value, status, err)
		}
	}
	for _, value := range []int{-1, 256, 1000} {
		if status, err := records.HotfixStatusFilter(value); err == nil || status != nil {
			t.Fatalf("status %d accepted: %v", value, status)
		}
	}
}

func TestParseHotfixTimeFormats(t *testing.T) {
	rfc, err := records.ParseHotfixTime("2026-08-05T22:09:07Z")
	if err != nil || rfc.Year() != 2026 {
		t.Fatalf("RFC3339 = %v err = %v", rfc, err)
	}
	native, err := records.ParseHotfixTime("2026-08-05 22:09:07")
	if err != nil || !native.Equal(rfc) {
		t.Fatalf("native = %v err = %v", native, err)
	}
	if _, err := records.ParseHotfixTime("05/08/2026"); err == nil {
		t.Fatal("guessed format accepted")
	}
}

func TestHotfixCursorCodec(t *testing.T) {
	page := records.HotfixCursorPage{Page: 2, Key: strings.Repeat("c", 64), SHA256: strings.Repeat("d", 64), Total: 10, LastPage: 5, NextPageURL: "/hotfixes?page=3"}
	encoded, err := records.EncodeHotfixCursor(records.HotfixCursor{Source: "wago", Ref: strings.Repeat("a", 64), Scope: "scope", ResumePage: 3, After: "id:9", Previous: &page, Self: &page})
	if err != nil {
		t.Fatal(err)
	}
	decoded, err := records.DecodeHotfixCursor(encoded)
	if err != nil {
		t.Fatal(err)
	}
	if decoded.Schema != records.HotfixCursorSchema || decoded.Source != "wago" || decoded.ResumePage != 3 || decoded.After != "id:9" || decoded.Self == nil || decoded.Self.Page != 2 {
		t.Fatalf("decoded = %+v", decoded)
	}
	if _, err := records.DecodeHotfixCursor("not-base64!!"); err == nil {
		t.Fatal("malformed cursor accepted")
	}
	bad, err := records.EncodeHotfixCursor(records.HotfixCursor{Source: "wago", Ref: "short", ResumePage: 1})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := records.DecodeHotfixCursor(bad); err == nil || !strings.Contains(err.Error(), "records.hotfix_cursor") {
		t.Fatalf("malformed cursor error = %v", err)
	}
}

func TestCacheFilterFromQuery(t *testing.T) {
	query := wagoTestQuery()
	filter, err := records.CacheFilterFromQuery(query)
	if err != nil {
		t.Fatal(err)
	}
	if filter.Build != 68974 || filter.Limit != query.Limit {
		t.Fatalf("filter = %+v", filter)
	}
	query.Table = "SpellEffect"
	if _, err := records.CacheFilterFromQuery(query); err == nil {
		t.Fatal("table name accepted without hash")
	}
	query = wagoTestQuery()
	query.Search = "x"
	if _, err := records.CacheFilterFromQuery(query); err == nil {
		t.Fatal("search accepted for cache rows")
	}
	query = wagoTestQuery()
	query.PushID = records.HotfixInt32(-3)
	query.Status = records.HotfixUint8(2)
	query.RegionID = records.HotfixUint32(71)
	filter, err = records.CacheFilterFromQuery(query)
	if err != nil || filter.Push == nil || *filter.Push != -3 || filter.Status == nil || *filter.Status != 2 || filter.Region == nil || *filter.Region != 71 {
		t.Fatalf("filter = %+v err = %v", filter, err)
	}
}
