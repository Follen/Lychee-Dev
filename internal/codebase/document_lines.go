package codebase

import (
	"context"
	"errors"
)

const maxLineMemoBytes = 8 << 20

// Positions use byte offsets into the verified document. Only the requested
// line is converted to a string; UTF-16 conversion remains at the LSP boundary.
func (c *snapshotCache) documentLine(ctx context.Context, name string, line int) (string, error) {
	data, _, err := c.document(ctx, name)
	if err != nil {
		return "", err
	}
	starts := c.lineMemo[name]
	if starts == nil {
		starts = []uint32{0}
		for i, b := range data {
			if i%(64<<10) == 0 {
				if err := ctx.Err(); err != nil {
					return "", err
				}
			}
			if b == '\n' {
				starts = append(starts, uint32(i+1))
			}
		}
		c.work.LineIndexBuilds++
		cost := len(starts) * 4
		if cost <= maxLineMemoBytes {
			if c.lineMemo == nil {
				c.lineMemo = make(map[string][]uint32)
			}
			if c.lineMemoBytes+cost > maxLineMemoBytes || len(c.lineMemo) >= maxDocumentMemoFiles {
				clear(c.lineMemo)
				c.lineMemoBytes = 0
			}
			c.lineMemo[name] = starts
			c.lineMemoBytes += cost
		}
	}
	if line < 1 || line > len(starts) {
		return "", errors.New("codebase.symbol_line_out_of_range")
	}
	end := len(data)
	if line < len(starts) {
		end = int(starts[line]) - 1
	}
	return string(data[int(starts[line-1]):end]), nil
}
