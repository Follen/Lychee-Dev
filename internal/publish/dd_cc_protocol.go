package publish

import (
	"bytes"
	"compress/zlib"
	"encoding/binary"
	"encoding/json"
	"io"
	"unicode/utf8"
)

const ddCCSID = 44204
const ddCCLimit = 8 << 20

type ddCCResponse struct {
	sid, command uint16
	code         uint32
	sequence     string
	data         map[string]any
}

func ddCCProtocol() error {
	return &Error{Code: "publish.dd_cc_protocol", Message: "invalid CC session packet", Exit: 4}
}
func ddJSON(raw []byte, target any) error {
	d := json.NewDecoder(bytes.NewReader(raw))
	d.UseNumber()
	count := 0
	value, err := decodeValue(d, "$", 0, &count)
	if err != nil {
		return ddCCProtocol()
	}
	if _, err = d.Token(); err != io.EOF {
		return ddCCProtocol()
	}
	normalized, err := json.Marshal(value)
	if err != nil {
		return ddCCProtocol()
	}
	d = json.NewDecoder(bytes.NewReader(normalized))
	d.UseNumber()
	d.DisallowUnknownFields()
	if d.Decode(target) != nil {
		return ddCCProtocol()
	}
	return nil
}
func ddCCEncode(command uint16, sequence string, data any) ([]byte, error) {
	ext, _ := json.Marshal(map[string]any{"seq": sequence})
	prop, _ := json.Marshal(map[string]any{"extInfo": string(ext)})
	body, err := json.Marshal(data)
	if err != nil || len(prop) > 65535 || len(body) > 65535 {
		return nil, ddCCProtocol()
	}
	out := make([]byte, 18)
	binary.LittleEndian.PutUint16(out, ddCCSID)
	binary.LittleEndian.PutUint16(out[2:], command)
	binary.LittleEndian.PutUint16(out[16:], uint16(len(prop)))
	out = append(out, prop...)
	out = binary.LittleEndian.AppendUint16(out, uint16(len(body)))
	return append(out, body...), nil
}
func ddCCDecode(raw []byte) (ddCCResponse, error) {
	var r ddCCResponse
	if len(raw) < 16 || len(raw) > ddCCLimit {
		return r, ddCCProtocol()
	}
	r.sid = binary.LittleEndian.Uint16(raw)
	r.command = binary.LittleEndian.Uint16(raw[2:])
	body := raw[16:]
	if binary.LittleEndian.Uint32(raw[12:])&0x20000 != 0 {
		if len(raw) < 20 {
			return r, ddCCProtocol()
		}
		z, err := zlib.NewReader(bytes.NewReader(raw[20:]))
		if err != nil {
			return r, ddCCProtocol()
		}
		body, err = io.ReadAll(io.LimitReader(z, ddCCLimit+1))
		z.Close()
		if err != nil || len(body) > ddCCLimit {
			return r, ddCCProtocol()
		}
	}
	readString := func() (string, error) {
		if len(body) < 2 {
			return "", ddCCProtocol()
		}
		n := int(binary.LittleEndian.Uint16(body))
		body = body[2:]
		if n > len(body) || !utf8.Valid(body[:n]) {
			return "", ddCCProtocol()
		}
		s := string(body[:n])
		body = body[n:]
		return s, nil
	}
	prop, err := readString()
	if err != nil {
		return r, err
	}
	if len(body) < 4 {
		return r, ddCCProtocol()
	}
	r.code = binary.LittleEndian.Uint32(body)
	body = body[4:]
	if _, err = readString(); err != nil {
		return r, err
	}
	data, err := readString()
	if err != nil || len(body) != 0 {
		return r, ddCCProtocol()
	}
	var properties map[string]any
	if ddJSON([]byte(prop), &properties) != nil || ddJSON([]byte(data), &r.data) != nil {
		return r, ddCCProtocol()
	}
	if ext, ok := properties["extInfo"]; ok && ext != nil && ext != "" {
		text, ok := ext.(string)
		if !ok {
			return r, ddCCProtocol()
		}
		var values map[string]any
		if ddJSON([]byte(text), &values) != nil {
			return r, ddCCProtocol()
		}
		r.sequence, _ = values["seq"].(string)
		if r.sequence == "" {
			return r, ddCCProtocol()
		}
	}
	return r, nil
}
