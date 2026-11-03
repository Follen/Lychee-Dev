package publish

import "context"

const ddNEPSHA256 = "e86b40e5b2377ff3d0c51ee251f21bd4a1e5a9d1eb86549bb8878c7e92232319"

type ddSigner interface {
	Sign(context.Context, string, []byte, [][2]string) (string, error)
	Close() error
}
