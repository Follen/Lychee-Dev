package records

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/follenfang/lycheedev/internal/records/container"
	"github.com/follenfang/lycheedev/internal/records/resource"
	"github.com/follenfang/lycheedev/internal/vault"
)

// Public keys are pinned independently of the game's immutable data pin.
// Upgrading this snapshot must update the commit, byte count and digest together.
const publicKeyCommit = "71b75360752840dd62a412013a38b5a972524a50"
const publicKeyURL = "https://raw.githubusercontent.com/wowdev/TACTKeys/" + publicKeyCommit + "/WoW.txt"

var publicKeyBlob = vault.BlobRef{SHA256: "6d83241ed776f9e74e27ed65d1ec52f1a1464a4e9b16bed497258fdd61020b52", Bytes: 983550}

type KeySource struct {
	Kind   string `json:"kind"`
	State  string `json:"state"`
	Commit string `json:"commit,omitempty"`
	SHA256 string `json:"sha256,omitempty"`
}

// prepareKeys loads explicit local keys once, or lazily loads the pinned public
// snapshot on first encrypted chunk. Offline never fetches. Only public key
// documents are cached; private material stays in this request's memory.
func (r *Reader) prepareKeys(ctx context.Context, q FileQuery) (container.KeyLookup, *KeySource, error) {
	if q.Keys != nil {
		if q.KeyFile != "" {
			return nil, nil, ErrRequestConflict
		}
		return q.Keys, &KeySource{Kind: "provided", State: "provided"}, nil
	}
	if q.KeyFile != "" {
		f, err := os.Open(q.KeyFile)
		if err != nil {
			return nil, nil, ErrKeyDocument
		}
		defer f.Close()
		stat, err := f.Stat()
		if err != nil || !stat.Mode().IsRegular() || stat.Size() > 4<<20 {
			return nil, nil, ErrKeyDocument
		}
		if err := reserveKeyDocument(q.budget, stat.Size()); err != nil {
			return nil, nil, err
		}
		raw, err := io.ReadAll(io.LimitReader(f, stat.Size()+1))
		if err != nil || int64(len(raw)) != stat.Size() {
			return nil, nil, ErrKeyDocument
		}
		digest := sha256.Sum256(raw)
		format := "text"
		if strings.EqualFold(filepath.Ext(q.KeyFile), ".json") {
			format = "json"
		}
		set, err := ReadKeySet(ctx, bytes.NewReader(raw), format, hex.EncodeToString(digest[:]))
		if err != nil {
			return nil, nil, err
		}
		return set.Lookup, &KeySource{Kind: "file", State: "loaded", SHA256: set.SHA256()}, nil
	}
	info := &KeySource{Kind: "public", State: "not_needed", Commit: publicKeyCommit, SHA256: publicKeyBlob.SHA256}
	var once sync.Once
	var set *KeySet
	var loadErr error
	lookup := func(ctx context.Context, id uint64) ([]byte, error) {
		once.Do(func() {
			if err := reserveKeyDocument(q.budget, publicKeyBlob.Bytes); err != nil {
				info.State, loadErr = "unavailable", err
				return
			}
			raw, err := r.store.ReadBlob(ctx, publicKeyBlob, 4<<20)
			if errors.Is(err, os.ErrNotExist) {
				if q.Offline {
					info.State = "unavailable_offline"
					loadErr = container.ErrKeyUnavailable
					return
				}
				raw, err = fetchPublicKeys(ctx, q.budget)
				if err == nil {
					_, err = r.store.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewReader(raw), MaxBytes: 4 << 20, ExpectedSHA256: publicKeyBlob.SHA256})
				}
			}
			if err == nil {
				set, err = ReadKeySet(ctx, bytes.NewReader(raw), "text", publicKeyBlob.SHA256)
			}
			loadErr = err
			info.State = "loaded"
			if err != nil {
				info.State = "unavailable"
			}
		})
		if loadErr != nil {
			return nil, loadErr
		}
		return set.Lookup(ctx, id)
	}
	return lookup, info, nil
}

func reserveKeyDocument(budget *resource.Budget, size int64) error {
	// Cover bounded input/read-copy growth, parsing buffers and the immutable
	// key map before any materialization. Public keys reserve only on first use.
	return budget.Charge(resource.Cost{RetainedBytes: size*8 + 64<<10, MetadataBytes: size*8 + 64<<10, DecodeWork: (size + 65535) / 65536})
}

func fetchPublicKeys(ctx context.Context, budgets ...*resource.Budget) ([]byte, error) {
	var budget *resource.Budget
	if len(budgets) > 0 {
		budget = budgets[0]
	}
	client := &http.Client{Timeout: 30 * time.Second, Transport: newBudgetTransport(nil, budget), CheckRedirect: func(req *http.Request, via []*http.Request) error {
		if len(via) >= 3 || req.URL.Scheme != "https" || req.URL.Host != "raw.githubusercontent.com" {
			return ErrKeyDocument
		}
		return nil
	}}
	return fetchPublicKeysWithClient(ctx, client)
}

func fetchPublicKeysWithClient(ctx context.Context, client *http.Client) ([]byte, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, publicKeyURL, nil)
	if err != nil {
		return nil, ErrKeyDocument
	}
	response, err := client.Do(req)
	if err != nil {
		if errors.Is(err, resource.ErrBudget) || errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
			return nil, err
		}
		return nil, ErrKeyDocument
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return nil, ErrKeyDocument
	}
	raw, err := io.ReadAll(io.LimitReader(response.Body, publicKeyBlob.Bytes+1))
	if errors.Is(err, resource.ErrBudget) || errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
		return nil, err
	}
	if err != nil || int64(len(raw)) != publicKeyBlob.Bytes {
		return nil, ErrKeyDocument
	}
	return raw, nil
}
