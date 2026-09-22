package vault

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
)

func TestBlobRoundTripAndCorruption(t *testing.T) {
	ctx := context.Background()
	s, err := Initialize(ctx, filepath.Join(t.TempDir(), "workspace"))
	if err != nil {
		t.Fatal(err)
	}
	ref, err := s.PublishBlob(ctx, BlobInput{Reader: strings.NewReader("original\r\nbytes"), MaxBytes: 100})
	if err != nil {
		t.Fatal(err)
	}
	if err := s.VerifyBlob(ctx, ref); err != nil {
		t.Fatal(err)
	}
	data, err := s.ReadBlob(ctx, ref, 100)
	if err != nil || string(data) != "original\r\nbytes" {
		t.Fatalf("%q %v", data, err)
	}
	if _, err := s.ReadBlob(ctx, ref, 1); !errors.Is(err, ErrBlobLimit) {
		t.Fatal(err)
	}
	if err := os.WriteFile(s.blobPath(ref.SHA256), []byte("corrupt"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := s.VerifyBlob(ctx, ref); !errors.Is(err, ErrBlobIntegrity) {
		t.Fatal(err)
	}
	if _, err := s.PublishBlob(ctx, BlobInput{Reader: strings.NewReader("original\r\nbytes"), MaxBytes: 100}); !errors.Is(err, ErrBlobIntegrity) {
		t.Fatalf("corruption silently replaced: %v", err)
	}
}

func TestBlobRejectsIncompletePublication(t *testing.T) {
	ctx := context.Background()
	s, err := Initialize(ctx, filepath.Join(t.TempDir(), "workspace"))
	if err != nil {
		t.Fatal(err)
	}
	for _, input := range []BlobInput{
		{Reader: strings.NewReader("too much"), MaxBytes: 1},
		{Reader: strings.NewReader("wrong digest"), MaxBytes: 100, ExpectedSHA256: strings.Repeat("0", 64)},
	} {
		if _, err := s.PublishBlob(ctx, input); err == nil {
			t.Fatal("accepted invalid input")
		}
	}
	for _, dir := range []string{"tmp", "blobs"} {
		entries, err := os.ReadDir(filepath.Join(s.Root(), dir))
		if err != nil {
			t.Fatal(err)
		}
		if len(entries) != 0 {
			t.Fatalf("partial objects in %s: %v", dir, entries)
		}
	}
}

func TestConcurrentBlobPublication(t *testing.T) {
	ctx := context.Background()
	s, err := Initialize(ctx, filepath.Join(t.TempDir(), "workspace"))
	if err != nil {
		t.Fatal(err)
	}
	var group sync.WaitGroup
	for range 12 {
		group.Go(func() {
			ref, err := s.PublishBlob(ctx, BlobInput{Reader: strings.NewReader("shared bytes"), MaxBytes: 100})
			if err != nil {
				t.Error(err)
				return
			}
			if err := s.VerifyBlob(ctx, ref); err != nil {
				t.Error(err)
			}
		})
	}
	group.Wait()
}
