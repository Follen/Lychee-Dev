//go:build windows && amd64

package duplexhost

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/live/duplex"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

func TestDirectPublicationJournalsExactPinsAndDoesNotReplayUnknown(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	n := &Native{TraceDir: t.TempDir()}
	n.Target.Window.ProcessID, n.Target.Window.ProcessStartedAt, n.Target.Window.Executable = 7, 9, "Wow.exe"
	n.Target.Client.FullBuild, n.Target.Client.Product = "12.1.0.69933", "retail"
	binding := memory.LuaRootBinding{ExecutableSHA256: memory.RetailLuaMailboxExecutableSHA256}
	message := stoppedMessageFixture(t)
	path := filepath.Join(n.TraceDir, fmt.Sprintf("%s-%d.jsonl", message.Header.MessageID, message.Header.PublicationSeq))
	calls := 0
	publish := func(gotCtx context.Context, request memory.DirectPublicationRequest) (duplex.WriteOutcome, []memory.DuplexWriteRange, error) {
		calls++
		if gotCtx != ctx || request.Target.PID != 7 || request.Target.Created != 9 || request.Target.Image != "Wow.exe" || request.ActorGUID != "actor" || request.ExecutableSHA256 != binding.ExecutableSHA256 || request.Message.Header != message.Header {
			t.Fatal("direct request changed identity")
		}
		body, err := os.ReadFile(path)
		if err != nil {
			t.Fatal("direct publication preceded intent", err)
		}
		var intent struct {
			Mode          string
			PayloadBytes  int
			PayloadSHA256 string
			WriterProfile memory.DuplexWriterProfile
		}
		if err := json.Unmarshal(bytes.TrimSpace(body), &intent); err != nil || intent.Mode != "direct" || intent.PayloadBytes != len(message.Payload) || intent.PayloadSHA256 != digestBytes(message.Payload) || !intent.WriterProfile.CanDirectWrite() {
			t.Fatal("incomplete intent profile", intent, err)
		}
		return duplex.WriteOutcome{State: duplex.UnknownWrite}, nil, duplex.ErrUnknown
	}
	if out, err := n.publishJournaled(ctx, message, binding, "actor", publish); out.State != duplex.UnknownWrite || !errors.Is(err, duplex.ErrUnknown) {
		t.Fatal(out, err)
	}
	body, err := os.ReadFile(path)
	if err != nil || strings.Contains(string(body), `"stop"`) || strings.Contains(string(body), "stopped") {
		t.Fatal("production direct trace retained helper semantics", err, string(body))
	}
	if _, err := n.publishJournaled(ctx, message, binding, "actor", publish); !errors.Is(err, duplex.ErrPersistence) || calls != 1 {
		t.Fatal("uncertain direct publication replayed", calls, err)
	}
}

func TestLegacySelectionCannotBeRetiredByNewHost(t *testing.T) {
	p, id, meta, _, st := retirementFixture(t, false)
	meta.ProcessClaimVersion = 0
	if _, err := p.retireLocalSelection(context.Background(), id, meta, st); err == nil || !strings.Contains(err.Error(), "legacy_process_claim") {
		t.Fatal("legacy selection was silently retired", err)
	}
	if err := verifyConnectionClaim(context.Background(), meta.Target, meta.Claim); err != nil {
		t.Fatal("legacy refusal changed ownership", err)
	}
}

func TestDoctorQualificationUsesFinalTypedMailboxObservation(t *testing.T) {
	profile := memory.DuplexWriteCapability(memory.RetailLuaMailboxExecutableSHA256, "12.1.0.69933", "retail")
	r := ProjectResult{Diagnostics: map[string]Diagnostic{}}
	observation := memory.DuplexProfileObservation{ProcessIdentified: true, RootRecipeID: memory.LuaMailboxRootRecipeID, LifecycleID: memory.RetailReloadRecipeID}
	projectWriterQualification(&r, profile, observation)
	if r.WriterQualification == nil || r.WriterQualification.DirectWrite || r.WriterQualification.Level != memory.QualificationL0 {
		t.Fatal("pre-mailbox observation granted writes", r.WriterQualification)
	}
	observation.TypedMailbox, observation.NumericRow = true, true
	final := projectWriterQualification(&r, profile, observation)
	if r.WriterQualification == nil || !r.WriterQualification.DirectWrite || r.WriterQualification.Level != memory.QualificationL3 || !final.DirectWrite || r.Diagnostics["writerProfile"].State != string(memory.QualificationL3) {
		t.Fatal("doctor retained preliminary qualification instead of final row evidence", r.WriterQualification, final)
	}
	observation.NumericRow = false
	projectWriterQualification(&r, profile, observation)
	if r.WriterQualification.DirectWrite || r.WriterQualification.Level != memory.QualificationL1 || r.Diagnostics["writerProfile"].State != string(memory.QualificationL1) {
		t.Fatal("changed row retained stale qualification", r.WriterQualification)
	}
}
