package codebase

import (
	"context"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/selection"
)

// indexedSearchFixture builds and indexes one scratch pinned tree.
func indexedSearchFixture(t *testing.T, files map[string]string) (*Browser, selection.SourcePin) {
	t.Helper()
	b, seed := sourceFixture(t)
	pin := testCommit(t, b, seed, files, "search fixture")
	if _, err := b.IndexSource(context.Background(), pin); err != nil {
		t.Fatalf("IndexSource: %v", err)
	}
	return b, pin
}

func searchWidgetTree() map[string]string {
	return map[string]string{
		"Interface/AddOns/Test/Core.lua":           "function Widget()\nend\nfunction WidgetFactory()\nend\nframe:RegisterEvent(\"WIDGET_READY\")\n",
		"Interface/AddOns/Test/Locale/Strings.lua": "function Widget()\nend\n",
		"Interface/AddOns/Test/libs/Lib.lua":       "function Widget()\nend\n",
		"Interface/AddOns/Test/Addon.toc":          "## Interface: 120100\nCore.lua\n",
	}
}

func TestSearchPreciseStopsAtStrongestTier(t *testing.T) {
	b, pin := indexedSearchFixture(t, searchWidgetTree())
	ctx := context.Background()
	response, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Mode: SearchModePrecise, Text: "Widget"})
	if err != nil {
		t.Fatal(err)
	}
	if !response.Complete || response.Truncated {
		t.Fatalf("bound honesty: %+v", response)
	}
	if len(response.Results) != 3 {
		t.Fatalf("results = %+v", response.Results)
	}
	for _, match := range response.Results {
		if match.MatchedBy != "exact_symbol" {
			t.Fatalf("precise mode escalated needlessly: %+v", match)
		}
	}
}

func TestSearchExploratoryAlwaysIncludesWeakerTiers(t *testing.T) {
	b, pin := indexedSearchFixture(t, searchWidgetTree())
	ctx := context.Background()
	response, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Mode: SearchModeExploratory, Text: "Widget"})
	if err != nil {
		t.Fatal(err)
	}
	seenPrefix := false
	for _, match := range response.Results {
		if match.MatchedBy == "symbol_prefix" && match.Name == "WidgetFactory" {
			seenPrefix = true
		}
	}
	if !seenPrefix {
		t.Fatalf("exploratory mode dropped the prefix tier: %+v", response.Results)
	}
}

func TestSearchFullTextTierEscalatesAndORJoins(t *testing.T) {
	b, pin := indexedSearchFixture(t, searchWidgetTree())
	ctx := context.Background()
	precise, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Mode: SearchModePrecise, Text: "widget factory helper"})
	if err != nil {
		t.Fatal(err)
	}
	// Precise full-text requires every term; "helper" appears nowhere.
	if len(precise.Results) != 0 || len(precise.Suggestions) == 0 {
		t.Fatalf("precise full-text matched partial terms: %+v", precise)
	}
	broad, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Mode: SearchModeExploratory, Text: "widget factory helper"})
	if err != nil {
		t.Fatal(err)
	}
	found := false
	for _, match := range broad.Results {
		if match.MatchedBy == "fts5" {
			found = true
			if match.ScoreParts["match"] < 70 || match.ScoreParts["match"] > 79 {
				t.Fatalf("fts tier rank out of bounds: %+v", match.ScoreParts)
			}
		}
	}
	if !found {
		t.Fatalf("exploratory full-text OR-join matched nothing: %+v", broad.Results)
	}
}

func TestSearchRolePenaltiesRankProjectFirst(t *testing.T) {
	b, pin := indexedSearchFixture(t, searchWidgetTree())
	ctx := context.Background()
	response, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Mode: SearchModePrecise, Text: "Widget"})
	if err != nil {
		t.Fatal(err)
	}
	wantRoles := []string{"project", "vendor", "locale"}
	if len(response.Results) != len(wantRoles) {
		t.Fatalf("results = %+v", response.Results)
	}
	for i, match := range response.Results {
		if match.Role != wantRoles[i] {
			t.Fatalf("rank %d role = %q, want %q: %+v", i, match.Role, wantRoles[i], response.Results)
		}
	}
	penalties := map[string]int{"project": 0, "vendor": -15, "locale": -20}
	for _, match := range response.Results {
		if match.Score != 100+penalties[match.Role] {
			t.Fatalf("score = %d for role %q: %+v", match.Score, match.Role, match)
		}
		if match.ScoreParts["match"] != 100 || match.ScoreParts["rolePenalty"] != penalties[match.Role] {
			t.Fatalf("scoreParts = %+v", match.ScoreParts)
		}
	}
}

func pngFixture(width, height int) string {
	data := make([]byte, 32)
	copy(data, []byte{137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13})
	copy(data[12:], "IHDR")
	data[16] = byte(width >> 24)
	data[17] = byte(width >> 16)
	data[18] = byte(width >> 8)
	data[19] = byte(width)
	data[20] = byte(height >> 24)
	data[21] = byte(height >> 16)
	data[22] = byte(height >> 8)
	data[23] = byte(height)
	return string(data)
}

func topicTree() map[string]string {
	return map[string]string{
		"Interface/AddOns/Blizzard_APIDocumentationGenerated/Docs.lua": `return {Type="System",Name="C_Topic",Namespace="C_Topic",Functions={{Type="Function",Name="Do"}}}`,
		"Interface/AddOns/Test/Core.lua":                               "function Plain()\nend\n",
		"Interface/AddOns/Test/UI.xml":                                 "<Ui><Frame name=\"TopicFrame\" inherits=\"BaseTemplate\" /></Ui>",
		"Interface/AddOns/Test/Addon.toc":                              "## Interface: 120100\nCore.lua\n",
		"Interface/AddOns/Test/Textures/logo.png":                      pngFixture(64, 32),
	}
}

func TestSearchTopicFilters(t *testing.T) {
	b, pin := indexedSearchFixture(t, topicTree())
	ctx := context.Background()
	cases := []struct {
		text, topic string
		wantKind    func(Match) bool
	}{
		{"C_Topic.Do", "api", func(m Match) bool { return strings.HasPrefix(m.Kind, "api-") }},
		{"Plain", "lua", func(m Match) bool { return strings.HasSuffix(m.Path, ".lua") }},
		{"TopicFrame", "xml", func(m Match) bool { return strings.HasSuffix(m.Path, ".xml") }},
		{"Interface", "toc", func(m Match) bool { return strings.HasSuffix(m.Path, ".toc") && m.Kind == "toc" }},
		{"logo", "asset", func(m Match) bool { return m.Kind == "asset" && m.MatchedBy == "asset_path" }},
	}
	for _, testCase := range cases {
		response, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Mode: SearchModePrecise, Text: testCase.text, Topic: testCase.topic})
		if err != nil {
			t.Fatalf("%s: %v", testCase.topic, err)
		}
		if len(response.Results) == 0 {
			t.Fatalf("topic %q matched nothing: %+v", testCase.topic, response)
		}
		for _, match := range response.Results {
			if !testCase.wantKind(match) {
				t.Fatalf("topic %q leaked %+v", testCase.topic, match)
			}
		}
	}
}

func TestSearchAssetPathExactRanksFirst(t *testing.T) {
	b, pin := indexedSearchFixture(t, topicTree())
	ctx := context.Background()
	response, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Text: "Interface/AddOns/Test/Textures/logo.png", Topic: "asset"})
	if err != nil {
		t.Fatal(err)
	}
	if len(response.Results) != 1 {
		t.Fatalf("results = %+v", response.Results)
	}
	match := response.Results[0]
	if match.Score != 100 || match.ContentHash == "" || match.Excerpt != match.Path {
		t.Fatalf("asset row = %+v", match)
	}
}

func TestSearchRejectsInvalidTopicAndMode(t *testing.T) {
	b, pin := indexedSearchFixture(t, topicTree())
	ctx := context.Background()
	if _, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Text: "x", Topic: "bogus"}); err == nil || !strings.Contains(err.Error(), "codebase.invalid_topic") {
		t.Fatalf("invalid topic accepted: %v", err)
	}
	if _, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Text: "x", Mode: "chaotic"}); err == nil || !strings.Contains(err.Error(), "codebase.invalid_search_mode") {
		t.Fatalf("invalid mode accepted: %v", err)
	}
}

func TestSearchLimitTruncatesHonestly(t *testing.T) {
	b, pin := indexedSearchFixture(t, searchWidgetTree())
	ctx := context.Background()
	response, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Mode: SearchModeExploratory, Text: "Widget", Limit: 2})
	if err != nil {
		t.Fatal(err)
	}
	if len(response.Results) != 2 || !response.Truncated || response.Complete {
		t.Fatalf("truncation not reported: %+v", response)
	}
}

func TestSearchRelationsExactAndExploratory(t *testing.T) {
	b, pin := indexedSearchFixture(t, searchWidgetTree())
	ctx := context.Background()
	precise, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Mode: SearchModePrecise, Text: "WIDGET"})
	if err != nil {
		t.Fatal(err)
	}
	if len(precise.Relations) != 0 {
		t.Fatalf("precise relations matched by substring: %+v", precise.Relations)
	}
	broad, err := b.Search(ctx, "PIN-TEST", pin, SearchQuery{Mode: SearchModeExploratory, Text: "WIDGET"})
	if err != nil {
		t.Fatal(err)
	}
	found := false
	for _, relation := range broad.Relations {
		if relation.Target == "WIDGET_READY" && relation.Kind == "event-registration" {
			found = true
		}
	}
	if !found {
		t.Fatalf("exploratory relations missed the edge: %+v", broad.Relations)
	}
}
