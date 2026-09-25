package live

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"regexp"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

const probeRevisionPrefix = "PRB-"

var probeNamePattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$`)

// ProbeRevision is immutable source. Names are mutable selectors and are never
// stored in an operation; live work freezes this revision and its digest.
type ProbeRevision struct {
	Schema    string    `json:"schema"`
	ID        string    `json:"id"`
	SHA256    string    `json:"sha256"`
	Bytes     int       `json:"bytes"`
	Code      []byte    `json:"code,omitempty"`
	CreatedAt time.Time `json:"createdAt"`
}

type ProbeName struct {
	Schema    string    `json:"schema"`
	Name      string    `json:"name"`
	Revision  string    `json:"revision"`
	Active    bool      `json:"active"`
	UpdatedAt time.Time `json:"updatedAt"`
}

type ProbePutResult struct {
	Name     ProbeName     `json:"name"`
	Revision ProbeRevision `json:"revision"`
	Changed  bool          `json:"changed"`
}

type ProbeListResult struct {
	Items []ProbeName `json:"items"`
}

func PutProbe(ctx context.Context, root, name string, code []byte) (ProbePutResult, error) {
	var zero ProbePutResult
	if !probeNamePattern.MatchString(name) {
		return zero, errors.New("live.probe_name_invalid")
	}
	if len(code) == 0 || len(code) > 256<<10 {
		return zero, errors.New("bridge.queue_invalid_code")
	}
	digest := sha256.Sum256(code)
	hexDigest := hex.EncodeToString(digest[:])
	revision := ProbeRevision{
		Schema: "lycheedev.probe-revision.v1", ID: probeRevisionPrefix + hexDigest,
		SHA256: hexDigest, Bytes: len(code), Code: append([]byte(nil), code...), CreatedAt: time.Now().UTC(),
	}
	return vault.WriteMetadata(ctx, root, func(_ *vault.Store, metadata *vault.Metadata) (ProbePutResult, error) {
		revisionKey := "probe/revision/" + revision.ID
		revisionDoc, revisionErr := metadata.ReadDocument(ctx, revisionKey)
		if revisionErr == nil {
			var existing ProbeRevision
			if err := json.Unmarshal(revisionDoc.Value, &existing); err != nil || !validProbeRevision(existing) || existing.ID != revision.ID || existing.SHA256 != hexDigest || string(existing.Code) != string(code) {
				return zero, errors.New("live.probe_revision_corrupt")
			}
			revision = existing
		} else if !errors.Is(revisionErr, vault.ErrMissingRecord) {
			return zero, revisionErr
		}

		nameKey := "probe/name/" + name
		nameDoc, nameErr := metadata.ReadDocument(ctx, nameKey)
		if nameErr != nil && !errors.Is(nameErr, vault.ErrMissingRecord) {
			return zero, nameErr
		}
		if nameErr == nil {
			var existing ProbeName
			if err := json.Unmarshal(nameDoc.Value, &existing); err != nil || !validProbeName(existing) || existing.Name != name {
				return zero, errors.New("live.probe_name_corrupt")
			}
			if existing.Active && existing.Revision == revision.ID {
				return ProbePutResult{Name: existing, Revision: revision, Changed: false}, nil
			}
		}

		named := ProbeName{Schema: "lycheedev.probe-name.v1", Name: name, Revision: revision.ID, Active: true, UpdatedAt: time.Now().UTC()}
		nameRaw, _ := json.Marshal(named)
		changes := []vault.Mutation{{Key: nameKey, ExpectedGeneration: nameDoc.Generation, Value: nameRaw}}
		if errors.Is(revisionErr, vault.ErrMissingRecord) {
			revisionRaw, _ := json.Marshal(revision)
			changes = append(changes, vault.Mutation{Key: revisionKey, Value: revisionRaw})
		}
		if err := metadata.CommitDocuments(ctx, changes...); err != nil {
			return zero, err
		}
		return ProbePutResult{Name: named, Revision: revision, Changed: true}, nil
	})
}

func ListProbes(ctx context.Context, root string, includeRemoved bool, limit int) (ProbeListResult, error) {
	if limit < 1 || limit > 1000 {
		return ProbeListResult{}, errors.New("live.probe_limit_invalid")
	}
	return vault.ReadWorkspace(ctx, root, func(_ *vault.Store, metadata *vault.Metadata) (ProbeListResult, error) {
		docs, err := metadata.ListDocuments(ctx, "probe/name/", "", limit)
		if err != nil {
			return ProbeListResult{}, err
		}
		result := ProbeListResult{Items: make([]ProbeName, 0, len(docs))}
		for _, doc := range docs {
			var item ProbeName
			if err := json.Unmarshal(doc.Value, &item); err != nil || !validProbeName(item) || doc.Key != "probe/name/"+item.Name {
				return ProbeListResult{}, errors.New("live.probe_name_corrupt")
			}
			if item.Active || includeRemoved {
				result.Items = append(result.Items, item)
			}
		}
		return result, nil
	})
}

func ShowProbe(ctx context.Context, root, selector string) (ProbePutResult, error) {
	return vault.ReadWorkspace(ctx, root, func(_ *vault.Store, metadata *vault.Metadata) (ProbePutResult, error) {
		revisionID := selector
		var named ProbeName
		if !strings.HasPrefix(selector, probeRevisionPrefix) {
			if !probeNamePattern.MatchString(selector) {
				return ProbePutResult{}, errors.New("live.probe_selector_invalid")
			}
			doc, err := metadata.ReadDocument(ctx, "probe/name/"+selector)
			if err != nil {
				return ProbePutResult{}, err
			}
			if err := json.Unmarshal(doc.Value, &named); err != nil || !validProbeName(named) || named.Name != selector {
				return ProbePutResult{}, errors.New("live.probe_name_corrupt")
			}
			if !named.Active {
				return ProbePutResult{}, errors.New("live.probe_removed")
			}
			revisionID = named.Revision
		}
		doc, err := metadata.ReadDocument(ctx, "probe/revision/"+revisionID)
		if err != nil {
			return ProbePutResult{}, err
		}
		var revision ProbeRevision
		if err := json.Unmarshal(doc.Value, &revision); err != nil || !validProbeRevision(revision) || revision.ID != revisionID {
			return ProbePutResult{}, errors.New("live.probe_revision_corrupt")
		}
		if named.Name == "" {
			named = ProbeName{Schema: "lycheedev.probe-name.v1", Revision: revision.ID, Active: true}
		}
		return ProbePutResult{Name: named, Revision: revision}, nil
	})
}

func RemoveProbe(ctx context.Context, root, name string) (ProbeName, error) {
	if !probeNamePattern.MatchString(name) {
		return ProbeName{}, errors.New("live.probe_name_invalid")
	}
	return vault.WriteMetadata(ctx, root, func(_ *vault.Store, metadata *vault.Metadata) (ProbeName, error) {
		doc, err := metadata.ReadDocument(ctx, "probe/name/"+name)
		if err != nil {
			return ProbeName{}, err
		}
		var named ProbeName
		if err := json.Unmarshal(doc.Value, &named); err != nil || !validProbeName(named) || named.Name != name {
			return ProbeName{}, errors.New("live.probe_name_corrupt")
		}
		if !named.Active {
			return named, nil
		}
		named.Active, named.UpdatedAt = false, time.Now().UTC()
		raw, _ := json.Marshal(named)
		if err := metadata.CommitDocuments(ctx, vault.Mutation{Key: doc.Key, ExpectedGeneration: doc.Generation, Value: raw}); err != nil {
			return ProbeName{}, err
		}
		return named, nil
	})
}

func validProbeName(value ProbeName) bool {
	return value.Schema == "lycheedev.probe-name.v1" && probeNamePattern.MatchString(value.Name) && strings.HasPrefix(value.Revision, probeRevisionPrefix) && !value.UpdatedAt.IsZero()
}

func validProbeRevision(value ProbeRevision) bool {
	if value.Schema != "lycheedev.probe-revision.v1" || value.ID != probeRevisionPrefix+value.SHA256 || len(value.SHA256) != 64 || value.Bytes != len(value.Code) || value.Bytes < 1 || value.Bytes > 256<<10 || value.CreatedAt.IsZero() {
		return false
	}
	digest := sha256.Sum256(value.Code)
	return hex.EncodeToString(digest[:]) == value.SHA256
}
