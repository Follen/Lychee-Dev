package sqlengine

import (
	"context"
	"database/sql"
	"database/sql/driver"
	"encoding/json"
	"fmt"
	"io"
	"net/url"
	"strings"
	"unicode/utf8"

	duckdb "github.com/duckdb/duckdb-go/v2"
)

const Version = "2.0.0"

type Limits struct {
	MemoryBytes int64
	ResultBytes int64
	SourceRows  int64
}

type Result struct {
	Columns []string          `json:"columns"`
	Types   []string          `json:"types"`
	Rows    [][]any           `json:"rows"`
	Scans   []ScanMeasurement `json:"scans"`
	Plan    *Explanation      `json:"plan,omitempty"`
}

type Explanation struct {
	Engine  string            `json:"engine"`
	Version string            `json:"version"`
	Analyze bool              `json:"analyze"`
	Entries map[string]string `json:"entries"`
}

// Query owns one in-memory DuckDB connection. No persistent SQL database,
// extension download, external file or ambient network source is admitted.
type Query struct {
	db         *sql.DB
	conn       *sql.Conn
	sql        string
	explain    bool
	analyze    bool
	tables     []TableUse
	parameters map[string]any
	hints      []identityHint
	limits     Limits
}

func Open(ctx context.Context, source string, parameters map[string]any, limits Limits) (*Query, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if len(source) > 256<<10 || limits.MemoryBytes <= 0 || limits.MemoryBytes > 1<<30 || limits.ResultBytes <= 0 || limits.SourceRows <= 0 {
		return nil, ErrBudget
	}
	if !utf8.ValidString(source) || strings.ContainsRune(source, 0) {
		return nil, ErrSyntax
	}
	body, explain, analyze := queryBody(source)
	settings := url.Values{
		"threads": {"1"}, "memory_limit": {fmt.Sprintf("%dB", limits.MemoryBytes)},
		"autoinstall_known_extensions": {"false"}, "autoload_known_extensions": {"false"},
		"max_expression_depth": {"128"},
	}
	db, err := sql.Open("duckdb", "?"+settings.Encode())
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)
	conn, err := db.Conn(ctx)
	if err != nil {
		db.Close()
		return nil, engineError(ctx, err)
	}
	q := &Query{db: db, conn: conn, sql: body, explain: explain, analyze: analyze, limits: limits, parameters: map[string]any{}}
	ready := false
	defer func() {
		if !ready {
			q.Close()
		}
	}()
	// Configuration order matters: disabling external access also locks the
	// temporary-directory setting. Disable spill first, before any user SQL.
	for _, setting := range []string{"SET temp_directory = ''", "SET enable_external_access = false"} {
		if _, err := conn.ExecContext(ctx, setting); err != nil {
			return nil, engineError(ctx, err)
		}
	}
	var serialized string
	if err := conn.QueryRowContext(ctx, "SELECT json_serialize_sql(?)::VARCHAR", body).Scan(&serialized); err != nil {
		return nil, engineError(ctx, err)
	}
	if len(serialized) > 16<<20 {
		return nil, ErrBudget
	}
	tree, err := decodeSyntax(serialized)
	if err != nil {
		return nil, err
	}
	statement := tree.Statements[0]
	if len(statement.Parameters) != len(parameters) {
		return nil, fmt.Errorf("%w: supplied parameter names must exactly match the query", ErrBinding)
	}
	for _, parameter := range statement.Parameters {
		value, ok := parameters[parameter.Key]
		if !ok {
			return nil, fmt.Errorf("%w: missing $%s", ErrBinding, parameter.Key)
		}
		q.parameters[parameter.Key], err = parameterValue(value)
		if err != nil {
			return nil, err
		}
	}
	q.tables, err = sourceUses(statement.Node)
	if err != nil {
		return nil, err
	}
	q.hints = indexHints(statement.Node, q.parameters)
	for _, schema := range []string{"static", "effective", "meta"} {
		if _, err := conn.ExecContext(ctx, "CREATE SCHEMA "+identifier(schema)); err != nil {
			return nil, engineError(ctx, err)
		}
	}
	if _, err := conn.ExecContext(ctx, "SET lock_configuration = true"); err != nil {
		return nil, engineError(ctx, err)
	}
	ready = true
	return q, nil
}

func (q *Query) Close() error {
	if q.conn != nil {
		q.conn.Close()
		q.conn = nil
	}
	return q.db.Close()
}

func identifier(name string) string { return `"` + strings.ReplaceAll(name, `"`, `""`) + `"` }

func (q *Query) Execute(ctx context.Context, resolve Resolver) (Result, error) {
	ctx, cancel := context.WithCancel(ctx)
	execution := &execution{ctx: ctx, rowLimit: q.limits.SourceRows}
	defer func() {
		cancel()
		execution.workers.Wait()
	}()
	stats := make([]*scanCounters, 0, len(q.tables))
	for i, use := range q.tables {
		source, err := resolve(ctx, use)
		if err != nil {
			return Result{}, err
		}
		columns, err := columnInfo(source)
		if err != nil {
			return Result{}, err
		}
		measurement := &scanCounters{table: use}
		stats = append(stats, measurement)
		var ids []uint64
		indexed := false
		for _, hint := range q.hints {
			if sourceKey(hint.table) == sourceKey(use) && source.Equal != nil && source.Identity != "" && strings.EqualFold(hint.column, source.Identity) {
				ids, indexed = hint.ids, true
				break
			}
		}
		name := fmt.Sprintf("lychee_source_%d", i)
		function := duckdb.RowTableFunction{BindArguments: func(_ map[string]any, _ ...any) (duckdb.RowTableSource, error) {
			return &tableStream{source: source, columns: columns, exec: execution, stats: measurement, ids: ids, indexed: indexed}, nil
		}}
		if err := duckdb.RegisterTableUDF(q.conn, name, function); err != nil {
			return Result{}, engineError(ctx, err)
		}
		view := "CREATE VIEW " + identifier(use.Catalog) + "." + identifier(use.Name) + " AS SELECT * FROM " + identifier(name) + "()"
		if _, err := q.conn.ExecContext(ctx, view); err != nil {
			return Result{}, engineError(ctx, err)
		}
		if use.Catalog == "static" {
			view = "CREATE VIEW main." + identifier(use.Name) + " AS SELECT * FROM static." + identifier(use.Name)
			if _, err := q.conn.ExecContext(ctx, view); err != nil {
				return Result{}, engineError(ctx, err)
			}
		}
	}
	query := q.sql
	if q.explain {
		prefix := "EXPLAIN "
		if q.analyze {
			prefix += "ANALYZE "
		}
		query = prefix + query
	}
	result := Result{Columns: []string{}, Types: []string{}, Rows: [][]any{}, Scans: []ScanMeasurement{}}
	err := q.conn.Raw(func(connection any) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		// Unlike PrepareContext, Prepare rejects multiple statements without
		// executing earlier statements. QueryContext carries cancellation.
		statement, err := connection.(*duckdb.Conn).Prepare(query)
		if err != nil {
			return err
		}
		defer statement.Close()
		native := statement.(*duckdb.Stmt)
		kind, err := native.StatementType()
		if err != nil {
			return err
		}
		if kind != duckdb.STATEMENT_TYPE_SELECT && !(q.explain && kind == duckdb.STATEMENT_TYPE_EXPLAIN) {
			return ErrReadOnly
		}
		args := make([]driver.NamedValue, native.NumInput())
		for i := range args {
			name, err := native.ParamName(i + 1)
			if err != nil {
				return err
			}
			args[i] = driver.NamedValue{Ordinal: i + 1, Name: name, Value: q.parameters[name]}
		}
		rows, err := native.QueryContext(ctx, args)
		if err != nil {
			return err
		}
		defer rows.Close()
		result.Columns = rows.Columns()
		if len(result.Columns) > 4096 {
			return ErrBudget
		}
		types := rows.(driver.RowsColumnTypeDatabaseTypeName)
		for i := range result.Columns {
			result.Types = append(result.Types, types.ColumnTypeDatabaseTypeName(i))
		}
		var used int64
		values := make([]driver.Value, len(result.Columns))
		for {
			if err := ctx.Err(); err != nil {
				return err
			}
			if err := rows.Next(values); err != nil {
				if err == io.EOF {
					break
				}
				return err
			}
			row := make([]any, len(values))
			for i, value := range values {
				row[i], err = outputValue(value)
				if err != nil {
					return err
				}
			}
			encoded, err := json.Marshal(row)
			if err != nil {
				return err
			}
			used += int64(len(encoded) + 32 + 32*len(row))
			if used > q.limits.ResultBytes {
				return ErrBudget
			}
			result.Rows = append(result.Rows, row)
		}
		return nil
	})
	if err != nil {
		if failure := execution.failure(); failure != nil {
			return Result{}, failure
		}
		return Result{}, engineError(ctx, err)
	}
	for _, counters := range stats {
		result.Scans = append(result.Scans, ScanMeasurement{Table: counters.table, Calls: counters.calls.Load(), Rows: counters.rows.Load(), Lookups: counters.lookups.Load()})
	}
	if q.explain {
		plan := &Explanation{Engine: "duckdb", Version: Version, Analyze: q.analyze, Entries: map[string]string{}}
		for _, row := range result.Rows {
			if len(row) == 2 {
				key, keyOK := row[0].(string)
				value, valueOK := row[1].(string)
				if keyOK && valueOK {
					plan.Entries[key] = value
				}
			}
		}
		result.Plan = plan
		result.Columns, result.Types, result.Rows = []string{}, []string{}, [][]any{}
	}
	return result, nil
}
