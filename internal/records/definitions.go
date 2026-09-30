package records

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/records/resource"
	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/vault"
)

var ErrDefinitionUnavailable = errors.New("records.definition_unavailable_offline")
var ErrDefinitionIdentity = errors.New("records.definition_identity")

type Definitions struct {
	budget   *resource.Budget
	store    *vault.Store
	metadata *vault.Metadata
	client   *http.Client
}

func (d *Definitions) WithBudget(b *resource.Budget) *Definitions { d.budget = b; return d }

func OpenDefinitions(store *vault.Store, metadata *vault.Metadata) *Definitions {
	return &Definitions{store: store, metadata: metadata, client: &http.Client{Timeout: 30 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}}
}

type DefinitionBundle struct {
	Commit     string               `json:"commit"`
	Identity   schema.TableIdentity `json:"identity"`
	Manifest   vault.BlobRef        `json:"manifest"`
	Definition vault.BlobRef        `json:"definition"`
}

// Prepare fetches only two regular text resources at an exact upstream commit.
// Cached source bytes are rehashed and reparsed, even in offline mode. A corrupt
// cache is an error, not a reason to silently download a replacement identity.
func (d *Definitions) Prepare(ctx context.Context, commit, name string, offline bool) (DefinitionBundle, error) {
	if err := ctx.Err(); err != nil {
		return DefinitionBundle{}, err
	}
	if d.store == nil || d.metadata == nil || len(commit) != 40 || strings.ToLower(commit) != commit {
		return DefinitionBundle{}, ErrDefinitionIdentity
	}
	if _, err := hex.DecodeString(commit); err != nil {
		return DefinitionBundle{}, ErrDefinitionIdentity
	}
	// Validate user input before network I/O; the request path later uses only
	// the canonical manifest value, whose parser independently validates names.
	if len(name) == 0 || len(name) > 128 {
		return DefinitionBundle{}, ErrDefinitionIdentity
	}
	for i, c := range name {
		if !(c >= 'A' && c <= 'Z' || c >= 'a' && c <= 'z' || c == '_' || i > 0 && (c >= '0' && c <= '9' || c == '-')) {
			return DefinitionBundle{}, ErrDefinitionIdentity
		}
	}
	base := "https://raw.githubusercontent.com/wowdev/WoWDBDefs/" + commit + "/"
	var manifest *schema.Manifest
	manifestRef, err := d.load(ctx, base+"manifest.json", offline, func(raw []byte) error { var err error; manifest, err = schema.ParseManifest(ctx, raw); return err })
	if err != nil {
		return DefinitionBundle{}, err
	}
	identity, err := manifest.Lookup(ctx, name)
	if err != nil {
		return DefinitionBundle{}, err
	}
	definitionRef, err := d.load(ctx, base+"definitions/"+identity.Name+".dbd", offline, func(raw []byte) error { _, err := schema.Parse(ctx, raw); return err })
	if err != nil {
		return DefinitionBundle{}, err
	}
	return DefinitionBundle{Commit: commit, Identity: identity, Manifest: manifestRef, Definition: definitionRef}, nil
}

type definitionObject struct {
	URL  string        `json:"url"`
	Blob vault.BlobRef `json:"blob"`
}

func (d *Definitions) load(ctx context.Context, url string, offline bool, validate func([]byte) error) (vault.BlobRef, error) {
	digest := sha256.Sum256([]byte(url))
	key := "definition/" + hex.EncodeToString(digest[:])
	read := func() (vault.BlobRef, error) {
		doc, err := d.metadata.ReadDocument(ctx, key)
		if err != nil {
			return vault.BlobRef{}, err
		}
		var object definitionObject
		if json.Unmarshal(doc.Value, &object) != nil || object.URL != url {
			return vault.BlobRef{}, ErrDefinitionIdentity
		}
		if object.Blob.Bytes < 0 || object.Blob.Bytes > 4<<20 {
			return vault.BlobRef{}, ErrMetadataLimit
		}
		if err := d.budget.Charge(resource.Cost{RetainedBytes: object.Blob.Bytes * 4, MetadataBytes: object.Blob.Bytes * 4, DecodeWork: (object.Blob.Bytes + 65535) / 65536}); err != nil {
			return vault.BlobRef{}, err
		}
		raw, err := d.store.ReadBlob(ctx, object.Blob, 4<<20)
		if err != nil {
			return vault.BlobRef{}, err
		}
		if err := validate(raw); err != nil {
			return vault.BlobRef{}, err
		}
		return object.Blob, nil
	}
	ref, err := read()
	if !errors.Is(err, vault.ErrMissingRecord) {
		return ref, err
	}
	if offline {
		return vault.BlobRef{}, ErrDefinitionUnavailable
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return vault.BlobRef{}, err
	}
	if err := d.budget.Charge(resource.Cost{NetworkRequests: 1}); err != nil {
		return vault.BlobRef{}, err
	}
	response, err := d.client.Do(request)
	if err != nil {
		return vault.BlobRef{}, err
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return vault.BlobRef{}, fmt.Errorf("records.definition_http: %d", response.StatusCode)
	}
	// The bounded read buffer is reserved before allocation, even when HTTP
	// omits Content-Length. Parsing is reserved separately before validation.
	if err := d.budget.Charge(resource.Cost{RetainedBytes: 8 << 20, MetadataBytes: 8 << 20}); err != nil {
		return vault.BlobRef{}, err
	}
	body := io.Reader(response.Body)
	if d.budget != nil {
		body = &definitionBudgetReader{source: response.Body, budget: d.budget}
	}
	raw, err := io.ReadAll(io.LimitReader(body, (4<<20)+1))
	if err != nil {
		return vault.BlobRef{}, err
	}
	if len(raw) > 4<<20 {
		return vault.BlobRef{}, ErrMetadataLimit
	}
	if err := d.budget.Charge(resource.Cost{RetainedBytes: int64(len(raw)) * 3, MetadataBytes: int64(len(raw)) * 3, DecodeWork: (int64(len(raw)) + 65535) / 65536}); err != nil {
		return vault.BlobRef{}, err
	}
	if err := validate(raw); err != nil {
		return vault.BlobRef{}, err
	}
	ref, err = d.store.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewReader(raw), MaxBytes: 4 << 20})
	if err != nil {
		return vault.BlobRef{}, err
	}
	serialized, err := json.Marshal(definitionObject{URL: url, Blob: ref})
	if err != nil {
		return vault.BlobRef{}, err
	}
	err = d.metadata.CommitDocuments(ctx, vault.Mutation{Key: key, Value: serialized})
	if errors.Is(err, vault.ErrGeneration) {
		other, readErr := read()
		if readErr != nil {
			return vault.BlobRef{}, readErr
		}
		if other != ref {
			return vault.BlobRef{}, ErrDefinitionIdentity
		}
		return other, nil
	}
	return ref, err
}

type definitionBudgetReader struct {
	source io.Reader
	budget *resource.Budget
}

func (r *definitionBudgetReader) Read(p []byte) (int, error) {
	remaining := r.budget.RemainingNetworkBytes()
	if int64(len(p)) > remaining+1 {
		p = p[:remaining+1]
	}
	n, err := r.source.Read(p)
	if charge := r.budget.Charge(resource.Cost{NetworkBytes: int64(n)}); charge != nil {
		return n, charge
	}
	return n, err
}
