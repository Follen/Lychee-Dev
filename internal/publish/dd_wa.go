package publish

// DD consumes the LibDeflate print alphabet, raw DEFLATE and either
// LibSerialize or AceSerializer. This parser never evaluates Lua.
import (
	"bytes"
	"compress/flate"
	"context"
	"encoding/binary"
	"io"
	"math"
	"strconv"
	"strings"
	"unicode/utf8"
)

type ddWAIdentity struct {
	UID string `json:"parse_wa_uid"`
	ID  string `json:"parse_wa_id"`
}
type ddWAValue struct {
	kind   byte
	text   string
	number float64
	table  int
}
type ddWAParser struct {
	ctx        context.Context
	b          []byte
	pos, count int
	tables     [][3]ddWAValue
	refs       []string
	err        error
}

func ddWAError() error {
	return &Error{Code: "publish.dd_wa", Message: "invalid or unsupported WA export", Exit: 4}
}
func parseDDWA(ctx context.Context, input string) (ddWAIdentity, error) {
	var zero ddWAIdentity
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	if len(input) > 32<<20 {
		return zero, ddWAError()
	}
	input = strings.TrimSpace(input)
	version := 1
	if strings.HasPrefix(input, "!WA:") {
		var v string
		var ok bool
		v, input, ok = strings.Cut(input[4:], "!")
		if !ok {
			return zero, ddWAError()
		}
		for _, c := range v {
			if c < '0' || c > '9' {
				return zero, ddWAError()
			}
		}
		n, err := strconv.Atoi(v)
		if err != nil || n < 0 || n > 2 {
			return zero, ddWAError()
		}
		version = n
	} else if strings.HasPrefix(input, "!") {
		input = input[1:]
	} else {
		return zero, ddWAError()
	}
	input = strings.TrimSpace(input)
	if len(input) == 1 {
		return zero, ddWAError()
	}
	alphabet := "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789()"
	compressed := make([]byte, 0, len(input)*3/4)
	var bits uint32
	var count uint
	for i := 0; i < len(input); i++ {
		n := strings.IndexByte(alphabet, input[i])
		if n < 0 {
			return zero, ddWAError()
		}
		bits |= uint32(n) << count
		count += 6
		if count >= 8 {
			compressed = append(compressed, byte(bits))
			bits >>= 8
			count -= 8
		}
	}
	r := flate.NewReader(bytes.NewReader(compressed))
	raw, err := io.ReadAll(io.LimitReader(r, (32<<20)+1))
	r.Close()
	if err != nil || len(raw) > 32<<20 {
		return zero, ddWAError()
	}
	p := &ddWAParser{ctx: ctx, b: raw}
	var root ddWAValue
	if version == 2 {
		if p.integer(1) > 2 {
			p.err = ddWAError()
		}
		root = p.value(0)
		for p.pos < len(p.b) && p.err == nil {
			p.value(0)
		}
	} else {
		filtered := make([]byte, 0, len(p.b))
		for _, b := range p.b {
			if b > 32 && b != 127 {
				filtered = append(filtered, b)
			}
		}
		p.b = filtered
		code, _ := p.token()
		if code != '1' {
			p.err = ddWAError()
		}
		code, s := p.token()
		root = p.ace(code, s, 0)
		for p.err == nil {
			code, s = p.token()
			if code == '^' {
				break
			}
			p.ace(code, s, 0)
		}
	}
	if p.err != nil {
		return zero, p.err
	}
	if err = ctx.Err(); err != nil {
		return zero, err
	}
	if root.kind != 't' {
		return zero, ddWAError()
	}
	d := p.tables[root.table][0]
	if d.kind != 't' {
		return zero, ddWAError()
	}
	uid, id := p.tables[d.table][1], p.tables[d.table][2]
	if uid.kind != 's' || id.kind != 's' || uid.text == "" || id.text == "" || !utf8.ValidString(uid.text) || !utf8.ValidString(id.text) {
		return zero, ddWAError()
	}
	return ddWAIdentity{uid.text, id.text}, nil
}
func (p *ddWAParser) tick(depth int) bool {
	p.count++
	if p.count > 1000000 || depth > 128 {
		p.err = ddWAError()
	}
	if p.count%256 == 0 && p.err == nil {
		p.err = p.ctx.Err()
	}
	return p.err == nil
}
func (p *ddWAParser) take(n int) []byte {
	if p.err != nil {
		return nil
	}
	if n < 0 || n > len(p.b)-p.pos {
		p.err = ddWAError()
		return nil
	}
	b := p.b[p.pos : p.pos+n]
	p.pos += n
	return b
}
func (p *ddWAParser) integer(n int) uint64 {
	var v uint64
	for _, b := range p.take(n) {
		v = v<<8 | uint64(b)
	}
	return v
}
func (p *ddWAParser) str(n int) ddWAValue {
	s := string(p.take(n))
	if n > 2 {
		p.refs = append(p.refs, s)
	}
	return ddWAValue{kind: 's', text: s}
}
func (p *ddWAParser) assign(t int, k, v ddWAValue) {
	if k.kind == 0 || k.kind == 'n' && math.IsNaN(k.number) {
		p.err = ddWAError()
		return
	}
	if k.kind == 's' {
		switch k.text {
		case "d":
			p.tables[t][0] = v
		case "uid":
			p.tables[t][1] = v
		case "id":
			p.tables[t][2] = v
		}
	}
}
func (p *ddWAParser) table(a, m, depth int) ddWAValue {
	if a > 1000000 || m > 500000 {
		p.err = ddWAError()
		return ddWAValue{}
	}
	idx := len(p.tables)
	p.tables = append(p.tables, [3]ddWAValue{})
	for i := 0; i < a && p.err == nil; i++ {
		p.value(depth + 1)
	}
	for i := 0; i < m && p.err == nil; i++ {
		k := p.value(depth + 1)
		v := p.value(depth + 1)
		p.assign(idx, k, v)
	}
	return ddWAValue{kind: 't', table: idx}
}
func (p *ddWAParser) number(s string) float64 {
	switch s {
	case "1.#INF", "+1.#INF":
		return math.Inf(1)
	case "-1.#INF":
		return math.Inf(-1)
	}
	n, err := strconv.ParseFloat(strings.TrimSpace(s), 64)
	if err != nil {
		p.err = ddWAError()
	}
	return n
}
func (p *ddWAParser) value(depth int) ddWAValue {
	if !p.tick(depth) {
		return ddWAValue{}
	}
	b := byte(p.integer(1))
	num := func(n float64) ddWAValue { return ddWAValue{kind: 'n', number: n} }
	if b&1 != 0 {
		return num(float64(b >> 1))
	}
	if b&3 == 2 {
		n := int(b >> 4)
		switch (b >> 2) & 3 {
		case 0:
			return p.str(n)
		case 1:
			return p.table(0, n, depth)
		case 2:
			return p.table(n, 0, depth)
		default:
			return p.table(n%4+1, n/4+1, depth)
		}
	}
	if b&7 == 4 {
		n := (p.integer(1)*256 + uint64(b) - 4) / 16
		if b&15 == 12 {
			return num(-float64(n))
		}
		return num(float64(n))
	}
	typ := b >> 3
	switch {
	case typ == 0:
		return ddWAValue{}
	case typ <= 8:
		n := p.integer([]int{2, 2, 3, 3, 4, 4, 7, 7}[typ-1])
		sign := 1.0
		if typ%2 == 0 {
			sign = -1
		}
		return num(sign * float64(n))
	case typ == 9:
		raw := p.take(8)
		if len(raw) != 8 {
			return ddWAValue{}
		}
		return num(math.Float64frombits(binary.BigEndian.Uint64(raw)))
	case typ == 10 || typ == 11:
		n := p.number(string(p.take(int(p.integer(1)))))
		if typ == 11 {
			n = -n
		}
		return num(n)
	case typ == 12 || typ == 13:
		return ddWAValue{kind: 'b'}
	case typ <= 16:
		return p.str(int(p.integer(int(typ - 13))))
	case typ <= 19:
		return p.table(0, int(p.integer(int(typ-16))), depth)
	case typ <= 22:
		return p.table(int(p.integer(int(typ-19))), 0, depth)
	case typ <= 25:
		n := int(typ - 22)
		a, m := p.integer(n), p.integer(n)
		return p.table(int(a), int(m), depth)
	case typ <= 28:
		i := p.integer(int(typ - 25))
		if i == 0 || i > uint64(len(p.refs)) {
			p.err = ddWAError()
			return ddWAValue{}
		}
		return ddWAValue{kind: 's', text: p.refs[i-1]}
	default:
		i := p.integer(int(typ - 28))
		if i == 0 || i > uint64(len(p.tables)) {
			p.err = ddWAError()
			return ddWAValue{}
		}
		return ddWAValue{kind: 't', table: int(i - 1)}
	}
}
func (p *ddWAParser) token() (byte, string) {
	if p.err != nil {
		return 0, ""
	}
	start := bytes.IndexByte(p.b[p.pos:], '^')
	if start < 0 || p.pos+start+1 >= len(p.b) {
		p.err = ddWAError()
		return 0, ""
	}
	start += p.pos
	code := p.b[start+1]
	start += 2
	end := bytes.IndexByte(p.b[start:], '^')
	if end < 0 {
		end = len(p.b) - start
	}
	p.pos = start + end
	return code, string(p.b[start:p.pos])
}
func (p *ddWAParser) ace(code byte, s string, depth int) ddWAValue {
	if !p.tick(depth) {
		return ddWAValue{}
	}
	switch code {
	case 'S':
		out := make([]byte, 0, len(s))
		for i := 0; i < len(s); i++ {
			b := s[i]
			if b == '~' && i+1 < len(s) {
				i++
				switch c := s[i]; {
				case c >= 64 && c <= 121:
					b = c - 64
				case c == 'z':
					b = 30
				case c == '{':
					b = 127
				case c == '|':
					b = 126
				case c == '}':
					b = 94
				default:
					p.err = ddWAError()
				}
			}
			out = append(out, b)
		}
		return ddWAValue{kind: 's', text: string(out)}
	case 'N':
		return ddWAValue{kind: 'n', number: p.number(s)}
	case 'F':
		n := p.number(s)
		c, e := p.token()
		if c != 'f' {
			p.err = ddWAError()
		}
		return ddWAValue{kind: 'n', number: n * math.Pow(2, p.number(e))}
	case 'B', 'b':
		return ddWAValue{kind: 'b'}
	case 'Z':
		return ddWAValue{}
	case 'T':
		idx := len(p.tables)
		p.tables = append(p.tables, [3]ddWAValue{})
		for p.err == nil {
			c, t := p.token()
			if c == 't' {
				break
			}
			k := p.ace(c, t, depth+1)
			c, t = p.token()
			v := p.ace(c, t, depth+1)
			if v.kind == 0 {
				p.err = ddWAError()
			}
			p.assign(idx, k, v)
		}
		return ddWAValue{kind: 't', table: idx}
	default:
		p.err = ddWAError()
		return ddWAValue{}
	}
}
