//go:build !windows

package publish

func bbProtect([]byte, bool) ([]byte, error) {
	return nil, capability("publish.blackbox_storage", "Heybox credential storage requires Windows CurrentUser DPAPI")
}
