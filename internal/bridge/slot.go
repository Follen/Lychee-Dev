package bridge

import (
	"fmt"
	"hash/adler32"
	"strings"
)

const SlotCount = 200
const SlotSchema = "lycheedev.slot.v2"
const LegacySlotSchema = "lycheedev.slot.v1"

type SlotEnvelope struct {
	Schema         string `json:"schema"`
	Index          int    `json:"index"`
	StartSlot      int    `json:"startSlot,omitempty"`
	Runtime        string `json:"runtime"`
	Owner          string `json:"owner"`
	Fence          uint64 `json:"fence"`
	Nonce          string `json:"nonce"`
	Ticket         string `json:"ticket"`
	Action         string `json:"action"`
	GUID           string `json:"guid"`
	Build          string `json:"build"`
	Code           string `json:"code,omitempty"`
	CodeBytes      int    `json:"codeBytes,omitempty"`
	CodeChecksum   uint32 `json:"codeChecksum,omitempty"`
	Budget         int    `json:"budget,omitempty"`
	Challenge      string `json:"challenge,omitempty"`
	PreparedNonce  string `json:"preparedNonce,omitempty"`
	ReportBytes    uint32 `json:"reportBytes,omitempty"`
	ReportChecksum uint32 `json:"reportChecksum,omitempty"`
}

func luaString(s string) string {
	var b strings.Builder
	b.WriteByte('"')
	for _, c := range []byte(s) {
		if c == '"' || c == '\\' {
			b.WriteByte('\\')
			b.WriteByte(c)
		} else if c < 32 || c > 126 {
			fmt.Fprintf(&b, "\\%03d", c)
		} else {
			b.WriteByte(c)
		}
	}
	b.WriteByte('"')
	return b.String()
}

// SlotPayload is generated data, never top-level user code. Decimal escapes
// round-trip every byte under Lua 5.1, including Unicode and delimiter attacks.
func SlotPayload(e SlotEnvelope) ([]byte, error) {
	if (e.Schema != SlotSchema && e.Schema != LegacySlotSchema) || e.Index < 1 || e.Index > SlotCount || (e.Schema == LegacySlotSchema && (e.Index > 64 || e.StartSlot != 0)) || e.StartSlot < 0 || e.StartSlot > e.Index || len(e.Code) > 262144 || e.Fence < 1 || e.Fence > 9007199254740991 {
		return nil, fmt.Errorf("bridge.invalid_slot_envelope")
	}
	for _, token := range []string{e.Runtime, e.Owner, e.Nonce, e.Ticket} {
		if len(token) != 32 || strings.Trim(token, "0123456789abcdef") != "" {
			return nil, fmt.Errorf("bridge.invalid_slot_token")
		}
	}
	if e.Code != "" {
		e.CodeBytes = len(e.Code)
		e.CodeChecksum = adler32.Checksum([]byte(e.Code))
	}
	var b strings.Builder
	b.WriteString("LycheeDevSlotEnvelope = {\n")
	if e.StartSlot != 0 {
		fmt.Fprintf(&b, "  startSlot = %d,\n", e.StartSlot)
	}
	for _, p := range [][2]string{{"schema", e.Schema}, {"runtime", e.Runtime}, {"owner", e.Owner}, {"nonce", e.Nonce}, {"ticket", e.Ticket}, {"action", e.Action}, {"guid", e.GUID}, {"build", e.Build}, {"code", e.Code}, {"challenge", e.Challenge}, {"preparedNonce", e.PreparedNonce}} {
		fmt.Fprintf(&b, "  %s = %s,\n", p[0], luaString(p[1]))
	}
	fmt.Fprintf(&b, "  index = %d, fence = %d, codeBytes = %d, codeChecksum = %d, budget = %d, reportBytes = %d, reportChecksum = %d,\n}\n", e.Index, e.Fence, e.CodeBytes, e.CodeChecksum, e.Budget, e.ReportBytes, e.ReportChecksum)
	return []byte(b.String()), nil
}
