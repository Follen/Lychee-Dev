package delivery

import (
	"errors"
	"strings"
	"testing"
)

const testReleaseCommit = "0123456789abcdef0123456789abcdef01234567"

func validSourceArchive() *SourceArchive {
	return &SourceArchive{
		Repository: "https://github.com/Follen/Lychee-Dev",
		Commit:     testReleaseCommit,
		Tag:        "v2.0.0",
		Archive:    "lycheedev-2.0.0-corresponding-source.tar.gz",
		SHA256:     strings.Repeat("ab", 32),
	}
}

func TestValidateSourceArchiveOptional(t *testing.T) {
	if err := validateSourceArchive(nil, testReleaseCommit); err != nil {
		t.Fatalf("nil record must be accepted: %v", err)
	}
}

func TestValidateSourceArchiveAcceptsExactRecord(t *testing.T) {
	if err := validateSourceArchive(validSourceArchive(), testReleaseCommit); err != nil {
		t.Fatalf("valid record rejected: %v", err)
	}
}

func TestValidateSourceArchiveRejects(t *testing.T) {
	cases := map[string]func(*SourceArchive){
		"commit mismatch":       func(s *SourceArchive) { s.Commit = strings.Repeat("0", 40) },
		"commit not release":    func(s *SourceArchive) { s.Commit = strings.Repeat("c", 40) },
		"commit uppercase":      func(s *SourceArchive) { s.Commit = strings.ToUpper(testReleaseCommit) },
		"repository empty":      func(s *SourceArchive) { s.Repository = "" },
		"repository over bound": func(s *SourceArchive) { s.Repository = strings.Repeat("a", 513) },
		"tag empty":             func(s *SourceArchive) { s.Tag = "" },
		"tag newline":           func(s *SourceArchive) { s.Tag = "v2.0.0\nx" },
		"archive empty":         func(s *SourceArchive) { s.Archive = "" },
		"archive with slash":    func(s *SourceArchive) { s.Archive = "a/b.tar.gz" },
		"archive with backslash": func(s *SourceArchive) {
			s.Archive = `a\b.tar.gz`
		},
		"sha short":   func(s *SourceArchive) { s.SHA256 = strings.Repeat("ab", 31) },
		"sha upper":   func(s *SourceArchive) { s.SHA256 = strings.ToUpper(strings.Repeat("ab", 32)) },
		"sha non-hex": func(s *SourceArchive) { s.SHA256 = strings.Repeat("zz", 32) },
	}
	for name, mutate := range cases {
		source := validSourceArchive()
		mutate(source)
		err := validateSourceArchive(source, testReleaseCommit)
		if !errors.Is(err, ErrRelease) {
			t.Fatalf("%s: want ErrRelease, got %v", name, err)
		}
	}
}
