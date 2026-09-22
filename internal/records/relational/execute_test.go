package relational

import (
	"context"
	"errors"
	"reflect"
	"testing"
)

func fixtureResolver(_ context.Context, table TableUse) (Source, error) {
	var rows [][]any
	switch table.Name {
	case "a":
		rows = [][]any{{1, "one"}, {2, "two"}, {3, "three"}}
	case "b":
		rows = [][]any{{1, "x"}, {1, "y"}, {3, "z"}}
	default:
		return Source{}, ErrBinding
	}
	return Source{Columns: []string{"ID", "Name"}, Scan: func(ctx context.Context, yield func([]any) error) error {
		for _, row := range rows {
			if err := ctx.Err(); err != nil {
				return err
			}
			if err := yield(row); err != nil {
				return err
			}
		}
		return nil
	}}, nil
}

func TestExecuteJoins(t *testing.T) {
	for _, test := range []struct {
		sql  string
		want [][]any
	}{
		{"SELECT a.ID, b.Name FROM a INNER JOIN b ON a.ID=b.ID ORDER BY a.ID,b.Name", [][]any{{int64(1), "x"}, {int64(1), "y"}, {int64(3), "z"}}},
		{"SELECT a.ID, b.Name FROM a LEFT JOIN b ON a.ID=b.ID WHERE b.ID IS NULL", [][]any{{int64(2), nil}}},
		{"SELECT COUNT(*) FROM a CROSS JOIN b", [][]any{{int64(9)}}},
		{"SELECT a.Name, COUNT(b.ID) AS n FROM a LEFT JOIN b ON a.ID=b.ID GROUP BY a.Name ORDER BY n DESC,a.Name", [][]any{{"one", int64(2)}, {"three", int64(1)}, {"two", int64(0)}}},
	} {
		p, err := Compile(context.Background(), test.sql)
		if err != nil {
			t.Fatal(err)
		}
		got, err := p.Execute(context.Background(), fixtureResolver, nil, Limits{Work: 100000, MemoryBytes: 1 << 20})
		if err != nil || !reflect.DeepEqual(got.Rows, test.want) {
			t.Fatalf("%s: %#v %v", test.sql, got, err)
		}
	}
}

func TestExecuteBindingAndBudget(t *testing.T) {
	for _, sql := range []string{"SELECT ID FROM a JOIN b ON a.ID=b.ID", "SELECT a.ID FROM a JOIN b ON c.ID=a.ID", "SELECT a.ID FROM a a JOIN b a ON TRUE"} {
		p, err := Compile(context.Background(), sql)
		if err != nil {
			t.Fatal(err)
		}
		if result, err := p.Execute(context.Background(), fixtureResolver, nil, Limits{Work: 1000, MemoryBytes: 100000}); !errors.Is(err, ErrBinding) || result.Rows != nil {
			t.Fatal(sql, result, err)
		}
	}
	p, err := Compile(context.Background(), "SELECT * FROM a CROSS JOIN b")
	if err != nil {
		t.Fatal(err)
	}
	if result, err := p.Execute(context.Background(), fixtureResolver, nil, Limits{Work: 10, MemoryBytes: 100000}); !errors.Is(err, ErrBudget) || result.Rows != nil {
		t.Fatal(result, err)
	}
}
