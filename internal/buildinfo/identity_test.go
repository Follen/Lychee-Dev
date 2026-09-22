package buildinfo

import (
	"runtime/debug"
	"strings"
	"testing"
)

func TestBinarySourceIdentity(t *testing.T) {
	revision := strings.Repeat("a", 40)
	for _, value := range []string{"true", "false", "", "invalid"} {
		info := &debug.BuildInfo{Settings: []debug.BuildSetting{{Key: "vcs", Value: "git"}, {Key: "vcs.revision", Value: revision}, {Key: "vcs.modified", Value: value}}}
		got := fromBuildInfo(info)
		if got.Commit != revision {
			t.Fatal("revision lost")
		}
		if value == "true" || value == "false" {
			if got.WorkspaceDirty == nil || *got.WorkspaceDirty != (value == "true") {
				t.Fatalf("wrong dirty state: %+v", got)
			}
		} else if got.WorkspaceDirty != nil {
			t.Fatal("unknown state became clean")
		}
	}
	for _, info := range []*debug.BuildInfo{nil, {}, {Settings: []debug.BuildSetting{{Key: "vcs", Value: "git"}, {Key: "vcs.revision", Value: "invalid"}}}, {Settings: []debug.BuildSetting{{Key: "vcs", Value: "git"}, {Key: "vcs.revision", Value: revision}, {Key: "vcs.modified", Value: "false"}, {Key: "vcs.modified", Value: "true"}}}} {
		got := fromBuildInfo(info)
		if got.Commit != "" || got.WorkspaceDirty != nil {
			t.Fatalf("invalid identity accepted: %+v", got)
		}
	}
}

func TestCurrentIdentityIncludesRuntime(t *testing.T) {
	got := Current()
	if got.Version != Version || got.Go == "" || got.Platform == "" {
		t.Fatalf("missing runtime identity: %+v", got)
	}
}
