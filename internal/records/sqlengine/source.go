// Package sqlengine executes read-only DuckDB queries over fixed Data sources.
package sqlengine

import (
	"context"
	"errors"
	"fmt"
	"sync"
	"sync/atomic"

	duckdb "github.com/duckdb/duckdb-go/v2"
)

// Kind is supplied by the DBD/metadata adapter, including for empty tables.
// Values never travel through float64 merely to cross the SQL boundary.
type Kind uint8

const (
	Signed Kind = iota + 1
	Unsigned
	Real
	Text
	WideInteger
)

type TableUse struct {
	Catalog string `json:"catalog"`
	Name    string `json:"name"`
}

// Source owns no SQL syntax or execution logic. Its rows are immutable until
// yield returns; Scan and Equal must honor cancellation and yield errors.
type Source struct {
	Columns      []string
	Types        []Kind
	Identity     string
	Scan         func(context.Context, func([]any) error) error
	ScanSelected func(context.Context, []int, func([]any) error) error
	Equal        func(context.Context, string, any, func([]any) error) (bool, error)
}

type Resolver func(context.Context, TableUse) (Source, error)

type ScanMeasurement struct {
	Table   TableUse `json:"table"`
	Calls   int64    `json:"calls"`
	Rows    int64    `json:"rows"`
	Lookups int64    `json:"lookups"`
}

type scanCounters struct {
	table                TableUse
	calls, rows, lookups atomic.Int64
}

type execution struct {
	ctx       context.Context
	workers   sync.WaitGroup
	rows      atomic.Int64
	rowLimit  int64
	mu        sync.Mutex
	sourceErr error
}

func (e *execution) fail(err error) {
	if err == nil {
		return
	}
	e.mu.Lock()
	defer e.mu.Unlock()
	if e.sourceErr == nil {
		e.sourceErr = err
	}
}

func (e *execution) failure() error {
	e.mu.Lock()
	defer e.mu.Unlock()
	return e.sourceErr
}

// A table function streams at most one owned row ahead of DuckDB. Cancellation
// releases blocked producers, including when LIMIT ends a scan early.
type tableStream struct {
	source  Source
	columns []duckdb.ColumnInfo
	exec    *execution
	stats   *scanCounters
	ids     []uint64
	indexed bool
	started bool
	rows    chan []any
	err     error
}

func columnInfo(source Source) ([]duckdb.ColumnInfo, error) {
	if source.Scan == nil || len(source.Columns) == 0 || len(source.Columns) > 4096 || len(source.Columns) != len(source.Types) {
		return nil, fmt.Errorf("%w: invalid source columns/types", ErrBinding)
	}
	columns := make([]duckdb.ColumnInfo, len(source.Columns))
	for i, name := range source.Columns {
		var kind duckdb.Type
		switch source.Types[i] {
		case Signed:
			kind = duckdb.TYPE_BIGINT
		case Unsigned:
			kind = duckdb.TYPE_UBIGINT
		case Real:
			kind = duckdb.TYPE_DOUBLE
		case Text:
			kind = duckdb.TYPE_VARCHAR
		case WideInteger:
			kind = duckdb.TYPE_HUGEINT
		default:
			return nil, fmt.Errorf("%w: column %s has no Data type", ErrType, name)
		}
		t, err := duckdb.NewTypeInfo(kind)
		if err != nil {
			return nil, err
		}
		columns[i] = duckdb.ColumnInfo{Name: name, T: t}
	}
	return columns, nil
}

func (s *tableStream) ColumnInfos() []duckdb.ColumnInfo     { return s.columns }
func (s *tableStream) Cardinality() *duckdb.CardinalityInfo { return nil }
func (s *tableStream) Init()                                {}

func (s *tableStream) start(row duckdb.Row) {
	s.started = true
	s.rows = make(chan []any)
	selected := make([]int, 0, len(s.columns))
	for i := range s.columns {
		if row.IsProjected(i) {
			selected = append(selected, i)
		}
	}
	s.exec.workers.Add(1)
	go func() {
		defer s.exec.workers.Done()
		defer close(s.rows)
		s.stats.calls.Add(1)
		yield := func(values []any) error {
			if err := s.exec.ctx.Err(); err != nil {
				return err
			}
			if len(values) != len(s.columns) {
				return fmt.Errorf("%w: source row width", ErrBinding)
			}
			if s.exec.rows.Add(1) > s.exec.rowLimit {
				return ErrBudget
			}
			s.stats.rows.Add(1)
			copy := append([]any(nil), values...)
			select {
			case s.rows <- copy:
				return nil
			case <-s.exec.ctx.Done():
				return s.exec.ctx.Err()
			}
		}
		if s.indexed {
			for _, id := range s.ids {
				s.stats.lookups.Add(1)
				handled, err := s.source.Equal(s.exec.ctx, s.source.Identity, id, yield)
				if err != nil {
					s.err = err
					break
				}
				if !handled {
					s.err = fmt.Errorf("%w: source declined declared identity index", ErrBinding)
					break
				}
			}
		} else if s.source.ScanSelected != nil {
			s.err = s.source.ScanSelected(s.exec.ctx, selected, yield)
		} else {
			s.err = s.source.Scan(s.exec.ctx, yield)
		}
		if !errors.Is(s.err, context.Canceled) || s.exec.ctx.Err() == nil {
			s.exec.fail(s.err)
		}
	}()
}

func (s *tableStream) FillRow(row duckdb.Row) (bool, error) {
	if !s.started {
		s.start(row)
	}
	select {
	case <-s.exec.ctx.Done():
		return false, s.exec.ctx.Err()
	case values, ok := <-s.rows:
		if !ok {
			return false, s.err
		}
		for i, value := range values {
			if !row.IsProjected(i) {
				continue
			}
			if s.source.Types[i] == WideInteger && value != nil {
				var err error
				value, err = wideInteger(value)
				if err != nil {
					s.exec.fail(err)
					return false, err
				}
			}
			if err := row.SetRowValue(i, value); err != nil {
				err = fmt.Errorf("%w: %s: %v", ErrType, s.source.Columns[i], err)
				s.exec.fail(err)
				return false, err
			}
		}
		return true, nil
	}
}
