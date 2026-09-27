package relational

import (
	"fmt"
	"sort"
	"strings"
)

type BindingDiagnostic struct {
	Column     string   `json:"column"`
	Candidates []string `json:"candidates"`
}

func (d *BindingDiagnostic) Error() string {
	return fmt.Sprintf("%v: column %s; candidates: %s", ErrBinding, d.Column, strings.Join(d.Candidates, ", "))
}
func (d *BindingDiagnostic) Unwrap() error { return ErrBinding }

func missingColumn(fields []field, qualifier, name string) error {
	type candidate struct {
		name     string
		distance int
	}
	var candidates []candidate
	for _, f := range fields {
		if qualifier != "" && !strings.EqualFold(qualifier, f.qualifier) {
			continue
		}
		distance := columnDistance(strings.ToLower(name), strings.ToLower(f.name))
		if distance > 3 {
			continue
		}
		label := f.name
		if f.qualifier != "" {
			label = f.qualifier + "." + label
		}
		candidates = append(candidates, candidate{label, distance})
	}
	sort.Slice(candidates, func(i, j int) bool {
		if candidates[i].distance != candidates[j].distance {
			return candidates[i].distance < candidates[j].distance
		}
		return candidates[i].name < candidates[j].name
	})
	result := &BindingDiagnostic{Column: name, Candidates: []string{}}
	if qualifier != "" {
		result.Column = qualifier + "." + name
	}
	for _, c := range candidates {
		if len(result.Candidates) == 8 {
			break
		}
		result.Candidates = append(result.Candidates, c.name)
	}
	return result
}
func columnDistance(a, b string) int {
	x, y := []rune(a), []rune(b)
	if len(x) > 256 || len(y) > 256 {
		return 4
	}
	prev := make([]int, len(y)+1)
	for i := range prev {
		prev[i] = i
	}
	for i, c := range x {
		next := make([]int, len(y)+1)
		next[0] = i + 1
		for j, d := range y {
			cost := 0
			if c != d {
				cost = 1
			}
			next[j+1] = min(prev[j+1]+1, next[j]+1, prev[j]+cost)
		}
		prev = next
	}
	return prev[len(y)]
}
