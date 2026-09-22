package records

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/records/relational"
	"testing"
)

func TestQueryRejectsInvalidRequestBeforeWorkspace(t *testing.T) {
	for _, test := range []struct {
		query DataQuery
		bytes int64
		want  error
	}{
		{DataQuery{SQL: "DELETE FROM t"}, 1 << 20, relational.ErrSyntax},
		{DataQuery{SQL: "SELECT ID FROM t WHERE ID=:id"}, 1 << 20, relational.ErrBinding},
		{DataQuery{SQL: "SELECT ID FROM t", Parameters: map[string]any{"extra": 1}}, 1 << 20, relational.ErrBinding},
		{DataQuery{SQL: "SELECT ID FROM t"}, 0, relational.ErrBudget},
	} {
		result, err := QueryData(context.Background(), "must-not-open", "unused", FileQuery{Installation: "unused", ContentBytes: test.bytes, Offline: true}, test.query)
		if !errors.Is(err, test.want) || result.Result.Sources != nil {
			t.Fatal(result, err, test.want)
		}
	}
}
