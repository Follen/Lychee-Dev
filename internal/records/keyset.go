package records

import (
	"bufio"
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"strings"

	"github.com/follenfang/lycheedev/internal/records/container"
)

var ErrKeyDocument = errors.New("records.invalid_key_document")

// KeySet is an immutable, request-owned set. It does not discover files, use
// environment settings, fetch URLs or persist key material. Lookup returns a
// copy, so independent decoders cannot change each other's keys.
type KeySet struct {
	keys   map[uint64][]byte
	digest string
}

func (k *KeySet) String() string   { return "records.KeySet(redacted)" }
func (k *KeySet) GoString() string { return k.String() }
func (k *KeySet) SHA256() string   { return k.digest }
func (k *KeySet) Count() int       { return len(k.keys) }

func (k *KeySet) Lookup(ctx context.Context, id uint64) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if k == nil {
		return nil, container.ErrKeyUnavailable
	}
	key, ok := k.keys[id]
	if !ok {
		return nil, container.ErrKeyUnavailable
	}
	return bytes.Clone(key), nil
}

// ReadKeySet accepts explicit "text" (hex ID, whitespace, hex key per line) or
// "json" (string-to-string object). It requires a pinned raw SHA-256, limits
// input to 4 MiB/65536 keys, rejects duplicates and never includes raw material
// or upstream I/O error text in errors. Caller owns cancellation of blocking I/O.
func ReadKeySet(ctx context.Context, source io.Reader, format, expectedSHA256 string) (*KeySet, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	digest, err := hex.DecodeString(expectedSHA256)
	if source == nil || err != nil || len(digest) != 32 || hex.EncodeToString(digest) != expectedSHA256 || (format != "text" && format != "json") {
		return nil, ErrKeyDocument
	}
	var raw bytes.Buffer
	var scratch [32 * 1024]byte
	for stalls := 0; ; {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		n, readErr := source.Read(scratch[:])
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if n < 0 || n > len(scratch) {
			return nil, ErrKeyDocument
		}
		if raw.Len()+n > 4<<20 {
			return nil, ErrMetadataLimit
		}
		raw.Write(scratch[:n])
		if readErr != nil {
			if readErr == io.EOF {
				break
			}
			return nil, ErrKeyDocument
		}
		if n == 0 {
			stalls++
			if stalls >= 100 {
				return nil, ErrKeyDocument
			}
		} else {
			stalls = 0
		}
	}
	sum := sha256.Sum256(raw.Bytes())
	if !bytes.Equal(sum[:], digest) {
		return nil, container.ErrIntegrity
	}
	set := &KeySet{keys: make(map[uint64][]byte), digest: expectedSHA256}
	add := func(name, value string) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		if len(name) != 16 || (len(value) != 32 && len(value) != 64) {
			return ErrKeyDocument
		}
		nameBytes, err := hex.DecodeString(name)
		if err != nil {
			return ErrKeyDocument
		}
		id := binary.BigEndian.Uint64(nameBytes)
		key, err := hex.DecodeString(value)
		if err != nil {
			return ErrKeyDocument
		}
		if _, exists := set.keys[id]; exists {
			return ErrKeyDocument
		}
		if len(set.keys) >= 65536 {
			return ErrMetadataLimit
		}
		set.keys[id] = key
		return nil
	}
	if format == "text" {
		scanner := bufio.NewScanner(bytes.NewReader(raw.Bytes()))
		for scanner.Scan() {
			line := strings.TrimSpace(scanner.Text())
			if line == "" || strings.HasPrefix(line, "#") {
				continue
			}
			fields := strings.Fields(line)
			if len(fields) != 2 {
				return nil, ErrKeyDocument
			}
			if err := add(fields[0], fields[1]); err != nil {
				return nil, err
			}
		}
		if scanner.Err() != nil {
			return nil, ErrKeyDocument
		}
	} else {
		decoder := json.NewDecoder(bytes.NewReader(raw.Bytes()))
		token, err := decoder.Token()
		if err != nil || token != json.Delim('{') {
			return nil, ErrKeyDocument
		}
		for decoder.More() {
			token, err := decoder.Token()
			if err != nil {
				return nil, ErrKeyDocument
			}
			name, ok := token.(string)
			if !ok {
				return nil, ErrKeyDocument
			}
			var value string
			if decoder.Decode(&value) != nil {
				return nil, ErrKeyDocument
			}
			if err := add(name, value); err != nil {
				return nil, err
			}
		}
		if token, err := decoder.Token(); err != nil || token != json.Delim('}') {
			return nil, ErrKeyDocument
		}
		if _, err := decoder.Token(); err != io.EOF {
			return nil, ErrKeyDocument
		}
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if len(set.keys) == 0 {
		return nil, ErrKeyDocument
	}
	return set, nil
}
