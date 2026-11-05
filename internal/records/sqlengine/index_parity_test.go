package sqlengine

import (
	"context"
	"fmt"
	"reflect"
	"strings"
	"testing"
)

func TestIndexedPredicatesMatchFullScan(t *testing.T) {
	rows := [][]any{{uint64(0), int64(3), "zero"}, {uint64(1), int64(2), "one"}, {uint64(2), int64(1), "two"}, {uint64(35), int64(0), "last"}}
	predicates := []string{"ID=0", "ID=35.0", "35=ID", "ID=3.5e1", "ID IN (NULL,0,2,35,35)", "ID=2 AND Value=1", "ID=1 OR ID=2", "ID NOT IN (1,2)", "ID IN (SELECT ID FROM Sample WHERE Value<2)", "ID=2 AND EXISTS (SELECT 1 FROM Sample s WHERE s.ID=35)"}
	for _, predicate := range predicates {
		query := "SELECT ID,Value,Name FROM Sample WHERE " + predicate + " ORDER BY ID"
		t.Run(predicate, func(t *testing.T) {
			resolve := func(index bool) Resolver {
				return func(_ context.Context, _ TableUse) (Source, error) {
					source := fixedSource(rows)
					if index {
						source.Equal = func(_ context.Context, _ string, value any, yield func([]any) error) (bool, error) {
							for _, row := range rows {
								if row[0] == value {
									return true, yield(row)
								}
							}
							return true, nil
						}
					}
					return source, nil
				}
			}
			indexed, err := executeTest(t, query, nil, resolve(true))
			if err != nil {
				t.Fatal(err)
			}
			scanned, err := executeTest(t, query, nil, resolve(false))
			if err != nil {
				t.Fatal(err)
			}
			if !reflect.DeepEqual(indexed.Rows, scanned.Rows) {
				t.Fatalf("indexed=%v scan=%v", indexed.Rows, scanned.Rows)
			}
		})
	}
}

func TestNestedCTESourceBinding(t *testing.T) {
	queries := []string{
		`WITH a AS (SELECT ID FROM Sample), b AS (SELECT * FROM a) SELECT * FROM b`,
		`WITH t AS (SELECT ID FROM Sample) SELECT * FROM (WITH t AS (SELECT ID FROM Related) SELECT * FROM t)`,
	}
	for _, query := range queries {
		t.Run(query, func(t *testing.T) {
			result, err := executeTest(t, query, nil, func(_ context.Context, use TableUse) (Source, error) {
				if !strings.EqualFold(use.Name, "Sample") && !strings.EqualFold(use.Name, "Related") {
					return Source{}, fmt.Errorf("CTE treated as Data table: %v", use)
				}
				return fixedSource([][]any{{uint64(1), int64(2), "row"}}), nil
			})
			if err != nil || len(result.Rows) != 1 {
				t.Fatalf("result=%v err=%v", result, err)
			}
		})
	}
}
