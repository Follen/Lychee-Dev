package container_test

import (
	"bytes"
	"context"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/container"
)

// Independent ciphertext generated using PyCryptodome 3.23.0 Salsa20, not the
// production implementation. Plaintext is 'N' followed by bytes 0..129; block
// index 1 is XORed into nonce[0]. This spans three cipher blocks.
func cipherFixture(t *testing.T, keySize, ivSize int) ([]byte, []byte) {
	t.Helper()
	encoded := "c6a962b5d9c759d5848fa20eb97090e9fffdbc6d153fa9a617fb38d12edf9cc810c6308db54af1f8882085126a009de140c78929ba297f290b146901afc1551de77ad39a74d79a9fab02e23f2511bf23652ca2cde62c8857313bb5f0a78574df17e9f797241fd0412403ff76ba48575064a49cde0c357a02a1b950e13e28650990d096"
	if keySize == 32 {
		encoded = "e1594157c5490dbc04c96a0665a008510a2a00be425f8891436e9dd977aad5f0e2c7e21f99b3c2746cac6ac74c1e38e07348695a8e17c77b5113704c346676d36c059c78df75c38589dfba44912e8ae31f11f4aae59b686280a82679529082058b26154c3ea3d293d69205b9f815da6073452ae1ebead78370115827d9fd0ee5000c9a"
	}
	ciphertext, err := hex.DecodeString(encoded)
	if err != nil {
		t.Fatal(err)
	}
	header := make([]byte, 11+ivSize)
	header[0], header[9] = 8, byte(ivSize)
	binary.LittleEndian.PutUint64(header[1:9], 0x1234567890abcdef)
	copy(header[10:], []byte{0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88}[:ivSize])
	header[10+ivSize] = 'S'
	key := make([]byte, keySize)
	for i := range key {
		key[i] = byte(i)
	}
	return append([]byte{'E'}, append(header, ciphertext...)...), key
}

func TestEncryptedIndependentFixtures(t *testing.T) {
	for _, sizes := range [][2]int{{16, 4}, {32, 8}} {
		raw, key := cipherFixture(t, sizes[0], sizes[1])
		input := framedBLTE(plainBlock("prefix"), fixtureBlock{data: raw, decoded: 130})
		lookup := func(ctx context.Context, name uint64) ([]byte, error) {
			if name != 0x1234567890abcdef {
				t.Fatalf("wrong key identity %x", name)
			}
			return key, nil
		}
		var output bytes.Buffer
		n, err := container.DecodeWithKeys(context.Background(), &output, bytes.NewReader(input), generousLimits, lookup)
		want := []byte("prefix")
		for i := 0; i < 130; i++ {
			want = append(want, byte(i))
		}
		if err != nil || n != int64(len(want)) || !bytes.Equal(output.Bytes(), want) {
			t.Fatalf("key=%d iv=%d: n=%d error=%v", sizes[0], sizes[1], n, err)
		}
	}
}

func TestEncryptedErrorsAndCancellation(t *testing.T) {
	raw, key := cipherFixture(t, 16, 4)
	for _, mode := range []string{"missing", "provider-error", "key-length", "bad-header", "algorithm", "cancel", "depth"} {
		t.Run(mode, func(t *testing.T) {
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			chunk := bytes.Clone(raw)
			limits := generousLimits
			wantErr := container.ErrKeyUnavailable
			lookup := container.KeyLookup(func(context.Context, uint64) ([]byte, error) { return key, nil })
			switch mode {
			case "missing":
				lookup = nil
			case "provider-error":
				lookup = func(context.Context, uint64) ([]byte, error) { return nil, errors.New("secret-material") }
			case "key-length":
				lookup = func(context.Context, uint64) ([]byte, error) { return []byte{1}, nil }
			case "bad-header":
				chunk[1] = 7
				wantErr = container.ErrMalformed
			case "algorithm":
				chunk[15] = 'A'
				wantErr = container.ErrUnsupported
			case "cancel":
				lookup = func(context.Context, uint64) ([]byte, error) { cancel(); return key, nil }
				wantErr = context.Canceled
			case "depth":
				limits.Depth = 1
				wantErr = container.ErrLimit
			}
			input := framedBLTE(plainBlock("prefix"), fixtureBlock{data: chunk, decoded: 130})
			var output bytes.Buffer
			_, err := container.DecodeWithKeys(ctx, &output, bytes.NewReader(input), limits, lookup)
			if !errors.Is(err, wantErr) || strings.Contains(err.Error(), "secret-material") || output.String() != "prefix" {
				t.Fatalf("%q %v", output.String(), err)
			}
		})
	}
}
