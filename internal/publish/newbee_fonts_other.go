//go:build !windows || !amd64

package publish

import "context"

func newBeeUploadFonts(context.Context, string) (string, error) {
	return "", capability("publish.newbee_native", "native font uploader requires Windows amd64")
}

func newBeeFontReady() error {
	return capability("publish.newbee_native", "native font uploader requires Windows amd64")
}
