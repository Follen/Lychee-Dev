package bridge

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"strings"
	"testing"
)

func reportFixture(t *testing.T, body, code []byte) ([]byte, SignalExpectation) {
	t.Helper()
	s := Signal{Schema: "lycheedev.signal.v1", Release: "2.0.0", Kind: "reported", SessionNonce: "session", RequestID: "OP-test", Character: "character", Realm: "realm", Product: "retail", Build: "12.1.0.69875", Sequence: 3, ReportBytes: uint32(len(body)), ReportAdler32: byteChecksum(body), CodeBytes: uint32(len(code))}
	if len(code) > 0 {
		s.CodeAdler32 = byteChecksum(code)
	}
	raw, err := json.Marshal(s)
	if err != nil {
		t.Fatal(err)
	}
	return raw, SignalExpectation{Kind: s.Kind, Release: s.Release, SessionNonce: s.SessionNonce, RequestID: s.RequestID, Character: s.Character, Realm: s.Realm, Product: s.Product, Build: s.Build, AfterSequence: 2}
}

func TestReportPreservesExactBytesAndOwnsCopy(t *testing.T) {
	body, code := []byte("{\r\n\"text\":\"世界\",\"value\":1.0}\n"), []byte("return 1")
	raw, expected := reportFixture(t, body, code)
	report, err := VerifyReport(raw, body, code, expected)
	if err != nil {
		t.Fatal(err)
	}
	digest := sha256.Sum256(body)
	if !bytes.Equal(body, report.Body) || report.SHA256 != hex.EncodeToString(digest[:]) {
		t.Fatal("raw evidence changed")
	}
	body[0] = '!'
	raw[0] = '!'
	if report.ReceiptBytes[0] != '{' {
		t.Fatal("receipt aliases caller memory")
	}
	if report.Body[0] != '{' {
		t.Fatal("report aliases caller memory")
	}
}

func TestReportRejectsMismatchedTransport(t *testing.T) {
	body, code := []byte(`{"value":1}`), []byte("return 1")
	for _, name := range []string{"code", "body", "session", "request", "build", "stale", "missing_expectation", "oversize_code"} {
		t.Run(name, func(t *testing.T) {
			raw, expected := reportFixture(t, body, code)
			actualBody, actualCode := body, code
			switch name {
			case "code":
				actualCode = []byte("return 2")
			case "body":
				actualBody = []byte(`{"value":2}`)
			case "session":
				expected.SessionNonce = "foreign"
			case "request":
				expected.RequestID = "other"
			case "build":
				expected.Build = "12.1.0.99999"
			case "stale":
				expected.AfterSequence = 3
			case "missing_expectation":
				expected.Character = ""
			case "oversize_code":
				actualCode = bytes.Repeat([]byte("x"), 256<<10+1)
			}
			if _, err := VerifyReport(raw, actualBody, actualCode, expected); err == nil {
				t.Fatal("mismatch accepted")
			}
		})
	}
}

func TestReportRejectsAmbiguousOrUnboundedJSON(t *testing.T) {
	for _, body := range []string{`{"a":1,"a":2}`, `{"a":1,"\u0061":2}`, `{"nested":{"a":1,"a":2}}`, `{} {}`, `{"a":`, strings.Repeat("[", 34) + "0" + strings.Repeat("]", 34), "[" + strings.Repeat("0,", 32768) + "0]", "\"\xff\""} {
		raw, expected := reportFixture(t, []byte(body), nil)
		if _, err := VerifyReport(raw, []byte(body), nil, expected); err == nil {
			t.Fatalf("invalid payload accepted: %.80s", body)
		}
	}
}

func TestBuiltinReportAndMaximumPayload(t *testing.T) {
	body := []byte(`{"text":"` + strings.Repeat("x", (512<<10)-11) + `"}`)
	if len(body) != 512<<10 {
		t.Fatal(len(body))
	}
	raw, expected := reportFixture(t, body, nil)
	if _, err := VerifyReport(raw, body, nil, expected); err != nil {
		t.Fatal(err)
	}
	body = append(body, ' ')
	raw, expected = reportFixture(t, body, nil)
	if _, err := VerifyReport(raw, body, nil, expected); err == nil {
		t.Fatal("oversized report accepted")
	}
}

func TestSignalRejectsDuplicateIdentity(t *testing.T) {
	raw, _ := reportFixture(t, []byte(`{}`), nil)
	raw = append([]byte(`{"sessionNonce":"foreign",`), raw[1:]...)
	if _, err := ParseSignal(raw); err == nil {
		t.Fatal("duplicate identity accepted")
	}
}

func FuzzReportTransport(f *testing.F) {
	f.Add([]byte(`{"value":1}`), []byte("return 1"))
	f.Add([]byte(`{"value":1,"value":2}`), []byte{})
	f.Add([]byte("\"\xff\""), []byte("return nil"))
	f.Fuzz(func(t *testing.T, body, code []byte) {
		// Keep each fuzz iteration bounded independently of the engine's inputs.
		if len(body) > 512<<10 || len(code) > 256<<10 {
			return
		}
		raw, expected := reportFixture(t, body, code)
		report, err := VerifyReport(raw, body, code, expected)
		if err != nil {
			return
		}
		if !bytes.Equal(report.Body, body) {
			t.Fatal("verified bytes changed")
		}
		// Changing only the expected operation must always invalidate a report.
		expected.RequestID += "-other"
		if _, err := VerifyReport(raw, body, code, expected); err == nil {
			t.Fatal("foreign operation accepted")
		}
	})
}
