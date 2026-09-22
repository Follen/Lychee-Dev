package relational

import "strings"

type rowScope struct {
	parent  *rowScope
	fields  []field
	column  func(string, string) (any, error)
	allowed map[field]bool
}

// A local qualifier shadows the outer qualifier even if its requested column
// is absent. An ambiguous local unqualified name must not fall through either.
func localColumn(fields []field, qualifier, name string) (int, bool, error) {
	index := -1
	owned := false
	for i, f := range fields {
		if qualifier != "" && !strings.EqualFold(qualifier, f.qualifier) {
			continue
		}
		if qualifier != "" {
			owned = true
		}
		if !strings.EqualFold(name, f.name) {
			continue
		}
		owned = true
		if index >= 0 {
			return -1, true, ErrBinding
		}
		index = i
	}
	if owned && index < 0 {
		return -1, true, ErrBinding
	}
	return index, owned, nil
}

func bindColumn(e *evaluation, fields []field, node *term) (*term, error) {
	copy := *node
	index, owned, err := localColumn(fields, node.qualifier, node.value)
	if err != nil {
		return nil, err
	}
	if owned {
		copy.qualifier, copy.value = fields[index].qualifier, fields[index].name
		return &copy, nil
	}
	depth := 0
	for scope := e.outer; scope != nil; scope = scope.parent {
		index, owned, err := localColumn(scope.fields, node.qualifier, node.value)
		if err != nil {
			return nil, err
		}
		if owned {
			if scope.allowed != nil && !scope.allowed[scope.fields[index]] {
				return nil, ErrBinding
			}
			copy.kind = "outer"
			copy.outerDepth = depth
			copy.qualifier, copy.value = scope.fields[index].qualifier, scope.fields[index].name
			return &copy, nil
		}
		depth++
	}
	return nil, ErrBinding
}
