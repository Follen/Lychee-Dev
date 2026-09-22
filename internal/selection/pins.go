// Package selection fixes independently versioned sources into immutable sets.
package selection

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strconv"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

type SourcePin struct {
	Repository     string `json:"repository"`
	Product        string `json:"product"`
	RequestedRef   string `json:"requestedRef"`
	ExactCommit    string `json:"exactCommit"`
	ParserRevision string `json:"parserRevision"`
}
type DataPin struct {
	Product          string `json:"product"`
	Region           string `json:"region"`
	FullBuild        string `json:"fullBuild"`
	BuildConfig      string `json:"buildConfig"`
	CDNConfig        string `json:"cdnConfig"`
	Language         string `json:"language"`
	DefinitionCommit string `json:"definitionCommit"`
}
type ChangePin struct {
	Provider  string    `json:"provider"`
	Product   string    `json:"product"`
	FullBuild string    `json:"fullBuild"`
	Region    string    `json:"region"`
	Scope     string    `json:"scope"`
	FetchedAt time.Time `json:"fetchedAt"`
	SHA256    string    `json:"sha256"`
}
type SelectionSpec struct {
	Parent  string     `json:"parent,omitempty"`
	Source  *SourcePin `json:"source,omitempty"`
	Data    *DataPin   `json:"data,omitempty"`
	Changes *ChangePin `json:"changes,omitempty"`
}
type PinnedSet struct {
	Schema string `json:"schema"`
	ID     string `json:"id"`
	SelectionSpec
}

type Pinner struct{ metadata *vault.Metadata }

func OpenPinner(metadata *vault.Metadata) *Pinner { return &Pinner{metadata: metadata} }

// PinSelection accepts resolved identities only. Network resolution belongs to
// the source/data providers; symbolic latest is never durable identity.
func (p *Pinner) PinSelection(ctx context.Context, spec SelectionSpec) (PinnedSet, error) {
	if spec.Parent != "" {
		parent, err := p.ReadPinnedSet(ctx, spec.Parent)
		if err != nil {
			return PinnedSet{}, err
		}
		if spec.Source == nil {
			spec.Source = parent.Source
		} else if parent.Source != nil && *spec.Source != *parent.Source {
			return PinnedSet{}, errors.New("selection.fixed_source_conflict")
		}
		if spec.Data == nil {
			spec.Data = parent.Data
		} else if parent.Data != nil && *spec.Data != *parent.Data {
			return PinnedSet{}, errors.New("selection.fixed_data_conflict")
		}
		if spec.Changes == nil {
			spec.Changes = parent.Changes
		} else if parent.Changes != nil && *spec.Changes != *parent.Changes {
			return PinnedSet{}, errors.New("selection.fixed_changes_conflict")
		}
	}
	if err := validatePins(spec); err != nil {
		return PinnedSet{}, err
	}
	pin := PinnedSet{Schema: "lycheedev.selection.v1", SelectionSpec: spec}
	unsigned, err := json.Marshal(pin)
	if err != nil {
		return pin, err
	}
	hash := sha256.Sum256(unsigned)
	pin.ID = "PIN-" + hex.EncodeToString(hash[:])
	data, err := json.Marshal(pin)
	if err != nil {
		return pin, err
	}
	err = p.metadata.CommitDocuments(ctx, vault.Mutation{Key: "pin/" + pin.ID, Value: data})
	if errors.Is(err, vault.ErrGeneration) {
		return p.ReadPinnedSet(ctx, pin.ID)
	}
	if err != nil {
		return PinnedSet{}, err
	}
	// Decoding prevents callers mutating the original pointer values from changing
	// the returned identity in memory after publication.
	var copy PinnedSet
	err = json.Unmarshal(data, &copy)
	return copy, err
}

func (p *Pinner) ReadPinnedSet(ctx context.Context, id string) (PinnedSet, error) {
	doc, err := p.metadata.ReadDocument(ctx, "pin/"+id)
	if err != nil {
		return PinnedSet{}, err
	}
	var pin PinnedSet
	if err := json.Unmarshal(doc.Value, &pin); err != nil {
		return pin, err
	}
	if pin.ID != id || pin.Schema != "lycheedev.selection.v1" {
		return pin, errors.New("selection.invalid_pin")
	}
	return pin, validatePinnedSet(pin)
}

func validatePinnedSet(pin PinnedSet) error {
	if pin.Schema != "lycheedev.selection.v1" || !strings.HasPrefix(pin.ID, "PIN-") || !digest(strings.TrimPrefix(pin.ID, "PIN-"), 64) {
		return errors.New("selection.invalid_pin")
	}
	if err := validatePins(pin.SelectionSpec); err != nil {
		return err
	}
	unsigned := pin
	unsigned.ID = ""
	data, err := json.Marshal(unsigned)
	if err != nil {
		return err
	}
	hash := sha256.Sum256(data)
	if "PIN-"+hex.EncodeToString(hash[:]) != pin.ID {
		return errors.New("selection.pin_integrity")
	}
	return nil
}

func validatePins(spec SelectionSpec) error {
	if spec.Source == nil && spec.Data == nil && spec.Changes == nil {
		return errors.New("selection.empty")
	}
	if s := spec.Source; s != nil {
		if s.Repository == "" || s.Product == "" || s.ParserRevision == "" || !digest(s.ExactCommit, 40) {
			return errors.New("selection.unresolved_source")
		}
	}
	if d := spec.Data; d != nil {
		if d.Product == "" || d.Region == "" || d.Language == "" || !build(d.FullBuild) || !digest(d.BuildConfig, 32) || !digest(d.CDNConfig, 32) || !digest(d.DefinitionCommit, 40) {
			return errors.New("selection.unresolved_data")
		}
	}
	if c := spec.Changes; c != nil {
		if c.Provider == "" || c.Product == "" || c.Region == "" || c.Scope == "" || !build(c.FullBuild) || c.FetchedAt.IsZero() || !digest(c.SHA256, 64) {
			return errors.New("selection.unresolved_changes")
		}
	}
	product := ""
	for _, value := range []string{sourceProduct(spec.Source), dataProduct(spec.Data), changeProduct(spec.Changes)} {
		if value == "" {
			continue
		}
		if product != "" && value != product {
			return fmt.Errorf("selection.product_mismatch: %s / %s", product, value)
		}
		product = value
	}
	return nil
}
func digest(s string, n int) bool {
	if len(s) != n {
		return false
	}
	for _, c := range s {
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f') {
			return false
		}
	}
	return true
}
func build(s string) bool {
	parts := strings.Split(s, ".")
	if len(parts) != 4 {
		return false
	}
	for _, p := range parts {
		if p == "" {
			return false
		}
		for _, c := range p {
			if c < '0' || c > '9' {
				return false
			}
		}
		if _, err := strconv.ParseUint(p, 10, 32); err != nil {
			return false
		}
	}
	return true
}
func sourceProduct(s *SourcePin) string {
	if s == nil {
		return ""
	}
	return s.Product
}
func dataProduct(s *DataPin) string {
	if s == nil {
		return ""
	}
	return s.Product
}
func changeProduct(s *ChangePin) string {
	if s == nil {
		return ""
	}
	return s.Product
}
