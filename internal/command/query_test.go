package command

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/records/relational"
)

func TestQueryJSONContract(t *testing.T) {
	request, err := decodeDataQuery([]byte(`{"sql":"SELECT ID FROM t WHERE ID=:id","parameters":{"id":18446744073709551615}}`))
	if err != nil || request.Parameters["id"] != json.Number("18446744073709551615") {
		t.Fatal(request, err)
	}
	for _, raw := range []string{`{}`, `{"sql":null}`, `{"sql":"a","sql":"b"}`, `{"sql":"a","extra":1}`, `{"sql":"a","parameters":{"id":1,"id":2}}`, `{"sql":"a","parameters":{"id":[]}}`, `{"sql":"a","parameters":null}`, `{"sql":"a"} {}`} {
		if _, err := decodeDataQuery([]byte(raw)); err == nil {
			t.Fatal(raw)
		}
	}
}

func TestQueryStructuredErrors(t *testing.T) {
	for _, test := range []struct {
		cause error
		exit  int
		code  string
	}{
		{records.ErrRemoteRange, 4, "records.remote_range"}, {records.ErrRemoteObjectMissing, 3, "records.remote_object_missing"},
		{relational.ErrSyntax, 2, "query.invalid_syntax"}, {relational.ErrBinding, 2, "query.unresolved_binding"},
		{relational.ErrUnsupported, 3, "query.unsupported_expression"}, {relational.ErrBudget, 3, "query.budget_exceeded"},
		{relational.ErrType, 4, "query.type_mismatch"}, {relational.ErrNumericRange, 4, "query.numeric_range"}, {relational.ErrCardinality, 4, "query.invalid_cardinality"},
	} {
		exit, code, ok := queryFault(fmt.Errorf("wrapped: %w", test.cause))
		if !ok || exit != test.exit || code != test.code {
			t.Fatal(exit, code, ok)
		}
	}
	path := filepath.Join(t.TempDir(), "query.json")
	if err := os.WriteFile(path, []byte(`{"sql":"SELECT\nFROM x"}`), 0600); err != nil {
		t.Fatal(err)
	}
	result, code := invoke(t, "data", "sql", "--snapshot", "unused", "--installation", "unused", "--file", path, "--home", filepath.Join(t.TempDir(), "unopened"), "--format=json")
	if code != 2 || result.Error == nil || result.Error.Code != "query.invalid_syntax" || result.Error.Stage != "query" || result.Error.Location == nil || result.Error.Location.Line != 2 {
		t.Fatal(result, code)
	}
}

func TestDataSourceSelectionAdmission(t *testing.T) {
	for _, route := range [][]string{{"data", "db2", "--table", "Map"}, {"data", "sql", "--file", "unused.json"}, {"asset", "inspect", "--file-id", "1349477"}} {
		for _, source := range [][]string{nil, {"--cdn", "--installation", "unused"}, {"--cdn", "--cdn"}} {
			args := append(append([]string{}, route...), "--snapshot", "unused", "--format=json")
			result, code := invoke(t, append(args, source...)...)
			if code != 2 || result.OK || result.Error == nil || result.Error.Code != "command.invalid_arguments" {
				t.Fatalf("%v %v: exit=%d fault=%+v", route, source, code, result.Error)
			}
		}
		result, code := invoke(t, append(append([]string{}, route[:2]...), "--cdn", "--offline", "--help", "--format=json")...)
		if code != 0 || !result.OK {
			t.Fatalf("CDN flags not admitted: %v, exit=%d fault=%+v", route, code, result.Error)
		}
	}
	result, code := invoke(t, "data", "hotfix", "--cdn", "--format=json")
	if code != 2 || result.OK {
		t.Fatal("static CDN selector admitted by Hotfix", code)
	}
}

func TestQueryRouteArguments(t *testing.T) {
	for _, args := range [][]string{{"data", "sql"}, {"data", "sql", "--table", "x"}, {"data", "sql", "--id", "1"}, {"data", "sql", "--limit", "1"}, {"data", "sql", "--file-id", "1"}} {
		result, code := invoke(t, append(args, "--format=json")...)
		if code != 2 || result.OK {
			t.Fatal(args, result, code)
		}
	}
}
