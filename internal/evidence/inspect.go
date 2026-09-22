package evidence

import (
	"context"
	"github.com/follenfang/lycheedev/internal/vault"
)

func InspectEvidence(ctx context.Context, root, id string, verify bool) (CaptureRef, error) {
	return vault.ReadWorkspace(ctx, root, func(s *vault.Store, m *vault.Metadata) (CaptureRef, error) {
		a := OpenArchive(s, m)
		ref, err := a.InspectCapture(ctx, id)
		if err != nil {
			return ref, err
		}
		if verify {
			err = a.VerifyCapture(ctx, id)
		}
		return ref, err
	})
}
