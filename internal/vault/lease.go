package vault

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"time"
)

// Lease is an OS-owned lock. The file is deliberately retained: deleting it
// while another process holds its inode would create a second lock domain.
type Lease struct{ file *os.File }

var ErrLeaseBusy = errors.New("vault.lease_busy")

func (l *Lease) Close() error { return l.file.Close() }

// AcquireLease serializes one resource within an explicitly selected scope.
// Window callers must pass a machine-wide scope, not a workspace directory.
func AcquireLease(ctx context.Context, scope, resource string) (*Lease, error) {
	f, err := openLease(ctx, scope, resource)
	if err != nil {
		return nil, err
	}
	return waitLease(ctx, f)
}

// TryAcquireLease never queues behind another holder. A successful acquisition
// has exactly the same OS ownership and lifetime as AcquireLease.
func TryAcquireLease(ctx context.Context, scope, resource string) (*Lease, error) {
	f, err := openLease(ctx, scope, resource)
	if err != nil {
		return nil, err
	}
	locked, err := tryLease(f)
	if err != nil || !locked {
		if err == nil {
			err = ErrLeaseBusy
		}
		return nil, errors.Join(err, f.Close())
	}
	if err := ctx.Err(); err != nil {
		return nil, errors.Join(err, f.Close())
	}
	return &Lease{file: f}, nil
}

func openLease(ctx context.Context, scope, resource string) (*os.File, error) {
	if resource == "" {
		return nil, errors.New("vault: empty lock resource")
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if err := os.MkdirAll(scope, 0700); err != nil {
		return nil, err
	}
	digest := sha256.Sum256([]byte(resource))
	return os.OpenFile(filepath.Join(scope, hex.EncodeToString(digest[:])+".lock"), os.O_CREATE|os.O_RDWR, 0600)
}

func waitLease(ctx context.Context, f *os.File) (*Lease, error) {
	for {
		if err := ctx.Err(); err != nil {
			f.Close()
			return nil, err
		}
		locked, err := tryLease(f)
		if err != nil {
			f.Close()
			return nil, err
		}
		if locked {
			return &Lease{file: f}, nil
		}
		timer := time.NewTimer(25 * time.Millisecond)
		select {
		case <-ctx.Done():
			timer.Stop()
			f.Close()
			return nil, ctx.Err()
		case <-timer.C:
		}
	}
}
