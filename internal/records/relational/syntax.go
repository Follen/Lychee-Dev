package relational

import "context"

// Program is immutable after compilation. Its syntax nodes are private so an
// execution caller cannot construct an unvalidated write or bypass node budgets.
type Program struct {
	root             *retrieval
	explain, analyze bool
	source           string
	tables           []TableUse
	parameters       []string
}
type retrieval struct {
	bindings            []binding
	recursive, distinct bool
	outputs             []projection
	input               relation
	links               []link
	filter, having      *term
	groups              []*term
	order               []ordering
	limit, offset       *term
	union               *retrieval
}
type binding struct {
	name  string
	query *retrieval
}
type projection struct {
	value *term
	alias string
}
type relation struct {
	catalog, name, alias string
	query                *retrieval
}
type link struct {
	kind      string
	input     relation
	condition *term
}
type ordering struct {
	value      *term
	descending bool
	nulls      string
}
type term struct {
	kind, value, qualifier string
	offset                 int
	args                   []*term
	query                  *retrieval
	distinct               bool
	outerDepth             int
}

type compiler struct {
	ctx       context.Context
	source    string
	pieces    []lexeme
	at, depth int
}

// Compile accepts one read-only retrieval, optionally EXPLAIN [ANALYZE]. It
// preserves numeric lexemes losslessly and rejects trailing statements. Parsing
// alone neither resolves tables nor authorizes or executes any external effect.
func Compile(ctx context.Context, source string) (*Program, error) {
	pieces, err := tokenize(ctx, source)
	if err != nil {
		return nil, err
	}
	c := &compiler{ctx: ctx, source: source, pieces: pieces}
	program := &Program{source: source}
	program.explain = c.takeWord("EXPLAIN")
	if program.explain {
		program.analyze = c.takeWord("ANALYZE")
	}
	program.root, err = c.retrieve()
	if err != nil {
		return nil, err
	}
	c.takeSymbol(";")
	if c.peek().kind != end {
		return nil, c.fail("expected end of statement")
	}
	program.tables, program.parameters, err = inspect(ctx, program.root)
	if err != nil {
		return nil, err
	}
	return program, nil
}
