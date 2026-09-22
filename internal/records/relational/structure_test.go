package relational

import (
	"context"
	"errors"
	"strings"
	"testing"
)

func TestCompilerOperatorPrecedenceAndLiteralIdentity(t *testing.T) {
	p, err := Compile(context.Background(), "SELECT 18446744073709551615 AS MaxValue, -1+2*3 AS Value FROM T WHERE NOT A = :a OR B BETWEEN 1 AND 2 AND C NOT IN (3,4)")
	if err != nil {
		t.Fatal(err)
	}
	if p.root.outputs[0].value.value != "18446744073709551615" {
		t.Fatal("integer literal rounded")
	}
	math := p.root.outputs[1].value
	if math.value != "+" || math.args[0].kind != "unary" || math.args[0].value != "-" || math.args[1].value != "*" {
		t.Fatalf("wrong arithmetic tree: %+v", math)
	}
	filter := p.root.filter
	if filter.value != "OR" || filter.args[0].value != "NOT" || filter.args[0].args[0].value != "=" || filter.args[1].value != "AND" || filter.args[1].args[0].value != "BETWEEN" || filter.args[1].args[1].value != "NOT IN" {
		t.Fatal("wrong boolean precedence")
	}
}

func TestCompilerCaseAndExplain(t *testing.T) {
	p, err := Compile(context.Background(), "EXPLAIN ANALYZE SELECT CASE A WHEN 1 THEN 'one' WHEN 2 THEN 'two' ELSE 'other' END, CASE WHEN B IS NOT NULL THEN CAST(B AS TEXT) END FROM T")
	if err != nil {
		t.Fatal(err)
	}
	if !p.explain || !p.analyze {
		t.Fatal("explain metadata lost")
	}
	one, two := p.root.outputs[0].value, p.root.outputs[1].value
	if one.value != "simple" || len(one.args) != 6 || two.value != "" || len(two.args) != 3 || two.args[1].kind != "cast" || two.args[2].value != "NULL" {
		t.Fatal("case layout lost")
	}
}

func TestCompilerGuardsLeftDeepExpressions(t *testing.T) {
	_, err := Compile(context.Background(), "SELECT "+strings.Repeat("1+", 200)+"1 FROM T")
	if !errors.Is(err, ErrBudget) {
		t.Fatalf("unbounded left-deep expression: %v", err)
	}
}

func TestCompilerDiagnosticByteAndRunePosition(t *testing.T) {
	source := "SELECT 名 FROM T\r\nWHERE @"
	_, err := Compile(context.Background(), source)
	var diagnostic *Diagnostic
	if !errors.As(err, &diagnostic) || diagnostic.Line != 2 || diagnostic.Column != 7 || diagnostic.Offset != strings.Index(source, "@") {
		t.Fatalf("wrong location: %+v", err)
	}
}
