package command

import (
	"bytes"
	"context"
	"encoding/json"
	"github.com/follenfang/lycheedev/internal/vault"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestDataTimeoutStopsWholeCommandAndReleasesResources(t *testing.T) {
	root, pin := dataCommandFixture(t)
	lease, err := vault.AcquireLease(context.Background(), filepath.Join(root, "locks"), "metadata-schema")
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	args := append([]string{"data", "db2", "schema", "Map", "--timeout-seconds", "1"}, dataArgs(pin, root, "--format=json")...)
	ctx, cancel := context.WithTimeout(context.Background(), 4*time.Second)
	defer cancel()
	var out, log bytes.Buffer
	started := time.Now()
	code := Execute(ctx, args, &out, &log)
	var response Envelope
	if err := json.Unmarshal(out.Bytes(), &response); err != nil {
		t.Fatal(err)
	}
	if code != 5 || response.OK || response.Error == nil || response.Error.Code != "command.deadline" || response.Result != nil || time.Since(started) > 3*time.Second {
		t.Fatalf("timeout not enforced: %d %s", code, out.String())
	}
	lease.Close()
	response, code = invoke(t, args...)
	if code != 0 || !response.OK {
		t.Fatalf("resources not released: %d %+v", code, response.Error)
	}
}

func TestDataTimeoutContract(t *testing.T) {
	for _, route := range []string{"data sql", "data db2", "data db2 search", "data spell info"} {
		contract, _, err := findCommandContract(strings.Fields(route))
		if err != nil {
			t.Fatal(err)
		}
		if _, ok := contract.flag("--timeout-seconds"); !ok {
			t.Errorf("%s has no total query timeout", route)
		}
	}
	for _, v := range []string{"1", "300", "3600"} {
		if _, err := parseOptions([]string{"data", "db2", "--timeout-seconds", v}); err != nil {
			t.Errorf("valid timeout %s: %v", v, err)
		}
	}
	for _, v := range []string{"0", "-1", "3601", "bad"} {
		if _, err := parseOptions([]string{"data", "db2", "--timeout-seconds", v}); err == nil {
			t.Errorf("invalid timeout %s accepted", v)
		}
	}
}
