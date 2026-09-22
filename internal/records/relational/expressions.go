package relational

import "strings"

func (c *compiler) expression(minimum int) (*term, error) {
	if err := c.enter(); err != nil {
		return nil, err
	}
	defer func() { c.depth-- }()
	left, err := c.primary()
	if err != nil {
		return nil, err
	}
	for {
		p := c.peek()
		operation, precedence := infix(p)
		negated := false
		if c.word("NOT") && c.at+1 < len(c.pieces) {
			operation, precedence = infix(c.pieces[c.at+1])
			negated = operation == "IN" || operation == "BETWEEN" || operation == "LIKE"
			if !negated {
				break
			}
		}
		if precedence < minimum || operation == "" {
			break
		}
		c.at++
		if negated {
			c.at++
		}
		node := &term{kind: "operator", value: operation, offset: p.offset, args: []*term{left}}
		switch operation {
		case "IS":
			if c.takeWord("NOT") {
				node.value = "IS NOT"
			}
			if err := c.requireWord("NULL"); err != nil {
				return nil, err
			}
		case "BETWEEN":
			low, err := c.expression(precedence + 1)
			if err != nil {
				return nil, err
			}
			if err := c.requireWord("AND"); err != nil {
				return nil, err
			}
			high, err := c.expression(precedence + 1)
			if err != nil {
				return nil, err
			}
			node.args = append(node.args, low, high)
		case "IN":
			if err := c.requireSymbol("("); err != nil {
				return nil, err
			}
			if c.word("SELECT") || c.word("WITH") {
				node.query, err = c.retrieve()
				if err != nil {
					return nil, err
				}
			} else {
				for {
					value, err := c.expression(0)
					if err != nil {
						return nil, err
					}
					node.args = append(node.args, value)
					if !c.takeSymbol(",") {
						break
					}
				}
			}
			if err := c.requireSymbol(")"); err != nil {
				return nil, err
			}
		default:
			right, err := c.expression(precedence + 1)
			if err != nil {
				return nil, err
			}
			node.args = append(node.args, right)
		}
		if negated {
			node.value = "NOT " + node.value
		}
		left = node
	}
	return left, nil
}
func infix(p lexeme) (string, int) {
	if p.kind == word {
		switch strings.ToUpper(p.text) {
		case "OR":
			return "OR", 1
		case "AND":
			return "AND", 2
		case "IS", "IN", "BETWEEN", "LIKE":
			return strings.ToUpper(p.text), 3
		}
	}
	if p.kind == symbol {
		switch p.text {
		case "=", "!=", "<>", "<", ">", "<=", ">=":
			return p.text, 3
		case "||":
			return p.text, 4
		case "+", "-":
			return p.text, 5
		case "*", "/", "%":
			return p.text, 6
		}
	}
	return "", -1
}
func (c *compiler) primary() (*term, error) {
	p := c.peek()
	if c.takeWord("NOT") {
		value, err := c.expression(3)
		return &term{kind: "unary", value: "NOT", offset: p.offset, args: []*term{value}}, err
	}
	if c.takeSymbol("+") || c.takeSymbol("-") {
		value, err := c.expression(7)
		return &term{kind: "unary", value: p.text, offset: p.offset, args: []*term{value}}, err
	}
	if c.takeSymbol("(") {
		var node *term
		var err error
		if c.word("SELECT") || c.word("WITH") {
			var query *retrieval
			query, err = c.retrieve()
			node = &term{kind: "subquery", offset: p.offset, query: query}
		} else {
			node, err = c.expression(0)
		}
		if err != nil {
			return nil, err
		}
		if err = c.requireSymbol(")"); err != nil {
			return nil, err
		}
		return node, nil
	}
	if c.takeWord("EXISTS") {
		if err := c.requireSymbol("("); err != nil {
			return nil, err
		}
		query, err := c.retrieve()
		if err != nil {
			return nil, err
		}
		if err = c.requireSymbol(")"); err != nil {
			return nil, err
		}
		return &term{kind: "exists", query: query, offset: p.offset}, nil
	}
	if c.takeWord("CASE") {
		return c.caseValue(p.offset)
	}
	if c.takeWord("CAST") {
		if err := c.requireSymbol("("); err != nil {
			return nil, err
		}
		value, err := c.expression(0)
		if err != nil {
			return nil, err
		}
		if err := c.requireWord("AS"); err != nil {
			return nil, err
		}
		kind, err := c.name()
		if err != nil {
			return nil, err
		}
		if err := c.requireSymbol(")"); err != nil {
			return nil, err
		}
		return &term{kind: "cast", value: strings.ToUpper(kind), offset: p.offset, args: []*term{value}}, nil
	}
	if c.takeWord("NULL") || c.takeWord("TRUE") || c.takeWord("FALSE") {
		return &term{kind: "constant", value: strings.ToUpper(p.text), offset: p.offset}, nil
	}
	if p.kind == number || p.kind == textValue || p.kind == parameter {
		c.at++
		kind := "number"
		if p.kind == textValue {
			kind = "text"
		}
		if p.kind == parameter {
			kind = "parameter"
		}
		return &term{kind: kind, value: p.text, offset: p.offset}, nil
	}
	if c.takeSymbol("*") {
		return &term{kind: "star", offset: p.offset}, nil
	}
	name, err := c.name()
	if err != nil {
		return nil, err
	}
	if c.takeSymbol("(") {
		node := &term{kind: "call", value: strings.ToUpper(name), offset: p.offset, distinct: c.takeWord("DISTINCT")}
		if !c.takeSymbol(")") {
			for {
				value, err := c.expression(0)
				if err != nil {
					return nil, err
				}
				node.args = append(node.args, value)
				if !c.takeSymbol(",") {
					break
				}
			}
			if err := c.requireSymbol(")"); err != nil {
				return nil, err
			}
		}
		return node, nil
	}
	node := &term{kind: "column", value: name, offset: p.offset}
	if c.takeSymbol(".") {
		node.qualifier = name
		if c.takeSymbol("*") {
			node.kind = "star"
			node.value = ""
		} else {
			node.value, err = c.name()
			if err != nil {
				return nil, err
			}
		}
	}
	return node, nil
}
func (c *compiler) caseValue(offset int) (*term, error) {
	node := &term{kind: "case", offset: offset}
	if !c.word("WHEN") {
		value, err := c.expression(0)
		if err != nil {
			return nil, err
		}
		node.value = "simple"
		node.args = append(node.args, value)
	}
	branches := 0
	for c.takeWord("WHEN") {
		condition, err := c.expression(0)
		if err != nil {
			return nil, err
		}
		if err := c.requireWord("THEN"); err != nil {
			return nil, err
		}
		value, err := c.expression(0)
		if err != nil {
			return nil, err
		}
		node.args = append(node.args, condition, value)
		branches++
	}
	if branches == 0 {
		return nil, c.fail("CASE requires WHEN")
	}
	if c.takeWord("ELSE") {
		value, err := c.expression(0)
		if err != nil {
			return nil, err
		}
		node.args = append(node.args, value)
	} else {
		node.args = append(node.args, &term{kind: "constant", value: "NULL", offset: offset})
	}
	if err := c.requireWord("END"); err != nil {
		return nil, err
	}
	return node, nil
}
