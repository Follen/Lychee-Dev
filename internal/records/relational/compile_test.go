package relational_test

import (
	"context"
	"errors"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/records/relational"
)

func TestCompileDependenciesAcrossCTEsExpressionSubqueriesAndSiblings(t *testing.T) {
	source := `SELECT *
FROM (
    WITH local AS (SELECT * FROM "catalog"."cte_source")
    SELECT * FROM local
    WHERE EXISTS (SELECT 1 FROM expr_source)
) AS derived
LEFT JOIN local ON derived.id = local.id
WHERE EXISTS (
    WITH local AS (SELECT * FROM "catalog"."expr_cte")
    SELECT * FROM local
)
UNION ALL SELECT * FROM union_source`

	program, err := relational.Compile(context.Background(), source)
	if err != nil {
		t.Fatal(err)
	}

	want := []relational.TableUse{
		{Catalog: "catalog", Name: "cte_source"},
		{Catalog: "catalog", Name: "expr_cte"},
		{Catalog: "static", Name: "expr_source"},
		{Catalog: "static", Name: "local"},
		{Catalog: "static", Name: "union_source"},
	}
	if got := program.SourceTables(); !reflect.DeepEqual(got, want) {
		t.Fatalf("SourceTables() = %#v, want %#v", got, want)
	}
}

func TestCompileSupportedRelationalClausesAndExpressions(t *testing.T) {
	source := `SELECT
    CASE WHEN a.score > :minimum THEN CAST(a.name AS TEXT) ELSE 'n/a' END AS label,
    COUNT(DISTINCT a.id) AS total
FROM "catalog".accounts AS a
INNER JOIN balances AS b ON a.id = b.account_id
LEFT OUTER JOIN flags AS f ON a.id = f.account_id
CROSS JOIN calendar AS c
WHERE a.enabled = TRUE
GROUP BY a.name
HAVING COUNT(*) > 1
ORDER BY total DESC NULLS LAST
LIMIT :page_size OFFSET 2
UNION ALL SELECT * FROM archive_accounts`

	program, err := relational.Compile(context.Background(), source)
	if err != nil {
		t.Fatal(err)
	}
	wantTables := []relational.TableUse{
		{Catalog: "catalog", Name: "accounts"},
		{Catalog: "static", Name: "archive_accounts"},
		{Catalog: "static", Name: "balances"},
		{Catalog: "static", Name: "calendar"},
		{Catalog: "static", Name: "flags"},
	}
	if got := program.SourceTables(); !reflect.DeepEqual(got, wantTables) {
		t.Fatalf("SourceTables() = %#v, want %#v", got, wantTables)
	}
	if got, want := program.NamedParameters(), []string{"minimum", "page_size"}; !reflect.DeepEqual(got, want) {
		t.Fatalf("NamedParameters() = %#v, want %#v", got, want)
	}
}

func TestCompileNamedParametersIncludeSubqueriesAndCounts(t *testing.T) {
	source := `SELECT :z, COUNT(:a)
FROM records
WHERE key IN (SELECT :query_value FROM matches LIMIT :limit OFFSET :offset)
  AND EXISTS (SELECT 1 FROM audit WHERE audit.key = :a)
  AND marker = :z`

	program, err := relational.Compile(context.Background(), source)
	if err != nil {
		t.Fatal(err)
	}
	want := []string{"a", "limit", "offset", "query_value", "z"}
	if got := program.NamedParameters(); !reflect.DeepEqual(got, want) {
		t.Fatalf("NamedParameters() = %#v, want %#v", got, want)
	}
}

func TestCompileQuotedKeywordIdentifiers(t *testing.T) {
	source := "SELECT \"SELECT\".\"FROM\" FROM \"CAT\".\"TABLE\" AS \"WHERE\" " +
		"INNER JOIN `JOIN` AS \"ON\" ON \"WHERE\".\"ID\" = \"ON\".\"ID\" " +
		"WHERE \"WHERE\".\"GROUP\" = :from"

	program, err := relational.Compile(context.Background(), source)
	if err != nil {
		t.Fatal(err)
	}
	wantTables := []relational.TableUse{
		{Catalog: "cat", Name: "TABLE"},
		{Catalog: "static", Name: "JOIN"},
	}
	if got := program.SourceTables(); !reflect.DeepEqual(got, wantTables) {
		t.Fatalf("SourceTables() = %#v, want %#v", got, wantTables)
	}
	if got, want := program.NamedParameters(), []string{"from"}; !reflect.DeepEqual(got, want) {
		t.Fatalf("NamedParameters() = %#v, want %#v", got, want)
	}
}

func TestCompileRejectsWritesAndMultipleStatements(t *testing.T) {
	cases := []string{
		"INSERT INTO t VALUES (1)",
		"UPDATE t SET value = 1",
		"DELETE FROM t",
		"CREATE TABLE t (id)",
		"DROP TABLE t",
		"PRAGMA user_version",
		"SELECT * FROM t; SELECT * FROM u",
		"SELECT * FROM t; DROP TABLE t",
		"SELECT * FROM t;;",
	}
	for _, source := range cases {
		t.Run(source, func(t *testing.T) {
			_, err := relational.Compile(context.Background(), source)
			if !errors.Is(err, relational.ErrSyntax) {
				t.Fatalf("Compile(%q) error = %v, want relational.ErrSyntax", source, err)
			}
		})
	}
}

func TestCompileRejectsMalformedLexicalInput(t *testing.T) {
	invalidUTF8 := "SELECT " + string([]byte{0xff}) + " FROM t"
	cases := []struct {
		name   string
		source string
	}{
		{name: "invalid utf8", source: invalidUTF8},
		{name: "nul", source: "SELECT " + string(rune(0)) + " FROM t"},
		{name: "unclosed string", source: "SELECT 'unterminated FROM t"},
		{name: "unclosed quoted identifier", source: "SELECT \"unterminated FROM t"},
		{name: "unclosed comment", source: "SELECT /* unterminated FROM t"},
		{name: "missing exponent", source: "SELECT 1e+ FROM t"},
		{name: "missing parameter name", source: "SELECT : FROM t"},
		{name: "unsupported pipe", source: "SELECT | FROM t"},
		{name: "empty quoted identifier", source: "SELECT \"\" FROM t"},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			_, err := relational.Compile(context.Background(), test.source)
			if !errors.Is(err, relational.ErrSyntax) {
				t.Fatalf("Compile(%q) error = %v, want relational.ErrSyntax", test.source, err)
			}
		})
	}
}

func TestCompileEnforcesSourceAndTokenBudgets(t *testing.T) {
	const maxSourceBytes = 256 << 10
	valid := "SELECT 1 FROM t"
	withinSourceBudget := strings.Repeat(" ", maxSourceBytes-len(valid)) + valid
	if len(withinSourceBudget) != maxSourceBytes {
		t.Fatalf("test setup produced %d bytes, want %d", len(withinSourceBudget), maxSourceBytes)
	}
	if _, err := relational.Compile(context.Background(), withinSourceBudget); err != nil {
		t.Fatalf("source at budget failed: %v", err)
	}

	if _, err := relational.Compile(context.Background(), withinSourceBudget+" "); !errors.Is(err, relational.ErrBudget) {
		t.Fatalf("source over budget error = %v, want relational.ErrBudget", err)
	}

	tooManyTokens := strings.Repeat("1 ", 32768)
	if _, err := relational.Compile(context.Background(), tooManyTokens); !errors.Is(err, relational.ErrBudget) {
		t.Fatalf("token budget error = %v, want relational.ErrBudget", err)
	}
}

func TestCompileEnforcesNestingBudget(t *testing.T) {
	source := "SELECT 1 FROM t"
	for i := 0; i < 130; i++ {
		source = "SELECT (" + source + ") FROM t"
	}
	if _, err := relational.Compile(context.Background(), source); !errors.Is(err, relational.ErrBudget) {
		t.Fatalf("nesting budget error = %v, want relational.ErrBudget", err)
	}
}

func TestCompileHonorsCancellationDuringCompilation(t *testing.T) {
	ctx := &cancelAfterChecksContext{cancelAt: 2, done: make(chan struct{})}
	_, err := relational.Compile(ctx, "SELECT "+strings.Repeat("1 ", 1000)+"FROM t")
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("Compile cancellation error = %v, want context.Canceled", err)
	}
}

type cancelAfterChecksContext struct {
	cancelAt int
	checks   int
	done     chan struct{}
	canceled bool
}

func (c *cancelAfterChecksContext) Deadline() (time.Time, bool) { return time.Time{}, false }
func (c *cancelAfterChecksContext) Done() <-chan struct{}       { return c.done }
func (c *cancelAfterChecksContext) Err() error {
	c.checks++
	if c.checks >= c.cancelAt {
		if !c.canceled {
			close(c.done)
			c.canceled = true
		}
		return context.Canceled
	}
	return nil
}
func (c *cancelAfterChecksContext) Value(key any) any { return nil }

func TestCompileMetadataResultsAreImmutable(t *testing.T) {
	program, err := relational.Compile(context.Background(), "SELECT * FROM catalog.accounts JOIN audit ON 1 = 1 WHERE id = :id")
	if err != nil {
		t.Fatal(err)
	}

	tables := program.SourceTables()
	parameters := program.NamedParameters()
	tables[0].Catalog = "changed"
	tables = append(tables, relational.TableUse{Catalog: "changed", Name: "table"})
	parameters[0] = "changed"
	parameters = append(parameters, "extra")

	wantTables := []relational.TableUse{
		{Catalog: "catalog", Name: "accounts"},
		{Catalog: "static", Name: "audit"},
	}
	if got := program.SourceTables(); !reflect.DeepEqual(got, wantTables) {
		t.Fatalf("SourceTables() changed through returned slice: %#v", got)
	}
	if got, want := program.NamedParameters(), []string{"id"}; !reflect.DeepEqual(got, want) {
		t.Fatalf("NamedParameters() changed through returned slice: %#v", got)
	}
}

func FuzzCompileBounded(f *testing.F) {
	for _, seed := range []string{
		"SELECT 1 FROM t",
		"WITH c AS (SELECT * FROM t) SELECT * FROM c",
		"SELECT * FROM t WHERE x IN (SELECT y FROM u LIMIT :n)",
		"SELECT 'unterminated",
	} {
		f.Add(seed)
	}

	const fuzzInputLimit = 8 << 10
	f.Fuzz(func(t *testing.T, source string) {
		if len(source) > fuzzInputLimit {
			source = source[:fuzzInputLimit]
		}
		_, _ = relational.Compile(context.Background(), source)
	})
}
