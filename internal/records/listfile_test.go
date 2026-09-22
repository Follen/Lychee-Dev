package records_test

import (
	"bytes"
	"context"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
)

func TestParseTextListfileCommunityCSV(t *testing.T) {
	data := []byte("\xEF\xBB\xBF# comment line\r\n" +
		"132089;Interface\\Icons\\Ability_Ambush.blp\r\n" + // backslash + case
		"132090,interface/icons/ability_backstab.blp\n" + // comma separator
		`132091;"quoted/name with, comma.blp"` + "\n" + // optional CSV quoting
		"134400;world/china/xinnian.blp\n" + // UTF-8 name
		"132089;interface/icons/other_name.blp\n" + // duplicate ID, second name
		"999999;dup.blp\n1000000;dup.blp\n" + // duplicate name, two IDs
		"\r\nnot-a-listing\nabc;name.blp\n0;zero.blp\n4294967296;huge.blp\n12345;\n   \n")
	index, err := records.ParseTextListfile(records.ListfileCommunityCSV, data, records.ListfileLimits{})
	if err != nil {
		t.Fatal(err)
	}
	if index.Len() != 7 || index.Provenance().EntryCount != 7 {
		t.Fatalf("entries = %d, %+v", index.Len(), index.Provenance())
	}
	// Rejected: not-a-listing, abc;name.blp, 0;zero.blp, 4294967296;huge.blp, 12345;
	if index.Provenance().Rejected != 5 {
		t.Fatalf("rejected = %d, want 5", index.Provenance().Rejected)
	}
	names := index.Names(132089)
	if len(names) != 2 || names[0].FileName != "interface/icons/ability_ambush.blp" || names[0].SourceName != "Interface\\Icons\\Ability_Ambush.blp" || names[1].FileName != "interface/icons/other_name.blp" {
		t.Fatalf("id 132089 names = %+v", names)
	}
	if got := index.Names(12345); got != nil {
		t.Fatalf("rejected ID kept: %+v", got)
	}
	if matches := index.IDs(`Interface\Icons\Ability_Ambush.blp`); len(matches) != 1 || matches[0].FileDataID != 132089 {
		t.Fatalf("normalized lookup = %+v", matches)
	}
	if matches := index.IDs("quoted/name with, comma.blp"); len(matches) != 1 || matches[0].FileDataID != 132091 {
		t.Fatalf("quoted name lookup = %+v", matches)
	}
	if matches := index.IDs("dup.blp"); len(matches) != 2 || matches[0].FileDataID != 999999 || matches[1].FileDataID != 1000000 {
		t.Fatalf("multi-ID lookup = %+v", matches)
	}
	if matches := index.IDs("world/china/xinnian.blp"); len(matches) != 1 || matches[0].SourceName != "world/china/xinnian.blp" {
		t.Fatalf("UTF-8 lookup = %+v", matches)
	}
}

func TestParseTextListfileWowExportTextAndBudgets(t *testing.T) {
	if _, err := records.ParseTextListfile(records.ListfileWowExportBinary, []byte("1;a.blp"), records.ListfileLimits{}); !errors.Is(err, records.ErrListfileQuery) {
		t.Fatalf("binary kind into text parser: %v", err)
	}
	index, err := records.ParseTextListfile(records.ListfileWowExportText, []byte("1;a.blp\n2;b.blp"), records.ListfileLimits{})
	if err != nil || index.Len() != 2 {
		t.Fatalf("wow.export text: %v %d", err, index.Len())
	}
	big := bytes.Repeat([]byte("12345;a-name-that-is-long.blp\n"), 200)
	if _, err := records.ParseTextListfile(records.ListfileCommunityCSV, big, records.ListfileLimits{Bytes: 1024}); !errors.Is(err, records.ErrListfileLimit) {
		t.Fatalf("byte budget: %v", err)
	}
	if _, err := records.ParseTextListfile(records.ListfileCommunityCSV, big, records.ListfileLimits{Entries: 1}); !errors.Is(err, records.ErrListfileLimit) {
		t.Fatalf("entry budget: %v", err)
	}
	longLines := append([]byte("12345;"), bytes.Repeat([]byte("x"), 200)...)
	longLines = append(longLines, ".blp\n"...)
	longLines = bytes.Repeat(longLines, 5)
	index, err = records.ParseTextListfile(records.ListfileCommunityCSV, longLines, records.ListfileLimits{LineBytes: 128})
	if err != nil {
		t.Fatal(err)
	}
	if index.Len() != 0 || index.Provenance().Rejected != 5 {
		t.Fatalf("line budget = %d entries, %d rejected", index.Len(), index.Provenance().Rejected)
	}
	if _, err := records.ParseTextListfile(records.ListfileCommunityCSV, nil, records.ListfileLimits{Entries: -1}); !errors.Is(err, records.ErrListfileLimit) {
		t.Fatalf("invalid limits: %v", err)
	}
}

// buildBinaryListfile assembles wow.export's componentized binary listfile.
func buildBinaryListfile(t *testing.T, entries []records.ListfileMatch) map[string][]byte {
	t.Helper()
	pools := map[string]*bytes.Buffer{}
	components := map[string][]byte{}
	for _, name := range []string{"listfile-strings.dat", "listfile-pf-models.dat", "listfile-pf-textures.dat", "listfile-pf-sounds.dat", "listfile-pf-videos.dat", "listfile-pf-text.dat", "listfile-pf-fonts.dat"} {
		pools[name] = &bytes.Buffer{}
	}
	var index bytes.Buffer
	poolNames := records.BinaryListfileComponents()[1:]
	for _, entry := range entries {
		component := entry.Component
		if component == "" {
			component = poolNames[0]
		}
		pool := pools[component]
		offset := pool.Len()
		pool.WriteString(entry.SourceName)
		pool.WriteByte(0)
		position := -1
		for i, name := range poolNames {
			if name == component {
				position = i
			}
		}
		index.Write([]byte{byte(entry.FileDataID >> 24), byte(entry.FileDataID >> 16), byte(entry.FileDataID >> 8), byte(entry.FileDataID)})
		index.Write([]byte{byte(offset >> 24), byte(offset >> 16), byte(offset >> 8), byte(offset)})
		index.WriteByte(byte(position))
	}
	components["listfile-id-index.dat"] = index.Bytes()
	for name, pool := range pools {
		components[name] = pool.Bytes()
	}
	return components
}

func TestParseBinaryListfile(t *testing.T) {
	components := buildBinaryListfile(t, []records.ListfileMatch{
		{FileDataID: 7, SourceName: `Creature\Bear.blp`, Component: "listfile-pf-textures.dat"},
		{FileDataID: 8, SourceName: "interface/framexml/ui.m2", Component: "listfile-pf-models.dat"},
		{FileDataID: 9, SourceName: "sound/cow.ogg", Component: "listfile-pf-sounds.dat"},
	})
	index, err := records.ParseBinaryListfile(components, records.ListfileLimits{})
	if err != nil {
		t.Fatal(err)
	}
	if index.Len() != 3 || index.Provenance().Kind != records.ListfileWowExportBinary {
		t.Fatalf("binary index = %+v", index.Provenance())
	}
	matches := index.Names(7)
	if len(matches) != 1 || matches[0].FileName != "creature/bear.blp" || matches[0].Component != "listfile-pf-textures.dat" {
		t.Fatalf("pool attribution = %+v", matches)
	}
	broken := buildBinaryListfile(t, []records.ListfileMatch{{FileDataID: 7, SourceName: "a.blp"}})
	broken["listfile-id-index.dat"] = broken["listfile-id-index.dat"][:4]
	if _, err := records.ParseBinaryListfile(broken, records.ListfileLimits{}); !errors.Is(err, records.ErrListfileFormat) {
		t.Fatalf("bad index length: %v", err)
	}
	missing := buildBinaryListfile(t, []records.ListfileMatch{{FileDataID: 7, SourceName: "a.blp"}})
	delete(missing, "listfile-strings.dat")
	if _, err := records.ParseBinaryListfile(missing, records.ListfileLimits{}); !errors.Is(err, records.ErrListfileFormat) {
		t.Fatalf("missing pool: %v", err)
	}
	// A valid index entry pointing at an unterminated string must fail loudly.
	unterminated := buildBinaryListfile(t, []records.ListfileMatch{{FileDataID: 7, SourceName: "a.blp"}})
	unterminated["listfile-strings.dat"] = []byte("a.blp") // no NUL terminator
	if _, err := records.ParseBinaryListfile(unterminated, records.ListfileLimits{}); !errors.Is(err, records.ErrListfileFormat) {
		t.Fatalf("unterminated string: %v", err)
	}
	outOfRange := buildBinaryListfile(t, []records.ListfileMatch{{FileDataID: 7, SourceName: "a.blp"}})
	indexBytes := outOfRange["listfile-id-index.dat"]
	indexBytes[7] = 0xF0 // string offset far past the pool
	if _, err := records.ParseBinaryListfile(outOfRange, records.ListfileLimits{}); !errors.Is(err, records.ErrListfileFormat) {
		t.Fatalf("offset out of range: %v", err)
	}
	badPool := buildBinaryListfile(t, []records.ListfileMatch{{FileDataID: 7, SourceName: "a.blp"}})
	badPool["listfile-id-index.dat"][8] = 9
	if _, err := records.ParseBinaryListfile(badPool, records.ListfileLimits{}); !errors.Is(err, records.ErrListfileFormat) {
		t.Fatalf("bad pool index: %v", err)
	}
}

func TestListfileIndexQueries(t *testing.T) {
	index, err := records.ParseTextListfile(records.ListfileCommunityCSV, []byte(
		"1;interface/icons/alpha.blp\n2;interface/icons/beta.blp\n3;interface/icons/beta.blp\n"+
			"4;creature/wolf.m2\n5;interface/misc/blip.wav\n"), records.ListfileLimits{})
	if err != nil {
		t.Fatal(err)
	}
	total, files := index.Search("ICONS/", 2)
	if total != 3 || len(files) != 2 || files[0].FileDataID != 1 {
		t.Fatalf("search = %d %+v", total, files)
	}
	total, files = index.Search("nothing-here", 5)
	if total != 0 || files != nil {
		t.Fatalf("empty search = %d %+v", total, files)
	}
	total, files = index.ByExtension("blp", 10)
	if total != 3 || len(files) != 3 || files[0].FileName != "interface/icons/alpha.blp" {
		t.Fatalf("extension = %d %+v", total, files)
	}
	total, files = index.ByExtension(".BLP", 1)
	if total != 3 || len(files) != 1 {
		t.Fatalf("extension limit = %d %+v", total, files)
	}
	matches := index.ResolveName("Creature\\Wolf.MDX")
	if len(matches) != 1 || matches[0].FileDataID != 4 || matches[0].MatchedVia != "m2-alias" {
		t.Fatalf("model alias = %+v", matches)
	}
	if matches := index.ResolveName("creature/wolf.m2"); len(matches) != 1 || matches[0].MatchedVia != "" {
		t.Fatalf("exact match mislabeled: %+v", matches)
	}
	if matches := index.ResolveName("creature/wolf.mdl"); len(matches) != 1 || matches[0].MatchedVia != "m2-alias" {
		t.Fatalf("mdl alias = %+v", matches)
	}
}

func TestFileNameLookupEntryPoints(t *testing.T) {
	ctx := context.Background()
	workspace := workspaceRoot(t)
	pin := cachedAssetPin(t, workspace, []byte("payload"))
	listfile := records.ListfileRequest{
		Kind: records.ListfileCommunityCSV,
		Fetch: records.ListfileFetchFunc(func(ctx context.Context, url string, maxBytes int64) ([]byte, error) {
			return []byte("11;Interface\\Icons\\Test.blp\n12;missing/file.blp\n13;dup.blp\n14;dup.blp\n"), nil
		}),
	}
	lookup, err := records.LookupFileNames(ctx, workspace, records.FileNameLookupRequest{
		Snapshot: pin.ID, Listfile: listfile, FileDataIDs: []uint32{11, 13, 99},
	})
	if err != nil {
		t.Fatal(err)
	}
	if lookup.Entries[0].FileName != "interface/icons/test.blp" || lookup.Entries[2].FileName != "unknown" {
		t.Fatalf("lookup = %+v", lookup.Entries)
	}
	if len(lookup.Entries[1].Names) != 1 || lookup.Context.Snapshot != pin.ID || lookup.Context.Pin != *pin.Data || lookup.Context.Listfile == nil {
		t.Fatalf("lookup context = %+v", lookup)
	}
	resolution, err := records.ResolveFileDataIDs(ctx, workspace, records.FileNameResolveRequest{
		Snapshot: pin.ID, Listfile: listfile, FileName: `interface\icons\test.BLP`,
	})
	if err != nil {
		t.Fatal(err)
	}
	if !resolution.Resolved || len(resolution.Matches) != 1 || resolution.Matches[0].FileDataID != 11 {
		t.Fatalf("resolution = %+v", resolution)
	}
	duplicated, err := records.ResolveFileDataIDs(ctx, workspace, records.FileNameResolveRequest{
		Snapshot: pin.ID, Listfile: listfile, FileName: "dup.blp",
	})
	if err != nil {
		t.Fatal(err)
	}
	if len(duplicated.Matches) != 2 || duplicated.Matches[0].FileDataID != 13 || duplicated.Matches[1].FileDataID != 14 {
		t.Fatalf("multi-match resolution = %+v", duplicated.Matches)
	}
	unresolved, err := records.ResolveFileDataIDs(ctx, workspace, records.FileNameResolveRequest{
		Snapshot: pin.ID, Listfile: listfile, FileName: "unknown/name.blp",
	})
	if err != nil {
		t.Fatal(err)
	}
	if unresolved.Resolved || len(unresolved.Matches) != 0 {
		t.Fatalf("unresolved = %+v", unresolved)
	}
	search, err := records.SearchFileNames(ctx, workspace, records.FileSearchRequest{
		Snapshot: pin.ID, Listfile: listfile, Query: "interface", Limit: 1,
	})
	if err != nil {
		t.Fatal(err)
	}
	if search.Total != 1 || search.Returned != 1 || search.Truncated {
		t.Fatalf("search = %+v", search)
	}
	searchAll, err := records.SearchFileNames(ctx, workspace, records.FileSearchRequest{
		Snapshot: pin.ID, Listfile: listfile, Query: "blp", Limit: 100,
	})
	if err != nil {
		t.Fatal(err)
	}
	if searchAll.Total != 4 || searchAll.Truncated {
		t.Fatalf("search all = %+v", searchAll)
	}
	searchLimited, err := records.SearchFileNames(ctx, workspace, records.FileSearchRequest{
		Snapshot: pin.ID, Listfile: listfile, Query: "blp", Limit: 2,
	})
	if err != nil {
		t.Fatal(err)
	}
	if searchLimited.Total != 4 || searchLimited.Returned != 2 || !searchLimited.Truncated {
		t.Fatalf("search limit = %+v", searchLimited)
	}
	extension, err := records.ListFileExtensions(ctx, workspace, records.FileExtensionRequest{
		Snapshot: pin.ID, Listfile: listfile, Extension: "blp", Limit: 2,
	})
	if err != nil {
		t.Fatal(err)
	}
	if extension.Total != 4 || extension.Returned != 2 || !extension.Truncated {
		t.Fatalf("extension = %+v", extension)
	}
	if _, err := records.SearchFileNames(ctx, workspace, records.FileSearchRequest{Snapshot: pin.ID, Listfile: listfile, Query: "x", Limit: 0}); !errors.Is(err, records.ErrListfileLimit) {
		t.Fatalf("limit validation: %v", err)
	}
	if _, err := records.LookupFileNames(ctx, workspace, records.FileNameLookupRequest{Snapshot: "", Listfile: listfile, FileDataIDs: []uint32{11}}); !errors.Is(err, records.ErrSnapshotRequired) {
		t.Fatalf("snapshot validation: %v", err)
	}
}
