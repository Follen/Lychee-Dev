package evidence

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/vault"
)

func TestLegacyCaptureReferenceDoesNotScanArbitraryPayloads(t *testing.T) {
	for _, name := range []string{"other-kind", "other-schema", "other-field", "other-media-type", "other-target-kind", "other-hotfix-provider", "explicit-dependencies"} {
		t.Run(name, func(t *testing.T) {
			ctx := context.Background()
			s, err := vault.Initialize(ctx, t.TempDir())
			if err != nil {
				t.Fatal(err)
			}
			m, err := s.OpenMetadata(ctx)
			if err != nil {
				t.Fatal(err)
			}
			defer m.Close()
			a := OpenArchive(s, m)
			targetKind := "asset-demux-frame"
			if name == "other-target-kind" {
				targetKind = "fixture"
			} else if name == "other-hotfix-provider" {
				targetKind = "hotfix-cache"
			} else if name == "explicit-dependencies" {
				targetKind = "asset-image"
			}
			target, err := a.CommitCapture(ctx, CaptureDraft{Reader: strings.NewReader("original bytes"), MaxBytes: 1024,
				MediaType: "video/VP9", Complete: true, Provenance: Provenance{Kind: targetKind, Locator: "frame"}})
			if err != nil {
				t.Fatal(err)
			}
			kind, schema, mediaType := "asset-demux-manifest", "lycheedev.asset-demux.v1", "application/json"
			switch name {
			case "other-kind":
				kind = "fixture"
			case "other-schema":
				schema = "unrelated.v1"
			case "other-media-type":
				mediaType = "application/octet-stream"
			}
			body := fmt.Sprintf(`{"schema":%q,"frames":[{"capture":%q}]}`, schema, target.ID)
			if name == "other-field" {
				body = fmt.Sprintf(`{"schema":%q,"notes":{"frames":[{"capture":%q}]}}`, schema, target.ID)
			}
			locator := "unrelated"
			if name == "other-hotfix-provider" {
				kind, locator = "hotfix-records", "wago:sha256:remote"
				body = fmt.Sprintf(`{"source":{"schema":"lycheedev.capture.v1","id":%q}}`, target.ID)
			}
			var dependencies []string
			if name == "explicit-dependencies" {
				kind = "asset-export-manifest"
				body = fmt.Sprintf(`{"schema":"lycheedev.asset-export.v1","artifactCapture":{"schema":"lycheedev.capture.v1","id":%q}}`, target.ID)
				other, err := a.CommitCapture(ctx, CaptureDraft{Reader: strings.NewReader("other bytes"), MaxBytes: 1024,
					MediaType: "text/plain", Complete: true, Provenance: Provenance{Kind: "fixture", Locator: "other"}})
				if err != nil {
					t.Fatal(err)
				}
				dependencies = []string{other.ID}
			}
			owner, err := a.CommitCapture(ctx, CaptureDraft{Reader: strings.NewReader(body), MaxBytes: 1024,
				MediaType: mediaType, Complete: true, Provenance: Provenance{Kind: kind, Locator: locator}, Dependencies: dependencies})
			if err != nil {
				t.Fatal(err)
			}
			if _, err := a.RemoveCapture(ctx, target.ID); err != nil {
				t.Fatalf("unrelated body became a capture reference: %v", err)
			}
			_, got, err := a.FetchCapture(ctx, owner.ID, 1024)
			if err != nil || string(got) != body {
				t.Fatalf("unrelated capture changed: %v", err)
			}
		})
	}
}

func TestLegacyCaptureReferenceUsesOriginalProducerByteBound(t *testing.T) {
	ctx := context.Background()
	s, err := vault.Initialize(ctx, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	m, err := s.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer m.Close()
	a := OpenArchive(s, m)
	target, err := a.CommitCapture(ctx, CaptureDraft{Reader: strings.NewReader("image bytes"), MaxBytes: 1024,
		MediaType: "image/png", Complete: true, Provenance: Provenance{Kind: "asset-image", Locator: "image"}})
	if err != nil {
		t.Fatal(err)
	}
	// A purported legacy export beyond its producer's 1 MiB bound must not
	// turn a removal check into an unbounded payload scan or lose the target.
	body := fmt.Sprintf(`{"schema":"lycheedev.asset-export.v1","artifactCapture":{"schema":"lycheedev.capture.v1","id":%q},"padding":%q}`, target.ID, strings.Repeat("x", 1<<20))
	owner, err := a.CommitCapture(ctx, CaptureDraft{Reader: strings.NewReader(body), MaxBytes: 2 << 20,
		MediaType: "application/json", Complete: true, Provenance: Provenance{Kind: "asset-export-manifest", Locator: "export"}})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := a.RemoveCapture(ctx, target.ID); !errors.Is(err, vault.ErrBlobLimit) {
		t.Fatalf("oversized legacy dependency scan = %v", err)
	}
	if err := a.VerifyCapture(ctx, target.ID); err != nil {
		t.Fatal(err)
	}
	if _, err := a.RemoveCapture(ctx, owner.ID); err != nil {
		t.Fatal(err)
	}
	if _, err := a.RemoveCapture(ctx, target.ID); err != nil {
		t.Fatal(err)
	}
}
