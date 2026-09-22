package buildinfo

import (
	"runtime"
	"runtime/debug"
	"strings"
)

// Identity describes the binary itself. Missing VCS settings remain unknown;
// inspecting the user's current directory would identify a different artifact.
type Identity struct {
	Version        string `json:"version"`
	Go             string `json:"go"`
	Platform       string `json:"platform"`
	Commit         string `json:"commit,omitempty"`
	WorkspaceDirty *bool  `json:"workspaceDirty"`
}

func Current() Identity {
	info, _ := debug.ReadBuildInfo()
	result := fromBuildInfo(info)
	result.Version = Version
	result.Go = runtime.Version()
	result.Platform = runtime.GOOS + "/" + runtime.GOARCH
	return result
}

func fromBuildInfo(info *debug.BuildInfo) Identity {
	var result Identity
	if info == nil {
		return result
	}
	values := make(map[string]string)
	for _, setting := range info.Settings {
		if _, duplicate := values[setting.Key]; duplicate {
			return result
		}
		values[setting.Key] = setting.Value
	}
	if values["vcs"] != "git" {
		return result
	}
	revision := values["vcs.revision"]
	if len(revision) != 40 || strings.IndexFunc(revision, func(c rune) bool { return !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f') }) >= 0 {
		return result
	}
	result.Commit = revision
	switch values["vcs.modified"] {
	case "true":
		dirty := true
		result.WorkspaceDirty = &dirty
	case "false":
		dirty := false
		result.WorkspaceDirty = &dirty
	}
	return result
}
