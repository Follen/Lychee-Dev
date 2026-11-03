//go:build !windows

package publish

import "os"

func openArtifact(path string) (*os.File, error) { return os.Open(path) }
