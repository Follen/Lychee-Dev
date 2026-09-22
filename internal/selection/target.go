package selection

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

// TargetSchema versions the mutable named target configuration document.
// A named target is user intent: it is edited freely and resolved into a fresh
// immutable identity at command start.
const TargetSchema = "lycheedev.target.v1"

// TargetReferenceSchema versions the reference ledger used to keep removal of
// a named target from breaking running work.
const TargetReferenceSchema = "lycheedev.target-ref.v1"

// Target resolution origins. Both are explicit; nothing defaults silently.
const (
	TargetSourceInstallation = "installation"
	TargetSourceRemote       = "remote"
)

var (
	ErrTargetMissing   = errors.New("selection.target_missing")
	ErrTargetExists    = errors.New("selection.target_exists")
	ErrTargetAmbiguous = errors.New("selection.target_ambiguous")
	ErrTargetInUse     = errors.New("selection.target_in_use")
	ErrTargetFormat    = errors.New("selection.target_format")
)

// TargetConfig is a mutable named target: explicit product, region, locale,
// an optional exact full-build constraint, and an explicit resolution origin
// (a local installation directory or the remote release identity). There are
// no silent product, region or locale defaults.
type TargetConfig struct {
	Schema       string `json:"schema"`
	Name         string `json:"name"`
	Product      string `json:"product"`
	Region       string `json:"region"`
	Locale       string `json:"locale"`
	FullBuild    string `json:"fullBuild,omitempty"`
	Source       string `json:"source"`
	Installation string `json:"installation,omitempty"`
	Repository   string `json:"repository,omitempty"`
	Definitions  string `json:"definitions,omitempty"`
}

// TargetReference records one resolved identity or one running operation that
// leans on a named target. Pin references never block removal (the immutable
// set survives on its own); active operation references do, so removal cannot
// break running work.
type TargetReference struct {
	Schema    string    `json:"schema"`
	Name      string    `json:"name"`
	Kind      string    `json:"kind"` // "pin" | "operation"
	ID        string    `json:"id"`
	CreatedAt time.Time `json:"createdAt"`
}

// TargetView is the show() result: the current intent plus its references.
type TargetView struct {
	Config           TargetConfig      `json:"config"`
	References       []TargetReference `json:"references"`
	ActiveOperations []string          `json:"activeOperations,omitempty"`
	ResolvedPins     []string          `json:"resolvedPins,omitempty"`
}

// StoreTargetOptions controls creation versus replacement.
type StoreTargetOptions struct {
	// Replace allows overwriting an existing name. Without it a conflict is
	// reported instead of silently replacing the user's configuration.
	Replace bool
}

// RemoveTargetReport describes what removal touched. Resolved pinned sets and
// their content are never deleted by removal.
type RemoveTargetReport struct {
	Name            string   `json:"name"`
	RemovedPins     []string `json:"removedPins,omitempty"`
	KeptPins        []string `json:"keptPins,omitempty"`
	RemovedRefCount int      `json:"removedRefCount"`
}

// Validate checks that every required field is explicit and well formed.
func (c TargetConfig) Validate() error {
	if c.Schema != "" && c.Schema != TargetSchema {
		return fmt.Errorf("%w: schema %q", ErrTargetFormat, c.Schema)
	}
	if !targetName(c.Name) {
		return fmt.Errorf("%w: name %q", ErrTargetFormat, c.Name)
	}
	if _, err := DataProduct(c.Product); err != nil {
		return err
	}
	if _, err := DataLocale(c.Region, c.Locale); err != nil {
		return err
	}
	if c.FullBuild != "" && !build(c.FullBuild) {
		return fmt.Errorf("%w: fullBuild %q", ErrTargetFormat, c.FullBuild)
	}
	switch c.Source {
	case TargetSourceInstallation:
		if strings.TrimSpace(c.Installation) == "" {
			return fmt.Errorf("%w: source %q requires an installation path", ErrTargetFormat, c.Source)
		}
	case TargetSourceRemote:
		if c.Installation != "" {
			return fmt.Errorf("%w: remote target must not name an installation", ErrTargetFormat)
		}
	default:
		return fmt.Errorf("%w: source %q", ErrTargetFormat, c.Source)
	}
	if len(c.Repository) > 200 || len(c.Definitions) > 200 {
		return fmt.Errorf("%w: field too long", ErrTargetFormat)
	}
	return nil
}

// PutTarget creates or explicitly replaces one named target configuration.
func PutTarget(ctx context.Context, root string, config TargetConfig, options StoreTargetOptions) (TargetConfig, error) {
	config.Schema = TargetSchema
	if err := config.Validate(); err != nil {
		return TargetConfig{}, err
	}
	raw, err := json.Marshal(config)
	if err != nil {
		return TargetConfig{}, err
	}
	_, err = vault.WriteMetadata(ctx, root, func(_ *vault.Store, metadata *vault.Metadata) (bool, error) {
		existing, readErr := metadata.ReadDocument(ctx, "target/"+config.Name)
		switch {
		case readErr == nil && !options.Replace:
			return false, ErrTargetExists
		case readErr == nil && options.Replace:
			if err := metadata.CommitDocuments(ctx, vault.Mutation{
				Key: "target/" + config.Name, ExpectedGeneration: existing.Generation, Value: raw,
			}); err != nil {
				return false, err
			}
			return true, nil
		case errors.Is(readErr, vault.ErrMissingRecord):
			if err := metadata.CommitDocuments(ctx, vault.Mutation{Key: "target/" + config.Name, Value: raw}); err != nil {
				return false, err
			}
			return true, nil
		default:
			return false, readErr
		}
	})
	if err != nil {
		return TargetConfig{}, err
	}
	return config, nil
}

// ListTargets returns every named target configuration in name order. A brand
// new workspace without a metadata database simply has no targets.
func ListTargets(ctx context.Context, root string) ([]TargetConfig, error) {
	configs, err := vault.ReadWorkspace(ctx, root, func(_ *vault.Store, metadata *vault.Metadata) ([]TargetConfig, error) {
		return listTargets(ctx, metadata)
	})
	if errors.Is(err, vault.ErrMissingRecord) {
		return []TargetConfig{}, nil
	}
	return configs, err
}

func listTargets(ctx context.Context, metadata *vault.Metadata) ([]TargetConfig, error) {
	configs := make([]TargetConfig, 0)
	after := "target/"
	for {
		docs, err := metadata.ListDocuments(ctx, "target/", after, 1000)
		if err != nil {
			return nil, err
		}
		for _, doc := range docs {
			var config TargetConfig
			if err := json.Unmarshal(doc.Value, &config); err != nil {
				return nil, fmt.Errorf("%w: %v", ErrTargetFormat, err)
			}
			if err := config.Validate(); err != nil {
				return nil, err
			}
			configs = append(configs, config)
			after = doc.Key
		}
		if len(docs) < 1000 {
			break
		}
	}
	sort.Slice(configs, func(i, j int) bool { return configs[i].Name < configs[j].Name })
	return configs, nil
}

// LookupTarget resolves a name exactly, or by unique prefix. A prefix that
// matches several targets is an error; only a unique match auto-resolves.
func LookupTarget(ctx context.Context, root, key string) (TargetConfig, error) {
	configs, err := ListTargets(ctx, root)
	if err != nil {
		return TargetConfig{}, err
	}
	return matchTarget(configs, key)
}

func matchTarget(configs []TargetConfig, key string) (TargetConfig, error) {
	if key == "" {
		return TargetConfig{}, ErrTargetMissing
	}
	matches := make([]string, 0, 1)
	lower := strings.ToLower(key)
	for _, config := range configs {
		name := strings.ToLower(config.Name)
		if name == lower || strings.HasPrefix(name, lower) {
			matches = append(matches, config.Name)
		}
	}
	switch len(matches) {
	case 0:
		return TargetConfig{}, fmt.Errorf("%w: %s", ErrTargetMissing, key)
	case 1:
		for _, config := range configs {
			if config.Name == matches[0] {
				return config, nil
			}
		}
		return TargetConfig{}, fmt.Errorf("%w: %s", ErrTargetMissing, key)
	default:
		sort.Strings(matches)
		exact := false
		for _, name := range matches {
			if strings.ToLower(name) == lower {
				exact = true
			}
		}
		if exact {
			for _, config := range configs {
				if strings.ToLower(config.Name) == lower {
					return config, nil
				}
			}
		}
		return TargetConfig{}, fmt.Errorf("%w: %s matches %s", ErrTargetAmbiguous, key, strings.Join(matches, ", "))
	}
}

// ShowTarget returns one named target together with its recorded references.
func ShowTarget(ctx context.Context, root, key string) (TargetView, error) {
	view, err := vault.ReadWorkspace(ctx, root, func(_ *vault.Store, metadata *vault.Metadata) (TargetView, error) {
		configs, err := listTargets(ctx, metadata)
		if err != nil {
			return TargetView{}, err
		}
		config, err := matchTarget(configs, key)
		if err != nil {
			return TargetView{}, err
		}
		refs, err := listTargetReferences(ctx, metadata, config.Name)
		if err != nil {
			return TargetView{}, err
		}
		result := TargetView{Config: config, References: refs}
		for _, ref := range refs {
			switch ref.Kind {
			case "operation":
				result.ActiveOperations = append(result.ActiveOperations, ref.ID)
			case "pin":
				result.ResolvedPins = append(result.ResolvedPins, ref.ID)
			}
		}
		return result, nil
	})
	if errors.Is(err, vault.ErrMissingRecord) {
		return TargetView{}, fmt.Errorf("%w: %s", ErrTargetMissing, key)
	}
	return view, err
}

// RemoveTarget removes one named target configuration and its own reference
// ledger. Resolved pinned sets stay intact and readable; running operations
// that still reference the target block removal with selection.target_in_use.
func RemoveTarget(ctx context.Context, root, key string) (RemoveTargetReport, error) {
	report := RemoveTargetReport{}
	_, err := vault.WriteMetadata(ctx, root, func(_ *vault.Store, metadata *vault.Metadata) (bool, error) {
		configs, err := listTargets(ctx, metadata)
		if err != nil {
			return false, err
		}
		config, err := matchTarget(configs, key)
		if err != nil {
			return false, err
		}
		report.Name = config.Name
		refs, err := listTargetReferences(ctx, metadata, config.Name)
		if err != nil {
			return false, err
		}
		active := make([]string, 0)
		for _, ref := range refs {
			if ref.Kind == "operation" {
				active = append(active, ref.ID)
			} else {
				report.RemovedPins = append(report.RemovedPins, ref.ID)
			}
		}
		if len(active) > 0 {
			return false, fmt.Errorf("%w: %s", ErrTargetInUse, strings.Join(active, ", "))
		}
		doc, err := metadata.ReadDocument(ctx, "target/"+config.Name)
		if err != nil {
			return false, err
		}
		if err := metadata.DeleteDocuments(ctx, vault.Deletion{Key: "target/" + config.Name, ExpectedGeneration: doc.Generation}); err != nil {
			return false, err
		}
		refRemovals := make([]vault.Deletion, 0, len(refs))
		for _, ref := range refs {
			refDoc, err := metadata.ReadDocument(ctx, refKey(config.Name, ref))
			if err != nil {
				return false, err
			}
			refRemovals = append(refRemovals, vault.Deletion{Key: refKey(config.Name, ref), ExpectedGeneration: refDoc.Generation})
		}
		for start := 0; start < len(refRemovals); start += 64 {
			end := start + 64
			if end > len(refRemovals) {
				end = len(refRemovals)
			}
			if err := metadata.DeleteDocuments(ctx, refRemovals[start:end]...); err != nil {
				return false, err
			}
			report.RemovedRefCount += end - start
		}
		return true, nil
	})
	if err != nil {
		return RemoveTargetReport{}, err
	}
	return report, nil
}

// RegisterTargetOperation records running work against a named target so
// removal cannot break it. Release it when the work finishes.
func RegisterTargetOperation(ctx context.Context, root, name, operationID string) error {
	return recordTargetReference(ctx, root, name, TargetReference{
		Schema: TargetReferenceSchema, Name: name, Kind: "operation", ID: operationID, CreatedAt: time.Now().UTC(),
	})
}

// ReleaseTargetOperation drops the running-work reference again.
func ReleaseTargetOperation(ctx context.Context, root, name, operationID string) error {
	_, err := vault.WriteMetadata(ctx, root, func(_ *vault.Store, metadata *vault.Metadata) (bool, error) {
		ref := TargetReference{Kind: "operation", ID: operationID}
		doc, err := metadata.ReadDocument(ctx, refKey(name, ref))
		if err != nil {
			return false, err
		}
		return true, metadata.DeleteDocuments(ctx, vault.Deletion{Key: refKey(name, ref), ExpectedGeneration: doc.Generation})
	})
	return err
}

func recordTargetReference(ctx context.Context, root, name string, ref TargetReference) error {
	raw, err := json.Marshal(ref)
	if err != nil {
		return err
	}
	_, err = vault.WriteMetadata(ctx, root, func(_ *vault.Store, metadata *vault.Metadata) (bool, error) {
		err := metadata.CommitDocuments(ctx, vault.Mutation{Key: refKey(name, ref), Value: raw})
		if errors.Is(err, vault.ErrGeneration) {
			return true, nil
		}
		return err == nil, err
	})
	return err
}

func listTargetReferences(ctx context.Context, metadata *vault.Metadata, name string) ([]TargetReference, error) {
	prefix := "target-ref/" + name + "/"
	refs := make([]TargetReference, 0)
	after := prefix
	for {
		docs, err := metadata.ListDocuments(ctx, prefix, after, 1000)
		if err != nil {
			return nil, err
		}
		for _, doc := range docs {
			var ref TargetReference
			if err := json.Unmarshal(doc.Value, &ref); err != nil {
				return nil, fmt.Errorf("%w: %v", ErrTargetFormat, err)
			}
			if ref.Schema != TargetReferenceSchema || ref.Name != name {
				return nil, fmt.Errorf("%w: reference identity", ErrTargetFormat)
			}
			refs = append(refs, ref)
			after = doc.Key
		}
		if len(docs) < 1000 {
			break
		}
	}
	sort.Slice(refs, func(i, j int) bool {
		if refs[i].Kind != refs[j].Kind {
			return refs[i].Kind < refs[j].Kind
		}
		return refs[i].ID < refs[j].ID
	})
	return refs, nil
}

func refKey(name string, ref TargetReference) string {
	return "target-ref/" + name + "/" + ref.Kind + "/" + ref.ID
}

// targetName allows stable, path-safe configuration names.
func targetName(name string) bool {
	if name == "" || len(name) > 64 {
		return false
	}
	alphaNumeric := false
	for _, r := range name {
		switch {
		case r >= 'a' && r <= 'z', r >= 'A' && r <= 'Z', r >= '0' && r <= '9':
			alphaNumeric = true
		case r == '-', r == '_', r == '.':
		default:
			return false
		}
	}
	return alphaNumeric
}
