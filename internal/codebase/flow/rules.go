package flow

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"sort"
	"strings"

	"github.com/follenfang/lycheedev/internal/codebase/environment"
)

// DigestDocuments binds both source paths and exact bytes to a rule identity.
// It uses the same framing as environment.Build's input manifest.
func DigestDocuments(documents map[string][]byte) string {
	names := sortedPaths(documents)
	h := sha256.New()
	for _, name := range names {
		fmt.Fprintf(h, "%d:%s:%d:", len(name), name, len(documents[name]))
		_, _ = h.Write(documents[name])
	}
	return hex.EncodeToString(h.Sum(nil))
}

func sortedPaths(documents map[string][]byte) []string {
	names := make([]string, 0, len(documents))
	for name := range documents {
		names = append(names, name)
	}
	sort.Strings(names)
	return names
}

// ExtractRules is a convenience for callers that do not already have the
// fixed environment result. The same parser supplies both LuaLS definitions
// and flow facts; generated documentation is never executed.
func ExtractRules(ctx context.Context, clientID, apiCommit, metadataDigest, ruleVersion string, documents map[string][]byte) (RuleSet, error) {
	identity := RuleIdentity{Client: clientID, APICommit: apiCommit, MetadataDigest: metadataDigest, Version: ruleVersion}
	if clientID == "" || apiCommit == "" || ruleVersion == "" || metadataDigest == "" || len(documents) == 0 || metadataDigest != DigestDocuments(documents) {
		return RuleSet{}, fmt.Errorf("%w: rule identity or metadata digest", ErrInput)
	}
	parsed, err := environment.Build(ctx, environment.Identity{Repository: "Blizzard_APIDocumentationGenerated", Commit: apiCommit, Client: clientID, GeneratorVersion: environment.GeneratorVersion}, documents, environment.Limits{MaxDocuments: 2048, MaxFileBytes: 1 << 20, MaxTotalBytes: 32 << 20})
	if err != nil {
		return RuleSet{}, err
	}
	return RulesFromFacts(identity, parsed)
}

// RulesFromFacts translates a single verified metadata result into bounded
// propagation rules. Unsupported expressions remain explicit gaps. A System
// display group never qualifies a global API name; environment already
// resolves the callable from an explicit Namespace when present.
func RulesFromFacts(identity RuleIdentity, parsed environment.Result) (RuleSet, error) {
	rules := RuleSet{Identity: identity, Calls: map[string]CallRule{}, Events: map[string]CallRule{}, Issues: []Boundary{}}
	if identity.Client == "" || identity.APICommit == "" || identity.MetadataDigest == "" || identity.Version == "" || parsed.Manifest.InputSHA256 != identity.MetadataDigest || parsed.Manifest.Identity.Client != identity.Client || parsed.Manifest.Identity.Commit != identity.APICommit {
		return RuleSet{}, fmt.Errorf("%w: rule identity or environment manifest", ErrInput)
	}
	for _, gap := range parsed.Coverage.Gaps {
		if gap.Code == "unknown_type" || gap.Code == "may_return_nothing" {
			// LuaLS annotation gaps do not change secret-value facts.
			continue
		}
		rules.Issues = append(rules.Issues, Boundary{Path: gap.Path, Line: gap.Line, Code: "metadata_" + gap.Code, Detail: gap.Detail})
	}
	ambiguousCalls, ambiguousEvents := map[string]bool{}, map[string]bool{}
	for _, record := range parsed.Facts {
		if record.Kind != "function" && record.Kind != "event" {
			continue
		}
		if record.Name == "" {
			rules.Issues = append(rules.Issues, Boundary{Path: record.Path, Line: record.Line, Code: "metadata_unnamed_callable"})
			continue
		}
		rule := CallRule{Name: record.Name, Path: record.Path, Line: record.Line, Returns: map[int]SlotRule{}, Arguments: map[int]ArgumentRule{}, Raw: map[string]any{}}
		for key, value := range record.Raw {
			if strings.Contains(key, "Secret") || strings.Contains(key, "Restricted") || key == "IsProtectedFunction" {
				rule.Raw[key] = value
			}
		}
		returnSlots, argumentSlots := record.Returns, record.Parameters
		if record.Kind == "event" {
			returnSlots, argumentSlots = record.Payload, record.Payload
		}
		condition := rawCondition(rule.Raw, "SecretReturns", "SecretReturnsForAspect", "SecretPayloads")
		for _, slot := range returnSlots {
			if slot.Index < 1 {
				continue
			}
			state, slotCondition := "unknown", condition
			if slot.Raw["NeverSecret"] == true {
				state = "never"
			} else if condition != "" || slot.Raw["ConditionalSecret"] == true {
				state = "possible"
			}
			if slot.Raw["ConditionalSecret"] == true {
				slotCondition = strings.TrimSpace(slotCondition + "; ConditionalSecret")
			}
			if state != "unknown" {
				rule.Returns[slot.Index] = SlotRule{State: state, Condition: strings.TrimPrefix(slotCondition, "; ")}
			}
		}
		argumentCondition := rawCondition(rule.Raw, "SecretArguments", "SecretArgumentsAddAspect", "IsPreventingSecretValues")
		if argumentCondition != "" {
			secretState := "unknown"
			// The pinned generated docs use both values. We retain the
			// execution condition verbatim and report only a possible
			// candidate; the static analyzer cannot establish taint state.
			if mode, ok := rule.Raw["SecretArguments"].(string); ok && (mode == "AllowedWhenTainted" || mode == "AllowedWhenUntainted") {
				secretState = "conditional"
			}
			for _, slot := range argumentSlots {
				if slot.Index > 0 {
					rule.Arguments[slot.Index] = ArgumentRule{Secret: secretState, Condition: argumentCondition}
				}
			}
		}
		if len(rule.Returns) == 0 && len(rule.Arguments) == 0 && len(rule.Raw) == 0 {
			continue
		}
		destination, ambiguous := rules.Calls, ambiguousCalls
		if record.Kind == "event" {
			destination, ambiguous = rules.Events, ambiguousEvents
		}
		if ambiguous[rule.Name] {
			continue
		}
		if previous, exists := destination[rule.Name]; exists {
			rules.Issues = append(rules.Issues, Boundary{Path: record.Path, Line: record.Line, Code: "duplicate_rule", Detail: "also declared in " + previous.Path})
			delete(destination, rule.Name)
			ambiguous[rule.Name] = true
			continue
		}
		destination[rule.Name] = rule
	}
	if len(rules.Events) > 0 {
		rules.Issues = append(rules.Issues, Boundary{Code: "event_payload_mapping_unavailable", Detail: "event payload metadata is retained separately; handler registration and payload routing are outside this flow scope"})
	}
	return rules, nil
}

func rawCondition(raw map[string]any, keys ...string) string {
	parts := []string{}
	for _, key := range keys {
		if value, ok := raw[key]; ok {
			parts = append(parts, fmt.Sprintf("%s=%v", key, value))
		}
	}
	return strings.Join(parts, "; ")
}
