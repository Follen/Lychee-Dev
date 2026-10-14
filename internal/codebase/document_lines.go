package codebase

import (
	"context"
	"errors"
	"fmt"
	"strings"
)

const maxLineMemoBytes = 8 << 20

// Positions use byte offsets into the verified document. Only the requested
// line is converted to a string; UTF-16 conversion remains at the LSP boundary.
func (c *snapshotCache) documentLine(ctx context.Context, name string, line int) (string, error) {
	data, _, err := c.document(ctx, name)
	if err != nil {
		return "", err
	}
	starts, err := c.documentLineStarts(ctx, name, data)
	if err != nil {
		return "", err
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

func (c *snapshotCache) documentLineStarts(ctx context.Context, name string, data []byte) ([]uint32, error) {
	starts := c.lineMemo[name]
	if starts == nil {
		starts = []uint32{0}
		for i, b := range data {
			if i%(64<<10) == 0 {
				if err := ctx.Err(); err != nil {
					return nil, err
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
	return starts, nil
}

// numberedLines renders only the requested lines and returns the verified
// document hash. Search, inspection and LSP positions share one bounded index.
func (c *snapshotCache) numberedLines(ctx context.Context, name string, start, end, maxLines int) (string, string, error) {
	data, hash, err := c.document(ctx, name)
	if err != nil {
		return "", "", err
	}
	starts, err := c.documentLineStarts(ctx, name, data)
	if err != nil {
		return "", "", err
	}
	if end < start {
		end = start
	}
	if maxLines > 0 && end-start+1 > maxLines {
		end = start + maxLines - 1
	}
	var out strings.Builder
	for n := start; n <= end && n <= len(starts); n++ {
		first, last := int(starts[n-1]), len(data)
		if n < len(starts) {
			last = int(starts[n]) - 1
			// Preserve CRLF normalization before the terminal-CR trim below.
			if last > first && data[last-1] == '\r' {
				last--
			}
		}
		if n > start {
			out.WriteByte('\n')
		}
		fmt.Fprintf(&out, "%d: %s", n, strings.TrimSuffix(string(data[first:last]), "\r"))
	}
	return out.String(), hash, nil
}
