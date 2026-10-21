package addoncheck

// Declaration and Relationship describe source observations, not execution.
// The document's pinned identity is supplied by its containing source index.
type Declaration struct {
	Name      string `json:"name"`
	Category  string `json:"category"`
	Scope     string `json:"scope,omitempty"`
	Line      int    `json:"line"`
	EndLine   int    `json:"endLine"`
	Signature string `json:"signature,omitempty"`
}
type Relationship struct {
	From       string `json:"from"`
	To         string `json:"to"`
	Category   string `json:"category"`
	Confidence string `json:"confidence"`
	Line       int    `json:"line"`
}
type LoadReference struct {
	Path string `json:"path"`
	Line int    `json:"line"`
	// Kind records how the document is pulled in: "toc", "script", or "include".
	Kind string `json:"kind"`
}
type HeaderValue struct {
	Key   string `json:"key"`
	Value string `json:"value"`
	Line  int    `json:"line"`
}
type SyntaxNote struct {
	Message string `json:"message"`
	Line    int    `json:"line"`
}

// EncodingWarning describes text display without implying a syntax failure.
type EncodingWarning struct {
	Message    string `json:"message"`
	Line       int    `json:"line"`
	ByteOffset int    `json:"byteOffset"` // Zero-based offset in the original bytes.
}

type DocumentFacts struct {
	Declarations  []Declaration     `json:"declarations"`
	Relationships []Relationship    `json:"relationships"`
	Loads         []LoadReference   `json:"loads"`
	Headers       []HeaderValue     `json:"headers"`
	Diagnostics   []SyntaxNote      `json:"diagnostics"`
	Warnings      []EncodingWarning `json:"warnings,omitempty"`
}

func emptyFacts() DocumentFacts {
	return DocumentFacts{Declarations: []Declaration{}, Relationships: []Relationship{}, Loads: []LoadReference{}, Headers: []HeaderValue{}, Diagnostics: []SyntaxNote{}}
}
