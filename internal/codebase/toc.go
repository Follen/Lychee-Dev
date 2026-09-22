package codebase

import (
	"bytes"
	"strings"
)

// scanManifest reads the ordered declarations in a World of Warcraft TOC.
// It intentionally leaves validation, client selection, and path handling to
// the checker that consumes these facts.
func scanManifest(data []byte) DocumentFacts {
	facts := emptyFacts()
	data = bytes.TrimPrefix(data, []byte{0xef, 0xbb, 0xbf})

	for lineNumber, rawLine := range strings.Split(string(data), "\n") {
		line := strings.TrimSpace(rawLine)
		if line == "" {
			continue
		}

		if strings.HasPrefix(line, "##") {
			key, value, ok := strings.Cut(strings.TrimSpace(strings.TrimPrefix(line, "##")), ":")
			if !ok {
				continue
			}
			facts.Headers = append(facts.Headers, HeaderValue{
				Key:   strings.TrimSpace(key),
				Value: strings.TrimSpace(value),
				Line:  lineNumber + 1,
			})
			continue
		}

		if strings.HasPrefix(line, "#") {
			continue
		}
		facts.Loads = append(facts.Loads, LoadReference{Path: line, Line: lineNumber + 1, Kind: "toc"})
	}

	return facts
}
