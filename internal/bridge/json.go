package bridge

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
)

// encoding/json normally accepts duplicate object keys (last value wins).
// Protocol documents cannot have two interpretations of identity or results.
func validateJSON(data []byte, depthLimit, entryLimit int) error {
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	entries := 0
	var visit func(int) error
	visit = func(depth int) error {
		if depth > depthLimit {
			return errors.New("bridge.json_depth_limit")
		}
		token, err := decoder.Token()
		if err != nil {
			return err
		}
		delimiter, container := token.(json.Delim)
		if !container {
			return nil
		}
		if delimiter != '{' && delimiter != '[' {
			return errors.New("bridge.invalid_json_container")
		}
		keys := make(map[string]bool)
		for decoder.More() {
			entries++
			if entries > entryLimit {
				return errors.New("bridge.json_entry_limit")
			}
			if delimiter == '{' {
				key, err := decoder.Token()
				if err != nil {
					return err
				}
				name, ok := key.(string)
				if !ok || keys[name] {
					return errors.New("bridge.json_duplicate_or_invalid_key")
				}
				keys[name] = true
			}
			if err := visit(depth + 1); err != nil {
				return err
			}
		}
		closing, err := decoder.Token()
		if err != nil {
			return err
		}
		if delimiter == '{' && closing != json.Delim('}') || delimiter == '[' && closing != json.Delim(']') {
			return errors.New("bridge.invalid_json_container")
		}
		return nil
	}
	if err := visit(0); err != nil {
		return err
	}
	if _, err := decoder.Token(); err != io.EOF {
		return errors.New("bridge.json_trailing_data")
	}
	return nil
}
