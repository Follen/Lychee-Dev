package command

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestLiveResumeRejectsInvalidIntentWithoutMutation(t *testing.T) {
	ctx := context.Background()
	root := filepath.Join(t.TempDir(), "home")
	store, err := vault.Initialize(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer metadata.Close()
	book := journal.OpenBook(metadata)
	record, err := book.BeginWork(ctx, journal.WorkIntent{Kind: "probe", Resource: "window/fixture", Request: json.RawMessage(`{}`)})
	if err != nil {
		t.Fatal(err)
	}
	result, code := invoke(t, "live", "resume", record.OperationID, "--home", root, "--format=json")
	if code == 0 || result.OK || result.Result == nil || result.OperationID != record.OperationID || result.Error == nil || result.Error.Stage != "prepared" || result.Error.Retryable || result.Error.ResumeOperationID != record.OperationID {
		t.Fatalf("unsupported stage: %+v code=%d", result, code)
	}
	current, err := book.InspectWork(ctx, record.OperationID)
	if err != nil || current.Generation != record.Generation || current.Stage != record.Stage {
		t.Fatal("unsupported resume mutated work", err)
	}
	for _, args := range [][]string{{"live", "resume"}, {"live", "resume", record.OperationID, "extra"}} {
		response, code := invoke(t, append(args, "--home", root, "--format=json")...)
		if code != 2 || response.OK {
			t.Fatal("invalid arguments accepted")
		}
	}
	absent := filepath.Join(t.TempDir(), "absent")
	result, code = invoke(t, "live", "resume", record.OperationID, "--home", absent, "--format=json")
	if code == 0 || result.OK || result.Result != nil {
		t.Fatal("missing home resumed")
	}
	if _, err := os.Stat(absent); !os.IsNotExist(err) {
		t.Fatal("resume created a workspace")
	}
	result, code = invoke(t, "live", "resume", "OP-missing", "--home", root, "--format=json")
	if code == 0 || result.OK || result.Result != nil || result.Error.Code != "vault.record_missing" {
		t.Fatal("missing operation resumed")
	}
	found := false
	for _, definition := range definitions {
		if definition.Path == "live resume" {
			found = definition.Mutates
		}
	}
	if !found {
		t.Fatal("resume mutation missing from discovery")
	}
}

func TestLiveResumeDoesNotPermitRetargeting(t *testing.T) {
	if _, err := parseOptions([]string{"live", "resume", "OP-existing"}); err != nil {
		t.Fatal(err)
	}
	for _, flag := range []string{"--snapshot=PIN-other", "--pid=2", "--nonce=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "--character=Other", "--realm=Other", "--installation=C:/Other", "--region=window", "--region=0,0,600,600", "--session=SESSION-other", "--account=OTHER"} {
		if _, err := parseOptions([]string{"live", "resume", "OP-existing", flag}); err == nil {
			t.Fatal("accepted recovery retarget or invalid crop", flag)
		}
	}
	absent := filepath.Join(t.TempDir(), "absent")
	result, code := invoke(t, "live", "resume", "OP-missing", "--home", absent, "--format=json")
	if code == 0 || result.OK || result.Result != nil {
		t.Fatal(result, code)
	}
	if _, err := os.Stat(absent); !os.IsNotExist(err) {
		t.Fatal("resume initialized workspace", err)
	}
}
