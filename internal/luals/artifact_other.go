//go:build !windows

package luals

func watchSessionArtifacts(string, func()) (func(), error) { return nil, ErrUnavailable }

func watchWorkspace(string, func()) (func(), error) { return nil, ErrUnavailable }

func WatchIdentityDirectory(string, func()) (func(), error) { return nil, ErrUnavailable }
