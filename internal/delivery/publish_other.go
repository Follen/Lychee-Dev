//go:build !windows && !linux && !darwin

package delivery

import "errors"

func publishDirectory(source, target string) error {
	return errors.New("delivery.unsupported_platform")
}
