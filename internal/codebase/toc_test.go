package codebase

import (
	"reflect"
	"testing"
)

func TestScanManifestPreservesOrderAndDuplicates(t *testing.T) {
	data := []byte("## Interface: 120100\nCore/First.lua\n## Unknown: first\n# ignored\nCore/Second.lua\n## Unknown: second\nCore/First.lua\n## malformed\n")

	facts := scanManifest(data)
	wantHeaders := []HeaderValue{
		{Key: "Interface", Value: "120100", Line: 1},
		{Key: "Unknown", Value: "first", Line: 3},
		{Key: "Unknown", Value: "second", Line: 6},
	}
	wantLoads := []LoadReference{
		{Path: "Core/First.lua", Line: 2, Kind: "toc"},
		{Path: "Core/Second.lua", Line: 5, Kind: "toc"},
		{Path: "Core/First.lua", Line: 7, Kind: "toc"},
	}

	if !reflect.DeepEqual(facts.Headers, wantHeaders) {
		t.Fatalf("headers = %#v, want %#v", facts.Headers, wantHeaders)
	}
	if !reflect.DeepEqual(facts.Loads, wantLoads) {
		t.Fatalf("loads = %#v, want %#v", facts.Loads, wantLoads)
	}
}

func TestScanManifestRemovesBOMWithoutChangingLineNumbers(t *testing.T) {
	data := append([]byte{0xef, 0xbb, 0xbf}, []byte("## Version: 2.0.0\nCore/Bootstrap.lua\n")...)

	facts := scanManifest(data)
	if got, want := facts.Headers, []HeaderValue{{Key: "Version", Value: "2.0.0", Line: 1}}; !reflect.DeepEqual(got, want) {
		t.Fatalf("headers = %#v, want %#v", got, want)
	}
	if got, want := facts.Loads, []LoadReference{{Path: "Core/Bootstrap.lua", Line: 2, Kind: "toc"}}; !reflect.DeepEqual(got, want) {
		t.Fatalf("loads = %#v, want %#v", got, want)
	}
}

func TestScanManifestSplitsHeaderAtFirstColonAndHandlesCRLF(t *testing.T) {
	data := []byte("## Note: left: right\r\n\r\n# comment\r\nModules/Path:With:Colons.lua\r\n")

	facts := scanManifest(data)
	wantHeaders := []HeaderValue{{Key: "Note", Value: "left: right", Line: 1}}
	wantLoads := []LoadReference{{Path: "Modules/Path:With:Colons.lua", Line: 4, Kind: "toc"}}
	if !reflect.DeepEqual(facts.Headers, wantHeaders) {
		t.Fatalf("headers = %#v, want %#v", facts.Headers, wantHeaders)
	}
	if !reflect.DeepEqual(facts.Loads, wantLoads) {
		t.Fatalf("loads = %#v, want %#v", facts.Loads, wantLoads)
	}
}
