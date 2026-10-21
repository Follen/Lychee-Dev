package codebase

import (
	"bytes"
	"context"
	"crypto/sha256"
	"fmt"
	"strings"
	"testing"
	"unicode/utf8"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestPinnedQueriesSurviveNonUTF8LuaComment(t *testing.T) {
	const tutorialPath = "Interface/AddOns/Blizzard_NewPlayerExperience/Camelot/Blizzard_TutorialData.lua"
	tutorial := "function TutorialBefore() end\n-- Night Web\x92s Hollow\nfunction TutorialAfter() end\n"
	b, seed := sourceFixture(t)
	seed.Product = "forever"
	seed.RequestedRef = "refs/heads/forever"
	pin := testCommit(t, b, seed, map[string]string{
		tutorialPath: tutorial,
		"Interface/AddOns/Blizzard_APIDocumentationGenerated/SecretDocumentation.lua": `SecretAPI = {
  Name = "SecretAPI", Type = "System",
  Functions = {{ Name = "issecretvalue", Type = "Function", SecretReturns = true }},
}`,
	}, "non-UTF8 Lua comment")
	ctx := context.Background()
	if err := b.EnsureIndex(ctx, pin); err != nil {
		t.Fatalf("one comment blocked snapshot preparation: %v", err)
	}
	for _, mode := range []SearchMode{SearchModePrecise, SearchModeExploratory} {
		for _, topic := range []string{"api", "lua"} {
			for _, term := range []string{"issecretvalue", "SecretReturns"} {
				t.Run(string(mode)+"/"+topic+"/"+term, func(t *testing.T) {
					result, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Mode: mode, Topic: topic, Text: term, Limit: 30})
					if err != nil || len(result.Results) == 0 || result.ResolvedCommit != pin.ExactCommit {
						t.Fatalf("query = %+v, err = %v", result, err)
					}
					if result.Coverage.AnalyzedDocuments != 2 || result.Coverage.SkippedDocuments != 0 || !result.Coverage.Complete || !result.Complete || result.Coverage.Diagnostics != 0 || len(result.Coverage.DiagnosticSample) != 0 {
						t.Fatalf("encoding warning incorrectly reduced coverage: %+v", result)
					}
					if result.Coverage.Warnings != 1 || len(result.Coverage.WarningSample) != 1 {
						t.Fatalf("encoding warning lost: %+v", result.Coverage)
					}
					warning := result.Coverage.WarningSample[0]
					if warning.Path != tutorialPath || warning.Line != 2 || warning.ByteOffset != strings.IndexByte(tutorial, 0x92) || !strings.Contains(warning.Message, "0x92") {
						t.Fatalf("unlocated encoding warning: %+v", warning)
					}
				})
			}
		}
	}
	// The affected document remains searchable, including its original comment.
	for _, term := range []string{"TutorialBefore", "TutorialAfter", "Night Web"} {
		result, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Mode: SearchModePrecise, Topic: "lua", Text: term, Limit: 10})
		if err != nil || len(result.Results) != 1 || result.Results[0].Path != tutorialPath {
			t.Fatalf("affected-file query %q = %+v, err = %v", term, result, err)
		}
		if !utf8.ValidString(result.Results[0].Excerpt) || !strings.Contains(result.Results[0].Excerpt, term) {
			t.Fatalf("unreadable affected-file excerpt: %q", result.Results[0].Excerpt)
		}
	}
	t.Run("inspect", func(t *testing.T) {
		set, err := vault.WriteMetadata(ctx, b.store.Root(), func(_ *vault.Store, metadata *vault.Metadata) (selection.PinnedSet, error) {
			return selection.OpenPinner(metadata).PinSelection(ctx, selection.SelectionSpec{Source: &pin})
		})
		if err != nil {
			t.Fatal(err)
		}
		reading, err := InspectSource(ctx, b.store.Root(), set.ID, SpanQuery{Path: tutorialPath, FirstLine: 2, LineCount: 1})
		excerpt := reading.Excerpt
		if err != nil || excerpt.Text != "-- Night Web\ufffds Hollow\n" || excerpt.FirstLine != 2 || excerpt.LastLine != 2 || excerpt.TotalLines != 3 {
			t.Fatalf("legacy comment inspection = %+v, %v", excerpt, err)
		}
		result, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Mode: SearchModePrecise, Topic: "lua", Text: "Night Web", Limit: 10})
		if err != nil || len(result.Results) != 1 || !strings.Contains(result.Results[0].Excerpt, "2: "+strings.TrimSuffix(excerpt.Text, "\n")) {
			t.Fatalf("query and inspect display disagree: %+v, %v", result, err)
		}
		if len(excerpt.Warnings) != 1 || excerpt.Warnings[0] != result.Coverage.WarningSample[0] {
			t.Fatalf("query and inspect warnings disagree: %+v vs %+v", result.Coverage.WarningSample, excerpt.Warnings)
		}
		if !reading.Capture.Complete || reading.Capture.MediaType != "text/plain" || reading.Capture.Blob != excerpt.Blob || reading.Capture.Provenance.SourceCommit != pin.ExactCommit {
			t.Fatalf("raw source capture mislabeled: %+v", reading.Capture)
		}
		data, err := b.store.ReadBlob(ctx, excerpt.Blob, maxSourceBytes)
		if err != nil || !bytes.Equal(data, []byte(tutorial)) {
			t.Fatalf("inspection changed the archived source: %x, %v", data, err)
		}
	})
	t.Run("status_and_compare", func(t *testing.T) {
		status, err := b.SnapshotStatus(ctx, pin)
		if err != nil || !status.Ready || !status.Complete || status.ASTFiles != 2 || status.Coverage.Warnings != 1 {
			t.Fatalf("encoding warning reduced AST coverage: %+v, %v", status, err)
		}
		comparison, err := b.CompareTrees(ctx, SourcePair{From: pin, To: pin}, ChangeQuery{Limit: 1})
		if err != nil || comparison.DocumentChanges != 0 || comparison.DeclarationChanges != 0 || comparison.Truncated {
			t.Fatalf("encoding warning blocked comparison: %+v, %v", comparison, err)
		}
	})
	cache, _, err := b.openIndex(ctx, pin)
	if err != nil {
		t.Fatal(err)
	}
	defer cache.Close()
	data, digest, err := cache.document(ctx, tutorialPath)
	if err != nil || !bytes.Equal(data, []byte(tutorial)) || digest != fmt.Sprintf("%x", sha256.Sum256([]byte(tutorial))) {
		t.Fatalf("raw source identity changed: %x, %s, %v", data, digest, err)
	}
}

func TestInspectAddonEncodingWarningKeepsLoadValid(t *testing.T) {
	assessment, _, _, err := inspectClosureFixture(t, context.Background(), map[string][]byte{
		"Addon.toc":    []byte("Tutorial.lua\n"),
		"Tutorial.lua": []byte("-- Night Web\x92s Hollow\nfunction Tutorial() end\n"),
	})
	if err != nil || !assessment.LoadValid || len(assessment.Issues) != 1 {
		t.Fatalf("encoding warning invalidated addon load: %+v, %v", assessment, err)
	}
	issue := assessment.Issues[0]
	if issue.Severity != "warning" || issue.Code != "source_encoding" || issue.Path != "Tutorial.lua" || issue.Line != 1 || !strings.Contains(issue.Message, "0x92") {
		t.Fatalf("encoding warning not retained: %+v", issue)
	}
}
