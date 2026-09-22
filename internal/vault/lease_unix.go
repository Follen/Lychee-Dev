//go:build linux || darwin

package vault

import (
	"errors"
	"golang.org/x/sys/unix"
	"os"
)

func tryLease(f *os.File) (bool, error) {
	err := unix.Flock(int(f.Fd()), unix.LOCK_EX|unix.LOCK_NB)
	if errors.Is(err, unix.EWOULDBLOCK) {
		return false, nil
	}
	return err == nil, err
}
