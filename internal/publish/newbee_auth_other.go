//go:build !windows

package publish

func newBeeRoaming() (string, error) {
	return "", capability("publish.newbee_platform", "NewBeeBox Creator desktop sessions require Windows")
}
