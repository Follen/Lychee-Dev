// Package flow performs bounded, evidence-carrying secret-value propagation
// over frozen Lua source. Its results are static candidates, never proof of
// runtime secrecy, safety, or secure execution taint.
package flow

import "time"

type RuleIdentity struct {
	Client         string `json:"client"`
	APICommit      string `json:"apiCommit"`
	MetadataDigest string `json:"metadataDigest"`
	Version        string `json:"version"`
}

type SlotRule struct {
	// State is secret, possible, never, or unknown. Extraction uses possible
	// for conditional metadata unless the caller supplies stronger evidence.
	State     string `json:"state"`
	Condition string `json:"condition,omitempty"`
}

type ArgumentRule struct {
	// Secret is forbidden, allowed, conditional, or unknown. Unknown is not
	// interpreted as a violation even when its argument may be secret.
	Secret    string `json:"secret"`
	Condition string `json:"condition,omitempty"`
}

type CallRule struct {
	Name      string               `json:"name"`
	Path      string               `json:"path"`
	Line      int                  `json:"line"`
	Returns   map[int]SlotRule     `json:"returns,omitempty"`
	Arguments map[int]ArgumentRule `json:"arguments,omitempty"`
	Raw       map[string]any       `json:"raw,omitempty"`
}

type RuleSet struct {
	Identity RuleIdentity        `json:"identity"`
	Calls    map[string]CallRule `json:"calls"`
	Events   map[string]CallRule `json:"events,omitempty"`
	Issues   []Boundary          `json:"issues,omitempty"`
}

type Target struct {
	Path   string `json:"path,omitempty"`
	Line   int    `json:"line,omitempty"`
	Symbol string `json:"symbol,omitempty"`
}

type Budget struct {
	MaxFiles       int       `json:"maxFiles,omitempty"`
	MaxFunctions   int       `json:"maxFunctions,omitempty"`
	MaxDepth       int       `json:"maxDepth,omitempty"`
	MaxPaths       int       `json:"maxPaths,omitempty"`
	MaxMillis      int       `json:"maxMillis,omitempty"`
	MaxOutputBytes int       `json:"maxOutputBytes,omitempty"`
	Deadline       time.Time `json:"-"`
}

type Request struct {
	Files  map[string][]byte `json:"-"`
	Target Target            `json:"target"`
	Rules  RuleSet           `json:"rules"`
	Budget Budget            `json:"budget"`
}

type Step struct {
	Path      string `json:"path"`
	Line      int    `json:"line"`
	Kind      string `json:"kind"`
	Name      string `json:"name,omitempty"`
	Detail    string `json:"detail,omitempty"`
	Condition string `json:"condition,omitempty"`
	State     string `json:"state,omitempty"`
}

type Finding struct {
	State     string `json:"state"`
	Source    string `json:"source"`
	Use       string `json:"use"`
	Argument  int    `json:"argument"`
	RulePath  string `json:"rulePath,omitempty"`
	RuleLine  int    `json:"ruleLine,omitempty"`
	Condition string `json:"condition,omitempty"`
	Steps     []Step `json:"steps"`
}

type Boundary struct {
	Path   string `json:"path,omitempty"`
	Line   int    `json:"line,omitempty"`
	Code   string `json:"code"`
	Detail string `json:"detail,omitempty"`
}

type Coverage struct {
	State     string `json:"state"`
	Files     int    `json:"files"`
	Functions int    `json:"functions"`
	Paths     int    `json:"paths"`
	Complete  bool   `json:"complete"`
}

type Result struct {
	RuleIdentity RuleIdentity `json:"ruleIdentity"`
	Findings     []Finding    `json:"findings"`
	Boundaries   []Boundary   `json:"boundaries"`
	Coverage     Coverage     `json:"coverage"`
	Truncated    bool         `json:"truncated"`
}
