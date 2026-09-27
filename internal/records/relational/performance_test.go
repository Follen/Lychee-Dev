package relational

import (
	"context"
	"fmt"
	"testing"
)

func generatedSource(n int) Source {
	return Source{Columns: []string{"ID", "Value"}, Scan: func(ctx context.Context, yield func([]any) error) error {
		for i := 0; i < n; i++ {
			if err := ctx.Err(); err != nil {
				return err
			}
			if err := yield([]any{int64(i), int64(n - i)}); err != nil {
				return err
			}
		}
		return nil
	}}
}

func BenchmarkQueries(b *testing.B) {
	for _, tc := range []struct {
		name, sql string
		n         int
	}{
		{"equality-join-1000", "SELECT COUNT(*) FROM a JOIN b ON a.ID=b.ID", 1000},
		{"top20-20000", "SELECT ID, Value FROM a ORDER BY Value LIMIT 20", 20000},
		{"id-20000", "SELECT Value FROM a WHERE ID=19999", 20000},
	} {
		b.Run(tc.name, func(b *testing.B) {
			ctx := context.Background()
			p, err := Compile(ctx, tc.sql)
			if err != nil {
				b.Fatal(err)
			}
			resolve := func(context.Context, TableUse) (Source, error) { return generatedSource(tc.n), nil }
			b.ReportAllocs()
			b.ResetTimer()
			for i := 0; i < b.N; i++ {
				result, err := p.Execute(ctx, resolve, nil, Limits{Work: 100000000, MemoryBytes: 64 << 20})
				if err != nil {
					b.Fatal(err)
				}
				if len(result.Rows) == 0 {
					b.Fatal(fmt.Errorf("empty benchmark result"))
				}
			}
		})
	}
}
