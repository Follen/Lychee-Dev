//go:build !amd64

package noncelab

import "bytes"

func SIMDEnabled() bool                  { return false }
func IndexSIMD(data, pattern []byte) int { return bytes.Index(data, pattern) }
