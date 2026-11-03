//go:build !windows

package publish

func ddBackupProtect(raw []byte, decrypt bool) ([]byte, error) {
	return nil, capability("publish.dd_backup", "DD backup grant storage requires Windows")
}
