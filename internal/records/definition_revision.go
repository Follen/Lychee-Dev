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
	"net/url"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/follenfang/lycheedev/internal/vault"
)

const (
	defaultDefinitionReference = "refs/heads/master"
	definitionRevisionURL      = "https://api.github.com/repos/wowdev/WoWDBDefs/commits/"
	definitionRevisionMaxBody  = 128
)

type definitionRevisionDocument struct {
	Reference  string `json:"reference"`
	Commit     string `json:"commit"`
	Generation int64  `json:"-"`
}

// ResolveRevision turns a WoWDBDefs symbolic reference into an immutable
// commit. A symbolic reference is refreshed online on every call, while its
// last verified answer is available to offline callers under a key derived
// from that reference. Exact commit IDs are already immutable and return
// without touching the network or metadata.
func (d *Definitions) ResolveRevision(ctx context.Context, reference string, offline bool) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	reference, exact := normalizeDefinitionReference(reference)
	if reference == "" {
		return "", ErrDefinitionIdentity
	}
	if exact {
		return reference, nil
	}
	if d == nil || d.metadata == nil {
		return "", ErrDefinitionIdentity
	}

	key := definitionRevisionKey(reference)
	document, err := d.readDefinitionRevision(ctx, key, reference)
	haveCached := err == nil
	if err != nil && !errors.Is(err, vault.ErrMissingRecord) {
		return "", err
	}
	if offline {
		if haveCached {
			return document.Commit, nil
		}
		return "", ErrDefinitionUnavailable
	}

	commit, err := d.fetchDefinitionRevision(ctx, reference)
	if err != nil {
		return "", err
	}
	if err := ctx.Err(); err != nil {
		return "", err
	}
	payload, err := json.Marshal(definitionRevisionDocument{Reference: reference, Commit: commit})
	if err != nil {
		return "", err
	}
	expectedGeneration := int64(0)
	if haveCached {
		expectedGeneration = document.Generation
	}
	err = d.metadata.CommitDocuments(ctx, vault.Mutation{
		Key:                key,
		ExpectedGeneration: expectedGeneration,
		Value:              payload,
	})
	if err == nil {
		return commit, nil
	}
	if !errors.Is(err, vault.ErrGeneration) {
		return "", err
	}

	// The offline alias is only a remembered observation, not a global target.
	// Concurrent callers may observe different commits as a branch advances;
	// each must pin its own response rather than adopt another caller's target.
	_, readErr := d.readDefinitionRevision(ctx, key, reference)
	if readErr != nil {
		return "", readErr
	}
	return commit, nil
}

func normalizeDefinitionReference(reference string) (string, bool) {
	if reference == "" {
		return defaultDefinitionReference, false
	}
	if definitionCommit(reference) {
		return reference, true
	}
	if !validDefinitionReference(reference) {
		return "", false
	}
	return reference, false
}

func validDefinitionReference(reference string) bool {
	if reference == "" || len(reference) > 1024 || !utf8.ValidString(reference) {
		return false
	}
	if !strings.HasPrefix(reference, "refs/heads/") && !strings.HasPrefix(reference, "refs/tags/") {
		return false
	}
	suffix := strings.TrimPrefix(strings.TrimPrefix(reference, "refs/heads/"), "refs/tags/")
	if suffix == "" || strings.HasPrefix(suffix, "/") || strings.HasSuffix(suffix, "/") || strings.Contains(suffix, "//") {
		return false
	}
	if strings.Contains(suffix, "..") || strings.Contains(suffix, "@{") || strings.HasSuffix(suffix, ".") || strings.HasSuffix(suffix, ".lock") {
		return false
	}
	for _, component := range strings.Split(suffix, "/") {
		if component == "" || component == "." || component == ".." || strings.HasPrefix(component, ".") || strings.HasSuffix(component, ".") || strings.HasSuffix(component, ".lock") {
			return false
		}
	}
	for _, c := range reference {
		if unicode.IsControl(c) || c <= 0x20 || c == 0x7f || strings.ContainsRune("~^:?*[\\", c) {
			return false
		}
	}
	return true
}

func definitionCommit(value string) bool {
	if len(value) != 40 || strings.ToLower(value) != value {
		return false
	}
	for _, c := range value {
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f') {
			return false
		}
	}
	return true
}

func definitionRevisionKey(reference string) string {
	digest := sha256.Sum256([]byte(reference))
	return "definition-ref/" + hex.EncodeToString(digest[:])
}

func (d *Definitions) readDefinitionRevision(ctx context.Context, key, reference string) (definitionRevisionDocument, error) {
	doc, err := d.metadata.ReadDocument(ctx, key)
	if err != nil {
		return definitionRevisionDocument{}, err
	}
	var result definitionRevisionDocument
	if json.Unmarshal(doc.Value, &result) != nil || result.Reference != reference || !definitionCommit(result.Commit) {
		return definitionRevisionDocument{}, ErrDefinitionIdentity
	}
	result.Generation = doc.Generation
	return result, nil
}

func (d *Definitions) fetchDefinitionRevision(ctx context.Context, reference string) (string, error) {
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, definitionRevisionURL+url.PathEscape(reference), nil)
	if err != nil {
		return "", err
	}
	request.Header.Set("Accept", "application/vnd.github.sha")
	client := d.client
	if client == nil {
		return "", ErrDefinitionIdentity
	}
	clientCopy := *client
	clientCopy.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	response, err := clientCopy.Do(request)
	if err != nil {
		return "", err
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return "", fmt.Errorf("records.definition_http: %d", response.StatusCode)
	}
	raw, err := io.ReadAll(io.LimitReader(response.Body, definitionRevisionMaxBody+1))
	if err != nil {
		return "", err
	}
	if len(raw) > definitionRevisionMaxBody {
		return "", ErrMetadataLimit
	}
	commit := string(bytes.TrimRight(raw, "\r\n"))
	if !definitionCommit(commit) {
		return "", ErrDefinitionIdentity
	}
	return commit, nil
}
