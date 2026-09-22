package bridge

import (
	"bytes"
	"crypto/sha256"
	"errors"
	"fmt"
	"hash/adler32"
	"io"
	"sort"
	"strings"
	"unicode/utf8"
)

const queueHeader = "local _, ns = ...\nns.ProbeDefinitions = "
const MaxProbeQueueFileBytes = 5 << 20

// ProbeDefinition carries intent, not permission to execute. The game must
// independently match its active session before loading the code.
type ProbeDefinition struct {
	RequestID, Release, SessionNonce, ReloadNonce string
	Character, Realm, GUID, Product, Build, Code  string
}

func queueLabel(s string) bool {
	if s == "" || len(s) > 128 || !utf8.ValidString(s) {
		return false
	}
	for _, c := range s {
		if c < 32 || c == 127 {
			return false
		}
	}
	return true
}
func queueHex(s string, n int) bool {
	if len(s) != n {
		return false
	}
	for _, c := range s {
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f') {
			return false
		}
	}
	return true
}
func queueKey(s string) bool {
	if !queueLabel(s) {
		return false
	}
	for _, c := range s {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '_' || c == '-') {
			return false
		}
	}
	return true
}
func luaBytes(s string) string {
	var out strings.Builder
	out.WriteByte('"')
	for i := 0; i < len(s); i++ {
		c := s[i]
		if c >= 32 && c <= 126 && c != '"' && c != '\\' {
			out.WriteByte(c)
		} else {
			fmt.Fprintf(&out, "\\%03d", c)
		}
	}
	out.WriteByte('"')
	return out.String()
}

// EncodeProbeQueue generates only literal data, in deterministic request order.
// Code digests are calculated here, never accepted as claims from the caller.
func EncodeProbeQueue(definitions []ProbeDefinition) ([]byte, error) {
	if len(definitions) > 16 {
		return nil, errors.New("bridge.queue_limit")
	}
	definitions = append([]ProbeDefinition(nil), definitions...)
	sort.Slice(definitions, func(i, j int) bool { return definitions[i].RequestID < definitions[j].RequestID })
	total := 0
	var out strings.Builder
	out.WriteString(queueHeader + "{schema=\"lycheedev.queue.v1\",entries={\n")
	for i, d := range definitions {
		if !queueKey(d.RequestID) || i > 0 && d.RequestID == definitions[i-1].RequestID {
			return nil, errors.New("bridge.queue_invalid_request")
		}
		for _, value := range []string{d.Release, d.Character, d.Realm, d.GUID, d.Build} {
			if !queueLabel(value) {
				return nil, errors.New("bridge.queue_invalid_identity")
			}
		}
		if !queueHex(d.SessionNonce, 32) || !queueHex(d.ReloadNonce, 32) {
			return nil, errors.New("bridge.queue_invalid_nonce")
		}
		switch d.Product {
		case "retail", "classic", "titan", "forever":
		default:
			return nil, errors.New("bridge.queue_invalid_product")
		}
		if len(d.Code) == 0 || len(d.Code) > 256<<10 || !utf8.ValidString(d.Code) || strings.IndexByte(d.Code, 0) >= 0 || d.Code[0] == 27 {
			return nil, errors.New("bridge.queue_invalid_code")
		}
		total += len(d.Code)
		if total > 1<<20 {
			return nil, errors.New("bridge.queue_limit")
		}
		fmt.Fprintf(&out, "[%s]={", luaBytes(d.RequestID))
		for _, pair := range [][2]string{
			{"release", d.Release}, {"sessionNonce", d.SessionNonce}, {"reloadNonce", d.ReloadNonce},
			{"character", d.Character}, {"realm", d.Realm}, {"guid", d.GUID}, {"product", d.Product}, {"build", d.Build},
			{"code", d.Code}, {"codeSHA256", fmt.Sprintf("%x", sha256.Sum256([]byte(d.Code)))},
			{"codeAdler32", fmt.Sprintf("%08x", adler32.Checksum([]byte(d.Code)))},
		} {
			fmt.Fprintf(&out, "%s=%s,", pair[0], luaBytes(pair[1]))
		}
		fmt.Fprintf(&out, "codeBytes=%d", len(d.Code))
		out.WriteString("},\n")
	}
	out.WriteString("}}\n")
	return []byte(out.String()), nil
}

// DecodeProbeQueue accepts only this owned canonical format. It never evaluates
// Lua. Unknown fields, changed digests and executable suffixes all fail closed.
func DecodeProbeQueue(reader io.Reader) ([]ProbeDefinition, error) {
	if reader == nil {
		return nil, errors.New("bridge.queue_missing_reader")
	}
	data, err := io.ReadAll(io.LimitReader(reader, MaxProbeQueueFileBytes+1))
	if err != nil {
		return nil, err
	}
	if len(data) > MaxProbeQueueFileBytes || !bytes.HasPrefix(data, []byte(queueHeader)) {
		return nil, errors.New("bridge.queue_invalid_file")
	}
	literal := io.MultiReader(strings.NewReader("Queue="), bytes.NewReader(data[len(queueHeader):]))
	globals, err := DecodeSavedState(literal, DecodeLimits{FileBytes: MaxProbeQueueFileBytes, StringBytes: 256 << 10, Entries: 1024, Depth: 4})
	if err != nil {
		return nil, err
	}
	root, ok := globals["Queue"].(LiteralTable)
	if !ok {
		return nil, errors.New("bridge.queue_invalid_root")
	}
	entries, ok := root["entries"].(LiteralTable)
	if !ok || len(entries) > 16 {
		return nil, errors.New("bridge.queue_invalid_entries")
	}
	definitions := make([]ProbeDefinition, 0, len(entries))
	for key, value := range entries {
		id, valid := key.(string)
		entry, table := value.(LiteralTable)
		if !valid || !table {
			return nil, errors.New("bridge.queue_invalid_entry")
		}
		get := func(key string) string { s, _ := entry[key].(string); return s }
		definition := ProbeDefinition{RequestID: id, Release: get("release"), SessionNonce: get("sessionNonce"), ReloadNonce: get("reloadNonce"), Character: get("character"), Realm: get("realm"), GUID: get("guid"), Product: get("product"), Build: get("build"), Code: get("code")}
		if _, exists := entry["acknowledgement"]; exists {
			return nil, errors.New("bridge.queue_acknowledgement_not_supported")
		}
		definitions = append(definitions, definition)
	}
	canonical, err := EncodeProbeQueue(definitions)
	if err != nil {
		return nil, err
	}
	if !bytes.Equal(data, canonical) {
		return nil, errors.New("bridge.queue_noncanonical_or_modified")
	}
	sort.Slice(definitions, func(i, j int) bool { return definitions[i].RequestID < definitions[j].RequestID })
	return definitions, nil
}
