package relational

import (
	"context"
	"errors"
	"strings"
)

var ErrCardinality = errors.New("relational.invalid_cardinality")

// Source describes one immutable, version-pinned table. Scan is synchronous;
// it must honor ctx and stop on yield's first error. Rows may be reused after
// yield returns. The adapter owns backing resources for the Execute lifetime.
type Source struct {
	Columns []string
	Scan    func(context.Context, func([]any) error) error
}
type Resolver func(context.Context, TableUse) (Source, error)
type Limits struct{ Work, MemoryBytes int64 }
type Result struct {
	Columns []string     `json:"columns"`
	Rows    [][]any      `json:"rows"`
	Plan    *Explanation `json:"plan,omitempty"`
}

// Execute never acquires ambient data or writes to a source. Provider errors,
// cancellation, semantic errors and budget exhaustion return no partial result.
func (p *Program) Execute(ctx context.Context, resolve Resolver, parameters map[string]any, limits Limits) (Result, error) {
	if err := ctx.Err(); err != nil {
		return Result{}, err
	}
	if p == nil || resolve == nil || limits.Work <= 0 || limits.MemoryBytes <= 0 || limits.MemoryBytes > 1<<30 {
		return Result{}, ErrBudget
	}
	e := &evaluation{ctx: ctx, remaining: limits.Work, retainedBytes: limits.MemoryBytes, parameters: make(map[string]any)}
	for _, name := range p.parameters {
		v, ok := parameters[name]
		if !ok {
			return Result{}, ErrBinding
		}
		v, err := canonicalScalar(v)
		if err != nil {
			return Result{}, err
		}
		e.parameters[name] = v
	}
	// A schema-only pass binds every expression before any Scan, including
	// unreachable CASE branches, empty subqueries and recursive members.
	// Reuse the same resolved immutable sources in both passes.
	cache := make(map[TableUse]Source)
	scans := make(map[TableUse]*ScanMeasurement)
	cached := func(ctx context.Context, use TableUse) (Source, error) {
		key := TableUse{Catalog: strings.ToLower(use.Catalog), Name: strings.ToLower(use.Name)}
		if source, ok := cache[key]; ok {
			return source, nil
		}
		source, err := resolve(ctx, use)
		if err != nil {
			return Source{}, err
		}
		cost := int64(128 + 32*len(source.Columns))
		for _, name := range source.Columns {
			cost += int64(len(name))
		}
		if cost > e.retainedBytes {
			return Source{}, ErrBudget
		}
		e.retainedBytes -= cost
		source.Columns = append([]string(nil), source.Columns...)
		if source.Scan != nil {
			measurement := &ScanMeasurement{Table: key}
			scans[key] = measurement
			scan := source.Scan
			source.Scan = func(ctx context.Context, yield func([]any) error) error {
				measurement.Calls++
				return scan(ctx, func(row []any) error { measurement.Rows++; return yield(row) })
			}
		}
		cache[key] = source
		return source, nil
	}
	e.planning = true
	bound, err := executeQuery(e, p.root, cached, nil)
	if err != nil {
		return Result{}, err
	}
	e.planning = false
	var explanation *Explanation
	if p.explain {
		explanation, err = p.explanation(e)
		if err != nil {
			return Result{}, err
		}
		explanation.OutputColumns = bound.Columns
		if !p.analyze {
			return Result{Plan: explanation}, nil
		}
	}
	result, err := executeQuery(e, p.root, cached, nil)
	if err != nil {
		return Result{}, err
	}
	if explanation != nil {
		explanation.Measured = measureExecution(scans, limits.Work-e.remaining, limits.MemoryBytes-e.retainedBytes, len(result.Rows))
		return Result{Plan: explanation}, nil
	}
	return result, nil
}

func executeSelect(e *evaluation, root *retrieval, resolve Resolver, scope *queryScope) (Result, error) {
	ctx := e.ctx
	previousSubquery := e.subquery
	previousFields := e.fields
	previousAllowed := e.groupAllowed
	e.groupAllowed = nil
	e.subquery = func(query *retrieval) (Result, error) {
		previous := e.outer
		e.outer = &rowScope{parent: previous, fields: e.fields, column: e.column, allowed: e.groupAllowed}
		defer func() { e.outer = previous }()
		return executeQuery(e, query, resolve, scope)
	}
	defer func() { e.subquery = previousSubquery; e.fields = previousFields; e.groupAllowed = previousAllowed }()
	inputs := []relation{root.input}
	for _, join := range root.links {
		inputs = append(inputs, join.input)
	}
	sources := make([]Source, 0, len(inputs))
	fields := []field{}
	widths := []int{}
	aliases := map[string]bool{}
	for _, input := range inputs {
		if err := e.spendMatch(); err != nil {
			return Result{}, err
		}
		alias := input.alias
		if alias == "" {
			alias = input.name
		}
		fold := strings.ToLower(alias)
		if aliases[fold] {
			return Result{}, ErrBinding
		}
		aliases[fold] = true
		catalog := input.catalog
		if catalog == "" {
			catalog = "static"
		}
		var source Source
		var err error
		if input.query != nil {
			var result Result
			result, err = executeQuery(e, input.query, resolve, scope)
			source = resultSource(result)
		} else if result, found := scope.lookup(input.name); input.catalog == "" && found {
			if result == nil {
				return Result{}, ErrBinding
			}
			source = resultSource(*result)
		} else {
			source, err = resolve(ctx, TableUse{Catalog: catalog, Name: input.name})
		}
		if err != nil {
			return Result{}, err
		}
		if source.Scan == nil || len(source.Columns) == 0 || len(fields)+len(source.Columns) > 4096 {
			return Result{}, ErrBinding
		}
		seen := map[string]bool{}
		for _, name := range source.Columns {
			key := strings.ToLower(name)
			if name == "" || seen[key] {
				return Result{}, ErrBinding
			}
			seen[key] = true
			fields = append(fields, field{qualifier: alias, name: name})
		}
		sources = append(sources, source)
		widths = append(widths, len(fields))
	}
	conditions := make([]*term, len(root.links))
	e.fields = fields
	for i, join := range root.links {
		condition, err := bindTerm(e, join.condition, fields[:widths[i+1]], false)
		if err != nil {
			return Result{}, err
		}
		conditions[i] = condition
	}
	query := *root
	query.links = nil
	query.bindings = nil
	query.input.query = nil
	stream := func(yield func([]any) error) error {
		// Materialize each right table once. Every retained cell is charged and
		// copied; no provider-owned mutable row is kept after its callback.
		rights := make([][][]any, len(sources)-1)
		for i := 1; i < len(sources); i++ {
			err := sources[i].Scan(ctx, func(row []any) error {
				if err := e.spendMatch(); err != nil {
					return err
				}
				if len(row) != len(sources[i].Columns) {
					return ErrBinding
				}
				owned := make([]any, len(row))
				cost := int64(64 + 32*len(row))
				for j, v := range row {
					scalar, err := canonicalScalar(v)
					if err != nil {
						return err
					}
					owned[j] = scalar
					if text, ok := scalar.(string); ok {
						cost += int64(len(text))
					}
				}
				if cost > e.retainedBytes {
					return ErrBudget
				}
				e.retainedBytes -= cost
				rights[i-1] = append(rights[i-1], owned)
				return nil
			})
			if err != nil {
				return err
			}
		}
		var joinRow func(int, []any) error
		joinRow = func(at int, left []any) error {
			if err := e.spendMatch(); err != nil {
				return err
			}
			if at == len(rights) {
				return yield(left)
			}
			matched := false
			for _, right := range rights[at] {
				if err := e.spendMatch(); err != nil {
					return err
				}
				row := make([]any, 0, len(left)+len(right))
				row = append(row, left...)
				row = append(row, right...)
				accept := true
				if conditions[at] != nil {
					previous := e.column
					previousFields := e.fields
					e.fields = fields[:len(row)]
					e.column = func(q, n string) (any, error) {
						index, err := fieldIndex(fields[:len(row)], q, n)
						if err != nil {
							return nil, err
						}
						return row[index], nil
					}
					v, err := e.value(conditions[at])
					e.column = previous
					e.fields = previousFields
					if err != nil {
						return err
					}
					truth, err := booleanState(v)
					if err != nil {
						return err
					}
					accept = truth == 1
				}
				if accept {
					matched = true
					if err := joinRow(at+1, row); err != nil {
						return err
					}
				}
			}
			if !matched && root.links[at].kind == "left" {
				row := make([]any, widths[at+1])
				copy(row, left)
				return joinRow(at+1, row)
			}
			return nil
		}
		return sources[0].Scan(ctx, func(row []any) error {
			if len(row) != len(sources[0].Columns) {
				return ErrBinding
			}
			return joinRow(0, row)
		})
	}
	result, err := executeRows(e, &query, fields, stream)
	if err != nil {
		return Result{}, err
	}
	return Result{Columns: result.names, Rows: result.rows}, nil
}
