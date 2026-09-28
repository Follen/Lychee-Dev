//go:build !amd64

package memory

import "bytes"

func SIMDEnabled() bool                  { return false }
func IndexSIMD(data, pattern []byte) int { return bytes.Index(data, pattern) }
