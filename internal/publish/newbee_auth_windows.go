package publish

import "golang.org/x/sys/windows"

func newBeeRoaming() (string, error) {
	return windows.KnownFolderPath(windows.FOLDERID_RoamingAppData, 0)
}
