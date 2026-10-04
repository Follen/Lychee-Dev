//go:build !windows || !amd64

package memory

import "io"

// Non-Windows binaries have no native publisher helper roles.
func RunMailboxMemoryHelper([]string, io.Reader, io.Writer) (bool, int) { return false, 0 }
