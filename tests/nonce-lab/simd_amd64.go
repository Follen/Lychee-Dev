//go:build amd64

package noncelab

import (
	"bytes"
	"golang.org/x/sys/cpu"
)

//go:noescape
func anchorAVX2(data []byte, first, last byte, distance int) int

func SIMDEnabled() bool { return cpu.X86.HasAVX2 }

// IndexSIMD tests 32 starting positions per vector step using two anchors.
// Exact matching and the tail remain bounds-checked Go code.
func IndexSIMD(data, pattern []byte) int {
	if !SIMDEnabled() || len(pattern) < 2 {
		return bytes.Index(data, pattern)
	}
	offset := 0
	for len(data)-len(pattern)+1 >= 32 {
		candidate := anchorAVX2(data, pattern[0], pattern[len(pattern)-1], len(pattern)-1)
		if candidate < 0 {
			scanned := (len(data) - len(pattern) + 1) / 32 * 32
			tail := bytes.Index(data[scanned:], pattern)
			if tail < 0 {
				return -1
			}
			return offset + scanned + tail
		}
		if bytes.Equal(data[candidate:candidate+len(pattern)], pattern) {
			return offset + candidate
		}
		consumed := candidate + 1
		data = data[consumed:]
		offset += consumed
	}
	tail := bytes.Index(data, pattern)
	if tail < 0 {
		return -1
	}
	return offset + tail
}
