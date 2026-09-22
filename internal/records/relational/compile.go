package relational

import "strings"

func (c *compiler) peek() lexeme { return c.pieces[c.at] }
func (c *compiler) word(value string) bool {
	return c.peek().kind == word && strings.EqualFold(c.peek().text, value)
}
func (c *compiler) takeWord(value string) bool {
	if c.word(value) {
		c.at++
		return true
	}
	return false
}
func (c *compiler) takeSymbol(value string) bool {
	if c.peek().kind == symbol && c.peek().text == value {
		c.at++
		return true
	}
	return false
}
func (c *compiler) fail(detail string) error {
	return problem(c.source, c.peek().offset, ErrSyntax, detail)
}
func (c *compiler) requireWord(value string) error {
	if !c.takeWord(value) {
		return c.fail("expected " + value)
	}
	return nil
}
func (c *compiler) requireSymbol(value string) error {
	if !c.takeSymbol(value) {
		return c.fail("expected " + value)
	}
	return nil
}
func (c *compiler) enter() error {
	if err := c.ctx.Err(); err != nil {
		return err
	}
	c.depth++
	if c.depth > 128 {
		return ErrBudget
	}
	return nil
}
func (c *compiler) name() (string, error) {
	p := c.peek()
	if p.kind != quoted && (p.kind != word || reserved(p.text)) {
		return "", c.fail("expected identifier")
	}
	c.at++
	return p.text, nil
}
func reserved(value string) bool {
	switch strings.ToUpper(value) {
	case "SELECT", "WITH", "RECURSIVE", "FROM", "WHERE", "GROUP", "BY", "HAVING", "ORDER", "LIMIT", "OFFSET", "UNION", "ALL", "AS", "JOIN", "INNER", "LEFT", "RIGHT", "FULL", "CROSS", "OUTER", "ON", "DISTINCT", "ASC", "DESC", "NULLS", "FIRST", "LAST", "AND", "OR", "NOT", "IS", "NULL", "IN", "BETWEEN", "LIKE", "EXISTS", "CASE", "WHEN", "THEN", "ELSE", "END", "CAST", "TRUE", "FALSE", "EXPLAIN", "ANALYZE":
		return true
	}
	return false
}
func (c *compiler) retrieve() (*retrieval, error) {
	if err := c.enter(); err != nil {
		return nil, err
	}
	defer func() { c.depth-- }()
	q := &retrieval{}
	if c.takeWord("WITH") {
		q.recursive = c.takeWord("RECURSIVE")
		names := map[string]bool{}
		for {
			name, err := c.name()
			if err != nil {
				return nil, err
			}
			key := strings.ToLower(name)
			if names[key] {
				return nil, c.fail("duplicate common table name")
			}
			names[key] = true
			if err := c.requireWord("AS"); err != nil {
				return nil, err
			}
			if err := c.requireSymbol("("); err != nil {
				return nil, err
			}
			query, err := c.retrieve()
			if err != nil {
				return nil, err
			}
			if err := c.requireSymbol(")"); err != nil {
				return nil, err
			}
			q.bindings = append(q.bindings, binding{name, query})
			if !c.takeSymbol(",") {
				break
			}
		}
	}
	if err := c.requireWord("SELECT"); err != nil {
		return nil, err
	}
	q.distinct = c.takeWord("DISTINCT")
	for {
		value, err := c.expression(0)
		if err != nil {
			return nil, err
		}
		alias, err := c.optionalAlias()
		if err != nil {
			return nil, err
		}
		q.outputs = append(q.outputs, projection{value, alias})
		if !c.takeSymbol(",") {
			break
		}
	}
	if err := c.requireWord("FROM"); err != nil {
		return nil, err
	}
	input, err := c.relation()
	if err != nil {
		return nil, err
	}
	q.input = input
	for {
		kind := ""
		switch {
		case c.takeWord("JOIN"):
			kind = "inner"
		case c.takeWord("INNER"):
			kind = "inner"
			err = c.requireWord("JOIN")
		case c.takeWord("LEFT"):
			kind = "left"
			c.takeWord("OUTER")
			err = c.requireWord("JOIN")
		case c.takeWord("CROSS"):
			kind = "cross"
			err = c.requireWord("JOIN")
		}
		if err != nil {
			return nil, err
		}
		if kind == "" {
			break
		}
		input, err := c.relation()
		if err != nil {
			return nil, err
		}
		joined := link{kind: kind, input: input}
		if kind != "cross" {
			if err := c.requireWord("ON"); err != nil {
				return nil, err
			}
			joined.condition, err = c.expression(0)
			if err != nil {
				return nil, err
			}
		}
		q.links = append(q.links, joined)
	}
	if c.takeWord("WHERE") {
		q.filter, err = c.expression(0)
		if err != nil {
			return nil, err
		}
	}
	if c.takeWord("GROUP") {
		if err := c.requireWord("BY"); err != nil {
			return nil, err
		}
		for {
			value, err := c.expression(0)
			if err != nil {
				return nil, err
			}
			q.groups = append(q.groups, value)
			if !c.takeSymbol(",") {
				break
			}
		}
	}
	if c.takeWord("HAVING") {
		q.having, err = c.expression(0)
		if err != nil {
			return nil, err
		}
	}
	if c.takeWord("ORDER") {
		if err := c.requireWord("BY"); err != nil {
			return nil, err
		}
		for {
			value, err := c.expression(0)
			if err != nil {
				return nil, err
			}
			order := ordering{value: value, descending: c.takeWord("DESC")}
			if !order.descending {
				c.takeWord("ASC")
			}
			if c.takeWord("NULLS") {
				if c.takeWord("FIRST") {
					order.nulls = "first"
				} else if c.takeWord("LAST") {
					order.nulls = "last"
				} else {
					return nil, c.fail("expected FIRST or LAST")
				}
			}
			q.order = append(q.order, order)
			if !c.takeSymbol(",") {
				break
			}
		}
	}
	if c.takeWord("LIMIT") {
		q.limit, err = c.count()
		if err != nil {
			return nil, err
		}
	}
	if c.takeWord("OFFSET") {
		q.offset, err = c.count()
		if err != nil {
			return nil, err
		}
	}
	if c.takeWord("UNION") {
		if err := c.requireWord("ALL"); err != nil {
			return nil, err
		}
		q.union, err = c.retrieve()
		if err != nil {
			return nil, err
		}
	}
	return q, nil
}
func (c *compiler) count() (*term, error) {
	p := c.peek()
	if p.kind != number && p.kind != parameter {
		return nil, c.fail("expected nonnegative integer or named parameter")
	}
	if p.kind == number {
		for _, r := range p.text {
			if r < '0' || r > '9' {
				return nil, c.fail("expected integer count")
			}
		}
	}
	c.at++
	kind := "number"
	if p.kind == parameter {
		kind = "parameter"
	}
	return &term{kind: kind, value: p.text, offset: p.offset}, nil
}
func (c *compiler) optionalAlias() (string, error) {
	if c.takeWord("AS") {
		return c.name()
	}
	if c.peek().kind == quoted || c.peek().kind == word && !reserved(c.peek().text) {
		return c.name()
	}
	return "", nil
}
func (c *compiler) relation() (relation, error) {
	var value relation
	if c.takeSymbol("(") {
		q, err := c.retrieve()
		if err != nil {
			return value, err
		}
		value.query = q
		if err := c.requireSymbol(")"); err != nil {
			return value, err
		}
	} else {
		name, err := c.name()
		if err != nil {
			return value, err
		}
		value.name = name
		if c.takeSymbol(".") {
			value.catalog = name
			value.name, err = c.name()
			if err != nil {
				return value, err
			}
		}
	}
	alias, err := c.optionalAlias()
	value.alias = alias
	return value, err
}
