package delivery

import (
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func slotRetirementCalls(parent string, e bridge.SlotEnvelope) map[string]func(context.Context) error {
	return map[string]func(context.Context) error{
		"runtime": func(ctx context.Context) error {
			return RetireSlotRuntime(ctx, parent, "2.5.1", "123/456", e, strings.Repeat("1", 32))
		},
		"process": func(ctx context.Context) error {
			return RetireSlotProcess(ctx, parent, "2.5.1", "123/456", e)
		},
	}
}

func damagedRetirementPoolFixture(t *testing.T) (string, bridge.SlotEnvelope, string) {
	t.Helper()
	parent := t.TempDir()
	e := bridge.SlotEnvelope{Index: 1, Runtime: strings.Repeat("8", 32), Nonce: strings.Repeat("3", 32)}
	payload := []byte("journaled payload\n")
	hash := slotDigest(payload)
	pool := SlotPool{Schema: slotPoolSchema, Version: "2.5.1", State: "ready", Files: make([]SlotFile, bridge.SlotCount)}
	pool.Files[0] = SlotFile{PayloadHash: hash, Nonce: e.Nonce, Runtime: e.Runtime, Consumer: "123/456"}
	if err := saveSlotPool(context.Background(), parent, pool); err != nil {
		t.Fatal(err)
	}
	for _, index := range []int{1, 2} {
		if err := os.Mkdir(SlotDirectory(parent, index), 0700); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(SlotDirectory(parent, 1), "Payload.lua"), payload, 0600); err != nil {
		t.Fatal(err)
	}
	return parent, e, hash
}

func TestSlotRetirementWithLostManifestDoesNotReconstructDiskState(t *testing.T) {
	for _, damage := range []string{"missing", "syntax", "type", "shape", "oversized"} {
		for _, kind := range []string{"runtime", "process"} {
			t.Run(damage+"/"+kind, func(t *testing.T) {
				parent := t.TempDir()
				e := bridge.SlotEnvelope{Index: 1, Runtime: strings.Repeat("8", 32), Nonce: strings.Repeat("3", 32)}
				if err := os.Mkdir(SlotDirectory(parent, 1), 0700); err != nil {
					t.Fatal(err)
				}
				marker := filepath.Join(parent, slotMarker)
				var damaged []byte
				switch damage {
				case "missing":
				case "syntax":
					damaged = []byte("{broken")
				case "type":
					damaged = []byte(`{"schema":4}`)
				case "shape":
					damaged = []byte(`{"schema":"lycheedev.slots.v3","version":"2.5.1","state":"ready","files":[]}`)
				case "oversized":
					damaged = bytes.Repeat([]byte{' '}, slotPoolManifestLimit+1)
				}
				if damaged != nil {
					if err := os.WriteFile(marker, damaged, 0600); err != nil {
						t.Fatal(err)
					}
				}
				payloadPath := filepath.Join(SlotDirectory(parent, 1), "Payload.lua")
				// With no authoritative manifest, unknown payload bytes stay intact.
				payload := []byte("unknown payload preserved\n")
				if err := os.WriteFile(payloadPath, payload, 0600); err != nil {
					t.Fatal(err)
				}
				for attempt := 0; attempt < 2; attempt++ {
					if err := slotRetirementCalls(parent, e)[kind](context.Background()); err != nil {
						t.Fatal("verified retirement blocked by lost manifest", err)
					}
				}
				after, err := os.ReadFile(marker)
				if damage == "missing" {
					if !errors.Is(err, os.ErrNotExist) {
						t.Fatal("retirement reconstructed manifest", err)
					}
				} else if err != nil || !bytes.Equal(after, damaged) {
					t.Fatal("retirement changed invalid manifest", err)
				}
				if after, err = os.ReadFile(payloadPath); err != nil || !bytes.Equal(after, payload) {
					t.Fatal("retirement changed unknown payload", err)
				}
				if err = VerifySlotPublication(context.Background(), parent, "2.5.1", "123/456", e); err == nil {
					t.Fatal("lost manifest authorized input")
				}
			})
		}
	}
}

func TestSlotRetirementDoesNotRequireUnrelatedPoolFiles(t *testing.T) {
	for _, kind := range []string{"runtime", "process"} {
		t.Run(kind, func(t *testing.T) {
			parent, e, wantHash := damagedRetirementPoolFixture(t)
			other := filepath.Join(SlotDirectory(parent, 2), "Loader.lua")
			unknown := []byte("unknown static damage\n")
			if err := os.WriteFile(other, unknown, 0600); err != nil {
				t.Fatal(err)
			}
			if err := slotRetirementCalls(parent, e)[kind](context.Background()); err != nil {
				t.Fatal("unrelated static damage blocked proven retirement", err)
			}
			pool, err := readSlotPool(parent)
			if err != nil {
				t.Fatal(err)
			}
			s := pool.Files[0]
			if s.Consumed || s.PendingHash != "" || s.PayloadHash != wantHash || kind == "runtime" && s.RetiredRuntime != strings.Repeat("1", 32) || kind == "process" && !s.RetiredProcess {
				t.Fatalf("incorrect retirement: %+v", s)
			}
			if after, err := os.ReadFile(other); err != nil || !bytes.Equal(after, unknown) {
				t.Fatal("retirement changed unrelated damage", err)
			}
			if _, err := InspectSlots(context.Background(), parent, "2.5.1"); err == nil {
				t.Fatal("retirement declared damaged pool healthy")
			}
		})
	}
}

func TestSlotRetirementDamagedPoolKeepsExactReservationChecks(t *testing.T) {
	for _, kind := range []string{"runtime", "process"} {
		for _, conflict := range []string{"consumer", "runtime", "payload", "new-nonce"} {
			t.Run(kind+"/"+conflict, func(t *testing.T) {
				parent, e, _ := damagedRetirementPoolFixture(t)
				pool, err := readSlotPool(parent)
				if err != nil {
					t.Fatal(err)
				}
				switch conflict {
				case "consumer":
					pool.Files[0].Consumer = "foreign"
				case "runtime":
					pool.Files[0].Runtime = strings.Repeat("9", 32)
				case "payload":
					if err := os.WriteFile(filepath.Join(SlotDirectory(parent, 1), "Payload.lua"), []byte("unknown payload"), 0600); err != nil {
						t.Fatal(err)
					}
				case "new-nonce":
					pool.Files[0].Nonce = strings.Repeat("7", 32)
				}
				if err := saveSlotPool(context.Background(), parent, pool); err != nil {
					t.Fatal(err)
				}
				before, err := os.ReadFile(filepath.Join(parent, slotMarker))
				if err != nil {
					t.Fatal(err)
				}
				err = slotRetirementCalls(parent, e)[kind](context.Background())
				if (err == nil) != (conflict == "new-nonce") {
					t.Fatal("incorrect reservation check", err)
				}
				if after, err := os.ReadFile(filepath.Join(parent, slotMarker)); err != nil || !bytes.Equal(before, after) {
					t.Fatal("changed conflicting reservation", err)
				}
			})
		}
	}
}

func TestSlotRetirementPreservesCancellationAndIOErrors(t *testing.T) {
	parent := t.TempDir()
	e := bridge.SlotEnvelope{Index: 1, Runtime: strings.Repeat("8", 32), Nonce: strings.Repeat("3", 32)}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	for kind, retire := range slotRetirementCalls(parent, e) {
		if err := retire(ctx); !errors.Is(err, context.Canceled) {
			t.Fatal(kind, "ignored cancellation", err)
		}
	}
	invalidParent := filepath.Join(t.TempDir(), "regular-file")
	if err := os.WriteFile(invalidParent, []byte("not a directory"), 0600); err != nil {
		t.Fatal(err)
	}
	for kind, retire := range slotRetirementCalls(invalidParent, e) {
		if err := retire(context.Background()); err == nil {
			t.Fatal(kind, "ignored filesystem access failure")
		}
	}
}
