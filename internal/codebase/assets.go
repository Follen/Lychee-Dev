package codebase

import (
	"bytes"
	"encoding/binary"
	"fmt"
	"strings"
)

// maxAssetBytes bounds the asset content that is hashed and archived. Larger
// assets keep a metadata-only row without a content identity.
const maxAssetBytes = 64 << 20

// AssetRow is the indexed metadata for one non-code file in a source tree.
// Width and height are zero when they cannot be derived from the header bytes;
// SHA256 and Local stay empty when the asset exceeds the archive byte budget.
type AssetRow struct {
	Path         string `json:"path"`
	ContentHash  string `json:"contentHash"`
	Bytes        int64  `json:"bytes"`
	Extension    string `json:"extension"`
	MIME         string `json:"mime"`
	Format       string `json:"format"`
	Width        int    `json:"width"`
	Height       int    `json:"height"`
	Local        string `json:"local,omitempty"`
	Normalized   string `json:"-"`
	SHA256Stored string `json:"-"`
}

// isAssetExtension reports whether a tree file counts as an asset rather than
// a loadable document. The extension set matches the legacy catalog.
func isAssetExtension(ext string) bool {
	switch strings.ToLower(ext) {
	case ".blp", ".tga", ".png", ".jpg", ".jpeg", ".dds", ".ttf", ".otf", ".mp3", ".ogg", ".wav", ".m2", ".wmo":
		return true
	}
	return false
}

func assetMIME(ext string) string {
	switch strings.ToLower(ext) {
	case ".png":
		return "image/png"
	case ".jpg", ".jpeg":
		return "image/jpeg"
	case ".tga":
		return "image/x-tga"
	case ".blp":
		return "image/x-blp"
	case ".dds":
		return "image/vnd-ms.dds"
	case ".ttf":
		return "font/ttf"
	case ".otf":
		return "font/otf"
	case ".ogg":
		return "audio/ogg"
	case ".mp3":
		return "audio/mpeg"
	case ".wav":
		return "audio/wav"
	default:
		return "application/octet-stream"
	}
}

// assetImageInfo derives width, height and a format label from asset header
// bytes. PNG and BLP carry dimensions at fixed offsets; other formats fall back
// to the extension label with zero dimensions instead of guessing.
func assetImageInfo(data []byte, ext string) (int, int, string) {
	ext = strings.ToLower(ext)
	if ext == ".png" && len(data) >= 24 && bytes.Equal(data[:8], []byte{137, 80, 78, 71, 13, 10, 26, 10}) {
		return int(binary.BigEndian.Uint32(data[16:20])), int(binary.BigEndian.Uint32(data[20:24])), "png"
	}
	if (ext == ".jpg" || ext == ".jpeg") && len(data) > 4 {
		return 0, 0, "jpeg"
	}
	if ext == ".blp" && len(data) >= 20 {
		return int(binary.LittleEndian.Uint32(data[12:16])), int(binary.LittleEndian.Uint32(data[16:20])), "blp"
	}
	return 0, 0, strings.TrimPrefix(ext, ".")
}

// normalizeAssetPath mirrors the legacy asset path identity: lower case with
// backslash separators, so either slash style matches the same asset.
func normalizeAssetPath(value string) string {
	return strings.ToLower(strings.ReplaceAll(value, "/", "\\"))
}

// assetMetaExcerpt renders the legacy metadata line for an inspected asset.
func assetMetaExcerpt(asset AssetRow) string {
	return fmt.Sprintf("format=%s mime=%s bytes=%d width=%d height=%d local=%s", asset.Format, asset.MIME, asset.Bytes, asset.Width, asset.Height, asset.Local)
}
