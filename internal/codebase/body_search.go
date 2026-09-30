package codebase

import (
	"bufio"
	"bytes"
	"context"
	"errors"
	"fmt"
	"os/exec"
	"strconv"
	"strings"
	"time"
)

// bodyCandidates searches fixed source bytes independently of the parser.
// Syntax failures therefore cannot make source text disappear from research.
func (c *snapshotCache) bodyCandidates(ctx context.Context, text, topic string, limit int) ([]searchCandidate, bool, error) {
	if topic == "asset" {
		return nil, false, nil
	}
	if limit < 1 || limit > 10000 {
		return nil, false, errors.New("codebase.body_search_budget")
	}
	if c.manifest.FixtureRoot != "" {
		return c.fixtureBodyCandidates(ctx, text, topic, limit)
	}
	ctx, cancel := context.WithTimeout(ctx, 2*time.Minute)
	defer cancel()
	args := []string{"grep", "-n", "-I", "-i", "-F", "--full-name", "-e", text, c.pin.ExactCommit, "--"}
	// Apply the same topic restriction before Git's bounded output is filled.
	// icase matches bodyTopicPath for uppercase source extensions as well.
	switch topic {
	case "lua", "xml", "toc":
		args = append(args, ":(glob,icase)**/*."+topic)
	case "api":
		args = append(args, ":(glob,icase)**/Blizzard_APIDocumentationGenerated/**")
	}
	cmd := gitCommand(ctx, c.b.mirror(c.pin.Repository), args...)
	out := &boundedOutput{limit: 4 << 20, cancel: cancel}
	log := &boundedOutput{limit: 32 << 10, cancel: cancel}
	cmd.Stdout, cmd.Stderr = out, log
	err := cmd.Run()
	if err != nil {
		var exit *exec.ExitError
		if !errors.As(err, &exit) || exit.ExitCode() != 1 && !out.exceeded {
			return nil, false, fmt.Errorf("codebase.git_grep_failed: %w: %s", err, log.String())
		}
	}
	if ctx.Err() != nil && !out.exceeded {
		return nil, false, ctx.Err()
	}
	candidates := []searchCandidate{}
	cut := out.exceeded
	reader := bufio.NewScanner(bytes.NewReader(out.buffer.Bytes()))
	reader.Buffer(make([]byte, 64<<10), 1<<20)
	for reader.Scan() {
		line := reader.Text()
		_, remainder, ok := strings.Cut(line, ":")
		if !ok {
			continue
		}
		name, rest, ok := strings.Cut(remainder, ":")
		if !ok || !sourcePath(name) || !bodyTopicPath(topic, name) {
			continue
		}
		number, snippet, ok := strings.Cut(rest, ":")
		if !ok {
			continue
		}
		n, parseErr := strconv.Atoi(number)
		if parseErr != nil || n < 1 {
			continue
		}
		if len(candidates) >= limit {
			cut = true
			break
		}
		candidates = append(candidates, searchCandidate{symbol: SymbolMatch{Kind: "body", Name: text, Path: name, Line: n, EndLine: n, Signature: snippet, Confidence: "exact"}, matched: "body_text", rank: 60})
	}
	if err := reader.Err(); err != nil {
		return nil, cut, err
	}
	return candidates, cut, nil
}

func (c *snapshotCache) fixtureBodyCandidates(ctx context.Context, text, topic string, limit int) ([]searchCandidate, bool, error) {
	rows := []searchCandidate{}
	cut := false
	err := c.scanSelected(ctx, func(e recordOffset) bool { return e.Kind == "document" && bodyTopicPath(topic, e.Path) }, func(r sourceRecord) error {
		if r.Kind != "document" || !bodyTopicPath(topic, r.Path) {
			return nil
		}
		data, _, err := c.document(ctx, r.Path)
		if err != nil {
			return err
		}
		for i, line := range strings.Split(string(data), "\n") {
			if strings.Contains(strings.ToLower(line), strings.ToLower(text)) {
				if len(rows) >= limit {
					cut = true
					return nil
				}
				rows = append(rows, searchCandidate{symbol: SymbolMatch{Kind: "body", Name: text, Path: r.Path, Line: i + 1, EndLine: i + 1, Signature: line, Confidence: "exact"}, matched: "body_text", rank: 60})
			}
		}
		return nil
	})
	return rows, cut, err
}

func bodyTopicPath(topic, name string) bool {
	lower := strings.ToLower(name)
	switch topic {
	case "":
		return true
	case "api":
		return strings.Contains(lower, "/blizzard_apidocumentationgenerated/")
	case "lua", "xml", "toc":
		return strings.HasSuffix(lower, "."+topic)
	default:
		return false
	}
}
