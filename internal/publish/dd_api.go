package publish

import (
	"context"
	"net/url"
)

// A retained task owns the native/borrowed author identity. Business operations
// do not reacquire credentials or reconnect after its lifetime ends.
type ddAPI interface {
	call(context.Context, string, url.Values, any, bool) (map[string]any, error)
	channels(context.Context) (any, error)
	identity() map[string]any
	close() error
}
